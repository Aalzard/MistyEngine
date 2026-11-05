//! CyclesRoomDemo sample scene in `SampleProject`: a white Cornell-style room seen from its open
//! front, lit only by a square rectangle light in the ceiling (plus a matching emissive quad):
//! a tall pillar left of centre, a small cube front right and a thin canvas panel standing
//! against the back wall. It is the look-development target for "realtime images that look
//! like an offline path-traced render" (see `docs/pending/gfx-room.md`).
//!
//! Regenerate scene and materials with `cargo test -p misty_editor --release -- --ignored
//! write_cycles_room_demo`; `render_cycles_room` renders the comparison images into
//! `docs/screenshots/gfx/` (set `MISTY_GFX_STEP` to name the step).

use std::path::PathBuf;
use std::sync::Arc;

use glam::{Quat, Vec3};
use misty_engine::assets::Material;
use misty_engine::hecs::Entity;
use misty_engine::lightmap::{self, bake};
use misty_engine::render::MaterialParams;
use misty_engine::render::probes::{self, ProbeData};
use misty_engine::render_scene::{GameView, MaterialSource};
use misty_engine::scene::reflection_probe::ProbeClearFlags;
use misty_engine::scene::render_settings::{AmbientSource, SkyboxKind};
use misty_engine::scene::{
    Camera, Light, LightMode, MeshFilter, MeshRenderer, PostProcessVolume, PrimitiveMesh,
    ReflectionProbe, Scene, Shadows, TonemapLook, TonemapMode, Transform,
};

const SCENE: &str = "Assets/Scenes/CyclesRoomDemo.scene";
/// The same room with its bounce light baked into a lightmap (the light is Mixed).
const BAKED_SCENE: &str = "Assets/Scenes/CyclesRoomDemo Baked.scene";
const WALL: &str = "Assets/Materials/CyclesRoom/Wall.mat";
const FLOOR: &str = "Assets/Materials/CyclesRoom/Floor.mat";
const BOX: &str = "Assets/Materials/CyclesRoom/Box.mat";
const PANEL: &str = "Assets/Materials/CyclesRoom/Canvas.mat";
const EMITTER: &str = "Assets/Materials/CyclesRoom/Light Panel.mat";

/// Inner room size: width (x), height (y), depth (z). The open side faces −Z.
const ROOM: Vec3 = Vec3::new(2.4, 2.4, 2.4);
/// Side of the square ceiling light, metres.
const LIGHT_SIZE: f32 = 0.6;
/// Radiance of the ceiling light (light colour × intensity, and the emissive quad).
const LIGHT_RADIANCE: f32 = 16.0;

fn project() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../SampleProject")
}

