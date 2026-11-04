//! Headless checks of the screen-space effects: AO darkens the floor around a resting sphere
//! (less under a floating one), contact shadows darken the floor beside a cube, and with
//! both off the image is the same as a renderer that never ran them.

use glam::{Mat4, Quat, Vec3};

use super::super::gpu_tests::{headless, read_back};
use super::super::*;
use crate::camera::{Camera, Projection};
use crate::scene::{AmbientOcclusionOverride, ContactShadowsOverride, PrimitiveMesh};

const W: u32 = 320;
const H: u32 = 240;

fn camera() -> Camera {
    Camera {
        position: Vec3::new(0.0, 3.0, -4.0),
        rotation: Quat::from_rotation_x(3f32.atan2(4.0)),
        projection: Projection::Perspective { fov_y: 60.0 },
        near: 0.1,
        far: 100.0,
    }
}

fn draw(mesh: PrimitiveMesh, model: Mat4) -> MeshDraw {
    MeshDraw {
        mesh: mesh.into(),
        model,
        material: MaterialParams {
            color: [0.5, 0.5, 0.5, 1.0],
            smoothness: 0.2,
            ..Default::default()
        },
        cast_shadows: true,
        receive_shadows: true,
        outline: false,
        sorting: Default::default(),
    }
}

fn floor() -> MeshDraw {
    draw(PrimitiveMesh::Plane, Mat4::from_scale(Vec3::splat(4.0)))
}

/// Resting sphere at x = -1.5, one floating 1 m above the floor at x = 1.5.
fn spheres() -> Vec<MeshDraw> {
    vec![
        floor(),
        draw(
            PrimitiveMesh::Sphere,
            Mat4::from_translation(Vec3::new(-1.5, 0.5, 0.0)),
        ),
        draw(
            PrimitiveMesh::Sphere,
            Mat4::from_translation(Vec3::new(1.5, 1.5, 0.0)),
        ),
    ]
}

fn ao_on(temporal: bool) -> PostSettings {
    PostSettings {
        ao: AoSettings::from_override(&AmbientOcclusionOverride {
            active: true,
            full_resolution: true,
            temporal,
            ..Default::default()
        }),
        ..Default::default()
    }
}

fn render(
    renderer: &mut Renderer,
    gpu: &(wgpu::Device, wgpu::Queue),
    target: &RenderTarget,
    meshes: &[MeshDraw],
    lights: &[LightData],
    post: PostSettings,
) -> Vec<u8> {
    let (device, queue) = gpu;
    let cam = camera();
    let environment = Environment::default();
    let frame = Frame {
        camera: &cam,
        grid: None,
        skybox: true,
        background: [0.1; 3],
        environment: &environment,
        post,
        lines: &[],
        overlay: &[],
        meshes,
        particles: &[],
        terrains: &[],
        shaded: true,
        lights,
    };
    renderer.render(device, queue, target, &frame);
    read_back(device, queue, target)
}

/// Mean brightness (0..255) of a 5×5 block around the pixel showing world point `p`.
fn brightness(image: &[u8], p: Vec3) -> f32 {
    let ndc = camera()
        .view_projection(W as f32 / H as f32)
        .project_point3(p);
    let x = ((ndc.x * 0.5 + 0.5) * W as f32) as i32;
    let y = ((0.5 - ndc.y * 0.5) * H as f32) as i32;
    let mut sum = 0.0;
    for dy in -2..=2 {
        for dx in -2..=2 {
            let i = (((y + dy) as u32 * W + (x + dx) as u32) * 4) as usize;
            sum += image[i..i + 3].iter().map(|&c| c as f32).sum::<f32>() / 3.0;
        }
    }
    sum / 25.0
}

fn save(name: &str, image: &[u8]) {
    // `MISTY_GPU_TEST_PNG=<dir>` saves the images for a look.
    if let Some(dir) = std::env::var_os("MISTY_GPU_TEST_PNG") {
        let path = std::path::Path::new(&dir).join(format!("{name}.png"));
        image::RgbaImage::from_raw(W, H, image.to_vec())
            .expect("size")
            .save(path)
            .expect("png");
    }
}

