//! Process-wide cache of shader assets by project path, re-read when files change.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Instant, SystemTime};

use super::{ShaderAsset, ShaderError, parse};
use crate::assets;

struct Entry {
    modified: Option<SystemTime>,
    /// Bumped on every (re)load, so the renderer knows to recompile.
    generation: u64,
    shader: Result<Arc<ShaderAsset>, ShaderError>,
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

fn read(rel: &str) -> Entry {
    let shader = std::fs::read_to_string(assets::absolute(rel))
        .map_err(|e| ShaderError {
            line: 0,
            column: 0,
            message: format!("could not read the shader: {e}"),
        })
        .and_then(|text| {
            if crate::shader_graph::is_graph_path(rel) {
                graph_source(&text)
            } else {
                Ok(text)
            }
        })
        .and_then(|text| parse(&text).map(Arc::new));
    Entry {
        modified: mtime(rel),
        generation: next_generation(),
        shader,
    }
}

/// The shader text a `.shadergraph` file generates.
fn graph_source(text: &str) -> Result<String, ShaderError> {
    let error = |message: String| ShaderError {
        line: 0,
        column: 0,
        message,
    };
    let graph = crate::shader_graph::ShaderGraph::from_json(text).map_err(error)?;
    crate::shader_graph::generate(&graph)
        .map(|g| g.source)
        .map_err(|e| error(format!("Shader Graph: {}", e.message)))
}

/// The shader at a project path ("Assets/Shaders/Water.shader"), read on first use.
pub fn load(rel: &str) -> Result<Arc<ShaderAsset>, ShaderError> {
    let mut lib = library().lock().unwrap_or_else(|e| e.into_inner());
    lib.entries
        .entry(rel.to_owned())
        .or_insert_with(|| read(rel))
        .shader
        .clone()
}

/// Load counter of a shader (0 = never loaded); changes whenever its source does.
pub fn generation(rel: &str) -> u64 {
    let lib = library().lock().unwrap_or_else(|e| e.into_inner());
    lib.entries.get(rel).map_or(0, |e| e.generation)
}

/// Re-read shaders whose files changed (at most twice a second). Returns the changed paths.
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
        if entry.modified != mtime(rel) {
            *entry = read(rel);
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
        Entry {
            modified: mtime(rel),
            generation: next_generation(),
            shader: parse(text).map(Arc::new),
        },
    );
}