fn materials() -> [(&'static str, Material); 5] {
    let m = |v: f32, smoothness| Material {
        color: [v, v, v, 1.0],
        metallic: 0.0,
        smoothness,
        ..Default::default()
    };
    [
        (WALL, m(0.8, 0.1)),
        (FLOOR, m(0.78, 0.7)),
        (BOX, m(0.8, 0.1)),
        (PANEL, m(0.82, 0.1)),
        (
            EMITTER,
            Material {
                emission_enabled: true,
                emission: [1.0, 1.0, 1.0],
                emission_intensity: LIGHT_RADIANCE,
                ..m(1.0, 0.0)
            },
        ),
    ]
}

/// The demo's materials without reading the project.
pub struct DemoMaterials;

impl MaterialSource for DemoMaterials {
    fn params(&self, rel: Option<&str>) -> MaterialParams {
        materials()
            .iter()
            .find(|(p, _)| Some(*p) == rel)
            .map(|(_, m)| m.params())
            .unwrap_or_default()
    }
}

fn placed(position: Vec3, rotation: Quat, scale: Vec3) -> Transform {
    Transform {
        position,
        rotation,
        scale,
        ..Default::default()
    }
}

fn object(
    s: &mut Scene,
    name: &str,
    parent: Option<Entity>,
    kind: PrimitiveMesh,
    t: Transform,
    mat: &str,
) -> Entity {
    let e = s.spawn(name, parent);
    s.set_transform(e, t);
    s.add(e, MeshFilter::primitive(kind));
    s.add(
        e,
        MeshRenderer {
            material: Some(mat.into()),
            ..Default::default()
        },
    );
    s.game_object_mut(e).is_static = true;
    e
}

/// Floor, ceiling and three walls from 10 cm thick boxes (closed, so shadow maps don't leak),
/// then the pillar, the cube and the canvas.
fn room(s: &mut Scene) {
    let room = s.spawn("Room", None);
    s.game_object_mut(room).is_static = true;
    let (w, h, d) = (ROOM.x, ROOM.y, ROOM.z);
    let t = 0.1;
    let parts = [
        (
            "Floor",
            FLOOR,
            Vec3::new(0.0, -t * 0.5, d * 0.5),
            Vec3::new(w + 2.0 * t, t, d),
        ),
        (
            "Ceiling",
            WALL,
            Vec3::new(0.0, h + t * 0.5, d * 0.5),
            Vec3::new(w + 2.0 * t, t, d),
        ),
        (
            "Back Wall",
            WALL,
            Vec3::new(0.0, h * 0.5, d + t * 0.5),
            Vec3::new(w + 2.0 * t, h + 2.0 * t, t),
        ),
        (
            "Left Wall",
            WALL,
            Vec3::new(-(w + t) * 0.5, h * 0.5, d * 0.5),
            Vec3::new(t, h, d),
        ),
        (
            "Right Wall",
            WALL,
            Vec3::new((w + t) * 0.5, h * 0.5, d * 0.5),
            Vec3::new(t, h, d),
        ),
    ];
    for (name, mat, at, size) in parts {
        object(
            s,
            name,
            Some(room),
            PrimitiveMesh::Cube,
            placed(at, Quat::IDENTITY, size),
            mat,
        );
    }
    let turn = |deg: f32| Quat::from_rotation_y(deg.to_radians());
    object(
        s,
        "Pillar",
        Some(room),
        PrimitiveMesh::Cube,
        placed(
            Vec3::new(-0.45, 0.7, 1.45),
            turn(20.0),
            Vec3::new(0.6, 1.4, 0.6),
        ),
        BOX,
    );
    object(
        s,
        "Cube",
        Some(room),
        PrimitiveMesh::Cube,
        placed(Vec3::new(0.5, 0.35, 0.75), turn(-18.0), Vec3::splat(0.7)),
        BOX,
    );
    // A thin canvas standing on the floor, 8 cm in front of the back wall.
    object(
        s,
        "Canvas",
        Some(room),
        PrimitiveMesh::Cube,
        placed(
            Vec3::new(0.35, 0.6, d - 0.1),
            Quat::IDENTITY,
            Vec3::new(1.2, 1.2, 0.04),
        ),
        PANEL,
    );
}

/// The ceiling light: a Rectangle light shining down and an emissive quad showing it (the quad
/// neither casts shadows nor joins the bake, so the light is counted once).
fn ceiling_light(s: &mut Scene, mode: LightMode) {
    let y = ROOM.y - 0.005;
    let at = Vec3::new(0.0, y, ROOM.z * 0.5);
    let e = s.spawn("Ceiling Light", None);
    s.set_transform(
        e,
        placed(
            at - Vec3::Y * 0.01,
            Quat::from_rotation_x(std::f32::consts::FRAC_PI_2),
            Vec3::ONE,
        ),
    );
    s.add(
        e,
        Light {
            color: [1.0, 1.0, 1.0],
            intensity: LIGHT_RADIANCE,
            range: 8.0,
            width: LIGHT_SIZE,
            height: LIGHT_SIZE,
            shadows: Shadows::Soft,
            show_emitter: false,
            mode,
            ..Light::rectangle()
        },
    );
    // The Quad faces −Z; turned so it faces down.
    let q = s.spawn("Light Panel", None);
    s.set_transform(
        q,
        placed(
            at,
            Quat::from_rotation_x(-std::f32::consts::FRAC_PI_2),
            Vec3::new(LIGHT_SIZE, LIGHT_SIZE, 1.0),
        ),
    );
    s.add(q, MeshFilter::primitive(PrimitiveMesh::Quad));
    s.add(
        q,
        MeshRenderer {
            material: Some(EMITTER.into()),
            cast_shadows: false,
            contribute_gi: false,
            ..Default::default()
        },
    );
}

/// Light bounces baked into the room's reflection probe.
const PROBE_BOUNCES: usize = 4;

/// Box-projected reflection probes: their SH is the room's bounce light (diffuse ambient), their
/// cubemaps the floor's glossy reflection. One fills the room; a second one near the open front
/// (more important, blended in) sees mostly the dark opening, so surfaces facing the camera
/// there get the dimmer bounce light they get in a path-traced render.
fn room_probe(s: &mut Scene, scene_path: &str) {
    let front = 0.9;
    let probes = [
        (
            "Room Probe",
            Vec3::new(0.0, ROOM.y * 0.5, ROOM.z * 0.5),
            // A little larger than the room so the walls themselves get the full weight.
            ROOM + Vec3::splat(0.2),
            Vec3::ZERO,
            1,
            0.05,
            true,
        ),
        (
            "Front Probe",
            Vec3::new(0.0, ROOM.y * 0.5, front * 0.5),
            // Reaching well past the floor, walls, ceiling and opening, so it only blends out
            // towards the back of the room (not along the floor, which showed as a seam).
            Vec3::new(ROOM.x + 1.6, ROOM.y + 1.6, front + 0.8),
            Vec3::new(0.0, 0.0, -0.4),
            2,
            0.35,
            // Plain lookups: its box is not the room.
            false,
        ),
    ];
    for (i, (name, at, size, center, importance, blend, box_projection)) in
        probes.into_iter().enumerate()
    {
        let e = s.spawn(name, None);
        s.set_transform(e, Transform::from_position(at));
        s.add(
            e,
            ReflectionProbe {
                box_projection,
                size,
                center,
                importance,
                blend_distance: blend,
                resolution: 128,
                near_clip: 0.05,
                far_clip: 40.0,
                clear_flags: ProbeClearFlags::SolidColor,
                background: [0.0, 0.0, 0.0],
                baked: Some(probes::asset_path_for(scene_path, i)),
                ..Default::default()
            },
        );
    }
}

/// Bake the room probe with `PROBE_BOUNCES` passes (each re-captures with the previous result
/// lighting the room). The emissive light panel is left out of the captures: its light is
/// already the Rectangle light's direct light, so the probe only holds bounce light. With
/// `maps`, lightmapped surfaces show their baked bounce light in the captures.
pub fn bake_probe(
    gpu: &(misty_engine::wgpu::Device, misty_engine::wgpu::Queue),
    scene: &Scene,
    maps: &lightmap::LightmapSet,
) -> Vec<Arc<ProbeData>> {
    use misty_engine::render::probes::capture::{self, CaptureInputs, CaptureSettings};
    let (device, queue) = gpu;
    let mut renderer = misty_engine::render::Renderer::new(device);
    let list = probes::scene_probe_list(scene);
    let mut inputs = CaptureInputs::from_scene(scene, &DemoMaterials);
    inputs.environment.lightmaps = maps.clone();
    inputs
        .meshes
        .retain(|d| d.material.emission_intensity <= 0.0 || d.material.emission == [0.0; 3]);
    let mut data: Vec<Arc<ProbeData>> = Vec::new();
    for pass in 0..PROBE_BOUNCES {
        if pass > 0 {
            inputs.environment.probes = capture::probe_set(scene, &list, &data);
        }
        data = list
            .iter()
            .map(|(e, p)| {
                let s = CaptureSettings::of(p, scene.world_position(*e));
                let cap = capture::ProbeCapture::new(&renderer, device, s.size);
                let cube = cap.cube(&mut renderer, device, queue, &inputs, &s);
                Arc::new(ProbeData::from_capture(&cube, capture::BAKE_SAMPLES))
            })
            .collect();
    }
    data
}

/// `view` with the room probe lit by `data`.
fn with_probes(view: &mut GameView, scene: &Scene, data: &[Arc<ProbeData>]) {
    let list = probes::scene_probe_list(scene);
    view.environment.probes = misty_engine::render::probes::capture::probe_set(scene, &list, data);
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Variant {
    /// Realtime light; bounce light from the baked room probe.
    Realtime,
    /// Mixed light (realtime direct light and shadows); bounce light path-traced into a
    /// lightmap.
    Baked,
}

/// The realtime demo scene.
pub fn demo_scene() -> Scene {
    scene(Variant::Realtime)
}

/// The baked demo scene as committed: renderers pointed at its lightmap (layout only).
pub fn baked_scene() -> Scene {
    let mut s = scene(Variant::Baked);
    let input = bake::input::gather(&s, &DemoMaterials);
    let atlas = bake::layout(&input);
    let results = bake::receiver_results(&input, &atlas);
    let paths = (0..atlas.count)
        .map(|i| lightmap::asset_path_for(BAKED_SCENE, i))
        .collect();
    crate::lightmap_tools::assign(&mut s, &results, paths);
    s
}

/// The room: camera centred in the opening at mid height, 42° vertical field of view.
fn scene(variant: Variant) -> Scene {
    let (name, path, mode) = match variant {
        Variant::Realtime => ("CyclesRoomDemo", SCENE, LightMode::Realtime),
        Variant::Baked => ("CyclesRoomDemo Baked", BAKED_SCENE, LightMode::Mixed),
    };
    let mut s = Scene::default_3d(name);
    if let Some(cam) = s.find_by_name("Main Camera") {
        s.set_transform(
            cam,
            Transform::from_position(Vec3::new(0.0, ROOM.y * 0.5, -3.2)),
        );
        if let Ok(mut c) = s.world.get::<&mut Camera>(cam) {
            c.fov = 42.0;
            c.near = 0.1;
            c.far = 50.0;
        }
    }
    if let Some(sun) = s.find_by_name("Directional Light") {
        s.despawn(sun);
    }
    // A closed room: no sky light or sky reflections leak in; the ceiling light is the only
    // source.
    let rs = &mut s.render_settings;
    rs.skybox.kind = SkyboxKind::SolidColor;
    rs.skybox.color = [0.0, 0.0, 0.0];
    rs.environment_lighting.source = AmbientSource::Color;
    rs.environment_lighting.sky_color = [0.0, 0.0, 0.0];
    rs.environment_lighting.intensity = 0.0;
    rs.reflection_intensity = 1.0;
    let lm = &mut rs.lightmapping;
    lm.resolution = 32.0;
    lm.max_size = 512;
    lm.samples = 1024;
    lm.bounces = 4;
    room(&mut s);
    ceiling_light(&mut s, mode);
    room_probe(&mut s, path);
    let fx = s.spawn("Global Volume", None);
    s.add(fx, volume(variant));
    s
}

/// Post-processing of the demo (see the step log for why each value). The baked variant is
/// the reference: its path-traced lightmap already holds the room's occlusion, so it keeps
/// the lighter AO and the exposure the room was first tuned with.
fn volume(variant: Variant) -> PostProcessVolume {
    let mut v = PostProcessVolume::default();
    v.tonemapping.active = true;
    v.tonemapping.mode = TonemapMode::Agx;
    v.bloom.active = true;
    v.bloom.intensity = 0.15;
    v.bloom.threshold = 1.0;
    v.bloom.scatter = 0.7;
    v.color_adjustments.active = true;
    // 0.08 stop less than without SSGI: its bounce off the walls and floor lifts the room
    // a little (see docs/pending/gfx-polish.md).
    v.color_adjustments.post_exposure = 0.92;
    let ao = &mut v.ambient_occlusion;
    ao.active = true;
    // Darker seams and wider contact shade under the boxes (also on the direct light a
    // little), as in the path-traced room.
    ao.intensity = 3.5;
    ao.radius = 1.6;
    ao.direct_lighting_strength = 0.6;
    if variant == Variant::Baked {
        v.color_adjustments.post_exposure = 1.0;
        ao.intensity = 2.0;
        ao.radius = 1.2;
        ao.direct_lighting_strength = 0.0;
    }
    ao.step_count = 10;
    ao.full_resolution = true;
    ao.temporal = true;
    // Per-surface bounce light (the probes' diffuse fills the rays that leave the screen) and
    // the glossy floor's reflection. Below 1: rays that leave the screen towards the dark
    // opening still get the probes' room-average diffuse, which brightens surfaces facing
    // the camera; a weaker bounce keeps the pillar and cube fronts nearer the path tracer.
    v.screen_space_gi.active = true;
    v.screen_space_gi.intensity = 0.6;
    v.screen_space_reflections.active = true;
    v
}

// ---- comparison renders -------------------------------------------------------------------

/// Side of the square comparison renders.
pub const SIZE: u32 = 1024;

type Gpu = (misty_engine::wgpu::Device, misty_engine::wgpu::Queue);

fn screenshots() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../docs/screenshots/gfx")
}

/// Render `view` at `SIZE`² for `frames` frames (temporal effects settle) as RGBA8.
fn render_rgba(gpu: &Gpu, view: &GameView, frames: u32) -> Vec<u8> {
    let (device, queue) = gpu;
    let mut renderer = misty_engine::render::Renderer::new(device);
    // `MISTY_GFX_OFF=ssgi,ssr,ao,mask,soft`: turn effects off to isolate one (debugging).
    let off = std::env::var("MISTY_GFX_OFF").unwrap_or_default();
    let off = |name: &str| off.split(',').any(|o| o.trim() == name);
    let mut quality = renderer.quality().clone();
    if off("mask") {
        quality.shadow_temporal = false;
        quality.shadow_denoise = false;
    }
    if off("soft") {
        quality.soft_shadows = misty_engine::quality::Tier::Off;
    }
    renderer.set_quality(device, &quality);
    let target = renderer.create_target(device, SIZE, SIZE);
    let mut frame = view.frame();
    frame.post.ssgi.enabled &= !off("ssgi");
    frame.post.ssr.enabled &= !off("ssr");
    frame.post.ao.enabled &= !off("ao");
    let unlit: Vec<_>;
    if off("panel") {
        unlit = frame
            .meshes
            .iter()
            .filter(|m| m.material.emission_intensity <= 0.0)
            .cloned()
            .collect();
        frame.meshes = &unlit;
    }
    for _ in 0..frames.max(1) {
        renderer.render(device, queue, &target, &frame);
    }
    crate::project_thumbnail::read_pixels(device, queue, &target)
}

/// Mean sRGB brightness (0..255) of a few 21×21 patches, printed for the step log.
fn patches(label: &str, rgba: &[u8]) {
    let spots = [
        ("ceiling", 512, 60),
        ("back wall", 512, 350),
        ("left wall", 100, 400),
        ("floor front", 512, 985),
        ("pillar front", 340, 650),
        ("cube front", 690, 820),
        ("floor right of cube", 900, 930),
    ];
    let mut line = format!("{label}:");
    for (name, x, y) in spots {
        let mut sum = 0.0;
        for dy in -10..=10i32 {
            for dx in -10..=10i32 {
                let i = (((y + dy) as u32 * SIZE + (x + dx) as u32) * 4) as usize;
                sum += rgba[i..i + 3].iter().map(|&c| c as f32).sum::<f32>() / 3.0;
            }
        }
        line += &format!(" {name} {:.0}", sum / 441.0);
    }
    eprintln!("{line}");
}

fn save_png(path: &std::path::Path, rgba: Vec<u8>) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    misty_engine::image::RgbaImage::from_raw(SIZE, SIZE, rgba)
        .unwrap()
        .save(path)
        .unwrap();
}

