//! `registry.json` (`bashcut-registry/1`): the plugins a host can install and where their files are.

use serde::{Deserialize, Serialize};

use crate::{Error, Result, err, manifest, valid_id};

pub const SCHEMA: &str = "bashcut-registry/1";
pub const FILE: &str = "registry.json";
pub const MAX_BYTES: u64 = 1024 * 1024;
const MAX_PLUGINS: usize = 1024;

/// The registry the app installs from unless told otherwise (`BASHCUT_PLUGIN_REGISTRY`).
pub const DEFAULT_REPO: &str = "dongnguyenvie/bashcutpro-plugins";
pub const DEFAULT_REF: &str = "main";

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Registry {
    pub schema: String,
    pub name: String,
    pub repo: String,
    pub plugins: Vec<Entry>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Entry {
    pub id: String,
    pub name: String,
    pub version: String,
    pub summary: String,
    pub category: String,
    pub platforms: Vec<String>,
    pub min_app_version: String,
    pub runtime: String,
    pub path: String,
    pub manifest: String,
    pub source: Source,
    /// `name` / `summary` in other languages, as in the manifest (`{"vi": {"name", "summary"}}`).
    pub i18n: manifest::I18n,
}

impl Entry {
    /// The name in `lang` when the registry has a non-empty one, else the English `name`.
    pub fn name_in(&self, lang: &str) -> &str {
        manifest::local(&self.i18n, lang, &self.name, |x| &x.name)
    }

    /// The summary in `lang` when the registry has a non-empty one, else the English `summary`.
    pub fn summary_in(&self, lang: &str) -> &str {
        manifest::local(&self.i18n, lang, &self.summary, |x| &x.summary)
    }
}

/// A folder of a git repository at a ref.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Source {
    #[serde(rename = "type")]
    pub kind: String,
    pub repo: String,
    #[serde(rename = "ref")]
    pub git_ref: String,
    pub path: String,
}

impl Registry {
    pub fn parse(text: &str) -> Result<Registry> {
        if text.len() as u64 > MAX_BYTES {
            return err(format!("registry larger than {MAX_BYTES} bytes"));
        }
        let r: Registry = serde_json::from_str(text).map_err(|e| Error(format!("registry: {e}")))?;
        if r.schema != SCHEMA {
            return err(format!("registry schema is `{}`, expected `{SCHEMA}`", r.schema));
        }
        if r.plugins.len() > MAX_PLUGINS {
            return err("registry lists too many plugins");
        }
        for p in &r.plugins {
            if !valid_id(&p.id) || !manifest::valid_version(&p.version) {
                return err(format!("registry entry `{}` has a bad id or version", p.id));
            }
            if p.source.kind != "git" || !safe_rel_path(&p.source.path) || !safe_ref(&p.source.git_ref) || p.source.repo.is_empty() {
                return err(format!("registry entry `{}` has a bad source", p.id));
            }
        }
        Ok(r)
    }

    pub fn find(&self, id: &str) -> Option<&Entry> {
        self.plugins.iter().find(|p| p.id == id)
    }
}

impl Entry {
    pub fn supports(&self, os: &str) -> bool {
        self.platforms.is_empty() || self.platforms.iter().any(|p| p == os)
    }
}

/// A relative path inside a repository: no root, no `..`, no backslashes or drive letters.
pub fn safe_rel_path(p: &str) -> bool {
    !p.is_empty()
        && p.len() <= 512
        && !p.starts_with('/')
        && !p.contains('\\')
        && !p.contains(':')
        && p.split('/').all(|part| !part.is_empty() && part != ".." && part != ".")
}

/// A branch or tag name git accepts and that cannot be read as an option.
pub fn safe_ref(r: &str) -> bool {
    !r.is_empty() && r.len() <= 200 && !r.starts_with('-') && !r.contains("..") && r.bytes().all(|b| b.is_ascii_alphanumeric() || b"._-/+".contains(&b))
}

