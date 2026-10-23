//! `bashcut-mcp --version` prints its name and version and exits instead of starting the server (PK3).

use std::process::{Command, Stdio};

#[test]
fn version_prints_name_and_exits() {
    for flag in ["--version", "-V"] {
        let out = Command::new(env!("CARGO_BIN_EXE_bashcut-mcp")).arg(flag).stdin(Stdio::null()).output().unwrap();
        assert!(out.status.success(), "{flag}: {out:?}");
        assert_eq!(String::from_utf8_lossy(&out.stdout), format!("bashcut-mcp {}\n", env!("CARGO_PKG_VERSION")));
    }
}
