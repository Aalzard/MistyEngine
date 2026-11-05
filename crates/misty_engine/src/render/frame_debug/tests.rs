//! Frame Debugger capture: the event list of a small scene, replay up to an event, and no
//! recording when no capture is installed. GPU tests skip without an adapter.

use glam::{Mat4, Quat, Vec3};

use super::*;
use crate::camera::{Camera, Projection};
use crate::render::gpu_tests::{headless, read_back};
use crate::render::{
    Environment, Frame, LightData, MaterialParams, MeshDraw, PostSettings, Renderer, RenderingMode,
};
use crate::scene::{LightType, PrimitiveMesh, Shadows};

fn material(color: [f32; 4], mode: RenderingMode) -> MaterialParams {
    MaterialParams {
        color,
        mode,
        ..Default::default()
    }
}

fn draw(mesh: PrimitiveMesh, at: Vec3, scale: f32, m: &MaterialParams) -> MeshDraw {
    MeshDraw {
        mesh: mesh.into(),
        model: Mat4::from_scale_rotation_translation(Vec3::splat(scale), Quat::IDENTITY, at),
        material: m.clone(),
        cast_shadows: true,
        receive_shadows: true,
        outline: false,
        sorting: Default::default(),
        lod: None,
    }
}

/// Ground plane (red), three red cubes (one instanced call), a blue sphere and a glass cube.
fn meshes() -> Vec<MeshDraw> {
    let red = material([0.9, 0.2, 0.2, 1.0], RenderingMode::Opaque);
    let blue = material([0.2, 0.3, 0.9, 1.0], RenderingMode::Opaque);
    let glass = material([1.0, 1.0, 0.4, 0.5], RenderingMode::Fade);
    vec![
        draw(PrimitiveMesh::Plane, Vec3::ZERO, 2.0, &red),
        draw(PrimitiveMesh::Cube, Vec3::new(-2.0, 0.5, 0.0), 1.0, &red),
        draw(PrimitiveMesh::Cube, Vec3::new(0.0, 0.5, 0.0), 1.0, &red),
        draw(PrimitiveMesh::Cube, Vec3::new(2.0, 0.5, 0.0), 1.0, &red),
        draw(PrimitiveMesh::Sphere, Vec3::new(0.0, 0.8, 2.5), 1.2, &blue),
        draw(PrimitiveMesh::Cube, Vec3::new(0.0, 1.0, -2.5), 1.0, &glass),
    ]
}

fn labels() -> Vec<DrawLabel> {
    ["Ground", "Cube A", "Cube B", "Cube C", "Ball", "Glass"]
        .iter()
        .map(|n| DrawLabel {
            object: (*n).to_owned(),
            material: Some(format!("Assets/Materials/{n}.mat")),
            submesh: 0,
        })
        .collect()
}

fn sun() -> Vec<LightData> {
    vec![LightData {
        kind: LightType::Directional,
        position: Vec3::ZERO,
        direction: Vec3::new(0.3, -1.0, 0.4).normalize(),
        color: [1.0; 3],
        intensity: 1.0,
        range: 10.0,
        spot_angle: 30.0,
        shadows: Shadows::Hard,
        shadow: Default::default(),
        volumetric: Default::default(),
        contact_shadows: false,
        area: Default::default(),
    }]
}

/// Render the scene once (with `request` installed when given); the output pixels and the capture.
fn render(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    renderer: &mut Renderer,
    request: Option<CaptureRequest>,
) -> (Vec<u8>, Option<FrameCapture>) {
    let camera = Camera {
        position: Vec3::new(0.0, 5.0, -9.0),
        rotation: Quat::from_rotation_x(28f32.to_radians()),
        projection: Projection::Perspective { fov_y: 50.0 },
        near: 0.3,
        far: 60.0,
    };
    let (meshes, lights) = (meshes(), sun());
    let environment = Environment::default();
    let frame = Frame {
        camera: &camera,
        grid: None,
        skybox: true,
        background: [0.1; 3],
        environment: &environment,
        post: PostSettings::default(),
        lines: &[],
        overlay: &[],
        meshes: &meshes,
        particles: &[],
        terrains: &[],
        shaded: true,
        lights: &lights,
        water: &[],
    };
    let target = renderer.create_target(device, 160, 100);
    if let Some(r) = request {
        renderer.begin_capture(r);
    }
    renderer.render(device, queue, &target, &frame);
    let capture = renderer.end_capture();
    (read_back(device, queue, &target), capture)
}

fn full(labels: Vec<DrawLabel>) -> Option<CaptureRequest> {
    Some(CaptureRequest {
        stop_after: None,
        labels,
    })
}

#[test]
fn no_events_without_a_capture() {
    let Some((device, queue)) = headless() else {
        eprintln!("no GPU adapter: skipped");
        return;
    };
    let mut r = Renderer::with_queue(&device, &queue);
    let (_, capture) = render(&device, &queue, &mut r, None);
    assert!(capture.is_none() && !r.capturing());
    // A finished capture leaves nothing installed: the next frame records nothing.
    let (_, first) = render(&device, &queue, &mut r, full(Vec::new()));
    assert!(!first.unwrap().events.is_empty());
    assert!(!r.capturing());
    let (_, after) = render(&device, &queue, &mut r, None);
    assert!(after.is_none());
}