/// Where to clone `repo` from: `owner/name` is a GitHub repository; a URL or a local path is
/// used as is (tests and mirrors).
pub fn clone_url(repo: &str) -> Result<String> {
    if repo.starts_with('-') || repo.is_empty() || repo.len() > 1024 {
        return err(format!("bad repository `{repo}`"));
    }
    if repo.contains("://") || repo.starts_with('/') || repo.starts_with('.') || std::path::Path::new(repo).is_absolute() {
        return Ok(repo.to_string());
    }
    let parts: Vec<&str> = repo.split('/').collect();
    let ok = parts.len() == 2 && parts.iter().all(|p| !p.is_empty() && p.bytes().all(|b| b.is_ascii_alphanumeric() || b"._-".contains(&b)));
    if !ok {
        return err(format!("bad repository `{repo}`"));
    }
    Ok(format!("https://github.com/{repo}.git"))
}

/// `repo[@ref]` (the `BASHCUT_PLUGIN_REGISTRY` form) into a repo and a ref. An `@` followed by
/// `/` or `:` belongs to the repository (`ssh://git@host/…`, `git@host:o/r`).
pub fn split_location(loc: &str) -> (String, String) {
    match loc.rsplit_once('@') {
        Some((repo, r)) if !repo.is_empty() && !r.contains('/') && !r.contains(':') && safe_ref(r) => (repo.to_string(), r.to_string()),
        _ => (loc.to_string(), DEFAULT_REF.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn sample() -> serde_json::Value {
        json!({"schema": "bashcut-registry/1", "name": "t", "repo": "o/r", "plugins": [
            {"id": "test.echo", "name": "Echo", "version": "0.1.0", "platforms": ["macos", "linux"],
             "source": {"type": "git", "repo": "o/r", "ref": "main", "path": "plugins/echo"}}
        ]})
    }

    #[test]
    fn parses_and_finds() {
        let r = Registry::parse(&sample().to_string()).unwrap();
        let e = r.find("test.echo").unwrap();
        assert!(e.supports("linux") && !e.supports("windows"));
        assert_eq!(clone_url("o/r").unwrap(), "https://github.com/o/r.git");
        assert_eq!(clone_url("/tmp/repo").unwrap(), "/tmp/repo");
        assert!(clone_url("--upload-pack=x").is_err());
        assert!(clone_url("a/b/c").is_err());
        assert_eq!(split_location("o/r@v1.0"), ("o/r".to_string(), "v1.0".to_string()));
        assert_eq!(split_location("o/r"), ("o/r".to_string(), "main".to_string()));
        assert_eq!(split_location("ssh://git@host/o/r").1, "main");
    }

    #[test]
    fn entry_text_follows_the_language() {
        // an entry without `i18n` (every one before VI3) reads in English
        let old = Registry::parse(&sample().to_string()).unwrap();
        assert_eq!((old.plugins[0].name_in("vi"), old.plugins[0].summary_in("vi")), ("Echo", ""));
        let mut v = sample();
        v["plugins"][0]["summary"] = json!("Says it back.");
        v["plugins"][0]["i18n"] = json!({"vi": {"name": "Tiếng vọng", "summary": " "}});
        let r = Registry::parse(&v.to_string()).unwrap();
        let e = &r.plugins[0];
        assert_eq!((e.name_in("vi"), e.summary_in("vi"), e.name_in("en"), e.summary_in("en")), ("Tiếng vọng", "Says it back.", "Echo", "Says it back."));
        v["plugins"][0]["i18n"] = json!({"vi": "x"});
        assert!(Registry::parse(&v.to_string()).is_err(), "a translation is an object");
    }

    #[test]
    fn rejects_unsafe_sources() {
        for (k, v) in
            [("path", json!("../../etc")), ("path", json!("/abs")), ("ref", json!("--upload-pack=x")), ("type", json!("zip")), ("path", json!("a\\b"))]
        {
            let mut r = sample();
            r["plugins"][0]["source"][k] = v;
            assert!(Registry::parse(&r.to_string()).is_err(), "{k}");
        }
        let mut r = sample();
        r["plugins"][0]["id"] = json!("Bad Id");
        assert!(Registry::parse(&r.to_string()).is_err());
        assert!(Registry::parse("{}").is_err());
    }
}
