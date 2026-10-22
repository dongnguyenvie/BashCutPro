//! Pipeline files (`bashcut-batch/1`): what to do with every input. A pipeline is plain JSON, so
//! agents and the Batch section (#46) write it the same way; relative paths in it are resolved
//! against the folder of the file (or the working folder for a preset).

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{BTreeMap, HashSet};
use std::path::{Path, PathBuf};

pub const SCHEMA: &str = "bashcut-batch/1";

/// Extensions (lower case) a folder input expands to.
pub const MEDIA_EXTENSIONS: &[&str] =
    &["mp4", "mov", "m4v", "mkv", "webm", "avi", "mxf", "mts", "m2ts", "wav", "mp3", "m4a", "aac", "flac", "aif", "aiff", "ogg", "png", "jpg", "jpeg"];
/// The project extension: such an input is copied, not imported.
pub const PROJECT_EXTENSION: &str = "fcproj";

const MAX_FILE_BYTES: u64 = 4 << 20;
const MAX_INPUTS: usize = 10_000;
const MAX_STEPS: usize = 500;
const MAX_OUTPUTS: usize = 64;
pub const MAX_CONCURRENCY: usize = 64;
const MAX_DEPTH: usize = 64;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Pipeline {
    pub schema: String,
    #[serde(default)]
    pub name: String,
    /// One line for preset lists.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub description: String,
    /// Files, folders (their media files, not recursive) and `.fcproj` projects.
    pub inputs: Vec<String>,
    /// How each item's project starts (default: a new project that imports the input).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub template: Option<Template>,
    #[serde(default)]
    pub steps: Vec<Step>,
    #[serde(default)]
    pub outputs: Vec<Output>,
    #[serde(default)]
    pub on_failure: OnFailure,
    #[serde(default = "one")]
    pub concurrency: usize,
    /// RAM / CPU / disk limits for the runner (#61, [`crate::governor::Limits`]).
    #[serde(default)]
    pub resources: crate::governor::Limits,
    /// Agent steps running at once, across items (separate from `concurrency`; default 1).
    #[serde(default = "one")]
    pub agent_concurrency: usize,
    /// Item (and project file) name; `{stem}` is the input's file stem, `{index}` its 1-based
    /// number (001…). Default `{stem}`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub output_name: Option<String>,
    /// Where the item projects go (`<name>.fcproj`); the job book lives in its `.bashcut-batch/`.
    pub projects_folder: String,
    /// Where exports go (default: `<projectsFolder>/exports`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub outputs_folder: Option<String>,
    /// Extensions a folder input expands to (default: [`MEDIA_EXTENSIONS`]).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub extensions: Vec<String>,
}

fn one() -> usize {
    1
}

fn yes() -> bool {
    true
}

fn empty_object() -> Value {
    Value::Object(serde_json::Map::new())
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum OnFailure {
    /// The failed item stops; the others go on.
    #[default]
    Skip,
    /// No new item starts after a failure; items already running stop before their next step.
    Stop,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Template {
    /// A project copied for every item (a template, #42).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub project: Option<String>,
    /// Commands that build a new project from the input, instead of [`default_create_commands`].
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub commands: Vec<CommandLine>,
}

/// One engine command: a line of a `filmcraft-cli run` script.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CommandLine {
    pub id: String,
    #[serde(default = "empty_object")]
    pub params: Value,
}

/// A pipeline step: exactly one of `command`, `commands`, `plugin`, `agent`.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Step {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    /// An engine command id, with `params`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub command: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub params: Option<Value>,
    /// Several engine commands in one process: all saved, or none.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub commands: Vec<CommandLine>,
    /// A plugin action (or any program): it connects to the project itself (`--project {project}`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub plugin: Option<ProcessStep>,
    /// The plugin action a `plugin` step was made from, with its params as given (BT1). Steps saved
    /// before it have only the command line.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub plugin_action: Option<PluginAction>,
    /// A kit skill run by a headless agent (#60 adds review rounds and caps).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent: Option<AgentStep>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timeout_seconds: Option<u64>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct ProcessStep {
    pub argv: Vec<String>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub env: BTreeMap<String, String>,
}

