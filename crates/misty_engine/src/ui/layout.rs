//! Pure Rect Transform maths: rect from parent rect + anchors, offsets, anchor presets and the
//! Canvas Scaler factor. Canvas space is y-up with the origin at the canvas' lower-left corner.

use std::sync::atomic::{AtomicU32, Ordering};

use glam::Vec2;

use crate::scene::{CanvasScaler, RectTransform, ScreenMatchMode, UiScaleMode};

/// Axis-aligned rectangle (canvas units, y up; or pixels, y down, depending on use).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Rect {
    pub min: Vec2,
    pub max: Vec2,
}

impl Rect {
    pub fn new(min: Vec2, max: Vec2) -> Self {
        Self { min, max }
    }

    pub fn from_size(size: Vec2) -> Self {
        Self::new(Vec2::ZERO, size)
    }

    pub fn size(&self) -> Vec2 {
        self.max - self.min
    }

    pub fn center(&self) -> Vec2 {
        (self.min + self.max) * 0.5
    }

    /// Inclusive of the min edges, exclusive of the max edges.
    pub fn contains(&self, p: Vec2) -> bool {
        p.x >= self.min.x && p.y >= self.min.y && p.x < self.max.x && p.y < self.max.y
    }

    pub fn scaled(&self, s: f32) -> Self {
        Self::new(self.min * s, self.max * s)
    }

    /// Overlap of two rects (empty when they don't overlap: max ≤ min).
    pub fn intersect(&self, o: &Rect) -> Rect {
        let min = self.min.max(o.min);
        Self::new(min, self.max.min(o.max).max(min))
    }

    pub fn is_empty(&self) -> bool {
        self.max.x <= self.min.x || self.max.y <= self.min.y
    }

    /// Mirror vertically inside a space of height `h` (y-up ↔ y-down).
    pub fn flip_y(&self, h: f32) -> Self {
        Self::new(
            Vec2::new(self.min.x, h - self.max.y),
            Vec2::new(self.max.x, h - self.min.y),
        )
    }
}

impl RectTransform {
    /// This rect inside `parent` (both y up).
    pub fn rect_in(&self, parent: Rect) -> Rect {
        let ps = parent.size();
        let a_min = parent.min + self.anchor_min * ps;
        let a_max = parent.min + self.anchor_max * ps;
        let size = (a_max - a_min) + self.size_delta;
        let pivot_pos = a_min + (a_max - a_min) * self.pivot + self.anchored_position;
        let min = pivot_pos - size * self.pivot;
        Rect::new(min, min + size)
    }

    /// Lower-left corner relative to the lower-left anchor.
    pub fn offset_min(&self) -> Vec2 {
        self.anchored_position - self.size_delta * self.pivot
    }

    /// Upper-right corner relative to the upper-right anchor.
    pub fn offset_max(&self) -> Vec2 {
        self.anchored_position + self.size_delta * (Vec2::ONE - self.pivot)
    }

    /// Set both corner offsets (keeps anchors and pivot).
    pub fn set_offsets(&mut self, min: Vec2, max: Vec2) {
        self.size_delta = max - min;
        self.anchored_position = min + self.size_delta * self.pivot;
    }

    /// Change anchors without moving the rect inside a parent of `parent_size`.
    pub fn set_anchors_keep_rect(&mut self, parent_size: Vec2, min: Vec2, max: Vec2) {
        let rect = self.rect_in(Rect::from_size(parent_size));
        self.anchor_min = min;
        self.anchor_max = max;
        self.set_offsets(rect.min - min * parent_size, rect.max - max * parent_size);
    }

    /// Change the pivot without moving the rect.
    pub fn set_pivot_keep_rect(&mut self, pivot: Vec2) {
        let (min, max) = (self.offset_min(), self.offset_max());
        self.pivot = pivot;
        self.set_offsets(min, max);
    }
}

/// One axis of an anchor preset.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AnchorAxis {
    /// Left / bottom.
    Min,
    Center,
    /// Right / top.
    Max,
    Stretch,
}

impl AnchorAxis {
    pub const ALL: [Self; 4] = [Self::Min, Self::Center, Self::Max, Self::Stretch];

    /// (anchor min, anchor max, pivot) along this axis.
    pub fn values(self) -> (f32, f32, f32) {
        match self {
            Self::Min => (0.0, 0.0, 0.0),
            Self::Center => (0.5, 0.5, 0.5),
            Self::Max => (1.0, 1.0, 1.0),
            Self::Stretch => (0.0, 1.0, 0.5),
        }
    }

    /// The preset an axis currently matches, if any.
    pub fn of(min: f32, max: f32) -> Option<Self> {
        Self::ALL.into_iter().find(|a| {
            let (lo, hi, _) = a.values();
            (lo - min).abs() < 1e-4 && (hi - max).abs() < 1e-4
        })
    }
}

