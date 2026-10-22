//! BashCutPro doctor (#73): checks a machine before a long unattended run, and says how to fix what is wrong.
//!
//! Checks: the engine, `bashcutpro` or `filmcraft-cli` (found, version, the `bashcut.*` commands a run needs), the agent kit, installed plugins
//! and their runtimes, downloaded models (sizes and SHA-256 against `models.json`), agent CLIs (claude, codex), free
//! disk, memory and the GPU backend. [`updates`] compares installed versions with the registry and the release list
//! and only reports; nothing is installed. Everything the doctor reads goes through [`Env`], so tests use fakes.
//! Design note: `bashcut/doctor.md`.

#![cfg_attr(not(test), deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable))]

pub mod env;
pub mod updates;

use std::path::{Path, PathBuf};

use serde_json::{Value, json};

pub use env::{Env, Output, RealEnv};

/// Commands a batch or recipe run calls; a build without them is too old.
pub const REQUIRED_COMMANDS: &[&str] = &["bashcut.profiles.list", "bashcut.profiles.apply", "bashcut.template.instantiate", "bashcut.audio.polish"];

const GB: u64 = 1 << 30;
const MAX_MANIFEST: usize = 4 << 20;
const MAX_PLUGINS: usize = 200;
const MAX_MODEL_FILES: usize = 2_000;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Level {
    Ok,
    Info,
    Warn,
    Fail,
}

impl Level {
    pub fn label(self) -> &'static str {
        match self {
            Level::Ok => "ok",
            Level::Info => "info",
            Level::Warn => "warn",
            Level::Fail => "fail",
        }
    }
}

#[derive(Clone, Debug)]
pub struct Finding {
    /// `cli`, `cli.commands`, `kit`, `plugin.<id>`, `model.<id>`, `agent.claude`, `disk`, `ram`, `gpu`, `update.*`.
    pub id: String,
    pub level: Level,
    pub message: String,
    pub fix: Option<String>,
}

impl Finding {
    pub fn new(id: impl Into<String>, level: Level, message: impl Into<String>) -> Self {
        Finding { id: id.into(), level, message: message.into(), fix: None }
    }
    pub fn fix(mut self, fix: impl Into<String>) -> Self {
        self.fix = Some(fix.into());
        self
    }
}

#[derive(Clone, Debug)]
pub struct Options {
    pub cli: Option<String>,
    pub data_dir: Option<PathBuf>,
    pub kit_dir: Option<PathBuf>,
    pub plugins_dir: Option<PathBuf>,
    /// Registry file (path or URL) for update checks.
    pub registry: Option<String>,
    /// Hash every downloaded model file (slow for large models); sizes are always checked.
    pub hash_models: bool,
    pub min_free_gb: u64,
    pub min_ram_gb: u64,
}

impl Default for Options {
    fn default() -> Self {
        Options { cli: None, data_dir: None, kit_dir: None, plugins_dir: None, registry: None, hash_models: true, min_free_gb: 10, min_ram_gb: 8 }
    }
}

#[derive(Clone, Debug, Default)]
pub struct Report {
    pub findings: Vec<Finding>,
}

impl Report {
    pub fn worst(&self) -> Level {
        self.findings.iter().map(|f| f.level).max().unwrap_or(Level::Ok)
    }

    /// 1 when anything failed (a run would fail), else 0.
    pub fn exit_code(&self) -> i32 {
        i32::from(self.worst() == Level::Fail)
    }

    pub fn to_json(&self) -> Value {
        json!({
            "schema": "bashcut-doctor/1",
            "ok": self.worst() != Level::Fail,
            "worst": self.worst().label(),
            "findings": self.findings.iter().map(|f| json!({"id": f.id, "level": f.level.label(), "message": f.message, "fix": f.fix})).collect::<Vec<_>>(),
        })
    }

