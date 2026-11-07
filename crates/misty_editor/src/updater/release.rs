//! GitHub's "latest release" JSON and the choice of this platform's package in it
//! (`MistyEngine-vX.Y.Z-windows-x64.zip` on Windows, `…-linux-x64.tar.gz` on Linux x64).

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
    /// This platform's package (a `.zip` on Windows, a `.tar.gz` on Linux; the field keeps its
    /// old name for the cache file) and its checksum file; `None` when the release has none.
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

/// A platform the releases carry an in-place-installable package for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Platform {
    WindowsX64,
    LinuxX64,
}

impl Platform {
    /// The package this build installs (`None`: no package for this OS / CPU, so an update
    /// is offered with the release page only). Windows on ARM64 runs the x64 package.
    pub const HOST: Option<Platform> = if cfg!(windows) {
        Some(Platform::WindowsX64)
    } else if cfg!(all(target_os = "linux", target_arch = "x86_64")) {
        Some(Platform::LinuxX64)
    } else {
        None
    };

    /// End of the package name, after `MistyEngine-v<version>`.
    pub fn suffix(self) -> &'static str {
        match self {
            Platform::WindowsX64 => "-windows-x64.zip",
            Platform::LinuxX64 => "-linux-x64.tar.gz",
        }
    }

    /// For messages ("This release has no Linux package …").
    pub fn label(self) -> &'static str {
        match self {
            Platform::WindowsX64 => "Windows",
            Platform::LinuxX64 => "Linux",
        }
    }
}

/// The package name a release is expected to carry for `platform`.
pub fn package_name(version: &str, platform: Platform) -> String {
    format!("MistyEngine-v{version}{}", platform.suffix())
}

/// The platform's package: the exact expected name first, else any
/// `MistyEngine-*-windows-x64.zip` / `MistyEngine-*-linux-x64.tar.gz`.
pub fn select_package<'a>(
    assets: &'a [Asset],
    version: &str,
    platform: Platform,
) -> Option<&'a Asset> {
    let exact = package_name(version, platform).to_lowercase();
    assets
        .iter()
        .find(|a| a.name.to_lowercase() == exact)
        .or_else(|| {
            assets.iter().find(|a| {
                let n = a.name.to_lowercase();
                n.starts_with("mistyengine-") && n.ends_with(platform.suffix())
            })
        })
}

/// `<package>.sha256.txt` (also accepted: `<package>.sha256`, `<package stem>.sha256.txt`).
pub fn select_checksum<'a>(assets: &'a [Asset], package_name: &str) -> Option<&'a Asset> {
    let zip = package_name.to_lowercase();
    let stem = zip
        .strip_suffix(".tar.gz")
        .or_else(|| zip.strip_suffix(".zip"))
        .unwrap_or(&zip);
    let names = [
        format!("{zip}.sha256.txt"),
        format!("{zip}.sha256"),
        format!("{stem}.sha256.txt"),
    ];
    names
        .iter()
        .find_map(|want| assets.iter().find(|a| a.name.to_lowercase() == *want))
}

/// The update a release offers this build (`None` for drafts, pre-releases and unparsable tags).
pub fn to_update(r: &Release, fallback_page: &str) -> Option<Update> {
    to_update_for(r, fallback_page, Platform::HOST)
}

/// [`to_update`] with `platform`'s package (`None`: the release page only).
pub fn to_update_for(
    r: &Release,
    fallback_page: &str,
    platform: Option<Platform>,
) -> Option<Update> {
    if r.draft || r.prerelease {
        return None;
    }
    let v = Version::parse(&r.tag_name)?;
    let version = r.tag_name.trim().trim_start_matches(['v', 'V']).to_owned();
    debug_assert!(Version::parse(&version) == Some(v));
    let zip = platform.and_then(|p| select_package(&r.assets, &version, p));
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
