//! Tree meshes from a [`TreeAsset`]: a trunk, main limbs reaching for the crown's outline,
//! side branches, and leaf-cluster cards on the outer branches (each card a small twig of
//! leaves from the leaf atlas, folded along its middle so it has some depth).
//!
//! The look aims for a soft, natural crown that reads as one volume: leaf normals lean towards
//! the crown outline's normal (a dome lit as a whole, not thousands of flat cards), and every
//! vertex carries an ambient occlusion term from its depth inside the crown. Wind weights per
//! vertex let the shader bend the trunk, sway each limb with its side branches (one phase per
//! limb, so nothing tears apart) and flutter the clusters.
//!
//! Two mesh LODs: LOD 0 (everything) and LOD 1 (fewer sides, only thick side branches, a third
//! of the cards made larger to cover the same crown). The far LOD is the impostor
//! (`impostor.rs`), baked from LOD 0.

use std::ops::Range;

use glam::{Vec2, Vec3};

use super::{CrownShape, TreeAsset};

/// One vertex of a tree mesh (`tree.wgsl` `TreeIn`).
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, bytemuck::Pod, bytemuck::Zeroable)]
pub struct TreeVertex {
    pub position: [f32; 3],
    pub normal: [f32; 3],
    /// Bark: tiled texture coordinates. Leaves: atlas coordinates (2 × 2 cells).
    pub uv: [f32; 2],
    /// x = branch sway weight (metres of sway per unit of wind), y = phase of the limb 0..1,
    /// z = leaf flutter weight 0..1 (0 on bark), w = height share 0 (foot) .. 1 (top).
    pub wind: [f32; 4],
    /// x = ambient occlusion, y = per-cluster colour variation, z = 255 on leaves, w unused.
    pub extra: [u8; 4],
}

/// One level of detail: bark triangles first, then leaf triangles.
#[derive(Clone, Debug, Default)]
pub struct TreeMesh {
    pub vertices: Vec<TreeVertex>,
    pub indices: Vec<u32>,
    /// Index ranges of the bark and the leaves.
    pub bark: Range<u32>,
    pub leaves: Range<u32>,
}

impl TreeMesh {
    pub fn triangles(&self) -> usize {
        self.indices.len() / 3
    }
}

/// Everything the renderer needs to draw a tree.
#[derive(Clone, Debug)]
pub struct TreeModel {
    pub lods: [TreeMesh; 2],
    pub impostor: super::impostor::Impostor,
    /// Object-space bounds (foot at the origin).
    pub min: Vec3,
    pub max: Vec3,
    /// Crown centre and radii (horizontal, vertical) of the outline the leaves fill.
    pub crown_center: Vec3,
    pub crown_radii: Vec2,
    pub height: f32,
}

impl TreeModel {
    /// Radius of a sphere around the foot that holds the whole tree.
    pub fn bounding_radius(&self) -> f32 {
        let c = (self.min + self.max) * 0.5;
        c.length() + (self.max - self.min).length() * 0.5
    }
}

/// Small deterministic generator (SplitMix64).
pub(super) struct Rng(u64);

impl Rng {
    pub fn new(seed: u64) -> Self {
        Self(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) ^ 0x2545_F491_4F6C_DD1D)
    }

    pub fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    /// 0..1
    pub fn f(&mut self) -> f32 {
        (self.next() >> 40) as f32 / (1u64 << 24) as f32
    }

    pub fn range(&mut self, lo: f32, hi: f32) -> f32 {
        lo + (hi - lo) * self.f()
    }

    /// -1..1
    pub fn signed(&mut self) -> f32 {
        self.f() * 2.0 - 1.0
    }

    /// A random unit vector.
    pub fn dir(&mut self) -> Vec3 {
        loop {
            let v = Vec3::new(self.signed(), self.signed(), self.signed());
            let l = v.length_squared();
            if l > 1e-3 && l <= 1.0 {
                return v / l.sqrt();
            }
        }
    }
}

