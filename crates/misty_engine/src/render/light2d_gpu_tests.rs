//! Headless renders of 2D lights (skipped without an adapter): a sprite lit by a point light is bright
//! near it and dark far away; a shadow caster darkens the part behind it; with no 2D light
//! the sprite is unlit.

use glam::{Mat4, Quat, Vec2, Vec3};

use crate::camera::{Camera, Projection};
use crate::light2d::*;
use crate::render::gpu_tests::{headless, read_back};
use crate::render::{Environment, Frame, MeshDraw, PostSettings, Renderer};
use crate::scene::{SpriteRenderer, Transform};

const W: u32 = 200;
const H: u32 = 100;

fn camera() -> Camera {
    Camera {
        position: Vec3::new(0.0, 0.0, -10.0),
        rotation: Quat::IDENTITY,
        projection: Projection::Orthographic { size: 2.5 },
        near: 0.1,
        far: 100.0,
    }
}

/// A white 10×5 sprite filling the view.
fn backdrop() -> MeshDraw {
    let r = SpriteRenderer {
        sprite: Some(crate::sprites::builtin::path("Square")),
        ..Default::default()
    };
    let m = Mat4::from_scale_rotation_translation(
        Vec3::new(10.0, 5.0, 1.0),
        Quat::IDENTITY,
        Vec3::ZERO,
    );
    crate::sprites::draw_of(&r, m, false).expect("built-in sprite")
}

fn render(device: &wgpu::Device, queue: &wgpu::Queue, lights: Lights2D) -> Vec<u8> {
    let mut r = Renderer::new(device);
    let cam = camera();
    let mut env = Environment::default();
    env.settings.environment_lighting.intensity = 0.0;
    env.lights_2d = lights;
    let meshes = [backdrop()];
    let frame = Frame {
        camera: &cam,
        grid: None,
        skybox: false,
        background: [0.0; 3],
        environment: &env,
        post: PostSettings {
            bloom_intensity: 0.0,
            ..Default::default()
        },
        lines: &[],
        overlay: &[],
        meshes: &meshes,
        particles: &[],
        terrains: &[],
        water: &[],
        shaded: true,
        lights: &[],
    };
    let target = r.create_target(device, W, H);
    r.render(device, queue, &target, &frame);
    let image = read_back(device, queue, &target);
    if let Some(dir) = std::env::var_os("MISTY_LIGHT2D_TEST_DIR") {
        static N: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
        let n = N.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        image::RgbaImage::from_raw(W, H, image.clone())
            .unwrap()
            .save(std::path::Path::new(&dir).join(format!("light2d_{n}.png")))
            .unwrap();
    }
    image
}

/// Brightness (0..255) at a world point (view spans x −5..5, y −2.5..2.5).
fn at(image: &[u8], p: Vec2) -> f32 {
    let x = ((p.x + 5.0) / 10.0 * W as f32) as u32;
    let y = ((2.5 - p.y) / 5.0 * H as f32) as u32;
    let i = ((y.min(H - 1) * W + x.min(W - 1)) * 4) as usize;
    (image[i] as f32 + image[i + 1] as f32 + image[i + 2] as f32) / 3.0
}

#[test]
fn point_light_falloff_and_shadow_on_a_headless_render() {
    let Some((device, queue)) = headless() else {
        return;
    };
    // No 2D light: the sprite is unlit (white).
    let unlit = render(&device, &queue, Lights2D::default());
    assert!(at(&unlit, Vec2::new(4.0, 2.0)) > 240.0);

    let mut s = crate::scene::Scene::new("Lit");
    let torch = s.spawn("Torch", None);
    s.set_transform(torch, Transform::from_position(Vec3::new(-2.0, 0.0, 0.0)));
    s.add(
        torch,
        Light2D {
            outer_radius: 3.0,
            inner_radius: 0.5,
            shadows_enabled: true,
            shadow_softness: 0.0,
            ..Default::default()
        },
    );
    let lit = render(&device, &queue, collect(&s));
    let near = at(&lit, Vec2::new(-2.0, 0.2));
    let mid = at(&lit, Vec2::new(-0.5, 0.0));
    let far = at(&lit, Vec2::new(3.0, 0.0));
    assert!(near > 200.0, "bright inside the inner radius: {near}");
    assert!(
        mid > 10.0 && mid < near - 40.0,
        "fades with distance: {mid}"
    );
    assert!(far < 3.0, "dark beyond the outer radius: {far}");

    // A caster above the torch: the column above it falls into shadow.
    let post = s.spawn("Post", None);
    let mut t = Transform::from_position(Vec3::new(-2.0, 1.0, 0.0));
    t.scale = Vec3::new(1.0, 0.3, 1.0);
    s.set_transform(post, t);
    s.add(
        post,
        SpriteRenderer {
            sprite: Some(crate::sprites::builtin::path("Square")),
            ..Default::default()
        },
    );
    s.add(post, ShadowCaster2D::default());
    let shadowed = render(&device, &queue, collect(&s));
    let above_before = at(&lit, Vec2::new(-2.0, 1.8));
    let above_after = at(&shadowed, Vec2::new(-2.0, 1.8));
    assert!(above_before > 40.0, "lit before: {above_before}");
    assert!(above_after < 3.0, "shadowed after: {above_after}");
    let below = at(&shadowed, Vec2::new(-2.0, -1.0));
    assert!(
        (below - at(&lit, Vec2::new(-2.0, -1.0))).abs() < 3.0,
        "below unchanged"
    );
}
