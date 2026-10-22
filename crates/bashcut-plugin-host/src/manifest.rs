//! `bashcut-plugin.json` (`bashcut-plugin/1`, format: `bashcutpro-plugins/README.md`).

use std::collections::BTreeMap;
use std::path::Path;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{Error, Result, err, valid_id};

pub const SCHEMA: &str = "bashcut-plugin/1";
pub const RESULT_SCHEMA: &str = "bashcut-result/1";
pub const FILE: &str = "bashcut-plugin.json";
/// Largest manifest read.
pub const MAX_BYTES: u64 = 256 * 1024;
const MAX_ITEMS: usize = 256;
const MAX_ARGS: usize = 64;
const MAX_TEXT: usize = 4096;

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Manifest {
    pub schema: String,
    pub id: String,
    pub name: String,
    pub summary: String,
    pub version: String,
    pub publisher: String,
    pub license: String,
    pub homepage: String,
    pub category: String,
    /// `tool` (the default: actions) or `agent` (also a conversational agent, see [`AgentSection`]).
    pub kind: String,
    pub platforms: Vec<String>,
    pub runtime: Runtime,
    pub entry: Entry,
    pub connect: Connect,
    pub output: String,
    pub app: AppNeeds,
    pub settings: Vec<Setting>,
    pub actions: Vec<Action>,
    pub skills: Vec<String>,
    /// How the Agent section starts it, when `kind` is `agent`.
    pub agent: Option<AgentSection>,
    /// `name` / `summary` in other languages (see [`I18n`]).
    pub i18n: I18n,
}

/// Translations of the text a person sees, by language code (`{"vi": {"title": …, "help": …}}`;
/// on the plugin `name` / `summary`; on an enum setting or param `choices`, a map from choice value
/// to its label). Optional: an empty or missing entry falls back to English.
pub type I18n = BTreeMap<String, Text>;

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Text {
    #[serde(skip_serializing_if = "String::is_empty")]
    pub name: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub summary: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub title: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub help: String,
    /// Labels of enum choices by choice value (`{"gemini": "Gemini (Google)"}`); the value is what runs.
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub choices: BTreeMap<String, String>,
}

/// `english` in `lang` when the manifest has a non-empty translation, else `english`.
fn local<'a>(i18n: &'a I18n, lang: &str, english: &'a str, pick: fn(&Text) -> &str) -> &'a str {
    match i18n.get(lang).map(pick) {
        Some(x) if !x.trim().is_empty() => x,
        _ => english,
    }
}

/// The label of choice `value` in `lang` when the manifest has a non-empty one, else the value itself.
pub fn choice_label<'a>(i18n: &'a I18n, lang: &str, value: &'a str) -> &'a str {
    match i18n.get(lang).and_then(|t| t.choices.get(value)) {
        Some(x) if !x.trim().is_empty() => x,
        _ => value,
    }
}

/// An agent plugin (`bashcut/in-app-ui.md` §6): the entry plus `args` runs one turn, reading one
/// stream-json user message on stdin and printing `events` on stdout.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct AgentSection {
    pub args: Vec<String>,
    /// Arguments passing the MCP config file (`{config}`); empty: the agent does not use the app.
    pub mcp_config: Vec<String>,
    /// Arguments resuming a session (`{session}`); empty: every turn starts fresh.
    pub resume: Vec<String>,
    /// stdout event format; only `claude-stream-json` for now.
    pub events: String,
}

pub const AGENT_EVENTS: &str = "claude-stream-json";

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Runtime {
    #[serde(rename = "type")]
    pub kind: String,
    pub version: String,
    pub packages: Vec<String>,
}

/// The program to run, relative to the plugin folder; an OS key replaces `command` there.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Entry {
    pub command: Vec<String>,
    pub macos: Option<Vec<String>>,
    pub windows: Option<Vec<String>>,
    pub linux: Option<Vec<String>>,
}