#[test]
fn ao_darkens_the_floor_around_a_resting_sphere_and_fades_with_height() {
    let Some(gpu) = headless() else {
        eprintln!("no GPU adapter: skipped");
        return;
    };
    let mut renderer = Renderer::new(&gpu.0);
    let target = renderer.create_target(&gpu.0, W, H);
    let meshes = spheres();
    let off = render(
        &mut renderer,
        &gpu,
        &target,
        &meshes,
        &[],
        Default::default(),
    );
    let on = render(&mut renderer, &gpu, &target, &meshes, &[], ao_on(false));
    save("ao_spheres_off", &off);
    save("ao_spheres_on", &on);

    let near_rest = Vec3::new(-1.5, 0.0, -0.4);
    let near_float = Vec3::new(1.5, 0.0, -0.4);
    let open = Vec3::new(0.0, 0.0, -2.0);
    let (rest, float, far) = (
        brightness(&on, near_rest),
        brightness(&on, near_float),
        brightness(&on, open),
    );
    eprintln!("AO on: rest {rest}, floating {float}, open {far}");
    // Without AO the floor is evenly lit.
    let base = brightness(&off, near_rest);
    assert!((base - brightness(&off, open)).abs() < 3.0);
    assert!(
        (far - brightness(&off, open)).abs() < 4.0,
        "open floor keeps its light"
    );
    assert!(rest < far * 0.85, "contact darkens: {rest} vs {far}");
    assert!(
        float > rest + 3.0,
        "a floating sphere darkens less: {float} vs {rest}"
    );

    // Temporal accumulation converges to about the same image.
    let mut temporal = Vec::new();
    for _ in 0..12 {
        temporal = render(&mut renderer, &gpu, &target, &meshes, &[], ao_on(true));
    }
    save("ao_spheres_temporal", &temporal);
    let t = brightness(&temporal, near_rest);
    assert!((t - rest).abs() < 12.0, "temporal {t} vs {rest}");
}

#[test]
fn contact_shadows_darken_the_floor_beside_a_cube() {
    let Some(gpu) = headless() else {
        eprintln!("no GPU adapter: skipped");
        return;
    };
    let mut renderer = Renderer::new(&gpu.0);
    let target = renderer.create_target(&gpu.0, W, H);
    let meshes = vec![
        floor(),
        draw(
            PrimitiveMesh::Cube,
            Mat4::from_translation(Vec3::new(0.0, 0.5, 0.0)),
        ),
    ];
    // No shadow map: only contact shadows can darken the floor.
    let light = |contact_shadows| LightData {
        kind: LightType::Directional,
        position: Vec3::ZERO,
        direction: Vec3::new(-1.0, -1.0, 0.0).normalize(),
        color: [1.0; 3],
        intensity: 1.0,
        range: 0.0,
        spot_angle: 0.0,
        shadows: Shadows::None,
        shadow: Default::default(),
        volumetric: Default::default(),
        contact_shadows,
    };
    let post = PostSettings {
        contact_shadows: ContactShadowSettings::from_override(&ContactShadowsOverride {
            active: true,
            length: 0.5,
            ..Default::default()
        }),
        ..Default::default()
    };
    let plain = render(&mut renderer, &gpu, &target, &meshes, &[light(false)], post);
    let contact = render(&mut renderer, &gpu, &target, &meshes, &[light(true)], post);
    save("contact_off", &plain);
    save("contact_on", &contact);
    let beside = Vec3::new(-0.6, 0.0, -0.45);
    let open = Vec3::new(-2.0, 0.0, -0.45);
    let (b0, b1) = (brightness(&plain, beside), brightness(&contact, beside));
    eprintln!("beside the cube: {b0} → {b1}");
    assert!(b1 < b0 * 0.75, "{b1} vs {b0}");
    assert!((brightness(&plain, open) - brightness(&contact, open)).abs() < 2.0);
}

