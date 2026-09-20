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
    pub const ALL: [Appearance; 3] = [Appearance::System, Appearance::Light, Appearance::Dark];

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
    /// `None` until the user changes it; read through [`Config::font_size`].
    pub editor_font_size: Option<f32>,
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

pub const DEFAULT_FONT_SIZE: f32 = 13.0;
pub const FONT_SIZE_RANGE: (f32, f32) = (10.0, 24.0);

impl Config {
    /// The editor's font size, kept inside a range the layout copes with even if the file was edited by hand.
    pub fn font_size(&self) -> f32 {
        self.editor_font_size.unwrap_or(DEFAULT_FONT_SIZE).clamp(FONT_SIZE_RANGE.0, FONT_SIZE_RANGE.1)
    }

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
        assert_eq!(config.font_size(), DEFAULT_FONT_SIZE);
    }

    #[test]
    fn the_font_size_is_clamped_to_a_usable_range() {
        let mut config = Config::at(None);
        config.editor_font_size = Some(200.0);
        assert_eq!(config.font_size(), FONT_SIZE_RANGE.1);
        config.editor_font_size = Some(1.0);
        assert_eq!(config.font_size(), FONT_SIZE_RANGE.0);
    }
}
