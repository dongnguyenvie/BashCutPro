//! What the doctor reads from the machine, behind a trait so tests use fakes.

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

use bashcut_dirs::child;

/// How long a probe (`--version`, `commands`, `exec transcript.models`, `df`) may run before it is
/// stopped (I7, DR1): a stuck engine must not hang the doctor.
pub const RUN_TIMEOUT: Duration = Duration::from_secs(30);
/// What a probe's stdout may hold (the `commands bashcut --json` list is a few KiB); the rest is read and dropped.
const MAX_STDOUT: usize = 4 << 20;
/// The tail of stderr a probe keeps.
const MAX_STDERR: usize = 64 << 10;

/// A finished program: exit code (None = killed by a signal) and its output.
#[derive(Clone, Debug, Default)]
pub struct Output {
    pub code: Option<i32>,
    pub stdout: String,
    pub stderr: String,
}

impl Output {
    pub fn ok(&self) -> bool {
        self.code == Some(0)
    }
}

pub trait Env {
    /// An environment variable (empty counts as unset).
    fn var(&self, key: &str) -> Option<String>;
    /// Run a program (stdin null, stopped after [`RUN_TIMEOUT`]); None when it cannot be started
    /// (not installed). A stopped program has no exit code, so it is not [`Output::ok`].
    fn run(&self, program: &str, args: &[&str]) -> Option<Output>;
    /// Read a whole file (callers keep to small files or hash in [`Env::hash_file`]).
    fn read(&self, path: &Path) -> Option<Vec<u8>>;
    /// Size of a file in bytes.
    fn file_size(&self, path: &Path) -> Option<u64>;
    /// SHA-256 of a file, lower-case hex.
    fn hash_file(&self, path: &Path) -> Option<String>;
    /// Entries of a folder (full paths), sorted.
    fn list(&self, dir: &Path) -> Vec<PathBuf>;
    /// Free bytes on the volume that holds `path`.
    fn free_bytes(&self, path: &Path) -> Option<u64>;
    /// Installed memory in bytes.
    fn total_ram(&self) -> Option<u64>;
    /// `macos` | `windows` | `linux` | other.
    fn os(&self) -> &str;
    /// This program's folder, where `bashcut_dirs::engine::lookup` looks first (RC2, DR1). None for
    /// fakes, which name the engine by variable or put a real folder on [`Env::path`].
    fn exe_dir(&self) -> Option<PathBuf> {
        None
    }
    /// The `PATH` the engine lookup searches.
    fn path(&self) -> Option<OsString> {
        self.var("PATH").map(OsString::from)
    }
}

/// The real machine.
pub struct RealEnv;

const MAX_LIST: usize = 10_000;

impl Env for RealEnv {
    fn var(&self, key: &str) -> Option<String> {
        std::env::var(key).ok().filter(|v| !v.is_empty())
    }

    fn run(&self, program: &str, args: &[&str]) -> Option<Output> {
        let mut cmd = Command::new(program);
        // the engine probes (`commands`, `exec`) check the engine itself, not the open app (CC3)
        cmd.args(args).env("BASHCUT_BRIDGE", "off");
        // stdin null, its own process group so a timeout also stops what it started (RC3 supervisor, DR1)
        let mut proc = child::spawn(cmd, &child::Opts { stdin: None, group: true, stderr: true }).ok()?;
        let out = child::pump("bashcut-doctor-stdout", proc.stdout.take(), |r| child::read_lines(r, MAX_STDOUT, 0, |_, _| {}).0);
        let err = child::pump("bashcut-doctor-stderr", proc.stderr.take(), |r| child::tail(r, MAX_STDERR));
        let exit = child::supervise(&mut proc, RUN_TIMEOUT, None);
        let stdout = child::collect(&out, child::STDOUT_WAIT);
        let mut stderr = String::from_utf8_lossy(&child::collect(&err, child::STDERR_WAIT)).into_owned();
        // a stopped probe has no exit code, so it never counts as ok
        let code = if exit.stop.is_some() { None } else { exit.code() };
        if exit.stop.is_some() {
            stderr.push_str(&format!("\n(stopped after {} s)", RUN_TIMEOUT.as_secs()));
        }
        Some(Output { code, stdout: String::from_utf8_lossy(&stdout).into_owned(), stderr })
    }

