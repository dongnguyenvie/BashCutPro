//! `--bridge` defaults to the running BashCutPro (CC2, `bashcut/decisions.md`): for a subcommand
//! that talks to the bridge, with no `--bridge` and nothing that asks for the headless engine, the
//! address published in `<data dir>/bashcut/automation.json` (CC1) is added to the arguments.
//! `BASHCUT_BRIDGE=off` keeps the headless engine. `build.rs` routes upstream's argument list
//! through [`with_default_bridge`] and generates `BRIDGE_SUBCOMMANDS` from upstream's `main.rs`.

use std::time::Duration;

include!(concat!(env!("OUT_DIR"), "/bridge_subcommands.rs"));

/// Options that mean the headless engine (a project to open, save or a data directory to read).
const HEADLESS: &[&str] = &["--project", "--demo", "--save", "--save-as", "--data-dir"];
/// How long the published port may take to accept a connection.
const CONNECT: Duration = Duration::from_millis(300);

pub fn with_default_bridge(args: Vec<String>) -> Vec<String> {
    if std::env::var("BASHCUT_BRIDGE").is_ok_and(|v| v.trim().eq_ignore_ascii_case("off")) || !wants_bridge(&args) {
        return args;
    }
    let Some(live) = bashcut_dirs::data_dir().and_then(|d| bashcut_connect::automation::live(&d, CONNECT)) else {
        return args;
    };
    crate::diag(format_args!("bashcutpro: using the running BashCutPro at {} (BASHCUT_BRIDGE=off for headless)", live.address));
    let mut out = args;
    out.extend(["--bridge".to_string(), live.address]);
    out
}

/// A bridge subcommand with no `--bridge` (in any spelling) and no headless option.
fn wants_bridge(args: &[String]) -> bool {
    let a = crate::args::Args::parse(args.iter().cloned());
    let given = |k: &str| args.iter().any(|s| s == k || s.strip_prefix(k).is_some_and(|r| r.starts_with('=')));
    a.pos(0).is_some_and(|sub| BRIDGE_SUBCOMMANDS.contains(&sub)) && !given("--bridge") && !HEADLESS.iter().any(|k| given(k))
}

#[cfg(test)]
mod tests {
    #[test]
    #[ignore = "test ticket: exec/run/inspect/mcp with no --bridge get `--bridge <automation.json address>` when a live window is published; --project/--demo/--save/--save-as/--data-dir, an explicit --bridge, BASHCUT_BRIDGE=off, render/probe/help and a dead pid leave the args unchanged"]
    fn bridge_defaults_to_the_running_app() {}
}
