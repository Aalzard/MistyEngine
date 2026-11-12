//! Light ▸ Glow: headless renders of a dark scene with a bright rectangle light and a bright
//! point light (with a halo). Spread keeps the glow's energy and cools its core.
//!
//! `MISTY_GLOW_SHOTS=<dir> cargo test --release -p misty_engine --lib glow_spread_shots --
//! --ignored --nocapture` saves the before/after shots (and the study of what glows).

use glam::{Mat4, Quat, Vec2, Vec3};

use super::GlowSource;
use crate::camera::{Camera, Projection};
use crate::render::gpu_tests::{headless, read_back};
use crate::render::{
    AreaShape, Environment, FogSettings, Frame, LightData, MaterialParams, MeshDraw, PostSettings,
    Renderer,
};
use crate::scene::post_process::TonemapMode;
use crate::scene::render_settings::RenderSettings;
use crate::scene::{
    FogOverride, FogQuality, LightGlow, LightHalo, LightType, PrimitiveMesh, Shadows,
};

const W: u32 = 640;
const H: u32 = 360;

fn camera() -> Camera {
    Camera {
        position: Vec3::new(0.0, 1.6, -8.0),
        rotation: Quat::IDENTITY,
        projection: Projection::Perspective { fov_y: 50.0 },
        near: 0.1,
        far: 100.0,
    }
}

fn floor() -> Vec<MeshDraw> {
    vec![MeshDraw {
        mesh: PrimitiveMesh::Plane.into(),
        model: Mat4::from_scale(Vec3::new(3.0, 1.0, 3.0)),
        material: MaterialParams {
            color: [0.4, 0.4, 0.4, 1.0],
            ..Default::default()
        },
        cast_shadows: false,
        receive_shadows: false,
        outline: false,
        sorting: Default::default(),
        lod: None,
    }]
}

const WARM: [f32; 3] = [1.0, 0.62, 0.32];

/// A 1.2 m × 0.8 m panel facing the camera with its emitter shown.
fn rect_light(intensity: f32) -> LightData {
    LightData {
        kind: LightType::Rectangle,
        position: Vec3::new(-1.6, 1.8, 0.0),
        direction: Vec3::NEG_Z,
        color: WARM,
        intensity,
        range: 15.0,
        spot_angle: 0.0,
        shadows: Shadows::None,
        shadow: Default::default(),
        volumetric: Default::default(),
        contact_shadows: false,
        area: AreaShape {
            size: Vec2::new(1.2, 0.8),
            // right × up = forward (-Z).
            right: Vec3::NEG_X,
            up: Vec3::Y,
            two_sided: false,
            show_emitter: true,
            glow: LightGlow::default(),
        },
    }
}

fn point_light(intensity: f32) -> LightData {
    LightData {
        kind: LightType::Point,
        position: Vec3::new(1.8, 1.8, 0.0),
        direction: Vec3::Z,
        area: AreaShape::default(),
        ..rect_light(intensity)
    }
}

fn halo_of(l: &LightData) -> GlowSource {
    GlowSource {
        kind: l.kind,
        position: l.position,
        direction: l.direction,
        spot_angle: l.spot_angle,
        color: l.color,
        intensity: l.intensity,
        glow: l.area.glow,
        halo: Some(LightHalo {
            enabled: true,
            size: 0.6,
            ..Default::default()
        }),
        flare: None,
    }
}

struct Shot {
    lights: Vec<LightData>,
    environment: Environment,
    meshes: Vec<MeshDraw>,
    post: PostSettings,
    camera: Camera,
}

impl Shot {
    fn new(intensity: f32) -> Self {
        let mut settings = RenderSettings::default();
        settings.environment_lighting.intensity = 0.0;
        settings.reflection_intensity = 0.0;
        let lights = vec![rect_light(intensity), point_light(intensity)];
        let mut environment = Environment::new(settings, &[]);
        environment.glows = vec![halo_of(&lights[1])];
        Self {
            lights,
            environment,
            meshes: floor(),
            post: PostSettings::default(),
            camera: camera(),
        }
    }

    /// Light ▸ Glow on both lights.
    fn glow(mut self, glow: LightGlow) -> Self {
        for l in &mut self.lights {
            l.area.glow = glow;
        }
        for g in &mut self.environment.glows {
            g.glow = glow;
        }
        self
    }

    fn fog(mut self) -> Self {
        self.post.fog = FogSettings::from_override(&FogOverride {
            active: true,
            max_fog_distance: 64.0,
            volumetric_distance: 64.0,
            attenuation_distance: 300.0,
            base_height: 50.0,
            maximum_height: 100.0,
            volumetric: true,
            quality: FogQuality::Medium,
            denoising: false,
            ..Default::default()
        });
        self
    }

