//! Generates the `bashcutpro-cli` source from upstream `apps/filmcraft-cli/src` (SB2,
//! `bashcut/decisions.md`): copies it into `OUT_DIR` with the user-visible name changed
//! (`filmcraft-cli` → `bashcutpro-cli`, "FilmCraft" → "BashCutPro" in the help). Command ids, JSON
//! fields, MCP tool names and `FILMCRAFT_*` variables are a protocol and stay as they are.
//!
//! The anchors are tripwires: every one must be found, else the build fails, so when upstream
//! rewrites its help we fix this table instead of shipping half-renamed text. After them, the rest
//! of the text gets [`rename`], which only touches the program name as a word (`filmcraft-cli`
//! followed by a space or `:`), so a future `filmcraft-cli/1` schema, `filmcraft-cli-…` file name or
//! other protocol string keeps upstream's spelling.
//!
//! On Windows it also embeds the BashCutPro icon and version info (`bashcut-winres`, PK4).

use std::fs;
use std::path::{Path, PathBuf};

const UPSTREAM: &str = "../filmcraft-cli";

/// (file, text that must be there, its replacement); applied before [`rename`].
const ANCHORS: &[(&str, &str, &str)] = &[
    ("main.rs", "filmcraft-cli: drive FilmCraft from the shell", "bashcutpro-cli: drive BashCutPro from the shell"),
    ("main.rs", "USAGE\n  filmcraft-cli [OPTIONS]", "USAGE\n  bashcutpro-cli [OPTIONS]"),
    ("main.rs", "FilmCraft data directory", "BashCutPro data directory"),
    ("main.rs", "outln!(\"filmcraft-cli {}\", env!(\"CARGO_PKG_VERSION\"))", "outln!(\"bashcutpro-cli {}\", env!(\"CARGO_PKG_VERSION\"))"),
    ("main.rs", "\"filmcraft-cli: {msg}\\nRun `filmcraft-cli help` for usage.\"", "\"bashcutpro-cli: {msg}\\nRun `bashcutpro-cli help` for usage.\""),
    ("main.rs", "diag(format_args!(\"filmcraft-cli: {msg}\"))", "diag(format_args!(\"bashcutpro-cli: {msg}\"))"),
    (
        "main.rs",
        "send commands to the running app (`filmcraft --control PORT`)
",
        "send commands to the running app (`filmcraft --control PORT`)
                                default: the open BashCutPro (automation.json); BASHCUT_BRIDGE=off: headless
",
    ),
    // CC2: `--bridge` defaults to the running BashCutPro (`src/bridge_default.rs`).
    (
        "main.rs",
        "Args::parse(std::env::args_os().skip(1).map(|a| a.to_string_lossy().into_owned()))",
        "Args::parse(bridge_default::with_default_bridge(std::env::args_os().skip(1).map(|a| a.to_string_lossy().into_owned()).collect()))",
    ),
];

/// CC2: the subcommands that get a default `--bridge`, and the ones whose arm mentions the bridge
/// but stay headless. Together they must be exactly upstream's `match cmd` arms that call
/// `Backend::open(` or read `"--bridge"`, else the build fails (`bridge_subcommands`).
const BRIDGED: &[&str] = &["commands", "describe", "exec", "run", "inspect", "import", "export", "mcp"];
const NOT_BRIDGED: &[&str] = &["render"];

/// Upstream's file list and its `mod` lines (rewritten to `#[path]` into `OUT_DIR`).
const FILES: &[&str] = &["main.rs", "args.rs", "probe.rs"];
const MODS: &[&str] = &["args", "probe"];
/// Upstream's crate-level attribute; `src/main.rs` carries the same one (`include!` takes no
/// inner attributes).
const INNER_ATTR: &str =
    "#![cfg_attr(not(test), deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable))]";

/// `filmcraft-cli` → `bashcutpro-cli` where it is the program name in text (`filmcraft-cli help`,
/// `filmcraft-cli: …`), not part of a longer token.
fn rename(text: &str) -> String {
    const FROM: &str = "filmcraft-cli";
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(i) = rest.find(FROM) {
        let (before, after) = rest.split_at(i);
        let tail = after.get(FROM.len()..).unwrap_or_default();
        out.push_str(before);
        out.push_str(if tail.starts_with([' ', ':']) { "bashcutpro-cli" } else { FROM });
        rest = tail;
    }
    out.push_str(rest);
    out
}

fn read(p: &Path) -> Result<String, String> {
    fs::read_to_string(p).map_err(|e| format!("bashcutpro-cli: can't read {}: {e}", p.display()))
}

/// The `[dependencies]` lines of a manifest (comments and blank lines dropped).
fn deps(manifest: &str) -> Vec<String> {
    let mut on = false;
    let mut out = Vec::new();
    for line in manifest.lines().map(str::trim) {
        if line.starts_with('[') {
            on = line == "[dependencies]";
        } else if on && !line.is_empty() && !line.starts_with('#') {
            out.push(line.to_string());
        }
    }
    out
}

