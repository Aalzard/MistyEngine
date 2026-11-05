use glam::{Quat, Vec2, Vec3};

use super::*;
use crate::scene::{Scene, SpriteRenderer, Transform};

fn lamp(radius: f32) -> Light2D {
    Light2D {
        outer_radius: radius,
        inner_radius: 0.0,
        falloff_strength: 0.0,
        ..Default::default()
    }
}

#[test]
fn falloff_fades_from_inner_to_outer_radius() {
    assert_eq!(falloff(0.5, 1.0, 4.0, 0.5), 1.0, "inside the inner radius");
    assert_eq!(falloff(4.0, 1.0, 4.0, 0.5), 0.0, "at the outer radius");
    assert_eq!(falloff(9.0, 1.0, 4.0, 0.5), 0.0);
    let linear = falloff(2.5, 1.0, 4.0, 0.0);
    let soft = falloff(2.5, 1.0, 4.0, 1.0);
    assert!(
        linear > soft,
        "stronger falloff fades sooner: {linear} vs {soft}"
    );
    assert!(
        (falloff(2.5, 1.0, 4.0, 0.2) - 0.5).abs() < 1e-5,
        "exponent 1 = linear"
    );
    // Spot cones: full inside the inner angle, none outside the outer.
    assert_eq!(cone(1.0, 30.0, 60.0), 1.0);
    assert_eq!(cone((40f32).to_radians().cos(), 30.0, 60.0), 0.0);
    assert_eq!(cone(-1.0, 360.0, 360.0), 1.0, "a full circle");
}

#[test]
fn casters_shadow_what_lies_behind_them() {
    // A box between x = 2 and 3, the light at the origin.
    let caster = Caster {
        points: vec![
            Vec2::new(2.0, -0.5),
            Vec2::new(3.0, -0.5),
            Vec2::new(3.0, 0.5),
            Vec2::new(2.0, 0.5),
        ],
        self_shadows: false,
        group: 0,
    };
    let row = shadow_row(Vec2::ZERO, 10.0, std::slice::from_ref(&caster));
    let right = row[bin_of(Vec2::X)];
    assert!(
        (right - 3.0).abs() < 0.05,
        "shadow starts at the far side: {right}"
    );
    assert_eq!(row[bin_of(-Vec2::X)], NO_SHADOW, "nothing to the left");
    assert_eq!(row[bin_of(Vec2::Y)], NO_SHADOW);
    assert_eq!(
        shadow_lit(&row, Vec2::new(2.5, 0.0)),
        1.0,
        "the caster itself is lit"
    );
    assert_eq!(
        shadow_lit(&row, Vec2::new(5.0, 0.0)),
        0.0,
        "behind it is dark"
    );
    let selfish = Caster {
        self_shadows: true,
        ..caster.clone()
    };
    let row = shadow_row(Vec2::ZERO, 10.0, &[selfish]);
    assert!(
        (row[bin_of(Vec2::X)] - 2.0).abs() < 0.05,
        "self shadows start at the near side"
    );
    // Out of reach: no shadow.
    let far = shadow_row(Vec2::new(-20.0, 0.0), 5.0, std::slice::from_ref(&caster));
    assert!(far.iter().all(|d| *d == NO_SHADOW));
    // A second box touching the first, of the same object: the shadow starts past both.
    let next = Caster {
        points: caster.points.iter().map(|p| *p + Vec2::X).collect(),
        ..caster.clone()
    };
    let row = shadow_row(Vec2::ZERO, 10.0, &[caster.clone(), next.clone()]);
    assert!((row[bin_of(Vec2::X)] - 4.0).abs() < 0.05, "merged solid");
    // Another object's box shadows from the first one's far side.
    let other = Caster { group: 1, ..next };
    let row = shadow_row(Vec2::ZERO, 10.0, &[caster, other]);
    assert!((row[bin_of(Vec2::X)] - 3.0).abs() < 0.05);
}

/// Sorting Layers are process-wide (other tests may install theirs), so these tests only rely
/// on "Default" (value 0, bit 16) being present.
fn layers_scene() -> Scene {
    Scene::new("Lights 2D")
}

