//! Persisted app state: registered projects, the last active one, and each worktree's pinned base
//! branch (git can't say which branch a worktree was forked from, so the user's pick is stored).
use std::{collections::HashMap, path::PathBuf};

use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Default)]
pub struct Config {
    pub projects: Vec<String>,
    pub last_project: Option<String>,
    /// repo path -> (worktree path -> base branch)
    pub pins: HashMap<String, HashMap<String, String>>,
    #[serde(skip)]
    path: Option<PathBuf>,
}

fn config_path() -> Option<PathBuf> {
    let dir = if cfg!(target_os = "macos") {
        PathBuf::from(std::env::var_os("HOME")?).join("Library/Application Support/maditor")
    } else if cfg!(target_os = "windows") {
        PathBuf::from(std::env::var_os("APPDATA")?).join("maditor")
    } else {
        std::env::var_os("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))?
            .join("maditor")
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
