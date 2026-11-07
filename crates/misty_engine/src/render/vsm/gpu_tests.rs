//! Headless GPU checks of the virtual shadow map: it shadows where the cascades do, sharper;
//! a still view stops rendering pages once its pages are cached; a moving caster re-renders
//! only the pages under it; every debug view renders.

use glam::{Mat4, Quat, Vec3};

use crate::camera::{Camera, Projection};
use crate::quality::{QualityLevel, ShadowMode};
use crate::render::gpu_tests::{headless, read_back};
use crate::render::*;
use crate::scene::PrimitiveMesh;

const W: u32 = 640;
const H: u32 = 360;

fn draw(mesh: PrimitiveMesh, model: Mat4) -> MeshDraw {
    MeshDraw {
        mesh: mesh.into(),
        model,
        material: MaterialParams {
            smoothness: 0.0,
            ..Default::default()
        },
        cast_shadows: true,
        receive_shadows: true,
        outline: false,
        sorting: Default::default(),
        lod: None,
    }
}

fn camera(x: f32) -> Camera {
    Camera {
        position: Vec3::new(x, 3.0, -9.0),
        rotation: Quat::from_rotation_x(16f32.to_radians()),
        projection: Projection::Perspective { fov_y: 50.0 },
        near: 0.1,
        far: 300.0,
    }
}

/// A ground plane, a pole and a cube (moved by `cube_x`), lit by a sun at 40°.
fn scene(cube_x: f32) -> Vec<MeshDraw> {
    vec![
        draw(
            PrimitiveMesh::Plane,
            Mat4::from_scale(Vec3::new(8.0, 1.0, 8.0)),
        ),
        draw(
            PrimitiveMesh::Cube,
            Mat4::from_scale_rotation_translation(
                Vec3::new(0.15, 3.0, 0.15),
                Quat::IDENTITY,
                Vec3::new(-1.0, 1.5, 0.0),
            ),
        ),
        draw(
            PrimitiveMesh::Cube,
            Mat4::from_translation(Vec3::new(cube_x, 0.5, 3.0)),
        ),
    ]
}

fn sun(shadows: Shadows) -> LightData {
    let e = 40f32.to_radians();
    LightData {
        kind: LightType::Directional,
        position: Vec3::ZERO,
        direction: Vec3::new(e.cos() * 0.6, -e.sin(), e.cos() * 0.8).normalize(),
        color: [1.0; 3],
        intensity: 1.5,
        range: 0.0,
        spot_angle: 0.0,
        shadows,
        contact_shadows: false,
        area: Default::default(),
        shadow: LightShadow {
            size: 0.5,
            ..Default::default()
        },
        volumetric: Default::default(),
    }
}

struct View {
    camera: Camera,
    meshes: Vec<MeshDraw>,
    lights: [LightData; 1],
    environment: Environment,
}

impl View {
    fn new(cam_x: f32, cube_x: f32, shadows: Shadows) -> Self {
        Self {
            camera: camera(cam_x),
            meshes: scene(cube_x),
            lights: [sun(shadows)],
            environment: Environment::default(),
        }
    }

    fn frame(&self) -> Frame<'_> {
        Frame {
            camera: &self.camera,
            grid: None,
            skybox: false,
            background: [0.0; 3],
            environment: &self.environment,
            post: PostSettings::default(),
            lines: &[],
            overlay: &[],
            meshes: &self.meshes,
            particles: &[],
            terrains: &[],
            water: &[],
            shaded: true,
            lights: &self.lights,
        }
    }
}

fn quality(mode: ShadowMode) -> QualityLevel {
    QualityLevel {
        shadow_mode: mode,
        temporal_aa: false,
        shadow_temporal: false,
        shadow_denoise: false,
        ambient_occlusion: crate::quality::Tier::Off,
        volumetric_fog: crate::quality::Tier::Off,
        sun_shafts: crate::quality::Tier::Off,
        volumetric_clouds: crate::quality::Tier::Off,
        ..QualityLevel::high()
    }
}

/// Render `frames` frames, waiting for the GPU between them (the page feedback arrives a
/// frame later); returns the last image.
fn render(
    r: &mut Renderer,
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    t: &RenderTarget,
    v: &View,
    frames: usize,
) -> Vec<u8> {
    for _ in 0..frames {
        r.render(device, queue, t, &v.frame());
        let _ = device.poll(wgpu::PollType::wait_indefinitely());
    }
    read_back(device, queue, t)
}

fn save(name: &str, pixels: &[u8]) {
    if let Some(dir) = std::env::var_os("MISTY_GPU_TEST_DIR") {
        let path = std::path::Path::new(&dir).join(format!("vsm_{name}.png"));
        image::RgbaImage::from_raw(W, H, pixels.to_vec())
            .expect("size")
            .save(path)
            .expect("png");
    }
}

fn luma(p: &[u8], i: usize) -> f32 {
    p[i * 4] as f32 * 0.2126 + p[i * 4 + 1] as f32 * 0.7152 + p[i * 4 + 2] as f32 * 0.0722
}

