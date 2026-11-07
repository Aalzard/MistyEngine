//! Headless checks of Misty GI: a sunlit red wall bleeds onto the floor (also when the wall is
//! behind the camera, where screen-space GI can't see it), a roof occludes the sky, a moving
//! light moves the bounce within a second of frames, and the shaders build on the device.

use glam::{Mat4, Quat, Vec3};

use super::super::gpu_tests::{headless, read_back};
use super::super::*;
use crate::camera::{Camera, Projection};
use crate::scene::{MistyGlobalIlluminationOverride, PrimitiveMesh, ScreenSpaceQuality};

const W: u32 = 320;
const H: u32 = 240;

fn camera() -> Camera {
    Camera {
        position: Vec3::new(0.0, 2.2, -5.0),
        rotation: Quat::from_rotation_x(18f32.to_radians()),
        projection: Projection::Perspective { fov_y: 60.0 },
        near: 0.1,
        far: 100.0,
    }
}

fn boxed(center: Vec3, size: Vec3, color: [f32; 3]) -> MeshDraw {
    MeshDraw {
        mesh: PrimitiveMesh::Cube.into(),
        model: Mat4::from_scale_rotation_translation(size, Quat::IDENTITY, center),
        material: MaterialParams {
            color: [color[0], color[1], color[2], 1.0],
            smoothness: 0.1,
            ..Default::default()
        },
        cast_shadows: true,
        receive_shadows: true,
        outline: false,
        sorting: Default::default(),
        lod: None,
    }
}

/// White floor, a red wall on the left (inner face at x = -2), a white back wall.
fn room() -> Vec<MeshDraw> {
    vec![
        boxed(
            Vec3::new(0.0, -0.05, 1.0),
            Vec3::new(8.0, 0.1, 10.0),
            [0.8; 3],
        ),
        boxed(
            Vec3::new(-2.15, 1.5, 1.0),
            Vec3::new(0.3, 3.0, 10.0),
            [0.9, 0.08, 0.06],
        ),
        boxed(
            Vec3::new(0.0, 1.5, 3.15),
            Vec3::new(8.0, 3.0, 0.3),
            [0.8; 3],
        ),
    ]
}

/// Sun travelling along `direction`.
fn sun(direction: Vec3) -> LightData {
    LightData {
        kind: LightType::Directional,
        position: Vec3::ZERO,
        direction: direction.normalize(),
        color: [1.0; 3],
        intensity: 1.5,
        range: 0.0,
        spot_angle: 0.0,
        shadows: Shadows::Hard,
        shadow: Default::default(),
        volumetric: Default::default(),
        contact_shadows: false,
        area: Default::default(),
    }
}

/// Low sun from the right: it lights the red wall's inner face.
fn sun_on_red_wall() -> LightData {
    sun(Vec3::new(-1.0, -0.5, 0.3))
}

fn misty(quality: ScreenSpaceQuality) -> PostSettings {
    PostSettings {
        misty_gi: MistyGiSettings::from_override(&MistyGlobalIlluminationOverride {
            active: true,
            quality,
            ..Default::default()
        }),
        ..Default::default()
    }
}

fn render(
    renderer: &mut Renderer,
    gpu: &(wgpu::Device, wgpu::Queue),
    target: &RenderTarget,
    cam: &Camera,
    meshes: &[MeshDraw],
    lights: &[LightData],
    post: PostSettings,
    frames: u32,
) -> Vec<u8> {
    let (device, queue) = gpu;
    let environment = Environment::default();
    let frame = Frame {
        camera: cam,
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
        water: &[],
        shaded: true,
        lights,
    };
    for _ in 0..frames.max(1) {
        renderer.render(device, queue, target, &frame);
    }
    read_back(device, queue, target)
}

