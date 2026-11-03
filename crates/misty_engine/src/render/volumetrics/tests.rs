//! CPU checks of the fog: shaders validate, slice mapping, phase and height-fog math (CPU
//! copies of the WGSL), settings defaults and old-file compatibility, uniform packing.

use glam::{Mat4, Quat, Vec3};
use wgpu::naga;

use super::settings::{FogMode, FogSettings, LocalFog};
use super::uniforms::{self, ApplyUniform, FroxelUniform, Slices};
use crate::camera::{Camera, Projection};
use crate::render::shadows::ShadowPlan;
use crate::render::{Environment, Frame, LightData, PostSettings, VolumetricLight};
use crate::scene::{FogOverride, FogVolume, Light, LightType, PostProcessVolume, Shadows};

fn validate(name: &str, src: &str) {
    let module = naga::front::wgsl::parse_str(src)
        .unwrap_or_else(|e| panic!("{name}: {}", e.emit_to_string(src)));
    naga::valid::Validator::new(
        naga::valid::ValidationFlags::all(),
        naga::valid::Capabilities::all(),
    )
    .validate(&module)
    .unwrap_or_else(|e| panic!("{name}: {e:?}"));
}

#[test]
fn fog_shaders_validate() {
    validate(
        "froxel light",
        &super::froxels::source(super::froxels::LIGHT_SRC),
    );
    validate(
        "froxel integrate",
        &super::froxels::source(super::froxels::INTEGRATE_SRC),
    );
    let common = concat!(
        include_str!("../shaders/common.wgsl"),
        "\n",
        include_str!("shaders/fog_apply.wgsl")
    );
    validate(
        "sun shafts",
        &format!("{common}\n{}", super::sun_shafts::SRC),
    );
}

#[test]
fn uniform_sizes_match_wgsl() {
    assert_eq!(std::mem::size_of::<ApplyUniform>(), 8 * 16);
    // 3 matrices, 13 vec4s, 8 lights, 16 shadow matrices, 16 boxes.
    assert_eq!(
        std::mem::size_of::<FroxelUniform>(),
        3 * 64 + 13 * 16 + 8 * 80 + 16 * 64 + 16 * 96
    );
}

#[test]
fn slices_are_exponential_and_invert() {
    let s = Slices::new(64.0, 64);
    assert_eq!(s.depth(0.0), 0.0);
    assert!((s.depth(1.0) - 64.0).abs() < 1e-3);
    for d in [0.6, 2.0, 10.0, 40.0] {
        assert!((s.depth(s.coord(d)) - d).abs() < 1e-3 * d, "{d}");
    }
    // Thin slices near the eye, thick ones far away.
    let near = s.depth(2.0 / 64.0) - s.depth(1.0 / 64.0);
    let far = s.depth(1.0) - s.depth(63.0 / 64.0);
    assert!(near < 0.1 && far > 3.0, "{near} {far}");
}

/// CPU copy of `phase` / `vfog_phase`.
fn phase(g: f32, c: f32) -> f32 {
    let g2 = g * g;
    (1.0 - g2) / (4.0 * std::f32::consts::PI * (1.0 + g2 - 2.0 * g * c).max(1e-4).powf(1.5))
}

#[test]
fn phase_is_normalised_and_forward_peaked() {
    for g in [0.0, 0.4, 0.8, -0.5] {
        // ∫ p dω = 2π ∫ p(cos) dcos.
        let n = 20_000;
        let sum: f32 = (0..n)
            .map(|i| {
                let c = -1.0 + (i as f32 + 0.5) * 2.0 / n as f32;
                phase(g, c) * 2.0 / n as f32
            })
            .sum::<f32>()
            * 2.0
            * std::f32::consts::PI;
        assert!((sum - 1.0).abs() < 0.01, "g {g}: {sum}");
    }
    assert!(phase(0.6, 1.0) > 10.0 * phase(0.6, -1.0));
}

