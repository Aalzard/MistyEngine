//! Memory Profiler snapshots: what the engine holds, item by item, with estimated sizes.
//!
//! Sizes are estimates from known formats and dimensions (GPU textures and buffers, decoded
//! audio, component structs, live particles); the process working set is the operating system's
//! figure when it is available. Two snapshots can be compared ([`diff`]).

#[cfg(test)]
mod tests;

use std::collections::{BTreeMap, HashMap};

use crate::particles::ParticleEmitter;
use crate::scene::{AudioSource, Scene};

/// Totals are grouped by these.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum MemCategory {
    Textures,
    Meshes,
    RenderTargets,
    Buffers,
    Audio,
    Scripts,
    SceneObjects,
    Particles,
    Lightmaps,
}

impl MemCategory {
    pub const ALL: [Self; 9] = [
        Self::Textures,
        Self::Meshes,
        Self::RenderTargets,
        Self::Buffers,
        Self::Audio,
        Self::Scripts,
        Self::SceneObjects,
        Self::Particles,
        Self::Lightmaps,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::Textures => "Textures",
            Self::Meshes => "Meshes",
            Self::RenderTargets => "Render Targets",
            Self::Buffers => "Buffers",
            Self::Audio => "Audio",
            Self::Scripts => "Scripts (Lua heap)",
            Self::SceneObjects => "Scene Objects",
            Self::Particles => "Particles / VFX",
            Self::Lightmaps => "Lightmaps",
        }
    }
}

/// One thing that holds memory.
#[derive(Clone, Debug, PartialEq)]
pub struct MemItem {
    pub name: String,
    /// Type label ("Texture2D", "Mesh", "Component").
    pub kind: String,
    pub category: MemCategory,
    pub bytes: u64,
    /// Users (material sets, Mesh Filters, component instances…; 0 = unknown).
    pub refs: u32,
}

impl MemItem {
    pub fn new(category: MemCategory, kind: &str, name: impl Into<String>, bytes: u64) -> Self {
        Self {
            name: name.into(),
            kind: kind.to_owned(),
            category,
            bytes,
            refs: 0,
        }
    }

    pub fn with_refs(mut self, refs: u32) -> Self {
        self.refs = refs;
        self
    }

    /// Identity across snapshots.
    fn key(&self) -> (MemCategory, &str, &str) {
        (self.category, self.kind.as_str(), self.name.as_str())
    }
}

/// Everything captured at one moment.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct MemorySnapshot {
    pub label: String,
    pub items: Vec<MemItem>,
    /// Process working set (resident memory) in bytes, when the OS reports it.
    pub working_set: Option<u64>,
}

impl MemorySnapshot {
    pub fn new(label: impl Into<String>, items: Vec<MemItem>) -> Self {
        Self {
            label: label.into(),
            items,
            working_set: working_set(),
        }
    }

    /// Bytes per category, in [`MemCategory::ALL`] order.
    pub fn totals(&self) -> Vec<(MemCategory, u64)> {
        MemCategory::ALL
            .iter()
            .map(|c| (*c, self.category_total(*c)))
            .collect()
    }

    pub fn category_total(&self, c: MemCategory) -> u64 {
        self.items
            .iter()
            .filter(|i| i.category == c)
            .map(|i| i.bytes)
            .sum()
    }

    /// Sum of every item.
    pub fn total(&self) -> u64 {
        self.items.iter().map(|i| i.bytes).sum()
    }

    /// Item indices, largest first.
    pub fn largest(&self) -> Vec<usize> {
        let mut order: Vec<usize> = (0..self.items.len()).collect();
        order.sort_by(|a, b| self.items[*b].bytes.cmp(&self.items[*a].bytes));
        order
    }
}

/// How an item changed between two snapshots.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Change {
    Added,
    Removed,
    Grew,
    Shrank,
    Same,
}

