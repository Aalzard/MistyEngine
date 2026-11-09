//! Switches for the look-preserving shortcuts of performance rounds 3 and 22
//! (docs/pending/perf3.md, docs/pending/r22-perf.md).
//! All on by default; the timing and look harnesses turn them off one at a time to measure
//! what each saves and to check that the picture stays the same. `MISTY_FAST_OFF=a,b` (or
//! `all`) in the environment turns them off in any build (the editor and the player too).

use std::sync::Once;
use std::sync::atomic::{AtomicBool, Ordering};

/// Every shortcut, by the name the harnesses use (`MISTY_AB=fast:<name>`, `MISTY_FAST_OFF`).
pub const NAMES: [&str; 5] = [
    "terrain_late",
    "sky_late",
    "gi_settle",
    "grass_vis_depth",
    "shaft_cascades",
];

static TERRAIN_LATE: AtomicBool = AtomicBool::new(true);
static SKY_LATE: AtomicBool = AtomicBool::new(true);
static GI_SETTLE: AtomicBool = AtomicBool::new(true);
static GRASS_VIS_DEPTH: AtomicBool = AtomicBool::new(true);
static SHAFT_CASCADES: AtomicBool = AtomicBool::new(true);

fn flag(name: &str) -> Option<&'static AtomicBool> {
    match name {
        "terrain_late" => Some(&TERRAIN_LATE),
        "sky_late" => Some(&SKY_LATE),
        "gi_settle" => Some(&GI_SETTLE),
        "grass_vis_depth" => Some(&GRASS_VIS_DEPTH),
        "shaft_cascades" => Some(&SHAFT_CASCADES),
        _ => None,
    }
}

/// `MISTY_FAST_OFF`, read once before the first use.
fn init() {
    static INIT: Once = Once::new();
    INIT.call_once(|| {
        let names = std::env::var("MISTY_FAST_OFF").unwrap_or_default();
        for n in names.split(',').map(str::trim).filter(|n| !n.is_empty()) {
            for f in NAMES {
                if n == "all" || n == f {
                    flag(f).expect("listed").store(false, Ordering::Relaxed);
                }
            }
        }
    });
}

/// Turn the shortcut `name` on or off (false when there is no such shortcut).
pub fn set(name: &str, on: bool) -> bool {
    init();
    flag(name).map(|f| f.store(on, Ordering::Relaxed)).is_some()
}

fn get(f: &AtomicBool) -> bool {
    init();
    f.load(Ordering::Relaxed)
}

/// The main pass draws the terrain after the trees and the grass's depth (see
/// `Renderer::main_pass`).
pub fn terrain_late() -> bool {
    get(&TERRAIN_LATE)
}

/// The main pass draws the sky after the opaque scene, depth tested on the far plane.
pub fn sky_late() -> bool {
    get(&SKY_LATE)
}

/// Misty GI's settled levels trace their converged probes at a fraction of the rate and the
/// surface cache relights less often (misty_gi/mod.rs `SETTLE_FRAMES`).
pub fn gi_settle() -> bool {
    get(&GI_SETTLE)
}

/// The grass visibility raster's standing blades write the grass depth (the main pass draws
/// only the terrain's depth before it and shades the terrain after it), so the blades are
/// rasterised once instead of twice (`Renderer::grass_vis_passes`).
pub fn grass_vis_depth() -> bool {
    get(&GRASS_VIS_DEPTH)
}

/// The sharp sun shafts' march reads the first sun cascade whose texels it can resolve
/// instead of always the tightest (volumetrics/sharp_shafts.rs `FOOTPRINT_SHARE`).
pub fn shaft_cascades() -> bool {
    get(&SHAFT_CASCADES)
}

