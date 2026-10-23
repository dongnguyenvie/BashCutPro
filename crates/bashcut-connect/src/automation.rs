//! `<data dir>/bashcut/automation.json` (`bashcut-automation/1`), written by the open window
//! (`crates/ui-egui/src/bashcut_ui/connect.rs`) while its control channel runs.

use serde::{Deserialize, Serialize};
use std::net::{SocketAddr, TcpStream};
use std::path::{Path, PathBuf};
use std::time::Duration;

pub const SCHEMA: &str = "bashcut-automation/1";
const MAX_BYTES: u64 = 64 * 1024;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Automation {
    pub schema: String,
    /// `127.0.0.1:<port>`.
    pub address: String,
    /// The window's control session token (64 hex; `null` in files from older builds).
    #[serde(default)]
    pub token: Option<String>,
    pub pid: u32,
    /// The project open in the window (`None`: unsaved or none).
    #[serde(default)]
    pub project: Option<String>,
}

pub fn file(data_dir: &Path) -> PathBuf {
    data_dir.join("bashcut").join("automation.json")
}

/// The file, if it is there and well-formed (not checked for liveness).
pub fn read(data_dir: &Path) -> Option<Automation> {
    let path = file(data_dir);
    if std::fs::metadata(&path).ok()?.len() > MAX_BYTES {
        return None;
    }
    let a: Automation = serde_json::from_str(&std::fs::read_to_string(&path).ok()?).ok()?;
    (a.schema == SCHEMA).then_some(a)
}

/// The session token of the window at `address`: the file is for that same address, its process
/// is alive and the token is well-formed.
pub fn token_for(data_dir: &Path, address: &str) -> Option<String> {
    let a = read(data_dir)?;
    (a.address == address && crate::pid_alive(a.pid)).then_some(())?;
    a.token.filter(|t| bashcut_dirs::token::valid(t))
}

/// The window to bridge to: the file is well-formed, its process is alive, the address is a
/// loopback address and something accepts connections there within `timeout`.
pub fn live(data_dir: &Path, timeout: Duration) -> Option<Automation> {
    let a = read(data_dir)?;
    if !crate::pid_alive(a.pid) {
        return None;
    }
    let addr: SocketAddr = a.address.parse().ok()?;
    if !addr.ip().is_loopback() {
        return None;
    }
    TcpStream::connect_timeout(&addr, timeout).ok()?;
    Some(a)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::TcpListener;

    fn dir(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("bashcut-connect-auto-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(d.join("bashcut")).unwrap();
        d
    }

    fn put(d: &Path, v: serde_json::Value) {
        std::fs::write(file(d), v.to_string()).unwrap();
    }

    #[test]
    fn token_for_needs_address_pid_and_valid_token() {
        let d = dir("token");
        let token = "ab".repeat(32);
        let put_with = |address: &str, token: serde_json::Value, pid: u32| {
            put(&d, serde_json::json!({"schema": SCHEMA, "address": address, "token": token, "pid": pid}));
        };
        let me = std::process::id();
        put_with("127.0.0.1:9876", token.clone().into(), me);
        assert_eq!(token_for(&d, "127.0.0.1:9876"), Some(token.clone()));
        assert_eq!(token_for(&d, "127.0.0.1:9877"), None, "another address");
        put_with("127.0.0.1:9876", token.clone().into(), u32::MAX - 7);
        assert_eq!(token_for(&d, "127.0.0.1:9876"), None, "dead pid");
        for bad in [serde_json::json!("short"), serde_json::json!("z".repeat(64)), serde_json::Value::Null] {
            put_with("127.0.0.1:9876", bad, me);
            assert_eq!(token_for(&d, "127.0.0.1:9876"), None);
        }
        std::fs::write(file(&d), r#"{"schema":"bashcut-automation/1","address":"127.0.0.1:9876","pid":1}"#).unwrap();
        assert_eq!(token_for(&d, "127.0.0.1:9876"), None, "a file from an older build has no token");
        std::fs::remove_file(file(&d)).unwrap();
        assert_eq!(token_for(&d, "127.0.0.1:9876"), None, "no file");
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn a_live_window_is_found() {
        let d = dir("live");
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap().to_string();
        put(&d, serde_json::json!({"schema": SCHEMA, "address": addr, "token": null, "pid": std::process::id(), "project": "/p/a.fcproj"}));
        let a = live(&d, Duration::from_millis(500)).unwrap();
        assert_eq!(a.project.as_deref(), Some("/p/a.fcproj"));
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn stale_or_foreign_files_are_ignored() {
        let d = dir("stale");
        assert!(read(&d).is_none(), "no file");
        put(&d, serde_json::json!({"schema": "other/1", "address": "127.0.0.1:1", "pid": std::process::id()}));
        assert!(read(&d).is_none(), "wrong schema");
        // a dead process
        put(&d, serde_json::json!({"schema": SCHEMA, "address": "127.0.0.1:1", "pid": u32::MAX - 7}));
        assert!(read(&d).is_some());
        assert!(live(&d, Duration::from_millis(200)).is_none());
        // alive, but nothing listens
        let port = TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port();
        put(&d, serde_json::json!({"schema": SCHEMA, "address": format!("127.0.0.1:{port}"), "pid": std::process::id()}));
        assert!(live(&d, Duration::from_millis(200)).is_none());
        // not loopback
        put(&d, serde_json::json!({"schema": SCHEMA, "address": "10.0.0.1:9876", "pid": std::process::id()}));
        assert!(live(&d, Duration::from_millis(200)).is_none());
        std::fs::write(file(&d), "not json").unwrap();
        assert!(read(&d).is_none());
        let _ = std::fs::remove_dir_all(&d);
    }
}