/// Mean sRGB colour (0..255) of a 5×5 block around the pixel showing world point `p`.
fn rgb(cam: &Camera, image: &[u8], p: Vec3) -> Vec3 {
    let ndc = cam.view_projection(W as f32 / H as f32).project_point3(p);
    let x = ((ndc.x * 0.5 + 0.5) * W as f32) as i32;
    let y = ((0.5 - ndc.y * 0.5) * H as f32) as i32;
    assert!(
        (2..W as i32 - 2).contains(&x) && (2..H as i32 - 2).contains(&y),
        "{p} is off screen"
    );
    let mut sum = Vec3::ZERO;
    for dy in -2..=2 {
        for dx in -2..=2 {
            let i = (((y + dy) as u32 * W + (x + dx) as u32) * 4) as usize;
            sum += Vec3::new(image[i] as f32, image[i + 1] as f32, image[i + 2] as f32);
        }
    }
    sum / 25.0
}

fn redness(c: Vec3) -> f32 {
    c.x - (c.y + c.z) * 0.5
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
fn misty_gi_bleeds_the_red_wall_onto_the_floor() {
    let Some(gpu) = headless() else {
        eprintln!("no GPU adapter: skipped");
        return;
    };
    let mut renderer = Renderer::new(&gpu.0);
    let target = renderer.create_target(&gpu.0, W, H);
    let (meshes, lights, cam) = (room(), [sun_on_red_wall()], camera());
    let off = render(
        &mut renderer,
        &gpu,
        &target,
        &cam,
        &meshes,
        &lights,
        Default::default(),
        2,
    );
    for q in ScreenSpaceQuality::ALL {
        let on = render(
            &mut renderer,
            &gpu,
            &target,
            &cam,
            &meshes,
            &lights,
            misty(q),
            40,
        );
        save("misty_room_off", &off);
        save(&format!("misty_room_{}", q.label()), &on);
        let beside = Vec3::new(-1.6, 0.0, 0.0);
        let (a, b) = (rgb(&cam, &off, beside), rgb(&cam, &on, beside));
        eprintln!("{}: floor beside the red wall: off {a}, on {b}", q.label());
        assert!(
            redness(b) > 6.0 && redness(b) > redness(a) + 6.0,
            "{}: colour bleeding: {} → {}",
            q.label(),
            redness(a),
            redness(b)
        );
    }
}

/// The red wall behind the camera still bleeds onto the floor in front of it: the world-space
/// tier sees what the screen doesn't.
#[test]
fn misty_gi_bounces_light_from_off_screen() {
    let Some(gpu) = headless() else {
        eprintln!("no GPU adapter: skipped");
        return;
    };
    let mut renderer = Renderer::new(&gpu.0);
    let target = renderer.create_target(&gpu.0, W, H);
    // Looking right and down, away from the red wall (behind the camera).
    let cam = Camera {
        position: Vec3::new(-1.2, 1.6, -0.5),
        rotation: Quat::from_rotation_y(80f32.to_radians())
            * Quat::from_rotation_x(45f32.to_radians()),
        ..camera()
    };
    let (meshes, lights) = (room(), [sun_on_red_wall()]);
    let floor = Vec3::new(-0.4, 0.0, -0.5);
    let on = render(
        &mut renderer,
        &gpu,
        &target,
        &cam,
        &meshes,
        &lights,
        misty(ScreenSpaceQuality::High),
        40,
    );
    let mut grey = meshes.clone();
    grey[1].material.color = [0.5, 0.5, 0.5, 1.0];
    let neutral = render(
        &mut renderer,
        &gpu,
        &target,
        &cam,
        &grey,
        &lights,
        misty(ScreenSpaceQuality::High),
        40,
    );
    save("misty_offscreen_red", &on);
    save("misty_offscreen_grey", &neutral);
    let (a, b) = (rgb(&cam, &neutral, floor), rgb(&cam, &on, floor));
    eprintln!("floor in front of the camera: grey wall {a}, red wall {b}");
    assert!(redness(b) > 4.0 && redness(b) > redness(a) + 4.0, "{a} → {b}");
}

/// A roof on posts shades the floor under it from the sky (no sun).
#[test]
fn misty_gi_occludes_the_sky_under_a_roof() {
    let Some(gpu) = headless() else {
        eprintln!("no GPU adapter: skipped");
        return;
    };
    let mut renderer = Renderer::new(&gpu.0);
    let target = renderer.create_target(&gpu.0, W, H);
    let cam = camera();
    let meshes = vec![
        boxed(
            Vec3::new(0.0, -0.05, 4.0),
            Vec3::new(40.0, 0.1, 40.0),
            [0.8; 3],
        ),
        boxed(Vec3::new(1.5, 1.2, 1.5), Vec3::new(3.0, 0.1, 3.0), [0.8; 3]),
    ];
    let lights: [LightData; 0] = [];
    let off = render(
        &mut renderer,
        &gpu,
        &target,
        &cam,
        &meshes,
        &lights,
        Default::default(),
        2,
    );
    let on = render(
        &mut renderer,
        &gpu,
        &target,
        &cam,
        &meshes,
        &lights,
        misty(ScreenSpaceQuality::High),
        40,
    );
    save("misty_roof_off", &off);
    save("misty_roof_on", &on);
    let (under, open) = (Vec3::new(1.5, 0.0, 1.5), Vec3::new(-2.0, 0.0, 0.5));
    let (u_off, u_on) = (rgb(&cam, &off, under), rgb(&cam, &on, under));
    let (o_off, o_on) = (rgb(&cam, &off, open), rgb(&cam, &on, open));
    eprintln!("under the roof: off {u_off}, on {u_on}; open floor: off {o_off}, on {o_on}");
    // The blue sky light is what the roof takes away.
    assert!(
        u_on.z < u_off.z * 0.75,
        "sky occlusion under the roof: {u_off} → {u_on}"
    );
    // Open ground keeps about the ambient it had.
    let ratio = o_on.element_sum() / o_off.element_sum().max(1.0);
    assert!((0.75..1.3).contains(&ratio), "open floor: {o_off} → {o_on}");
}

/// When the sun swings from the red wall to the back wall, the floor's red tint moves to its
/// new value (a fresh view's, settled) within about half a second of frames.
#[test]
fn misty_gi_follows_a_moving_light() {
    let Some(gpu) = headless() else {
        eprintln!("no GPU adapter: skipped");
        return;
    };
    let (meshes, cam) = (room(), camera());
    let beside = Vec3::new(-1.6, 0.0, 0.0);
    let post = misty(ScreenSpaceQuality::High);
    // Sun from behind the camera onto the back wall; the red wall's inner face is in shade.
    let moved = [sun(Vec3::new(0.3, -0.5, 1.0))];
    let settled = |lights: &[LightData]| {
        let mut r = Renderer::new(&gpu.0);
        let t = r.create_target(&gpu.0, W, H);
        let image = render(&mut r, &gpu, &t, &cam, &meshes, lights, post, 60);
        redness(rgb(&cam, &image, beside))
    };
    let (start, goal) = (settled(&[sun_on_red_wall()]), settled(&moved));
    let mut renderer = Renderer::new(&gpu.0);
    let target = renderer.create_target(&gpu.0, W, H);
    render(
        &mut renderer,
        &gpu,
        &target,
        &cam,
        &meshes,
        &[sun_on_red_wall()],
        post,
        60,
    );
    let mut half = None;
    let mut ninety = None;
    let mut last = start;
    for frame in 1..=90 {
        let image = render(&mut renderer, &gpu, &target, &cam, &meshes, &moved, post, 1);
        last = redness(rgb(&cam, &image, beside));
        let done = (start - last) / (start - goal);
        if half.is_none() && done >= 0.5 {
            half = Some(frame);
        }
        if ninety.is_none() && done >= 0.9 {
            ninety = Some(frame);
        }
    }
    eprintln!(
        "redness {start} → {last} (settled {goal}); half-way after {half:?}, 90 % after {ninety:?} frames"
    );
    assert!(start > goal + 10.0, "the move changes the bounce: {start} vs {goal}");
    assert!((last - goal).abs() < 0.15 * (start - goal), "{last} vs {goal}");
    assert!(half.is_some_and(|f| f <= 20), "{half:?}");
}

/// Copy `bytes` of `buffer` back to the CPU.
fn read_buffer(gpu: &(wgpu::Device, wgpu::Queue), buffer: &wgpu::Buffer, bytes: u64) -> Vec<f32> {
    let (device, queue) = gpu;
    let out = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("misty gi test readback"),
        size: bytes,
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    let mut encoder = device.create_command_encoder(&Default::default());
    encoder.copy_buffer_to_buffer(buffer, 0, &out, 0, bytes);
    queue.submit([encoder.finish()]);
    let slice = out.slice(..);
    slice.map_async(wgpu::MapMode::Read, |_| {});
    let _ = device.poll(wgpu::PollType::wait_indefinitely());
    let data = slice.get_mapped_range().expect("mapped");
    bytemuck::cast_slice(&data).to_vec()
}

/// Level 0's SDF at its middle z slice (distance channel), as f32.
fn read_sdf_slice(gpu: &(wgpu::Device, wgpu::Queue), view: &wgpu::TextureView, dim: u32, z: u32) -> Vec<f32> {
    let (device, queue) = gpu;
    let row = (dim * 8).div_ceil(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT) * wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;
    let out = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("misty gi test sdf"),
        size: (row * dim) as u64,
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    let mut encoder = device.create_command_encoder(&Default::default());
    encoder.copy_texture_to_buffer(
        wgpu::TexelCopyTextureInfo {
            texture: view.texture(),
            mip_level: 0,
            origin: wgpu::Origin3d { x: 0, y: 0, z },
            aspect: wgpu::TextureAspect::All,
        },
        wgpu::TexelCopyBufferInfo {
            buffer: &out,
            layout: wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(row),
                rows_per_image: Some(dim),
            },
        },
        wgpu::Extent3d { width: dim, height: dim, depth_or_array_layers: 1 },
    );
    queue.submit([encoder.finish()]);
    let slice = out.slice(..);
    slice.map_async(wgpu::MapMode::Read, |_| {});
    let _ = device.poll(wgpu::PollType::wait_indefinitely());
    let data = slice.get_mapped_range().expect("mapped");
    let mut values = Vec::new();
    for y in 0..dim as usize {
        for x in 0..dim as usize {
            let i = y * row as usize + x * 8;
            values.push(half::f16::from_le_bytes([data[i], data[i + 1]]).to_f32());
        }
    }
    values
}

