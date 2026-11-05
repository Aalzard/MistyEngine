//! What a bake reads, gathered from a scene on the main thread (then owned by the bake thread):
//! world triangles of the static objects with their materials, the lightmapped receivers, the
//! baked lights and the sky.

use std::collections::HashMap;

use glam::{Mat4, Vec2, Vec3};

use crate::render::environment::Environment;
use crate::render::srgb_to_linear;
use crate::render_scene::{self, MaterialSource};
use crate::scene::lightmapping::{LightMode, LightmapSettings, ReceiveGi};
use crate::scene::{Light, LightType, MeshFilter, MeshRenderer, Scene, Shadows};

/// Surface response of a triangle.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BakeMaterial {
    /// Linear diffuse albedo (× (1 − metallic)).
    pub albedo: Vec3,
    /// Linear emitted radiance.
    pub emission: Vec3,
    pub double_sided: bool,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum LightShape {
    Directional,
    Point,
    /// Cosines of the outer and inner half angles.
    Spot {
        cos_outer: f32,
        cos_inner: f32,
    },
    /// Half-extent axes; emits towards `hx × hy` (or both ways).
    Rectangle {
        hx: Vec3,
        hy: Vec3,
        two_sided: bool,
    },
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BakeLight {
    pub shape: LightShape,
    pub position: Vec3,
    /// Direction the light travels.
    pub direction: Vec3,
    /// Linear colour × intensity.
    pub color: Vec3,
    pub range: f32,
    /// Point / spot source radius (soft shadows); 0 = hard.
    pub radius: f32,
    pub shadows: bool,
    /// Baked: direct light goes into lightmaps. Mixed: only its bounces do.
    pub bake_direct: bool,
}

/// A uv2 triangle of a receiver in world space.
#[derive(Clone, Copy, Debug)]
pub struct ReceiverTriangle {
    pub p: [Vec3; 3],
    pub n: [Vec3; 3],
    pub uv: [Vec2; 3],
}

/// A lightmapped renderer.
#[derive(Clone, Debug)]
pub struct Receiver {
    /// Caller's key (entity bits for scenes).
    pub id: u64,
    pub triangles: Vec<ReceiverTriangle>,
    /// Scale In Lightmap.
    pub scale: f32,
}

impl Receiver {
    pub fn world_area(&self) -> f32 {
        self.triangles
            .iter()
            .map(|t| (t.p[1] - t.p[0]).cross(t.p[2] - t.p[0]).length() * 0.5)
            .sum()
    }

    pub fn uv_area(&self) -> f32 {
        self.triangles
            .iter()
            .map(|t| (t.uv[1] - t.uv[0]).perp_dot(t.uv[2] - t.uv[0]).abs() * 0.5)
            .sum()
    }
}

/// Sky radiance by direction: the realtime ambient gradient (up, equator, down), linear.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Sky {
    pub up: Vec3,
    pub equator: Vec3,
    pub down: Vec3,
}

impl Sky {
    pub fn radiance(&self, d: Vec3) -> Vec3 {
        if d.y >= 0.0 {
            self.equator.lerp(self.up, d.y)
        } else {
            self.equator.lerp(self.down, -d.y)
        }
    }

    /// The realtime ambient of an environment (intensity applied).
    pub fn of(env: &Environment) -> Self {
        let k = env.settings.environment_lighting.intensity.max(0.0);
        let [up, equator, down] = env.ambient();
        Self {
            up: up * k,
            equator: equator * k,
            down: down * k,
        }
    }
}

#[derive(Clone, Debug, Default)]
pub struct BakeInput {
    /// Every static triangle that blocks and bounces light (receivers included).
    pub triangles: Vec<[Vec3; 3]>,
    pub materials: Vec<BakeMaterial>,
    /// Material of each triangle.
    pub triangle_material: Vec<u32>,
    pub receivers: Vec<Receiver>,
    pub lights: Vec<BakeLight>,
    pub sky: Sky,
    pub settings: LightmapSettings,
}

impl BakeInput {
    /// Add a mesh in world space; `receiver` = also lightmap it under that key.
    pub fn add_mesh(
        &mut self,
        mesh: &crate::render::mesh::MeshData,
        model: Mat4,
        material: BakeMaterial,
        receiver: Option<(u64, f32)>,
    ) {
        let m = self.materials.len() as u32;
        self.materials.push(material);
        let normal = model.inverse().transpose();
        let mirror = model.determinant() < 0.0;
        let mut rec = receiver.map(|(id, scale)| Receiver {
            id,
            triangles: Vec::new(),
            scale,
        });
        for t in mesh.indices.as_chunks::<3>().0 {
            let v = t.map(|i| mesh.vertices[i as usize]);
            let p = v.map(|v| model.transform_point3(Vec3::from(v.position)));
            // Keep the outward winding under mirroring transforms.
            let p = if mirror { [p[0], p[2], p[1]] } else { p };
            let v = if mirror { [v[0], v[2], v[1]] } else { v };
            self.triangles.push(p);
            self.triangle_material.push(m);
            if let Some(r) = &mut rec {
                r.triangles.push(ReceiverTriangle {
                    p,
                    n: v.map(|v| {
                        normal
                            .transform_vector3(Vec3::from(v.normal))
                            .normalize_or(Vec3::Y)
                    }),
                    uv: v.map(|v| Vec2::from(v.uv2)),
                });
            }
        }
        if let Some(r) = rec {
            self.receivers.push(r);
        }
    }
}

