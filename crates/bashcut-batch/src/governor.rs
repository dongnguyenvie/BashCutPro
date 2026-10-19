//! Resource governor (#61): long unattended batches must not run the machine out of memory or disk,
//! and should use it well.
//!
//! - **Concurrency** is decided again before every item starts: at most the pipeline's
//!   `concurrency`, one item per `memoryPerItemGb` of free RAM, and half the CPU cores (at least one
//!   item always runs). FilmCraft keeps decoded media in memory for the whole session, so many small
//!   items beat one huge one.
//! - **Memory watch**: a step whose process passes `maxItemMemoryGb` is killed; its item goes back
//!   to `notRun` with the category `resources` ("retry later"), and `resume` / `retry` run it again.
//! - **Disk**: before an item's project is created and before each export, the projects and outputs
//!   folders need `minFreeDiskGb` plus `exportReserveGb`; when low the batch pauses (the job book says
//!   why in `paused`) and goes on by itself when space is freed, or stops on cancel.
//! - **Sleep**: [`KeepAwake`] keeps the machine from idle-sleeping while a batch runs (`caffeinate` on
//!   macOS, `systemd-inhibit` on Linux, `SetThreadExecutionState` through PowerShell on Windows); a step's
//!   timeout counts only time the machine was awake (`process::run`), and a batch the machine stopped
//!   resumes through the job book (tripwire, `resume`).
//! - **Report**: peak process memory, time per step and items per hour go into the run's [`Summary`].
//!
//! [`Probe`] reads the machine; [`SystemProbe`] asks the OS (best effort: an unknown value never
//! limits anything), tests use [`FakeProbe`].
//!
//! [`Summary`]: crate::Summary

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::{Mutex, PoisonError};
use std::time::{Duration, Instant};

const GB: f64 = 1024.0 * 1024.0 * 1024.0;

/// What the governor needs to know about the machine. `None` = unknown (never limits).
pub trait Probe: Send + Sync + std::fmt::Debug {
    /// RAM available to new processes, bytes.
    fn free_memory(&self) -> Option<u64>;
    /// Logical CPU cores.
    fn cores(&self) -> usize;
    /// Free space on the volume holding `path` (or its nearest existing parent), bytes.
    fn free_disk(&self, path: &Path) -> Option<u64>;
    /// Resident memory of a process, bytes.
    fn process_memory(&self, pid: u32) -> Option<u64>;
}

/// Limits, from the pipeline's `resources` (all optional).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Limits {
    /// Free RAM each running item needs (default 3 GB).
    pub memory_per_item_gb: f64,
    /// A step process above this is killed and its item retried later (default 8 GB; 0 = no cap).
    pub max_item_memory_gb: f64,
    /// Free disk to keep on the projects and outputs volumes (default 2 GB).
    pub min_free_disk_gb: f64,
    /// Extra free disk an export needs on top of that (default 1 GB).
    pub export_reserve_gb: f64,
    /// Use at most half the cores (default true).
    pub half_cores: bool,
    /// Agent steps at once, apart from the item concurrency (#60; default 1).
    pub agent_concurrency: usize,
}

impl Default for Limits {
    fn default() -> Self {
        Limits { memory_per_item_gb: 3.0, max_item_memory_gb: 8.0, min_free_disk_gb: 2.0, export_reserve_gb: 1.0, half_cores: true, agent_concurrency: 1 }
    }
}

impl Limits {
    pub fn validate(&self) -> Result<(), String> {
        let ok = |v: f64| v.is_finite() && (0.0..=1024.0).contains(&v);
        if !ok(self.memory_per_item_gb) || !ok(self.max_item_memory_gb) || !ok(self.min_free_disk_gb) || !ok(self.export_reserve_gb) {
            return Err("pipeline: resources: sizes must be 0 to 1024 GB".into());
        }
        if self.agent_concurrency == 0 || self.agent_concurrency > 64 {
            return Err("pipeline: resources: agentConcurrency must be 1 to 64".into());
        }
        Ok(())
    }

    fn bytes(gb: f64) -> u64 {
        (gb.max(0.0) * GB) as u64
    }

