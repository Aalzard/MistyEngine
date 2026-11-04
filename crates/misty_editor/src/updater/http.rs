//! Where updates come from and the HTTPS requests to get them. Every URL (including each
//! redirect hop) must be HTTPS on an allowed host; plain HTTP is only accepted for a loopback
//! `MISTY_UPDATE_URL` (the local test server).

use std::io::{Read, Write};
use std::path::Path;
use std::time::Duration;

use sha2::{Digest, Sha256};

use super::release::{self, Release};

/// Hosts GitHub serves releases, release assets and their redirects from.
pub const GITHUB_HOSTS: &[&str] = &[
    "api.github.com",
    "github.com",
    "objects.githubusercontent.com",
    "release-assets.githubusercontent.com",
];

/// Largest release JSON / checksum file read.
const MAX_TEXT: u64 = 2 << 20;
/// Largest package downloaded.
const MAX_PACKAGE: u64 = 4 << 30;

#[derive(Clone, Debug, PartialEq)]
pub struct Source {
    /// The latest-release JSON.
    pub api_url: String,
    /// The releases page (fallback when installing in place is not possible).
    pub page_url: String,
    pub allowed_hosts: Vec<String>,
    /// Plain HTTP allowed (loopback test servers only).
    pub allow_http: bool,
}

impl Source {
    /// `MISTY_UPDATE_URL` (a JSON shaped like GitHub's latest release), else the GitHub repo
    /// from `MISTY_UPDATE_REPO` or [`super::REPO`]. `None` = updates are off.
    pub fn from_env() -> Option<Self> {
        let var = |k| std::env::var(k).ok().filter(|v| !v.trim().is_empty());
        if let Some(url) = var("MISTY_UPDATE_URL") {
            return Self::custom(url.trim());
        }
        Self::github(var("MISTY_UPDATE_REPO").as_deref().unwrap_or(super::REPO))
    }

    /// `owner/name` on GitHub; placeholders ("OWNER/...") and malformed names give `None`.
    pub fn github(repo: &str) -> Option<Self> {
        let repo = repo.trim().trim_matches('/');
        let (owner, name) = repo.split_once('/')?;
        let ok = |s: &str| {
            !s.is_empty()
                && s.bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b"-_.".contains(&b))
        };
        if !ok(owner) || !ok(name) || owner.eq_ignore_ascii_case("OWNER") {
            return None;
        }
        Some(Self {
            api_url: format!("https://api.github.com/repos/{owner}/{name}/releases/latest"),
            page_url: format!("https://github.com/{owner}/{name}/releases"),
            allowed_hosts: GITHUB_HOSTS.iter().map(|h| (*h).to_owned()).collect(),
            allow_http: false,
        })
    }

    /// A custom release JSON: its own host is allowed besides GitHub's.
    pub fn custom(url: &str) -> Option<Self> {
        let host = host(url)?.to_owned();
        let mut allowed_hosts: Vec<String> = GITHUB_HOSTS.iter().map(|h| (*h).to_owned()).collect();
        let allow_http = is_loopback(&host);
        allowed_hosts.push(host);
        Some(Self {
            api_url: url.to_owned(),
            page_url: url.to_owned(),
            allowed_hosts,
            allow_http,
        })
    }

    /// Ok when `url` may be fetched from this source.
    pub fn check_url(&self, url: &str) -> Result<(), String> {
        let lower = url.to_ascii_lowercase();
        let h = host(url);
        let scheme_ok = lower.starts_with("https://")
            || (self.allow_http && lower.starts_with("http://") && h.is_some_and(is_loopback));
        let host_ok =
            h.is_some_and(|h| self.allowed_hosts.iter().any(|a| a.eq_ignore_ascii_case(h)));
        if scheme_ok && host_ok {
            Ok(())
        } else {
            Err(format!("refusing to download from {url}"))
        }
    }
}

/// The host of an http(s) URL, without user info or port.
pub fn host(url: &str) -> Option<&str> {
    let (scheme, rest) = url.split_once("://")?;
    if !scheme.eq_ignore_ascii_case("https") && !scheme.eq_ignore_ascii_case("http") {
        return None;
    }
    let authority = rest.split(['/', '?', '#']).next()?;
    let authority = authority.rsplit('@').next()?;
    let h = if let Some(v6) = authority.strip_prefix('[') {
        v6.split(']').next()?
    } else {
        authority.split(':').next()?
    };
    (!h.is_empty()).then_some(h)
}

