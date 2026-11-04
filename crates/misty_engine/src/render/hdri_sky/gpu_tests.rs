//! Headless HDRI sky checks (skipped without an adapter): the background faces point the right
//! way and follow Rotation, a probe capture sees the HDRI, and spheres are lit by it (diffuse SH
//! and prefiltered reflections) with no lights in the scene.

use std::sync::Arc;

use glam::{Mat4, Quat, Vec3};

use crate::hdri::{HdrImage, HdriHandle, HdriSky};
use crate::render::gpu_tests::headless;
use crate::render::probes::capture::{CaptureInputs, CaptureSettings, ProbeCapture};
use crate::render::{Environment, MaterialParams, MeshDraw, Renderer};
use crate::scene::PrimitiveMesh;
use crate::scene::reflection_probe::ProbeClearFlags;
use crate::scene::render_settings::{RenderSettings, SkyboxKind};

fn environment(img: HdrImage, rotation: f32) -> Environment {
    let mut settings = RenderSettings::default();
    settings.skybox.kind = SkyboxKind::Hdri;
    settings.skybox.hdri.rotation = rotation;
    Environment {
        settings,
        hdri: Some(HdriHandle(Arc::new(HdriSky::new(img)))),
        ..Default::default()
    }
}

fn inputs(meshes: Vec<MeshDraw>, environment: Environment) -> CaptureInputs {
    CaptureInputs {
        meshes,
        lights: Vec::new(),
        terrains: Vec::new(),
        environment,
    }
}

fn settings(size: u32) -> CaptureSettings {
    CaptureSettings {
        position: Vec3::ZERO,
        size,
        near: 0.05,
        far: 100.0,
        clear: ProbeClearFlags::Skybox,
        background: [0.0; 3],
    }
}

fn sphere(at: Vec3, metallic: f32, smoothness: f32) -> MeshDraw {
    MeshDraw {
        mesh: PrimitiveMesh::Sphere.into(),
        model: Mat4::from_scale_rotation_translation(Vec3::splat(2.0), Quat::IDENTITY, at),
        material: MaterialParams {
            color: [1.0, 1.0, 1.0, 1.0],
            metallic,
            smoothness,
            ..Default::default()
        },
        cast_shadows: false,
        receive_shadows: false,
        outline: false,
        sorting: Default::default(),
        lod: None,
    }
}

/// +X half red, the rest blue: a probe capture's +X face sees red, −X blue; 180° swaps them.
#[test]
fn background_faces_and_rotation() {
    let Some((device, queue)) = headless() else {
        eprintln!("no GPU adapter: skipped");
        return;
    };
    let img = HdrImage::from_fn(512, 256, |d| {
        if d.x > 0.0 {
            Vec3::new(2.0, 0.0, 0.0)
        } else {
            Vec3::new(0.0, 0.0, 2.0)
        }
    });
    let mut renderer = Renderer::new(&device);
    let capture = ProbeCapture::new(&renderer, &device, 32);
    let center = |r: &mut Renderer, env: Environment, face: usize| {
        let i = inputs(Vec::new(), env);
        capture.face(r, &device, &queue, &i, &settings(32), face)[16 * 32 + 16]
    };
    let px = center(&mut renderer, environment(img.clone(), 0.0), 0);
    let nx = center(&mut renderer, environment(img.clone(), 0.0), 1);
    assert!(px.x > 1.5 && px.z < 0.1, "+X {px}");
    assert!(nx.z > 1.5 && nx.x < 0.1, "-X {nx}");
    let turned = center(&mut renderer, environment(img, 180.0), 0);
    assert!(turned.z > 1.5 && turned.x < 0.1, "rotated +X {turned}");
    let ms = renderer.hdri_build_ms().unwrap();
    eprintln!("512×256 HDRI → cubes: {ms:.2} ms");
}

/// With no lights, a uniform HDRI lights a white diffuse sphere and a chrome sphere with its
/// own colour; a sky bright above and dark below lights the diffuse sphere's top.
#[test]
fn spheres_are_lit_by_the_hdri() {
    let Some((device, queue)) = headless() else {
        eprintln!("no GPU adapter: skipped");
        return;
    };
    let mut renderer = Renderer::new(&device);
    let capture = ProbeCapture::new(&renderer, &device, 64);
    let c = Vec3::new(0.6, 0.3, 0.15);
    let uniform = environment(HdrImage::from_fn(256, 128, |_| c), 0.0);
    let at = Vec3::new(0.0, 0.0, 3.0);
    for (metallic, smoothness, tolerance) in [(0.0, 0.0, 0.2), (1.0, 0.95, 0.15)] {
        let i = inputs(vec![sphere(at, metallic, smoothness)], uniform.clone());
        let face = capture.face(&mut renderer, &device, &queue, &i, &settings(64), 4);
        let got = face[32 * 64 + 32];
        let err = ((got - c) / c).abs().max_element();
        assert!(err < tolerance, "metallic {metallic}: {got} vs {c}");
    }
    let lit_from_above = environment(
        HdrImage::from_fn(256, 128, |d| {
            Vec3::splat(if d.y > 0.0 { 2.0 } else { 0.05 })
        }),
        0.0,
    );
    let i = inputs(vec![sphere(at, 0.0, 0.0)], lit_from_above);
    let face = capture.face(&mut renderer, &device, &queue, &i, &settings(64), 4);
    // The sphere spans rows ~11..53 of the 90° face: compare near its top and bottom.
    let (top, bottom) = (face[16 * 64 + 32], face[48 * 64 + 32]);
    assert!(top.x > bottom.x * 3.0, "top {top} bottom {bottom}");

    // A 2048 × 1024 HDRI (the sample's size) for the timing in docs/progress.md.
    let big = environment(
        HdrImage::from_fn(2048, 1024, |d| Vec3::splat(d.y + 1.0)),
        0.0,
    );
    let _ = capture.face(
        &mut renderer,
        &device,
        &queue,
        &inputs(vec![], big),
        &settings(64),
        0,
    );
    let ms = renderer.hdri_build_ms().unwrap();
    eprintln!("2048×1024 HDRI → 512² background + GGX chain: {ms:.2} ms");
}
