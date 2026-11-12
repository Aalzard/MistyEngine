//! CPU checks of the glow instances: spot facing, fog response, flare elements and fades,
//! and gathering sources from a scene.

use glam::{Quat, Vec2, Vec3};

use super::GlowSource;
use super::instances::{self, edge_fade, facing};
use crate::camera::{Camera, Projection};
use crate::render::{Environment, FogSettings, Frame, PostSettings};
use crate::scene::{FogOverride, LensFlare, Light, LightHalo, LightType, Scene};

fn camera() -> Camera {
    Camera {
        position: Vec3::new(0.0, 2.0, -10.0),
        rotation: Quat::IDENTITY,
        projection: Projection::Perspective { fov_y: 60.0 },
        near: 0.1,
        far: 200.0,
    }
}

fn halo() -> LightHalo {
    LightHalo {
        enabled: true,
        ..Default::default()
    }
}

fn source(kind: LightType, position: Vec3, direction: Vec3) -> GlowSource {
    GlowSource {
        kind,
        position,
        direction,
        spot_angle: 40.0,
        color: [1.0, 0.8, 0.6],
        intensity: 3.0,
        glow: Default::default(),
        halo: Some(halo()),
        flare: None,
    }
}

fn frame<'a>(cam: &'a Camera, env: &'a Environment, post: PostSettings) -> Frame<'a> {
    Frame {
        camera: cam,
        grid: None,
        skybox: false,
        background: [0.0; 3],
        environment: env,
        post,
        lines: &[],
        overlay: &[],
        meshes: &[],
        particles: &[],
        terrains: &[],
        shaded: true,
        lights: &[],
        water: &[],
    }
}

fn foggy(attenuation: f32) -> PostSettings {
    PostSettings {
        fog: FogSettings::from_override(&FogOverride {
            active: true,
            attenuation_distance: attenuation,
            base_height: 20.0,
            ..Default::default()
        }),
        ..Default::default()
    }
}

#[test]
fn spot_halos_show_only_when_looking_into_the_cone() {
    let eye = Vec3::new(0.0, 0.0, -10.0);
    let toward = source(LightType::Spot, Vec3::ZERO, Vec3::NEG_Z);
    let away = source(LightType::Spot, Vec3::ZERO, Vec3::Z);
    let side = source(LightType::Spot, Vec3::ZERO, Vec3::X);
    assert_eq!(facing(&toward, eye), 1.0);
    assert_eq!(facing(&away, eye), 0.0);
    assert_eq!(facing(&side, eye), 0.0);
    // Just outside the cone edge it is fading, not cut.
    let edge = Quat::from_rotation_y(25f32.to_radians()) * Vec3::NEG_Z;
    let f = facing(&source(LightType::Spot, Vec3::ZERO, edge), eye);
    assert!(f > 0.0 && f < 1.0, "{f}");
    assert_eq!(
        facing(&source(LightType::Point, Vec3::ZERO, Vec3::Z), eye),
        1.0
    );
}

#[test]
fn fog_widens_and_brightens_halos() {
    let cam = camera();
    let mut env = Environment {
        glows: vec![source(LightType::Point, Vec3::new(0.0, 2.0, 5.0), Vec3::Z)],
        ..Default::default()
    };
    let clear = instances::build(&frame(&cam, &env, PostSettings::default()), 1.6, true, 3.6);
    let thick = instances::build(&frame(&cam, &env, foggy(8.0)), 1.6, true, 3.6);
    let (a, b) = (clear.halos[0], thick.halos[0]);
    assert!(b.center[3] > a.center[3] * 1.4, "radius {a:?} → {b:?}");
    assert!(b.color[0] > a.color[0] * 3.0, "brightness {a:?} → {b:?}");
    assert!(b.shape[0] > a.shape[0], "softer in fog");
    // Fog Response 0 ignores the fog.
    env.glows[0].halo.as_mut().unwrap().fog_response = 0.0;
    let ignored = instances::build(&frame(&cam, &env, foggy(8.0)), 1.6, true, 3.6);
    assert_eq!(ignored.halos[0], a);
}

#[test]
fn halos_behind_the_camera_and_directional_lights_are_skipped() {
    let cam = camera();
    let env = Environment {
        glows: vec![
            source(LightType::Point, Vec3::new(0.0, 2.0, -30.0), Vec3::Z),
            source(LightType::Directional, Vec3::ZERO, Vec3::NEG_Y),
        ],
        ..Default::default()
    };
    let list = instances::build(&frame(&cam, &env, PostSettings::default()), 1.6, true, 3.6);
    assert!(list.halos.is_empty());
}

#[test]
fn flares_follow_quality_and_fade_at_screen_edges() {
    let cam = camera();
    let mut env = Environment::default();
    let mut s = source(LightType::Point, Vec3::new(1.0, 3.0, 10.0), Vec3::Z);
    s.halo = None;
    s.flare = Some(LensFlare::default());
    env.glows = vec![s];
    let f = frame(&cam, &env, PostSettings::default());
    let on = instances::build(&f, 1.6, true, 3.6);
    // Glow + streak + ring + 4 ghosts.
    assert_eq!(on.flares.len(), 7);
    assert!(
        instances::build(&f, 1.6, false, 3.6).is_empty(),
        "Low: no flares"
    );
    assert_eq!(edge_fade(Vec2::ZERO), 1.0);
    assert!(edge_fade(Vec2::new(0.95, 0.0)) < 1.0);
    assert_eq!(edge_fade(Vec2::new(0.0, 1.2)), 0.0);
    // Off screen to the side: nothing.
    env.glows[0].position = Vec3::new(40.0, 2.0, 0.0);
    let f = frame(&cam, &env, PostSettings::default());
    assert!(instances::build(&f, 1.6, true, 3.6).flares.is_empty());
}

#[test]
fn sun_flares_sit_opposite_the_light_direction() {
    let cam = camera();
    let mut env = Environment::default();
    let mut sun = source(LightType::Directional, Vec3::ZERO, Vec3::NEG_Z);
    sun.halo = None;
    sun.flare = Some(LensFlare {
        ghosts: 0,
        ring: false,
        streak: false,
        ..Default::default()
    });
    env.glows = vec![sun];
    let list = instances::build(&frame(&cam, &env, PostSettings::default()), 1.6, true, 3.6);
    // Light travelling -Z comes from +Z: in front of the camera (which looks along +Z).
    assert_eq!(list.flares.len(), 1);
    assert!(list.flares[0].center[2] > 50.0);
    assert_eq!(
        list.flares[0].color[3], 0.0,
        "no occlusion bias at infinity"
    );
}

#[test]
fn scenes_gather_halos_and_flares() {
    let mut s = Scene::new("t");
    let plain = s.spawn("Plain", None);
    s.add(plain, Light::default());
    let glowing = s.spawn("Glowing", None);
    s.add(
        glowing,
        Light {
            halo: halo(),
            ..Default::default()
        },
    );
    let flare_only = s.spawn("Flare", None);
    s.add(flare_only, LensFlare::default());
    let off = s.spawn("Off", None);
    s.add(
        off,
        Light {
            enabled: false,
            halo: halo(),
            ..Default::default()
        },
    );
    s.add(off, LensFlare::default());
    let list = crate::render_scene::glow_sources(&s);
    assert_eq!(list.len(), 2);
    assert!(list[0].halo.is_some() && list[0].flare.is_none());
    assert!(list[1].halo.is_none() && list[1].flare.is_some());
    assert_eq!(list[1].color, [1.0; 3], "a flare without a light is white");
}