    /// The process memory cap in bytes (`None`: no cap).
    pub fn memory_cap(&self) -> Option<u64> {
        (self.max_item_memory_gb > 0.0).then(|| Limits::bytes(self.max_item_memory_gb))
    }
}

/// How many items may run now, given the configured `max`.
pub fn allowed(probe: &dyn Probe, limits: &Limits, max: usize) -> usize {
    let mut n = max.max(1);
    if limits.half_cores {
        n = n.min((probe.cores() / 2).max(1));
    }
    if limits.memory_per_item_gb > 0.0
        && let Some(free) = probe.free_memory()
    {
        // running items already hold their memory, so this counts the free RAM for the new one
        n = n.min(((free as f64 / GB) / limits.memory_per_item_gb).floor().max(1.0) as usize);
    }
    n
}

/// How long a started item counts as not holding its memory yet (its process is still loading).
pub const SETTLE_MS: u64 = 10_000;

/// Whether one more item may start while `running` run (re-checked before every item). `settling` of
/// them started less than [`SETTLE_MS`] ago and may not hold their memory yet, so free RAM must cover
/// them as well as the new item.
pub fn may_start(probe: &dyn Probe, limits: &Limits, max: usize, running: usize, settling: usize) -> bool {
    if running == 0 {
        return true;
    }
    if running >= max {
        return false;
    }
    if limits.half_cores && running >= (probe.cores() / 2).max(1) {
        return false;
    }
    match probe.free_memory() {
        Some(free) if limits.memory_per_item_gb > 0.0 => {
            let items = settling.min(running).saturating_add(1) as f64;
            free >= Limits::bytes(limits.memory_per_item_gb * items)
        }
        _ => true,
    }
}

/// Why the disk is too full for the next step at `paths`, if it is.
pub fn disk_low(probe: &dyn Probe, limits: &Limits, paths: &[PathBuf], export: bool) -> Option<String> {
    let need = limits.min_free_disk_gb + if export { limits.export_reserve_gb } else { 0.0 };
    if need <= 0.0 {
        return None;
    }
    for p in paths {
        if let Some(free) = probe.free_disk(p)
            && free < Limits::bytes(need)
        {
            return Some(format!(
                "low disk space on {}: {:.1} GB free, {need:.1} GB needed; free some space and the batch goes on",
                p.display(),
                free as f64 / GB
            ));
        }
    }
    None
}

/// Per-run measurements for the summary.
#[derive(Debug, Default)]
pub struct Meter {
    inner: Mutex<MeterData>,
}

#[derive(Debug, Default)]
struct MeterData {
    peak: u64,
    /// step label → (count, total ms)
    steps: BTreeMap<String, (u64, u64)>,
    items: u64,
}

impl Meter {
    pub fn memory(&self, bytes: u64) {
        let mut d = self.inner.lock().unwrap_or_else(PoisonError::into_inner);
        d.peak = d.peak.max(bytes);
    }

    pub fn step(&self, label: &str, ms: u64) {
        let mut d = self.inner.lock().unwrap_or_else(PoisonError::into_inner);
        let e = d.steps.entry(label.to_string()).or_insert((0, 0));
        e.0 = e.0.saturating_add(1);
        e.1 = e.1.saturating_add(ms);
    }

    pub fn item_done(&self) {
        let mut d = self.inner.lock().unwrap_or_else(PoisonError::into_inner);
        d.items = d.items.saturating_add(1);
    }

    pub fn report(&self, elapsed_ms: u64, concurrency: usize) -> Report {
        let d = self.inner.lock().unwrap_or_else(PoisonError::into_inner);
        let hours = elapsed_ms as f64 / 3_600_000.0;
        Report {
            peak_memory_mb: d.peak / (1024 * 1024),
            items_finished: d.items,
            items_per_hour: if hours > 0.0 { ((d.items as f64 / hours) * 10.0).round() / 10.0 } else { 0.0 },
            concurrency,
            steps: d
                .steps
                .iter()
                .map(|(k, (n, ms))| StepTime { step: k.clone(), count: *n, total_ms: *ms, average_ms: if *n > 0 { ms / n } else { 0 } })
                .collect(),
        }
    }
}

