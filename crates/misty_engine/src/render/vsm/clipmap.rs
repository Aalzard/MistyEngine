//! Clipmap geometry of the sun's virtual shadow map: levels of 8192 × 8192 texels (64 × 64
//! pages of 128 × 128) centred on the camera, each level's texel twice the size of the one
//! before. Pages sit on a grid fixed in light space (the light view has a fixed origin, as the
//! cascades' does), so a page's content does not depend on where the camera is: a page
//! rendered once stays valid while it is cached, and the window of each level only decides
//! which cached pages the page table can address (toroidally: page `g` lives in table slot
//! `g mod 64`).
//!
//! Coordinates: `q` = light-space (x, -y) in metres, so that a page's texel rows grow with `q.y`
//! like the rows of the texture it is rendered into. Global page of `q` at level `l` =
//! `floor(q / page_world(l))`.

use glam::{IVec2, Mat4, Vec2, Vec3};

/// Texels on a page side.
pub const PAGE: u32 = 128;
/// Pages on a level side (the level's window and its page table).
pub const PAGES: i32 = 64;
/// Most clipmap levels.
pub const MAX_LEVELS: usize = 12;
/// Texel of level 0 at a resolution scale of 1 (metres).
pub const FINEST_TEXEL: f32 = 1.0 / 1024.0;
/// The PCSS blocker search and filter run at the finest level whose texel is at least
/// 1 / this of their radius (so kernels stay within a few pages).
pub const KERNEL_TEXELS: f32 = 8.0;
/// Blockers further above the receiver than this are not searched for (shadows.wgsl
/// `MAX_BLOCKER_DISTANCE`).
pub const MAX_BLOCKER_DISTANCE: f32 = 60.0;
/// The coarse "safety" level covers every page within the shadow distance with pages at least
/// this share of the distance wide (about 8 × 8 pages across the shadowed disk).
const SAFETY_PAGE_SHARE: f32 = 1.0 / 8.0;

/// The sun's light view (fixed origin), the same frame the cascades use.
pub fn light_view(dir: Vec3) -> Mat4 {
    let dir = dir.normalize_or(Vec3::NEG_Y);
    let up = if dir.y.abs() > 0.99 { Vec3::Z } else { Vec3::Y };
    glam::camera::lh::view::look_to_mat4(Vec3::ZERO, dir, up)
}

/// Light-space `q` coordinates (x, -y) of a light-view point.
pub fn q_of(light_space: Vec3) -> Vec2 {
    Vec2::new(light_space.x, -light_space.y)
}

/// Clipmap level a receiver at `dist` metres from the camera samples: the finest level whose
/// texel covers its pixel footprint (`pixel` = footprint per metre of distance). Must match
/// `vsm_level` in vsm_common.wgsl.
pub fn level_for(dist: f32, pixel: f32, finest: f32, levels: usize) -> usize {
    let footprint = (dist * pixel).max(1e-9);
    let l = (footprint / finest).log2().ceil();
    (l.max(0.0) as usize).min(levels.saturating_sub(1))
}

/// Level the PCSS search runs at for a light whose blockers reach `reach` metres sideways:
/// the finest level with `reach <= KERNEL_TEXELS` texels.
pub fn kernel_level(reach: f32, finest: f32, levels: usize) -> usize {
    level_for(reach / KERNEL_TEXELS, 1.0, finest, levels)
}

/// The coarse level that holds every page within `distance` of the camera (the fallback for
/// pixels whose own pages are not rendered yet, and for the fog).
pub fn safety_level(distance: f32, finest: f32) -> usize {
    let page = distance * SAFETY_PAGE_SHARE;
    let l = (page / (finest * PAGE as f32)).log2().ceil().max(0.0) as usize;
    l.min(MAX_LEVELS - 1)
}

/// The window of every level around one camera position.
#[derive(Clone, Debug, PartialEq)]
pub struct Clipmap {
    pub levels: usize,
    /// Texel of level 0 (metres).
    pub finest: f32,
    /// Light-view position of the camera.
    pub camera: Vec3,
    /// First global page of each level's window.
    pub origins: [IVec2; MAX_LEVELS],
}

impl Clipmap {
    pub fn new(light_view: &Mat4, camera: Vec3, levels: usize, finest: f32) -> Self {
        let camera = light_view.transform_point3(camera);
        let q = q_of(camera);
        let mut origins = [IVec2::ZERO; MAX_LEVELS];
        let levels = levels.clamp(1, MAX_LEVELS);
        for (l, o) in origins.iter_mut().enumerate().take(levels) {
            let pw = finest * PAGE as f32 * (1u32 << l) as f32;
            let page = (q.as_dvec2() / pw as f64).floor().as_ivec2();
            *o = page - IVec2::splat(PAGES / 2);
        }
        Self {
            levels,
            finest,
            camera,
            origins,
        }
    }

    pub fn texel(&self, level: usize) -> f32 {
        self.finest * (1u32 << level) as f32
    }

    /// Side of a page in metres.
    pub fn page_world(&self, level: usize) -> f32 {
        self.texel(level) * PAGE as f32
    }

