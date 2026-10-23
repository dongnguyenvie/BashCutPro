//! Doctor checks against a fake machine (#73).

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use bashcut_doctor::env::{RUN_TIMEOUT, parse_df_free, parse_meminfo_total};
use bashcut_doctor::{Env, Level, Options, Output, REQUIRED_COMMANDS, RealEnv, Report, cli_program, meets, parse_version, run_checks, updates};

const GB: u64 = 1 << 30;

#[derive(Default)]
struct Fake {
    vars: BTreeMap<String, String>,
    programs: BTreeMap<String, Output>,
    files: BTreeMap<PathBuf, Vec<u8>>,
    hashes: BTreeMap<PathBuf, String>,
    free: Option<u64>,
    ram: Option<u64>,
    /// `linux` when unset.
    os: Option<&'static str>,
}

impl Fake {
    /// A healthy machine: the CLI with every required command, a kit, one Python plugin, one verified model.
    fn healthy() -> Self {
        let mut f = Fake { free: Some(200 * GB), ram: Some(32 * GB), ..Default::default() };
        f.vars.insert("FILMCRAFT_DATA_DIR".into(), "data".into());
        f.vars.insert("BASHCUTPRO_KIT_DIR".into(), "kit".into());
        f.program("filmcraft-cli --version", 0, "filmcraft-cli 0.9.0\n");
        let ids: Vec<String> = REQUIRED_COMMANDS.iter().map(|id| format!("{{\"id\":\"{id}\"}}")).collect();
        f.program("filmcraft-cli commands bashcut --json", 0, &format!("[{}]", ids.join(",")));
        f.program("filmcraft-cli exec transcript.models", 0, &speech(true, true));
        f.program("python3 --version", 0, "Python 3.12.1\n");
        f.program("claude --version", 0, "2.1.0 (Claude Code)\n");
        f.file("kit/.claude-plugin/plugin.json", br#"{"name":"bcp","version":"0.3.0"}"#);
        f.file("kit/skills/setup/SKILL.md", b"---\n");
        f.file(
            "data/bashcut-plugins/tc/bashcut-plugin.json",
            br#"{"schema":"bashcut-plugin/1","id":"bashcut.translate-captions","version":"0.1.0","runtime":{"type":"python","version":">=3.9"},"entry":{"command":["python3","plugin.py"],"windows":["py","-3","plugin.py"]}}"#,
        );
        f.file(
            "data/bashcut-plugins/models.json",
            br#"{"schema":"bashcut-models/1","models":[{"id":"m1","files":[{"path":"a.onnx","size":3,"sha256":"abc"}]}]}"#,
        );
        f.file("data/bashcut-models/m1/a.onnx", b"xyz");
        f.hashes.insert(PathBuf::from("data/bashcut-models/m1/a.onnx"), "abc".into());
        f
    }
    fn program(&mut self, line: &str, code: i32, stdout: &str) {
        self.programs.insert(line.into(), Output { code: Some(code), stdout: stdout.into(), stderr: String::new() });
    }
    fn file(&mut self, path: &str, bytes: &[u8]) {
        self.files.insert(PathBuf::from(path), bytes.to_vec());
    }
}

impl Env for Fake {
    fn var(&self, key: &str) -> Option<String> {
        self.vars.get(key).cloned()
    }
    fn run(&self, program: &str, args: &[&str]) -> Option<Output> {
        let line = std::iter::once(program).chain(args.iter().copied()).collect::<Vec<_>>().join(" ");
        self.programs.get(&line).cloned()
    }
    fn read(&self, path: &Path) -> Option<Vec<u8>> {
        self.files.get(&norm(path)).cloned()
    }
    fn file_size(&self, path: &Path) -> Option<u64> {
        self.files.get(&norm(path)).map(|b| b.len() as u64)
    }
    fn hash_file(&self, path: &Path) -> Option<String> {
        self.hashes.get(&norm(path)).cloned()
    }
    fn list(&self, dir: &Path) -> Vec<PathBuf> {
        let dir = norm(dir);
        let mut v: Vec<PathBuf> =
            self.files.keys().filter_map(|f| f.strip_prefix(&dir).ok().and_then(|rest| rest.components().next()).map(|c| dir.join(c.as_os_str()))).collect();
        v.sort();
        v.dedup();
        v
    }
    fn free_bytes(&self, _: &Path) -> Option<u64> {
        self.free
    }
    fn total_ram(&self) -> Option<u64> {
        self.ram
    }
    fn os(&self) -> &str {
        self.os.unwrap_or("linux")
    }
}

/// Paths compare by components, so `data/x` and `data\x` are the same on every OS.
fn norm(p: &Path) -> PathBuf {
    p.components().collect()
}

/// `transcript.models` as the CLI answers it.
fn speech(available: bool, installed: bool) -> String {
    format!(
        r#"{{"available":{available},"default":"whisper-base","models":[{{"id":"whisper-tiny","installed":true}},{{"id":"whisper-base","installed":{installed}}}]}}"#
    )
}

fn level(r: &Report, id: &str) -> Level {
    r.findings.iter().find(|f| f.id == id).map(|f| f.level).unwrap_or_else(|| panic!("no finding `{id}`: {}", r.to_text()))
}

#[test]
fn a_healthy_machine_passes() {
    let r = run_checks(&Fake::healthy(), &Options::default());
    assert_eq!(r.exit_code(), 0, "{}", r.to_text());
    for id in ["cli", "cli.commands", "speech", "kit", "plugin.bashcut.translate-captions", "model.m1", "agent.claude", "disk", "ram", "gpu"] {
        assert_eq!(level(&r, id), Level::Ok, "{id}: {}", r.to_text());
    }
    assert_eq!(level(&r, "agent.codex"), Level::Info, "one agent CLI is enough");
    assert_eq!(r.to_json()["ok"], true);
}

#[test]
fn missing_cli_fails_with_a_fix() {
    let mut f = Fake::healthy();
    f.programs.retain(|k, _| !k.starts_with("filmcraft-cli"));
    let r = run_checks(&f, &Options::default());
    assert_eq!(level(&r, "cli"), Level::Fail);
    assert!(r.findings.iter().any(|x| x.id == "cli" && x.fix.as_deref().is_some_and(|fix| fix.contains("bcp:setup"))));
    assert_eq!(r.exit_code(), 1);
    assert_eq!(r.to_json()["ok"], false);
}

#[test]
fn pro_name_first_and_filmcraft_cli_still_works() {
    // only bashcutpro installed (#154): the lookup finds it on the fake's PATH (DR1)
    let dir = std::env::temp_dir().join(format!("bashcut-doctor-pro-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let pro = dir.join(format!("bashcutpro{}", std::env::consts::EXE_SUFFIX));
    std::fs::write(&pro, b"").unwrap();
    let pro = pro.to_string_lossy().into_owned();
    let mut f = Fake::healthy();
    f.vars.insert("PATH".into(), dir.to_string_lossy().into_owned());
    let old: Vec<(String, Output)> = std::mem::take(&mut f.programs).into_iter().collect();
    for (k, v) in old {
        f.programs.insert(k.replace("filmcraft-cli", &pro), v);
    }
    let r = run_checks(&f, &Options::default());
    assert_eq!(level(&r, "cli"), Level::Ok);
    assert!(r.findings.iter().any(|x| x.id == "cli" && x.message.ends_with(&format!("({pro})"))), "{:?}", r.findings);
    assert_eq!(level(&r, "cli.commands"), Level::Ok);
    let _ = std::fs::remove_dir_all(&dir);
    // only filmcraft-cli: the healthy machine as it was
    let r = run_checks(&Fake::healthy(), &Options::default());
    assert!(r.findings.iter().any(|x| x.id == "cli" && x.message.contains("(filmcraft-cli)")), "{:?}", r.findings);
    // FILMCRAFT_CLI names it when BASHCUTPRO_CLI does not
    let mut f = Fake::healthy();
    f.vars.insert("FILMCRAFT_CLI".into(), "filmcraft-cli".into());
    f.program("bashcutpro --version", 0, "filmcraft-cli 0.9.0\n");
    assert!(run_checks(&f, &Options::default()).findings.iter().any(|x| x.id == "cli" && x.message.contains("(filmcraft-cli)")));
}

#[test]
fn speech_to_text_missing_or_not_downloaded_warns() {
    let mut f = Fake::healthy();
    f.program("filmcraft-cli exec transcript.models", 0, &speech(false, false));
    let r = run_checks(&f, &Options::default());
    assert_eq!(level(&r, "speech"), Level::Warn);
    assert!(r.findings.iter().any(|x| x.id == "speech" && x.fix.as_deref().is_some_and(|fix| fix.contains("--features whisper"))));
    f.program("filmcraft-cli exec transcript.models", 0, &speech(true, false));
    let r = run_checks(&f, &Options::default());
    assert_eq!(level(&r, "speech"), Level::Warn);
    assert!(r.findings.iter().any(|x| x.id == "speech" && x.fix.as_deref().is_some_and(|fix| fix.contains("transcript.downloadModel"))));
    f.programs.remove("filmcraft-cli exec transcript.models");
    assert_eq!(level(&run_checks(&f, &Options::default()), "speech"), Level::Info, "an old CLI is not blamed");
}

#[test]
fn an_old_build_without_our_commands_fails() {
    let mut f = Fake::healthy();
    f.program("filmcraft-cli commands bashcut --json", 0, r#"[{"id":"bashcut.profiles.list"}]"#);
    let r = run_checks(&f, &Options::default());
    assert_eq!(level(&r, "cli.commands"), Level::Fail);
}

#[test]
fn wrong_model_hash_or_size_fails() {
    let mut f = Fake::healthy();
    f.hashes.insert(PathBuf::from("data/bashcut-models/m1/a.onnx"), "def".into());
    let r = run_checks(&f, &Options::default());
    assert_eq!(level(&r, "model.m1"), Level::Fail, "{}", r.to_text());
    // sizes only: the hash is not read
    let r = run_checks(&f, &Options { hash_models: false, ..Options::default() });
    assert_eq!(level(&r, "model.m1"), Level::Ok);
    f.file("data/bashcut-models/m1/a.onnx", b"too long");
    let r = run_checks(&f, &Options { hash_models: false, ..Options::default() });
    assert_eq!(level(&r, "model.m1"), Level::Fail);
    // not downloaded yet is fine
    f.files.remove(&PathBuf::from("data/bashcut-models/m1/a.onnx"));
    assert_eq!(level(&run_checks(&f, &Options::default()), "model.m1"), Level::Info);
}

#[test]
fn low_disk_warns_then_fails() {
    let mut f = Fake::healthy();
    f.free = Some(5 * GB);
    assert_eq!(level(&run_checks(&f, &Options::default()), "disk"), Level::Warn);
    f.free = Some(GB);
    let r = run_checks(&f, &Options::default());
    assert_eq!(level(&r, "disk"), Level::Fail);
    assert_eq!(r.exit_code(), 1);
    f.free = None;
    assert_eq!(level(&run_checks(&f, &Options::default()), "disk"), Level::Info);
}

#[test]
fn old_python_missing_agents_and_low_ram_warn() {
    let mut f = Fake::healthy();
    f.program("python3 --version", 0, "Python 3.8.10\n");
    f.programs.remove("claude --version");
    f.ram = Some(4 * GB);
    let r = run_checks(&f, &Options::default());
    assert_eq!(level(&r, "plugin.bashcut.translate-captions"), Level::Warn);
    assert_eq!(level(&r, "agent"), Level::Warn);
    assert_eq!(level(&r, "ram"), Level::Warn);
    assert_eq!(r.exit_code(), 0, "warnings do not stop a run");
}

#[test]
fn hostile_files_do_not_crash() {
    let mut f = Fake::healthy();
    f.file("data/bashcut-plugins/models.json", br#"{"models":[{"id":"../../x","files":[{"path":"../../etc/passwd"}]},{"id":"m2","files":"no"},7]}"#);
    f.file("data/bashcut-plugins/bad/bashcut-plugin.json", b"{");
    f.file("data/bashcut-plugins/odd/bashcut-plugin.json", br#"{"schema":"bashcut-plugin/1","runtime":{"type":"python"},"entry":{"command":[]}}"#);
    f.program("filmcraft-cli commands bashcut --json", 0, "not json");
    let r = run_checks(&f, &Options::default());
    assert!(!r.findings.iter().any(|x| x.id.contains("..")), "{}", r.to_text());
    assert_eq!(level(&r, "cli.commands"), Level::Warn);
}

#[test]
fn updates_are_reported_not_installed() {
    let mut f = Fake::healthy();
    f.file("registry.json", br#"{"schema":"bashcut-registry/1","plugins":[{"id":"bashcut.translate-captions","version":"0.2.0"}]}"#);
    f.program("git -C kit rev-parse HEAD", 0, "aaa\n");
    f.program("git -C kit ls-remote origin HEAD", 0, "bbb\tHEAD\n");
    f.program("gh release view -R dongnguyenvie/BashCutPro --json tagName -q .tagName", 0, "v0.10.0\n");
    let opts = Options { kit_dir: Some(PathBuf::from("kit")), registry: Some("registry.json".into()), ..Options::default() };
    let found = updates::check_updates(&f, &opts);
    let lvl = |id: &str| found.iter().find(|x| x.id == id).map(|x| (x.level, x.message.clone()));
    assert!(lvl("update.plugin.bashcut.translate-captions").is_some_and(|(_, m)| m.contains("0.2.0")), "{found:?}");
    assert!(lvl("update.kit").is_some_and(|(_, m)| m.contains("newer")));
    assert!(lvl("update.app").is_some_and(|(_, m)| m.contains("v0.10.0")));
    assert!(found.iter().all(|x| x.level <= Level::Info), "updates never fail a run");
    // nothing reachable: skipped, still no failure
    let quiet = updates::check_updates(&Fake::healthy(), &Options { registry: Some("missing.json".into()), ..Options::default() });
    assert!(quiet.iter().all(|x| x.level <= Level::Info));
}

#[test]
fn version_helpers() {
    assert_eq!(parse_version("Python 3.12.1"), Some((3, 12, 1)));
    assert_eq!(parse_version("filmcraft-cli 0.9.0"), Some((0, 9, 0)));
    assert_eq!(parse_version("v1.2"), Some((1, 2, 0)));
    assert_eq!(parse_version("no digits"), None);
    assert!(meets((3, 9, 0), ">=3.9"));
    assert!(!meets((3, 8, 10), ">=3.9"));
    assert_eq!(parse_df_free("Filesystem 1024-blocks Used Available Capacity Mounted\n/dev/sda1 100 50 40 50% /\n"), Some(40 * 1024));
    assert_eq!(parse_df_free("garbage"), None);
    assert_eq!(parse_meminfo_total("MemTotal:       16314728 kB\nMemFree: 1 kB\n"), Some(16_314_728 * 1024));
}

/// The Claude Code plugin install (the kit README's way) and the setup's clone count as the kit (#182).
#[test]
fn kit_installed_as_claude_plugin_or_by_setup_is_found() {
    let mut f = Fake::healthy();
    f.vars.remove("BASHCUTPRO_KIT_DIR");
    f.vars.insert("HOME".into(), "home".into());
    assert_eq!(level(&run_checks(&f, &Options::default()), "kit"), Level::Warn);

    f.file(
        "home/.claude/plugins/installed_plugins.json",
        br#"{"version":2,"plugins":{"bcp@bashcutpro-agent-kit":[{"scope":"user","installPath":"home/.claude/plugins/cache/bashcutpro-agent-kit/bcp/0.2.16","version":"0.2.16"}]}}"#,
    );
    // a registry entry whose files are gone is not a kit
    assert_eq!(level(&run_checks(&f, &Options::default()), "kit"), Level::Warn);
    f.file("home/.claude/plugins/cache/bashcutpro-agent-kit/bcp/0.2.16/skills/setup/SKILL.md", b"---\n");
    let r = run_checks(&f, &Options::default());
    assert_eq!(level(&r, "kit"), Level::Ok, "{}", r.to_text());
    assert!(r.to_text().contains("Claude Code plugin bcp 0.2.16"), "{}", r.to_text());

    let mut g = Fake::healthy();
    g.vars.remove("BASHCUTPRO_KIT_DIR");
    g.vars.insert("HOME".into(), "home".into());
    g.file("home/.bashcutpro/bashcutpro-agent-kit/skills/setup/SKILL.md", b"---\n");
    assert_eq!(level(&run_checks(&g, &Options::default()), "kit"), Level::Ok);
}

#[test]
fn shared_engine_lookup_honours_bcpro_and_bashcut_filmcraft_cli() {
    let cli = |f: &Fake| cli_program(f, &Options::default());
    let finding = |f: &Fake| run_checks(f, &Options::default()).findings.into_iter().find(|x| x.id == "cli").map(|x| (x.level, x.message));
    // the older variable batch and bashcut-mcp used
    let mut f = Fake::healthy();
    f.vars.insert("BASHCUT_FILMCRAFT_CLI".into(), "/opt/bc/engine".into());
    f.program("/opt/bc/engine --version", 0, "bashcutpro 0.9.0\n");
    assert_eq!(cli(&f), "/opt/bc/engine");
    assert_eq!(finding(&f), Some((Level::Ok, "bashcutpro 0.9.0 (/opt/bc/engine)".into())));
    // BASHCUTPRO_CLI wins; an empty one counts as unset
    f.vars.insert("BASHCUTPRO_CLI".into(), "/opt/pro".into());
    assert_eq!(cli(&f), "/opt/pro");
    f.vars.insert("BASHCUTPRO_CLI".into(), String::new());
    assert_eq!(cli(&f), "/opt/bc/engine");

    // no variable, and the fake's PATH holds only `bcpro` (the macOS bundle's name)
    let dir = std::env::temp_dir().join(format!("bashcut-doctor-bcpro-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let bcpro = dir.join(format!("bcpro{}", std::env::consts::EXE_SUFFIX));
    std::fs::write(&bcpro, b"").unwrap();
    let mut f = Fake::healthy();
    f.vars.insert("PATH".into(), dir.to_string_lossy().into_owned());
    let program = bcpro.to_string_lossy().into_owned();
    assert_eq!(cli(&f), program);
    f.program(&format!("{program} --version"), 0, "bashcutpro 0.9.0\n");
    let (level, message) = finding(&f).unwrap();
    assert_eq!(level, Level::Ok);
    assert!(message.ends_with(&format!("({program})")) && message.contains("bcpro"), "{message}");
    let _ = std::fs::remove_dir_all(&dir);

    // nothing names or holds an engine
    assert_eq!(cli(&Fake::default()), "filmcraft-cli");
}

#[test]
fn an_invalid_manifest_warns_under_its_folder_name() {
    let mut f = Fake::healthy();
    f.file("data/bashcut-plugins/broken/bashcut-plugin.json", br#"{"schema":"bashcut-plugin/1","id":"../evil id","version":"x"}"#);
    f.file("data/bashcut-plugins/we!rd.name/bashcut-plugin.json", b"{");
    let r = run_checks(&f, &Options::default());
    assert_eq!(level(&r, "plugin.broken"), Level::Warn, "{}", r.to_text());
    assert_eq!(level(&r, "plugin.werdname"), Level::Warn, "only letters, digits, - and _ of the folder name");
    assert!(r.findings.iter().any(|x| x.id == "plugin.broken" && x.fix.as_deref().is_some_and(|fix| fix.contains("reinstall"))));
    assert!(!r.findings.iter().any(|x| x.id.contains("evil")), "{}", r.to_text());
    assert_eq!(level(&r, "plugin.bashcut.translate-captions"), Level::Ok, "the good plugin is still checked");
    assert_eq!(r.exit_code(), 0, "a bad plugin does not stop a run");
}

#[test]
fn other_unixes_use_the_linux_entry() {
    let mut f = Fake::healthy();
    f.file(
        "data/bashcut-plugins/tc/bashcut-plugin.json",
        br#"{"schema":"bashcut-plugin/1","id":"bashcut.translate-captions","version":"0.1.0","runtime":{"type":"python","version":">=3.9"},"entry":{"command":["python3","plugin.py"],"linux":["python3.11","plugin.py"],"windows":["py","-3","plugin.py"]}}"#,
    );
    f.program("python3.11 --version", 0, "Python 3.11.4\n");
    f.program("py -3 --version", 0, "Python 3.13.0\n");
    let msg =
        |f: &Fake| run_checks(f, &Options::default()).findings.into_iter().find(|x| x.id == "plugin.bashcut.translate-captions").map(|x| (x.level, x.message));
    for os in ["linux", "freebsd", "openbsd"] {
        f.os = Some(os);
        assert_eq!(msg(&f), Some((Level::Ok, "bashcut.translate-captions 0.1.0, Python 3.11.4".into())), "{os}");
    }
    f.os = Some("windows");
    assert!(msg(&f).is_some_and(|(_, m)| m.ends_with("Python 3.13.0")), "the -3 flag is kept for py");
    // macOS has no key: the plain command
    f.os = Some("macos");
    assert!(msg(&f).is_some_and(|(_, m)| m.ends_with("Python 3.12.1")));
}

#[test]
fn real_run_times_out_with_null_stdin() {
    use std::time::{Duration, Instant};
    // a program that reads stdin ends at once: stdin is null, not the test's
    let (reader, args): (&str, &[&str]) = if cfg!(windows) { ("findstr", &["x"]) } else { ("cat", &[]) };
    let t = Instant::now();
    let out = RealEnv.run(reader, args).expect("starts");
    assert!(out.code.is_some(), "{out:?}");
    assert!(t.elapsed() < Duration::from_secs(10), "waited on stdin");
    assert!(RealEnv.run("bashcut-doctor-no-such-program", &[]).is_none());

    // one that hangs is stopped after RUN_TIMEOUT, with no exit code
    let (sleeper, args): (&str, &[&str]) = if cfg!(windows) { ("ping", &["-n", "120", "127.0.0.1"]) } else { ("sleep", &["120"]) };
    let t = Instant::now();
    let out = RealEnv.run(sleeper, args).expect("starts");
    let took = t.elapsed();
    assert_eq!(out.code, None, "{out:?}");
    assert!(!out.ok());
    assert!(out.stderr.contains("stopped after"), "{}", out.stderr);
    assert!(took >= RUN_TIMEOUT && took < RUN_TIMEOUT + Duration::from_secs(20), "{took:?}");
}