/// Per level: probes updated, inside, mean and largest red c0 (diagnostics).
fn probe_report(gpu: &(wgpu::Device, wgpu::Queue), target: &RenderTarget) -> Vec<f32> {
    let slot = target.misty_gi.0.lock().unwrap();
    let state = slot.as_ref().expect("misty state");
    let s = state.sizes;
    let per = (s.probes as usize).pow(3);
    let probes = read_buffer(gpu, &state.probes, (s.levels as usize * per * 64) as u64);
    let mut means = Vec::new();
    for k in 0..s.levels as usize {
        let range = k * per..(k + 1) * per;
        let valid = range.clone().filter(|i| probes[i * 16 + 15] > 0.0).count();
        let inside = range.clone().filter(|i| probes[i * 16 + 15] < 0.0).count();
        let mean = range.clone().map(|i| probes[i * 16]).sum::<f32>() / per as f32;
        let max = range.clone().map(|i| probes[i * 16]).fold(0.0f32, f32::max);
        eprintln!("level {k}: valid {valid}, inside {inside}, red c0 mean {mean:.3}, max {max:.3}");
        let mut top: Vec<usize> = range.clone().collect();
        top.sort_by(|a, b| probes[b * 16].total_cmp(&probes[a * 16]));
        for &i in top.iter().take(4) {
            let g = Vec3::new(probes[i * 16 + 12], probes[i * 16 + 13], probes[i * 16 + 14]);
            let spacing = s.dim as f32 * s.voxel * (1u32 << k) as f32 / s.probes as f32;
            eprintln!(
                "    c0 {:.2} at {} (c1 {:.2} {:.2} {:.2})",
                probes[i * 16],
                (g + 0.5) * spacing,
                probes[i * 16 + 1],
                probes[i * 16 + 2],
                probes[i * 16 + 3]
            );
        }
        means.push(mean);
    }
    means
}