#[test]
fn capture_lists_each_pass_of_a_small_scene() {
    let Some((device, queue)) = headless() else {
        eprintln!("no GPU adapter: skipped");
        return;
    };
    let mut r = Renderer::with_queue(&device, &queue);
    let (plain, _) = render(&device, &queue, &mut r, None);
    let (captured, capture) = render(&device, &queue, &mut r, full(labels()));
    let c = capture.expect("captured");
    assert_eq!(plain, captured, "capturing the whole frame changes nothing");
    assert_eq!(c.size, (160, 100));
    assert_eq!(c.stopped_at, None);

    let roots = c.roots();
    let order = ["Shadows", "Sky", "Opaque", "Transparent", "Post"];
    let positions: Vec<usize> = order
        .iter()
        .map(|p| {
            roots
                .iter()
                .position(|r| r == p)
                .unwrap_or_else(|| panic!("{p} in {roots:?}"))
        })
        .collect();
    assert!(positions.windows(2).all(|w| w[0] < w[1]), "{roots:?}");

    // Opaque: the plane, the three red cubes as one instanced call, the sphere.
    let opaque: Vec<&FrameEvent> = c.events.iter().filter(|e| e.root() == "Opaque").collect();
    assert_eq!(opaque.len(), 3, "{opaque:#?}");
    let cubes = opaque
        .iter()
        .find(|e| e.instances == 3)
        .expect("instanced cubes");
    assert_eq!(cubes.mesh.as_deref(), Some("Cube"));
    assert!(cubes.batch.as_ref().unwrap().starts_with("GPU instanced"));
    assert_eq!(cubes.indices, 36);
    let ball = opaque
        .iter()
        .find(|e| e.object.as_deref() == Some("Ball"))
        .unwrap();
    assert_eq!(ball.material.as_deref(), Some("Assets/Materials/Ball.mat"));
    assert!(ball.batch.as_ref().unwrap().starts_with("Not batched"));
    assert!(ball.shader.starts_with("Standard (Opaque)"));
    assert_eq!(ball.display, EventTarget::Hdr);
    assert_eq!(ball.target.width, 160);
    assert!(ball.properties.iter().any(|(k, _)| k == "Metallic"));

    // Transparent: the glass cube, blended.
    let glass: Vec<&FrameEvent> = c
        .events
        .iter()
        .filter(|e| e.root() == "Transparent")
        .collect();
    assert_eq!(glass.len(), 1);
    assert_eq!(glass[0].object.as_deref(), Some("Glass"));
    assert!(glass[0].blend.contains("SrcAlpha"));

    // Sky: the clear and the sky triangle.
    assert_eq!(c.count_in("Sky"), 2);
    // Shadows: every cascade starts with a clear and draws the opaque casters.
    let cascades: std::collections::BTreeSet<&String> = c
        .events
        .iter()
        .filter(|e| e.root() == "Shadows")
        .map(|e| &e.pass[1])
        .collect();
    assert!(!cascades.is_empty());
    for cascade in &cascades {
        let events: Vec<&FrameEvent> = c
            .events
            .iter()
            .filter(|e| e.pass.get(1) == Some(*cascade))
            .collect();
        assert_eq!(events[0].kind(), EventKind::Clear, "{cascade}");
        assert!(events.len() > 1, "{cascade} draws casters");
        assert!(matches!(
            events[1].display,
            EventTarget::Shadow { local: false, .. }
        ));
        assert!(
            events[1..]
                .iter()
                .all(|e| e.instances >= 1 && e.object.is_some())
        );
    }
    // Post: bloom chain then the composite, which writes the final image last.
    let post: Vec<&FrameEvent> = c.events.iter().filter(|e| e.root() == "Post").collect();
    assert!(post.len() >= 3);
    assert_eq!(post[0].name, "Bloom Prefilter");
    assert!(post.iter().any(|e| e.pass[1] == "Tonemap"));
    assert_eq!(c.events.last().unwrap().display, EventTarget::Output);
}

#[test]
fn replay_shows_the_frame_up_to_the_selected_event() {
    let Some((device, queue)) = headless() else {
        eprintln!("no GPU adapter: skipped");
        return;
    };
    let mut r = Renderer::with_queue(&device, &queue);
    let (final_image, capture) = render(&device, &queue, &mut r, full(labels()));
    let c = capture.unwrap();
    let first_opaque = c.events.iter().position(|e| e.root() == "Opaque").unwrap();
    let stop = |n: usize| {
        Some(CaptureRequest {
            stop_after: Some(n),
            labels: labels(),
        })
    };
    let mut r = Renderer::with_queue(&device, &queue);
    let (at_n, cap_n) = render(&device, &queue, &mut r, stop(first_opaque));
    let (at_next, _) = render(&device, &queue, &mut r, stop(first_opaque + 1));
    let cap_n = cap_n.unwrap();
    assert_eq!(cap_n.stopped_at, Some(first_opaque));
    assert_eq!(
        cap_n.events.len(),
        c.events.len(),
        "every event is still listed"
    );
    let differing =
        |a: &[u8], b: &[u8]| a.chunks(4).zip(b.chunks(4)).filter(|(x, y)| x != y).count();
    assert!(differing(&at_n, &at_next) > 50, "the next draw adds pixels");
    assert!(differing(&at_n, &final_image) > 50);
    // A shadow-map event shows the depth map (grey levels), not the scene.
    let shadow = c
        .events
        .iter()
        .position(|e| e.root() == "Shadows" && e.object.is_some());
    let (depth, _) = render(&device, &queue, &mut r, stop(shadow.unwrap()));
    assert!(differing(&depth, &final_image) > 1000);
    assert!(
        depth.chunks(4).all(|p| p[0] == p[1] && p[1] == p[2]),
        "greyscale"
    );
    // Stopping at the last event is the full frame.
    let (last, _) = render(&device, &queue, &mut r, stop(c.events.len() - 1));
    assert_eq!(differing(&last, &final_image), 0);
}
