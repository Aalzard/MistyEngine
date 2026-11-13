//! Material previews rendered by the engine renderer: the sphere at the bottom of the material
//! Inspector and the Project window's material thumbnails (cached per file version).
//! Panels ask for previews while drawing; [`render`] makes the missing ones after the UI pass,
//! so a new preview shows up one frame later.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use misty_engine::assets::Material;
use misty_engine::camera::{Camera, Projection};
use misty_engine::glam::{Mat4, Quat, Vec3};
use misty_engine::render::{Environment, Frame, LightData, MeshDraw, RenderTarget};
use misty_engine::scene::{LightType, PrimitiveMesh, Shadows};
use misty_engine::wgpu;

use crate::viewport::Gpu;

/// Thumbnail size in pixels.
pub const THUMB_SIZE: u32 = 128;
/// Thumbnails rendered per frame at most (the rest wait for the next frames).
const THUMBS_PER_FRAME: usize = 6;
/// Main-thread time the thumbnails may take in one frame; the first one always renders.
const THUMB_BUDGET: std::time::Duration = std::time::Duration::from_millis(6);
/// A thumbnail no panel asked for in this many frames is dropped with its GPU texture (a
/// long session scrolling through a big project kept every one it ever showed, 64 KB each,
/// and those of deleted and renamed files).
const THUMB_KEEP_FRAMES: u64 = 1800;

/// Preview shape in the Inspector.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum PreviewShape {
    #[default]
    Sphere,
    Cube,
    /// A ground plane tilted towards the camera (the Shader Graph preview).
    Plane,
}

struct Thumb {
    modified: Option<SystemTime>,
    material: Option<Material>,
    texture: Option<(wgpu::Texture, egui::TextureId)>,
    rendered: bool,
    /// [`MaterialPreviews::frame`] when a panel last asked for it.
    used: u64,
}

/// What the Inspector preview shows.
#[derive(Clone, PartialEq)]
struct InspectorKey {
    material: Material,
    size: [u32; 2],
    yaw: f32,
    pitch: f32,
    shape: PreviewShape,
    /// Load counter of a custom shader (a recompiled shader needs a new render).
    generation: u64,
    /// Bumped every frame for animated previews.
    frame: u64,
}

/// A rendered preview: what it shows, its target and its egui texture.
type Slot = Option<(InspectorKey, RenderTarget, egui::TextureId)>;

fn shader_generation(m: &Material) -> u64 {
    if m.is_custom() {
        misty_engine::shader_asset::generation(&m.shader)
    } else {
        0
    }
}

#[derive(Default)]
pub struct MaterialPreviews {
    thumbs: HashMap<PathBuf, Thumb>,
    /// Frames rendered (thumbnails not asked for in a while are dropped).
    frame: u64,
    /// Shared target the thumbnails are rendered into before being copied out.
    thumb_target: Option<RenderTarget>,
    inspector_wanted: Option<InspectorKey>,
    inspector: Slot,
    /// The Shader Graph window's main preview.
    graph_wanted: Option<InspectorKey>,
    graph: Slot,
    graph_frame: u64,
    /// Inspector preview rotation (degrees, dragged with the mouse) and shape.
    pub yaw: f32,
    pub pitch: f32,
    pub shape: PreviewShape,
}

impl MaterialPreviews {
    /// Thumbnail of the `.mat` file at `path` (None until rendered or when it can't be read).
    pub fn thumbnail(&mut self, path: &Path) -> Option<egui::TextureId> {
        // The background file watch's stamp (no `stat` on the main thread every frame).
        let modified = misty_engine::file_watch::cached_modified(path);
        let frame = self.frame;
        let t = self.thumbs.entry(path.to_path_buf()).or_insert(Thumb {
            modified: None,
            material: None,
            texture: None,
            rendered: false,
            used: frame,
        });
        t.used = frame;
        if t.material.is_none() || t.modified != modified {
            t.modified = modified;
            t.material = Material::load(path).ok();
            t.rendered = false;
        }
        t.texture.as_ref().map(|(_, id)| *id)
    }

