//! BashCutPro's own xtask pieces (fork-only file; see `bashcut/in-app-ui.md` §2).
//!
//! - Layers of our crates: `layer_of` in `main.rs` asks this table first (one inserted line), so
//!   upstream's `LAYERS` table is never edited. Names are full package names: ours carry no
//!   `filmcraft-` prefix.
//! - `cargo xtask dev [--release] [--features F] [-- <app args>]` (DV1): one `cargo build` of the app
//!   and every binary it spawns (engine CLI, MCP server, batch runner, doctor), then runs the app with
//!   the args and returns its exit code. `cargo run -p filmcraft` alone leaves the engine stale.
//!   The hook is the first line of `main()` in `main.rs`.

use std::process::{Command, ExitCode};

use serde_json::Value;

/// (package name, layer).
const LAYERS: &[(&str, u8)] = &[
    // the data folder; no deps at all, so the engine and every app may use it
    ("bashcut-dirs", 0),
    // the command-catalog branding rule (RC14); deps only serde_json, so the CLI and ui-egui share it
    ("bashcut-brand", 0),
    // the plugin manifest reader; deps only on bashcut-dirs, so the engine (L4) may use it (RC5)
    ("bashcut-manifest", 1),
    // read-only measurements; deps only on L0–L1 crates, so the engine (L4) may use it
    ("bashcut-analysis", 3),
    // no workspace deps; only ui-egui (L5) and our apps use them
    ("bashcut-plugin-host", 4),
    ("bashcut-agent", 4),
    ("bashcut-batch", 4),
    ("bashcut-connect", 4),
    // apps, and the build helper only their build scripts use (Windows icon + VERSIONINFO, PK4)
    ("bashcutpro-cli", 6),
    ("bashcut-batch-cli", 6),
    ("bashcut-doctor", 6),
    ("bashcut-mcp", 6),
    ("bashcut-winres", 6),
];

/// The layer of one of our crates, or `None` for any other crate.
pub fn layer_of(name: &str) -> Option<u8> {
    LAYERS.iter().find(|(n, _)| *n == name).map(|(_, l)| *l)
}

/// The hook in `main.rs`'s `layer_of`: returns our crate's layer early. A macro so the hook stays one
/// line after `cargo fmt`.
macro_rules! our_layer_first {
    ($name:expr) => {
        if let Some(l) = $crate::bashcut::layer_of($name) {
            return Some(l);
        }
    };
}
pub(crate) use our_layer_first;

/// Packages `cargo xtask dev` builds: the app first, then what it spawns from its own folder.
const DEV_PACKAGES: &[&str] = &["filmcraft", "bashcutpro-cli", "bashcut-mcp", "bashcut-batch-cli", "bashcut-doctor"];

/// Options of `cargo xtask dev`.
#[derive(Debug, Default, PartialEq)]
struct DevArgs {
    release: bool,
    /// Feature names as given (`whisper`, `filmcraft/whisper`), split on commas and spaces.
    features: Vec<String>,
    /// Everything after `--`, passed to the app.
    app_args: Vec<String>,
}

fn parse_dev_args(args: &[String]) -> Result<DevArgs, String> {
    let mut out = DevArgs::default();
    let mut it = args.iter();
    let split = |v: &str| v.split([',', ' ']).filter(|f| !f.is_empty()).map(str::to_string).collect::<Vec<_>>();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--" => {
                out.app_args = it.cloned().collect();
                break;
            }
            "--release" | "-r" => out.release = true,
            "--features" | "-F" => {
                let Some(v) = it.next() else { return Err("dev: --features needs a value".into()) };
                out.features.extend(split(v));
            }
            _ => match a.strip_prefix("--features=") {
                Some(v) => out.features.extend(split(v)),
                None => return Err(format!("dev: unknown option `{a}`\n{DEV_USAGE}")),
            },
        }
    }
    Ok(out)
}

const DEV_USAGE: &str = "usage: cargo xtask dev [--release] [--features F[,F…]] [-- <app args>]";

/// `pkg/feature` for every dev package that declares a bare feature (`--features whisper` turns on
/// whisper in the app and the engine CLI alike); qualified names pass through.
fn qualify_features(features: &[String], md: &Value) -> Result<Vec<String>, String> {
    let mut out = Vec::new();
    for f in features {
        if f.contains('/') {
            out.push(f.clone());
            continue;
        }
        let owners: Vec<&str> = md["packages"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|p| p["name"].as_str().filter(|n| DEV_PACKAGES.contains(n)).filter(|_| p["features"].get(f.as_str()).is_some()))
            .collect();
        if owners.is_empty() {
            return Err(format!("dev: no package of {} has a feature `{f}`", DEV_PACKAGES.join(", ")));
        }
        out.extend(owners.iter().map(|p| format!("{p}/{f}")));
    }
    Ok(out)
}

/// `cargo xtask dev`: build the app and its engine binaries together, then run the app.
pub fn dev(root: &std::path::Path, args: &[String]) -> ExitCode {
    match dev_inner(root, args) {
        Ok(code) => code,
        Err(e) => {
            eprintln!("{e}");
            ExitCode::FAILURE
        }
    }
}