    fn read(&self, path: &Path) -> Option<Vec<u8>> {
        std::fs::read(path).ok()
    }

    fn file_size(&self, path: &Path) -> Option<u64> {
        std::fs::metadata(path).ok().filter(|m| m.is_file()).map(|m| m.len())
    }

    fn hash_file(&self, path: &Path) -> Option<String> {
        use sha2::{Digest, Sha256};
        use std::io::Read;
        let mut f = std::fs::File::open(path).ok()?;
        let mut h = Sha256::new();
        let mut buf = vec![0u8; 1 << 20];
        loop {
            let n = f.read(&mut buf).ok()?;
            if n == 0 {
                break;
            }
            h.update(buf.get(..n)?);
        }
        Some(h.finalize().iter().map(|b| format!("{b:02x}")).collect())
    }

    fn list(&self, dir: &Path) -> Vec<PathBuf> {
        let Ok(rd) = std::fs::read_dir(dir) else { return Vec::new() };
        let mut v: Vec<PathBuf> = rd.flatten().take(MAX_LIST).map(|e| e.path()).collect();
        v.sort();
        v
    }

    fn free_bytes(&self, path: &Path) -> Option<u64> {
        let p = existing_ancestor(path)?;
        if cfg!(windows) {
            // the drive letter of the path, e.g. `C`
            let drive = p.to_string_lossy().chars().next().filter(char::is_ascii_alphabetic)?;
            let script = format!("(Get-PSDrive -Name {drive}).Free");
            let out = self.run("powershell", &["-NoProfile", "-NonInteractive", "-Command", &script])?;
            return out.stdout.trim().parse().ok();
        }
        let ps = p.to_string_lossy().into_owned();
        let out = self.run("df", &["-Pk", &ps])?;
        parse_df_free(&out.stdout)
    }

    fn total_ram(&self) -> Option<u64> {
        if cfg!(target_os = "macos") {
            return self.run("sysctl", &["-n", "hw.memsize"])?.stdout.trim().parse().ok();
        }
        if cfg!(windows) {
            let script = "(Get-CimInstance Win32_ComputerSystem).TotalPhysicalMemory";
            return self.run("powershell", &["-NoProfile", "-NonInteractive", "-Command", script])?.stdout.trim().parse().ok();
        }
        let text = String::from_utf8(self.read(Path::new("/proc/meminfo"))?).ok()?;
        parse_meminfo_total(&text)
    }

    fn os(&self) -> &str {
        std::env::consts::OS
    }

    fn exe_dir(&self) -> Option<PathBuf> {
        std::env::current_exe().ok().and_then(|e| e.parent().map(Path::to_path_buf))
    }

    fn path(&self) -> Option<OsString> {
        std::env::var_os("PATH")
    }
}

fn existing_ancestor(path: &Path) -> Option<&Path> {
    path.ancestors().find(|a| a.exists())
}

/// Free bytes from `df -Pk` output (the 4th column of the second line, in KiB).
pub fn parse_df_free(text: &str) -> Option<u64> {
    let line = text.lines().nth(1)?;
    let kib: u64 = line.split_whitespace().nth(3)?.parse().ok()?;
    kib.checked_mul(1024)
}

/// `MemTotal` from `/proc/meminfo` (kB) in bytes.
pub fn parse_meminfo_total(text: &str) -> Option<u64> {
    let line = text.lines().find(|l| l.starts_with("MemTotal:"))?;
    let kb: u64 = line.split_whitespace().nth(1)?.parse().ok()?;
    kb.checked_mul(1024)
}
