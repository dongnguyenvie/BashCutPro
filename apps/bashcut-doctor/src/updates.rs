//! Update notices for the kit, the plugins and the app. Reports only: nothing is downloaded or installed.
//!
//! PLK7: the kit check now compares the installed kit's tag against the pinned tag from
//! `bashcut/kit.lock`. "Newer kit available" stays an [`Level::Info`] finding so a doctor run before
//! a long batch stays non-fatal.

use std::path::{Path, PathBuf};

use serde_json::Value;

use crate::{Env, Finding, Level, Options, Output, cli_program, data_dir, installed_plugins, parse_version};

/// The kit tag this build is pinned to (PLK7), from `bashcut/kit.lock`. Trimmed of whitespace.
const KIT_LOCK_TAG: &str = include_str!("../../../bashcut/kit.lock");

/// Where releases of the app are listed.
pub const APP_REPO: &str = "dongnguyenvie/BashCutPro";
/// The plugin registry on the default branch.
pub const REGISTRY_URL: &str = "https://raw.githubusercontent.com/dongnguyenvie/bashcutpro-plugins/main/registry.json";

const MAX_REGISTRY: usize = 4 << 20;

pub fn check_updates(env: &dyn Env, opts: &Options) -> Vec<Finding> {
    let mut out = Vec::new();
    plugins(env, opts, &mut out);
    kit(env, opts, &mut out);
    app(env, opts, &mut out);
    out
}

/// The registry: a local file, or a URL fetched with `curl` (or `gh api` for the private repository).
fn registry(env: &dyn Env, opts: &Options) -> Option<Value> {
    let src = opts.registry.clone().or_else(|| env.var("BASHCUTPRO_REGISTRY")).unwrap_or_else(|| REGISTRY_URL.to_string());
    let bytes = if src.starts_with("https://") || src.starts_with("http://") {
        env.run("curl", &["-fsSL", "--max-time", "20", &src]).filter(Output::ok).map(|o| o.stdout.into_bytes()).or_else(|| {
            let api = "repos/dongnguyenvie/bashcutpro-plugins/contents/registry.json";
            env.run("gh", &["api", "-H", "Accept: application/vnd.github.raw", api]).filter(Output::ok).map(|o| o.stdout.into_bytes())
        })?
    } else {
        env.read(Path::new(&src))?
    };
    if bytes.len() > MAX_REGISTRY {
        return None;
    }
    serde_json::from_slice::<Value>(&bytes).ok().filter(|v| v.get("schema").and_then(Value::as_str) == Some("bashcut-registry/1"))
}

fn plugins(env: &dyn Env, opts: &Options, out: &mut Vec<Finding>) {
    let dir = opts.plugins_dir.clone().or_else(|| opts.data_dir.clone().or_else(|| data_dir(env)).map(|d| d.join(bashcut_dirs::PLUGINS_DIR)));
    let Some(dir) = dir else { return };
    let installed = installed_plugins(env, &dir);
    if !installed.iter().any(|(_, m)| m.is_ok()) {
        return;
    }
    let Some(reg) = registry(env, opts) else {
        out.push(Finding::new("update.plugins", Level::Info, "plugin registry not reachable; update check skipped"));
        return;
    };
    let listed = reg.get("plugins").and_then(Value::as_array).cloned().unwrap_or_default();
    for m in installed.iter().filter_map(|(_, m)| m.as_ref().ok()) {
        let (id, have) = (m.id.as_str(), m.version.as_str());
        let key = format!("update.plugin.{id}");
        let Some(entry) = listed.iter().find(|e| e.get("id").and_then(Value::as_str) == Some(id)) else {
            out.push(Finding::new(key, Level::Info, format!("{id} is not in the registry any more")));
            continue;
        };
        let latest = entry.get("version").and_then(Value::as_str).unwrap_or("0");
        match (parse_version(have), parse_version(latest)) {
            (Some(h), Some(l)) if l > h => out.push(
                Finding::new(key, Level::Info, format!("{id} {have} → {latest} available"))
                    .fix("update it from Plugins (installs come only from the registry)"),
            ),
            _ => out.push(Finding::new(key, Level::Ok, format!("{id} {have} is current"))),
        }
    }
}

fn kit(env: &dyn Env, opts: &Options, out: &mut Vec<Finding>) {
    let Some(dir) = opts.kit_dir.clone().or_else(|| env.var("BASHCUTPRO_KIT_DIR").map(PathBuf::from)) else { return };
    let d = dir.to_string_lossy().into_owned();
    let pinned = KIT_LOCK_TAG.trim();
    // What tag is currently checked out in the kit directory?
    let current_tag = env.run("git", &["-C", &d, "describe", "--tags", "--exact-match", "HEAD"]).filter(Output::ok).map(|o| o.stdout.trim().to_string());
    match current_tag {
        Some(ref tag) if tag == pinned => out.push(Finding::new("update.kit", Level::Ok, format!("kit is at pinned tag {pinned}"))),
        Some(ref tag) => {
            // A newer kit tag is available from the registry (PLK7): stays Info so a pre-batch check is non-fatal.
            out.push(
                Finding::new("update.kit", Level::Info, format!("kit tag {tag} differs from pinned {pinned}; newer kit available"))
                    .fix(format!("git -C {d} fetch --tags && git -C {d} checkout {pinned}")),
            );
        }
        None => {
            // Not at a tag or not a git clone; fall back to comparing HEAD with origin HEAD (pre-PLK7 behavior).
            let head = env.run("git", &["-C", &d, "rev-parse", "HEAD"]).filter(Output::ok).map(|o| o.stdout.trim().to_string());
            let remote = env
                .run("git", &["-C", &d, "ls-remote", "origin", "HEAD"])
                .filter(Output::ok)
                .and_then(|o| o.stdout.split_whitespace().next().map(str::to_string));
            match (head, remote) {
                (Some(h), Some(r)) if h != r => out.push(
                    Finding::new("update.kit", Level::Info, format!("newer kit available; pinned to {pinned}"))
                        .fix(format!("git -C {d} fetch --tags && git -C {d} checkout {pinned}")),
                ),
                (Some(_), Some(_)) => out.push(Finding::new("update.kit", Level::Ok, format!("kit is current (pinned to {pinned})"))),
                _ => out.push(Finding::new(
                    "update.kit",
                    Level::Info,
                    format!("{d}: not a git clone or origin not reachable; update check skipped (pinned to {pinned})"),
                )),
            }
        }
    }
}

fn app(env: &dyn Env, opts: &Options, out: &mut Vec<Finding>) {
    let cli = cli_program(env, opts);
    let have = env.run(&cli, &["--version"]).filter(Output::ok).and_then(|o| parse_version(&o.stdout));
    let latest = env
        .run("gh", &["release", "view", "-R", APP_REPO, "--json", "tagName", "-q", ".tagName"])
        .filter(Output::ok)
        .map(|o| o.stdout.trim().to_string())
        .filter(|t| !t.is_empty());
    match (have, latest.as_deref().and_then(parse_version)) {
        (Some(h), Some(l)) if l > h => out.push(
            Finding::new("update.app", Level::Info, format!("BashCutPro {} available (this build {}.{}.{})", latest.as_deref().unwrap_or(""), h.0, h.1, h.2))
                .fix(format!("download it from https://github.com/{APP_REPO}/releases")),
        ),
        (Some(_), Some(_)) => out.push(Finding::new("update.app", Level::Ok, "app is current")),
        _ => out.push(Finding::new("update.app", Level::Info, "release list not reachable (gh not logged in?); update check skipped")),
    }
}