    pub fn to_text(&self) -> String {
        let mut out = String::new();
        for f in &self.findings {
            out.push_str(&format!("[{:<4}] {:<22} {}\n", f.level.label(), f.id, f.message));
            if let Some(fix) = &f.fix {
                out.push_str(&format!("       {:<22} fix: {fix}\n", ""));
            }
        }
        let n = |l: Level| self.findings.iter().filter(|f| f.level == l).count();
        out.push_str(&format!("{} failed, {} warnings, {} ok\n", n(Level::Fail), n(Level::Warn), n(Level::Ok)));
        out
    }
}

/// BashCutPro's data directory, as the app finds it (`bashcut_dirs`: `BASHCUTPRO_DATA_DIR` /
/// `FILMCRAFT_DATA_DIR`, a portable install, else its own per-user folder next to FilmCraft's).
pub fn data_dir(env: &dyn Env) -> Option<PathBuf> {
    let var = |k: &str| env.var(k);
    let exe_dir = std::env::current_exe().ok().and_then(|e| e.parent().map(Path::to_path_buf));
    bashcut_dirs::Host { var: &var, os: env.os(), exe_dir }.data_dir()
}

fn home(env: &dyn Env) -> Option<PathBuf> {
    env.var("HOME").or_else(|| env.var("USERPROFILE")).map(PathBuf::from)
}

/// `1.2.3` (or `v1.2.3`, or the first such word of a line) as numbers; missing parts are 0.
pub fn parse_version(text: &str) -> Option<(u64, u64, u64)> {
    let word = text.split_whitespace().find(|w| w.trim_start_matches('v').starts_with(|c: char| c.is_ascii_digit()))?;
    let mut parts = word.trim_start_matches('v').split(['.', '-', '+']).map(|p| p.parse::<u64>().ok());
    let major = parts.next().flatten()?;
    Some((major, parts.next().flatten().unwrap_or(0), parts.next().flatten().unwrap_or(0)))
}

/// Whether `version` meets a requirement like `>=3.9` (only `>=` and plain versions are understood).
pub fn meets(version: (u64, u64, u64), requirement: &str) -> bool {
    let want = requirement.trim().trim_start_matches(">=").trim();
    match parse_version(want) {
        Some(w) => version >= w,
        None => true,
    }
}

/// The engine program (#154): `--cli`, `BASHCUTPRO_CLI` or `FILMCRAFT_CLI`, else `bashcutpro` when it
/// starts, else `filmcraft-cli`.
pub fn cli_program(env: &dyn Env, opts: &Options) -> String {
    let named = opts.cli.clone().or_else(|| env.var("BASHCUTPRO_CLI")).or_else(|| env.var("FILMCRAFT_CLI"));
    named.unwrap_or_else(|| {
        let pro = env.run("bashcutpro", &["--version"]).is_some_and(|o| o.ok());
        if pro { "bashcutpro" } else { "filmcraft-cli" }.to_string()
    })
}

/// Every check, in a fixed order.
pub fn run_checks(env: &dyn Env, opts: &Options) -> Report {
    let mut r = Report::default();
    let data = opts.data_dir.clone().or_else(|| data_dir(env));
    check_cli(env, opts, &mut r);
    check_kit(env, opts, &mut r);
    let plugins = opts.plugins_dir.clone().or_else(|| data.as_ref().map(|d| d.join(bashcut_dirs::PLUGINS_DIR)));
    check_plugins(env, plugins.as_deref(), &mut r);
    check_models(env, opts, data.as_deref(), &mut r);
    check_agents(env, &mut r);
    check_disk(env, opts, data.as_deref(), &mut r);
    check_ram(env, opts, &mut r);
    check_gpu(env, &mut r);
    r
}

