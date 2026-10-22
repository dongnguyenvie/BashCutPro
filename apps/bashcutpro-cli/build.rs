//! Generates the `bashcutpro` source from upstream `apps/filmcraft-cli/src` (SB2,
//! `bashcut/decisions.md`): copies it into `OUT_DIR` with the user-visible name changed
//! (`filmcraft-cli` → `bashcutpro`, "FilmCraft" → "BashCutPro" in the help). Command ids, JSON
//! fields, MCP tool names and `FILMCRAFT_*` variables are a protocol and stay as they are.
//!
//! Every anchor below must be found, else the build fails: when upstream rewrites its help we fix
//! this table instead of shipping half-renamed text.

use std::fs;
use std::path::{Path, PathBuf};

const UPSTREAM: &str = "../filmcraft-cli";

/// (file, text that must be there, its replacement); applied before the blanket rename.
const ANCHORS: &[(&str, &str, &str)] = &[
    ("main.rs", "filmcraft-cli: drive FilmCraft from the shell", "bashcutpro: drive BashCutPro from the shell"),
    ("main.rs", "USAGE\n  filmcraft-cli [OPTIONS]", "USAGE\n  bashcutpro [OPTIONS]"),
    ("main.rs", "FilmCraft data directory", "BashCutPro data directory"),
    ("main.rs", "outln!(\"filmcraft-cli {}\", env!(\"CARGO_PKG_VERSION\"))", "outln!(\"bashcutpro {}\", env!(\"CARGO_PKG_VERSION\"))"),
    ("main.rs", "\"filmcraft-cli: {msg}\\nRun `filmcraft-cli help` for usage.\"", "\"bashcutpro: {msg}\\nRun `bashcutpro help` for usage.\""),
    ("main.rs", "diag(format_args!(\"filmcraft-cli: {msg}\"))", "diag(format_args!(\"bashcutpro: {msg}\"))"),
];

/// Upstream's file list and its `mod` lines (rewritten to `#[path]` into `OUT_DIR`).
const FILES: &[&str] = &["main.rs", "args.rs", "probe.rs"];
const MODS: &[&str] = &["args", "probe"];
/// Upstream's crate-level attribute; `src/main.rs` carries the same one (`include!` takes no
/// inner attributes).
const INNER_ATTR: &str =
    "#![cfg_attr(not(test), deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable))]";

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

fn main() -> Result<(), String> {
    let up = Path::new(UPSTREAM);
    let out = PathBuf::from(std::env::var("OUT_DIR").map_err(|e| format!("bashcutpro-cli: OUT_DIR: {e}"))?);
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed={UPSTREAM}/Cargo.toml");
    println!("cargo:rerun-if-changed={UPSTREAM}/src");

    let (theirs, ours) = (deps(&read(&up.join("Cargo.toml"))?), deps(&read(Path::new("Cargo.toml"))?));
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
        // Inner doc comments can't be `include!`d; the rest of the text gets the blanket rename.
        let body: Vec<&str> = src.lines().filter(|l| !l.starts_with("//!")).collect();
        let text = body.join("\n").replace("filmcraft-cli", "bashcutpro") + "\n";
        fs::write(out.join(file), text).map_err(|e| format!("bashcutpro-cli: can't write {file}: {e}"))?;
    }
    Ok(())
}