    /// The global page lies inside the level's window.
    pub fn contains(&self, level: usize, page: IVec2) -> bool {
        let r = page - self.origins[level];
        level < self.levels && r.x >= 0 && r.y >= 0 && r.x < PAGES && r.y < PAGES
    }

    /// Page table slot of a global page.
    pub fn wrap(page: IVec2) -> IVec2 {
        IVec2::new(page.x.rem_euclid(PAGES), page.y.rem_euclid(PAGES))
    }

    /// The global page in the level's window whose table slot is `slot`.
    pub fn global_page(&self, level: usize, slot: IVec2) -> IVec2 {
        let o = self.origins[level];
        o + IVec2::new(
            (slot.x - o.x).rem_euclid(PAGES),
            (slot.y - o.y).rem_euclid(PAGES),
        )
    }

    /// Window origin relative to the camera in `q` metres (what the shaders add to a
    /// camera-relative point), computed in double precision.
    pub fn origin_offset(&self, level: usize) -> Vec2 {
        let pw = self.finest as f64 * PAGE as f64 * (1u64 << level) as f64;
        let o = self.origins[level].as_dvec2() * pw;
        (o - q_of(self.camera).as_dvec2()).as_vec2()
    }

    /// Global pages of level `level` whose texels cover the light-space `q` rectangle.
    pub fn page_range(&self, level: usize, lo: Vec2, hi: Vec2) -> (IVec2, IVec2) {
        let pw = self.page_world(level);
        ((lo / pw).floor().as_ivec2(), (hi / pw).floor().as_ivec2())
    }

    /// View-projection of the pages `lo..hi` (global, exclusive end) of `level`, depth
    /// `z_near..z_far` along the light: what a viewport of `(hi - lo) × PAGE` texels shows.
    pub fn rect_view_proj(
        &self,
        light_view: &Mat4,
        level: usize,
        lo: IVec2,
        hi: IVec2,
        z: (f32, f32),
    ) -> Mat4 {
        let pw = self.page_world(level) as f64;
        let (x0, x1) = (lo.x as f64 * pw, hi.x as f64 * pw);
        // q.y = -y: page row lo.y is the top of the viewport (largest light-space y).
        let (top, bottom) = (-(lo.y as f64) * pw, -(hi.y as f64) * pw);
        let proj = glam::camera::lh::proj::directx::orthographic(
            x0 as f32,
            x1 as f32,
            bottom as f32,
            top as f32,
            z.0,
            z.1,
        );
        proj * *light_view
    }
}

/// Every global page of `level` whose square meets the disk of `radius` metres around the
/// camera (in `q`), inside the level's window.
pub fn disk_pages(map: &Clipmap, level: usize, radius: f32) -> Vec<IVec2> {
    let c = q_of(map.camera);
    let pw = map.page_world(level);
    let (lo, hi) = map.page_range(level, c - Vec2::splat(radius), c + Vec2::splat(radius));
    let mut out = Vec::new();
    for y in lo.y..=hi.y {
        for x in lo.x..=hi.x {
            let p = IVec2::new(x, y);
            if !map.contains(level, p) {
                continue;
            }
            let min = p.as_vec2() * pw;
            let nearest = c.clamp(min, min + Vec2::splat(pw));
            if nearest.distance(c) <= radius {
                out.push(p);
            }
        }
    }
    out
}