/// One row of a snapshot comparison.
#[derive(Clone, Debug, PartialEq)]
pub struct DiffRow {
    pub name: String,
    pub kind: String,
    pub category: MemCategory,
    pub before: u64,
    pub after: u64,
}

impl DiffRow {
    pub fn delta(&self) -> i64 {
        self.after as i64 - self.before as i64
    }

    pub fn change(&self) -> Change {
        match (self.before, self.after) {
            (0, a) if a > 0 => Change::Added,
            (b, 0) if b > 0 => Change::Removed,
            (b, a) if a > b => Change::Grew,
            (b, a) if a < b => Change::Shrank,
            _ => Change::Same,
        }
    }
}

/// Every item of either snapshot (matched by category, type and name; duplicates are summed),
/// largest change first.
pub fn diff(before: &MemorySnapshot, after: &MemorySnapshot) -> Vec<DiffRow> {
    let mut rows: BTreeMap<(MemCategory, String, String), (u64, u64)> = BTreeMap::new();
    for i in &before.items {
        let (c, k, n) = i.key();
        rows.entry((c, k.to_owned(), n.to_owned())).or_default().0 += i.bytes;
    }
    for i in &after.items {
        let (c, k, n) = i.key();
        rows.entry((c, k.to_owned(), n.to_owned())).or_default().1 += i.bytes;
    }
    let mut out: Vec<DiffRow> = rows
        .into_iter()
        .map(|((category, kind, name), (before, after))| DiffRow {
            name,
            kind,
            category,
            before,
            after,
        })
        .collect();
    out.sort_by_key(|r| std::cmp::Reverse(r.delta().unsigned_abs()));
    out
}

/// Per category: (before, after).
pub fn category_diff(
    before: &MemorySnapshot,
    after: &MemorySnapshot,
) -> Vec<(MemCategory, u64, u64)> {
    MemCategory::ALL
        .iter()
        .map(|c| (*c, before.category_total(*c), after.category_total(*c)))
        .collect()
}

/// Component structs per type (count × struct size; heap data such as strings is not counted)
/// and live particle buffers.
pub fn scene_items(scene: &Scene) -> Vec<MemItem> {
    let entities: Vec<hecs::Entity> = scene.world.iter().map(|e| e.entity()).collect();
    let mut out = Vec::new();
    for info in crate::scene::registry::components() {
        let count = entities
            .iter()
            .filter(|e| (info.has)(&scene.world, **e))
            .count() as u64;
        if count > 0 {
            let item = MemItem::new(
                MemCategory::SceneObjects,
                "Component",
                info.name,
                count * info.size as u64,
            );
            out.push(item.with_refs(count as u32));
        }
    }
    for (e, emitter) in scene
        .world
        .query::<(hecs::Entity, &ParticleEmitter)>()
        .iter()
    {
        let name = scene
            .world
            .get::<&crate::scene::GameObject>(e)
            .map_or_else(|_| "Particle System".to_owned(), |g| g.name.clone());
        out.push(MemItem::new(
            MemCategory::Particles,
            "ParticleSystem",
            name,
            particle_bytes(emitter),
        ));
    }
    out
}

/// Bytes of an emitter's particle storage (its allocated capacity).
pub fn particle_bytes(e: &ParticleEmitter) -> u64 {
    (e.particles.capacity() * std::mem::size_of::<crate::particles::Particle>()) as u64
}

/// Audio clips the scene's Audio Sources use, at their decoded size (32-bit float stereo
/// frames, the way clips are kept in memory to play).
pub fn audio_items(scene: &Scene) -> Vec<MemItem> {
    let mut clips: HashMap<String, u32> = HashMap::new();
    for s in scene.world.query::<&AudioSource>().iter() {
        if let Some(c) = &s.clip {
            *clips.entry(c.clone()).or_insert(0) += 1;
        }
    }
    let mut out: Vec<MemItem> = clips
        .into_iter()
        .map(|(clip, refs)| {
            let bytes = crate::vfs::read(crate::assets::absolute(&clip))
                .map(|b| decoded_audio_bytes(&clip, &b))
                .unwrap_or(0);
            MemItem::new(MemCategory::Audio, "AudioClip", clip, bytes).with_refs(refs)
        })
        .collect();
    out.sort_by(|a, b| a.name.cmp(&b.name));
    out
}

