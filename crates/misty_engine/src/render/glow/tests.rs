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

/// A point light's Glow Spread halo stays about as bright as its sprite when the camera walks
/// up to the light, and fades out with the sprite (it used to put the sprite's whole bloom
/// share, which grows with the sprite's screen area, into Gaussians around one point: about
/// 10^4 times brighter at 0.1 m than at 10 m, a white flash over the screen).
#[test]
fn halo_spread_stays_bounded_as_the_camera_walks_into_the_light() {
    use super::spread::{HaloSprite, halo_spread};
    use crate::scene::LightGlow;
    let cam = camera();
    let env = Environment::default();
    let f = frame(&cam, &env, PostSettings::default());
    let glow = LightGlow {
        spread: 0.5,
        ..Default::default()
    };
    let peak = |d: f32| {
        let sprite = HaloSprite {
            position: cam.position + cam.forward() * d,
            radius: 1.0,
            color: Vec3::ONE,
            falloff: 0.5,
        };
        halo_spread(&f, &sprite, &glow, 16.0 / 9.0, 3.0).map(|s| s.weight[0])
    };
    let far = peak(10.0).expect("a spread halo at 10 m");
    let near = peak(1.5).expect("a spread halo at 1.5 m");
    assert!(far.is_finite() && far > 0.0, "{far}");
    assert!(near < far * 4.0, "near {near} vs far {far}");
    assert_eq!(peak(0.1), None, "faded out with the sprite inside it");
    assert_eq!(peak(0.25), None);
}

/// Every halo that hands a share of its bloom to Glow Spread has a spread halo drawn for it:
/// past the spread cap the halos keep their whole bloom (they used to lose the share).
#[test]
fn halos_past_the_spread_cap_keep_their_bloom() {
    use crate::scene::LightGlow;
    let cam = camera();
    let glows: Vec<GlowSource> = (0..100)
        .map(|i| {
            let mut s = source(
                LightType::Point,
                Vec3::new((i % 10) as f32 - 5.0, (i / 10) as f32 * 0.5, 5.0),
                Vec3::Z,
            );
            s.glow = LightGlow {
                spread: 0.5,
                ..Default::default()
            };
            s
        })
        .collect();
    let env = Environment {
        glows,
        ..Default::default()
    };
    let f = frame(&cam, &env, PostSettings::default());
    let list = instances::build(&f, 16.0 / 9.0, false, 3.0);
    assert!(list.halos.len() > list.spreads.len(), "the cap is reached");
    let sharing = list.halos.iter().filter(|h| h.shape[2] > 0.0).count();
    assert_eq!(sharing, list.spreads.len());
}

/// A two-sided Rectangle light behind an orthographic camera gets no Glow Spread halo (the
/// clip w of an orthographic camera is 1 everywhere, so it used to pass); in front it does.
#[test]
fn rectangle_spread_behind_an_orthographic_camera_is_not_drawn() {
    use super::spread::spread_instance;
    use crate::render::{AreaShape, LightData};
    use crate::scene::LightGlow;
    let mut cam = camera();
    cam.projection = Projection::Orthographic { size: 5.0 };
    let env = Environment::default();
    let f = frame(&cam, &env, PostSettings::default());
    let light = |z: f32| LightData {
        kind: LightType::Rectangle,
        position: cam.position + cam.forward() * z,
        direction: Vec3::NEG_Z,
        color: [1.0; 3],
        intensity: 20.0,
        range: 10.0,
        spot_angle: 45.0,
        shadows: Default::default(),
        shadow: Default::default(),
        volumetric: Default::default(),
        contact_shadows: false,
        area: AreaShape {
            two_sided: true,
            show_emitter: true,
            glow: LightGlow {
                spread: 0.5,
                ..Default::default()
            },
            ..Default::default()
        },
    };
    assert!(spread_instance(&f, &light(8.0), 16.0 / 9.0, 3.0).is_some());
    assert!(spread_instance(&f, &light(-8.0), 16.0 / 9.0, 3.0).is_none());
}

/// A Directional Light's lens flare shines with the light the scene gets: dimmer (and warmer)
/// for a low sun with Intensity from Sun Height, and gone at night once the sun has set (its
/// light is the moon's then). It used the light's raw intensity: a full-strength sun flare at
/// dusk and under the horizon.
#[test]
fn directional_flares_follow_the_sun_height_and_set_at_night() {
    let mut s = Scene::new("t");
    let sun = s.spawn("Sun", None);
    s.add(
        sun,
        Light {
            kind: LightType::Directional,
            intensity: 2.0,
            intensity_from_sun_height: true,
            ..Default::default()
        },
    );
    s.add(sun, LensFlare::default());
    // Shining along +Z and 3° down: the sun 3° above the horizon.
    let low = Quat::from_rotation_x(3f32.to_radians());
    let mut t = s.transform(sun);
    t.set_rotation(low);
    s.set_transform(sun, t);
    let list = crate::render_scene::glow_sources(&s);
    let (_, expected) = crate::scene::sun_height::apply([1.0; 3], 2.0, low * Vec3::Z);
    assert!(expected < 1.0, "{expected}");
    assert!(
        (list[0].intensity - expected).abs() < 1e-5,
        "{}",
        list[0].intensity
    );

    // At night (the sun 10° under the horizon) the sun's flare is gone; a flare on another
    // directional light stays.
    let cam = camera();
    let flare_of = |direction: Vec3| {
        let mut g = source(LightType::Directional, Vec3::ZERO, direction);
        g.halo = None;
        g.flare = Some(LensFlare::default());
        g
    };
    let under = Vec3::new(0.0, -10f32.to_radians().sin(), 10f32.to_radians().cos());
    let mut env = Environment::default();
    env.sun = Some(crate::render::environment::Sun {
        direction: under,
        color: [1.0; 3],
        intensity: 2.0,
    });
    // Light from the sun (travelling -under), and one from in front of the camera.
    env.glows = vec![flare_of(-under)];
    let f = frame(&cam, &env, PostSettings::default());
    assert!(instances::build(&f, 1.6, true, 3.6).flares.is_empty());
    env.glows = vec![flare_of(Vec3::NEG_Z)];
    let f = frame(&cam, &env, PostSettings::default());
    assert!(!instances::build(&f, 1.6, true, 3.6).flares.is_empty());
}
