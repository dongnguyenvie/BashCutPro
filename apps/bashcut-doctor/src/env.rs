//! What the doctor reads from the machine, behind a trait so tests use fakes.

use std::path::{Path, PathBuf};
use std::process::Command;

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
    /// Run a program; None when it cannot be started (not installed).
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
    /// The engine when no `--cli` or variable names it (RC2). The real machine asks
    /// `bashcut_dirs::engine` (next to this program, else on `PATH`); the default, for fakes, is the
    /// first of `bashcut_dirs::engine::NAMES` whose `--version` starts.
    fn find_engine(&self) -> Option<String> {
        bashcut_dirs::engine::NAMES.iter().find(|n| self.run(n, &["--version"]).is_some_and(|o| o.ok())).map(|n| n.to_string())
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
        let out = Command::new(program).args(args).output().ok()?;
        Some(Output {
            code: out.status.code(),
            stdout: String::from_utf8_lossy(&out.stdout).into_owned(),
            stderr: String::from_utf8_lossy(&out.stderr).into_owned(),
        })
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

    fn find_engine(&self) -> Option<String> {
        let found = bashcut_dirs::engine::beside_self().or_else(|| bashcut_dirs::engine::on_path(std::env::var_os("PATH").as_deref()));
        found.map(|p| p.to_string_lossy().into_owned())
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
