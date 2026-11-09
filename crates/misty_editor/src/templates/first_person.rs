//! The First Person template: an endless flat white ground under a clear sky and the sample
//! project's First Person Player prefab (with its controller and head bob scripts), whose
//! head camera is the scene's Main Camera. Press Play and walk.
//!
//! The prefab, its scripts and the sample's project settings (the Run and Crouch axes, the
//! Player layer) come from the sample project like MeadowDemo's ([`super::meadow`]).

use std::path::Path;

use glam::{Quat, Vec3};
use misty_engine::assets::Material;
use misty_engine::scene::prefab::Prefab;
use misty_engine::scene::render_settings::SunDisk;
use misty_engine::scene::{
    BoxCollider, Camera, MeshFilter, MeshRenderer, PrimitiveMesh, Scene, Transform,
};

use super::meadow::{self, SampleFiles};
use super::samples::{self, Source};
use super::scenes::{self, scaled};
use super::textures;
use crate::scene_files::SavedView;

pub const PREFAB: &str = "Assets/Prefabs/First Person Player.prefab";
pub const GROUND_MAT: &str = "Assets/Materials/Ground.mat";
pub const GRID_TEXTURE: &str = "Assets/Textures/Grid.png";
/// The ground slab: 6 km square, 1 m thick, its top at y = 0.
pub const GROUND_SIZE: Vec3 = Vec3::new(6000.0, 1.0, 6000.0);
/// Side of one square of the ground's collider (see [`scene`]).
pub const COLLIDER_TILE: f32 = 1000.0;
/// One grid tile (the texture's two major lines) covers this many metres.
const GRID_TILE: f32 = 8.0;
/// The player's head camera sees this far (the ground fades into the haze before its edge).
const VIEW_DISTANCE: f32 = 3000.0;
/// The player starts this far above the ground and drops onto it (a Character Controller
/// starting exactly on a surface can slip through it).
const SPAWN_HEIGHT: f32 = 0.1;
/// The sun, as Euler angles of the Directional Light (degrees): 24° up, so its glow is at the
/// top of the player's first view.
const SUN: Vec3 = Vec3::new(24.0, 200.0, 0.0);

/// Write the First Person template into `root` (a project folder with its settings). Returns
/// the Scene view to start with.
pub(super) fn create(root: &Path) -> Result<SavedView, String> {
    let source = samples::source().ok_or(
        "this install has no sample project to copy the First Person Player from \
         (Examples/SampleProject next to the editor)",
    )?;
    create_from(root, &source)
}

pub(super) fn create_from(root: &Path, source: &Source) -> Result<SavedView, String> {
    let src = SampleFiles::open(source)?;
    meadow::copy_files(&src, root, meadow::dependencies(&src, &[PREFAB], &[])?)?;
    textures::save(root, GRID_TEXTURE, &textures::grid())?;
    let tiles = GROUND_SIZE.x / GRID_TILE;
    Material {
        color: [0.97, 0.97, 0.97, 1.0],
        smoothness: 0.1,
        albedo_map: Some(GRID_TEXTURE.into()),
        tiling: [tiles, tiles],
        ..Default::default()
    }
    .save(&root.join(GROUND_MAT))?;
    let prefab = Prefab::load(&root.join(PREFAB))?;
    let (scene, view) = scene(&prefab);
    super::save_scene(root, super::SCENE, &scene)?;
    Ok(view)
}

/// Yaw (degrees) facing the sun of [`SUN`] horizontally.
fn sun_yaw() -> f32 {
    let light = misty_engine::camera::euler_to_quat(SUN);
    meadow::toward_sun_yaw(light)
}

/// The scene (the player standing at the origin, facing the sun) and a Scene view behind and
/// above the player's eyes.
pub(super) fn scene(prefab: &Prefab) -> (Scene, SavedView) {
    let mut s = Scene::default_3d("Main");
    // The player's head camera is the Main Camera.
    if let Some(cam) = s.find_by_name("Main Camera") {
        s.despawn(cam);
    }
    scenes::sun(&mut s, SUN, [1.0, 0.95, 0.88], 1.4);
    let sky = &mut s.render_settings.skybox;
    sky.sun_disk = SunDisk::HighQuality;
    // A pale horizon so the far ground and the sky meet without a seam.
    sky.ground = [0.8, 0.82, 0.85];

    let ground = s.spawn("Ground", None);
    s.set_transform(
        ground,
        scaled(Vec3::new(0.0, -GROUND_SIZE.y * 0.5, 0.0), GROUND_SIZE),
    );
    s.add(ground, MeshFilter::primitive(PrimitiveMesh::Cube));
    s.add(
        ground,
        MeshRenderer {
            material: Some(GROUND_MAT.into()),
            ..Default::default()
        },
    );
    // The collider in tiles: a Character Controller slips through one huge box (the shape
    // cast loses precision against a 6 km cuboid).
    let colliders = s.spawn("Ground Colliders", None);
    let n = (GROUND_SIZE.x / COLLIDER_TILE).round() as i32;
    for i in 0..n {
        for j in 0..n {
            let at = |k: i32| (k as f32 + 0.5) * COLLIDER_TILE - GROUND_SIZE.x * 0.5;
            let tile = s.spawn(&format!("Tile {}", i * n + j + 1), Some(colliders));
            s.set_transform(
                tile,
                scaled(
                    Vec3::new(at(i), -GROUND_SIZE.y * 0.5, at(j)),
                    Vec3::new(COLLIDER_TILE, GROUND_SIZE.y, COLLIDER_TILE),
                ),
            );
            s.add(tile, BoxCollider::default());
        }
    }

    let yaw = sun_yaw();
    let player = s.instantiate_prefab(prefab, Some(PREFAB), None);
    s.set_transform(
        player,
        Transform::from_position(Vec3::Y * SPAWN_HEIGHT).with_euler(Vec3::new(0.0, yaw, 0.0)),
    );
    let head = s
        .walk()
        .into_iter()
        .map(|(e, _)| e)
        .find(|&e| e != player && s.world.get::<&Camera>(e).is_ok());
    if let Some(head) = head {
        s.game_object_mut(head).name = "Main Camera".into();
        if let Ok(mut c) = s.world.get::<&mut Camera>(head) {
            c.far = VIEW_DISTANCE;
        }
    }

    let mut volume = scenes::sky_volume();
    // A light haze hides the ground's far edge and gives the plain some depth.
    volume.color_adjustments.post_exposure = 0.6;
    let f = &mut volume.fog;
    f.active = true;
    f.volumetric = false;
    f.max_fog_distance = VIEW_DISTANCE;
    f.attenuation_distance = 4000.0;
    f.base_height = 0.0;
    f.maximum_height = 200.0;
    scenes::add_volume(&mut s, volume);

    let rotation = Quat::from_rotation_y(yaw.to_radians());
    let forward = rotation * Vec3::Z;
    let eye = Vec3::Y * (SPAWN_HEIGHT + 1.7);
    let view = misty_engine::camera::euler_to_quat(Vec3::new(12.0, yaw, 0.0));
    let saved = SavedView {
        position: (eye - forward * 6.0 + Vec3::Y * 2.0).to_array(),
        rotation: view.to_array(),
    };
    (s, saved)
}