    /// Inspector preview of `m` at `size` pixels (None until the first render).
    pub fn inspector(&mut self, m: &Material, size: [u32; 2]) -> Option<egui::TextureId> {
        self.inspector_wanted = Some(InspectorKey {
            material: m.clone(),
            size: [size[0].max(16), size[1].max(16)],
            yaw: self.yaw,
            pitch: self.pitch,
            shape: self.shape,
            generation: shader_generation(m),
            frame: 0,
        });
        self.inspector.as_ref().map(|(_, _, id)| *id)
    }

    /// The Shader Graph window's main preview of `m` (re-rendered every frame when `animated`).
    pub fn graph_preview(
        &mut self,
        m: &Material,
        size: [u32; 2],
        yaw: f32,
        pitch: f32,
        shape: PreviewShape,
        animated: bool,
    ) -> Option<egui::TextureId> {
        if animated {
            self.graph_frame += 1;
        }
        self.graph_wanted = Some(InspectorKey {
            material: m.clone(),
            size: [size[0].max(16), size[1].max(16)],
            yaw,
            pitch,
            shape,
            generation: shader_generation(m),
            frame: self.graph_frame,
        });
        self.graph.as_ref().map(|(_, _, id)| *id)
    }

    /// Count a frame and take out the thumbnails no panel asked for in a while; returns
    /// their egui textures, to free (their GPU textures go with them).
    fn take_stale(&mut self) -> Vec<egui::TextureId> {
        self.frame += 1;
        let frame = self.frame;
        let mut freed = Vec::new();
        self.thumbs.retain(|_, t| {
            let keep = frame - t.used <= THUMB_KEEP_FRAMES;
            if !keep && let Some((_, id)) = t.texture.take() {
                freed.push(id);
            }
            keep
        });
        freed
    }

    /// True while some preview still waits to be rendered.
    pub fn pending(&self) -> bool {
        self.thumbs
            .values()
            .any(|t| !t.rendered && t.material.is_some())
            || self.inspector_wanted.as_ref() != self.inspector.as_ref().map(|i| &i.0)
            || self.graph_wanted.as_ref() != self.graph.as_ref().map(|i| &i.0)
    }
}

/// The preview scene: the material on a sphere (or cube) lit by a key and a fill light.
fn frame_parts(
    key_yaw: f32,
    key_pitch: f32,
    shape: PreviewShape,
) -> (Camera, Vec<LightData>, Mat4, PrimitiveMesh) {
    let camera = Camera {
        position: Vec3::new(0.0, 0.0, -3.6),
        rotation: Quat::IDENTITY,
        projection: Projection::Perspective { fov_y: 20.0 },
        near: 0.1,
        // Not closer: the view uniform fades the grid from 30 m on.
        far: 100.0,
    };
    let light = |direction: Vec3, intensity| LightData {
        kind: LightType::Directional,
        position: Vec3::ZERO,
        direction: direction.normalize(),
        color: [1.0; 3],
        intensity,
        range: 0.0,
        spot_angle: 0.0,
        shadows: Shadows::None,
        shadow: Default::default(),
        volumetric: Default::default(),
        contact_shadows: false,
        area: Default::default(),
    };
    let lights = vec![
        light(Vec3::new(0.6, -0.6, 0.8), 1.0),
        light(Vec3::new(-0.8, 0.3, 0.4), 0.35),
    ];
    let (mesh, scale) = match shape {
        PreviewShape::Sphere => (PrimitiveMesh::Sphere, 1.0),
        PreviewShape::Cube => (PrimitiveMesh::Cube, 0.62),
        // The 10 m plane, scaled to the sphere's size.
        PreviewShape::Plane => (PrimitiveMesh::Plane, 0.15),
    };
    let rotation = Quat::from_rotation_x(key_pitch.to_radians())
        * Quat::from_rotation_y(key_yaw.to_radians())
        * match shape {
            PreviewShape::Cube => {
                Quat::from_euler(misty_engine::glam::EulerRot::YXZ, 0.6, 0.45, 0.0)
            }
            PreviewShape::Plane => Quat::from_rotation_x(-1.0),
            PreviewShape::Sphere => Quat::IDENTITY,
        };
    let model = Mat4::from_scale_rotation_translation(Vec3::splat(scale), rotation, Vec3::ZERO);
    (camera, lights, model, mesh)
}