fn lightmap_set(maps: &[lightmap::LightmapData]) -> lightmap::LightmapSet {
    lightmap::LightmapSet {
        maps: maps
            .iter()
            .map(|d| Some(lightmap::LightmapHandle(Arc::new(d.clone()))))
            .collect(),
        debug: lightmap::LightmapDebug::Off,
    }
}

/// Path-trace the baked variant's lightmaps (CPU; a few seconds in release).
fn bake_lightmaps() -> Vec<lightmap::LightmapData> {
    let input = bake::input::gather(&scene(Variant::Baked), &DemoMaterials);
    bake::bake(&input, &bake::Progress::default()).unwrap().maps
}

/// A view of `scene` with its probe (and lightmaps) as given, the way the editor shows it.
fn view_of(scene: &Scene, maps: &lightmap::LightmapSet, probe: &[Arc<ProbeData>]) -> GameView {
    let mut view = GameView::new(scene, &DemoMaterials).unwrap();
    view.environment.lightmaps = maps.clone();
    with_probes(&mut view, scene, probe);
    view
}

#[test]
#[ignore = "writes into SampleProject and bakes (GPU probe, CPU lightmap); use --release"]
fn write_cycles_room_demo() {
    let root = project();
    for (path, m) in materials() {
        std::fs::create_dir_all(root.join(path).parent().unwrap()).unwrap();
        m.save(&root.join(path)).unwrap();
    }
    let gpu = crate::sample_render::headless().expect("a GPU adapter");
    let realtime = demo_scene();
    realtime.save(&root.join(SCENE)).unwrap();
    let probe = bake_probe(&gpu, &realtime, &Default::default());
    for (i, p) in probe.iter().enumerate() {
        p.save(&root.join(probes::asset_path_for(SCENE, i)))
            .unwrap();
    }
    let baked = baked_scene();
    baked.save(&root.join(BAKED_SCENE)).unwrap();
    let maps = bake_lightmaps();
    for (i, map) in maps.iter().enumerate() {
        map.save(&root.join(lightmap::asset_path_for(BAKED_SCENE, i)))
            .unwrap();
    }
    let probe = bake_probe(&gpu, &baked, &lightmap_set(&maps));
    for (i, p) in probe.iter().enumerate() {
        p.save(&root.join(probes::asset_path_for(BAKED_SCENE, i)))
            .unwrap();
    }
    // GUIDs for the new files and folders (kept when they already exist).
    let mut written: Vec<String> = materials().iter().map(|(p, _)| p.to_string()).collect();
    written.push("Assets/Materials/CyclesRoom".into());
    for scene in [SCENE, BAKED_SCENE] {
        written.push(scene.into());
        written.push(scene.strip_suffix(".scene").unwrap().into());
        written.extend((0..2).map(|i| probes::asset_path_for(scene, i)));
    }
    written.extend((0..maps.len()).map(|i| lightmap::asset_path_for(BAKED_SCENE, i)));
    for rel in written {
        let path = root.join(&rel);
        if misty_engine::asset_db::meta::read_guid(&path).is_none() {
            misty_engine::asset_db::meta::write_guid(&path, misty_engine::asset_db::Guid::new())
                .unwrap();
        }
    }
}

