//! Process-wide cache of shader assets by project path, re-read when files change. A Shader
//! Graph's entry also follows the Sub Graphs and Custom Function files it reads, and keeps the
//! graph so each keyword variant is generated on first use.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Instant, SystemTime};

use super::{ShaderAsset, ShaderError, parse};
use crate::assets;
use crate::shader_graph::{FileResolver, ShaderGraph};

type Loaded = Result<Arc<ShaderAsset>, ShaderError>;

struct Entry {
    modified: Option<SystemTime>,
    /// Bumped on every (re)load, so the renderer knows to recompile.
    generation: u64,
    shader: Loaded,
    /// A Shader Graph's graph (variants are generated from it).
    graph: Option<Arc<ShaderGraph>>,
    /// Other files the shader was built from, with their times.
    deps: Vec<(String, Option<SystemTime>)>,
    /// Variants generated so far, by key (`"_A=1"`).
    variants: HashMap<String, Loaded>,
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

fn error(message: String) -> ShaderError {
    ShaderError {
        line: 0,
        column: 0,
        message,
    }
}

fn entry(shader: Loaded, graph: Option<Arc<ShaderGraph>>, deps: Vec<String>, rel: &str) -> Entry {
    Entry {
        modified: mtime(rel),
        generation: next_generation(),
        shader,
        graph,
        deps: deps.into_iter().map(|d| (d.clone(), mtime(&d))).collect(),
        variants: HashMap::new(),
    }
}

fn read(rel: &str) -> Entry {
    let text = match std::fs::read_to_string(assets::absolute(rel)) {
        Ok(t) => t,
        Err(e) => {
            return entry(
                Err(error(format!("could not read the shader: {e}"))),
                None,
                Vec::new(),
                rel,
            );
        }
    };
    if crate::shader_graph::is_graph_path(rel) {
        match ShaderGraph::from_json(&text) {
            Ok(g) => from_graph(Arc::new(g), rel),
            Err(e) => entry(Err(error(e)), None, Vec::new(), rel),
        }
    } else {
        entry(parse(&text).map(Arc::new), None, Vec::new(), rel)
    }
}

/// The default variant of a graph, with what it read.
fn from_graph(g: Arc<ShaderGraph>, rel: &str) -> Entry {
    let generated = crate::shader_graph::generate(&g);
    let deps = generated
        .as_ref()
        .map(|cg| cg.deps.clone())
        .unwrap_or_default();
    let shader = generated
        .map_err(|e| error(format!("Shader Graph: {}", e.message)))
        .and_then(|cg| parse(&cg.source).map(Arc::new));
    entry(shader, Some(g), deps, rel)
}

/// A variant of an entry's shader.
fn variant(e: &Entry, key: &str) -> Loaded {
    let base = e.shader.clone()?;
    let v = crate::shader_graph::parse_variant_key(key);
    match &e.graph {
        Some(g) => crate::shader_graph::generate_variant(g, &v, &FileResolver)
            .map_err(|e| error(format!("Shader Graph: {}", e.message)))
            .and_then(|cg| parse(&cg.source).map(Arc::new)),
        None => base.with_variant(&v).map(Arc::new),
    }
}

/// The shader at a project path ("Assets/Shaders/Water.shader"), read on first use.
pub fn load(rel: &str) -> Loaded {
    let mut lib = library().lock().unwrap_or_else(|e| e.into_inner());
    lib.entries
        .entry(rel.to_owned())
        .or_insert_with(|| read(rel))
        .shader
        .clone()
}

/// One keyword variant of a shader (`key` from [`ShaderAsset::variant_key`]; "" = default).
pub fn load_variant(rel: &str, key: &str) -> Loaded {
    if key.is_empty() {
        return load(rel);
    }
    let mut lib = library().lock().unwrap_or_else(|e| e.into_inner());
    let e = lib
        .entries
        .entry(rel.to_owned())
        .or_insert_with(|| read(rel));
    if let Some(v) = e.variants.get(key) {
        return v.clone();
    }
    let v = variant(e, key);
    e.variants.insert(key.to_owned(), v.clone());
    v
}

/// Load counter of a shader (0 = never loaded); changes whenever its source does.
pub fn generation(rel: &str) -> u64 {
    let lib = library().lock().unwrap_or_else(|e| e.into_inner());
    lib.entries.get(rel).map_or(0, |e| e.generation)
}

/// Re-read shaders whose files (or Sub Graphs and function files) changed, at most twice a
/// second. Returns the changed paths.
pub fn poll_changes() -> Vec<String> {
    let mut lib = library().lock().unwrap_or_else(|e| e.into_inner());
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
        if entry.modified == cached_mtime(rel)
            && entry.deps.iter().all(|(d, t)| *t == cached_mtime(d))
        {
            continue;
        }
        let deps_changed = entry.deps.iter().any(|(d, t)| *t != mtime(d));
        if entry.modified != mtime(rel) || deps_changed {
            // Unsaved graph text (the Shader Graph window's preview) has no file: keep it.
            if entry.modified.is_none() && entry.graph.is_some() && !deps_changed {
                continue;
            }
            *entry = match (&entry.graph, entry.modified.is_none()) {
                (Some(g), true) => from_graph(g.clone(), rel),
                _ => read(rel),
            };
            changed.push(rel.clone());
        }
    }
    changed
}

/// Re-read one shader now (after saving it).
pub fn reload(rel: &str) {
    let mut lib = library().lock().unwrap_or_else(|e| e.into_inner());
    lib.entries.insert(rel.to_owned(), read(rel));
}

/// Use unsaved text for a shader (live editing in the Code Editor). Saving or a later file
/// change replaces it with the file's contents.
pub fn set_source(rel: &str, text: &str) {
    let mut lib = library().lock().unwrap_or_else(|e| e.into_inner());
    let unchanged = lib
        .entries
        .get(rel)
        .and_then(|e| e.shader.as_ref().ok())
        .is_some_and(|s| s.source == text);
    if unchanged {
        return;
    }
    lib.entries.insert(
        rel.to_owned(),
        entry(parse(text).map(Arc::new), None, Vec::new(), rel),
    );
}

/// Use an unsaved graph for a shader path (the Shader Graph window's main preview): its
/// variants generate from it.
pub fn set_graph(rel: &str, graph: &ShaderGraph) {
    let mut lib = library().lock().unwrap_or_else(|e| e.into_inner());
    let unchanged = lib
        .entries
        .get(rel)
        .and_then(|e| e.graph.as_deref())
        .is_some_and(|g| g == graph);
    if unchanged {
        return;
    }
    lib.entries
        .insert(rel.to_owned(), from_graph(Arc::new(graph.clone()), rel));
}