/// The crown outline: a surface of revolution around the trunk axis.
#[derive(Clone, Copy, Debug)]
pub(super) struct Crown {
    pub shape: CrownShape,
    pub bottom: f32,
    pub top: f32,
    /// Widest radius.
    pub radius: f32,
}

impl Crown {
    fn new(t: &TreeAsset) -> Self {
        Self {
            shape: t.crown_shape,
            bottom: t.height * t.crown_base,
            top: t.height,
            radius: t.height * t.crown_width * 0.5,
        }
    }

    pub fn center(&self) -> Vec3 {
        Vec3::new(0.0, (self.bottom + self.top) * 0.5, 0.0)
    }

    pub fn half_height(&self) -> f32 {
        (self.top - self.bottom) * 0.5
    }

    /// Outline radius at height `y`.
    pub fn radius_at(&self, y: f32) -> f32 {
        let u = (y - self.bottom) / (self.top - self.bottom).max(1e-3);
        self.radius * self.shape.profile(u)
    }

    /// 0 at the trunk axis / crown middle .. 1 on the outline (> 1 outside).
    pub fn depth(&self, p: Vec3) -> f32 {
        let r = self.radius_at(p.y.clamp(self.bottom, self.top)).max(0.05);
        let h = Vec2::new(p.x, p.z).length() / r;
        let v = (p.y - self.center().y) / self.half_height().max(0.05);
        // Mostly the horizontal distance: the outline is what matters.
        (h * h * 0.8 + v * v * 0.2).sqrt().max(h)
    }

    /// Outward normal of the crown's ellipsoid through `p` (soft, dome-like shading).
    pub fn normal(&self, p: Vec3) -> Vec3 {
        let c = self.center();
        let d = p - c;
        let n = Vec3::new(
            d.x / (self.radius * self.radius).max(1e-3),
            d.y / (self.half_height() * self.half_height()).max(1e-3),
            d.z / (self.radius * self.radius).max(1e-3),
        );
        n.try_normalize().unwrap_or(Vec3::Y)
    }

    /// Distance from `p` along `dir` to the outline (marched; 0 when already outside).
    pub fn reach(&self, p: Vec3, dir: Vec3) -> f32 {
        let step = (self.radius.max(self.half_height()) * 0.04).max(0.05);
        let mut t = 0.0;
        while t < self.radius * 4.0 + self.top {
            let q = p + dir * t;
            if q.y > self.top || self.depth(q) >= 1.0 {
                return t;
            }
            t += step;
        }
        t
    }

    /// Ambient occlusion at `p`: dark deep inside the crown and under it, bright at its rim.
    pub fn occlusion(&self, p: Vec3) -> f32 {
        let depth = self.depth(p).clamp(0.0, 1.2);
        let inside = smoothstep(0.15, 1.05, depth);
        // The lower half of the crown and the trunk under it get less sky.
        let up = smoothstep(self.bottom - self.half_height() * 0.6, self.top, p.y);
        (0.3 + 0.7 * inside) * (0.62 + 0.38 * up)
    }
}

