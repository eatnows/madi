//! Fetching a plugin asset. `file://` reads a local path (used by tests, and works for a
//! locally-curated catalog); `http(s)://` downloads it for real.
use std::io::Read;

pub fn fetch(url: &str) -> Result<Vec<u8>, String> {
    if let Some(path) = url.strip_prefix("file://") {
        return std::fs::read(path).map_err(|e| format!("can't read {path}: {e}"));
    }
    if url.starts_with("http://") || url.starts_with("https://") {
        let response = ureq::get(url).call().map_err(|e| format!("can't fetch {url}: {e}"))?;
        let mut bytes = Vec::new();
        response.into_reader().read_to_end(&mut bytes).map_err(|e| format!("can't read response from {url}: {e}"))?;
        return Ok(bytes);
    }
    Err(format!("unsupported URL scheme: {url}"))
}