/// Light-space depth range for the casters' bounds, padded and snapped to 64 m steps so small
/// changes of the scene keep it (a change re-renders every page).
pub fn depth_range(light_view: &Mat4, casters: &[(Vec3, Vec3)]) -> Option<(f32, f32)> {
    let mut lo = f32::MAX;
    let mut hi = f32::MIN;
    for (a, b) in casters {
        for corner in 0..8 {
            let p = Vec3::new(
                if corner & 1 == 0 { a.x } else { b.x },
                if corner & 2 == 0 { a.y } else { b.y },
                if corner & 4 == 0 { a.z } else { b.z },
            );
            let z = light_view.transform_point3(p).z;
            lo = lo.min(z);
            hi = hi.max(z);
        }
    }
    if lo > hi || !lo.is_finite() || !hi.is_finite() {
        return None;
    }
    const STEP: f32 = 64.0;
    let lo = ((lo - 2.0) / STEP).floor() * STEP;
    let hi = ((hi + 2.0) / STEP).ceil() * STEP;
    Some((lo, hi.max(lo + STEP)))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sun() -> Vec3 {
        Vec3::new(0.3, -0.6, 0.5).normalize()
    }

    #[test]
    fn levels_double_and_follow_the_footprint() {
        let k = 1.0e-3;
        assert_eq!(level_for(0.0, k, FINEST_TEXEL, 9), 0);
        assert_eq!(level_for(0.9, k, FINEST_TEXEL, 9), 0);
        // 10 m: 1 cm footprint, 2^-10 · 2^4 = 1.56 cm.
        assert_eq!(level_for(10.0, k, FINEST_TEXEL, 9), 4);
        assert_eq!(level_for(1.0e6, k, FINEST_TEXEL, 9), 8);
        // A texel always covers the footprint (one level finer would not).
        for d in [0.5f32, 3.0, 17.0, 140.0] {
            let l = level_for(d, k, FINEST_TEXEL, 12);
            let t = FINEST_TEXEL * (1 << l) as f32;
            assert!(t >= d * k * 0.999, "{d}: {t}");
            assert!(l == 0 || t * 0.5 < d * k, "{d}: {t}");
        }
    }

    #[test]
    fn the_window_contains_the_level_a_pixel_picks() {
        // Wherever the camera is, a receiver within the distance that picks level l lies in l's
        // window (8192 texels against a 1080p pixel leave a wide margin).
        let view = light_view(sun());
        let cam = Vec3::new(123.4, 5.0, -77.7);
        let map = Clipmap::new(&view, cam, 9, FINEST_TEXEL);
        let k = 2.0 * (30f32).to_radians().tan() / 1080.0;
        for &(dx, dz) in &[(0.5, 0.2), (3.0, -2.0), (25.0, 30.0), (-90.0, 100.0)] {
            let p = cam + Vec3::new(dx, -1.5, dz);
            let l = level_for(p.distance(cam), k, FINEST_TEXEL, 9);
            let q = q_of(view.transform_point3(p));
            let page = (q / map.page_world(l)).floor().as_ivec2();
            assert!(map.contains(l, page), "{p:?} level {l}");
        }
    }

    #[test]
    fn pages_stay_on_a_world_fixed_grid() {
        let view = light_view(sun());
        let a = Clipmap::new(&view, Vec3::new(1.0, 2.0, 3.0), 9, FINEST_TEXEL);
        let b = Clipmap::new(&view, Vec3::new(1.37, 2.0, 2.11), 9, FINEST_TEXEL);
        let lo = IVec2::new(-5, 7);
        let hi = lo + IVec2::new(2, 3);
        for level in 0..9 {
            // Same pages, same projection, whatever the camera.
            let ma = a.rect_view_proj(&view, level, lo, hi, (-100.0, 100.0));
            let mb = b.rect_view_proj(&view, level, lo, hi, (-100.0, 100.0));
            assert_eq!(ma, mb);
            // The rect's top-left corner is the first page's corner.
            let pw = a.page_world(level);
            let corner = Vec3::new(lo.x as f32 * pw, -(lo.y as f32) * pw, 0.0);
            let world = view.inverse().transform_point3(corner);
            let ndc = ma.project_point3(world);
            assert!(
                (ndc.x + 1.0).abs() < 1e-3 && (ndc.y - 1.0).abs() < 1e-3,
                "{ndc:?}"
            );
        }
    }

    #[test]
    fn slots_wrap_and_unwrap() {
        let view = light_view(sun());
        let map = Clipmap::new(&view, Vec3::new(-300.0, 0.0, 41.0), 9, FINEST_TEXEL);
        for level in 0..9 {
            let o = map.origins[level];
            for p in [
                o,
                o + IVec2::new(63, 0),
                o + IVec2::new(17, 63),
                o + IVec2::splat(31),
            ] {
                assert!(map.contains(level, p));
                assert_eq!(map.global_page(level, Clipmap::wrap(p)), p);
            }
            assert!(!map.contains(level, o - IVec2::X));
            assert!(!map.contains(level, o + IVec2::new(0, 64)));
            // The camera sits in the middle of the window.
            let off = map.origin_offset(level);
            let half = map.page_world(level) * PAGES as f32 * 0.5;
            assert!(off.x <= -half + 1e-3 && off.x > -half - map.page_world(level) - 1e-3);
        }
    }

    #[test]
    fn the_safety_level_covers_the_distance_with_few_pages() {
        let view = light_view(Vec3::NEG_Y);
        for distance in [40.0f32, 70.0, 150.0, 250.0] {
            let l = safety_level(distance, FINEST_TEXEL);
            let map = Clipmap::new(&view, Vec3::ZERO, l + 1, FINEST_TEXEL);
            let pages = disk_pages(&map, l, distance);
            assert!(
                (40..=400).contains(&pages.len()),
                "{distance}: {}",
                pages.len()
            );
            // Whole disk inside the window.
            let half = map.page_world(l) * (PAGES / 2 - 1) as f32;
            assert!(half > distance);
        }
    }

    #[test]
    fn depth_range_is_snapped_and_contains_the_casters() {
        let view = light_view(sun());
        let boxes = [(Vec3::new(-10.0, 0.0, -10.0), Vec3::new(10.0, 30.0, 10.0))];
        let (lo, hi) = depth_range(&view, &boxes).unwrap();
        assert_eq!(lo % 64.0, 0.0);
        assert_eq!(hi % 64.0, 0.0);
        let moved = [(Vec3::new(-10.0, 0.0, -10.0), Vec3::new(10.0, 30.5, 10.0))];
        assert_eq!(depth_range(&view, &moved), Some((lo, hi)));
        assert!(depth_range(&view, &[]).is_none());
    }
}
