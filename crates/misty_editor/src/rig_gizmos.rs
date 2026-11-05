//! Scene-view gizmos of Animation Rigging: bone chains, targets and hints of the constraints
//! related to the selection (the constraint itself, its Rig or Rig Builder, or one of its
//! bones, targets or hints). Effectors are ordinary objects moved with the Move tool.

use glam::{Mat4, Vec3};
use misty_engine::hecs::Entity;
use misty_engine::rigging::{self, ChainIk, DampedTransform, Kind, MultiAim, TwoBoneIk};
use misty_engine::scene::Scene;

use super::Lines;
use crate::selection::Selection;

const BONE: [f32; 4] = [0.35, 0.8, 1.0, 0.95];
const TARGET: [f32; 4] = [1.0, 0.45, 0.3, 0.95];
const HINT: [f32; 4] = [1.0, 0.85, 0.3, 0.95];
const SOURCE: [f32; 4] = [0.6, 1.0, 0.5, 0.8];

/// A constraint's bones and effectors in world space.
struct Parts {
    bones: Vec<Entity>,
    targets: Vec<Entity>,
    hints: Vec<Entity>,
}

fn parts(scene: &Scene, e: Entity, kind: Kind) -> Parts {
    let w = &scene.world;
    let alive = |o: Option<Entity>| o.filter(|o| scene.contains(*o));
    let mut p = Parts {
        bones: rigging::driven(scene, e, kind),
        targets: Vec::new(),
        hints: Vec::new(),
    };
    match kind {
        Kind::TwoBoneIk => {
            if let Ok(c) = w.get::<&TwoBoneIk>(e) {
                p.targets.extend(alive(c.target));
                p.hints.extend(alive(c.hint));
            }
        }
        Kind::ChainIk => {
            if let Ok(c) = w.get::<&ChainIk>(e) {
                p.targets.extend(alive(c.target));
            }
        }
        Kind::MultiAim => {
            if let Ok(c) = w.get::<&MultiAim>(e) {
                p.targets
                    .extend(c.sources.iter().filter_map(|s| alive(s.object)));
            }
        }
        Kind::Damped => {
            if let Ok(c) = w.get::<&DampedTransform>(e) {
                p.targets.extend(alive(c.source));
            }
        }
        Kind::Override => {}
    }
    p
}

/// Rig and Rig Builder objects above `e` (the ones whose selection shows `e`'s gizmos).
fn owners(scene: &Scene, e: Entity) -> Vec<Entity> {
    let mut out = vec![e];
    let mut cur = scene.parent(e);
    while let Some(p) = cur {
        out.push(p);
        cur = scene.parent(p);
    }
    // Rigs listed in a Rig Builder's layers also count, wherever they are.
    let rigs = out.clone();
    for (b, _) in scene.walk() {
        if let Ok(builder) = scene.world.get::<&rigging::RigBuilder>(b)
            && builder
                .layers
                .iter()
                .any(|l| l.rig.is_some_and(|r| rigs.contains(&r)))
        {
            out.push(b);
        }
    }
    out
}

impl Lines {
    /// Gizmos of every constraint related to the selection.
    pub fn rigging(&mut self, scene: &Scene, selection: &Selection) {
        if selection.is_empty() {
            return;
        }
        for (e, _) in scene.walk() {
            let kinds = Kind::of(scene, e);
            if kinds.is_empty() || !scene.active_in_hierarchy(e) {
                continue;
            }
            let own = owners(scene, e);
            for kind in kinds {
                let p = parts(scene, e, kind);
                let related = own.iter().any(|o| selection.contains(*o))
                    || p.bones
                        .iter()
                        .chain(&p.targets)
                        .chain(&p.hints)
                        .any(|o| selection.contains(*o));
                if related {
                    self.rig_constraint(scene, kind, &p);
                }
            }
        }
    }

    fn rig_constraint(&mut self, scene: &Scene, kind: Kind, p: &Parts) {
        let pts: Vec<Vec3> = p.bones.iter().map(|b| scene.world_position(*b)).collect();
        let span = pts
            .windows(2)
            .map(|w| (w[1] - w[0]).length())
            .fold(0.0f32, f32::max)
            .max(0.3);
        for w in pts.windows(2) {
            self.rig_bone(w[0], w[1]);
        }
        if let [only] = pts[..] {
            // Single constrained object: a small cross at its pivot and its aim line.
            self.rig_cross(only, span * 0.15, BONE);
        }
        let size = span * 0.12;
        let end = pts.last().copied();
        for t in &p.targets {
            let at = scene.world_position(*t);
            let color = if kind == Kind::MultiAim || kind == Kind::Damped {
                SOURCE
            } else {
                TARGET
            };
            let m = Mat4::from_translation(at);
            self.wire_box(&m, Vec3::ZERO, Vec3::splat(size), color);
            if let Some(end) = end {
                self.dashed(end, at, color);
            }
        }
        for h in &p.hints {
            let at = scene.world_position(*h);
            self.diamond(at, size, HINT);
            if let Some(mid) = pts.get(1) {
                self.dashed(*mid, at, HINT);
            }
        }
    }

    /// Octahedral bone from `a` to `b`.
    fn rig_bone(&mut self, a: Vec3, b: Vec3) {
        let d = b - a;
        let len = d.length();
        if len < 1e-5 {
            return;
        }
        let dir = d / len;
        let side = dir.any_orthonormal_vector() * len * 0.1;
        let up = dir.cross(side);
        let neck = a + d * 0.2;
        let ring = [neck + side, neck + up, neck - side, neck - up];
        for i in 0..4 {
            self.line(a, ring[i], BONE);
            self.line(ring[i], b, BONE);
            self.line(ring[i], ring[(i + 1) % 4], BONE);
        }
    }

    fn rig_cross(&mut self, at: Vec3, r: f32, color: [f32; 4]) {
        for axis in [Vec3::X, Vec3::Y, Vec3::Z] {
            self.line(at - axis * r, at + axis * r, color);
        }
    }

    fn diamond(&mut self, at: Vec3, r: f32, color: [f32; 4]) {
        let p = [
            Vec3::X,
            Vec3::Y,
            Vec3::Z,
            Vec3::NEG_X,
            Vec3::NEG_Y,
            Vec3::NEG_Z,
        ]
        .map(|v| at + v * r);
        for (a, b) in [
            (0, 1),
            (1, 3),
            (3, 4),
            (4, 0),
            (0, 2),
            (2, 3),
            (3, 5),
            (5, 0),
            (1, 2),
            (2, 4),
            (4, 5),
            (5, 1),
        ] {
            self.line(p[a], p[b], color);
        }
    }

    fn dashed(&mut self, a: Vec3, b: Vec3, color: [f32; 4]) {
        let n = 12;
        let faded = [color[0], color[1], color[2], color[3] * 0.6];
        for i in (0..n).step_by(2) {
            let t0 = i as f32 / n as f32;
            let t1 = (i + 1) as f32 / n as f32;
            self.line(a.lerp(b, t0), a.lerp(b, t1), faded);
        }
    }
}