fn draw(
    gpu: &mut Gpu,
    target: &RenderTarget,
    m: &Material,
    yaw: f32,
    pitch: f32,
    shape: PreviewShape,
) {
    let (camera, lights, model, mesh) = frame_parts(yaw, pitch, shape);
    let environment = Environment::new(Default::default(), &lights);
    let meshes = [MeshDraw {
        mesh: mesh.into(),
        model,
        material: m.params(),
        cast_shadows: false,
        receive_shadows: false,
        outline: false,
        sorting: Default::default(),
        lod: None,
    }];
    let frame = Frame {
        camera: &camera,
        grid: None,
        skybox: false,
        background: [0.19, 0.19, 0.19],
        environment: &environment,
        post: Default::default(),
        lines: &[],
        overlay: &[],
        meshes: &meshes,
        particles: &[],
        terrains: &[],
        shaded: true,
        lights: &lights,
        water: &[],
    };
    let rs = &gpu.state;
    // Textures and shader variants a preview needs for the first time load on the
    // background queues (a folder of materials with 4K textures froze the editor while
    // each one was decoded here); the preview is drawn again once they arrive.
    gpu.renderer.set_background_loads(true);
    gpu.renderer.render(&rs.device, &rs.queue, target, &frame);
    gpu.renderer.set_background_loads(false);
}

/// True when the last [`draw`] left something to load in the background (its image is
/// incomplete: a stand-in shader or a 1×1 texture).
fn incomplete(gpu: &Gpu) -> bool {
    gpu.renderer.background_pending() > 0
}