fn is_loopback(host: &str) -> bool {
    matches!(host, "127.0.0.1" | "localhost" | "::1")
}

/// What a check found.
#[derive(Debug)]
pub enum Fetched {
    Release(Box<Release>),
    /// 404: the repository has no published release yet (or is private) — not an error.
    NoRelease,
}

fn agent(timeout: Duration) -> ureq::Agent {
    ureq::Agent::config_builder()
        .timeout_connect(Some(Duration::from_secs(8)))
        .timeout_global(Some(timeout))
        // Redirects are followed by hand so each hop's host is checked.
        .max_redirects(0)
        .http_status_as_error(false)
        .user_agent(format!("MistyEngine-Updater/{}", super::VERSION))
        .build()
        .into()
}

/// GET with checked redirects; returns the first non-redirect response.
fn get(
    src: &Source,
    url: &str,
    accept: &str,
    timeout: Duration,
) -> Result<ureq::http::Response<ureq::Body>, String> {
    let agent = agent(timeout);
    let mut url = url.to_owned();
    for _ in 0..6 {
        src.check_url(&url)?;
        let resp = agent
            .get(&url)
            .header("Accept", accept)
            .call()
            .map_err(|e| format!("request failed: {e}"))?;
        if !resp.status().is_redirection() {
            return Ok(resp);
        }
        let next = resp
            .headers()
            .get("location")
            .and_then(|v| v.to_str().ok())
            .ok_or("redirect without a location")?;
        url = if next.starts_with('/') {
            // Same-origin redirect: keep scheme://host[:port].
            let origin_end = url.find("://").map_or(0, |i| i + 3);
            let path_start = url[origin_end..]
                .find('/')
                .map_or(url.len(), |i| origin_end + i);
            format!("{}{next}", &url[..path_start])
        } else {
            next.to_owned()
        };
    }
    Err("too many redirects".into())
}

fn read_text(resp: ureq::http::Response<ureq::Body>) -> Result<String, String> {
    resp.into_body()
        .with_config()
        .limit(MAX_TEXT)
        .read_to_string()
        .map_err(|e| format!("read failed: {e}"))
}

/// The latest release (short timeouts; callers treat errors as "offline").
pub fn fetch_release(src: &Source) -> Result<Fetched, String> {
    let resp = get(
        src,
        &src.api_url,
        "application/vnd.github+json",
        Duration::from_secs(12),
    )?;
    match resp.status().as_u16() {
        200 => Ok(Fetched::Release(Box::new(release::parse(&read_text(
            resp,
        )?)?))),
        404 => Ok(Fetched::NoRelease),
        s => Err(format!("HTTP {s}")),
    }
}

/// A small text asset (the checksum file).
pub fn fetch_text(src: &Source, url: &str) -> Result<String, String> {
    let resp = get(
        src,
        url,
        "application/octet-stream",
        Duration::from_secs(30),
    )?;
    match resp.status().as_u16() {
        200 => read_text(resp),
        s => Err(format!("HTTP {s} for {url}")),
    }
}

/// Download `url` to `dest`, reporting (bytes so far, total if known); returns its SHA-256.
pub fn download(
    src: &Source,
    url: &str,
    dest: &Path,
    mut progress: impl FnMut(u64, Option<u64>),
) -> Result<String, String> {
    let resp = get(
        src,
        url,
        "application/octet-stream",
        Duration::from_secs(60 * 60),
    )?;
    let status = resp.status().as_u16();
    if status != 200 {
        return Err(format!("HTTP {status} for {url}"));
    }
    let total = resp
        .headers()
        .get("content-length")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.parse::<u64>().ok());
    let mut reader = resp.into_body().into_reader();
    let mut file = std::fs::File::create(dest).map_err(|e| format!("{}: {e}", dest.display()))?;
    let mut hasher = Sha256::new();
    let mut buf = vec![0u8; 64 * 1024];
    let mut got = 0u64;
    progress(0, total);
    loop {
        let n = reader
            .read(&mut buf)
            .map_err(|e| format!("download failed: {e}"))?;
        if n == 0 {
            break;
        }
        got += n as u64;
        if got > MAX_PACKAGE {
            return Err("download too large".into());
        }
        hasher.update(&buf[..n]);
        file.write_all(&buf[..n])
            .map_err(|e| format!("{}: {e}", dest.display()))?;
        progress(got, total);
    }
    if total.is_some_and(|t| t != got) {
        return Err("download ended early".into());
    }
    file.flush().map_err(|e| e.to_string())?;
    Ok(super::install::hex(&hasher.finalize()))
}