/// Resource use of one run.
#[derive(Clone, Debug, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Report {
    /// Largest resident memory seen in one step process.
    pub peak_memory_mb: u64,
    pub items_finished: u64,
    pub items_per_hour: f64,
    /// Items the runner allowed at once when it started.
    pub concurrency: usize,
    pub steps: Vec<StepTime>,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StepTime {
    pub step: String,
    pub count: u64,
    pub total_ms: u64,
    pub average_ms: u64,
}

// ------------------------------------------------------------------------------------ probes

/// The OS, asked through standard tools (no extra crates); values are cached for a second.
#[derive(Debug, Default)]
pub struct SystemProbe {
    free: Mutex<Option<(Instant, Option<u64>)>>,
}

impl Probe for SystemProbe {
    fn free_memory(&self) -> Option<u64> {
        let mut c = self.free.lock().unwrap_or_else(PoisonError::into_inner);
        if let Some((at, v)) = *c
            && at.elapsed() < Duration::from_secs(1)
        {
            return v;
        }
        let v = os::free_memory();
        *c = Some((Instant::now(), v));
        v
    }

    fn cores(&self) -> usize {
        std::thread::available_parallelism().map(usize::from).unwrap_or(2)
    }

    fn free_disk(&self, path: &Path) -> Option<u64> {
        // the nearest folder that exists (the outputs folder may not yet)
        let mut p = path.to_path_buf();
        while !p.exists() {
            p = p.parent()?.to_path_buf();
        }
        os::free_disk(&p)
    }

    fn process_memory(&self, pid: u32) -> Option<u64> {
        os::process_memory(pid)
    }
}

#[cfg_attr(not(any(target_os = "linux", target_os = "macos", windows)), allow(dead_code))]
fn output(program: &str, args: &[&str]) -> Option<String> {
    let o = Command::new(program).args(args).stdin(Stdio::null()).stderr(Stdio::null()).output().ok()?;
    o.status.success().then(|| String::from_utf8_lossy(&o.stdout).into_owned())
}

/// `df -Pk <path>`: the "Available" column of the last line, in KB.
#[cfg(any(target_os = "linux", target_os = "macos"))]
fn df_free(path: &Path) -> Option<u64> {
    let text = output("df", &["-Pk", &path.to_string_lossy()])?;
    let kb: u64 = text.lines().last()?.split_whitespace().nth(3)?.parse().ok()?;
    kb.checked_mul(1024)
}

#[cfg(target_os = "linux")]
mod os {
    use super::*;

    pub fn free_memory() -> Option<u64> {
        let text = std::fs::read_to_string("/proc/meminfo").ok()?;
        kb_field(&text, "MemAvailable:")
    }

    pub fn free_disk(path: &Path) -> Option<u64> {
        df_free(path)
    }

    pub fn process_memory(pid: u32) -> Option<u64> {
        let text = std::fs::read_to_string(format!("/proc/{pid}/status")).ok()?;
        kb_field(&text, "VmRSS:")
    }

    fn kb_field(text: &str, key: &str) -> Option<u64> {
        let line = text.lines().find(|l| l.starts_with(key))?;
        let kb: u64 = line.split_whitespace().nth(1)?.parse().ok()?;
        kb.checked_mul(1024)
    }
}

#[cfg(target_os = "macos")]
mod os {
    use super::*;

    /// `vm_stat`: free + inactive + speculative pages (what new processes can take quickly).
    pub fn free_memory() -> Option<u64> {
        let text = output("vm_stat", &[])?;
        let page: u64 = text.lines().next()?.split("page size of").nth(1)?.split_whitespace().next()?.parse().ok()?;
        let pages = |key: &str| -> u64 {
            text.lines().find(|l| l.starts_with(key)).and_then(|l| l.rsplit(':').next()).and_then(|v| v.trim().trim_end_matches('.').parse().ok()).unwrap_or(0)
        };
        let n = pages("Pages free").saturating_add(pages("Pages inactive")).saturating_add(pages("Pages speculative"));
        n.checked_mul(page)
    }

    pub fn free_disk(path: &Path) -> Option<u64> {
        df_free(path)
    }

    pub fn process_memory(pid: u32) -> Option<u64> {
        let kb: u64 = output("ps", &["-o", "rss=", "-p", &pid.to_string()])?.trim().parse().ok()?;
        kb.checked_mul(1024)
    }
}

