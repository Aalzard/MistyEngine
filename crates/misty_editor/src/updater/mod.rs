//! In-app updater: checks GitHub Releases for a newer Misty Engine, offers it in a small message
//! box and a green "⬆ Update" button, and installs it in place (download, SHA-256 check, extract,
//! swap files, relaunch). How to publish a release: docs/release.md.
//!
//! A host (editor, Hub) owns one [`Updater`]: call [`Updater::start`] once, draw
//! [`ui::update_button`] somewhere visible and call [`ui::windows`] every frame. Both report
//! when the user wants to install; the host then calls [`Updater::install`] (the editor asks to
//! save modified scenes first). Help ▸ Check for Updates… calls [`Updater::check_now`].
//!
//! Environment: `MISTY_UPDATE_REPO=owner/name` follows another repository,
//! `MISTY_UPDATE_URL=<url>` reads a release JSON from anywhere (tests), `MISTY_UPDATER_DIR`
//! moves updater.json, and `MISTY_FAKE_UPDATE=0.0.2` pretends that version is out (screenshots;
//! no network).

pub mod cache;
pub mod editor;
pub mod http;
pub mod install;
pub mod release;
#[cfg(test)]
mod tests;
pub mod ui;
pub mod version;

use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use cache::Cache;
use http::{Fetched, Source};
pub use release::Update;

/// The GitHub repository updates come from ("owner/name"). If the repository moves, change this
/// one constant (or set `MISTY_UPDATE_REPO`). A placeholder such as "OWNER/MistyEngine" turns
/// the updater off silently. See docs/release.md.
pub const REPO: &str = "Aalzard/MistyEngine";
/// The running version (compared with the release tag `vX.Y.Z`).
pub const VERSION: &str = crate::branding::VERSION;

/// Where an install is.
#[derive(Clone, Debug, PartialEq)]
pub enum Phase {
    Idle,
    Checking,
    Downloading {
        received: u64,
        total: Option<u64>,
    },
    Verifying,
    Installing,
    /// Installed and the new exe started: the host window closes.
    Restarting,
    /// Installing in place isn't possible; offer the release page.
    Manual(String),
    Failed(String),
}

impl Phase {
    /// Download / install in progress (the progress window shows).
    pub fn busy(&self) -> bool {
        matches!(
            self,
            Phase::Downloading { .. } | Phase::Verifying | Phase::Installing | Phase::Restarting
        )
    }
}

/// State shared with the background threads.
#[derive(Debug)]
struct Shared {
    phase: Phase,
    update: Option<Update>,
    /// A new update to show the message box for (set by checks, taken by the UI).
    announce: bool,
    /// Result line of a manual check ("You're up to date.").
    notice: Option<String>,
}

pub struct Updater {
    source: Option<Source>,
    cache_path: Option<PathBuf>,
    /// The running executable (its folder is the install folder).
    exe: Option<PathBuf>,
    shared: Arc<Mutex<Shared>>,
    /// The message box is open.
    pub show_box: bool,
    /// Pretend update (`MISTY_FAKE_UPDATE`): no network, no install.
    fake: bool,
    /// Edit ▸ Preferences ▸ Check for updates at startup (on by default). Off, [`Updater::start`]
    /// makes no request; Help ▸ Check for Updates… still does.
    pub check_at_startup: bool,
    ctx: Option<egui::Context>,
}

impl Default for Updater {
    fn default() -> Self {
        Self::disabled()
    }
}

impl Updater {
    /// Updates off (tests, and the default before the editor starts the real one).
    pub fn disabled() -> Self {
        Self::with(None, None, None)
    }

    /// Source, cache file and running exe from the environment.
    pub fn from_env() -> Self {
        let mut u = Self::with(
            Source::from_env(),
            cache::path(),
            std::env::current_exe().ok(),
        );
        u.check_at_startup = crate::hub::config::check_updates_at_startup();
        if let Ok(v) = std::env::var("MISTY_FAKE_UPDATE")
            && version::Version::parse(&v).is_some()
        {
            u.set_fake(v.trim().trim_start_matches('v'));
        }
        u
    }

    pub fn with(source: Option<Source>, cache_path: Option<PathBuf>, exe: Option<PathBuf>) -> Self {
        Self {
            source,
            cache_path,
            exe,
            shared: Arc::new(Mutex::new(Shared {
                phase: Phase::Idle,
                update: None,
                announce: false,
                notice: None,
            })),
            show_box: false,
            fake: false,
            check_at_startup: true,
            ctx: None,
        }
    }

