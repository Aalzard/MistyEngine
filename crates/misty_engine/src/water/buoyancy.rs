//! Floater physics: before each fixed step, probes at a box's eight corners push
//! the Rigidbody up by how deep they sit below the waves, and water drag damps its motion
//! (relative to a river's flow).

use glam::{Vec2, Vec3};

use super::{Floater, WaterDraw, WaveState, draws};
use crate::physics::Physics;
use crate::scene::Scene;

/// Probe offsets in units of the box size: its eight corners (works at any orientation).
const PROBES: [Vec3; 8] = [
    Vec3::new(-0.5, -0.5, -0.5),
    Vec3::new(0.5, -0.5, -0.5),
    Vec3::new(-0.5, -0.5, 0.5),
    Vec3::new(0.5, -0.5, 0.5),
    Vec3::new(-0.5, 0.5, -0.5),
    Vec3::new(0.5, 0.5, -0.5),
    Vec3::new(-0.5, 0.5, 0.5),
    Vec3::new(0.5, 0.5, 0.5),
];

/// Push every floating body for one step of `dt` seconds.
pub fn apply(physics: &mut Physics, scene: &Scene, dt: f32) {
    let floaters: Vec<(hecs::Entity, Floater)> = scene
        .world
        .query::<(hecs::Entity, &Floater)>()
        .iter()
        .filter(|(e, f)| f.enabled && physics.has_body(*e) && scene.active_in_hierarchy(*e))
        .map(|(e, f)| (e, f.clone()))
        .collect();
    if floaters.is_empty() {
        return;
    }
    let waters: Vec<(WaterDraw, WaveState)> = draws(scene)
        .into_iter()
        .map(|d| {
            let w = d.waves();
            (d, w)
        })
        .collect();
    if waters.is_empty() {
        return;
    }
    // Water height and flow at a point (highest covering surface).
    let sample = |p: Vec2| -> Option<(f32, Vec2)> {
        waters
            .iter()
            .filter(|(d, _)| d.contains(p))
            .map(|(d, w)| (d.level() + w.height(p, d.flow()), d.flow()))
            .max_by(|a, b| a.0.total_cmp(&b.0))
    };
    let g = physics.gravity().length();
    for (e, f) in floaters {
        let (Some((pos, rot)), Some(mass)) = (physics.body_pose(e), physics.mass(e)) else {
            continue;
        };
        let (scale, _, _) = scene.world_matrix(e).to_scale_rotation_translation();
        let size = f.size * scale.abs();
        let mut wet = 0.0;
        let mut flow = Vec2::ZERO;
        for offset in PROBES {
            let world = pos + rot * (f.center * scale + offset * size);
            let Some((h, v)) = sample(Vec2::new(world.x, world.z)) else {
                continue;
            };
            // Each corner stands for the box slice around it: half under at the surface.
            let depth = ((h - world.y) / size.y.max(0.05) + 0.5).clamp(0.0, 1.0);
            if depth <= 0.0 {
                continue;
            }
            wet += depth / PROBES.len() as f32;
            flow += v * depth;
            let lift = f.buoyancy * mass * g * depth / PROBES.len() as f32;
            physics.apply_impulse_at_point(e, Vec3::Y * lift * dt, world);
        }
        if wet <= 0.0 {
            continue;
        }
        // Drag towards the water's own motion.
        let flow = flow / (wet * PROBES.len() as f32);
        if let Some(v) = physics.velocity(e) {
            let relative = v - Vec3::new(flow.x, 0.0, flow.y);
            let k = (f.water_drag * wet * dt).min(1.0);
            physics.apply_impulse(e, -relative * mass * k);
        }
        if let Some(w) = physics.angular_velocity(e) {
            let k = (f.water_angular_drag * wet * dt).min(1.0);
            physics.set_angular_velocity(e, w * (1.0 - k));
        }
    }
}

#[cfg(test)]
mod tests {
    use glam::Vec3;

    use crate::physics::Physics;
    use crate::scene::{BoxCollider, Rigidbody, Scene, Transform};
    use crate::water::{Floater, WaterKind, WaterPreset, WaterSurface, spawn_water};

    fn crate_scene(settings: crate::water::WaterSettings) -> (Scene, hecs::Entity) {
        let mut s = Scene::new("float");
        let sea = spawn_water(
            &mut s,
            "Sea",
            WaterKind::Ocean,
            Vec3::ZERO,
            Default::default(),
        );
        s.world.get::<&mut WaterSurface>(sea).unwrap().settings = settings;
        let c = s.spawn("Crate", None);
        s.set_transform(c, Transform::from_position(Vec3::new(0.0, 4.0, 0.0)));
        s.add(c, Rigidbody::default());
        s.add(c, BoxCollider::default());
        s.add(c, Floater::default());
        (s, c)
    }

    #[test]
    fn crates_float_and_bob() {
        // Calm water: the crate falls in, then settles about half submerged.
        let (mut s, c) = crate_scene(WaterPreset::Lake.settings());
        let mut physics = Physics::build(&s, Vec3::new(0.0, -9.81, 0.0));
        let mut lowest = f32::MAX;
        for _ in 0..600 {
            physics.update(&mut s, 1.0 / 60.0);
            lowest = lowest.min(s.world_position(c).y);
        }
        let y = s.world_position(c).y;
        assert!(lowest < 0.5, "fell into the water ({lowest})");
        assert!(y > -0.8 && y < 0.8, "floats near the surface ({y})");

        // Without a Floater it sinks.
        let (mut s2, c2) = crate_scene(WaterPreset::Lake.settings());
        s2.world.remove_one::<Floater>(c2).unwrap();
        let mut physics = Physics::build(&s2, Vec3::new(0.0, -9.81, 0.0));
        for _ in 0..180 {
            physics.update(&mut s2, 1.0 / 60.0);
        }
        assert!(s2.world_position(c2).y < -5.0);

        // Rough water keeps it moving up and down.
        let (mut s3, c3) = crate_scene(WaterPreset::Stormy.settings());
        let mut physics = Physics::build(&s3, Vec3::new(0.0, -9.81, 0.0));
        let mut heights = Vec::new();
        for i in 0..900 {
            crate::clock::set_play_time(i as f64 / 60.0);
            physics.update(&mut s3, 1.0 / 60.0);
            if i > 300 {
                heights.push(s3.world_position(c3).y);
            }
        }
        let (lo, hi) = heights
            .iter()
            .fold((f32::MAX, f32::MIN), |(a, b), h| (a.min(*h), b.max(*h)));
        assert!(hi - lo > 0.3, "bobs with the waves ({lo}..{hi})");
        assert!(lo > -6.0, "stays afloat ({lo})");
    }
}