/// Mean colour of an albedo texture (linear), cached per path.
fn texture_mean(path: &str, cache: &mut HashMap<String, Vec3>) -> Vec3 {
    if let Some(c) = cache.get(path) {
        return *c;
    }
    let mean = image::open(crate::assets::absolute(path))
        .ok()
        .map(|img| {
            let small = img.thumbnail(64, 64).to_rgb8();
            let n = (small.width() * small.height()).max(1) as f32;
            small.pixels().fold(Vec3::ZERO, |acc, p| {
                acc + Vec3::from(srgb_to_linear(p.0.map(|v| v as f32 / 255.0)))
            }) / n
        })
        .unwrap_or(Vec3::ONE);
    cache.insert(path.to_owned(), mean);
    mean
}

/// Is `e` a static renderer that takes part in GI?
pub fn contributes(scene: &Scene, e: hecs::Entity) -> bool {
    scene.game_object(e).is_static
        && scene
            .world
            .get::<&MeshRenderer>(e)
            .is_ok_and(|r| r.enabled && r.contribute_gi)
}

/// Is `e` lightmapped by a bake (contributes, receives from lightmaps)?
pub fn receives(scene: &Scene, e: hecs::Entity) -> bool {
    contributes(scene, e)
        && scene
            .world
            .get::<&MeshRenderer>(e)
            .is_ok_and(|r| r.receive_gi == ReceiveGi::Lightmaps && r.scale_in_lightmap > 0.0)
}

/// Bake input of a scene: static renderers that contribute GI, Mixed and Baked lights, the sky.
pub fn gather(scene: &Scene, materials: &dyn MaterialSource) -> BakeInput {
    let mut input = BakeInput {
        settings: scene.render_settings.lightmapping.sanitized(),
        ..Default::default()
    };
    let mut textures = HashMap::new();
    for e in scene.active_with::<MeshRenderer>() {
        if !contributes(scene, e) {
            continue;
        }
        let Some(mesh_ref) = scene
            .world
            .get::<&MeshFilter>(e)
            .ok()
            .and_then(|f| f.mesh.clone())
        else {
            continue;
        };
        let Some(mesh) = crate::models::mesh(&mesh_ref) else {
            continue;
        };
        let r = scene
            .world
            .get::<&MeshRenderer>(e)
            .expect("checked")
            .clone();
        let params = render_scene::renderer_material(scene, &r, 0, materials);
        let mut albedo = Vec3::from(srgb_to_linear([
            params.color[0],
            params.color[1],
            params.color[2],
        ]));
        if let Some(tex) = &params.albedo_map {
            albedo *= texture_mean(tex, &mut textures);
        }
        let metallic = if params.metallic_map.is_some() {
            0.5
        } else {
            params.metallic
        };
        let material = BakeMaterial {
            albedo: (albedo * (1.0 - metallic.clamp(0.0, 1.0))).clamp(Vec3::ZERO, Vec3::ONE),
            emission: Vec3::from(srgb_to_linear(params.emission)) * params.emission_intensity,
            double_sided: params.double_sided,
        };
        let receiver = receives(scene, e).then(|| (e.to_bits().get(), r.scale_in_lightmap));
        input.add_mesh(&mesh, scene.world_matrix(e), material, receiver);
    }
    for e in scene.active_with::<Light>() {
        let Ok(l) = scene.world.get::<&Light>(e) else {
            continue;
        };
        if !l.enabled || l.mode == LightMode::Realtime || l.intensity <= 0.0 {
            continue;
        }
        input.lights.push(bake_light(&l, scene.world_matrix(e)));
    }
    let lights = render_scene::lights(scene);
    input.sky = Sky::of(&Environment::new(scene.render_settings.clone(), &lights));
    input
}

/// A light component at `world` as the baker sees it.
pub fn bake_light(l: &Light, world: Mat4) -> BakeLight {
    let (_, rot, pos) = world.to_scale_rotation_translation();
    let half = (l.spot_angle * 0.5).to_radians();
    let shape = match l.kind {
        LightType::Directional => LightShape::Directional,
        LightType::Point => LightShape::Point,
        LightType::Spot => LightShape::Spot {
            cos_outer: half.cos(),
            cos_inner: (half * 0.8).cos(),
        },
        LightType::Rectangle => LightShape::Rectangle {
            hx: rot * Vec3::X * (l.width * 0.5).max(0.0),
            hy: rot * Vec3::Y * (l.height * 0.5).max(0.0),
            two_sided: l.two_sided,
        },
    };
    BakeLight {
        shape,
        position: pos,
        direction: (rot * Vec3::Z).normalize_or(Vec3::Z),
        color: Vec3::from(srgb_to_linear(l.color)) * l.intensity,
        range: l.range.max(1e-3),
        radius: match l.kind {
            LightType::Point | LightType::Spot => l.shadow_radius.max(0.0),
            _ => 0.0,
        },
        shadows: l.shadows != Shadows::None,
        bake_direct: l.mode == LightMode::Baked,
    }
}