    fn frame(&self) -> Frame<'_> {
        Frame {
            camera: &self.camera,
            grid: None,
            skybox: false,
            background: [0.0; 3],
            environment: &self.environment,
            post: self.post,
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

fn render(device: &wgpu::Device, queue: &wgpu::Queue, r: &mut Renderer, s: &Shot) -> Vec<u8> {
    let target = r.create_target(device, W, H);
    // A few frames: the fog's temporal history settles.
    for _ in 0..8 {
        r.render(device, queue, &target, &s.frame());
    }
    read_back(device, queue, &target)
}

fn save(image: &[u8], name: &str) {
    if let Some(dir) = std::env::var_os("MISTY_GLOW_SHOTS") {
        let path = std::path::Path::new(&dir).join(format!("{name}.png"));
        image::RgbaImage::from_raw(W, H, image.to_vec())
            .expect("size")
            .save(path)
            .expect("png");
    }
}

fn spread(spread: f32) -> LightGlow {
    LightGlow {
        spread,
        ..Default::default()
    }
}

#[test]
#[ignore = "shots: saves the glow images to MISTY_GLOW_SHOTS"]
fn glow_spread_shots() {
    let Some((device, queue)) = headless() else {
        eprintln!("no GPU adapter: skipped");
        return;
    };
    let mut renderer = Renderer::with_queue(&device, &queue);
    let no_bloom = |mut s: Shot| {
        s.post.bloom_intensity = 0.0;
        s
    };
    let no_halo = |mut s: Shot| {
        s.environment.glows.clear();
        s
    };
    let soft = LightGlow {
        spread: 0.5,
        soft_core: 1.0,
        ..Default::default()
    };
    let shots: Vec<(&str, Shot)> = vec![
        // What glows: bloom, the halo sprite, fog.
        ("study_a_default", Shot::new(60.0)),
        ("study_b_no_bloom", no_bloom(Shot::new(60.0))),
        ("study_c_no_halo", no_halo(Shot::new(60.0))),
        ("study_d_fog", Shot::new(60.0).fog()),
        ("study_e_fog_no_bloom", no_bloom(Shot::new(60.0).fog())),
        (
            "study_f_fog_no_bloom_no_halo",
            no_halo(no_bloom(Shot::new(60.0).fog())),
        ),
        // Before (default = spread 0) and after.
        ("i15_spread_0", Shot::new(15.0)),
        ("i15_spread_05", Shot::new(15.0).glow(spread(0.5))),
        ("i15_spread_1", Shot::new(15.0).glow(spread(1.0))),
        ("i15_spread_05_soft_core", Shot::new(15.0).glow(soft)),
        ("spread_0", Shot::new(60.0)),
        ("spread_05", Shot::new(60.0).glow(spread(0.5))),
        ("spread_1", Shot::new(60.0).glow(spread(1.0))),
        ("spread_05_soft_core", Shot::new(60.0).glow(soft)),
        ("fog_spread_0", Shot::new(60.0).fog()),
        ("fog_spread_05", Shot::new(60.0).fog().glow(spread(0.5))),
        ("fog_spread_1", Shot::new(60.0).fog().glow(spread(1.0))),
        (
            "glow_intensity_025",
            Shot::new(60.0).glow(LightGlow {
                intensity: 0.25,
                ..Default::default()
            }),
        ),
    ];
    for (name, s) in shots {
        let image = render(&device, &queue, &mut renderer, &s);
        save(&image, name);
    }
}

/// Linear light of an sRGB byte.
fn linear(v: u8) -> f64 {
    (v as f64 / 255.0).powf(2.2)
}

/// Sum of the linear red channel over the image, and the brightest pixel at `row` right of
/// the panel's edge (`from`..`to` in x).
fn energy(image: &[u8]) -> f64 {
    image.chunks(4).map(|p| linear(p[0])).sum()
}

/// Mean linear red over x `from..to` on row `row`.
fn mean_red(image: &[u8], row: u32, from: u32, to: u32) -> f64 {
    let sum: f64 = (from..to)
        .map(|x| linear(image[((row * W + x) * 4) as usize]))
        .sum();
    sum / (to - from) as f64
}

#[test]
fn glow_spread_keeps_the_energy_and_cools_the_core() {
    let Some((device, queue)) = headless() else {
        eprintln!("no GPU adapter: skipped");
        return;
    };
    let mut renderer = Renderer::with_queue(&device, &queue);
    // Only a dim white panel in the middle of a black screen, no tonemapping curve (linear
    // below 1), so the glow's energy is the image's sum minus the panel's.
    let shot = |spread: f32, bloom: bool| {
        let mut s = Shot::new(0.3);
        s.lights.truncate(1);
        s.lights[0].color = [1.0; 3];
        s.lights[0].position = Vec3::new(0.0, 1.6, 0.0);
        s.environment.glows.clear();
        s.meshes.clear();
        s.post.tonemap = TonemapMode::None;
        if !bloom {
            s.post.bloom_intensity = 0.0;
        }
        s.glow(LightGlow {
            spread,
            ..Default::default()
        })
    };
    let panel = render(&device, &queue, &mut renderer, &shot(0.0, false));
    let panel_energy = energy(&panel);
    // The panel ends about 29 px right of the centre: glow just outside its edge (the hot
    // core) and further out (the tail).
    let edge = W / 2 + 29;
    let near = |i: &[u8]| {
        mean_red(i, H / 2, edge + 3, edge + 8) - mean_red(&panel, H / 2, edge + 3, edge + 8)
    };
    let far = |i: &[u8]| mean_red(i, H / 2, edge + 80, edge + 100);
    let mut glows = Vec::new();
    for spread in [0.0, 0.5, 1.0] {
        let image = render(&device, &queue, &mut renderer, &shot(spread, true));
        save(&image, &format!("energy_spread_{}", (spread * 10.0) as u32));
        glows.push((
            spread,
            energy(&image) - panel_energy,
            near(&image),
            far(&image),
        ));
    }
    eprintln!("panel {panel_energy:.1}, (spread, glow energy, near edge, far): {glows:?}");
    let (_, base, base_near, base_far) = glows[0];
    assert!(
        base > panel_energy * 0.5,
        "bloom glows: {base} vs panel {panel_energy}"
    );
    for pair in glows.windows(2) {
        let ((_, _, near0, far0), (spread, e, near, far)) = (pair[0], pair[1]);
        // Spread 1 reaches past the screen's edges: a little of its energy is off screen.
        let low = if spread < 1.0 { 0.85 } else { 0.7 };
        assert!(
            e > base * low && e < base * 1.15,
            "spread {spread}: glow energy {e} vs {base} at spread 0"
        );
        // The core cools down and the tail grows as the spread goes up.
        assert!(
            near < near0 * 0.97,
            "spread {spread}: core {near} vs {near0}"
        );
        assert!(far > far0, "spread {spread}: tail {far} vs {far0}");
    }
    assert!(glows[2].2 < base_near * 0.7, "spread 1 core: {glows:?}");
    assert!(glows[2].3 > base_far * 2.0, "spread 1 tail: {glows:?}");
}

#[test]
fn defaults_draw_no_spread_halo() {
    let Some((device, queue)) = headless() else {
        eprintln!("no GPU adapter: skipped");
        return;
    };
    let mut renderer = Renderer::with_queue(&device, &queue);
    let a = render(&device, &queue, &mut renderer, &Shot::new(20.0));
    let b = render(
        &device,
        &queue,
        &mut renderer,
        &Shot::new(20.0).glow(LightGlow::default()),
    );
    assert_eq!(a, b);
    assert_eq!(renderer.glows.spreads, 0);
}

/// GPU cost of Glow Spread at 1080p: 8 rectangle lights with emitters and 8 haloed point
/// lights, spread 0 vs 1:
/// `cargo test --release -p misty_engine --lib glow_spread_benchmark -- --ignored --nocapture`
#[test]
#[ignore = "benchmark: GPU time of the Glow Spread halos at 1080p"]
fn glow_spread_benchmark() {
    let Some((device, queue)) = headless() else {
        eprintln!("no GPU adapter: skipped");
        return;
    };
    let mut renderer = Renderer::with_queue(&device, &queue);
    let target = renderer.create_target(&device, 1920, 1080);
    let scene = |amount: f32| {
        let mut s = Shot::new(15.0);
        let (rect, point) = (s.lights[0], s.lights[1]);
        s.lights.clear();
        s.environment.glows.clear();
        for i in 0..8 {
            let x = i as f32 - 3.5;
            let mut r = rect;
            r.position = Vec3::new(x, 2.4, 0.0);
            r.area.size = Vec2::new(0.6, 0.4);
            r.shadows = Shadows::None;
            let mut p = point;
            p.position = Vec3::new(x, 1.0, 0.0);
            s.environment.glows.push(halo_of(&p));
            s.lights.push(r);
            s.lights.push(p);
        }
        s.glow(spread(amount))
    };
    for (name, s) in [
        ("spread 0", scene(0.0)),
        ("spread 1", scene(1.0)),
        ("spread 0", scene(0.0)),
        ("spread 1", scene(1.0)),
    ] {
        let mut times = Vec::new();
        for i in 0..80 {
            renderer.render(&device, &queue, &target, &s.frame());
            let _ = device.poll(wgpu::PollType::wait_indefinitely());
            if i >= 20
                && let Some(ms) = renderer.gpu_ms()
            {
                times.push(ms);
            }
        }
        times.sort_by(f32::total_cmp);
        let median = times.get(times.len() / 2).copied().unwrap_or(f32::NAN);
        eprintln!("{name}: {median:.3} ms GPU (whole frame)");
    }
}
