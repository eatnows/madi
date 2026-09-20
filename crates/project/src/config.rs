//! Persisted app state: registered projects, the last active one, and each worktree's pinned base
//! branch (git can't say which branch a worktree was forked from, so the user's pick is stored).
use std::{collections::HashMap, path::PathBuf};

use serde::{Deserialize, Serialize};

/// Light/dark choice: follow the OS, or force one.
#[derive(Serialize, Deserialize, Default, Clone, Copy, PartialEq, Debug)]
#[serde(rename_all = "snake_case")]
pub enum Appearance {
    #[default]
    System,
    Light,
    Dark,
}

impl Appearance {
    pub fn next(self) -> Self {
        match self {
            Appearance::System => Appearance::Light,
            Appearance::Light => Appearance::Dark,
            Appearance::Dark => Appearance::System,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Appearance::System => "Auto",
            Appearance::Light => "Light",
            Appearance::Dark => "Dark",
        }
    }
}

/// `default` on the whole struct: a config missing a field (older file, hand-edited, or written by a
/// build before the field existed) must keep what it does have instead of failing to parse and
/// silently resetting everything, including the registered projects.
#[derive(Serialize, Deserialize, Default)]
#[serde(default)]
pub struct Config {
    pub appearance: Appearance,
    pub projects: Vec<String>,
    pub last_project: Option<String>,
    /// repo path -> (worktree path -> base branch)
    pub pins: HashMap<String, HashMap<String, String>>,
    #[serde(skip)]
    path: Option<PathBuf>,
}

fn config_path() -> Option<PathBuf> {
    let dir = if cfg!(target_os = "macos") {
        PathBuf::from(std::env::var_os("HOME")?).join("Library/Application Support/madi")
    } else if cfg!(target_os = "windows") {
        PathBuf::from(std::env::var_os("APPDATA")?).join("madi")
    } else {
        std::env::var_os("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))?
            .join("madi")
    };
    Some(dir.join("config.json"))
}

impl Config {
    pub fn load() -> Self {
        Self::at(config_path())
    }

    /// Reads (or starts empty) a config bound to `path`; tests point this at a temp file.
    pub fn at(path: Option<PathBuf>) -> Self {
        let mut config: Config = path
            .as_ref()
            .and_then(|p| std::fs::read_to_string(p).ok())
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default();
        config.path = path;
        config
    }

    /// Best-effort: failing to persist shouldn't interrupt using the app.
    pub fn save(&self) {
        let Some(path) = self.path.clone() else { return };
        if let Some(dir) = path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        if let Ok(json) = serde_json::to_string_pretty(self) {
            let _ = std::fs::write(path, json);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_partial_config_keeps_the_fields_it_has() {
        let dir = std::env::temp_dir().join("madi-test-config");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("partial.json");
        std::fs::write(&path, r#"{"projects":["/a","/b"],"appearance":"dark"}"#).unwrap();

        let config = Config::at(Some(path));
        assert_eq!(config.projects, ["/a", "/b"]);
        assert_eq!(config.appearance, Appearance::Dark);
        assert!(config.pins.is_empty() && config.last_project.is_none());
    }
}
