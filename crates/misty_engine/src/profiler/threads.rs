//! Timeline rows of threads other than the main one: rayon workers, the background pool and
//! the audio thread hand their finished top-level scopes to a shared buffer per thread, which
//! [`collect`] drains once per frame.

use std::borrow::Cow;
use std::cell::{Cell, RefCell};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use serde::{Deserialize, Serialize};

use super::{RawSample, Sample};

/// Samples a thread may hold between two frames before new ones are dropped.
const MAX_PENDING: usize = 50_000;

/// What a Timeline row is.
#[derive(
    Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize,
)]
pub enum ThreadKind {
    /// The thread that ends frames (game loop, editor UI and rendering).
    Main,
    /// The GPU queue (render passes from timestamp queries).
    Render,
    /// The mixer graph's audio callback.
    Audio,
    /// `background::pool()`: bakes and builds that take seconds.
    Background,
    /// rayon's global pool: the frame's parallel loops.
    Worker,
    /// Any other named thread.
    #[default]
    Other,
}

impl ThreadKind {
    pub fn label(self) -> &'static str {
        match self {
            Self::Main => "Main Thread",
            Self::Render => "Render (GPU)",
            Self::Audio => "Audio Thread",
            Self::Background => "Background",
            Self::Worker => "Worker",
            Self::Other => "Thread",
        }
    }
}

/// One thread's samples during a frame.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct ThreadTrack {
    pub name: String,
    pub kind: ThreadKind,
    /// Start relative to the frame's start (may be negative: began in an earlier frame).
    pub samples: Vec<Sample>,
}

struct Shared {
    name: String,
    kind: ThreadKind,
    samples: Vec<RawSample>,
}

static REGISTRY: Mutex<Vec<Arc<Mutex<Shared>>>> = Mutex::new(Vec::new());
/// Id of the thread that ends frames.
static MAIN: AtomicU64 = AtomicU64::new(u64::MAX);
static NEXT_ID: AtomicU64 = AtomicU64::new(1);

thread_local! {
    static ID: u64 = NEXT_ID.fetch_add(1, Ordering::Relaxed);
    static NAME: RefCell<Option<Cow<'static, str>>> = const { RefCell::new(None) };
    static SHARED: RefCell<Option<Arc<Mutex<Shared>>>> = const { RefCell::new(None) };
    static NAMED: Cell<bool> = const { Cell::new(false) };
}

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

/// Make the calling thread the Main Thread.
pub(super) fn claim_main() {
    MAIN.store(ID.with(|i| *i), Ordering::Relaxed);
}

pub(super) fn is_main() -> bool {
    ID.with(|i| *i) == MAIN.load(Ordering::Relaxed)
}

/// Give the calling thread a Timeline row name ("Audio Thread"); others are named from their
/// thread name or rayon index.
pub fn name_thread(name: impl Into<Cow<'static, str>>) {
    if NAMED.get() {
        return;
    }
    NAMED.set(true);
    let name = name.into();
    if let Some(s) = SHARED.with_borrow(|s| s.clone()) {
        let mut s = lock(&s);
        s.kind = kind_of(&name);
        s.name = name.to_string();
    }
    NAME.set(Some(name));
}

fn kind_of(name: &str) -> ThreadKind {
    if name.starts_with("Audio") {
        ThreadKind::Audio
    } else if name.starts_with("misty background") || name.starts_with("Background") {
        ThreadKind::Background
    } else if name.starts_with("Worker") {
        ThreadKind::Worker
    } else {
        ThreadKind::Other
    }
}

/// Row name of the calling thread.
fn row_name() -> (String, ThreadKind) {
    if let Some(n) = NAME.with_borrow(|n| n.clone()) {
        return (n.to_string(), kind_of(&n));
    }
    let current = std::thread::current();
    match current.name() {
        Some(n) if n.starts_with("misty background ") => {
            let i = n.trim_start_matches("misty background ");
            (format!("Background {i}"), ThreadKind::Background)
        }
        Some(n) => (n.to_owned(), kind_of(n)),
        None => match rayon::current_thread_index() {
            Some(i) => (format!("Worker {i}"), ThreadKind::Worker),
            None => (format!("Thread {:?}", current.id()), ThreadKind::Other),
        },
    }
}

/// Move a worker's finished samples into its shared buffer.
pub(super) fn flush(samples: &mut Vec<RawSample>) {
    if samples.is_empty() {
        return;
    }
    let shared = SHARED.with_borrow_mut(|s| {
        s.get_or_insert_with(|| {
            let (name, kind) = row_name();
            let shared = Arc::new(Mutex::new(Shared {
                name,
                kind,
                samples: Vec::new(),
            }));
            lock(&REGISTRY).push(shared.clone());
            shared
        })
        .clone()
    });
    let mut s = lock(&shared);
    let room = MAX_PENDING.saturating_sub(s.samples.len());
    let n = samples.len().min(room);
    s.samples.extend(samples.drain(..n));
    samples.clear();
}

/// Every other thread's samples since the last call, relative to `frame_start` (ns), one track
/// per thread that recorded something, sorted by kind and name.
pub(super) fn collect(frame_start: u64) -> Vec<ThreadTrack> {
    let registry: Vec<Arc<Mutex<Shared>>> = {
        let mut r = lock(&REGISTRY);
        // Threads that ended (only the registry holds them) and have nothing left.
        r.retain(|s| Arc::strong_count(s) > 1 || !lock(s).samples.is_empty());
        r.clone()
    };
    let enabled = super::enabled();
    let mut out: Vec<ThreadTrack> = Vec::new();
    for shared in registry {
        let mut s = lock(&shared);
        if s.samples.is_empty() {
            continue;
        }
        let raw = std::mem::take(&mut s.samples);
        if !enabled {
            continue;
        }
        out.push(ThreadTrack {
            name: s.name.clone(),
            kind: s.kind,
            samples: raw.into_iter().map(|r| r.relative(frame_start)).collect(),
        });
    }
    out.sort_by(|a, b| {
        a.kind
            .cmp(&b.kind)
            .then_with(|| natural_key(&a.name).cmp(&natural_key(&b.name)))
    });
    out
}

/// "Worker 10" after "Worker 9".
fn natural_key(name: &str) -> (String, u64) {
    let digits = name.trim_start_matches(|c: char| !c.is_ascii_digit());
    let n = digits.parse().unwrap_or(0);
    (name[..name.len() - digits.len()].to_owned(), n)
}
