//! Switches for the look-preserving shortcuts of performance round 3 (docs/pending/perf3.md).
//! All on by default; the timing and look harnesses turn them off one at a time to measure
//! what each saves and to check that the picture stays the same.

use std::sync::atomic::{AtomicBool, Ordering};

/// Every shortcut, by the name the harnesses use (`MISTY_AB=fast:<name>`, `MISTY_FAST_OFF`).
pub const NAMES: [&str; 4] = ["terrain_late", "sky_late", "gi_settle", "grass_vis_depth"];

static TERRAIN_LATE: AtomicBool = AtomicBool::new(true);
static SKY_LATE: AtomicBool = AtomicBool::new(true);
static GI_SETTLE: AtomicBool = AtomicBool::new(true);
static GRASS_VIS_DEPTH: AtomicBool = AtomicBool::new(true);

fn flag(name: &str) -> Option<&'static AtomicBool> {
    match name {
        "terrain_late" => Some(&TERRAIN_LATE),
        "sky_late" => Some(&SKY_LATE),
        "gi_settle" => Some(&GI_SETTLE),
        "grass_vis_depth" => Some(&GRASS_VIS_DEPTH),
        _ => None,
    }
}

/// Turn the shortcut `name` on or off (false when there is no such shortcut).
pub fn set(name: &str, on: bool) -> bool {
    flag(name).map(|f| f.store(on, Ordering::Relaxed)).is_some()
}

/// The main pass draws the terrain after the trees and the grass's depth (see
/// `Renderer::main_pass`).
pub fn terrain_late() -> bool {
    TERRAIN_LATE.load(Ordering::Relaxed)
}

/// The main pass draws the sky after the opaque scene, depth tested on the far plane.
pub fn sky_late() -> bool {
    SKY_LATE.load(Ordering::Relaxed)
}

/// Misty GI's settled levels trace their converged probes at a fraction of the rate and the
/// surface cache relights less often (misty_gi/mod.rs `SETTLE_FRAMES`).
pub fn gi_settle() -> bool {
    GI_SETTLE.load(Ordering::Relaxed)
}

/// The grass visibility raster's standing blades write the grass depth (the main pass draws
/// only the terrain's depth before it and shades the terrain after it), so the blades are
/// rasterised once instead of twice (`Renderer::grass_vis_passes`).
pub fn grass_vis_depth() -> bool {
    GRASS_VIS_DEPTH.load(Ordering::Relaxed)
}