fn check_cli(env: &dyn Env, opts: &Options, r: &mut Report) {
    let cli = cli_program(env, opts);
    let Some(out) = env.run(&cli, &["--version"]).filter(Output::ok) else {
        r.findings.push(
            Finding::new("cli", Level::Fail, format!("`{cli}` was not found or does not start"))
                .fix("install BashCutPro or build bashcutpro / filmcraft-cli (kit skill bcp:setup), or point BASHCUTPRO_CLI / --cli at it"),
        );
        return;
    };
    let version = out.stdout.trim().to_string();
    r.findings.push(Finding::new("cli", Level::Ok, format!("{version} ({cli})")));
    let listed = env.run(&cli, &["commands", "bashcut", "--json"]).filter(Output::ok).and_then(|o| serde_json::from_str::<Value>(&o.stdout).ok());
    let Some(Value::Array(list)) = listed else {
        r.findings.push(
            Finding::new("cli.commands", Level::Warn, "`commands bashcut --json` gave no command list")
                .fix("rebuild bashcutpro (or filmcraft-cli) from BashCutPro main"),
        );
        return;
    };
    let have: Vec<&str> = list.iter().filter_map(|c| c.get("id").and_then(Value::as_str)).collect();
    let missing: Vec<&str> = REQUIRED_COMMANDS.iter().copied().filter(|id| !have.contains(id)).collect();
    if missing.is_empty() {
        r.findings.push(Finding::new("cli.commands", Level::Ok, format!("{} bashcut.* commands", have.len())));
    } else {
        r.findings.push(
            Finding::new("cli.commands", Level::Fail, format!("this build lacks {}", missing.join(", ")))
                .fix("rebuild bashcutpro (or filmcraft-cli) from BashCutPro main"),
        );
    }
    check_speech(env, &cli, r);
}

/// Speech-to-text: the batch presets and the kit's caption skills start with a transcription, which
/// fails on a build without the `whisper` feature or before the default model is downloaded.
fn check_speech(env: &dyn Env, cli: &str, r: &mut Report) {
    let models = env.run(cli, &["exec", "transcript.models"]).filter(Output::ok).and_then(|o| serde_json::from_str::<Value>(&o.stdout).ok());
    let Some(models) = models else {
        r.findings.push(Finding::new("speech", Level::Info, "`transcript.models` gave no answer; speech-to-text not checked"));
        return;
    };
    if models.get("available").and_then(Value::as_bool) != Some(true) {
        r.findings.push(
            Finding::new("speech", Level::Warn, "this engine (filmcraft-cli) has no speech-to-text: transcribing (captions, Vietsub and shorts presets) fails")
                .fix("use a build with speech-to-text (cargo build -p bashcutpro-cli --features whisper)"),
        );
        return;
    }
    let default = models.get("default").and_then(Value::as_str).unwrap_or("whisper-base");
    let installed = models
        .get("models")
        .and_then(Value::as_array)
        .and_then(|list| list.iter().find(|m| m.get("id").and_then(Value::as_str) == Some(default)))
        .and_then(|m| m.get("installed"))
        .and_then(Value::as_bool)
        .unwrap_or(false);
    if installed {
        r.findings.push(Finding::new("speech", Level::Ok, format!("speech model {default} installed")));
    } else {
        r.findings.push(
            Finding::new("speech", Level::Warn, format!("the speech model {default} is not downloaded: transcribing fails until it is"))
                .fix(format!("{cli} exec transcript.downloadModel (or Text panel > Transcribe in the app)")),
        );
    }
}

fn read_json(env: &dyn Env, path: &Path) -> Option<Value> {
    let bytes = env.read(path).filter(|b| b.len() <= MAX_MANIFEST)?;
    serde_json::from_slice(&bytes).ok()
}

