//! `bashcutpro` and `bcpro` against a fake `filmcraft-cli` (`tests/support/fake_engine.rs`): same
//! arguments, stdin, stdout, stderr and exit code; the engine beside the wrapper, else on `PATH`.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

const FAKE: &str = env!("CARGO_BIN_EXE_bashcut-cli-fake-engine");
const PRO: &str = env!("CARGO_BIN_EXE_bashcutpro");
const ALIAS: &str = env!("CARGO_BIN_EXE_bcpro");

fn exe(name: &str) -> String {
    format!("{name}{}", std::env::consts::EXE_SUFFIX)
}

/// A folder with copies of both wrapper names and, when `engine`, the fake as `filmcraft-cli`.
fn install(name: &str, engine: bool) -> PathBuf {
    let d = std::env::temp_dir().join(format!("bashcut-cli-test-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    std::fs::copy(PRO, d.join(exe("bashcutpro"))).unwrap();
    std::fs::copy(ALIAS, d.join(exe("bcpro"))).unwrap();
    if engine {
        std::fs::copy(FAKE, d.join(exe("filmcraft-cli"))).unwrap();
    }
    d
}

fn run(program: &Path, args: &[&str], path: &Path, stdin: &str) -> Output {
    let mut c = Command::new(program).args(args).env("PATH", path).stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn().unwrap();
    c.stdin.take().unwrap().write_all(stdin.as_bytes()).unwrap();
    c.wait_with_output().unwrap()
}

#[test]
fn same_args_stdio_and_exit_code_as_the_engine() {
    let d = install("same", true);
    let nowhere = d.join("no-such-dir");
    let args = ["run", "script with spaces.jsonl", "--stdin", "--exit", "3", "ünï"];
    let direct = run(&d.join(exe("filmcraft-cli")), &args, &nowhere, "line one\nline two\n");
    assert_eq!(String::from_utf8_lossy(&direct.stdout), "args run|script with spaces.jsonl|--stdin|--exit|3|ünï\nstdin line one\nline two\n");
    for name in ["bashcutpro", "bcpro"] {
        let via = run(&d.join(exe(name)), &args, &nowhere, "line one\nline two\n");
        assert_eq!(via.status.code(), Some(3), "{name}");
        assert_eq!(via.stdout, direct.stdout, "{name}");
        assert_eq!(via.stderr, direct.stderr, "{name}");
    }
    assert_eq!(run(&d.join(exe("bashcutpro")), &["--version"], &nowhere, "").status.code(), Some(0));
}

#[test]
fn engine_on_path_when_none_beside() {
    let (d, engine) = (install("pathonly", false), install("engine", true));
    let out = run(&d.join(exe("bashcutpro")), &["--exit", "0"], &engine, "");
    assert_eq!(out.status.code(), Some(0), "{}", String::from_utf8_lossy(&out.stderr));
    assert!(String::from_utf8_lossy(&out.stdout).starts_with("args --exit|0"));
}

#[test]
fn no_engine_says_what_to_do() {
    let d = install("none", false);
    let out = run(&d.join(exe("bcpro")), &["--version"], &d.join("no-such-dir"), "");
    assert_eq!(out.status.code(), Some(127));
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(err.contains("filmcraft-cli") && err.contains("install BashCutPro again"), "{err}");
}