/// CPU copy of `vfog_optical_depth` (extinction 1 at and below height 0, falloff `k`).
fn optical_depth(a: Vec3, b: Vec3, k: f32) -> f32 {
    let avg = |h0: f32, h1: f32| {
        let span = (h1 - h0) * k;
        if span.abs() < 1e-3 {
            (-k * (h0 + h1) * 0.5).exp()
        } else {
            ((-k * h0).exp() - (-k * h1).exp()) / span
        }
    };
    let (lo, hi) = (a.y.min(b.y), a.y.max(b.y));
    let mean = if lo >= 0.0 {
        avg(lo, hi)
    } else if hi > 0.0 {
        let below = -lo / (hi - lo);
        below + (1.0 - below) * avg(0.0, hi)
    } else {
        1.0
    };
    a.distance(b) * mean
}

#[test]
fn analytic_height_fog_matches_numeric_integration() {
    let k = 0.2;
    let density = |p: Vec3| if p.y <= 0.0 { 1.0 } else { (-k * p.y).exp() };
    for (a, b) in [
        (Vec3::new(0.0, 1.0, 0.0), Vec3::new(30.0, 1.0, 5.0)),
        (Vec3::new(0.0, -3.0, 0.0), Vec3::new(10.0, 12.0, 0.0)),
        (Vec3::new(0.0, 20.0, 0.0), Vec3::new(5.0, 2.0, 40.0)),
        (Vec3::new(0.0, -2.0, 0.0), Vec3::new(9.0, -1.0, 0.0)),
    ] {
        let n = 10_000;
        let numeric: f32 = (0..n)
            .map(|i| density(a.lerp(b, (i as f32 + 0.5) / n as f32)))
            .sum::<f32>()
            * a.distance(b)
            / n as f32;
        let analytic = optical_depth(a, b, k);
        assert!(
            (numeric - analytic).abs() < 1e-3 * numeric.max(1.0),
            "{a} {b}: {numeric} vs {analytic}"
        );
    }
}

