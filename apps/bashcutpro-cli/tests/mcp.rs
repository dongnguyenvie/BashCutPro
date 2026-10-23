//! `bashcutpro-cli mcp` as a program (BR4/BR5, `src/mcp_brand.rs`): the headless server
//! (`BASHCUT_BRIDGE=off`, so a running BashCutPro on this machine is not used) answers as
//! BashCutPro, and nothing the agent reads in its context names FilmCraft.

use std::io::{BufRead, BufReader, Write};
use std::path::PathBuf;
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};

use serde_json::{Value, json};

static N: AtomicUsize = AtomicUsize::new(0);

/// A fresh data dir, removed on drop (keeps user presets away from the real one).
struct Scratch(PathBuf);

impl Scratch {
    fn new(tag: &str) -> Self {
        let d = std::env::temp_dir().join(format!("bashcutpro-cli-it-{tag}-{}-{}", std::process::id(), N.fetch_add(1, Ordering::Relaxed)));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        Scratch(d)
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// A minimal MCP client (newline-delimited JSON-RPC over stdio).
struct Mcp {
    child: Child,
    stdin: ChildStdin,
    stdout: BufReader<ChildStdout>,
    next_id: u64,
}

impl Mcp {
    fn start(data_dir: &std::path::Path) -> Self {
        let mut child = Command::new(env!("CARGO_BIN_EXE_bashcutpro-cli"))
            .arg("--data-dir")
            .arg(data_dir)
            .arg("mcp")
            .env("BASHCUT_BRIDGE", "off")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .expect("spawn bashcutpro-cli mcp");
        let stdin = child.stdin.take().unwrap();
        let stdout = BufReader::new(child.stdout.take().unwrap());
        Mcp { child, stdin, stdout, next_id: 1 }
    }

    fn send(&mut self, msg: &Value) {
        writeln!(self.stdin, "{msg}").unwrap();
        self.stdin.flush().unwrap();
    }

    /// Send a request and return its response (notifications in between are skipped).
    fn request(&mut self, method: &str, params: Value) -> Value {
        let id = self.next_id;
        self.next_id += 1;
        self.send(&json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params}));
        loop {
            let mut line = String::new();
            assert!(self.stdout.read_line(&mut line).unwrap() > 0, "the MCP server closed stdout while `{method}` was pending");
            let v: Value = serde_json::from_str(&line).unwrap_or_else(|e| panic!("not JSON-RPC: {e}: {line}"));
            if v["id"] == id {
                return v;
            }
        }
    }

    /// A tool call's first text item.
    fn tool_text(&mut self, name: &str, arguments: Value) -> String {
        let r = self.request("tools/call", json!({"name": name, "arguments": arguments}));
        assert!(r["error"].is_null(), "{name}: {r}");
        r["result"]["content"][0]["text"].as_str().unwrap_or_default().to_string()
    }
}

impl Drop for Mcp {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

#[test]
fn headless_mcp_answers_as_bashcutpro() {
    let d = Scratch::new("mcp");
    let mut m = Mcp::start(&d.0);
    let init =
        m.request("initialize", json!({"protocolVersion": "2025-06-18", "capabilities": {}, "clientInfo": {"name": "bashcutpro-cli-tests", "version": "1"}}));
    let result = &init["result"];
    assert_eq!(result["serverInfo"]["name"], "bashcutpro", "{init}");
    let instructions = result["instructions"].as_str().unwrap_or_default();
    assert!(instructions.contains("BashCutPro") && !instructions.contains("FilmCraft"), "{instructions}");
    m.send(&json!({"jsonrpc": "2.0", "method": "notifications/initialized"}));

    // tools/list: names keep upstream's protocol spelling, no title or description names FilmCraft
    let tools = m.request("tools/list", json!({}));
    let list = tools["result"]["tools"].as_array().cloned().unwrap_or_default();
    assert!(list.iter().any(|t| t["name"] == "command_list"), "{tools}");
    for t in &list {
        for key in ["title", "description"] {
            let s = t[key].as_str().unwrap_or_default();
            assert!(!s.contains("FilmCraft"), "{} {key}: {s}", t["name"]);
        }
    }

    // the command catalog (command_list, command_run command.list): ids stay, labels/params rebranded
    for (name, args) in [("command_list", json!({})), ("command_run", json!({"id": "command.list", "params": {}}))] {
        let text = m.tool_text(name, args);
        let catalog: Value = serde_json::from_str(&text).unwrap_or_else(|e| panic!("{name}: {e}: {text}"));
        let items = catalog.as_array().cloned().unwrap_or_default();
        assert!(items.iter().any(|c| c["id"] == "command.list"), "{name}");
        assert!(!text.contains("FilmCraft") && !text.contains("filmcraft-cli "), "{name} names FilmCraft");
    }
}