/// Apply an anchor preset (horizontal, vertical). The rect stays in place unless
/// `set_position` (then it snaps to the anchors); `set_pivot` also moves the pivot.
pub fn apply_anchor_preset(
    rt: &mut RectTransform,
    parent_size: Vec2,
    h: AnchorAxis,
    v: AnchorAxis,
    set_pivot: bool,
    set_position: bool,
) {
    let (hx, hy, hp) = h.values();
    let (vx, vy, vp) = v.values();
    if set_pivot {
        rt.set_pivot_keep_rect(Vec2::new(hp, vp));
    }
    rt.set_anchors_keep_rect(parent_size, Vec2::new(hx, vx), Vec2::new(hy, vy));
    if set_position {
        rt.anchored_position = Vec2::ZERO;
        let size = rt.rect_in(Rect::from_size(parent_size)).size();
        // Stretched axes fill the parent exactly.
        if h == AnchorAxis::Stretch {
            rt.size_delta.x = 0.0;
        } else {
            rt.size_delta.x = size.x;
        }
        if v == AnchorAxis::Stretch {
            rt.size_delta.y = 0.0;
        } else {
            rt.size_delta.y = size.y;
        }
    }
}

/// Screen DPI reported by the window (0 = unknown), stored as f32 bits.
static SCREEN_DPI: AtomicU32 = AtomicU32::new(0);

/// Tell Constant Physical Size canvases the screen's DPI (0 = unknown: their fallback DPI).
pub fn set_screen_dpi(dpi: f32) {
    let dpi = if dpi.is_finite() && dpi > 0.0 {
        dpi
    } else {
        0.0
    };
    SCREEN_DPI.store(dpi.to_bits(), Ordering::Relaxed);
}

/// The screen DPI set with [`set_screen_dpi`] (0 = unknown).
pub fn screen_dpi() -> f32 {
    f32::from_bits(SCREEN_DPI.load(Ordering::Relaxed))
}

/// Screen pixels per canvas unit.
pub fn scale_factor(scaler: Option<&CanvasScaler>, screen: Vec2) -> f32 {
    scale_factor_at(scaler, screen, screen_dpi())
}

/// [`scale_factor`] for a screen of `dpi` dots per inch (0 = unknown).
pub fn scale_factor_at(scaler: Option<&CanvasScaler>, screen: Vec2, dpi: f32) -> f32 {
    let Some(s) = scaler.filter(|s| s.enabled) else {
        return 1.0;
    };
    let f = match s.mode {
        UiScaleMode::ConstantPixelSize => s.scale_factor,
        UiScaleMode::ScaleWithScreenSize => {
            let r = s.reference_resolution.max(Vec2::ONE);
            let (sx, sy) = (screen.x.max(1.0) / r.x, screen.y.max(1.0) / r.y);
            match s.screen_match_mode {
                ScreenMatchMode::MatchWidthOrHeight => {
                    // Blend in log space so e.g. half width + double height averages to 1.
                    let (w, h) = (sx.log2(), sy.log2());
                    let t = s.match_width_or_height.clamp(0.0, 1.0);
                    2f32.powf(w + (h - w) * t)
                }
                ScreenMatchMode::Expand => sx.min(sy),
                ScreenMatchMode::Shrink => sx.max(sy),
            }
        }
        UiScaleMode::ConstantPhysicalSize => {
            let dpi = if dpi > 0.0 {
                dpi
            } else {
                s.fallback_screen_dpi
            };
            // One canvas unit is one physical unit: dpi / (units per inch) pixels.
            dpi / s.physical_unit.per_inch()
        }
    };
    if f.is_finite() && f > 1e-4 { f } else { 1.0 }
}