/// Renders the comparison images `docs/screenshots/gfx/cycles_room_<MISTY_GFX_STEP>.png`
/// (realtime) and, with `MISTY_GFX_BAKED=1`, `..._baked.png` (baked indirect light).
#[test]
#[ignore = "writes comparison renders into docs/screenshots/gfx; run on purpose"]
fn render_cycles_room() {
    let gpu = crate::sample_render::headless().expect("a GPU adapter");
    let step = std::env::var("MISTY_GFX_STEP").unwrap_or_else(|_| "latest".into());
    let scene = demo_scene();
    let probe = bake_probe(&gpu, &scene, &Default::default());
    let view = view_of(&scene, &Default::default(), &probe);
    let rgba = render_rgba(&gpu, &view, 24);
    patches("realtime", &rgba);
    save_png(&screenshots().join(format!("cycles_room_{step}.png")), rgba);
    if std::env::var("MISTY_GFX_BAKED").is_ok_and(|v| v == "1") {
        let scene = baked_scene();
        let maps = lightmap_set(&bake_lightmaps());
        let probe = bake_probe(&gpu, &scene, &maps);
        let view = view_of(&scene, &maps, &probe);
        let rgba = render_rgba(&gpu, &view, 24);
        patches("baked", &rgba);
        save_png(
            &screenshots().join(format!("cycles_room_{step}_baked.png")),
            rgba,
        );
    }
}