/// `{plugin, action, params}` of a plugin step: what reopens it as its form (a `false` that the
/// command line can't show included) and names the plugin when it is not installed.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct PluginAction {
    pub plugin: String,
    pub action: String,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub params: BTreeMap<String, Value>,
    /// Required params still empty: the step has no command line and the batch can't run.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub missing: Vec<String>,
}

/// An agent step (#60): a kit skill run headless against the item's project, then reviewed
/// (`bashcut.review.run`); blocking findings go back to the agent for at most `fixRounds` rounds,
/// after which the item ends **needs fix**.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentStep {
    pub skill: String,
    #[serde(default)]
    pub brief: String,
    /// `claude` (Claude Code, default) or `codex`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent: Option<String>,
    /// A command line of your own instead of the adapter's: `{prompt}`, `{session}` (empty on the
    /// first round) and `{mcpConfig}` besides the item placeholders; it should print Claude Code
    /// stream-json or Codex `--json` events to read usage from.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub argv: Vec<String>,
    /// Turns per round (Claude Code `--max-turns`; default 120).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_turns: Option<u32>,
    /// Wall time for the whole step, every round included (default 30).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_minutes: Option<f64>,
    /// Review after the agent (default true).
    #[serde(default = "yes")]
    pub review: bool,
    /// The output profile the review measures against (`bashcut.review.run {profile}`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub review_profile: Option<String>,
    /// Rounds that send blocking findings back to the agent (default 2, at most 5).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fix_rounds: Option<u32>,
    /// Give the agent the kit (`--kit`, `$BASHCUT_KIT`): Claude Code gets it as `--plugin-dir` and may
    /// run its scripts ([`crate::agent::KIT_TOOLS`]); every agent is told where it is (default true, #204).
    #[serde(default = "yes")]
    pub kit: bool,
    /// More tools the agent may use (Claude Code `--allowedTools` entries such as `"Bash(ffmpeg:*)"`).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub allowed_tools: Vec<String>,
}

/// Per round. Measured (bashcut/bench/2026-10-20-rescore.md): one-skill jobs take 39-115 turns, a tutorial
/// (cut, cards, zooms, captions) about 150 over two rounds; 40 stopped most of them half way (#219).
pub const DEFAULT_MAX_TURNS: u32 = 120;
pub const DEFAULT_MAX_MINUTES: f64 = 30.0;
pub const DEFAULT_FIX_ROUNDS: u32 = 2;
pub const MAX_FIX_ROUNDS: u32 = 5;
/// Agents the runner knows how to start and read.
pub const AGENTS: &[&str] = &["claude", "codex"];

impl AgentStep {
    fn check(&self) -> Result<(), String> {
        if self.agent.as_deref().is_some_and(|a| !AGENTS.contains(&a)) {
            return Err(format!("agent must be one of {}", AGENTS.join(", ")));
        }
        if self.max_turns.is_some_and(|t| t == 0 || t > 1000) {
            return Err("maxTurns must be 1 to 1000".into());
        }
        if self.max_minutes.is_some_and(|m| !m.is_finite() || m <= 0.0 || m > 24.0 * 60.0) {
            return Err("maxMinutes must be above 0 and at most 1440".into());
        }
        if self.fix_rounds.is_some_and(|r| r > MAX_FIX_ROUNDS) {
            return Err(format!("fixRounds must be at most {MAX_FIX_ROUNDS}"));
        }
        Ok(())
    }

    /// The adapter: `claude` unless set.
    pub fn agent_name(&self) -> &str {
        self.agent.as_deref().unwrap_or("claude")
    }

    pub fn max_turns(&self) -> u32 {
        self.max_turns.unwrap_or(DEFAULT_MAX_TURNS)
    }

    /// The step's whole wall time.
    pub fn max_duration(&self) -> std::time::Duration {
        std::time::Duration::from_secs_f64(self.max_minutes.unwrap_or(DEFAULT_MAX_MINUTES).clamp(0.001, 24.0 * 60.0) * 60.0)
    }

    pub fn fix_rounds(&self) -> u32 {
        self.fix_rounds.unwrap_or(DEFAULT_FIX_ROUNDS).min(MAX_FIX_ROUNDS)
    }
}