/// `MISTY_GI_DIAG=low|medium|high`: the bleed room at that quality, probe stats per level.
#[test]
#[ignore = "diagnostics, prints only"]
fn misty_gi_probe_diagnostics() {
    let Some(gpu) = headless() else {
        return;
    };
    let q = match std::env::var("MISTY_GI_DIAG").as_deref() {
        Ok("low") => ScreenSpaceQuality::Low,
        Ok("medium") => ScreenSpaceQuality::Medium,
        _ => ScreenSpaceQuality::High,
    };
    let mut renderer = Renderer::new(&gpu.0);
    let target = renderer.create_target(&gpu.0, W, H);
    let (meshes, lights, cam) = (room(), [sun_on_red_wall()], camera());
    for frames in [1, 4, 8, 16, 32] {
        render(&mut renderer, &gpu, &target, &cam, &meshes, &lights, misty(q), frames);
        eprintln!("after {frames} more frames:");
        probe_report(&gpu, &target);
    }
}

/// The SDF holds the floor (negative under it, positive above) and open-air probes see the sky.
#[test]
fn misty_gi_builds_the_sdf_and_lights_the_probes() {
    let Some(gpu) = headless() else {
        eprintln!("no GPU adapter: skipped");
        return;
    };
    let mut renderer = Renderer::new(&gpu.0);
    let target = renderer.create_target(&gpu.0, W, H);
    let cam = camera();
    let meshes = vec![boxed(
        Vec3::new(0.0, -0.05, 4.0),
        Vec3::new(40.0, 0.1, 40.0),
        [0.8; 3],
    )];
    let lights: [LightData; 0] = [];
    render(&mut renderer, &gpu, &target, &cam, &meshes, &lights, misty(ScreenSpaceQuality::High), 8);
    probe_report(&gpu, &target);
    let slot = target.misty_gi.0.lock().unwrap();
    let state = slot.as_ref().expect("misty state");
    let s = state.sizes;
    // A vertical line of voxels through the middle of level 1 (x = dim/2, all y, z = dim/2).
    let slice = read_sdf_slice(&gpu, &state.sdf.view, s.dim, s.dim + s.dim / 2);
    let column: Vec<f32> = (0..s.dim as usize).map(|y| slice[y * s.dim as usize + s.dim as usize / 2]).collect();
    let (lo, v) = super::level_box(&s, 1, cam.position);
    eprintln!("level 1 box from {lo}, voxel {v}: column {column:?}");
    let probes = read_buffer(&gpu, &state.probes, s.levels as u64 * (s.probes as u64).pow(3) * 64);
    let n = probes.len() / 16;
    let valid = (0..n).filter(|i| probes[i * 16 + 15] > 0.0).count();
    let inside = (0..n).filter(|i| probes[i * 16 + 15] < 0.0).count();
    let mean_c0: f32 = (0..n).map(|i| probes[i * 16]).sum::<f32>() / n as f32;
    eprintln!("probes: {n}, valid {valid}, inside {inside}, mean red c0 {mean_c0}");
    assert!(column.iter().any(|&d| d < 0.0) && column.iter().any(|&d| d > 0.0), "floor in the SDF");
    assert!(valid > n / 2, "probes updated");
    assert!(mean_c0 > 0.1, "probes see the sky");
}
