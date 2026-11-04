//! Automatic probe placement (Lighting ▸ Generate Probes).
//!
//! 1. Columns on a grid at `spacing` over the bounds of every active renderer and terrain.
//! 2. A ray down each column finds every floor (up-facing surface); a candidate sits `height`
//!    above it, lower when the ceiling is close. Candidates inside solids (most axis rays hit
//!    back faces) or touching geometry are dropped.
//! 3. The box is fitted with 3 × 3 fans of rays along ±X ±Y ±Z (median distance, so a doorway or
//!    a chair doesn't decide alone): a side that hits within reach lines up with that wall, an open
//!    side reaches 0.75 × spacing so neighbours overlap. Fully enclosed probes get box
//!    projection and win over open ones.
//! 4. Merging: probes whose boxes are nearly the same room are one probe at their mean; open
//!    probes merge with neighbours up to two cells away, so open areas get a coarser grid.

use glam::Vec3;

use super::bvh::Bvh;
use crate::navigation::geometry::{self, BakeGeometry};
use crate::scene::{ReflectionProbe, ReflectionProbeGroup, Scene};

/// Box faces sit this far beyond the walls so the walls themselves are fully inside.
pub const MARGIN: f32 = 0.1;
/// Closest a probe may be to any surface.
const CLEARANCE: f32 = 0.2;
/// Ray fan offset across each side, as a fraction of the spacing (at most 1 m).
const FAN: f32 = 0.25;

/// One generated probe.
#[derive(Clone, Debug, PartialEq)]
pub struct PlacedProbe {
    pub position: Vec3,
    pub box_min: Vec3,
    pub box_max: Vec3,
    /// Every side hit a surface (a room): box projection on.
    pub enclosed: bool,
    pub blend_distance: f32,
}

impl PlacedProbe {
    /// Component for this probe (its object sits at `position`).
    pub fn component(&self, resolution: u32) -> ReflectionProbe {
        let center = (self.box_min + self.box_max) * 0.5;
        ReflectionProbe {
            size: self.box_max - self.box_min,
            center: center - self.position,
            box_projection: self.enclosed,
            blend_distance: self.blend_distance,
            importance: if self.enclosed { 2 } else { 1 },
            resolution,
            ..Default::default()
        }
    }
}

/// Triangles of every active renderer and terrain.
pub fn scene_geometry(scene: &Scene) -> BakeGeometry {
    geometry::collect_where(scene, |s, e| s.active_in_hierarchy(e))
}

const AXES: [Vec3; 6] = [
    Vec3::X,
    Vec3::NEG_X,
    Vec3::Y,
    Vec3::NEG_Y,
    Vec3::Z,
    Vec3::NEG_Z,
];

struct Placer<'a> {
    bvh: Bvh,
    lo: Vec3,
    hi: Vec3,
    s: &'a ReflectionProbeGroup,
}

/// A candidate before merging: position, box and open side count.
#[derive(Clone, Debug)]
struct Candidate {
    p: Vec3,
    min: Vec3,
    max: Vec3,
    open: usize,
}