/// What a step does, borrowed from a validated [`Step`].
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum StepKind<'a> {
    Command(&'a str, Option<&'a Value>),
    Commands(&'a [CommandLine]),
    Plugin(&'a ProcessStep),
    Agent(&'a AgentStep),
}

impl Step {
    pub fn kind(&self) -> Result<StepKind<'_>, String> {
        let mut kinds = Vec::new();
        if let Some(id) = &self.command {
            kinds.push(StepKind::Command(id, self.params.as_ref()));
        }
        if !self.commands.is_empty() {
            kinds.push(StepKind::Commands(&self.commands));
        }
        if let Some(p) = &self.plugin {
            kinds.push(StepKind::Plugin(p));
        }
        if let Some(a) = &self.agent {
            kinds.push(StepKind::Agent(a));
        }
        match kinds.as_slice() {
            [k] => Ok(*k),
            [] => Err("a step needs one of command, commands, plugin, agent".into()),
            _ => Err("a step has more than one of command, commands, plugin, agent".into()),
        }
    }

    /// The required params of a plugin step still empty, when there are any.
    pub fn missing(&self) -> Option<&[String]> {
        self.plugin_action.as_ref().map(|a| a.missing.as_slice()).filter(|m| !m.is_empty())
    }

    /// A short label for the job book.
    pub fn describe(&self) -> String {
        if let Some(l) = self.label.as_ref().filter(|l| !l.trim().is_empty()) {
            return l.clone();
        }
        match self.kind() {
            Ok(StepKind::Command(id, _)) => id.to_string(),
            Ok(StepKind::Commands(c)) => format!("{} commands", c.len()),
            Ok(StepKind::Plugin(p)) => format!("plugin {}", p.argv.first().map(String::as_str).unwrap_or("?")),
            Ok(StepKind::Agent(a)) => format!("agent: {}", a.skill),
            Err(_) => "invalid step".into(),
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Output {
    /// File name in the outputs folder, with placeholders (`{name}-tiktok.mp4`).
    pub name: String,
    /// An export preset (`filmcraft-cli export --list-presets`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub preset: Option<String>,
    /// A format instead of (or over) the preset; guessed from the extension when both are missing.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub format: Option<String>,
    /// ExportSettings JSON merged over the preset.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub settings: Option<Value>,
    /// An output profile (#43: `tiktok`, `reels`, `shorts`, `youtube`, …): exported with
    /// `bashcut.profiles.apply`, which also applies the profile's loudness and caption rules.
    /// Instead of `preset` / `format` / `settings`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub profile: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timeout_seconds: Option<u64>,
}

/// The commands that make a new project from a media input when the pipeline has no template:
/// import the file, then a sequence from it (matching its size and rate). Headless there is no
/// Project panel selection, so the clip is named: the first item imported into a new project is 1.
pub fn default_create_commands() -> Vec<CommandLine> {
    vec![
        CommandLine { id: "file.import".into(), params: serde_json::json!({"paths": ["{media}"]}) },
        CommandLine { id: "file.newSequenceFromClip".into(), params: serde_json::json!({"items": [1]}) },
    ]
}

/// One expanded input.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Input {
    pub path: PathBuf,
    pub is_project: bool,
}

impl Pipeline {
    /// Read and check a pipeline file; relative paths are made absolute against its folder.
    pub fn load(path: &Path) -> Result<Pipeline, String> {
        let text = read_capped(path)?;
        let base = path.parent().map(Path::to_path_buf).unwrap_or_default();
        Pipeline::parse(&text, &absolute(&base))
    }

    /// Parse and check pipeline JSON; relative paths are resolved against `base`.
    pub fn parse(text: &str, base: &Path) -> Result<Pipeline, String> {
        let mut p: Pipeline = serde_json::from_str(text).map_err(|e| format!("pipeline: {e}"))?;
        p.validate()?;
        p.resolve(base);
        Ok(p)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != SCHEMA {
            return Err(format!("pipeline: schema must be \"{SCHEMA}\" (got \"{}\")", self.schema));
        }
        if self.inputs.is_empty() || self.inputs.len() > MAX_INPUTS {
            return Err(format!("pipeline: inputs must list 1 to {MAX_INPUTS} entries"));
        }
        if self.projects_folder.trim().is_empty() {
            return Err("pipeline: projectsFolder is required".into());
        }
        if self.steps.len() > MAX_STEPS || self.outputs.len() > MAX_OUTPUTS {
            return Err(format!("pipeline: at most {MAX_STEPS} steps and {MAX_OUTPUTS} outputs"));
        }
        if self.concurrency == 0 || self.concurrency > MAX_CONCURRENCY || self.agent_concurrency == 0 || self.agent_concurrency > MAX_CONCURRENCY {
            return Err(format!("pipeline: concurrency and agentConcurrency must be 1 to {MAX_CONCURRENCY}"));
        }
        for (i, s) in self.steps.iter().enumerate() {
            let kind = s.kind().map_err(|e| format!("pipeline: step {}: {e}", i + 1))?;
            if let Some(missing) = s.missing() {
                return Err(format!("pipeline: step {}: fill in {}", i + 1, missing.join(", ")));
            }
            let bad = match kind {
                StepKind::Command(id, _) => id.trim().is_empty(),
                StepKind::Commands(c) => c.iter().any(|c| c.id.trim().is_empty()),
                StepKind::Plugin(p) => p.argv.first().is_none_or(|a| a.trim().is_empty()),
                StepKind::Agent(a) => a.skill.trim().is_empty() || a.argv.first().is_some_and(|a| a.trim().is_empty()),
            };
            if bad {
                return Err(format!("pipeline: step {}: empty command id, program or skill", i + 1));
            }
            if let StepKind::Agent(a) = kind {
                a.check().map_err(|e| format!("pipeline: step {}: {e}", i + 1))?;
            }
        }
        self.resources.validate()?;
        if self.template.as_ref().is_some_and(|t| t.project.is_some() && !t.commands.is_empty()) {
            return Err("pipeline: template has both project and commands".into());
        }
        for (i, o) in self.outputs.iter().enumerate() {
            if o.name.trim().is_empty() {
                return Err(format!("pipeline: output {}: name is required", i + 1));
            }
            if o.profile.as_ref().is_some_and(|p| p.trim().is_empty()) {
                return Err(format!("pipeline: output {}: empty profile", i + 1));
            }
            if o.profile.is_some() && (o.preset.is_some() || o.format.is_some() || o.settings.is_some()) {
                return Err(format!("pipeline: output {}: a profile replaces preset, format and settings", i + 1));
            }
        }
        Ok(())
    }

    fn resolve(&mut self, base: &Path) {
        let abs = |s: &str| {
            let p = Path::new(s);
            if p.is_absolute() { s.to_string() } else { base.join(p).to_string_lossy().into_owned() }
        };
        self.inputs = self.inputs.iter().map(String::as_str).map(abs).collect();
        self.projects_folder = abs(&self.projects_folder);
        self.outputs_folder = self.outputs_folder.as_deref().map(abs);
        if let Some(t) = self.template.as_mut() {
            t.project = t.project.as_deref().map(abs);
        }
    }

    pub fn projects_dir(&self) -> PathBuf {
        PathBuf::from(&self.projects_folder)
    }

    pub fn outputs_dir(&self) -> PathBuf {
        self.outputs_folder.as_ref().map(PathBuf::from).unwrap_or_else(|| self.projects_dir().join("exports"))
    }

    /// Every input file: folders expand to their media files (sorted), duplicates drop out.
    pub fn expand_inputs(&self) -> Result<Vec<Input>, String> {
        let exts: Vec<String> = if self.extensions.is_empty() {
            MEDIA_EXTENSIONS.iter().map(|e| e.to_string()).collect()
        } else {
            self.extensions.iter().map(|e| e.trim_start_matches('.').to_ascii_lowercase()).collect()
        };
        let mut out: Vec<Input> = Vec::new();
        let mut seen: HashSet<PathBuf> = HashSet::new();
        let mut push = |path: PathBuf, out: &mut Vec<Input>| {
            if seen.insert(path.clone()) {
                let is_project = extension(&path).as_deref() == Some(PROJECT_EXTENSION);
                out.push(Input { path, is_project });
            }
        };
        for entry in &self.inputs {
            let path = PathBuf::from(entry);
            if path.is_dir() {
                let rd = std::fs::read_dir(&path).map_err(|e| format!("input {entry}: {e}"))?;
                let mut files: Vec<PathBuf> =
                    rd.filter_map(|e| e.ok().map(|e| e.path())).filter(|p| p.is_file() && extension(p).is_some_and(|x| exts.contains(&x))).collect();
                files.sort();
                for f in files {
                    push(f, &mut out);
                }
            } else if path.is_file() {
                push(path, &mut out);
            } else {
                return Err(format!("input not found: {entry}"));
            }
            if out.len() > MAX_INPUTS {
                return Err(format!("more than {MAX_INPUTS} inputs"));
            }
        }
        if out.is_empty() {
            return Err("no input files (folders hold no files with the listed extensions)".into());
        }
        Ok(out)
    }

    /// Item names for `inputs`, in order: `outputName` filled in, made safe for a file name and
    /// unique (`clip`, `clip-2`, …, compared without case).
    pub fn item_names(&self, inputs: &[Input]) -> Vec<String> {
        let pattern = self.output_name.as_deref().filter(|p| !p.trim().is_empty()).unwrap_or("{stem}");
        let mut names: Vec<String> = Vec::with_capacity(inputs.len());
        // lowercased names taken so far: a linear scan per candidate is O(n³) when stems repeat
        let mut taken: HashSet<String> = HashSet::with_capacity(inputs.len());
        for (i, input) in inputs.iter().enumerate() {
            let stem = input.path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
            let raw = pattern.replace("{stem}", &stem).replace("{index}", &format!("{:03}", i.saturating_add(1)));
            let base = safe_name(&raw);
            let mut name = base.clone();
            let mut n: usize = 2;
            while taken.contains(&name.to_ascii_lowercase()) {
                name = format!("{base}-{n}");
                n = n.saturating_add(1);
            }
            taken.insert(name.to_ascii_lowercase());
            names.push(name);
        }
        names
    }
}

/// Placeholder values for one item.
#[derive(Clone, Debug, Default)]
pub struct Vars(pub Vec<(&'static str, String)>);

impl Vars {
    /// The value of `key`, when set and not empty.
    pub fn get(&self, key: &str) -> Option<&str> {
        self.0.iter().find(|(k, _)| *k == key).map(|(_, v)| v.as_str()).filter(|v| !v.is_empty())
    }

    /// `s` with every `{key}` replaced; other braces are left alone.
    pub fn expand(&self, s: &str) -> String {
        let mut out = s.to_string();
        for (k, v) in &self.0 {
            let key = format!("{{{k}}}");
            if out.contains(&key) {
                out = out.replace(&key, v);
            }
        }
        out
    }

    /// Every string in `v` expanded (keys too), to a bounded depth.
    pub fn expand_value(&self, v: &Value) -> Value {
        self.expand_depth(v, 0)
    }

    fn expand_depth(&self, v: &Value, depth: usize) -> Value {
        if depth > MAX_DEPTH {
            return v.clone();
        }
        match v {
            Value::String(s) => Value::String(self.expand(s)),
            Value::Array(a) => Value::Array(a.iter().map(|x| self.expand_depth(x, depth + 1)).collect()),
            Value::Object(m) => Value::Object(m.iter().map(|(k, x)| (self.expand(k), self.expand_depth(x, depth + 1))).collect()),
            other => other.clone(),
        }
    }
}

/// A name safe in a file name on every OS: letters, digits, space, `-`, `_`, `.`; at most 120 chars.
pub fn safe_name(raw: &str) -> String {
    let s: String = raw.chars().map(|c| if c.is_alphanumeric() || matches!(c, ' ' | '-' | '_' | '.') { c } else { '_' }).take(120).collect();
    let s = s.trim().trim_matches('.').to_string();
    if s.is_empty() { "item".into() } else { s }
}

fn extension(p: &Path) -> Option<String> {
    p.extension().map(|e| e.to_string_lossy().to_ascii_lowercase())
}

/// `p` made absolute against the working folder (not canonicalized: it may not exist yet).
pub fn absolute(p: &Path) -> PathBuf {
    if p.is_absolute() {
        return p.to_path_buf();
    }
    match std::env::current_dir() {
        Ok(cwd) => cwd.join(p),
        Err(_) => p.to_path_buf(),
    }
}

/// A small text file (a pipeline, a state file), refused over 4 MB.
pub fn read_capped(path: &Path) -> Result<String, String> {
    let meta = std::fs::metadata(path).map_err(|e| format!("{}: {e}", path.display()))?;
    if meta.len() > MAX_FILE_BYTES {
        return Err(format!("{}: larger than {MAX_FILE_BYTES} bytes", path.display()));
    }
    std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn minimal(extra: Value) -> String {
        let mut v = json!({"schema": SCHEMA, "inputs": ["a.mp4"], "projectsFolder": "out"});
        if let (Some(m), Value::Object(e)) = (v.as_object_mut(), extra) {
            m.extend(e);
        }
        v.to_string()
    }

    #[test]
    fn parses_and_resolves_paths() {
        let p = Pipeline::parse(&minimal(json!({"steps": [{"command": "captions.auto"}]})), Path::new("/base")).unwrap();
        assert_eq!(p.inputs, vec![Path::new("/base").join("a.mp4").to_string_lossy().into_owned()]);
        assert_eq!(p.projects_dir(), Path::new("/base").join("out"));
        assert_eq!(p.outputs_dir(), Path::new("/base").join("out").join("exports"));
        assert_eq!(p.concurrency, 1);
        assert_eq!(p.on_failure, OnFailure::Skip);
        assert_eq!(p.steps[0].describe(), "captions.auto");
        let p = Pipeline::parse(&minimal(json!({"description": "d", "outputs": [{"name": "{name}.mp4", "profile": "tiktok"}]})), Path::new("/")).unwrap();
        assert_eq!((p.description.as_str(), p.outputs[0].profile.as_deref()), ("d", Some("tiktok")));
        let p = Pipeline::parse(&minimal(json!({"steps": [{"agent": {"skill": "rough-cut", "maxMinutes": 0.5}}]})), Path::new("/")).unwrap();
        let a = p.steps[0].agent.as_ref().unwrap();
        assert_eq!((a.agent_name(), a.max_turns(), a.fix_rounds(), a.review), ("claude", DEFAULT_MAX_TURNS, DEFAULT_FIX_ROUNDS, true));
        assert_eq!(a.max_duration(), std::time::Duration::from_secs(30));
        assert_eq!(p.agent_concurrency, 1);
    }

    #[test]
    fn rejects_bad_pipelines() {
        let bad = [
            json!({"schema": "nope"}),
            json!({"inputs": []}),
            json!({"concurrency": 0}),
            json!({"steps": [{}]}),
            json!({"steps": [{"command": "a", "plugin": {"argv": ["x"]}}]}),
            json!({"steps": [{"plugin": {"argv": []}}]}),
            json!({"outputs": [{"name": "", "preset": "x"}]}),
            json!({"outputs": [{"name": "a.mp4", "profile": "tiktok", "preset": "x"}]}),
            json!({"outputs": [{"name": "a.mp4", "profile": " "}]}),
            json!({"template": {"project": "t.fcproj", "commands": [{"id": "x"}]}}),
            json!({"agentConcurrency": 0}),
            json!({"steps": [{"agent": {"skill": "rough-cut", "agent": "gpt"}}]}),
            json!({"steps": [{"agent": {"skill": "rough-cut", "maxTurns": 0}}]}),
            json!({"steps": [{"agent": {"skill": "rough-cut", "maxMinutes": -1}}]}),
            json!({"steps": [{"agent": {"skill": "rough-cut", "fixRounds": 9}}]}),
        ];
        for b in bad {
            assert!(Pipeline::parse(&minimal(b.clone()), Path::new("/")).is_err(), "{b}");
        }
        assert!(Pipeline::parse("not json", Path::new("/")).is_err());
    }

    #[test]
    fn placeholders_expand_in_strings_and_nested_values() {
        let vars = Vars(vec![("name", "clip".into()), ("media", "/m/clip.mp4".into())]);
        assert_eq!(vars.expand("{name}-yt.mp4"), "clip-yt.mp4");
        assert_eq!(vars.expand("{unknown} {name}"), "{unknown} clip");
        let v = vars.expand_value(&json!({"paths": ["{media}"], "n": 3, "o": {"{name}": "{name}"}}));
        assert_eq!(v, json!({"paths": ["/m/clip.mp4"], "n": 3, "o": {"clip": "clip"}}));
    }

    #[test]
    fn item_names_are_safe_and_unique() {
        let p = Pipeline::parse(&minimal(json!({"outputName": "{index}_{stem}"})), Path::new("/")).unwrap();
        let inputs: Vec<Input> = ["/x/a b.mp4", "/y/a b.mp4", "/z/c:d?.mov"].iter().map(|s| Input { path: PathBuf::from(s), is_project: false }).collect();
        assert_eq!(p.item_names(&inputs), vec!["001_a b", "002_a b", "003_c_d_"]);
        let p = Pipeline::parse(&minimal(json!({})), Path::new("/")).unwrap();
        assert_eq!(p.item_names(&inputs), vec!["a b", "a b-2", "c_d_"]);
        assert_eq!(safe_name("../.."), "_");
        assert_eq!(safe_name(""), "item");
    }

    #[test]
    fn folders_expand_to_media_files() {
        let dir = std::env::temp_dir().join(format!("bashcut-batch-pipeline-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        for f in ["b.MP4", "a.mov", "notes.txt", "p.fcproj"] {
            std::fs::write(dir.join(f), b"x").unwrap();
        }
        let text = json!({"schema": SCHEMA, "inputs": [dir.to_string_lossy(), dir.join("p.fcproj").to_string_lossy()], "projectsFolder": "out"}).to_string();
        let p = Pipeline::parse(&text, Path::new("/")).unwrap();
        let inputs = p.expand_inputs().unwrap();
        let names: Vec<_> = inputs.iter().map(|i| i.path.file_name().unwrap().to_string_lossy().into_owned()).collect();
        assert_eq!(names, vec!["a.mov", "b.MP4", "p.fcproj"]);
        assert_eq!(inputs.iter().map(|i| i.is_project).collect::<Vec<_>>(), vec![false, false, true]);
        let missing = json!({"schema": SCHEMA, "inputs": [dir.join("nope.mp4").to_string_lossy()], "projectsFolder": "out"}).to_string();
        assert!(Pipeline::parse(&missing, Path::new("/")).unwrap().expand_inputs().is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// BT1: jobs and presets saved before `pluginAction` still load; a plugin step keeps its
    /// action and params (a `false` too), and one with empty required params can't run.
    #[test]
    fn plugin_steps_keep_their_action_and_old_ones_still_load() {
        let old = minimal(json!({"steps": [{"label": "Captions", "plugin": {"argv": ["python3", "p.py", "translate", "--burn"]}}]}));
        let p = Pipeline::parse(&old, Path::new("/")).unwrap();
        assert_eq!(p.steps[0].plugin_action, None);
        assert_eq!(p.steps[0].plugin.as_ref().map(|s| s.argv.len()), Some(4));

        let step = Step {
            plugin: Some(ProcessStep { argv: vec!["python3".into(), "p.py".into()], ..ProcessStep::default() }),
            plugin_action: Some(PluginAction {
                plugin: "test.cap".into(),
                action: "translate".into(),
                params: BTreeMap::from([("burn".to_string(), json!(false)), ("target".to_string(), json!("en"))]),
                missing: Vec::new(),
            }),
            ..Step::default()
        };
        let text = serde_json::to_string(&step).unwrap();
        assert!(text.contains(r#""pluginAction":{"plugin":"test.cap","action":"translate","params":{"burn":false,"target":"en"}}"#), "{text}");
        let back: Step = serde_json::from_str(&text).unwrap();
        assert_eq!(back, step);
        assert_eq!(back.plugin_action.unwrap().params["burn"], json!(false), "false survives save and load");

        let incomplete = minimal(json!({"steps": [{"plugin": {"argv": []}, "pluginAction": {"plugin": "test.cap", "action": "go", "missing": ["answer"]}}]}));
        let e = Pipeline::parse(&incomplete, Path::new("/")).unwrap_err();
        assert_eq!(e, "pipeline: step 1: fill in answer");
    }
}
