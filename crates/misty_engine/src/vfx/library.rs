//! Process-wide cache of compiled effects by project path, re-read when files change. The
//! Effect Graph window can put an unsaved working copy in place of a file (live preview).

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Instant, SystemTime};

use super::codegen::{CompiledEffect, GraphError, compile};
use super::graph::EffectGraph;
use crate::assets;

pub type Compiled = Result<Arc<CompiledEffect>, Vec<GraphError>>;

struct Entry {
    modified: Option<SystemTime>,
    /// Bumped on every (re)load.
    generation: u64,
    effect: Compiled,
    /// An unsaved graph from the editor is shown instead of the file.
    working_copy: bool,
    /// Sub-graph files the effect uses and their modification times (edits recompile it).
    deps: Vec<(String, Option<SystemTime>)>,
}

#[derive(Default)]
struct Library {
    entries: HashMap<String, Entry>,
    last_poll: Option<Instant>,
}

fn library() -> &'static Mutex<Library> {
    static LIB: OnceLock<Mutex<Library>> = OnceLock::new();
    LIB.get_or_init(Default::default)
}

fn lock() -> std::sync::MutexGuard<'static, Library> {
    library().lock().unwrap_or_else(|e| e.into_inner())
}

fn next_generation() -> u64 {
    static NEXT: AtomicU64 = AtomicU64::new(1);
    NEXT.fetch_add(1, Ordering::Relaxed)
}

fn mtime(rel: &str) -> Option<SystemTime> {
    std::fs::metadata(assets::absolute(rel))
        .and_then(|m| m.modified())
        .ok()
}

/// [`mtime`] as the background file watch last saw it.
fn cached_mtime(rel: &str) -> Option<SystemTime> {
    crate::file_watch::cached_modified(&assets::absolute(rel))
}

fn read(rel: &str) -> Entry {
    let mut deps = Vec::new();
    let effect = crate::vfs::read_to_string(assets::absolute(rel))
        .map_err(|e| {
            vec![GraphError {
                message: format!("could not read {rel}: {e}"),
                node: None,
            }]
        })
        .and_then(|text| {
            EffectGraph::from_json(&text).map_err(|m| {
                vec![GraphError {
                    message: m,
                    node: None,
                }]
            })
        })
        .and_then(|g| {
            deps = super::subgraph::dependencies(&g);
            compile(&g).map(Arc::new)
        });
    Entry {
        modified: mtime(rel),
        generation: next_generation(),
        effect,
        working_copy: false,
        deps,
    }
}

fn deps_changed(deps: &[(String, Option<SystemTime>)]) -> bool {
    deps.iter().any(|(p, t)| mtime(p) != *t)
}

/// The compiled effect at a project path, read on first use, and its load generation.
pub fn get(rel: &str) -> (u64, Compiled) {
    let mut lib = lock();
    let e = lib
        .entries
        .entry(rel.to_owned())
        .or_insert_with(|| read(rel));
    (e.generation, e.effect.clone())
}

/// Load generation of an effect (0 = never loaded).
pub fn generation(rel: &str) -> u64 {
    lock().entries.get(rel).map_or(0, |e| e.generation)
}

/// Re-read effects whose files changed (at most twice a second). Returns the changed paths.
pub fn poll_changes() -> Vec<String> {
    let mut lib = lock();
    let now = Instant::now();
    if lib
        .last_poll
        .is_some_and(|t| now.duration_since(t).as_secs_f32() < 0.5)
    {
        return Vec::new();
    }
    lib.last_poll = Some(now);
    let mut changed = Vec::new();
    for (rel, entry) in lib.entries.iter_mut() {
        // The background file watch's stamps first; a difference is checked on disk.
        let cached_same = entry.modified == cached_mtime(rel)
            && entry.deps.iter().all(|(p, t)| cached_mtime(p) == *t);
        if cached_same {
            continue;
        }
        if !entry.working_copy && (entry.modified != mtime(rel) || deps_changed(&entry.deps)) {
            *entry = read(rel);
            changed.push(rel.clone());
        }
    }
    changed
}

/// Effects in the cache that use the sub-graph at `rel`.
pub fn users_of(rel: &str) -> Vec<String> {
    lock()
        .entries
        .iter()
        .filter(|(_, e)| e.deps.iter().any(|(p, _)| p == rel))
        .map(|(k, _)| k.clone())
        .collect()
}

/// Re-read one effect now (after saving it); drops a working copy.
pub fn reload(rel: &str) {
    lock().entries.insert(rel.to_owned(), read(rel));
}

/// Show an unsaved graph for `rel` everywhere it is used. Unchanged graphs keep their
/// generation (no recompile, no restart).
pub fn set_working_copy(rel: &str, graph: &EffectGraph) {
    let compiled = compile(graph).map(Arc::new);
    let mut lib = lock();
    let same = lib
        .entries
        .get(rel)
        .is_some_and(|e| match (&e.effect, &compiled) {
            (Ok(a), Ok(b)) => {
                a.hash == b.hash && a.spawn == b.spawn && a.properties == b.properties
            }
            (Err(a), Err(b)) => a == b,
            _ => false,
        });
    if same {
        if let Some(e) = lib.entries.get_mut(rel) {
            e.working_copy = true;
        }
        return;
    }
    lib.entries.insert(
        rel.to_owned(),
        Entry {
            modified: mtime(rel),
            generation: next_generation(),
            effect: compiled,
            working_copy: true,
            deps: super::subgraph::dependencies(graph),
        },
    );
}

/// Go back to the file's contents (the window closed or switched graphs without saving).
pub fn clear_working_copy(rel: &str) {
    let has = lock().entries.get(rel).is_some_and(|e| e.working_copy);
    if has {
        reload(rel);
    }
}
