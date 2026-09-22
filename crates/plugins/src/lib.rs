//! Installing language plugins: a manifest names a grammar and a highlight-query file, each
//! fetched and checked against a SHA-256 the manifest declares, then stored under a per-plugin
//! directory. No parsing or highlighting yet — this crate only gets the files onto disk, verified;
//! turning them into colored text is the next step, once a plugin is actually installed by someone.
mod fetch;

use std::{fs, path::PathBuf};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

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
    pub grammar: Asset,
    pub highlights: Asset,
}

#[derive(Clone, Debug)]
pub struct InstalledPlugin {
    pub manifest: Manifest,
    pub grammar_path: PathBuf,
    pub highlights_path: PathBuf,
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
        Some(InstalledPlugin { grammar_path: dir.join("grammar.bin"), highlights_path: dir.join("highlights.scm"), manifest })
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

    /// Downloads the manifest at `manifest_url`, then its grammar and highlight-query assets,
    /// verifying each against the SHA-256 the manifest declares, and installs them, replacing any
    /// existing installation of the same plugin id.
    pub fn install_from_url(&self, manifest_url: &str) -> Result<InstalledPlugin, String> {
        let manifest_bytes = fetch::fetch(manifest_url)?;
        let manifest: Manifest = serde_json::from_slice(&manifest_bytes).map_err(|e| format!("invalid plugin manifest: {e}"))?;
        if manifest.id.is_empty() || manifest.extensions.is_empty() {
            return Err("the plugin manifest is missing an id or extensions".to_string());
        }
        let grammar = verified(&manifest.grammar)?;
        let highlights = verified(&manifest.highlights)?;

        fs::create_dir_all(&self.dir).map_err(|e| e.to_string())?;
        let tmp = self.dir.join(format!(".{}.tmp", manifest.id));
        let _ = fs::remove_dir_all(&tmp);
        fs::create_dir_all(&tmp).map_err(|e| e.to_string())?;
        fs::write(tmp.join("grammar.bin"), &grammar).map_err(|e| e.to_string())?;
        fs::write(tmp.join("highlights.scm"), &highlights).map_err(|e| e.to_string())?;
        fs::write(tmp.join("manifest.json"), &manifest_bytes).map_err(|e| e.to_string())?;

        let dir = self.plugin_dir(&manifest.id);
        let _ = fs::remove_dir_all(&dir);
        fs::rename(&tmp, &dir).map_err(|e| e.to_string())?;

        Ok(InstalledPlugin { grammar_path: dir.join("grammar.bin"), highlights_path: dir.join("highlights.scm"), manifest })
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

    /// A plugin catalog on disk (manifest + two assets), served back to the store as `file://` URLs.
    fn write_catalog(dir: &std::path::Path, id: &str, extensions: &[&str]) -> String {
        std::fs::create_dir_all(dir).unwrap();
        let grammar = b"fake grammar bytes".to_vec();
        let highlights = b"(identifier) @variable".to_vec();
        std::fs::write(dir.join("grammar.bin"), &grammar).unwrap();
        std::fs::write(dir.join("highlights.scm"), &highlights).unwrap();
        let manifest = Manifest {
            id: id.to_string(),
            name: id.to_string(),
            version: "1.0.0".to_string(),
            extensions: extensions.iter().map(|s| s.to_string()).collect(),
            grammar: Asset { url: format!("file://{}", dir.join("grammar.bin").display()), sha256: sha256_hex(&grammar) },
            highlights: Asset { url: format!("file://{}", dir.join("highlights.scm").display()), sha256: sha256_hex(&highlights) },
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
        assert_eq!(std::fs::read_to_string(&installed.highlights_path).unwrap(), "(identifier) @variable");

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
        std::fs::write(catalog.join("grammar.bin"), b"tampered").unwrap();

        let err = store.install_from_url(&url).unwrap_err();
        assert!(err.contains("checksum mismatch"), "{err}");
        assert!(store.list().is_empty(), "a failed install leaves nothing behind");
    }

    #[test]
    fn a_missing_manifest_field_is_rejected() {
        let (store, dir) = store("missing-field");
        std::fs::create_dir_all(&dir).unwrap();
        let manifest_path = dir.join("bad.json");
        std::fs::write(&manifest_path, br#"{"id":"","name":"x","version":"1","extensions":[],"grammar":{"url":"file:///x","sha256":"x"},"highlights":{"url":"file:///x","sha256":"x"}}"#).unwrap();
        let err = store.install_from_url(&format!("file://{}", manifest_path.display())).unwrap_err();
        assert!(err.contains("id or extensions"), "{err}");
    }
}
