//! Installing language plugins: a manifest names one `rules` asset — a JSON file of regex-based
//! highlight and symbol patterns, in the spirit of a VS Code TextMate grammar rather than a real
//! parser — fetched and checked against the SHA-256 the manifest declares, then stored under a
//! per-plugin directory.
mod fetch;
mod language;

use std::{fs, path::PathBuf};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

pub use language::{Language, Symbol};

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Asset {
    pub url: String,
    pub sha256: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Manifest {
    pub id: String,
    pub name: String,
    pub version: String,
    /// File extensions this plugin covers, without the dot (e.g. `"rs"`).
    pub extensions: Vec<String>,
    pub rules: Asset,
}

#[derive(Clone, Debug)]
pub struct InstalledPlugin {
    pub manifest: Manifest,
    pub rules_path: PathBuf,
}

pub struct PluginStore {
    dir: PathBuf,
}

impl PluginStore {
    /// A store rooted at `dir`; tests point this at a temp directory.
    pub fn at(dir: PathBuf) -> Self {
        Self { dir }
    }

    pub fn default_dir() -> Option<PathBuf> {
        madi_project::config::app_support_dir().map(|d| d.join("plugins"))
    }

    fn plugin_dir(&self, id: &str) -> PathBuf {
        self.dir.join(id)
    }

    fn read_installed(dir: PathBuf) -> Option<InstalledPlugin> {
        let manifest: Manifest = serde_json::from_str(&fs::read_to_string(dir.join("manifest.json")).ok()?).ok()?;
        Some(InstalledPlugin { rules_path: dir.join("rules.json"), manifest })
    }

    pub fn list(&self) -> Vec<InstalledPlugin> {
        let Ok(entries) = fs::read_dir(&self.dir) else { return Vec::new() };
        let mut out: Vec<InstalledPlugin> = entries
            .flatten()
            .filter(|e| e.file_type().is_ok_and(|t| t.is_dir()) && !e.file_name().to_string_lossy().starts_with('.'))
            .filter_map(|e| Self::read_installed(e.path()))
            .collect();
        out.sort_by(|a, b| a.manifest.name.cmp(&b.manifest.name));
        out
    }

    pub fn resolve_extension(&self, ext: &str) -> Option<InstalledPlugin> {
        self.list().into_iter().find(|p| p.manifest.extensions.iter().any(|e| e.eq_ignore_ascii_case(ext)))
    }

    /// Downloads the manifest at `manifest_url`, then its `rules` asset, verifying it against the
    /// SHA-256 the manifest declares, and installs them, replacing any existing installation of
    /// the same plugin id.
    pub fn install_from_url(&self, manifest_url: &str) -> Result<InstalledPlugin, String> {
        let manifest_bytes = fetch::fetch(manifest_url)?;
        let manifest: Manifest = serde_json::from_slice(&manifest_bytes).map_err(|e| format!("invalid plugin manifest: {e}"))?;
        if manifest.id.is_empty() || manifest.extensions.is_empty() {
            return Err("the plugin manifest is missing an id or extensions".to_string());
        }
        let rules = verified(&manifest.rules)?;
        // Fail before touching disk if the rules don't even parse as a language definition.
        language::Language::parse(&rules)?;

        fs::create_dir_all(&self.dir).map_err(|e| e.to_string())?;
        let tmp = self.dir.join(format!(".{}.tmp", manifest.id));
        let _ = fs::remove_dir_all(&tmp);
        fs::create_dir_all(&tmp).map_err(|e| e.to_string())?;
        fs::write(tmp.join("rules.json"), &rules).map_err(|e| e.to_string())?;
        fs::write(tmp.join("manifest.json"), &manifest_bytes).map_err(|e| e.to_string())?;

        let dir = self.plugin_dir(&manifest.id);
        let _ = fs::remove_dir_all(&dir);
        fs::rename(&tmp, &dir).map_err(|e| e.to_string())?;

        Ok(InstalledPlugin { rules_path: dir.join("rules.json"), manifest })
    }

    pub fn uninstall(&self, id: &str) -> Result<(), String> {
        let dir = self.plugin_dir(id);
        if !dir.exists() {
            return Err(format!("{id} is not installed"));
        }
        fs::remove_dir_all(&dir).map_err(|e| e.to_string())
    }
}

fn verified(asset: &Asset) -> Result<Vec<u8>, String> {
    let bytes = fetch::fetch(&asset.url)?;
    let digest = format!("{:x}", Sha256::digest(&bytes));
    if !digest.eq_ignore_ascii_case(&asset.sha256) {
        return Err(format!("checksum mismatch for {}: expected {}, got {digest}", asset.url, asset.sha256));
    }
    Ok(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sha256_hex(bytes: &[u8]) -> String {
        format!("{:x}", Sha256::digest(bytes))
    }

    const RULES: &str = r#"{
        "highlight": [
            {"scope": "comment", "pattern": "//.*"},
            {"scope": "string", "pattern": "\"([^\"\\\\]|\\\\.)*\""},
            {"scope": "keyword", "pattern": "\\b(fn|let)\\b"}
        ],
        "symbols": [
            {"kind": "function", "pattern": "fn\\s+([A-Za-z_][A-Za-z0-9_]*)"}
        ]
    }"#;

    /// A plugin catalog on disk (manifest + rules file), served back to the store as `file://` URLs.
    fn write_catalog(dir: &std::path::Path, id: &str, extensions: &[&str]) -> String {
        std::fs::create_dir_all(dir).unwrap();
        std::fs::write(dir.join("rules.json"), RULES).unwrap();
        let manifest = Manifest {
            id: id.to_string(),
            name: id.to_string(),
            version: "1.0.0".to_string(),
            extensions: extensions.iter().map(|s| s.to_string()).collect(),
            rules: Asset { url: format!("file://{}", dir.join("rules.json").display()), sha256: sha256_hex(RULES.as_bytes()) },
        };
        let manifest_path = dir.join("manifest.json");
        std::fs::write(&manifest_path, serde_json::to_vec(&manifest).unwrap()).unwrap();
        format!("file://{}", manifest_path.display())
    }

    fn store(name: &str) -> (PluginStore, PathBuf) {
        let dir = std::env::temp_dir().join(format!("madi-plugins-test-{name}"));
        let _ = std::fs::remove_dir_all(&dir);
        (PluginStore::at(dir.join("installed")), dir)
    }

    #[test]
    fn installs_lists_resolves_and_uninstalls_a_plugin() {
        let (store, dir) = store("roundtrip");
        let manifest_url = write_catalog(&dir.join("catalog/rust"), "rust", &["rs"]);

        let installed = store.install_from_url(&manifest_url).unwrap();
        assert_eq!(installed.manifest.id, "rust");
        assert_eq!(std::fs::read_to_string(&installed.rules_path).unwrap(), RULES);

        assert_eq!(store.list().len(), 1);
        assert!(store.resolve_extension("RS").is_some(), "extension matching ignores case");
        assert!(store.resolve_extension("py").is_none());

        store.uninstall("rust").unwrap();
        assert!(store.list().is_empty());
        assert!(store.uninstall("rust").is_err(), "uninstalling twice reports it isn't installed");
    }

    #[test]
    fn reinstalling_the_same_id_replaces_it() {
        let (store, dir) = store("reinstall");
        let url = write_catalog(&dir.join("catalog/rust"), "rust", &["rs"]);
        store.install_from_url(&url).unwrap();
        let url2 = write_catalog(&dir.join("catalog/rust2"), "rust", &["rs", "rlib"]);
        let installed = store.install_from_url(&url2).unwrap();
        assert_eq!(installed.manifest.extensions, vec!["rs", "rlib"]);
        assert_eq!(store.list().len(), 1, "the old install was replaced, not left alongside");
    }

    #[test]
    fn a_tampered_asset_fails_its_checksum() {
        let (store, dir) = store("checksum");
        let catalog = dir.join("catalog/rust");
        let url = write_catalog(&catalog, "rust", &["rs"]);
        std::fs::write(catalog.join("rules.json"), "tampered").unwrap();

        let err = store.install_from_url(&url).unwrap_err();
        assert!(err.contains("checksum mismatch"), "{err}");
        assert!(store.list().is_empty(), "a failed install leaves nothing behind");
    }

    #[test]
    fn a_missing_manifest_field_is_rejected() {
        let (store, dir) = store("missing-field");
        std::fs::create_dir_all(&dir).unwrap();
        let manifest_path = dir.join("bad.json");
        std::fs::write(&manifest_path, br#"{"id":"","name":"x","version":"1","extensions":[],"rules":{"url":"file:///x","sha256":"x"}}"#).unwrap();
        let err = store.install_from_url(&format!("file://{}", manifest_path.display())).unwrap_err();
        assert!(err.contains("id or extensions"), "{err}");
    }

    #[test]
    fn invalid_rules_are_rejected_before_anything_is_written() {
        let (store, dir) = store("bad-rules");
        let catalog = dir.join("catalog/rust");
        let url = write_catalog(&catalog, "rust", &["rs"]);
        std::fs::write(catalog.join("rules.json"), "not json").unwrap();
        // rewrite manifest's checksum to match the now-broken rules file so it downloads but fails to parse
        let broken = std::fs::read(catalog.join("rules.json")).unwrap();
        let manifest = Manifest {
            id: "rust".into(), name: "rust".into(), version: "1.0.0".into(), extensions: vec!["rs".into()],
            rules: Asset { url: format!("file://{}", catalog.join("rules.json").display()), sha256: sha256_hex(&broken) },
        };
        std::fs::write(catalog.join("manifest.json"), serde_json::to_vec(&manifest).unwrap()).unwrap();
        let _ = url;
        let err = store.install_from_url(&format!("file://{}", catalog.join("manifest.json").display())).unwrap_err();
        assert!(store.list().is_empty(), "{err}");
    }
}