#[cfg(windows)]
mod os {
    use super::*;

    fn powershell(script: &str) -> Option<String> {
        output("powershell", &["-NoProfile", "-NonInteractive", "-Command", script])
    }

    pub fn free_memory() -> Option<u64> {
        let kb: u64 = powershell("(Get-CimInstance Win32_OperatingSystem).FreePhysicalMemory")?.trim().parse().ok()?;
        kb.checked_mul(1024)
    }

    pub fn free_disk(path: &Path) -> Option<u64> {
        let p = path.to_string_lossy().replace('\'', "''");
        powershell(&format!("([System.IO.DriveInfo]::new([System.IO.Path]::GetPathRoot('{p}'))).AvailableFreeSpace"))?.trim().parse().ok()
    }

    /// `tasklist` CSV: the last column is "12,345 K".
    pub fn process_memory(pid: u32) -> Option<u64> {
        let text = output("tasklist", &["/FI", &format!("PID eq {pid}"), "/NH", "/FO", "CSV"])?;
        let line = text.lines().find(|l| l.contains(&format!("\"{pid}\"")))?;
        let last = line.rsplit("\",\"").next()?;
        let digits: String = last.chars().filter(char::is_ascii_digit).collect();
        digits.parse::<u64>().ok()?.checked_mul(1024)
    }
}

#[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
mod os {
    use super::*;

    pub fn free_memory() -> Option<u64> {
        None
    }
    pub fn free_disk(_: &Path) -> Option<u64> {
        None
    }
    pub fn process_memory(_: u32) -> Option<u64> {
        None
    }
}

/// A probe with values a test sets (and changes while a batch runs).
#[derive(Debug)]
pub struct FakeProbe {
    pub state: Mutex<FakeState>,
}

#[derive(Clone, Debug)]
pub struct FakeState {
    pub free_memory: Option<u64>,
    pub cores: usize,
    pub free_disk: Option<u64>,
    /// Every step process reports this much memory.
    pub process_memory: Option<u64>,
}

impl FakeProbe {
    /// Plenty of everything: 64 GB free, 16 cores, 1 TB of disk, 100 MB per process.
    pub fn roomy() -> FakeProbe {
        FakeProbe { state: Mutex::new(FakeState { free_memory: Some(64 << 30), cores: 16, free_disk: Some(1 << 40), process_memory: Some(100 << 20) }) }
    }

    pub fn set(&self, f: impl FnOnce(&mut FakeState)) {
        f(&mut self.state.lock().unwrap_or_else(PoisonError::into_inner));
    }

    fn get(&self) -> FakeState {
        self.state.lock().unwrap_or_else(PoisonError::into_inner).clone()
    }
}

impl Probe for FakeProbe {
    fn free_memory(&self) -> Option<u64> {
        self.get().free_memory
    }
    fn cores(&self) -> usize {
        self.get().cores
    }
    fn free_disk(&self, _: &Path) -> Option<u64> {
        self.get().free_disk
    }
    fn process_memory(&self, _: u32) -> Option<u64> {
        self.get().process_memory
    }
}

// --------------------------------------------------------------------------------- keep awake

/// Keeps the machine from idle-sleeping while it lives (best effort: nothing happens when the tool is
/// missing). The helper process also ends by itself when the runner's process is gone.
#[derive(Debug)]
pub struct KeepAwake {
    child: Option<std::process::Child>,
}

impl KeepAwake {
    pub fn start() -> KeepAwake {
        let pid = std::process::id().to_string();
        let mut cmd = if cfg!(target_os = "macos") {
            let mut c = Command::new("caffeinate");
            c.args(["-i", "-w", &pid]);
            c
        } else if cfg!(windows) {
            // ES_CONTINUOUS | ES_SYSTEM_REQUIRED, refreshed every 30 s while the runner lives
            let script = format!(
                "$s='[DllImport(\"kernel32.dll\")] public static extern uint SetThreadExecutionState(uint f);'; \
                 $k=Add-Type -MemberDefinition $s -Name P -Namespace W -PassThru; \
                 while (Get-Process -Id {pid} -ErrorAction SilentlyContinue) {{ [void]$k::SetThreadExecutionState(0x80000001); Start-Sleep -Seconds 30 }}"
            );
            let mut c = Command::new("powershell");
            c.args(["-NoProfile", "-NonInteractive", "-WindowStyle", "Hidden", "-Command", &script]);
            c
        } else {
            let mut c = Command::new("systemd-inhibit");
            c.args(["--what=idle:sleep", "--who=bashcut-batch", "--why=batch running", "--mode=block", "tail", &format!("--pid={pid}"), "-f", "/dev/null"]);
            c
        };
        let child = cmd.stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null()).spawn().ok();
        KeepAwake { child }
    }

    /// Whether the helper is running.
    pub fn active(&self) -> bool {
        self.child.is_some()
    }
}