    /// Pretend `version` is available (screenshots / UI tests).
    pub fn set_fake(&mut self, version: &str) {
        self.fake = true;
        let page = self.source.as_ref().map_or_else(
            || format!("https://github.com/{REPO}/releases"),
            |s| s.page_url.clone(),
        );
        let mut s = self.lock();
        s.update = Some(Update {
            version: version.to_owned(),
            notes: "• Misty Hub: a launcher with templates and recent projects\n\
                    • Faster terrain sculpting and painting\n\
                    • Light halos and lens flares\n\
                    • Many fixes in the Properties panel and Editor Scene"
                .into(),
            page_url: page,
            zip: None,
            checksum_url: None,
        });
        s.announce = true;
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Shared> {
        self.shared.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// On startup: clean up after a previous update and check (at most every 6 hours, and only
    /// while "Check for updates at startup" is on).
    pub fn start(&mut self, ctx: &egui::Context) {
        self.ctx = Some(ctx.clone());
        // Automated captures and tests never touch the network or the install folder.
        if self.fake || cfg!(test) || std::env::var_os("MISTY_SCREENSHOT").is_some() {
            return;
        }
        if let Some(dir) = self.install_dir() {
            std::thread::spawn(move || {
                // The previous exe may still be closing; retry for a few seconds.
                for _ in 0..20 {
                    if install::cleanup_old(&dir) {
                        break;
                    }
                    std::thread::sleep(std::time::Duration::from_millis(500));
                }
            });
        }
        self.startup_check();
    }

    /// The automatic check: nothing at all (no request, no cached offer) when the user turned
    /// "Check for updates at startup" off. Returns whether a check was asked for.
    pub fn startup_check(&mut self) -> bool {
        if !self.check_at_startup {
            return false;
        }
        self.check(false);
        true
    }

    /// Change and save the "Check for updates at startup" preference.
    pub fn set_check_at_startup(&mut self, on: bool) -> Result<(), String> {
        self.check_at_startup = on;
        crate::hub::config::set_check_updates_at_startup(on)
    }

    /// Help ▸ Check for Updates…: always asks GitHub and reports the result (also when the
    /// startup check is turned off).
    pub fn check_now(&mut self) {
        if self.fake {
            self.lock().announce = true;
            return;
        }
        if self.source.is_none() {
            self.lock().notice = Some("Updates are not set up for this build.".into());
            return;
        }
        self.check(true);
    }

    /// Check in the background; `manual` skips the 6-hour cache and the skipped version.
    pub fn check(&mut self, manual: bool) {
        let (Some(src), Some(cache_path)) = (self.source.clone(), self.cache_path.clone()) else {
            return;
        };
        {
            let mut s = self.lock();
            if s.phase.busy() || s.phase == Phase::Checking {
                return;
            }
            let cached = Cache::load(&cache_path);
            if !manual && cached.fresh(cache::now()) {
                apply_result(&mut s, cached.latest.as_ref(), &cached, false);
                return;
            }
            s.phase = Phase::Checking;
        }
        let shared = self.shared.clone();
        let ctx = self.ctx.clone();
        std::thread::spawn(move || {
            let result = http::fetch_release(&src);
            let mut cached = Cache::load(&cache_path);
            let mut s = shared.lock().unwrap_or_else(|e| e.into_inner());
            s.phase = Phase::Idle;
            match result {
                Ok(fetched) => {
                    let latest = match fetched {
                        Fetched::Release(r) => release::to_update(&r, &src.page_url),
                        Fetched::NoRelease => None,
                    };
                    cached.last_check = cache::now();
                    cached.latest = latest.clone();
                    let _ = cached.save(&cache_path);
                    apply_result(&mut s, latest.as_ref(), &cached, manual);
                }
                // Offline or GitHub unhappy: silent unless the user asked.
                Err(e) if manual => {
                    s.notice = Some(format!("Couldn't check for updates ({e})."));
                }
                Err(_) => {}
            }
            drop(s);
            if let Some(ctx) = ctx {
                ctx.request_repaint();
            }
        });
    }

    /// The update on offer, if any.
    pub fn available(&self) -> Option<Update> {
        self.lock().update.clone()
    }

    pub fn phase(&self) -> Phase {
        self.lock().phase.clone()
    }

    /// The new exe was started and this window should close.
    pub fn restarting(&self) -> bool {
        self.lock().phase == Phase::Restarting
    }

    /// "Later": close the message box, keep the button.
    pub fn later(&mut self) {
        self.show_box = false;
    }

    /// "Skip this version": no box or button until a newer version comes out.
    pub fn skip(&mut self) {
        self.show_box = false;
        let Some(update) = self.lock().update.take() else {
            return;
        };
        if let Some(path) = &self.cache_path
            && !self.fake
        {
            let mut c = Cache::load(path);
            c.skipped = Some(update.version);
            let _ = c.save(path);
        }
    }

    /// Download and install the update in the background, then relaunch.
    pub fn install(&mut self) {
        let Some(update) = self.available() else {
            return;
        };
        self.show_box = false;
        let (Some(src), Some(exe), Some(dir)) =
            (self.source.clone(), self.exe.clone(), self.install_dir())
        else {
            self.lock().phase = Phase::Manual(
                "Download the new version from the release page and install it.".into(),
            );
            return;
        };
        if self.fake {
            self.lock().phase = Phase::Manual(
                "This is a simulated update (MISTY_FAKE_UPDATE); nothing to install.".into(),
            );
            return;
        }
        {
            let mut s = self.lock();
            if s.phase.busy() {
                return;
            }
            s.phase = Phase::Downloading {
                received: 0,
                total: None,
            };
        }
        let shared = self.shared.clone();
        let ctx = self.ctx.clone();
        std::thread::spawn(move || {
            let set = |p: Phase| {
                shared.lock().unwrap_or_else(|e| e.into_inner()).phase = p;
                if let Some(ctx) = &ctx {
                    ctx.request_repaint();
                }
            };
            let work = std::env::temp_dir().join(format!(
                "MistyEngine-update-{}-{}",
                update.version,
                std::process::id()
            ));
            let phase = match install::run(&src, &update, &dir, &exe, &work, &set) {
                Ok(install::Outcome::Installed(new_exe)) => match relaunch(&new_exe) {
                    Ok(()) => Phase::Restarting,
                    Err(e) => Phase::Failed(format!(
                        "The update was installed but {} could not be started ({e}). \
                         Start Misty Engine again to use the new version.",
                        new_exe.display()
                    )),
                },
                Ok(install::Outcome::Manual(why)) => Phase::Manual(why),
                Err(e) => Phase::Failed(e),
            };
            set(phase);
        });
    }

    /// Close the progress / result window (after a failure or the manual-download offer).
    pub fn dismiss(&mut self) {
        let mut s = self.lock();
        if !s.phase.busy() {
            s.phase = Phase::Idle;
        }
        s.notice = None;
    }

    /// Take the "show the message box" request from a finished check.
    fn take_announce(&mut self) -> bool {
        std::mem::take(&mut self.lock().announce)
    }

    fn notice(&self) -> Option<String> {
        self.lock().notice.clone()
    }

    /// The releases page (fallback link).
    pub fn page_url(&self) -> String {
        self.available().map(|u| u.page_url).unwrap_or_else(|| {
            self.source.as_ref().map_or_else(
                || format!("https://github.com/{REPO}/releases"),
                |s| s.page_url.clone(),
            )
        })
    }

    fn install_dir(&self) -> Option<PathBuf> {
        self.exe.as_ref()?.parent().map(PathBuf::from)
    }
}

/// Record a check's result: offer `latest` when it's newer and not skipped.
fn apply_result(s: &mut Shared, latest: Option<&Update>, cached: &Cache, manual: bool) {
    match latest.filter(|u| cache::offer(u, VERSION, cached.skipped.as_deref(), manual)) {
        Some(u) => {
            s.update = Some(u.clone());
            s.announce = true;
        }
        None => {
            s.update = None;
            if manual {
                s.notice = Some(format!("Misty Engine {VERSION} is the latest version."));
            }
        }
    }
}

/// Start the installed exe with this process's arguments (only ever the exe in the install
/// folder that the package just placed).
fn relaunch(exe: &std::path::Path) -> std::io::Result<()> {
    let args: Vec<std::ffi::OsString> = std::env::args_os().skip(1).collect();
    let mut cmd = std::process::Command::new(exe);
    cmd.args(args);
    if let Ok(dir) = std::env::current_dir() {
        cmd.current_dir(dir);
    }
    cmd.spawn().map(|_| ())
}
