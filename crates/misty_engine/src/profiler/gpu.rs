//! GPU module data: the GPU time of each part of every render (from timestamp queries written
//! between passes, read back a frame or two later), grouped for the stacked chart.

use std::borrow::Cow;
use std::sync::atomic::{AtomicU8, Ordering};

use serde::{Deserialize, Serialize};

use super::FrameData;

/// One timed part of a render.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct GpuPass {
    /// The view rendered ("Game", "Scene"; a size when the caller gave no name).
    pub view: Cow<'static, str>,
    /// Part name ("Shadow maps (cascades, local)", "Main: opaque meshes").
    pub name: Cow<'static, str>,
    /// Start relative to the render's first timestamp.
    pub start_ms: f32,
    pub ms: f32,
}

impl GpuPass {
    pub fn group(&self) -> GpuGroup {
        GpuGroup::of(&self.name)
    }
}

/// Chart groups of the GPU module (stacked like the reference's GPU Usage module).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum GpuGroup {
    Shadows,
    Depth,
    Opaque,
    Vegetation,
    Transparent,
    Lighting,
    PostProcess,
    Other,
}

impl GpuGroup {
    pub const COUNT: usize = 8;
    pub const ALL: [Self; Self::COUNT] = [
        Self::Shadows,
        Self::Depth,
        Self::Opaque,
        Self::Vegetation,
        Self::Transparent,
        Self::Lighting,
        Self::PostProcess,
        Self::Other,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::Shadows => "Shadows",
            Self::Depth => "Depth / Culling",
            Self::Opaque => "Opaque",
            Self::Vegetation => "Grass & Trees",
            Self::Transparent => "Transparent & Effects",
            Self::Lighting => "Lighting / GI / Volumes",
            Self::PostProcess => "Post-processing",
            Self::Other => "Other",
        }
    }

    pub fn index(self) -> usize {
        self as usize
    }

    /// The group of a pass timer label.
    pub fn of(label: &str) -> Self {
        let l = label.to_ascii_lowercase();
        let has = |words: &[&str]| words.iter().any(|w| l.contains(w));
        if has(&[
            "shadow map",
            "shadow culling",
            "virtual shadow",
            "shadow maps",
        ]) {
            Self::Shadows
        } else if has(&["grass", "trees"]) {
            Self::Vegetation
        } else if has(&[
            "bloom",
            "tonemap",
            "taa",
            "depth of field",
            "motion blur",
            "exposure",
            "finish",
            "msaa resolve",
        ]) {
            Self::PostProcess
        } else if has(&[
            "ssgi",
            "gtao",
            "g cache",
            "fog",
            "cloud",
            "sun shaft",
            "shadow mask",
            "ssr",
            "glows",
        ]) {
            Self::Lighting
        } else if has(&[
            "prepass",
            "hi-z",
            "culling",
            "selection mask",
            "visibility raster",
        ]) {
            Self::Depth
        } else if has(&["decals", "water", "transparent", "particles"]) {
            Self::Transparent
        } else if has(&["main", "opaque", "sky", "ground", "virtual geometry"]) {
            Self::Opaque
        } else {
            Self::Other
        }
    }
}

/// How the GPU times of a frame were measured.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum GpuTiming {
    /// Timestamps between passes: per-pass times.
    Passes,
    /// Timestamps only at the start and end of a render (the device can't write them inside
    /// a command encoder): one time per render.
    FrameOnly,
    /// No timestamp queries on this device.
    #[default]
    Unsupported,
}

impl GpuTiming {
    fn from_u8(v: u8) -> Self {
        match v {
            2 => Self::Passes,
            1 => Self::FrameOnly,
            _ => Self::Unsupported,
        }
    }
}

static TIMING: AtomicU8 = AtomicU8::new(0);

/// What the renderer can measure (set when a renderer is made).
pub fn set_gpu_timing(t: GpuTiming) {
    let v = match t {
        GpuTiming::Passes => 2,
        GpuTiming::FrameOnly => 1,
        GpuTiming::Unsupported => 0,
    };
    TIMING.store(v, Ordering::Relaxed);
}

pub fn gpu_timing() -> GpuTiming {
    GpuTiming::from_u8(TIMING.load(Ordering::Relaxed))
}

/// Per-pass GPU times of a render of frame `index` (arrive a frame or two later).
pub fn add_gpu_passes(index: u64, passes: Vec<GpuPass>) {
    if !super::enabled() || passes.is_empty() {
        return;
    }
    let mut passes = passes;
    super::late(index, |_, to| to.append(&mut passes));
}

impl FrameData {
    /// GPU time per [`GpuGroup`] (indexed by [`GpuGroup::index`]); with no per-pass times the
    /// whole GPU time is Other.
    pub fn gpu_groups(&self) -> [f32; GpuGroup::COUNT] {
        let mut out = [0.0; GpuGroup::COUNT];
        if self.gpu_passes.is_empty() {
            out[GpuGroup::Other.index()] = self.gpu_ms.unwrap_or(0.0);
            return out;
        }
        for p in &self.gpu_passes {
            out[p.group().index()] += p.ms;
        }
        out
    }

    /// Sum of the per-pass times (or the frame's GPU time without them).
    pub fn gpu_total_ms(&self) -> Option<f32> {
        match self.gpu_passes.is_empty() {
            true => self.gpu_ms,
            false => Some(self.gpu_passes.iter().map(|p| p.ms).sum()),
        }
    }
}