/// Render the previews panels asked for this frame.
pub fn render(gpu: &mut Gpu, p: &mut MaterialPreviews) {
    let stale = p.take_stale();
    if !stale.is_empty() {
        let mut egui_renderer = gpu.state.renderer.write();
        for id in stale {
            egui_renderer.free_texture(&id);
        }
    }
    render_slot(gpu, p.inspector_wanted.as_ref(), &mut p.inspector);
    render_slot(gpu, p.graph_wanted.as_ref(), &mut p.graph);
    let waiting: Vec<PathBuf> = p
        .thumbs
        .iter()
        .filter(|(_, t)| !t.rendered && t.material.is_some())
        .map(|(k, _)| k.clone())
        .take(THUMBS_PER_FRAME)
        .collect();
    if waiting.is_empty() {
        return;
    }
    let target = p.thumb_target.get_or_insert_with(|| {
        gpu.renderer
            .create_target(&gpu.state.device, THUMB_SIZE, THUMB_SIZE)
    });
    let start = std::time::Instant::now();
    for (i, path) in waiting.into_iter().enumerate() {
        if i > 0 && start.elapsed() > THUMB_BUDGET {
            break;
        }
        let Some(t) = p.thumbs.get_mut(&path) else {
            continue;
        };
        let Some(m) = t.material.clone() else {
            continue;
        };
        draw(gpu, target, &m, 0.0, 0.0, PreviewShape::Sphere);
        if incomplete(gpu) {
            // Its textures or shader are still loading: keep the old thumbnail (if any)
            // and try again on a later frame.
            continue;
        }
        let device = &gpu.state.device;
        let (texture, _) = t.texture.get_or_insert_with(|| {
            let texture = device.create_texture(&wgpu::TextureDescriptor {
                label: Some("misty material thumbnail"),
                size: wgpu::Extent3d {
                    width: THUMB_SIZE,
                    height: THUMB_SIZE,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                // Gamma-encoded bytes, read as-is by egui (like the views' display format).
                format: wgpu::TextureFormat::Rgba8Unorm,
                usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
                view_formats: &[],
            });
            let view = texture.create_view(&Default::default());
            let id = gpu.state.renderer.write().register_native_texture(
                device,
                &view,
                wgpu::FilterMode::Linear,
            );
            (texture, id)
        });
        let mut encoder = device.create_command_encoder(&Default::default());
        target.copy_output(&mut encoder, texture);
        gpu.state.queue.submit([encoder.finish()]);
        t.rendered = true;
    }
}

/// Render one preview slot if what it shows changed.
fn render_slot(gpu: &mut Gpu, wanted: Option<&InspectorKey>, slot: &mut Slot) {
    let Some(want) = wanted.cloned() else {
        return;
    };
    if slot.as_ref().is_some_and(|(k, _, _)| *k == want) {
        return;
    }
    let [w, h] = want.size;
    let resized = slot.as_ref().is_none_or(|(_, t, _)| t.size() != (w, h));
    if resized {
        let target = gpu.renderer.create_target(&gpu.state.device, w, h);
        let mut egui_renderer = gpu.state.renderer.write();
        let filter = wgpu::FilterMode::Linear;
        let id = match &*slot {
            Some((_, _, id)) => {
                egui_renderer.update_egui_texture_from_wgpu_texture(
                    &gpu.state.device,
                    target.view(),
                    filter,
                    *id,
                );
                *id
            }
            None => egui_renderer.register_native_texture(&gpu.state.device, target.view(), filter),
        };
        drop(egui_renderer);
        *slot = Some((want.clone(), target, id));
    }
    let Some((key, target, _)) = slot.as_mut() else {
        return;
    };
    *key = want.clone();
    draw(
        gpu,
        target,
        &want.material,
        want.yaw,
        want.pitch,
        want.shape,
    );
    if incomplete(gpu) {
        // Something it shows is still loading: mark the slot stale so it is drawn again.
        if let Some((key, _, _)) = slot.as_mut() {
            key.frame = u64::MAX;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn thumbnails_reload_when_the_file_changes() {
        let dir = std::env::temp_dir().join("misty_material_preview_test");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("A.mat");
        Material::default().save(&path).unwrap();
        let mut p = MaterialPreviews::default();
        assert_eq!(p.thumbnail(&path), None, "rendered later");
        assert!(p.pending());
        p.thumbs.get_mut(&path).unwrap().rendered = true;
        assert!(!p.pending());
        // A new file version needs a new render.
        let m = Material {
            color: [1.0, 0.0, 0.0, 1.0],
            ..Default::default()
        };
        std::thread::sleep(std::time::Duration::from_millis(20));
        m.save(&path).unwrap();
        p.thumbnail(&path);
        assert!(p.pending());
        assert_eq!(p.thumbs[&path].material.as_ref(), Some(&m));
    }

    /// Thumbnails panels stop asking for (scrolled away, deleted, renamed) are dropped after
    /// a while; ones still shown stay.
    #[test]
    fn thumbnails_not_shown_for_a_while_are_dropped() {
        let dir = std::env::temp_dir().join(format!("misty_thumb_drop_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let (a, b) = (dir.join("A.mat"), dir.join("B.mat"));
        Material::default().save(&a).unwrap();
        Material::default().save(&b).unwrap();
        let mut p = MaterialPreviews::default();
        p.thumbnail(&a);
        p.thumbnail(&b);
        for _ in 0..THUMB_KEEP_FRAMES + 5 {
            assert!(p.take_stale().is_empty(), "nothing has a texture yet");
            p.thumbnail(&a);
        }
        assert!(p.thumbs.contains_key(&a), "still shown");
        assert!(!p.thumbs.contains_key(&b), "not shown for a while");
        assert_eq!(p.thumbs.len(), 1);
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn preview_sphere_fills_the_view() {
        let (camera, _, model, _) = frame_parts(0.0, 0.0, PreviewShape::Sphere);
        let vp = camera.view_projection(1.0) * model;
        let edge = vp.project_point3(Vec3::new(0.5, 0.0, 0.0));
        assert!((0.6..0.95).contains(&edge.x), "{edge}");
    }
}