/// Renders the demo with every tonemapper: `docs/screenshots/gfx/cycles_room_tonemap_<mode>.png`.
#[test]
#[ignore = "writes comparison renders into docs/screenshots/gfx; run on purpose"]
fn render_cycles_room_tonemappers() {
    let gpu = crate::sample_render::headless().expect("a GPU adapter");
    let scene = demo_scene();
    let probe = bake_probe(&gpu, &scene, &Default::default());
    let mut view = view_of(&scene, &Default::default(), &probe);
    let modes = [
        ("none", TonemapMode::None, TonemapLook::Base),
        ("aces", TonemapMode::Aces, TonemapLook::Base),
        ("agx", TonemapMode::Agx, TonemapLook::Base),
        ("agx_punchy", TonemapMode::Agx, TonemapLook::Punchy),
        ("pbr_neutral", TonemapMode::PbrNeutral, TonemapLook::Base),
    ];
    for (name, mode, look) in modes {
        view.post.tonemap = mode;
        view.post.tonemap_look = look;
        let rgba = render_rgba(&gpu, &view, 24);
        save_png(
            &screenshots().join(format!("cycles_room_tonemap_{name}.png")),
            rgba,
        );
    }
}

#[test]
fn cycles_room_demo_matches_its_generator() {
    let root = project();
    for (path, m) in materials() {
        assert_eq!(Material::load(&root.join(path)).unwrap(), m, "{path}");
    }
    for (path, generated) in [(SCENE, demo_scene()), (BAKED_SCENE, baked_scene())] {
        let text = std::fs::read_to_string(root.join(path)).unwrap();
        assert!(
            text.replace("\r\n", "\n") == generated.to_json(),
            "{path}: regenerate with write_cycles_room_demo"
        );
        let (scene, report) = Scene::from_json("CyclesRoom", &text).unwrap();
        assert!(report.warnings.is_empty(), "{:?}", report.warnings);
        // The ceiling light lights the room in realtime (Mixed: its bounces are baked).
        let lights = misty_engine::render_scene::lights(&scene);
        assert_eq!(lights.len(), 1);
        assert_eq!(lights[0].kind, misty_engine::scene::LightType::Rectangle);
        // The room probes are baked and small.
        for i in 0..2 {
            let probe = root.join(probes::asset_path_for(path, i));
            assert!(ProbeData::load(&probe).is_ok(), "{path}");
            assert!(std::fs::metadata(&probe).unwrap().len() < 600_000);
        }
    }
    // The baked variant: one small lightmap used by every static renderer.
    let (scene, _) = Scene::load(&root.join(BAKED_SCENE)).unwrap();
    let paths = &scene.render_settings.lightmapping.lightmaps;
    assert_eq!(paths.len(), 1);
    let map = lightmap::LightmapData::load(&root.join(&paths[0])).unwrap();
    assert!(map.width <= 512, "{}", map.width);
    assert!(std::fs::metadata(root.join(&paths[0])).unwrap().len() < 1_000_000);
    for name in ["Floor", "Back Wall", "Pillar", "Canvas"] {
        let e = scene.find_by_name(name).unwrap();
        assert_eq!(
            scene.world.get::<&MeshRenderer>(e).unwrap().lightmap_index,
            0
        );
    }
}

