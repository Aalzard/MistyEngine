//! Remote bundles: download over HTTPS (or read a `file://` URL), check the SHA-256 the catalog
//! recorded, and keep a copy in a local cache so the next run needs no download.

use std::path::{Path, PathBuf};
use std::time::Duration;

use sha2::{Digest, Sha256};

/// Bundles bigger than this are refused (a broken server must not fill the disk).
const MAX_BUNDLE: u64 = 2 << 30;

/// Lower-case hex SHA-256 of `bytes`.
pub fn sha256_hex(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    let mut s = String::with_capacity(64);
    for b in digest {
        s.push_str(&format!("{b:02x}"));
    }
    s
}

/// The bytes at `url`: `http://` / `https://` (via ureq) or `file://` (local path).
pub fn fetch(url: &str) -> Result<Vec<u8>, String> {
    if let Some(path) = url.strip_prefix("file://") {
        // file:///C:/x and file:///home/x both work.
        let path = if path.len() > 2 && path.starts_with('/') && path.as_bytes()[2] == b':' {
            &path[1..]
        } else {
            path
        };
        return std::fs::read(path).map_err(|e| format!("{url}: {e}"));
    }
    if !(url.starts_with("https://") || url.starts_with("http://")) {
        return Err(format!(
            "{url}: only https://, http:// and file:// URLs are supported"
        ));
    }
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .timeout_connect(Some(Duration::from_secs(10)))
        .timeout_global(Some(Duration::from_secs(600)))
        .user_agent(format!("MistyEngine/{}", env!("CARGO_PKG_VERSION")))
        .build()
        .into();
    let resp = agent.get(url).call().map_err(|e| format!("{url}: {e}"))?;
    resp.into_body()
        .with_config()
        .limit(MAX_BUNDLE)
        .read_to_vec()
        .map_err(|e| format!("{url}: {e}"))
}

/// Where downloaded bundles are kept (per user, outside the game folder).
pub fn default_cache_dir() -> PathBuf {
    let base = std::env::var_os("LOCALAPPDATA")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("XDG_CACHE_HOME").map(PathBuf::from))
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".cache")))
        .unwrap_or_else(std::env::temp_dir);
    base.join("MistyEngine").join("BundleCache")
}

/// The cache file for bundle `name` with `hash` (a new hash is a new file).
/// Only plain file-name characters of the catalog's name and hash are used, so the file stays
/// in `cache_dir` (and a hash with a multibyte character can't split it mid-character).
pub fn cache_path(cache_dir: &Path, name: &str, hash: &str) -> PathBuf {
    let plain = |s: &str| -> String {
        s.chars()
            .map(|c| {
                if c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.') {
                    c
                } else {
                    '_'
                }
            })
            .collect()
    };
    let short: String = plain(hash).chars().take(16).collect();
    cache_dir.join(format!("{short}_{}", plain(name)))
}

/// A verified local copy of the bundle at `url`: the cached one when its hash still matches,
/// else a fresh download (rejected when its hash differs from `hash`).
pub fn download_verified(
    url: &str,
    name: &str,
    hash: &str,
    cache_dir: &Path,
) -> Result<PathBuf, String> {
    let cached = cache_path(cache_dir, name, hash);
    if let Ok(bytes) = std::fs::read(&cached) {
        if sha256_hex(&bytes) == hash {
            return Ok(cached);
        }
        let _ = std::fs::remove_file(&cached);
    }
    let bytes = fetch(url)?;
    let got = sha256_hex(&bytes);
    if got != hash {
        return Err(format!(
            "{name}: hash mismatch (expected {hash}, downloaded {got}); bundle rejected"
        ));
    }
    std::fs::create_dir_all(cache_dir).map_err(|e| format!("{}: {e}", cache_dir.display()))?;
    // Write then rename, so a crash never leaves half a bundle under the final name.
    let tmp = cached.with_extension("part");
    std::fs::write(&tmp, &bytes).map_err(|e| format!("{}: {e}", tmp.display()))?;
    std::fs::rename(&tmp, &cached).map_err(|e| format!("{}: {e}", cached.display()))?;
    Ok(cached)
}
