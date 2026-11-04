//! CPU checks of the cloud shaders and uniforms.

use glam::{Vec2, Vec3};
use wgpu::naga;

use super::settings::CloudQuality;
use super::uniforms::{self, ApplyUniform, MarchUniform};
use crate::quality::Tier;
use crate::scene::{CloudPreset, CloudsOverride};

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
fn compute_shaders_validate() {
    for (name, src) in super::passes::sources() {
        validate(name, &src);
    }
    validate("cloud noise", include_str!("shaders/cloud_noise.wgsl"));
}

#[test]
fn uniform_layouts_match_wgsl() {
    // 2 matrices + 13 vec4s in `Clouds`; 3 vec4s in `CloudApply`.
    assert_eq!(std::mem::size_of::<MarchUniform>(), 2 * 64 + 13 * 16);
    assert_eq!(std::mem::size_of::<ApplyUniform>(), 3 * 16);
}

#[test]
fn tiers_trade_resolution_for_speed() {
    assert_eq!(CloudQuality::of(Tier::Off), None);
    let low = CloudQuality::of(Tier::Low).unwrap();
    let medium = CloudQuality::of(Tier::Medium).unwrap();
    let high = CloudQuality::of(Tier::High).unwrap();
    assert!(low.steps < medium.steps && medium.steps < high.steps);
    assert!(high.divisor <= medium.divisor && medium.divisor <= low.divisor);
    assert_eq!(medium.trace_size((1920, 1080)), (480, 270));
    assert_eq!(high.trace_size((1, 1)), (1, 1));
}

#[test]
fn wind_and_layer() {
    let north = uniforms::wind_direction(0.0);
    assert!((north - Vec2::new(0.0, 1.0)).length() < 1e-6);
    let east = uniforms::wind_direction(90.0);
    assert!((east - Vec2::new(1.0, 0.0)).length() < 1e-6);
    let mut o = CloudsOverride {
        altitude: -5.0,
        thickness: 1.0,
        ..Default::default()
    };
    assert_eq!(uniforms::layer(&o), (0.0, 50.0), "clamped to sane values");
    CloudPreset::Stormy.apply(&mut o);
    let (base, top) = uniforms::layer(&o);
    assert!(top - base > 3000.0, "storm clouds tower");
}

#[test]
fn shadow_map_follows_the_camera_in_whole_texels() {
    let q = CloudQuality::of(Tier::Medium).unwrap();
    let texel = q.shadow_extent / q.shadow_size as f32;
    let sun = Vec3::new(0.3, 0.8, 0.2).normalize();
    let a = uniforms::shadow_origin(Vec3::new(10.0, 2.0, -40.0), sun, 1500.0, &q);
    let b = uniforms::shadow_origin(Vec3::new(10.3, 2.0, -40.0), sun, 1500.0, &q);
    for v in [a, b] {
        let k = v / texel;
        assert!((k - k.round()).abs().max_element() < 1e-3, "{k}");
    }
    // The camera's spot on the cloud base (towards the sun) is near the map centre.
    let eye = Vec3::new(10.0, 2.0, -40.0);
    let spot = Vec2::new(eye.x, eye.z) + Vec2::new(sun.x, sun.z) / sun.y * (1500.0 - eye.y);
    let centre = a + Vec2::splat(q.shadow_extent * 0.5);
    assert!((spot - centre).length() <= texel * 1.5);
}