fn check_kit(env: &dyn Env, opts: &Options, r: &mut Report) {
    let dir = opts.kit_dir.clone().or_else(|| env.var("BASHCUTPRO_KIT_DIR").map(PathBuf::from));
    if let Some(dir) = dir {
        let manifest = read_json(env, &dir.join(".claude-plugin").join("plugin.json"));
        let version = manifest.as_ref().and_then(|m| m.get("version")).and_then(Value::as_str).map(str::to_string);
        if env.file_size(&dir.join("skills").join("setup").join("SKILL.md")).is_none() {
            r.findings.push(
                Finding::new("kit", Level::Warn, format!("{} is not a bashcutpro-agent-kit clone", dir.display()))
                    .fix("clone dongnguyenvie/bashcutpro-agent-kit there"),
            );
            return;
        }
        r.findings.push(Finding::new("kit", Level::Ok, format!("bashcutpro-agent-kit {} at {}", version.as_deref().unwrap_or("(no version)"), dir.display())));
        return;
    }
    // the Claude Code plugin (`claude plugin install bcp@bashcutpro-agent-kit`, the kit README's way)
    if let Some((path, version)) = home(env).and_then(|h| claude_plugin_kit(env, &h)) {
        let version = version.map(|v| format!(" {v}")).unwrap_or_default();
        r.findings.push(Finding::new("kit", Level::Ok, format!("Claude Code plugin bcp{version} at {}", path.display())));
        return;
    }
    // installed skills: Codex links them as ~/.agents/skills/bcp-<name>, Claude Code may copy them to ~/.claude/skills;
    // the first-run setup clones the kit to ~/.bashcutpro/bashcutpro-agent-kit
    let found = home(env).and_then(|h| {
        [
            h.join(".agents").join("skills").join("bcp-setup"),
            h.join(".claude").join("skills").join("bcp-setup"),
            h.join(".bashcutpro").join("bashcutpro-agent-kit").join("skills").join("setup"),
        ]
        .into_iter()
        .find(|d| env.file_size(&d.join("SKILL.md")).is_some())
    });
    match found {
        Some(d) => r.findings.push(Finding::new("kit", Level::Ok, format!("kit skills installed in {}", d.parent().unwrap_or(&d).display()))),
        None => r.findings.push(
            Finding::new("kit", Level::Warn, "no bashcutpro-agent-kit found")
                .fix("install the kit (its README: Claude Code plugin or scripts/install-codex.sh), or set BASHCUTPRO_KIT_DIR"),
        ),
    }
}

/// The kit installed as a Claude Code plugin: `(install path, version)` from `~/.claude/plugins/installed_plugins.json`
/// (`plugins["bcp@<marketplace>"]`), when its install path still holds the skills.
fn claude_plugin_kit(env: &dyn Env, home: &Path) -> Option<(PathBuf, Option<String>)> {
    let registry = read_json(env, &home.join(".claude").join("plugins").join("installed_plugins.json"))?;
    registry.get("plugins")?.as_object()?.iter().filter(|(k, _)| k.starts_with("bcp@")).find_map(|(_, installs)| {
        installs.as_array()?.iter().find_map(|i| {
            let path = PathBuf::from(i.get("installPath")?.as_str()?);
            env.file_size(&path.join("skills").join("setup").join("SKILL.md"))?;
            Some((path, i.get("version").and_then(Value::as_str).map(str::to_string)))
        })
    })
}

/// Installed plugin manifests: `(folder, manifest)`.
pub fn installed_plugins(env: &dyn Env, dir: &Path) -> Vec<(PathBuf, Value)> {
    env.list(dir)
        .into_iter()
        .take(MAX_PLUGINS)
        .filter_map(|d| read_json(env, &d.join("bashcut-plugin.json")).map(|m| (d, m)))
        .filter(|(_, m)| m.get("schema").and_then(Value::as_str) == Some("bashcut-plugin/1"))
        .collect()
}

/// The program a plugin's entry starts with on this OS (`entry.<os>` overrides `entry.command`).
pub fn entry_program(manifest: &Value, os: &str) -> Option<Vec<String>> {
    let entry = manifest.get("entry")?;
    let list = entry.get(os).or_else(|| entry.get("command"))?.as_array()?;
    let v: Vec<String> = list.iter().filter_map(Value::as_str).map(str::to_string).collect();
    (!v.is_empty()).then_some(v)
}