/// Sprite pixels per canvas unit (sliced borders, tiles): the scaler's Reference Pixels Per
/// Unit over the sprite's own, scaled by an Image's multiplier. Constant Physical Size counts
/// sprite pixels at their authoring DPI.
pub fn sprite_pixels_per_unit(
    scaler: Option<&CanvasScaler>,
    sprite_ppu: f32,
    multiplier: f32,
) -> f32 {
    let (reference, dpi_ratio) = match scaler.filter(|s| s.enabled) {
        Some(s) if s.mode == UiScaleMode::ConstantPhysicalSize => (
            s.reference_pixels_per_unit,
            s.physical_unit.per_inch() / s.default_sprite_dpi.max(1e-3),
        ),
        Some(s) => (s.reference_pixels_per_unit, 1.0),
        None => (100.0, 1.0),
    };
    let ppu = sprite_ppu.max(1e-3) / reference.max(1e-3) / dpi_ratio.max(1e-6);
    (ppu * multiplier.max(1e-3)).max(1e-4)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: Vec2, b: Vec2) -> bool {
        (a - b).abs().max_element() < 1e-3
    }

    const PARENT: Rect = Rect {
        min: Vec2::new(0.0, 0.0),
        max: Vec2::new(800.0, 600.0),
    };

    #[test]
    fn centred_fixed_size_rect() {
        let r = RectTransform::sized(160.0, 30.0).rect_in(PARENT);
        assert!(close(r.min, Vec2::new(320.0, 285.0)));
        assert!(close(r.size(), Vec2::new(160.0, 30.0)));
    }

    #[test]
    fn anchored_to_top_left_with_offset() {
        let rt = RectTransform {
            anchor_min: Vec2::new(0.0, 1.0),
            anchor_max: Vec2::new(0.0, 1.0),
            pivot: Vec2::new(0.0, 1.0),
            anchored_position: Vec2::new(10.0, -20.0),
            size_delta: Vec2::new(200.0, 50.0),
        };
        let r = rt.rect_in(PARENT);
        assert!(close(r.min, Vec2::new(10.0, 530.0)));
        assert!(close(r.max, Vec2::new(210.0, 580.0)));
    }

    #[test]
    fn stretched_rect_with_margins_follows_parent_size() {
        let mut rt = RectTransform::stretched();
        rt.set_offsets(Vec2::new(10.0, 20.0), Vec2::new(-30.0, -40.0));
        let r = rt.rect_in(PARENT);
        assert!(close(r.min, Vec2::new(10.0, 20.0)));
        assert!(close(r.max, Vec2::new(770.0, 560.0)));
        let big = rt.rect_in(Rect::from_size(Vec2::new(1000.0, 1000.0)));
        assert!(close(big.max, Vec2::new(970.0, 960.0)));
        assert!(close(rt.offset_min(), Vec2::new(10.0, 20.0)));
        assert!(close(rt.offset_max(), Vec2::new(-30.0, -40.0)));
    }

    #[test]
    fn nested_rects_use_the_parent_rect() {
        let panel = RectTransform::sized(400.0, 200.0).rect_in(PARENT);
        let child = RectTransform {
            anchor_min: Vec2::new(1.0, 0.0),
            anchor_max: Vec2::new(1.0, 0.0),
            pivot: Vec2::new(1.0, 0.0),
            anchored_position: Vec2::ZERO,
            size_delta: Vec2::new(50.0, 50.0),
        }
        .rect_in(panel);
        assert!(close(child.max, Vec2::new(600.0, 250.0)));
    }

    #[test]
    fn changing_anchors_and_pivot_keeps_the_rect() {
        let mut rt = RectTransform::sized(100.0, 40.0);
        rt.anchored_position = Vec2::new(30.0, -10.0);
        let before = rt.rect_in(PARENT);
        rt.set_anchors_keep_rect(PARENT.size(), Vec2::ZERO, Vec2::new(1.0, 0.0));
        assert!(close(rt.rect_in(PARENT).min, before.min));
        assert!(close(rt.rect_in(PARENT).max, before.max));
        rt.set_pivot_keep_rect(Vec2::new(0.0, 1.0));
        assert!(close(rt.rect_in(PARENT).min, before.min));
    }

    #[test]
    fn presets_snap_or_keep_position() {
        let mut rt = RectTransform::sized(100.0, 40.0);
        apply_anchor_preset(
            &mut rt,
            PARENT.size(),
            AnchorAxis::Max,
            AnchorAxis::Max,
            true,
            true,
        );
        let r = rt.rect_in(PARENT);
        assert!(close(r.max, PARENT.max), "top-right corner: {r:?}");
        assert!(close(r.size(), Vec2::new(100.0, 40.0)));

        let mut rt = RectTransform::sized(100.0, 40.0);
        apply_anchor_preset(
            &mut rt,
            PARENT.size(),
            AnchorAxis::Stretch,
            AnchorAxis::Stretch,
            true,
            true,
        );
        assert_eq!(rt.rect_in(PARENT), PARENT);
        assert_eq!(
            AnchorAxis::of(rt.anchor_min.x, rt.anchor_max.x),
            Some(AnchorAxis::Stretch)
        );
    }

    #[test]
    fn canvas_scaler_matches_width_height_in_log_space() {
        let mut s = CanvasScaler {
            reference_resolution: Vec2::new(800.0, 600.0),
            match_width_or_height: 0.0,
            ..Default::default()
        };
        assert!((scale_factor(Some(&s), Vec2::new(1600.0, 600.0)) - 2.0).abs() < 1e-5);
        s.match_width_or_height = 1.0;
        assert!((scale_factor(Some(&s), Vec2::new(1600.0, 600.0)) - 1.0).abs() < 1e-5);
        s.match_width_or_height = 0.5;
        // Double width, half height → geometric mean 1.
        assert!((scale_factor(Some(&s), Vec2::new(1600.0, 300.0)) - 1.0).abs() < 1e-5);
        s.mode = UiScaleMode::ConstantPixelSize;
        s.scale_factor = 1.5;
        assert_eq!(scale_factor(Some(&s), Vec2::new(10.0, 10.0)), 1.5);
        assert_eq!(scale_factor(None, Vec2::new(10.0, 10.0)), 1.0);
    }

    #[test]
    fn rect_flip_and_contains() {
        let r = Rect::new(Vec2::new(10.0, 20.0), Vec2::new(30.0, 50.0));
        let f = r.flip_y(100.0);
        assert_eq!(f.min, Vec2::new(10.0, 50.0));
        assert_eq!(f.max, Vec2::new(30.0, 80.0));
        assert!(r.contains(Vec2::new(10.0, 20.0)) && !r.contains(Vec2::new(30.0, 25.0)));
    }
}