/// The subcommands of upstream's `match cmd { … }` (arms at 8 spaces) whose body reaches the bridge.
fn bridge_arms(main_rs: &str) -> Result<Vec<String>, String> {
    let start =
        main_rs.find("    match cmd {\n").ok_or("bashcutpro-cli: `match cmd {` not found in apps/filmcraft-cli/src/main.rs; update bridge_arms in build.rs")?;
    let mut arms: Vec<(Vec<String>, String)> = Vec::new();
    for line in main_rs.get(start..).unwrap_or_default().lines().skip(1) {
        if line.starts_with("        other =>") {
            break;
        }
        match line.strip_prefix("        \"").and_then(|l| l.split_once("=>")) {
            Some((names, rest)) => arms.push((format!("\"{names}").split('|').map(|n| n.trim().trim_matches('"').to_string()).collect(), rest.to_string())),
            None => {
                if let Some((_, body)) = arms.last_mut() {
                    body.push_str(line);
                }
            }
        }
    }
    Ok(arms.into_iter().filter(|(_, body)| body.contains("Backend::open(") || body.contains("\"--bridge\"")).flat_map(|(names, _)| names).collect())
}

/// Checks [`bridge_arms`] against `BRIDGED` + `NOT_BRIDGED` and writes `BRIDGE_SUBCOMMANDS`.
fn bridge_subcommands(main_rs: &str, out: &Path) -> Result<(), String> {
    let mut found = bridge_arms(main_rs)?;
    let mut listed: Vec<String> = BRIDGED.iter().chain(NOT_BRIDGED).map(|s| s.to_string()).collect();
    found.sort();
    listed.sort();
    if found != listed {
        return Err(format!(
            "bashcutpro-cli: the upstream subcommands that use --bridge changed; update BRIDGED/NOT_BRIDGED in build.rs\n upstream: {found:?}\n ours: {listed:?}"
        ));
    }
    let code = format!("/// Generated by build.rs (`BRIDGED`).\npub const BRIDGE_SUBCOMMANDS: &[&str] = &{BRIDGED:?};\n");
    fs::write(out.join("bridge_subcommands.rs"), code).map_err(|e| format!("bashcutpro-cli: can't write bridge_subcommands.rs: {e}"))
}

fn main() -> Result<(), String> {
    let up = Path::new(UPSTREAM);
    let out = PathBuf::from(std::env::var("OUT_DIR").map_err(|e| format!("bashcutpro-cli: OUT_DIR: {e}"))?);
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed={UPSTREAM}/Cargo.toml");
    println!("cargo:rerun-if-changed={UPSTREAM}/src");

    // our own `bashcut-*` crates (CC2: `bashcut-connect`, `bashcut-dirs`) are on top of upstream's list
    let theirs = deps(&read(&up.join("Cargo.toml"))?);
    let ours: Vec<String> = deps(&read(Path::new("Cargo.toml"))?).into_iter().filter(|d| !d.starts_with("bashcut-")).collect();
    if theirs != ours {
        return Err(format!(
            "bashcutpro-cli: apps/filmcraft-cli [dependencies] changed; copy them into apps/bashcutpro-cli/Cargo.toml\n upstream: {theirs:?}\n ours: {ours:?}"
        ));
    }
    let listed: Vec<String> =
        fs::read_dir(up.join("src")).map_err(|e| format!("bashcutpro-cli: {e}"))?.filter_map(|e| e.ok()?.file_name().into_string().ok()).collect();
    if listed.len() != FILES.len() || !FILES.iter().all(|f| listed.iter().any(|l| l == f)) {
        return Err(format!("bashcutpro-cli: apps/filmcraft-cli/src has {listed:?}, expected {FILES:?}; update FILES/MODS in build.rs"));
    }

    for file in FILES {
        // Windows checkouts may carry CRLF; the anchors are written with `\n`.
        let mut src = read(&up.join("src").join(file))?.replace("\r\n", "\n");
        for (f, from, to) in ANCHORS.iter().filter(|(f, ..)| f == file) {
            if !src.contains(from) {
                return Err(format!(
                    "bashcutpro-cli: anchor not found in apps/filmcraft-cli/src/{f}: {from:?} (upstream changed its text; update ANCHORS in build.rs)"
                ));
            }
            src = src.replace(from, to);
        }
        if *file == "main.rs" {
            bridge_subcommands(&src, &out)?;
            if !src.contains(INNER_ATTR) {
                return Err(format!("bashcutpro-cli: apps/filmcraft-cli/src/main.rs lost its crate attribute {INNER_ATTR:?}; update build.rs and src/main.rs"));
            }
            src = src.replace(INNER_ATTR, "");
            for m in MODS {
                let line = format!("mod {m};");
                if !src.contains(&line) {
                    return Err(format!("bashcutpro-cli: `{line}` not found in apps/filmcraft-cli/src/main.rs; update MODS in build.rs"));
                }
                let path = out.join(format!("{m}.rs"));
                src = src.replace(&line, &format!("#[path = {:?}]\nmod {m};", path.to_string_lossy()));
            }
        }
        // Inner doc comments can't be `include!`d; the rest of the text gets the narrow rename.
        let body: Vec<&str> = src.lines().filter(|l| !l.starts_with("//!")).collect();
        let text = rename(&body.join("\n")) + "\n";
        fs::write(out.join(file), text).map_err(|e| format!("bashcutpro-cli: can't write {file}: {e}"))?;
    }
    // PK4: the BashCutPro icon and VERSIONINFO; one resource for both exes, so `bcpro.exe` names
    // `bashcutpro-cli.exe` as its original file name
    bashcut_winres::embed("bashcutpro-cli.exe", "BashCutPro command line")
}