#[test]
fn collect_sorts_globals_first_and_masks_layers() {
    let mut s = layers_scene();
    assert!(collect(&s).is_empty(), "no 2D lights: sprites stay unlit");
    let g = s.spawn("Global Light 2D", None);
    s.add(g, Light2D::global([1.0, 1.0, 1.0], 0.25));
    let p = s.spawn("Torch", None);
    s.set_transform(p, Transform::from_position(Vec3::new(4.0, 1.0, 0.0)));
    s.add(
        p,
        Light2D {
            color: [1.0, 0.5, 0.0],
            target_sorting_layers: Some(vec!["Default".into()]),
            ..lamp(3.0)
        },
    );
    let off = s.spawn("Off", None);
    s.add(
        off,
        Light2D {
            enabled: false,
            ..lamp(3.0)
        },
    );
    let l = collect(&s);
    assert_eq!(l.items.len(), 2, "disabled lights are skipped");
    assert_eq!(l.items[0].kind, Light2DType::Global);
    assert_eq!(l.items[1].position, Vec2::new(4.0, 1.0));
    // Default is value 0 → bit 16; the global light covers all three layers.
    assert_eq!(l.items[1].layer_mask, 1 << LAYER_OFFSET);
    assert!(l.items[0].layer_mask & (1 << LAYER_OFFSET) != 0);
    // Sampling: ambient everywhere, the torch only on Default and inside its radius.
    let ambient = l.sample(Vec2::new(-10.0, 0.0), 0);
    assert!((ambient.x - 0.25f32.powf(1.0)).abs() < 1e-5);
    let lit = l.sample(Vec2::new(4.0, 1.0), 0);
    assert!(lit.x > 1.0 && lit.z < 0.3, "{lit}");
    let other_layer = l.sample(Vec2::new(4.0, 1.0), 1);
    assert!(
        other_layer.x <= ambient.x + 1e-5,
        "the torch skips other layers"
    );
}

#[test]
fn spot_lights_follow_the_object_up_axis() {
    let mut s = layers_scene();
    let e = s.spawn("Spot", None);
    // Turned −90° about Z: up now points along +X.
    let t = Transform {
        rotation: Quat::from_rotation_z(-std::f32::consts::FRAC_PI_2),
        ..Default::default()
    };
    s.set_transform(e, t);
    s.add(
        e,
        Light2D {
            kind: Light2DType::Spot,
            inner_angle: 30.0,
            outer_angle: 60.0,
            ..lamp(6.0)
        },
    );
    let l = collect(&s);
    assert!((l.items[0].direction - Vec2::X).length() < 1e-4);
    assert!(l.sample(Vec2::new(2.0, 0.0), 0).x > 0.3);
    assert_eq!(
        l.sample(Vec2::new(0.0, 2.0), 0),
        Vec3::ZERO,
        "outside the cone"
    );
}

#[test]
fn shadow_casters_from_sprites_block_shadowed_lights() {
    let mut s = layers_scene();
    let light = s.spawn("Torch", None);
    s.add(
        light,
        Light2D {
            shadows_enabled: true,
            shadow_strength: 1.0,
            ..lamp(8.0)
        },
    );
    let wall = s.spawn("Wall", None);
    let mut t = Transform::from_position(Vec3::new(2.5, 0.0, 0.0));
    t.scale = Vec3::new(1.0, 3.0, 1.0);
    s.set_transform(wall, t);
    s.add(
        wall,
        SpriteRenderer {
            sprite: Some(crate::sprites::builtin::path("Square")),
            ..Default::default()
        },
    );
    s.add(wall, ShadowCaster2D::default());
    let l = collect(&s);
    assert_eq!(l.items[0].shadow_row, Some(0));
    assert_eq!(l.shadows.len(), SHADOW_BINS);
    let behind = l.sample(Vec2::new(5.0, 0.0), 0);
    let beside = l.sample(Vec2::new(0.0, 5.0), 0);
    assert_eq!(behind, Vec3::ZERO, "in the wall's shadow");
    assert!(beside.x > 0.0);
    // Shadow strength 0.5 lets half through.
    s.world.get::<&mut Light2D>(light).unwrap().shadow_strength = 0.5;
    let half = collect(&s).sample(Vec2::new(5.0, 0.0), 0);
    let open = l.sample(Vec2::new(-5.0, 0.0), 0);
    assert!((half.x - open.x * 0.5).abs() < 1e-4, "{half} vs {open}");
    // Without shadows enabled no map is built.
    s.world.get::<&mut Light2D>(light).unwrap().shadows_enabled = false;
    assert!(collect(&s).shadows.is_empty());
}