impl Placer<'_> {
    fn spacing(&self) -> f32 {
        self.s.spacing.max(0.5)
    }

    fn cast(&self, o: Vec3, d: Vec3, max: f32) -> Option<f32> {
        self.bvh.cast(o, d, max).map(|h| h.t)
    }

    /// Up-facing surfaces under column (x, z), top first.
    fn floors(&self, x: f32, z: f32) -> Vec<f32> {
        let mut out = Vec::new();
        let mut y = self.hi.y + 1.0;
        let bottom = self.lo.y - 1.0;
        for _ in 0..32 {
            let Some(h) = self.bvh.cast(Vec3::new(x, y, z), Vec3::NEG_Y, y - bottom) else {
                break;
            };
            let at = y - h.t;
            if h.normal.y > 0.7 {
                out.push(at);
            }
            y = at - 0.01;
        }
        out
    }

    /// Most axis rays hit the inside of a surface, or something is too close.
    fn blocked(&self, p: Vec3) -> bool {
        let mut back = 0;
        for d in AXES {
            if let Some(h) = self.bvh.cast(p, d, 1e5) {
                if h.t < CLEARANCE {
                    return true;
                }
                back += (h.normal.dot(d) > 0.0) as usize;
            }
        }
        back >= 4
    }

    /// Box around `p`: per side the median of a 3 × 3 fan of rays (closed) or the open reach.
    fn fit(&self, p: Vec3) -> Candidate {
        let reach = (self.spacing() * 4.0).max(8.0);
        let fan = (self.spacing() * FAN).min(1.0);
        let open_reach = self.spacing() * 0.75;
        let (mut min, mut max, mut open) = (p, p, 0);
        for d in AXES {
            let axis = d.abs().max_position();
            let (a, b) = ((axis + 1) % 3, (axis + 2) % 3);
            let mut dists: Vec<f32> = (0..9)
                .map(|i| {
                    let mut o = p;
                    o[a] += (i % 3) as f32 * fan - fan;
                    o[b] += (i / 3) as f32 * fan - fan;
                    self.cast(o, d, reach).unwrap_or(f32::INFINITY)
                })
                .collect();
            dists.sort_by(f32::total_cmp);
            let side = if dists[4].is_finite() {
                dists[4] + MARGIN
            } else {
                open += 1;
                // Upwards, open space still covers trees and roofs.
                if d.y > 0.0 {
                    open_reach.max(4.0)
                } else {
                    open_reach
                }
            };
            let edge = p + d * side;
            min = min.min(edge);
            max = max.max(edge);
        }
        Candidate { p, min, max, open }
    }

    fn candidates(&self) -> Vec<Candidate> {
        let s = self.spacing();
        let center = (self.lo + self.hi) * 0.5;
        let extent = self.hi - self.lo;
        let n = |e: f32| ((e / s).ceil() as i32).max(1);
        let (nx, nz) = (n(extent.x), n(extent.z));
        let mut out = Vec::new();
        for iz in 0..nz {
            for ix in 0..nx {
                let x = center.x + (ix as f32 - (nx - 1) as f32 * 0.5) * s;
                let z = center.z + (iz as f32 - (nz - 1) as f32 * 0.5) * s;
                for floor in self.floors(x, z) {
                    let up = Vec3::new(x, floor + 0.05, z);
                    let room = self
                        .cast(up, Vec3::Y, 1e5)
                        .map_or(f32::INFINITY, |t| t + 0.05);
                    if room < 0.8 {
                        continue;
                    }
                    let h = self.s.height.max(0.2);
                    let y = if h + 0.25 < room {
                        floor + h
                    } else {
                        floor + room * 0.5
                    };
                    let p = Vec3::new(x, y, z);
                    if !self.blocked(p) {
                        out.push(self.fit(p));
                    }
                }
            }
        }
        out
    }

    /// Same room (each inside the other's box, boxes mostly shared), or open neighbours within
    /// two cells.
    fn similar(&self, a: &Candidate, b: &Candidate) -> bool {
        let iou = overlap(a, b);
        if a.open == 0 && b.open == 0 {
            let contains = |c: &Candidate, p: Vec3| p.cmpge(c.min).all() && p.cmple(c.max).all();
            return iou > 0.3 && contains(a, b.p) && contains(b, a.p);
        }
        let near =
            Vec3::new(a.p.x - b.p.x, 0.0, a.p.z - b.p.z).length() <= self.spacing() * 2.0 + 1e-3;
        a.open > 0 && b.open > 0 && near && (a.p.y - b.p.y).abs() < 1.0
    }

    /// Merge clusters into one probe each.
    fn merge(&self, cands: Vec<Candidate>) -> Vec<Candidate> {
        let mut clusters: Vec<Vec<Candidate>> = Vec::new();
        for c in cands {
            match clusters.iter_mut().find(|k| self.similar(&k[0], &c)) {
                Some(k) => k.push(c),
                None => clusters.push(vec![c]),
            }
        }
        clusters
            .into_iter()
            .map(|k| {
                if k.len() == 1 {
                    return k.into_iter().next().expect("one");
                }
                let mean = k.iter().map(|c| c.p).sum::<Vec3>() / k.len() as f32;
                let at = if self.blocked(mean) {
                    k.iter()
                        .map(|c| c.p)
                        .min_by(|a, b| a.distance(mean).total_cmp(&b.distance(mean)))
                        .expect("non-empty")
                } else {
                    mean
                };
                let mut m = self.fit(at);
                if m.open > 0 {
                    // Open clusters cover their members' cells.
                    for c in &k {
                        m.min = m.min.min(c.min);
                        m.max = m.max.max(c.max);
                    }
                }
                m
            })
            .collect()
    }
}

/// Intersection over union of two boxes.
fn overlap(a: &Candidate, b: &Candidate) -> f32 {
    let i = (a.max.min(b.max) - a.min.max(b.min))
        .max(Vec3::ZERO)
        .element_product();
    let va = (a.max - a.min).element_product();
    let vb = (b.max - b.min).element_product();
    i / (va + vb - i).max(1e-6)
}