fn check_plugins(env: &dyn Env, dir: Option<&Path>, r: &mut Report) {
    let Some(dir) = dir else { return };
    let plugins = installed_plugins(env, dir);
    if plugins.is_empty() {
        r.findings.push(Finding::new("plugins", Level::Info, format!("no plugins installed in {}", dir.display())));
        return;
    }
    for (_, m) in &plugins {
        let id = m.get("id").and_then(Value::as_str).unwrap_or("?");
        let version = m.get("version").and_then(Value::as_str).unwrap_or("?");
        let key = format!("plugin.{id}");
        let runtime = m.get("runtime").cloned().unwrap_or(Value::Null);
        let kind = runtime.get("type").and_then(Value::as_str).unwrap_or("");
        if kind != "python" {
            r.findings.push(Finding::new(key, Level::Ok, format!("{id} {version}")));
            continue;
        }
        let want = runtime.get("version").and_then(Value::as_str).unwrap_or(">=3");
        let Some(argv) = entry_program(m, env.os()) else {
            r.findings.push(Finding::new(key, Level::Warn, format!("{id} {version}: no entry command")).fix("reinstall the plugin from the registry"));
            continue;
        };
        // the interpreter itself: `python3` or `py -3`
        let mut args: Vec<&str> = argv.iter().skip(1).map(String::as_str).take_while(|a| a.starts_with('-')).collect();
        args.push("--version");
        let program = argv.first().map(String::as_str).unwrap_or("python3");
        let out = env.run(program, &args).filter(Output::ok);
        let found = out.as_ref().and_then(|o| parse_version(&format!("{} {}", o.stdout, o.stderr)));
        match found {
            Some(v) if meets(v, want) => r.findings.push(Finding::new(key, Level::Ok, format!("{id} {version}, Python {}.{}.{}", v.0, v.1, v.2))),
            Some(v) => r.findings.push(
                Finding::new(key, Level::Warn, format!("{id} {version} needs Python {want}, found {}.{}.{}", v.0, v.1, v.2))
                    .fix(format!("install Python {want}")),
            ),
            None => r.findings.push(
                Finding::new(key, Level::Warn, format!("{id} {version}: `{program}` not found")).fix(format!("install Python {want} so `{program}` runs")),
            ),
        }
    }
}

fn check_models(env: &dyn Env, opts: &Options, data: Option<&Path>, r: &mut Report) {
    let manifest_path = env.var("BASHCUT_MODELS_MANIFEST").map(PathBuf::from).or_else(|| data.map(|d| d.join(bashcut_dirs::PLUGINS_DIR).join("models.json")));
    let Some(manifest_path) = manifest_path else { return };
    let Some(manifest) = read_json(env, &manifest_path) else {
        r.findings.push(Finding::new("models", Level::Info, format!("no model manifest at {}", manifest_path.display())));
        return;
    };
    let cache = env.var("BASHCUT_MODELS_DIR").map(PathBuf::from).or_else(|| data.map(|d| d.join("bashcut-models")));
    let Some(cache) = cache else { return };
    let models = manifest.get("models").and_then(Value::as_array).cloned().unwrap_or_default();
    let mut budget = MAX_MODEL_FILES;
    for m in &models {
        let Some(id) = m.get("id").and_then(Value::as_str).filter(|id| plain(id)) else { continue };
        let files = m.get("files").and_then(Value::as_array).cloned().unwrap_or_default();
        let (mut present, mut bad) = (0usize, Vec::new());
        for f in files.iter().take(budget) {
            budget = budget.saturating_sub(1);
            let Some(rel) = f.get("path").and_then(Value::as_str).filter(|p| !p.contains("..") && !p.starts_with(['/', '\\'])) else { continue };
            let path = cache.join(id).join(rel);
            let Some(size) = env.file_size(&path) else { continue };
            present += 1;
            if f.get("size").and_then(Value::as_u64).is_some_and(|want| want != size) {
                bad.push(format!("{rel} (size)"));
                continue;
            }
            if opts.hash_models {
                let want = f.get("sha256").and_then(Value::as_str).unwrap_or("").to_ascii_lowercase();
                if env.hash_file(&path).is_none_or(|got| got != want) {
                    bad.push(format!("{rel} (sha256)"));
                }
            }
        }
        let key = format!("model.{id}");
        if !bad.is_empty() {
            r.findings.push(
                Finding::new(key, Level::Fail, format!("{id}: wrong file(s) {}", bad.join(", ")))
                    .fix(format!("delete {} and let the plugin download it again", cache.join(id).display())),
            );
        } else if present == 0 {
            r.findings.push(Finding::new(key, Level::Info, format!("{id}: not downloaded (the plugin fetches it on first use)")));
        } else if present < files.len() {
            r.findings.push(
                Finding::new(key, Level::Warn, format!("{id}: {present} of {} files downloaded", files.len()))
                    .fix("run the plugin once to finish the download"),
            );
        } else {
            r.findings.push(Finding::new(key, Level::Ok, format!("{id}: {present} files verified")));
        }
    }
}