#[test]
fn virtual_shadows_land_where_the_cascades_do() {
    let Some((device, queue)) = headless() else {
        return;
    };
    let view = View::new(0.0, 1.5, Shadows::Hard);
    let mut images = Vec::new();
    for mode in [ShadowMode::Cascaded, ShadowMode::Virtual] {
        let mut r = Renderer::with_queue(&device, &queue);
        r.set_quality(&device, &quality(mode));
        let t = r.create_target(&device, W, H);
        let img = render(&mut r, &device, &queue, &t, &view, 8);
        save(&format!("{mode:?}").to_lowercase(), &img);
        if mode == ShadowMode::Virtual {
            let s = r.vsm_stats().expect("virtual");
            assert!(s.resident > 0 && s.wanted > 0, "{s:?}");
        }
        images.push(img);
    }
    // Shadowed pixels (well below the lit ground) agree almost everywhere: the cascades' edges
    // are blurrier, so only pixels clearly lit in one and clearly dark in the other count.
    let n = (W * H) as usize;
    let lit = (0..n).map(|i| luma(&images[0], i)).fold(0.0, f32::max);
    let (mut dark_c, mut dark_v, mut disagree) = (0, 0, 0);
    for i in 0..n {
        let (c, v) = (luma(&images[0], i), luma(&images[1], i));
        dark_c += (c < lit * 0.5) as usize;
        dark_v += (v < lit * 0.5) as usize;
        disagree +=
            ((c < lit * 0.35 && v > lit * 0.75) || (v < lit * 0.35 && c > lit * 0.75)) as usize;
    }
    assert!(dark_c > 2000, "the scene has shadows ({dark_c})");
    let ratio = dark_v as f32 / dark_c as f32;
    assert!((0.85..1.15).contains(&ratio), "{dark_c} vs {dark_v}");
    assert!(disagree < dark_c / 50, "{disagree} of {dark_c}");
}

#[test]
fn a_still_view_stops_rendering_and_a_moving_caster_renders_only_its_pages() {
    let Some((device, queue)) = headless() else {
        return;
    };
    let mut r = Renderer::with_queue(&device, &queue);
    r.set_quality(&device, &quality(ShadowMode::Virtual));
    let t = r.create_target(&device, W, H);
    let still = View::new(0.0, 1.5, Shadows::Soft);
    render(&mut r, &device, &queue, &t, &still, 10);
    let settled = r.vsm_stats().expect("virtual");
    assert_eq!(settled.rendered, 0, "{settled:?}");
    assert_eq!(settled.pending, 0, "{settled:?}");
    let resident = settled.resident;
    // The cube moves: only pages under its old and new place render again.
    let moved = View::new(0.0, 2.0, Shadows::Soft);
    r.render(&device, &queue, &t, &moved.frame());
    let _ = device.poll(wgpu::PollType::wait_indefinitely());
    let s = r.vsm_stats().expect("virtual");
    assert!(s.invalidated > 0, "{s:?}");
    assert!(s.invalidated * 4 < resident, "{s:?} of {resident}");
    assert!(!s.reset);
    // Still again: nothing more to render.
    render(&mut r, &device, &queue, &t, &moved, 4);
    assert_eq!(r.vsm_stats().expect("virtual").rendered, 0);
    // The camera slides half a metre: a few new pages, the rest stays cached.
    let slid = View::new(0.5, 2.0, Shadows::Soft);
    render(&mut r, &device, &queue, &t, &slid, 1);
    let s = r.vsm_stats().expect("virtual");
    assert!(s.rendered * 4 < s.resident, "{s:?}");
}

#[test]
fn debug_views_render() {
    let Some((device, queue)) = headless() else {
        return;
    };
    let mut r = Renderer::with_queue(&device, &queue);
    r.set_quality(&device, &quality(ShadowMode::Virtual));
    let t = r.create_target(&device, W, H);
    let v = View::new(0.0, 1.5, Shadows::Soft);
    render(&mut r, &device, &queue, &t, &v, 6);
    for (name, d) in [
        ("levels", VsmDebug::Levels),
        ("cache", VsmDebug::Cache),
        ("pages", VsmDebug::PageTable),
    ] {
        r.set_vsm_debug(d);
        let img = render(&mut r, &device, &queue, &t, &v, 1);
        save(&format!("debug_{name}"), &img);
        // Tinted: not grey.
        let coloured = (0..(W * H) as usize)
            .filter(|&i| {
                let p = &img[i * 4..i * 4 + 3];
                p.iter().max().unwrap() - p.iter().min().unwrap() > 40
            })
            .count();
        assert!(coloured > 1000, "{name}: {coloured}");
    }
    // Back to cascades: the virtual objects go.
    r.set_quality(&device, &quality(ShadowMode::Cascaded));
    assert!(r.vsm_stats().is_none());
    render(&mut r, &device, &queue, &t, &v, 1);
}