#[test]
fn fog_is_off_by_default_and_old_files_load() {
    // A volume without the new sections (every existing scene) has no fog or shafts.
    let old: PostProcessVolume =
        serde_json::from_str(r#"{"enabled": true, "priority": 1}"#).unwrap();
    let s = PostSettings::from_volume(&old);
    assert_eq!(s.fog.mode(), FogMode::Off);
    assert!(!s.sun_shafts.active);
    assert_eq!(s, PostSettings::default());
    // Untouched sections stay out of saved files.
    let text = serde_json::to_string(&PostProcessVolume::default()).unwrap();
    assert!(
        !text.contains("fog") && !text.contains("sun_shafts"),
        "{text}"
    );
    // Lights: new fields default to 1 and are left out while at 1.
    let l: Light = serde_json::from_str(r#"{"kind": "Spot"}"#).unwrap();
    assert_eq!(
        (l.volumetric_multiplier, l.volumetric_shadow_dimmer),
        (1.0, 1.0)
    );
    let text = serde_json::to_string(&Light::default()).unwrap();
    assert!(!text.contains("volumetric"), "{text}");
    // Ticking the override turns fog on (volumetric by default).
    let mut v = PostProcessVolume::default();
    v.fog.active = true;
    let s = PostSettings::from_volume(&v);
    assert_eq!(s.fog.mode(), FogMode::Volumetric);
    v.fog.volumetric = false;
    assert_eq!(PostSettings::from_volume(&v).fog.mode(), FogMode::Analytic);
    v.fog.enabled = false;
    assert_eq!(PostSettings::from_volume(&v).fog.mode(), FogMode::Off);
    let back: PostProcessVolume =
        serde_json::from_str(&serde_json::to_string(&v).unwrap()).unwrap();
    assert_eq!(back, v);
}

#[test]
fn fog_volume_box_weights() {
    let v = FogVolume {
        size: [4.0, 2.0, 4.0],
        blend_distance: 1.0,
        ..Default::default()
    };
    let world = Mat4::from_scale_rotation_translation(
        Vec3::new(2.0, 1.0, 1.0),
        Quat::from_rotation_y(0.5),
        Vec3::new(10.0, 0.0, 0.0),
    );
    let f = LocalFog::new(&v, world);
    assert!((f.half_extents - Vec3::new(4.0, 1.0, 2.0)).length() < 1e-4);
    assert!((f.extinction - 1.0 / v.fog_distance).abs() < 1e-6);
    assert_eq!(f.weight(Vec3::new(10.0, 0.0, 0.0)), 1.0);
    assert_eq!(f.weight(Vec3::new(30.0, 0.0, 0.0)), 0.0);
    // Half way into the 1 m soft edge at the top face.
    let w = f.weight(Vec3::new(10.0, 0.5, 0.0));
    assert!((w - 0.5).abs() < 1e-3, "{w}");
}

fn spot(multiplier: f32, dimmer: f32) -> LightData {
    LightData {
        kind: LightType::Spot,
        position: Vec3::new(0.0, 5.0, 0.0),
        direction: Vec3::NEG_Y,
        color: [1.0; 3],
        intensity: 2.0,
        range: 10.0,
        spot_angle: 40.0,
        shadows: Shadows::Hard,
        volumetric: VolumetricLight {
            multiplier,
            shadow_dimmer: dimmer,
        },
    }
}

#[test]
fn froxel_uniform_packs_lights_volumes_and_jitter() {
    let camera = Camera {
        position: Vec3::new(0.0, 2.0, -10.0),
        rotation: Quat::IDENTITY,
        projection: Projection::Perspective { fov_y: 60.0 },
        near: 0.1,
        far: 200.0,
    };
    let lights = [spot(0.5, 0.25), spot(0.0, 1.0), spot(1.0, 1.0)];
    let environment = Environment {
        fog_volumes: vec![LocalFog::new(&FogVolume::default(), Mat4::IDENTITY); 20],
        ..Default::default()
    };
    let post = PostSettings {
        fog: FogSettings::from_override(&FogOverride {
            active: true,
            ..Default::default()
        }),
        ..Default::default()
    };
    let frame = Frame {
        camera: &camera,
        grid: None,
        skybox: true,
        background: [0.0; 3],
        environment: &environment,
        post,
        lines: &[],
        overlay: &[],
        meshes: &[],
        particles: &[],
        terrains: &[],
        shaded: true,
        lights: &lights,
    };
    let plan = ShadowPlan {
        light_layers: vec![Some(0), Some(1), None],
        local: vec![Mat4::IDENTITY; 2],
        ..Default::default()
    };
    let slices = Slices::new(64.0, 64);
    let inputs = |frame_index| uniforms::FroxelInputs {
        frame: &frame,
        fog: &frame.post.fog,
        plan: &plan,
        view_proj: camera.view_projection(16.0 / 9.0),
        slices,
        size: [128, 72, 64],
        frame_index,
        prev: None,
        history: 0.0,
        time: 0.0,
    };
    let u = uniforms::froxel_uniform(&inputs(0));
    // The light with multiplier 0 adds nothing and is skipped; boxes are capped at 16.
    assert_eq!(u.counts[0], 2.0);
    assert_eq!(u.counts[1], 16.0);
    let first = u.lights[0];
    assert_eq!(first.color[0], 1.0, "intensity 2 × multiplier 0.5");
    assert_eq!(first.shadow, [1.0, 0.0, 0.25, 0.0]);
    assert_eq!(u.lights[1].shadow[0], 0.0, "no shadow layer");
    // Jitter changes every frame and stays inside the froxel.
    let other = uniforms::froxel_uniform(&inputs(1));
    assert_ne!(u.jitter, other.jitter);
    assert!(u.jitter[..3].iter().all(|j| (0.0..1.0).contains(j)));
}