fn dev_inner(root: &std::path::Path, args: &[String]) -> Result<ExitCode, String> {
    let a = parse_dev_args(args)?;
    let cargo = std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into());
    // `cargo metadata` gives the target dir cargo itself uses (CARGO_TARGET_DIR, config, default).
    let out = Command::new(&cargo)
        .current_dir(root)
        .args(["metadata", "--format-version", "1", "--no-deps"])
        .output()
        .map_err(|e| format!("dev: cargo metadata: {e}"))?;
    if !out.status.success() {
        return Err(String::from_utf8_lossy(&out.stderr).into());
    }
    let md: Value = serde_json::from_slice(&out.stdout).map_err(|e| format!("dev: cargo metadata: {e}"))?;
    let target = md["target_directory"].as_str().map(std::path::PathBuf::from).unwrap_or_else(|| root.join("target"));
    let features = qualify_features(&a.features, &md)?;

    let mut build = Command::new(&cargo);
    build.current_dir(root).arg("build");
    for p in DEV_PACKAGES {
        build.args(["-p", p]);
    }
    if a.release {
        build.arg("--release");
    }
    if !features.is_empty() {
        build.args(["--features", &features.join(",")]);
    }
    println!("dev: building {}{}", DEV_PACKAGES.join(", "), if a.release { " (release)" } else { "" });
    let status = build.status().map_err(|e| format!("dev: cargo build: {e}"))?;
    if !status.success() {
        return Err("dev: cargo build failed".into());
    }

    let app = target.join(if a.release { "release" } else { "debug" }).join(format!("filmcraft{}", std::env::consts::EXE_SUFFIX));
    println!("dev: running {}", app.display());
    let status = Command::new(&app).args(&a.app_args).status().map_err(|e| format!("dev: {}: {e}", app.display()))?;
    // A code above 255 (Windows) or none (killed by a signal) is reported as a plain failure.
    Ok(match status.code().and_then(|c| u8::try_from(c).ok()) {
        Some(c) => ExitCode::from(c),
        None => ExitCode::FAILURE,
    })
}

/// The hook in `main.rs`'s `main`: `cargo xtask dev …` returns here with the app's exit code. A macro
/// so the hook stays one line after `cargo fmt`.
macro_rules! dev_first {
    ($task:expr) => {
        if $task == "dev" {
            return $crate::bashcut::dev(&root(), &std::env::args().skip(2).collect::<Vec<_>>());
        }
    };
}
pub(crate) use dev_first;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn our_crates_have_layers_and_others_do_not() {
        assert_eq!(layer_of("bashcut-analysis"), Some(3));
        assert_eq!(layer_of("bashcut-plugin-host"), Some(4));
        assert_eq!(layer_of("filmcraft-engine"), None);
        assert_eq!(layer_of("engine"), None);
    }

    fn strs(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn dev_args_parse() {
        assert_eq!(parse_dev_args(&[]), Ok(DevArgs::default()));
        assert!(parse_dev_args(&strs(&["--release"])).is_ok_and(|a| a.release));
        assert!(parse_dev_args(&strs(&["-r"])).is_ok_and(|a| a.release));
        let a = parse_dev_args(&strs(&["--features", "a,b", "--features=c", "-F", "d e", "--features=,f,"]));
        assert_eq!(a.map(|a| a.features), Ok(strs(&["a", "b", "c", "d", "e", "f"])));
        // Everything after `--` goes to the app untouched, options included.
        let a = parse_dev_args(&strs(&["-r", "--", "--help", "--release", "--", "x"]));
        assert_eq!(a, Ok(DevArgs { release: true, features: vec![], app_args: strs(&["--help", "--release", "--", "x"]) }));
        assert!(parse_dev_args(&strs(&["--features"])).is_err_and(|e| e.contains("needs a value")));
        assert!(parse_dev_args(&strs(&["--bogus"])).is_err_and(|e| e.contains("`--bogus`") && e.contains("usage:")));
        // An app argument before `--` is an unknown option, not silently passed on.
        assert!(parse_dev_args(&strs(&["--control", "9876"])).is_err());
    }

    #[test]
    fn dev_features_qualify() {
        let md = serde_json::json!({ "packages": [
            { "name": "filmcraft", "features": { "default": [], "whisper": [] } },
            { "name": "bashcutpro-cli", "features": { "whisper": [] } },
            { "name": "bashcut-mcp", "features": {} },
            // Not a dev package: its `whisper` must not be turned on.
            { "name": "bashcut-transcribe", "features": { "whisper": [], "extra": [] } },
        ]});
        assert_eq!(qualify_features(&strs(&["whisper"]), &md), Ok(strs(&["filmcraft/whisper", "bashcutpro-cli/whisper"])));
        assert_eq!(qualify_features(&strs(&["default"]), &md), Ok(strs(&["filmcraft/default"])));
        assert_eq!(qualify_features(&strs(&["other/x", "whisper"]), &md), Ok(strs(&["other/x", "filmcraft/whisper", "bashcutpro-cli/whisper"])));
        assert!(qualify_features(&strs(&["extra"]), &md).is_err_and(|e| e.contains("`extra`")));
        assert!(qualify_features(&strs(&["nope"]), &md).is_err());
        assert_eq!(qualify_features(&[], &md), Ok(vec![]));
        // Metadata without packages: a bare feature has no owner.
        assert!(qualify_features(&strs(&["whisper"]), &serde_json::json!({})).is_err());
    }
}
