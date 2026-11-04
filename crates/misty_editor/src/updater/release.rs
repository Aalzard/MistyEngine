//! GitHub's "latest release" JSON and the choice of the Windows package in it.

use serde::{Deserialize, Serialize};

use super::version::Version;

/// The fields of `GET /repos/{owner}/{repo}/releases/latest` the updater reads.
#[derive(Clone, Debug, Deserialize)]
pub struct Release {
    pub tag_name: String,
    #[serde(default)]
    pub body: Option<String>,
    #[serde(default)]
    pub html_url: Option<String>,
    #[serde(default)]
    pub draft: bool,
    #[serde(default)]
    pub prerelease: bool,
    #[serde(default)]
    pub assets: Vec<Asset>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct Asset {
    pub name: String,
    pub browser_download_url: String,
    #[serde(default)]
    pub size: u64,
}

/// A newer version the user can install (also what the cache file remembers).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Update {
    /// Without the `v` ("0.0.2").
    pub version: String,
    /// Release notes (shortened).
    pub notes: String,
    /// The release's web page (the "Download the new version" fallback).
    pub page_url: String,
    /// The Windows package and its checksum file; `None` when the release has none.
    pub zip: Option<Download>,
    pub checksum_url: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Download {
    pub name: String,
    pub url: String,
    pub size: u64,
}

/// Longest release notes kept (the message box scrolls; the page has the rest).
const MAX_NOTES: usize = 4000;

pub fn parse(json: &str) -> Result<Release, String> {
    serde_json::from_str(json).map_err(|e| format!("bad release JSON: {e}"))
}

/// The package name a release is expected to carry.
pub fn package_name(version: &str) -> String {
    format!("MistyEngine-v{version}-windows-x64.zip")
}

/// The Windows package: the exact expected name first, else any `MistyEngine-*-windows-x64.zip`.
pub fn select_zip<'a>(assets: &'a [Asset], version: &str) -> Option<&'a Asset> {
    let exact = package_name(version).to_lowercase();
    assets
        .iter()
        .find(|a| a.name.to_lowercase() == exact)
        .or_else(|| {
            assets.iter().find(|a| {
                let n = a.name.to_lowercase();
                n.starts_with("mistyengine-") && n.ends_with("-windows-x64.zip")
            })
        })
}

/// `<zip>.sha256.txt` (also accepted: `<zip>.sha256`, `<zip stem>.sha256.txt`).
pub fn select_checksum<'a>(assets: &'a [Asset], zip_name: &str) -> Option<&'a Asset> {
    let zip = zip_name.to_lowercase();
    let stem = zip.trim_end_matches(".zip");
    let names = [
        format!("{zip}.sha256.txt"),
        format!("{zip}.sha256"),
        format!("{stem}.sha256.txt"),
    ];
    names
        .iter()
        .find_map(|want| assets.iter().find(|a| a.name.to_lowercase() == *want))
}

/// The update a release offers (`None` for drafts, pre-releases and unparsable tags).
pub fn to_update(r: &Release, fallback_page: &str) -> Option<Update> {
    if r.draft || r.prerelease {
        return None;
    }
    let v = Version::parse(&r.tag_name)?;
    let version = r.tag_name.trim().trim_start_matches(['v', 'V']).to_owned();
    debug_assert!(Version::parse(&version) == Some(v));
    let zip = select_zip(&r.assets, &version);
    let checksum_url = zip
        .and_then(|z| select_checksum(&r.assets, &z.name))
        .map(|a| a.browser_download_url.clone());
    Some(Update {
        notes: short_notes(r.body.as_deref().unwrap_or("")),
        page_url: r
            .html_url
            .clone()
            .unwrap_or_else(|| fallback_page.to_owned()),
        zip: zip.map(|a| Download {
            name: a.name.clone(),
            url: a.browser_download_url.clone(),
            size: a.size,
        }),
        checksum_url,
        version,
    })
}

fn short_notes(body: &str) -> String {
    let body = body.trim().replace("\r\n", "\n");
    if body.chars().count() <= MAX_NOTES {
        return body;
    }
    let cut: String = body.chars().take(MAX_NOTES).collect();
    format!("{cut}…")
}