/// Decoded size of a clip file: WAV from its header (frames × 8 bytes), compressed formats
/// estimated at 10× the file size.
pub fn decoded_audio_bytes(path: &str, file: &[u8]) -> u64 {
    if !path.to_ascii_lowercase().ends_with(".wav") {
        return file.len() as u64 * 10;
    }
    wav_frames(file).map_or(file.len() as u64, |frames| frames * 8)
}

/// Sample frames in a WAV file (`data` bytes / block align).
fn wav_frames(b: &[u8]) -> Option<u64> {
    if b.len() < 12 || &b[0..4] != b"RIFF" || &b[8..12] != b"WAVE" {
        return None;
    }
    let u16_at = |i: usize| Some(u16::from_le_bytes(b.get(i..i + 2)?.try_into().ok()?));
    let u32_at = |i: usize| Some(u32::from_le_bytes(b.get(i..i + 4)?.try_into().ok()?));
    let (mut i, mut align, mut data) = (12, None, None);
    while i + 8 <= b.len() {
        let len = u32_at(i + 4)? as usize;
        match &b[i..i + 4] {
            b"fmt " => align = u16_at(i + 20),
            b"data" => data = Some(len),
            _ => {}
        }
        i += 8 + len + (len & 1);
    }
    let align = align.filter(|a| *a > 0)? as u64;
    Some(data? as u64 / align)
}

/// The Lua state's heap.
pub fn lua_item(bytes: usize) -> MemItem {
    MemItem::new(MemCategory::Scripts, "Lua Heap", "Lua state", bytes as u64)
}

/// The process's working set in bytes (Windows; `None` elsewhere).
#[cfg(windows)]
pub fn working_set() -> Option<u64> {
    #[repr(C)]
    #[derive(Default)]
    struct Counters {
        cb: u32,
        page_fault_count: u32,
        peak_working_set: usize,
        working_set: usize,
        quota_peak_paged_pool: usize,
        quota_paged_pool: usize,
        quota_peak_non_paged_pool: usize,
        quota_non_paged_pool: usize,
        pagefile_usage: usize,
        peak_pagefile_usage: usize,
    }
    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn GetCurrentProcess() -> isize;
        fn K32GetProcessMemoryInfo(process: isize, counters: *mut Counters, cb: u32) -> i32;
    }
    let mut c = Counters {
        cb: std::mem::size_of::<Counters>() as u32,
        ..Default::default()
    };
    // SAFETY: the pseudo handle needs no closing; `c` is a correctly sized, writable struct.
    let ok = unsafe { K32GetProcessMemoryInfo(GetCurrentProcess(), &mut c, c.cb) };
    (ok != 0).then_some(c.working_set as u64)
}

#[cfg(not(windows))]
pub fn working_set() -> Option<u64> {
    None
}

/// "12.3 MB" style size.
pub fn format_bytes(bytes: u64) -> String {
    let b = bytes as f64;
    match bytes {
        0..1024 => format!("{bytes} B"),
        1024..1_048_576 => format!("{:.1} KB", b / 1024.0),
        1_048_576..1_073_741_824 => format!("{:.1} MB", b / 1_048_576.0),
        _ => format!("{:.2} GB", b / 1_073_741_824.0),
    }
}

/// Signed size change ("+1.5 MB", "-200 B").
pub fn format_delta(delta: i64) -> String {
    let sign = if delta < 0 { "-" } else { "+" };
    format!("{sign}{}", format_bytes(delta.unsigned_abs()))
}