impl Drop for KeepAwake {
    fn drop(&mut self) {
        if let Some(mut c) = self.child.take() {
            let _ = c.kill();
            let _ = c.wait();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn concurrency_follows_ram_and_cores() {
        let p = FakeProbe::roomy();
        let l = Limits::default();
        assert_eq!(allowed(&p, &l, 4), 4);
        p.set(|s| s.free_memory = Some(7 << 30));
        assert_eq!(allowed(&p, &l, 4), 2, "7 GB at 3 GB per item");
        p.set(|s| s.free_memory = Some(1 << 30));
        assert_eq!(allowed(&p, &l, 4), 1, "at least one");
        p.set(|s| {
            s.free_memory = None;
            s.cores = 6;
        });
        assert_eq!(allowed(&p, &l, 8), 3, "half the cores; unknown RAM does not limit");
        assert!(may_start(&p, &l, 8, 0, 0));
        assert!(may_start(&p, &l, 8, 2, 0));
        assert!(!may_start(&p, &l, 8, 3, 0));
        p.set(|s| s.free_memory = Some(2 << 30));
        assert!(!may_start(&p, &l, 8, 1, 0), "less than 3 GB free");
        assert!(may_start(&p, &l, 8, 0, 0), "an empty batch always starts one");
        p.set(|s| s.free_memory = Some(4 << 30));
        assert!(may_start(&p, &l, 8, 1, 0), "a settled item already holds its memory");
        assert!(!may_start(&p, &l, 8, 1, 1), "a settling item still needs its 3 GB");
    }

    #[test]
    fn disk_low_names_the_folder() {
        let p = FakeProbe::roomy();
        let l = Limits::default();
        let paths = [PathBuf::from("/out")];
        assert_eq!(disk_low(&p, &l, &paths, true), None);
        p.set(|s| s.free_disk = Some(5 << 29)); // 2.5 GB
        assert_eq!(disk_low(&p, &l, &paths, false), None, "2 GB needed");
        assert!(disk_low(&p, &l, &paths, true).is_some_and(|m| m.contains("low disk space")), "3 GB needed for an export");
    }

    #[test]
    fn limits_are_checked_and_have_defaults() {
        let l: Limits = serde_json::from_str("{\"maxItemMemoryGb\": 0}").unwrap();
        assert_eq!(l.memory_cap(), None);
        assert_eq!(l.memory_per_item_gb, 3.0);
        assert!(Limits { memory_per_item_gb: f64::NAN, ..Limits::default() }.validate().is_err());
        assert!(Limits { agent_concurrency: 0, ..Limits::default() }.validate().is_err());
        assert!(Limits::default().validate().is_ok());
    }

    #[test]
    fn meter_reports_peaks_and_step_times() {
        let m = Meter::default();
        m.memory(200 << 20);
        m.memory(100 << 20);
        m.step("step-1", 100);
        m.step("step-1", 300);
        m.item_done();
        let r = m.report(1_800_000, 2);
        assert_eq!(r.peak_memory_mb, 200);
        assert_eq!(r.items_per_hour, 2.0);
        assert_eq!(r.steps[0].average_ms, 200);
    }

    #[test]
    fn the_system_probe_answers_without_crashing() {
        let p = SystemProbe::default();
        assert!(p.cores() >= 1);
        let _ = p.free_memory();
        let _ = p.free_disk(&std::env::temp_dir().join("does/not/exist"));
        let _ = p.process_memory(std::process::id());
    }
}