/// Probes for `geo` with the group's settings (deterministic: same input, same probes).
pub fn place(geo: &BakeGeometry, settings: &ReflectionProbeGroup) -> Vec<PlacedProbe> {
    let Some((lo, hi)) = geo.bounds() else {
        return Vec::new();
    };
    let placer = Placer {
        bvh: Bvh::new(geo),
        lo,
        hi,
        s: settings,
    };
    let mut cands = placer.candidates();
    if settings.merge {
        cands = placer.merge(cands);
    }
    let spacing = placer.spacing();
    cands
        .into_iter()
        .map(|c| {
            let enclosed = c.open == 0;
            PlacedProbe {
                position: c.p,
                box_min: c.min,
                box_max: c.max,
                enclosed,
                blend_distance: if enclosed { MARGIN } else { spacing * 0.25 },
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A closed box room of walls 0.2 m thick (outer faces): inside x -3..3, y 0..3, z -4..4.
    pub(crate) fn room() -> BakeGeometry {
        let mut g = BakeGeometry::default();
        let mut slab = |lo: Vec3, hi: Vec3| {
            // Twelve triangles with outward normals.
            let c = |x: f32, y: f32, z: f32| {
                Vec3::new(
                    if x > 0.0 { hi.x } else { lo.x },
                    if y > 0.0 { hi.y } else { lo.y },
                    if z > 0.0 { hi.z } else { lo.z },
                )
            };
            let mid = (lo + hi) * 0.5;
            for axis in 0..3 {
                for side in [-1.0f32, 1.0] {
                    let (a, b) = ((axis + 1) % 3, (axis + 2) % 3);
                    let corner = |u: f32, v: f32| {
                        let mut k = Vec3::ZERO;
                        k[axis] = side;
                        k[a] = u;
                        k[b] = v;
                        c(k.x, k.y, k.z)
                    };
                    let mut out = Vec3::ZERO;
                    out[axis] = side;
                    let q = [
                        corner(-1., -1.),
                        corner(1., -1.),
                        corner(1., 1.),
                        corner(-1., 1.),
                    ];
                    let hint = (q[0] - mid).dot(out).signum() * out;
                    g.push([q[0], q[1], q[2]], hint);
                    g.push([q[0], q[2], q[3]], hint);
                }
            }
        };
        let t = 0.2;
        slab(
            Vec3::new(-3.0 - t, -t, -4.0 - t),
            Vec3::new(3.0 + t, 0.0, 4.0 + t),
        ); // floor
        slab(
            Vec3::new(-3.0 - t, 3.0, -4.0 - t),
            Vec3::new(3.0 + t, 3.0 + t, 4.0 + t),
        ); // ceiling
        slab(
            Vec3::new(-3.0 - t, 0.0, -4.0 - t),
            Vec3::new(-3.0, 3.0, 4.0 + t),
        );
        slab(
            Vec3::new(3.0, 0.0, -4.0 - t),
            Vec3::new(3.0 + t, 3.0, 4.0 + t),
        );
        slab(Vec3::new(-3.0, 0.0, -4.0 - t), Vec3::new(3.0, 3.0, -4.0));
        slab(Vec3::new(-3.0, 0.0, 4.0), Vec3::new(3.0, 3.0, 4.0 + t));
        // A solid pillar in the room.
        slab(Vec3::new(1.0, 0.0, 1.0), Vec3::new(2.0, 3.0, 2.0));
        g
    }

    fn inside_room(p: Vec3) -> bool {
        p.x > -3.0 && p.x < 3.0 && p.y > 0.0 && p.y < 3.0 && p.z > -4.0 && p.z < 4.0
    }

    #[test]
    fn probes_stay_out_of_walls_and_boxes_fit_the_room() {
        let settings = ReflectionProbeGroup {
            spacing: 1.5,
            merge: false,
            ..Default::default()
        };
        let placed = place(&room(), &settings);
        assert!(!placed.is_empty());
        for p in &placed {
            // Never inside a wall, the pillar or outside on the roof slab.
            let in_pillar =
                (1.0..2.0).contains(&p.position.x) && (1.0..2.0).contains(&p.position.z);
            assert!(!in_pillar, "{p:?}");
            assert!(inside_room(p.position) || p.position.y > 3.2, "{p:?}");
        }
        let indoor: Vec<_> = placed.iter().filter(|p| inside_room(p.position)).collect();
        assert!(indoor.len() > 10, "{}", indoor.len());
        for p in &indoor {
            assert!(
                (p.position.y - 1.5).abs() < 1e-3,
                "1.5 m above the floor: {p:?}"
            );
            assert!(p.enclosed, "{p:?}");
            // Floor and ceiling always line up.
            assert!((p.box_min.y - (0.0 - MARGIN)).abs() < 1e-3, "{p:?}");
            assert!((p.box_max.y - (3.0 + MARGIN)).abs() < 1e-3, "{p:?}");
        }
        // Probes in open parts of the room fit its walls on every side.
        let clear = indoor
            .iter()
            .filter(|p| p.position.x < 0.0 && p.position.z < 0.0)
            .collect::<Vec<_>>();
        assert!(!clear.is_empty());
        for p in clear {
            let want_min = Vec3::new(-3.0 - MARGIN, -MARGIN, -4.0 - MARGIN);
            let want_max = Vec3::new(3.0 + MARGIN, 3.0 + MARGIN, 4.0 + MARGIN);
            assert!((p.box_min - want_min).abs().max_element() < 1e-3, "{p:?}");
            assert!((p.box_max - want_max).abs().max_element() < 1e-3, "{p:?}");
        }
    }

    #[test]
    fn merging_leaves_few_probes_per_room() {
        let placed = place(&room(), &ReflectionProbeGroup::default());
        let indoor = placed.iter().filter(|p| inside_room(p.position)).count();
        assert!((1..=3).contains(&indoor), "{placed:?}");
        let all = place(
            &room(),
            &ReflectionProbeGroup {
                merge: false,
                ..Default::default()
            },
        );
        assert!(all.len() > placed.len());
        // Deterministic.
        assert_eq!(placed, place(&room(), &ReflectionProbeGroup::default()));
    }
}