/// The PP Ready template's sunset with each tonemapper (for the AgX-as-default decision):
/// `docs/screenshots/gfx/template_pp_ready_<mode>.png`.
#[test]
#[ignore = "writes comparison renders into docs/screenshots/gfx; run on purpose"]
fn render_template_tonemappers() {
    use crate::sample_render::{HEIGHT, WIDTH};
    use crate::templates::{SCENE as MAIN, Template, create};
    let gpu = crate::sample_render::headless().expect("a GPU adapter");
    let root = std::env::temp_dir().join(format!("misty_gfx_room_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    create(&root, Template::PpReady).unwrap();
    misty_engine::assets::set_project_root(&root);
    let (scene, _) = Scene::load(&root.join(MAIN)).unwrap();
    let assets = misty_engine::assets::AssetCache::open(&root);
    let mut view = GameView::new(&scene, &assets).unwrap();
    for (name, mode, look) in [
        ("aces", TonemapMode::Aces, TonemapLook::Base),
        ("agx", TonemapMode::Agx, TonemapLook::Base),
        ("agx_punchy", TonemapMode::Agx, TonemapLook::Punchy),
        ("pbr_neutral", TonemapMode::PbrNeutral, TonemapLook::Base),
    ] {
        view.post.tonemap = mode;
        view.post.tonemap_look = look;
        let img = crate::sample_render::render(&gpu, &view.frame(), 12);
        misty_engine::image::RgbaImage::from_raw(WIDTH, HEIGHT, img.pixels)
            .unwrap()
            .save(screenshots().join(format!("template_pp_ready_{name}.png")))
            .unwrap();
    }
    let _ = std::fs::remove_dir_all(&root);
}

/// Luminance banding of a flat region of a `SIZE`² RGBA8 image: RMS of the luminance
/// band-passed by a difference of box means (5×5 minus 31×31), i.e. the energy at roughly
/// 10–60 px wavelengths. Pixel noise averages out in the small box, the room's smooth
/// gradients in the difference.
fn banding(rgba: &[u8], (x0, y0, x1, y1): (i32, i32, i32, i32)) -> f32 {
    let n = SIZE as usize;
    // Summed-area table of the luminance (one row and column of zeros in front).
    let mut sat = vec![0.0f64; (n + 1) * (n + 1)];
    for y in 0..n {
        for x in 0..n {
            let p = &rgba[(y * n + x) * 4..];
            let l = 0.2126 * p[0] as f64 + 0.7152 * p[1] as f64 + 0.0722 * p[2] as f64;
            sat[(y + 1) * (n + 1) + x + 1] =
                l + sat[y * (n + 1) + x + 1] + sat[(y + 1) * (n + 1) + x] - sat[y * (n + 1) + x];
        }
    }
    let mean = |x: i32, y: i32, r: i32| {
        let at = |x: i32, y: i32| sat[y as usize * (n + 1) + x as usize];
        let (a, b) = ((x - r, y - r), (x + r + 1, y + r + 1));
        (at(b.0, b.1) - at(a.0, b.1) - at(b.0, a.1) + at(a.0, a.1)) / ((2 * r + 1).pow(2) as f64)
    };
    let mut sum = 0.0;
    let mut count = 0.0;
    for y in y0..=y1 {
        for x in x0..=x1 {
            let d = mean(x, y, 2) - mean(x, y, 15);
            sum += d * d;
            count += 1.0;
        }
    }
    (sum / count).sqrt() as f32
}

/// Regression: half-resolution SSGI took its interleaved-gradient noise at full-pixel
/// coordinates (every other pixel), which aliases into regular diagonal bands (~20–40 px
/// apart, ~30°) that 24 frames of accumulation and the à-trous filter left on every wall of
/// this room. With SSGI on, the back wall must stay about as smooth as without it. The
/// quality level is set explicitly so temporal anti-aliasing (which would only blur the
/// bands) stays out of it.
#[test]
fn cycles_room_ssgi_leaves_no_bands_on_the_walls() {
    let Some(gpu) = crate::sample_render::headless() else {
        eprintln!("no GPU adapter: skipped");
        return;
    };
    let root = project();
    let (scene, _) = Scene::load(&root.join(SCENE)).unwrap();
    let probe: Vec<_> = (0..2)
        .map(|i| Arc::new(ProbeData::load(&root.join(probes::asset_path_for(SCENE, i))).unwrap()))
        .collect();
    let view = view_of(&scene, &Default::default(), &probe);
    // The light panel sends its glow through SSGI here (Contribute GI on): rays that find it
    // bring back far more than the others, the hardest case for the ray pattern.
    let meshes: Vec<_> = view
        .frame()
        .meshes
        .iter()
        .cloned()
        .map(|mut m| {
            m.material.contribute_gi = true;
            m
        })
        .collect();
    let render = |ssgi: bool| {
        let (device, queue) = &gpu;
        let mut renderer = misty_engine::render::Renderer::new(device);
        renderer.set_quality(device, &misty_engine::quality::QualityLevel::high());
        let target = renderer.create_target(device, SIZE, SIZE);
        let mut frame = view.frame();
        frame.meshes = &meshes;
        frame.post.ssgi = misty_engine::render::SsgiSettings {
            enabled: ssgi,
            full_resolution: false,
            temporal: true,
            denoise: true,
            intensity: 1.0,
            ..frame.post.ssgi
        };
        frame.post.ssr.enabled = false;
        for _ in 0..24 {
            renderer.render(device, queue, &target, &frame);
        }
        crate::project_thumbnail::read_pixels(device, queue, &target)
    };
    // The back wall above the boxes and the left wall (clear of edges by the 15 px box).
    let back = (270, 250, 760, 420);
    let left = (40, 200, 190, 600);
    let (off, on) = (render(false), render(true));
    for (name, region) in [("back wall", back), ("left wall", left)] {
        let (b_off, b_on) = (banding(&off, region), banding(&on, region));
        eprintln!("{name} banding: SSGI off {b_off:.2}, on {b_on:.2}");
        // Before the fix: back wall 1.5, left wall 1.6 (SSGI off: 0.2, 0.5; after: 0.4, 0.4).
        assert!(
            b_on < b_off + 0.6,
            "{name}: SSGI bands the wall ({b_off} → {b_on})"
        );
    }
}

/// On a GPU: both committed variants render with their committed probes / lightmap, the room
/// is evenly lit without crushed blacks, and realtime and baked agree on the walls.
#[test]
fn cycles_room_renders_from_its_baked_data() {
    let Some(gpu) = crate::sample_render::headless() else {
        eprintln!("no GPU adapter: skipped");
        return;
    };
    let root = project();
    let load = |path: &str| {
        let (scene, _) = Scene::load(&root.join(path)).unwrap();
        let probe: Vec<_> = (0..2)
            .map(|i| {
                Arc::new(ProbeData::load(&root.join(probes::asset_path_for(path, i))).unwrap())
            })
            .collect();
        let maps = lightmap::LightmapSet {
            maps: scene
                .render_settings
                .lightmapping
                .lightmaps
                .iter()
                .map(|rel| {
                    lightmap::LightmapData::load(&root.join(rel))
                        .ok()
                        .map(|d| lightmap::LightmapHandle(Arc::new(d)))
                })
                .collect(),
            debug: lightmap::LightmapDebug::Off,
        };
        let view = view_of(&scene, &maps, &probe);
        crate::sample_render::render(&gpu, &view.frame(), 8)
    };
    let realtime = load(SCENE);
    let baked = load(BAKED_SCENE);
    let cam = demo_scene();
    let (camera, _) = misty_engine::render_scene::game_camera(&cam).unwrap();
    let back_wall = Vec3::new(-0.9, 1.8, ROOM.z);
    let left_wall = Vec3::new(-ROOM.x * 0.5, 1.6, 1.0);
    let shadow = Vec3::new(1.0, 0.0, 1.0);
    for (name, img) in [("realtime", &realtime), ("baked", &baked)] {
        let (b, l, s) = (
            img.brightness(&camera, back_wall),
            img.brightness(&camera, left_wall),
            img.brightness(&camera, shadow),
        );
        eprintln!("{name}: back wall {b:.0}, left wall {l:.0}, floor in shadow {s:.0}");
        assert!(b > 120.0 && l > 120.0, "{name}: bounce-lit walls");
        assert!(s > 25.0, "{name}: no crushed blacks in the shadows ({s})");
        assert!(s < b, "{name}: the shadow is darker than the wall");
    }
    let diff = realtime.brightness(&camera, back_wall) - baked.brightness(&camera, back_wall);
    assert!(diff.abs() < 25.0, "realtime and baked walls agree ({diff})");
}
