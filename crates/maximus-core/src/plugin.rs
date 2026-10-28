//! Lightweight plugins: a TOML manifest plus optional Maxima files.
//!
//! ```toml
//! name = "Physics constants"
//! version = "0.1.0"
//! description = "Common constants and helpers"
//! maxima_files = ["physics.mac"]      # loaded with load() at startup
//! init = "kill(foo)$"                 # optional extra Maxima code run at startup
//!
//! [[palette]]
//! name = "Physics"
//! symbols = ["ℏ", "∇", "Å"]
//!
//! [[menu]]
//! category = "Physics"
//! label = "Kinetic energy"
//! template = "kinetic_energy(<m>, <v>)"
//! description = "Returns m v^2 / 2"
//!
//! [[snippet]]
//! label = "Quadratic formula"
//! code = "solve(a*x^2+b*x+c=0,x)"
//! ```

use serde::Deserialize;
use std::path::{Path, PathBuf};

#[derive(Clone, Debug, Default, Deserialize)]
pub struct PluginPalette {
    pub name: String,
    #[serde(default)]
    pub symbols: Vec<String>,
}

#[derive(Clone, Debug, Default, Deserialize)]
pub struct PluginMenuItem {
    pub category: String,
    pub label: String,
    /// Maxima call with `<arg>` placeholders, e.g. `kinetic_energy(<m>, <v>)`.
    pub template: String,
    #[serde(default)]
    pub description: String,
}

#[derive(Clone, Debug, Default, Deserialize)]
pub struct PluginSnippet {
    pub label: String,
    pub code: String,
}

#[derive(Clone, Debug, Default, Deserialize)]
pub struct PluginManifest {
    pub name: String,
    #[serde(default)]
    pub version: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub maxima_files: Vec<String>,
    #[serde(default)]
    pub init: Option<String>,
    #[serde(default)]
    pub palette: Vec<PluginPalette>,
    #[serde(default)]
    pub menu: Vec<PluginMenuItem>,
    #[serde(default)]
    pub snippet: Vec<PluginSnippet>,
}

#[derive(Clone, Debug)]
pub struct LoadedPlugin {
    pub manifest: PluginManifest,
    pub dir: PathBuf,
    pub enabled: bool,
}

impl LoadedPlugin {
    /// Maxima statements to run at startup for this plugin.
    pub fn startup_code(&self) -> Vec<String> {
        let mut v = Vec::new();
        for f in &self.manifest.maxima_files {
            let p = self.dir.join(f);
            v.push(format!("load(\"{}\")$", p.display().to_string().replace('\\', "/")));
        }
        if let Some(i) = &self.manifest.init {
            v.push(i.clone());
        }
        v
    }
}

pub fn parse_manifest(text: &str) -> Result<PluginManifest, String> {
    let m: PluginManifest = toml::from_str(text).map_err(|e| e.to_string())?;
    if m.name.trim().is_empty() {
        return Err("plugin.toml needs a non-empty `name`".into());
    }
    Ok(m)
}

/// Loads every `<dir>/<plugin>/plugin.toml`. Broken plugins are reported in the second list.
pub fn load_plugins(dir: &Path) -> (Vec<LoadedPlugin>, Vec<String>) {
    let mut ok = Vec::new();
    let mut errs = Vec::new();
    let Ok(rd) = std::fs::read_dir(dir) else { return (ok, errs) };
    let mut dirs: Vec<PathBuf> = rd.filter_map(Result::ok).map(|e| e.path()).filter(|p| p.is_dir()).collect();
    dirs.sort();
    for d in dirs {
        let mf = d.join("plugin.toml");
        if !mf.exists() {
            continue;
        }
        match std::fs::read_to_string(&mf).map_err(|e| e.to_string()).and_then(|t| parse_manifest(&t)) {
            Ok(m) => ok.push(LoadedPlugin { manifest: m, dir: d, enabled: true }),
            Err(e) => errs.push(format!("{}: {}", mf.display(), e)),
        }
    }
    (ok, errs)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_manifest() {
        let m = parse_manifest(
            r#"
name = "Demo"
maxima_files = ["a.mac"]
[[palette]]
name = "P"
symbols = ["ℏ"]
[[menu]]
category = "Physics"
label = "KE"
template = "ke(<m>, <v>)"
"#,
        )
        .unwrap();
        assert_eq!(m.menu[0].template, "ke(<m>, <v>)");
        let p = LoadedPlugin { manifest: m, dir: "/tmp/demo".into(), enabled: true };
        assert_eq!(p.startup_code(), vec!["load(\"/tmp/demo/a.mac\")$".to_string()]);
        assert!(parse_manifest("version = \"1\"").is_err());
    }
}
