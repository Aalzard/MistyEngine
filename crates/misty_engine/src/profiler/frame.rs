//! One recorded frame and the Hierarchy view's merged call tree.

use std::borrow::Cow;

use serde::{Deserialize, Serialize};

use super::{Category, Counter, GpuPass, GpuTiming, MemoryStats, ThreadTrack};
use crate::render::RenderStats;

/// One timed scope.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Sample {
    pub name: Cow<'static, str>,
    pub category: Category,
    /// Nesting level (0 = top level).
    pub depth: u16,
    /// Start, in ms after the frame began (the previous frame's end).
    pub start_ms: f32,
    pub duration_ms: f32,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct FrameData {
    pub index: u64,
    /// Wall-clock time since the previous frame ended.
    pub cpu_ms: f32,
    /// GPU time of the frame's renders (timestamp queries), when supported.
    pub gpu_ms: Option<f32>,
    /// Self time per [`Category`] (indexed by [`Category::index`]).
    pub categories: [f32; Category::COUNT],
    /// Main Thread scopes.
    pub samples: Vec<Sample>,
    #[serde(default)]
    pub render: RenderStats,
    #[serde(default)]
    pub memory: MemoryStats,
    /// Module counters (indexed by [`Counter::index`]; shorter in older captures).
    #[serde(default)]
    pub counters: Vec<f32>,
    /// Scopes of the other threads (workers, background pool, audio thread).
    #[serde(default)]
    pub threads: Vec<ThreadTrack>,
    /// GPU time of each part of each render (when the device has the timestamps for it).
    #[serde(default)]
    pub gpu_passes: Vec<GpuPass>,
    #[serde(default)]
    pub gpu_timing: GpuTiming,
}

impl FrameData {
    pub fn new(index: u64, samples: Vec<Sample>, cpu_ms: f32) -> Self {
        let mut categories = [0.0; Category::COUNT];
        for (s, self_ms) in samples.iter().zip(self_times(&samples)) {
            categories[s.category.index()] += self_ms;
        }
        Self {
            index,
            cpu_ms,
            gpu_ms: None,
            categories,
            samples,
            render: RenderStats::default(),
            memory: MemoryStats::default(),
            counters: Vec::new(),
            threads: Vec::new(),
            gpu_passes: Vec::new(),
            gpu_timing: GpuTiming::default(),
        }
    }

    /// A module counter's value (0 when it wasn't reported).
    pub fn counter(&self, c: Counter) -> f32 {
        self.counters.get(c.index()).copied().unwrap_or(0.0)
    }

    /// Total time of the Main Thread scopes whose name starts with `prefix` (outermost
    /// matches only).
    pub fn time_in(&self, prefix: &str) -> f32 {
        let mut total = 0.0;
        let mut inside: Option<u16> = None;
        for s in &self.samples {
            if inside.is_some_and(|d| s.depth <= d) {
                inside = None;
            }
            if inside.is_none() && s.name.starts_with(prefix) {
                total += s.duration_ms;
                inside = Some(s.depth);
            }
        }
        total
    }

    /// Time in `category`; Others also gets the frame time no scope covered.
    pub fn category_ms(&self, category: Category) -> f32 {
        let ms = self.categories[category.index()];
        if category == Category::Others {
            let covered: f32 = self.categories.iter().sum();
            ms + (self.cpu_ms - covered).max(0.0)
        } else {
            ms
        }
    }
}

/// Each sample's duration minus its direct children's.
fn self_times(samples: &[Sample]) -> Vec<f32> {
    let mut out: Vec<f32> = samples.iter().map(|s| s.duration_ms).collect();
    let mut stack: Vec<usize> = Vec::new();
    for (i, s) in samples.iter().enumerate() {
        while stack.last().is_some_and(|&p| samples[p].depth >= s.depth) {
            stack.pop();
        }
        if let Some(&parent) = stack.last() {
            out[parent] -= s.duration_ms;
        }
        stack.push(i);
    }
    out.iter().map(|v| v.max(0.0)).collect()
}

/// Calls with the same name under the same parent, merged.
#[derive(Clone, Debug, PartialEq)]
pub struct HierarchyNode {
    pub name: String,
    pub category: Category,
    pub total_ms: f32,
    pub self_ms: f32,
    pub calls: u32,
    /// Indices into [`HierarchyTree::nodes`], longest first.
    pub children: Vec<usize>,
}

/// Merged call tree of a frame; node 0 is the frame itself.
#[derive(Clone, Debug)]
pub struct HierarchyTree {
    pub nodes: Vec<HierarchyNode>,
}

impl HierarchyTree {
    pub fn build(frame: &FrameData) -> Self {
        let mut nodes = vec![HierarchyNode {
            name: "Frame".into(),
            category: Category::Others,
            total_ms: frame.cpu_ms,
            self_ms: 0.0,
            calls: 1,
            children: Vec::new(),
        }];
        // (node, depth) of the open parents.
        let mut stack: Vec<(usize, u16)> = Vec::new();
        for s in &frame.samples {
            while stack.last().is_some_and(|&(_, d)| d >= s.depth) {
                stack.pop();
            }
            let parent = stack.last().map_or(0, |&(n, _)| n);
            let existing = nodes[parent]
                .children
                .iter()
                .copied()
                .find(|&c| nodes[c].name == s.name);
            let node = existing.unwrap_or_else(|| {
                nodes.push(HierarchyNode {
                    name: s.name.to_string(),
                    category: s.category,
                    total_ms: 0.0,
                    self_ms: 0.0,
                    calls: 0,
                    children: Vec::new(),
                });
                let n = nodes.len() - 1;
                nodes[parent].children.push(n);
                n
            });
            nodes[node].total_ms += s.duration_ms;
            nodes[node].calls += 1;
            stack.push((node, s.depth));
        }
        for i in 0..nodes.len() {
            let children: f32 = nodes[i].children.iter().map(|&c| nodes[c].total_ms).sum();
            nodes[i].self_ms = (nodes[i].total_ms - children).max(0.0);
            let mut kids = std::mem::take(&mut nodes[i].children);
            kids.sort_by(|&a, &b| nodes[b].total_ms.total_cmp(&nodes[a].total_ms));
            nodes[i].children = kids;
        }
        Self { nodes }
    }
}