fn smoothstep(a: f32, b: f32, x: f32) -> f32 {
    let t = ((x - a) / (b - a)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// A branch as a polyline with radii and wind weights.
#[derive(Clone, Debug)]
pub(super) struct Limb {
    pub points: Vec<Vec3>,
    pub radii: Vec<f32>,
    /// Sway weight at each point.
    pub weights: Vec<f32>,
    pub phase: f32,
    /// 0 = trunk, 1 = main limb, 2 = side branch.
    pub level: u8,
}

impl Limb {
    fn length(&self) -> f32 {
        self.points.windows(2).map(|w| w[0].distance(w[1])).sum()
    }

    /// Point, tangent, radius and weight at `s` (0..1 of the length).
    fn at(&self, s: f32) -> (Vec3, Vec3, f32, f32) {
        let n = self.points.len() - 1;
        let x = s.clamp(0.0, 1.0) * n as f32;
        let i = (x.floor() as usize).min(n - 1);
        let f = x - i as f32;
        let p = self.points[i].lerp(self.points[i + 1], f);
        let t = (self.points[i + 1] - self.points[i])
            .try_normalize()
            .unwrap_or(Vec3::Y);
        let r = self.radii[i] + (self.radii[i + 1] - self.radii[i]) * f;
        let w = self.weights[i] + (self.weights[i + 1] - self.weights[i]) * f;
        (p, t, r, w)
    }
}

/// A leaf cluster card: bottom-centre anchor, its up (along the twig) and facing directions.
#[derive(Clone, Copy, Debug)]
pub(super) struct Card {
    pub anchor: Vec3,
    pub up: Vec3,
    pub facing: Vec3,
    pub size: f32,
    pub cell: u32,
    pub weight: f32,
    pub phase: f32,
    pub variation: f32,
}

/// The tree's skeleton and leaf cards, before meshing.
pub(super) struct Skeleton {
    pub crown: Crown,
    pub limbs: Vec<Limb>,
    pub cards: Vec<Card>,
}

fn cubic(p0: Vec3, p1: Vec3, p2: Vec3, p3: Vec3, t: f32) -> Vec3 {
    let u = 1.0 - t;
    p0 * (u * u * u) + p1 * (3.0 * u * u * t) + p2 * (3.0 * u * t * t) + p3 * (t * t * t)
}

/// Any unit vector perpendicular to `v`.
fn perpendicular(v: Vec3) -> Vec3 {
    let a = if v.y.abs() < 0.9 { Vec3::Y } else { Vec3::X };
    v.cross(a).normalize()
}

/// Rotate `v` about the unit `axis` by `angle` radians.
fn rotate(v: Vec3, axis: Vec3, angle: f32) -> Vec3 {
    glam::Quat::from_axis_angle(axis, angle) * v
}

/// Trunk rings: height share = (ring share)^this (closer together at the foot).
const TRUNK_RINGS: f32 = 1.4;

pub(super) fn skeleton(t: &TreeAsset) -> Skeleton {
    let mut rng = Rng::new(t.seed as u64 * 7919 + 17);
    let crown = Crown::new(t);
    let h = t.height;
    let mut limbs = Vec::new();

    // Trunk: a gentle lean plus a slow wobble; it starts a little under the ground so it
    // stays planted on slopes. Decurrent trees end it a short way into the crown.
    let trunk_top = if t.leader >= 0.5 {
        crown.top - (crown.top - crown.bottom) * 0.04
    } else {
        crown.bottom + (crown.top - crown.bottom) * (0.18 + 0.4 * t.leader)
    };
    let lean_dir = {
        let a = rng.range(0.0, std::f32::consts::TAU);
        Vec3::new(a.cos(), 0.0, a.sin())
    };
    let tau = std::f32::consts::TAU;
    let (w1, w2) = (rng.range(0.0, tau), rng.range(0.0, tau));
    let side = perpendicular(lean_dir.with_y(0.0).normalize_or(Vec3::X));
    let trunk_segments = 18;
    let mut trunk = Limb {
        points: Vec::new(),
        radii: Vec::new(),
        weights: Vec::new(),
        phase: 0.0,
        level: 0,
    };
    let top_radius = if t.leader >= 0.5 { 0.12 } else { 0.62 };
    for i in 0..=trunk_segments {
        // Rings closer together at the foot (the root flare's shape).
        let s = (i as f32 / trunk_segments as f32).powf(TRUNK_RINGS);
        let y = -0.4 + (trunk_top + 0.4) * s;
        let k = (y.max(0.0) / h).powf(1.5);
        let wobble = (s * 7.0 + w1).sin() * 0.5 + (s * 13.0 + w2).sin() * 0.25;
        let offset = lean_dir * t.trunk_bend * h * 0.07 * k
            + side * wobble * t.trunk_bend * t.trunk_radius * 0.8 * s;
        trunk.points.push(Vec3::new(0.0, y, 0.0) + offset);
        // Taper, plus the root flare at the foot (its buttresses are added when meshing).
        let taper = 1.0 - (1.0 - top_radius) * s.powf(0.9);
        let flare = 1.0 + 0.4 * (-(y.max(0.0)) / 0.5).exp();
        trunk.radii.push(t.trunk_radius * taper * flare);
        trunk.weights.push(0.0);
    }
    limbs.push(trunk);

    // Main limbs: phyllotaxis around the trunk, each aimed at a point of the crown outline.
    let n = t.branches.max(1) as usize;
    let golden = 2.399_963;
    let start_y = (crown.bottom * 0.92).max(0.5);
    let mut main = Vec::new();
    for i in 0..n {
        let f = (i as f32 + rng.range(0.1, 0.9)) / n as f32;
        // Attach height: leader trees spread limbs up the whole trunk; decurrent ones
        // gather them towards the top of the trunk (where it divides).
        let attach_y = if t.leader >= 0.5 {
            start_y + (trunk_top - start_y) * f.powf(0.95) * 0.94
        } else {
            start_y + (trunk_top - start_y) * f.powf(0.55)
        };
        let s = ((attach_y + 0.4) / (trunk_top + 0.4)).clamp(0.0, 1.0);
        let (base, _, trunk_r, _) = limbs[0].at(s.powf(1.0 / TRUNK_RINGS));
        let azimuth = i as f32 * golden + rng.range(-0.35, 0.35);
        let out = Vec3::new(azimuth.cos(), 0.0, azimuth.sin());
        // Lower limbs leave flatter, upper ones steeper.
        let height_share = ((attach_y - crown.bottom) / (crown.top - crown.bottom)).clamp(0.0, 1.0);
        let angle = (t.branch_angle * (1.15 - 0.45 * height_share) + rng.range(-8.0, 8.0))
            .to_radians()
            .clamp(0.1, 1.9);
        let leave = (Vec3::Y * angle.cos() + out * angle.sin()).normalize();
        // Target on the outline in this limb's direction. Leader trees: a little above the
        // attach point. Decurrent ones spread their limbs' ends over the whole dome (low,
        // middle and top, by index), so the crown fills without a hole on top.
        let target = if t.leader >= 0.5 {
            let ty =
                (attach_y + (crown.top - attach_y) * rng.range(0.05, 0.35)).min(crown.top - 0.3);
            let reach = crown.radius_at(ty) * rng.range(0.7, 0.98);
            Vec3::new(out.x * reach, ty, out.z * reach)
        } else {
            let k = (i as f32 * 0.618_034 + rng.range(0.0, 0.2)).fract();
            let elevation = (-0.1 + 1.25 * k).min(1.32);
            let c = crown.center();
            let ty =
                (c.y + elevation.sin() * crown.half_height()).clamp(crown.bottom, crown.top - 0.3);
            let reach = crown.radius_at(ty) * elevation.cos().max(0.3) * rng.range(0.72, 0.95);
            Vec3::new(out.x * reach, ty, out.z * reach)
        };
        let span = target - base;
        let len = span.length().max(0.3);
        let curve = t.branch_curve;
        let p1 = base + leave * len * 0.4;
        let p2 = base + span * 0.65 + Vec3::Y * curve * len * 0.2;
        let mut limb = Limb {
            points: Vec::new(),
            radii: Vec::new(),
            weights: Vec::new(),
            phase: rng.f(),
            level: 1,
        };
        // Decurrent trees (oaks) divide into a few heavy limbs that stay thick a long way out.
        let r0 = trunk_r * if t.leader >= 0.5 { 0.42 } else { 0.72 } * rng.range(0.85, 1.1);
        let taper = if t.leader >= 0.5 { 0.85 } else { 1.2 };
        // A slow, irregular kink sideways and up and down (gnarled, not a smooth arc).
        let kink_a = perpendicular((target - base).normalize_or(Vec3::Y));
        let kink_b = (target - base).normalize_or(Vec3::Y).cross(kink_a);
        let (k1, k2) = (rng.range(0.0, tau), rng.range(0.0, tau));
        let gnarl = len * (0.025 + 0.03 * (1.0 - t.leader));
        let segments = 10;
        for j in 0..=segments {
            let u = j as f32 / segments as f32;
            // Droop sags the outer half under its own weight.
            let sag = if curve < 0.0 {
                Vec3::Y * curve * len * 0.25 * u * u
            } else {
                Vec3::ZERO
            };
            let bend = (std::f32::consts::PI * u).sin() * gnarl;
            let kink =
                kink_a * (u * 9.0 + k1).sin() * bend + kink_b * (u * 6.5 + k2).sin() * bend * 0.7;
            let mut p = cubic(base, p1, p2, target, u) + sag + kink;
            if j == 0 {
                // Start inside the trunk so the joint is hidden.
                p = base - leave * trunk_r * 0.3;
            }
            limb.points.push(p);
            limb.radii.push(r0 * (1.0 - 0.86 * u.powf(taper)));
            limb.weights.push(u.powf(1.5) * len / h);
        }
        main.push(limb);
    }

    // Side branches along each limb, towards the outline.
    let longest = main.iter().map(Limb::length).fold(0.1, f32::max);
    let mut side_branches = Vec::new();
    for (li, limb) in main.iter().enumerate() {
        let len = limb.length();
        let count = ((t.twigs as f32 * (len / longest).powf(0.8)).round() as usize).max(2);
        let mut turn = rng.range(0.0, std::f32::consts::TAU);
        for k in 0..count {
            let s = 0.22 + 0.74 * (k as f32 + rng.range(0.2, 0.8)) / count as f32;
            let (p, tangent, r, w) = limb.at(s);
            turn += 2.4 + rng.range(-0.4, 0.4);
            let around = rotate(perpendicular(tangent), tangent, turn);
            let spread = rng.range(0.6, 1.05);
            let mut dir = (tangent * spread.cos() + around * spread.sin()).normalize();
            // Towards the light, or hanging for drooping species.
            dir = (dir + Vec3::Y * t.branch_curve * 0.45 + crown.normal(p) * 0.25).normalize();
            let room = crown.reach(p, dir);
            let want = len * (0.6 - 0.32 * s) * rng.range(0.75, 1.15);
            let l2 = want.min(room * 1.05).max(0.35);
            let r2 = (r * 0.6).clamp(0.008, 0.13);
            let mut b = Limb {
                points: Vec::new(),
                radii: Vec::new(),
                weights: Vec::new(),
                phase: limb.phase,
                level: 2,
            };
            let segs = 5;
            let bend = Vec3::Y * t.branch_curve * l2 * 0.18 + rng.dir() * l2 * 0.08;
            for j in 0..=segs {
                let u = j as f32 / segs as f32;
                let mut q = p + dir * l2 * u + bend * u * u;
                if j == 0 {
                    q = p - dir * r * 0.5;
                }
                b.points.push(q);
                b.radii.push(r2 * (1.0 - 0.8 * u));
                b.weights.push(w + u.powf(1.5) * l2 / h * 0.9);
            }
            side_branches.push((li, b));
        }
    }

    // Leaf cards: along the outer part of the side branches and the limbs' tips, spaced so
    // the crown shell is covered `leaf_density` times.
    let shell = {
        // Area of the crown's ellipsoid (Knud Thomsen's approximation).
        let (a, c) = (crown.radius, crown.half_height());
        let p = 1.6;
        4.0 * std::f32::consts::PI * (((a * a).powf(p) + 2.0 * (a * c).powf(p)) / 3.0).powf(1.0 / p)
    };
    let card_area = t.leaf_size * t.leaf_size * 0.62;
    let wanted = (t.leaf_density * shell / card_area * 2.0).round().max(8.0) as usize;
    let mut stems: Vec<(&Limb, f32)> = side_branches.iter().map(|(_, b)| (b, 0.25)).collect();
    stems.extend(main.iter().map(|l| (l, 0.62)));
    let total: f32 = stems
        .iter()
        .map(|(b, from)| b.length() * (1.0 - from))
        .sum();
    let spacing = (total / wanted as f32).max(0.02);
    let mut cards = Vec::new();
    let mut carry = rng.f() * spacing;
    for (b, from) in stems {
        let len = b.length() * (1.0 - from);
        let mut d = carry;
        while d < len {
            let s = from + d / b.length();
            let (p, tangent, _, w) = b.at(s);
            let out = crown.normal(p);
            let jitter = rng.dir();
            // A little out from the twig: clusters hang around the branch, fuller outline.
            let anchor = p + jitter * t.leaf_size * 0.25 + out * t.leaf_size * 0.2;
            let up = (tangent * 0.45 + out * 0.55 + Vec3::Y * 0.25 + jitter * 0.45)
                .try_normalize()
                .unwrap_or(Vec3::Y);
            // Face outwards (seen from outside the crown), turned a little at random.
            let across = (out - up * out.dot(up))
                .try_normalize()
                .unwrap_or(perpendicular(up));
            let facing = rotate(across, up, rng.range(-0.9, 0.9));
            // Clusters deep inside the crown are sparser (they get little light); towards the
            // outline they get smaller and closer together (the same cover), so the silhouette
            // breaks up into many small sprays instead of a few big puffs.
            let inner = crown.depth(anchor);
            let rim = smoothstep(0.7, 1.05, inner);
            let scale = 1.08 - 0.43 * rim;
            if inner < 0.55 && rng.f() > inner / 0.55 {
                d += spacing * rng.range(0.6, 1.4);
                continue;
            }
            cards.push(Card {
                anchor,
                up,
                facing,
                size: t.leaf_size * rng.range(0.8, 1.2) * scale,
                cell: (rng.next() % 4) as u32,
                weight: w,
                phase: b.phase,
                variation: rng.f(),
            });
            d += spacing * rng.range(0.6, 1.4) * scale * scale;
        }
        carry = d - len;
    }

    limbs.extend(main);
    limbs.extend(side_branches.into_iter().map(|(_, b)| b));
    Skeleton {
        crown,
        limbs,
        cards,
    }
}

/// Mesh-building options of one LOD.
struct Detail {
    /// Sides around the trunk, a main limb and a side branch (0 = skip that level).
    sides: [u32; 3],
    /// Side branches thinner than this are skipped (metres).
    min_radius: f32,
    /// Keep one card in this many (the kept ones grow to cover the same area).
    card_stride: usize,
    /// Use every n-th ring along a limb (the last one always).
    ring_step: usize,
}

const LOD0: Detail = Detail {
    sides: [16, 8, 5],
    min_radius: 0.0,
    card_stride: 1,
    ring_step: 1,
};
const LOD1: Detail = Detail {
    sides: [7, 4, 3],
    min_radius: 0.04,
    card_stride: 3,
    ring_step: 2,
};

/// Metres of bark one texture repeat covers round a 0.4 m trunk.
const BARK_TILE: f32 = 0.9;

fn encode(v: f32) -> u8 {
    (v.clamp(0.0, 1.0) * 255.0).round() as u8
}

fn mesh(t: &TreeAsset, sk: &Skeleton, detail: &Detail) -> TreeMesh {
    let mut m = TreeMesh::default();
    let h = t.height;
    // Bark: a tube per limb. The (square) texture repeats every `tile` metres round the
    // trunk, a whole number of times round each limb: coarser bark on thicker trunks, finer on
    // thin branches.
    let tile = BARK_TILE * (t.trunk_radius / 0.4).clamp(0.3, 3.0).sqrt();
    // Buttresses: a few rounded lobes round the trunk's foot.
    let lobes = 4.0 + (t.seed % 3) as f32;
    let lobe_phase = t.seed as f32 * 1.7;
    let tau = std::f32::consts::TAU;
    for limb in &sk.limbs {
        let sides = detail.sides[limb.level as usize];
        if sides == 0 || (limb.level == 2 && limb.radii[0] < detail.min_radius) {
            continue;
        }
        let keep: Vec<usize> = (0..limb.points.len())
            .filter(|&i| i % detail.ring_step == 0 || i + 1 == limb.points.len())
            .collect();
        let limb = &Limb {
            points: keep.iter().map(|&i| limb.points[i]).collect(),
            radii: keep.iter().map(|&i| limb.radii[i]).collect(),
            weights: keep.iter().map(|&i| limb.weights[i]).collect(),
            phase: limb.phase,
            level: limb.level,
        };
        let girth = tau * limb.radii[0].max(0.01);
        let wraps = (girth / tile).round().max(1.0);
        let v_per_m = wraps / girth;
        let first = m.vertices.len() as u32;
        let mut v_acc = 0.0;
        let mut frame_n = perpendicular(
            (limb.points[1] - limb.points[0])
                .try_normalize()
                .unwrap_or(Vec3::Y),
        );
        let rings = limb.points.len();
        let n_sides = sides as usize;
        let ring = n_sides + 1;
        // Ring positions first, then the vertices with normals from the surface itself (the
        // flare, the buttresses and the taper tilt them).
        let mut pos = Vec::with_capacity(rings * ring);
        let mut radial = Vec::with_capacity(rings * ring);
        let mut shade = Vec::with_capacity(rings * ring);
        let mut v_of = Vec::with_capacity(rings);
        // Lobes need enough sides to be round (LOD 1 gets a hint of them).
        let lobe_amount = if limb.level == 0 {
            0.5 * (sides as f32 / 14.0).min(1.0)
        } else {
            0.0
        };
        for i in 0..rings {
            let p = limb.points[i];
            let dir = if i + 1 < rings {
                limb.points[i + 1] - p
            } else {
                p - limb.points[i - 1]
            }
            .try_normalize()
            .unwrap_or(Vec3::Y);
            // Parallel transport keeps the rings from twisting.
            frame_n = (frame_n - dir * frame_n.dot(dir))
                .try_normalize()
                .unwrap_or(perpendicular(dir));
            let frame_b = dir.cross(frame_n);
            if i > 0 {
                v_acc += limb.points[i - 1].distance(p) * v_per_m;
            }
            v_of.push(v_acc);
            // The crown's occlusion is meant for the leaf shell: the bark (near the axis, deep
            // "inside" by that measure) gets a milder share of it, or shaded trunks go black.
            let ao = (0.6 + 0.4 * sk.crown.occlusion(p)) * if limb.level == 0 { 1.0 } else { 0.95 };
            // The trunk's foot is darker where it meets the grass.
            let foot = smoothstep(-0.3, 1.2, p.y) * 0.35 + 0.65;
            let buttress = lobe_amount * (-(p.y.max(0.0)) / 0.7).exp();
            for k in 0..ring {
                let a = k as f32 / sides as f32 * tau;
                let n = frame_n * a.cos() + frame_b * a.sin();
                let wave = 0.5 + 0.5 * (lobes * a + lobe_phase + 0.7 * (2.0 * a).sin()).cos();
                pos.push(p + n * limb.radii[i] * (1.0 + wave * wave * buttress));
                radial.push(n);
                // Darker in the hollows between the buttresses.
                shade.push(ao * foot * (1.0 - 0.3 * buttress * (1.0 - wave)));
            }
        }
        let at = |i: usize, k: usize| pos[i * ring + k];
        for i in 0..rings {
            for k in 0..ring {
                // Around (growing angle: `dir × n`) × along (`dir`) points outwards.
                let around = at(i, (k + 1) % n_sides) - at(i, (k + n_sides - 1) % n_sides);
                let along = at((i + 1).min(rings - 1), k) - at(i.saturating_sub(1), k);
                let r = radial[i * ring + k];
                let n = around
                    .cross(along)
                    .try_normalize()
                    .filter(|n| n.dot(r) > 0.2)
                    .unwrap_or(r);
                let y = limb.points[i].y;
                m.vertices.push(TreeVertex {
                    position: at(i, k).to_array(),
                    normal: n.to_array(),
                    uv: [k as f32 / sides as f32 * wraps, -v_of[i]],
                    wind: [limb.weights[i], limb.phase, 0.0, (y / h).clamp(0.0, 1.0)],
                    extra: [encode(shade[i * ring + k]), 0, 0, 0],
                });
            }
        }
        // Outward faces: `cross(b - a, c - a)` points out of the tube (the engine's winding,
        // clockwise on screen; the bark pipelines cull back faces). Around the ring is
        // `dir × n`, along it `dir`, and `(dir × n) × dir = n`.
        let ring = ring as u32;
        for i in 0..rings as u32 - 1 {
            for k in 0..sides {
                let a = first + i * ring + k;
                let b = a + ring;
                m.indices.extend([a, a + 1, b, a + 1, b + 1, b]);
            }
        }
    }
    m.bark = 0..m.indices.len() as u32;

    // Leaves: each card folded along its middle (a shallow V away from the viewer outside),
    // shaded with normals leaning to the crown's dome.
    let stride = detail.card_stride.max(1);
    let grow = (stride as f32).sqrt() * 0.95;
    for (ci, c) in sk.cards.iter().enumerate() {
        if ci % stride != 0 {
            continue;
        }
        let size = c.size * grow;
        let side = c.up.cross(c.facing).normalize_or(Vec3::X);
        let base = c.anchor - c.up * size * 0.06;
        let fold = -c.facing * size * 0.14;
        let (cx, cy) = ((c.cell % 2) as f32 * 0.5, (c.cell / 2) as f32 * 0.5);
        let first = m.vertices.len() as u32;
        // Columns: left edge, folded middle, right edge; rows: bottom, top.
        for row in 0..2 {
            for col in 0..3 {
                let x = col as f32 * 0.5;
                let y = row as f32;
                let mut p = base + side * (x - 0.5) * size + c.up * y * size;
                if col == 1 {
                    p += fold;
                }
                let card_n = (c.facing + side * (0.5 - x) * 0.6).normalize();
                let dome = sk.crown.normal(p);
                let n = (card_n * 0.3 + dome * 0.7).normalize();
                let ao = sk.crown.occlusion(p);
                // Atlas: the twig enters at the bottom centre of the cell.
                let uv = [cx + x * 0.5, cy + 0.5 - y * 0.5];
                let flutter = (y * 0.85 + (x - 0.5).abs() * 0.3).min(1.0);
                m.vertices.push(TreeVertex {
                    position: p.to_array(),
                    normal: n.to_array(),
                    uv,
                    wind: [c.weight, c.phase, flutter, (p.y / h).clamp(0.0, 1.0)],
                    extra: [encode(ao), encode(c.variation), 255, 0],
                });
            }
        }
        // Front faces towards `facing` (outside the crown), so `front_facing` in the leaf shader
        // knows which side of the card it sees.
        let i = |r: u32, col: u32| first + r * 3 + col;
        for col in 0..2 {
            let (a, b, c2, d) = (i(0, col), i(0, col + 1), i(1, col), i(1, col + 1));
            m.indices.extend([a, b, c2, b, d, c2]);
        }
    }
    m.leaves = m.bark.end..m.indices.len() as u32;
    m
}

/// Build both mesh LODs and the impostor. `leaf_alpha` is the leaf atlas's alpha (for the
/// impostor's outline; None = treat cards as solid), `colors` the average linear bark and
/// leaf colours of the textures (the impostor's albedo).
pub fn build(t: &TreeAsset, textures: &super::impostor::BakeTextures) -> TreeModel {
    let sk = skeleton(t);
    let lod0 = mesh(t, &sk, &LOD0);
    let lod1 = mesh(t, &sk, &LOD1);
    let (mut min, mut max) = (Vec3::splat(f32::MAX), Vec3::splat(f32::MIN));
    for v in &lod0.vertices {
        let p = Vec3::from(v.position);
        min = min.min(p);
        max = max.max(p);
    }
    let impostor = super::impostor::bake(t, &lod0, min, max, textures);
    TreeModel {
        lods: [lod0, lod1],
        impostor,
        min,
        max,
        crown_center: sk.crown.center(),
        crown_radii: Vec2::new(sk.crown.radius, sk.crown.half_height()),
        height: t.height,
    }
}