/// Argument templates the host adds: `{project}`, `{address}`, `{cli}`.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Connect {
    pub project: Vec<String>,
    pub bridge: Vec<String>,
    pub cli: Vec<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct AppNeeds {
    pub min_version: String,
    pub connections: Vec<String>,
    pub tools: Vec<String>,
    pub commands: Vec<String>,
    pub effects: Vec<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Setting {
    pub id: String,
    pub title: String,
    /// string, secret, enum, integer, number, bool, path
    #[serde(rename = "type")]
    pub kind: String,
    pub default: Option<Value>,
    pub choices: Option<Vec<String>>,
    pub env: Vec<String>,
    pub flag: Option<String>,
    pub optional: bool,
    pub help: String,
    pub i18n: I18n,
}

impl Setting {
    pub fn is_secret(&self) -> bool {
        self.kind == "secret"
    }

    pub fn title_in(&self, lang: &str) -> &str {
        local(&self.i18n, lang, &self.title, |x| &x.title)
    }

    pub fn help_in(&self, lang: &str) -> &str {
        local(&self.i18n, lang, &self.help, |x| &x.help)
    }

    /// The label of choice `value` in `lang`, else the value.
    pub fn choice_in<'a>(&'a self, lang: &str, value: &'a str) -> &'a str {
        choice_label(&self.i18n, lang, value)
    }
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Action {
    pub id: String,
    pub title: String,
    pub args: Vec<String>,
    /// `project` or `none`
    pub needs: String,
    pub edits: bool,
    pub params: Vec<Param>,
    pub handoff: Option<Handoff>,
    /// The action shows a `bashcut-view/1` form in the plugin's card while it runs (see [`crate::view`]).
    pub view: bool,
    pub i18n: I18n,
}

impl Action {
    pub fn title_in(&self, lang: &str) -> &str {
        local(&self.i18n, lang, &self.title, |x| &x.title)
    }
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Param {
    pub id: String,
    pub flag: String,
    /// string, enum, integer, number, bool, path
    #[serde(rename = "type")]
    pub kind: String,
    pub default: Option<Value>,
    pub choices: Option<Vec<String>>,
    pub optional: bool,
    pub minimum: Option<f64>,
    pub maximum: Option<f64>,
    pub title: Option<String>,
    pub i18n: I18n,
}

impl Param {
    /// The label in `lang`, else the English `title`; `None` when the manifest has neither.
    pub fn title_in(&self, lang: &str) -> Option<&str> {
        let english = self.title.as_deref().unwrap_or_default();
        Some(local(&self.i18n, lang, english, |x| &x.title)).filter(|x| !x.trim().is_empty())
    }

    /// The label of choice `value` in `lang`, else the value.
    pub fn choice_in<'a>(&'a self, lang: &str, value: &'a str) -> &'a str {
        choice_label(&self.i18n, lang, value)
    }
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Handoff {
    pub exit: i32,
    pub then: String,
}

/// How a run reaches the app.
#[derive(Clone, Debug, PartialEq)]
pub enum Connection {
    /// The open app's control channel, `host:port`.
    Bridge(String),
    /// A project file, headless.
    Project(String),
    /// Neither (actions with `needs: "none"`).
    None,
}

const KINDS: &[&str] = &["string", "secret", "enum", "integer", "number", "bool", "path"];

impl Manifest {
    pub fn parse(text: &str) -> Result<Manifest> {
        if text.len() as u64 > MAX_BYTES {
            return err(format!("manifest larger than {MAX_BYTES} bytes"));
        }
        let m: Manifest = serde_json::from_str(text).map_err(|e| Error(format!("manifest: {e}")))?;
        m.check()?;
        Ok(m)
    }

    /// `<dir>/bashcut-plugin.json`.
    pub fn load(dir: &Path) -> Result<Manifest> {
        Self::parse(&crate::read_capped(&dir.join(FILE), MAX_BYTES)?)
    }

    fn check(&self) -> Result<()> {
        if self.schema != SCHEMA {
            return err(format!("manifest schema is `{}`, expected `{SCHEMA}`", self.schema));
        }
        if !valid_id(&self.id) {
            return err(format!("bad plugin id `{}`", self.id));
        }
        if !valid_version(&self.version) {
            return err(format!("{}: bad version `{}`", self.id, self.version));
        }
        if self.entry.command.is_empty() || self.entry.command.len() > MAX_ARGS {
            return err(format!("{}: entry.command is empty or too long", self.id));
        }
        if !self.output.is_empty() && self.output != RESULT_SCHEMA {
            return err(format!("{}: output `{}` is not {RESULT_SCHEMA}", self.id, self.output));
        }
        if self.settings.len() > MAX_ITEMS || self.actions.len() > MAX_ITEMS {
            return err(format!("{}: too many settings or actions", self.id));
        }
        let mut seen = Vec::new();
        for s in &self.settings {
            if !valid_id(&s.id.to_ascii_lowercase()) || seen.contains(&&s.id) {
                return err(format!("{}: bad or repeated setting id `{}`", self.id, s.id));
            }
            if !KINDS.contains(&s.kind.as_str()) {
                return err(format!("{}: setting `{}` has unknown type `{}`", self.id, s.id, s.kind));
            }
            if s.is_secret() && s.default.is_some() {
                return err(format!("{}: secret `{}` must not have a default", self.id, s.id));
            }
            if s.env.iter().any(|e| !valid_env(e)) {
                return err(format!("{}: setting `{}` has a bad env name", self.id, s.id));
            }
            seen.push(&s.id);
        }
        let mut seen = Vec::new();
        for a in &self.actions {
            if !valid_id(&a.id.to_ascii_lowercase()) || seen.contains(&&a.id) {
                return err(format!("{}: bad or repeated action id `{}`", self.id, a.id));
            }
            if a.args.len() > MAX_ARGS || a.params.len() > MAX_ITEMS {
                return err(format!("{}: action `{}` is too long", self.id, a.id));
            }
            for p in &a.params {
                if p.id.is_empty() || !p.flag.starts_with('-') || !KINDS.contains(&p.kind.as_str()) || p.kind == "secret" {
                    return err(format!("{}: action `{}` has a bad param `{}`", self.id, a.id, p.id));
                }
            }
            seen.push(&a.id);
        }
        match (self.kind.as_str(), &self.agent) {
            ("" | "tool", None) => {}
            ("agent", Some(a)) => {
                if a.args.len() > MAX_ARGS || a.mcp_config.len() > MAX_ARGS || a.resume.len() > MAX_ARGS {
                    return err(format!("{}: agent args are too long", self.id));
                }
                if !a.events.is_empty() && a.events != AGENT_EVENTS {
                    return err(format!("{}: agent events `{}` are not {AGENT_EVENTS}", self.id, a.events));
                }
                if !a.mcp_config.iter().any(|x| x.contains("{config}")) && !a.mcp_config.is_empty() {
                    return err(format!("{}: agent.mcpConfig must name {{config}}", self.id));
                }
                if !a.resume.iter().any(|x| x.contains("{session}")) && !a.resume.is_empty() {
                    return err(format!("{}: agent.resume must name {{session}}", self.id));
                }
            }
            ("agent", None) => return err(format!("{}: kind `agent` needs an `agent` section", self.id)),
            (k, _) => return err(format!("{}: bad kind `{k}` (tool or agent; `agent` needs kind `agent`)", self.id)),
        }
        if [&self.name, &self.summary, &self.homepage].iter().any(|t| t.len() > MAX_TEXT) {
            return err(format!("{}: text field too long", self.id));
        }
        Ok(())
    }

    pub fn name_in(&self, lang: &str) -> &str {
        local(&self.i18n, lang, &self.name, |x| &x.name)
    }

    pub fn summary_in(&self, lang: &str) -> &str {
        local(&self.i18n, lang, &self.summary, |x| &x.summary)
    }

    pub fn is_agent(&self) -> bool {
        self.kind == "agent" && self.agent.is_some()
    }

    pub fn action(&self, id: &str) -> Option<&Action> {
        self.actions.iter().find(|a| a.id == id)
    }

    pub fn setting(&self, id: &str) -> Option<&Setting> {
        self.settings.iter().find(|s| s.id == id)
    }

    /// Runs on this OS (an empty list means everywhere).
    pub fn supports(&self, os: &str) -> bool {
        self.platforms.is_empty() || self.platforms.iter().any(|p| p == os)
    }

    /// The entry argv for `os` (its override, else `command`).
    pub fn entry_for(&self, os: &str) -> &[String] {
        let over = match os {
            "macos" => self.entry.macos.as_ref(),
            "windows" => self.entry.windows.as_ref(),
            "linux" => self.entry.linux.as_ref(),
            _ => None,
        };
        over.filter(|v| !v.is_empty()).unwrap_or(&self.entry.command)
    }

    /// Engine commands the plugin needs that `known` (the running build's command ids) lacks.
    pub fn missing_commands(&self, known: &[String]) -> Vec<String> {
        self.app.commands.iter().filter(|c| !known.iter().any(|k| k == *c)).cloned().collect()
    }

    /// The full argument list for one action: entry, the action's args, connection args, an
    /// optional `--cli` and the params as flags (checked against the manifest). Params left out
    /// keep the plugin's own default.
    pub fn command_line(&self, os: &str, action: &str, conn: &Connection, cli: Option<&str>, params: &BTreeMap<String, Value>) -> Result<Vec<String>> {
        let Some(a) = self.action(action) else { return err(format!("{} has no action `{action}`", self.id)) };
        let mut argv: Vec<String> = self.entry_for(os).to_vec();
        argv.extend(a.args.iter().cloned());
        // an action that needs nothing gets no connection arguments (its parser may have none)
        let none = a.needs == "none";
        match conn {
            _ if none => {}
            Connection::Bridge(addr) if !self.connect.bridge.is_empty() => argv.extend(fill(&self.connect.bridge, "{address}", addr)),
            Connection::Project(path) if !self.connect.project.is_empty() => argv.extend(fill(&self.connect.project, "{project}", path)),
            Connection::None if a.needs == "project" => return err(format!("`{}` needs the open project: start BashCutPro with --control <port>", a.title)),
            Connection::None => {}
            _ if a.needs != "project" => {}
            _ => return err(format!("{} cannot connect this way", self.id)),
        }
        if let Some(cli) = cli.filter(|_| !none && !self.connect.cli.is_empty()) {
            argv.extend(fill(&self.connect.cli, "{cli}", cli));
        }
        for key in params.keys() {
            if !a.params.iter().any(|p| &p.id == key) {
                return err(format!("`{}` has no param `{key}`", a.id));
            }
        }
        for p in &a.params {
            match params.get(&p.id).filter(|v| !v.is_null()) {
                Some(v) => {
                    let text = param_text(p, v)?;
                    if p.kind == "bool" {
                        if text == "true" {
                            argv.push(p.flag.clone());
                        }
                    } else {
                        argv.push(p.flag.clone());
                        argv.push(text);
                    }
                }
                None if p.optional || p.default.is_some() => {}
                None => return err(format!("`{}` needs `{}`", a.title, p.id)),
            }
        }
        Ok(argv)
    }
}

fn fill(template: &[String], key: &str, value: &str) -> Vec<String> {
    template.iter().map(|t| t.replace(key, value)).collect()
}

/// A param value as its flag's text, checked against type, choices and range.
pub fn param_text(p: &Param, v: &Value) -> Result<String> {
    let bad = || Error(format!("param `{}`: expected {}, got {v}", p.id, p.kind));
    let text = match p.kind.as_str() {
        "bool" => v.as_bool().ok_or_else(bad)?.to_string(),
        "integer" => {
            let n = v.as_i64().or_else(|| v.as_str().and_then(|s| s.trim().parse().ok())).ok_or_else(bad)?;
            in_range(p, n as f64)?;
            n.to_string()
        }
        "number" => {
            let n = v.as_f64().or_else(|| v.as_str().and_then(|s| s.trim().parse().ok())).filter(|n: &f64| n.is_finite()).ok_or_else(bad)?;
            in_range(p, n)?;
            n.to_string()
        }
        _ => {
            let s = v.as_str().ok_or_else(bad)?;
            if s.len() > MAX_TEXT * 16 {
                return err(format!("param `{}` is too long", p.id));
            }
            s.to_string()
        }
    };
    if let Some(choices) = &p.choices
        && !choices.contains(&text)
    {
        return err(format!("param `{}` must be one of {}", p.id, choices.join(", ")));
    }
    Ok(text)
}

fn in_range(p: &Param, n: f64) -> Result<()> {
    if p.minimum.is_some_and(|m| n < m) || p.maximum.is_some_and(|m| n > m) {
        return err(format!("param `{}` = {n} is out of range", p.id));
    }
    Ok(())
}

/// `MAJOR.MINOR.PATCH`, numbers only.
pub fn valid_version(v: &str) -> bool {
    let parts: Vec<&str> = v.split('.').collect();
    parts.len() == 3 && parts.iter().all(|p| !p.is_empty() && p.len() <= 9 && p.bytes().all(|b| b.is_ascii_digit()))
}

/// Compare two `MAJOR.MINOR.PATCH` versions; unparsable ones compare as 0.0.0.
pub fn version_key(v: &str) -> (u64, u64, u64) {
    let mut it = v.split('.').map(|p| p.parse::<u64>().unwrap_or(0));
    (it.next().unwrap_or(0), it.next().unwrap_or(0), it.next().unwrap_or(0))
}

fn valid_env(name: &str) -> bool {
    !name.is_empty() && name.len() <= 128 && name.bytes().all(|b| b.is_ascii_uppercase() || b.is_ascii_digit() || b == b'_')
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    pub(crate) fn sample() -> String {
        json!({
            "schema": "bashcut-plugin/1", "id": "test.echo", "name": "Echo", "version": "1.2.3",
            "entry": {"command": ["python3", "plugin.py"], "windows": ["py", "-3", "plugin.py"]},
            "connect": {"project": ["--project", "{project}"], "bridge": ["--bridge", "{address}"], "cli": ["--cli", "{cli}"]},
            "output": "bashcut-result/1",
            "app": {"commands": ["captions.list", "file.save"]},
            "settings": [
                {"id": "service", "title": "Service", "type": "enum", "choices": ["a", "b"], "default": "a", "env": ["ECHO_SERVICE"], "flag": "--service"},
                {"id": "apiKey", "title": "API key", "type": "secret", "env": ["ECHO_KEY"], "optional": true}
            ],
            "actions": [
                {"id": "go", "title": "Go", "args": ["go"], "needs": "project", "edits": true, "params": [
                    {"id": "target", "flag": "--target", "type": "enum", "choices": ["vi", "en"], "default": "vi"},
                    {"id": "n", "flag": "--n", "type": "integer", "minimum": 1, "maximum": 5, "optional": true},
                    {"id": "dry", "flag": "--dry", "type": "bool", "optional": true},
                    {"id": "answer", "flag": "--answer", "type": "path"}
                ], "handoff": {"exit": 3, "then": "apply"}},
                {"id": "ping", "title": "Ping", "args": ["ping"], "needs": "none", "params": []}
            ]
        })
        .to_string()
    }

    #[test]
    fn parses_and_builds_the_command_line() {
        let m = Manifest::parse(&sample()).unwrap();
        assert_eq!(m.entry_for("windows")[0], "py");
        assert_eq!(m.entry_for("linux")[0], "python3");
        let params: BTreeMap<String, Value> =
            [("target".to_string(), json!("en")), ("n".to_string(), json!(2)), ("dry".to_string(), json!(true)), ("answer".to_string(), json!("/a b.json"))]
                .into();
        let argv = m.command_line("macos", "go", &Connection::Bridge("127.0.0.1:9876".into()), Some("/bin/filmcraft-cli"), &params).unwrap();
        assert_eq!(
            argv,
            [
                "python3",
                "plugin.py",
                "go",
                "--bridge",
                "127.0.0.1:9876",
                "--cli",
                "/bin/filmcraft-cli",
                "--target",
                "en",
                "--n",
                "2",
                "--dry",
                "--answer",
                "/a b.json"
            ]
        );
        assert_eq!(m.missing_commands(&["file.save".to_string()]), ["captions.list"]);
    }

    #[test]
    fn picks_the_ui_language_and_falls_back_to_english() {
        // a manifest without `i18n` (every one before I18N1) still loads and reads in English
        let old = Manifest::parse(&sample()).unwrap();
        assert!(old.i18n.is_empty());
        assert_eq!((old.name_in("vi"), old.settings[0].title_in("vi"), old.actions[0].title_in("vi")), ("Echo", "Service", "Go"));
        assert_eq!(old.actions[0].params[0].title_in("vi"), None);
        let mut v: Value = serde_json::from_str(&sample()).unwrap();
        v["i18n"] = json!({"vi": {"name": "Tiếng vọng", "summary": ""}});
        v["settings"][0]["help"] = json!("Who answers.");
        v["settings"][0]["i18n"] = json!({"vi": {"title": "Dịch vụ", "help": "Bên trả lời."}});
        v["actions"][0]["i18n"] = json!({"vi": {"title": "Chạy"}});
        v["actions"][0]["params"][0]["title"] = json!("Language");
        v["actions"][0]["params"][0]["i18n"] = json!({"vi": {"title": "Ngôn ngữ"}});
        v["actions"][0]["params"][1]["i18n"] = json!({"vi": {"title": "Số lần"}});
        let m = Manifest::parse(&v.to_string()).unwrap();
        assert_eq!((m.name_in("vi"), m.summary_in("vi"), m.name_in("en"), m.name_in("fr")), ("Tiếng vọng", "", "Echo", "Echo"));
        let s = &m.settings[0];
        assert_eq!((s.title_in("vi"), s.help_in("vi"), s.title_in("en"), s.help_in("en")), ("Dịch vụ", "Bên trả lời.", "Service", "Who answers."));
        assert_eq!((m.actions[0].title_in("vi"), m.actions[0].title_in("en")), ("Chạy", "Go"));
        let p = &m.actions[0].params;
        assert_eq!(
            (p[0].title_in("vi"), p[0].title_in("en"), p[1].title_in("vi"), p[1].title_in("en")),
            (Some("Ngôn ngữ"), Some("Language"), Some("Số lần"), None)
        );
        assert!(Manifest::parse(&json!({"i18n": {"vi": "x"}}).to_string()).is_err(), "a translation is an object");
    }

    #[test]
    fn choice_labels_follow_the_language_and_fall_back_to_the_value() {
        let old = Manifest::parse(&sample()).unwrap();
        assert_eq!((old.settings[0].choice_in("vi", "a"), old.actions[0].params[0].choice_in("vi", "vi")), ("a", "vi"));
        let mut v: Value = serde_json::from_str(&sample()).unwrap();
        v["settings"][0]["i18n"] = json!({"vi": {"title": "Dịch vụ", "choices": {"a": "Dịch vụ A", "b": " "}}});
        v["actions"][0]["params"][0]["i18n"] = json!({"vi": {"choices": {"vi": "Tiếng Việt", "en": "Tiếng Anh"}}});
        let m = Manifest::parse(&v.to_string()).unwrap();
        let s = &m.settings[0];
        assert_eq!((s.choice_in("vi", "a"), s.choice_in("vi", "b"), s.choice_in("en", "a"), s.choice_in("vi", "zz")), ("Dịch vụ A", "b", "a", "zz"));
        let p = &m.actions[0].params[0];
        assert_eq!((p.choice_in("vi", "en"), p.choice_in("en", "en"), p.title_in("vi")), ("Tiếng Anh", "en", None));
        assert!(Manifest::parse(&json!({"settings": [{"id": "s", "i18n": {"vi": {"choices": ["x"]}}}]}).to_string()).is_err(), "choices is a map");
        let back: Value = serde_json::to_value(&m.settings[0].i18n).unwrap();
        assert_eq!(back["vi"]["choices"]["a"], "Dịch vụ A");
    }

    #[test]
    fn rejects_bad_params() {
        let m = Manifest::parse(&sample()).unwrap();
        let bridge = Connection::Bridge("127.0.0.1:1".into());
        let p = |k: &str, v: Value| -> BTreeMap<String, Value> { [(k.to_string(), v), ("answer".to_string(), json!("x"))].into() };
        assert!(m.command_line("linux", "go", &bridge, None, &p("target", json!("fr"))).is_err());
        assert!(m.command_line("linux", "go", &bridge, None, &p("n", json!(9))).is_err());
        assert!(m.command_line("linux", "go", &bridge, None, &p("n", json!("x"))).is_err());
        assert!(m.command_line("linux", "go", &bridge, None, &p("nope", json!(1))).is_err());
        assert!(m.command_line("linux", "go", &bridge, None, &BTreeMap::new()).is_err(), "answer is required");
        assert!(m.command_line("linux", "go", &Connection::None, None, &p("n", json!(1))).is_err(), "needs a project");
        assert!(m.command_line("linux", "ping", &Connection::None, None, &BTreeMap::new()).is_ok());
        // an action that needs nothing gets no connection arguments, even with the app open
        let ping = m.command_line("linux", "ping", &Connection::Bridge("127.0.0.1:9876".into()), Some("/bin/filmcraft-cli"), &BTreeMap::new()).unwrap();
        assert_eq!(ping, ["python3", "plugin.py", "ping"]);
        assert!(m.command_line("linux", "nope", &bridge, None, &BTreeMap::new()).is_err());
    }

    #[test]
    fn rejects_hostile_manifests() {
        let mut v: Value = serde_json::from_str(&sample()).unwrap();
        for (path, bad) in [("id", json!("../x")), ("schema", json!("other/1")), ("version", json!("1.2")), ("entry", json!({"command": []}))] {
            let mut m = v.clone();
            m[path] = bad;
            assert!(Manifest::parse(&m.to_string()).is_err(), "{path}");
        }
        v["settings"][1]["default"] = json!("leak");
        assert!(Manifest::parse(&v.to_string()).is_err(), "secret with a default");
        assert!(Manifest::parse("[").is_err());
        assert!(Manifest::parse(&" ".repeat(MAX_BYTES as usize + 1)).is_err());
        let mut camel = v.clone();
        camel["settings"][1]["default"] = Value::Null;
        camel["actions"][1]["id"] = json!("packExport");
        assert!(Manifest::parse(&camel.to_string()).is_ok(), "camelCase action ids, as the registry checker allows");
        camel["actions"][1]["id"] = json!("../x");
        assert!(Manifest::parse(&camel.to_string()).is_err(), "a path is still no action id");
        assert_eq!(version_key("1.10.2"), (1, 10, 2));
        assert!(version_key("1.10.2") > version_key("1.9.9"));
    }

    #[test]
    fn agent_plugins_are_checked() {
        let mut v: Value = serde_json::from_str(&sample()).unwrap();
        v["kind"] = json!("agent");
        assert!(Manifest::parse(&v.to_string()).is_err(), "kind agent needs a section");
        v["agent"] = json!({"args": ["chat"], "mcpConfig": ["--mcp", "{config}"], "resume": ["--session", "{session}"], "events": "claude-stream-json"});
        let m = Manifest::parse(&v.to_string()).unwrap();
        assert!(m.is_agent());
        assert_eq!(m.agent.as_ref().map(|a| a.args.clone()), Some(vec!["chat".to_string()]));
        for (key, bad) in [("events", json!("other")), ("resume", json!(["--session"])), ("mcpConfig", json!(["--mcp"]))] {
            let mut b = v.clone();
            b["agent"][key] = bad;
            assert!(Manifest::parse(&b.to_string()).is_err(), "{key}");
        }
        let mut b = v.clone();
        b["kind"] = json!("robot");
        assert!(Manifest::parse(&b.to_string()).is_err());
        b["kind"] = json!("tool");
        assert!(Manifest::parse(&b.to_string()).is_err(), "an agent section needs kind agent");
        assert!(!Manifest::parse(&sample()).unwrap().is_agent());
    }
}