fn plain(id: &str) -> bool {
    !id.is_empty() && !id.contains(['/', '\\']) && !id.contains("..")
}

fn check_agents(env: &dyn Env, r: &mut Report) {
    let mut any = false;
    for name in ["claude", "codex"] {
        match env.run(name, &["--version"]).filter(Output::ok) {
            Some(o) => {
                any = true;
                r.findings.push(Finding::new(format!("agent.{name}"), Level::Ok, o.stdout.trim().to_string()));
            }
            None => r.findings.push(Finding::new(format!("agent.{name}"), Level::Info, format!("`{name}` not found"))),
        }
    }
    if !any {
        r.findings.push(Finding::new("agent", Level::Warn, "no agent CLI (claude or codex) found").fix("install Claude Code or Codex and log in"));
    }
}

fn check_disk(env: &dyn Env, opts: &Options, data: Option<&Path>, r: &mut Report) {
    let at = data.map(Path::to_path_buf).or_else(|| home(env)).unwrap_or_else(|| PathBuf::from("."));
    let Some(free) = env.free_bytes(&at) else {
        r.findings.push(Finding::new("disk", Level::Info, format!("free space at {} unknown", at.display())));
        return;
    };
    let gb = free / GB;
    let msg = format!("{gb} GB free at {}", at.display());
    if free < 2 * GB {
        r.findings.push(Finding::new("disk", Level::Fail, msg).fix("free disk space: exports, previews and models need room"));
    } else if gb < opts.min_free_gb {
        r.findings.push(Finding::new("disk", Level::Warn, msg).fix(format!("keep at least {} GB free for a batch", opts.min_free_gb)));
    } else {
        r.findings.push(Finding::new("disk", Level::Ok, msg));
    }
}

fn check_ram(env: &dyn Env, opts: &Options, r: &mut Report) {
    let Some(ram) = env.total_ram() else {
        r.findings.push(Finding::new("ram", Level::Info, "memory size unknown"));
        return;
    };
    let gb = ram.div_ceil(GB);
    if gb < opts.min_ram_gb {
        r.findings
            .push(Finding::new("ram", Level::Warn, format!("{gb} GB memory")).fix("4K exports and local models want 16 GB; use proxies and smaller models"));
    } else {
        r.findings.push(Finding::new("ram", Level::Ok, format!("{gb} GB memory")));
    }
}

fn check_gpu(env: &dyn Env, r: &mut Report) {
    let default = match env.os() {
        "macos" => "Metal",
        "windows" => "DirectX 12 (Vulkan fallback)",
        _ => "Vulkan",
    };
    match env.var("WGPU_BACKEND") {
        Some(b) => r.findings.push(Finding::new("gpu", Level::Info, format!("GPU backend forced to `{b}` (WGPU_BACKEND)"))),
        None => r.findings.push(Finding::new("gpu", Level::Ok, format!("GPU backend: {default} (wgpu default)"))),
    }
}