#[test]
fn effects_off_render_exactly_as_without_them() {
    let Some(gpu) = headless() else {
        eprintln!("no GPU adapter: skipped");
        return;
    };
    let mut meshes = spheres();
    meshes.push(draw(
        PrimitiveMesh::Cube,
        Mat4::from_translation(Vec3::new(0.0, 0.5, 1.0)),
    ));
    let light = |contact_shadows| LightData {
        kind: LightType::Directional,
        position: Vec3::ZERO,
        direction: Vec3::new(0.4, -1.0, 0.5).normalize(),
        color: [1.0; 3],
        intensity: 1.2,
        range: 0.0,
        spot_angle: 0.0,
        shadows: Shadows::Soft,
        shadow: Default::default(),
        volumetric: Default::default(),
        contact_shadows,
    };
    let fresh = |lights: &[LightData], post| {
        let mut renderer = Renderer::new(&gpu.0);
        let target = renderer.create_target(&gpu.0, W, H);
        render(&mut renderer, &gpu, &target, &meshes, lights, post)
    };
    let baseline = fresh(&[light(false)], PostSettings::default());
    // Settings present but switched off, and lights asking for contact shadows.
    let mut off = ao_on(true);
    off.ao.enabled = false;
    off.contact_shadows.length = 0.4;
    assert!(fresh(&[light(true)], off) == baseline);

    // Turning the effects on and off again leaves nothing behind.
    let mut renderer = Renderer::new(&gpu.0);
    let target = renderer.create_target(&gpu.0, W, H);
    let mut on = ao_on(true);
    on.contact_shadows.enabled = true;
    let lit = render(&mut renderer, &gpu, &target, &meshes, &[light(true)], on);
    assert!(lit != baseline);
    let again = render(
        &mut renderer,
        &gpu,
        &target,
        &meshes,
        &[light(false)],
        Default::default(),
    );
    assert!(again == baseline);
}

#[test]
fn ao_debug_view_shows_grey_occlusion() {
    let Some(gpu) = headless() else {
        eprintln!("no GPU adapter: skipped");
        return;
    };
    let mut renderer = Renderer::new(&gpu.0);
    let target = renderer.create_target(&gpu.0, W, H);
    let mut post = ao_on(false);
    post.ao = post.ao.debug_view();
    let image = render(&mut renderer, &gpu, &target, &spheres(), &[], post);
    save("ao_debug", &image);
    let open = brightness(&image, Vec3::new(0.0, 0.0, -2.0));
    let rest = brightness(&image, Vec3::new(-1.5, 0.0, -0.4));
    assert!(open > 245.0, "unoccluded floor is white: {open}");
    assert!(rest < open - 25.0, "{rest} vs {open}");

    // A wall standing on the floor: the crease is dark and it fades over the radius.
    let wall = draw(
        PrimitiveMesh::Cube,
        Mat4::from_scale_rotation_translation(
            Vec3::new(6.0, 3.0, 0.2),
            Quat::IDENTITY,
            Vec3::new(0.0, 1.5, 1.0),
        ),
    );
    for full_resolution in [true, false] {
        let mut post = post;
        post.ao.full_resolution = full_resolution;
        let image = render(
            &mut renderer,
            &gpu,
            &target,
            &[floor(), wall.clone()],
            &[],
            post,
        );
        save(&format!("ao_crease_{full_resolution}"), &image);
        let at = |z: f32| brightness(&image, Vec3::new(0.5, 0.0, z));
        let (crease, mid, away) = (at(0.8), at(0.2), at(-1.5));
        eprintln!("crease {crease}, 0.7 m {mid}, 2.4 m {away} (full res {full_resolution})");
        assert!(
            crease < 225.0 && crease < mid && mid < away,
            "{crease} {mid} {away}"
        );
        assert!(away > 245.0, "{away}");
    }
}
