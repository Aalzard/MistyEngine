//! Sample rocks in `SampleProject/Assets/Rocks`: six procedurally generated rock types (three
//! granite boulders, a layered sandstone outcrop, a small granite rock and a cluster of
//! cobbles), each a dense mesh imported with Virtual Geometry on plus a simplified LOD chain
//! (LOD Group) for the ordinary path, its own baked PBR maps (albedo, normal, occlusion and
//! smoothness) with lichen, moss on top faces and a grass-tinted base, a material and a prefab.
//! Everything is generated here and released as CC0 (see `Assets/Rocks/README.md`).
//!
//! Regenerate: `python tools/rock_textures.py target/rock-textures` (the tileable source
//! sets), then `cargo test --release -p misty_editor -- --ignored write_rocks`. The
//! MeadowDemo scene places them ([`meadow_rocks`], `sample_meadow.rs`).

pub mod bake;
mod glb;
pub mod shape;

use std::path::{Path, PathBuf};

use glam::{Quat, Vec2, Vec3};
use misty_engine::assets::Material;
use misty_engine::models::ModelImporter;
use misty_engine::scene::prefab::Prefab;
use misty_engine::scene::{
    LodGroup, LodLevel, MeshCollider, MeshFilter, MeshRef, MeshRenderer, Scene, Transform,
};
use misty_engine::terrain::TerrainData;

use bake::{Look, Stone as Kind};
use shape::{Stone, Strata, rand};

pub const DIR: &str = "Assets/Rocks";

/// One rock type.
pub struct Rock {
    pub name: &'static str,
    pub stones: Vec<Stone>,
    /// Icosphere subdivisions per stone (the last repeats).
    pub subdivisions: Vec<u32>,
    pub look: Look,
    /// Triangle share of LOD 1, 2, 3 (LOD 0 is the dense mesh).
    pub lods: [f32; 3],
    /// LOD Group switch heights (screen share) of LOD 0..3; under the last, culled.
    pub heights: [f32; 4],
    /// Albedo size; normal and mask maps are half of it.
    pub texture: usize,
    /// The player bumps into it (Mesh Collider on LOD 2).
    pub collider: bool,
    /// Rough horizontal radius at the ground line (metres, scale 1).
    pub footprint: f32,
}

impl Rock {
    pub fn mesh_path(&self) -> String {
        format!("{DIR}/Meshes/{}.glb", self.name)
    }
    pub fn lods_path(&self) -> String {
        format!("{DIR}/Meshes/{} LODs.glb", self.name)
    }
    pub fn albedo_path(&self) -> String {
        format!("{DIR}/Textures/{}_Albedo.jpg", self.name)
    }
    pub fn normal_path(&self) -> String {
        format!("{DIR}/Textures/{}_Normal.png", self.name)
    }
    pub fn mask_path(&self) -> String {
        format!("{DIR}/Textures/{}_Mask.png", self.name)
    }
    pub fn material_path(&self) -> String {
        format!("{DIR}/Materials/{}.mat", self.name)
    }
    pub fn prefab_path(&self) -> String {
        format!("{DIR}/Prefabs/{}.prefab", self.name)
    }

    fn is_big(&self) -> bool {
        self.footprint > 0.8
    }
}

fn granite(moss: f32, lichen: f32, stain: f32, band: f32, seed: u32) -> Look {
    Look {
        stone: Kind::Granite,
        relief: 0.004,
        // Weathered, lichen-dusted granite reads darker than fresh stone.
        tint: Vec3::splat(0.72),
        moss,
        lichen,
        stain,
        band,
        seed,
    }
}

/// The six sample rocks.
pub fn rocks() -> Vec<Rock> {
    let big_lods = [0.15, 0.04, 0.01];
    let big_heights = [0.25, 0.08, 0.025, 0.002];
    let small_lods = [0.2, 0.06, 0.02];
    let small_heights = [0.12, 0.04, 0.012, 0.004];
    let cobbles = (0..7)
        .map(|k| {
            let s = 900 + k;
            let r = 0.13 + 0.14 * rand(s, 1) * if k < 2 { 1.6 } else { 1.0 };
            let angle = k as f32 * 2.4 + rand(s, 2);
            let dist = if k == 0 {
                0.0
            } else {
                0.32 + 0.55 * rand(s, 3)
            };
            Stone {
                radii: Vec3::new(r * (1.0 + 0.3 * rand(s, 4)), r * 0.7, r),
                squareness: 2.0 + 0.6 * rand(s, 5),
                lumps: 0.1,
                lump_size: r * 2.0,
                facets: 2 + (k % 2),
                facet_keep: [0.8, 0.95],
                facet_round: r * 0.25,
                detail: 0.003,
                detail_size: 0.07,
                bury: 0.25 + 0.15 * rand(s, 6),
                settle: 0.2,
                seed: s,
                offset: Vec3::new(angle.cos() * dist, 0.0, angle.sin() * dist),
                yaw: rand(s, 7) * std::f32::consts::TAU,
                ..Default::default()
            }
        })
        .collect();
    vec![
        Rock {
            name: "Boulder Large",
            stones: vec![Stone {
                radii: Vec3::new(2.5, 1.9, 2.1),
                squareness: 2.2,
                lumps: 0.18,
                lump_size: 1.6,
                facets: 6,
                facet_keep: [0.68, 0.88],
                facet_round: 0.35,
                flakes: 0.06,
                cracks: 1,
                crack_depth: 0.03,
                crack_width: 0.025,
                detail: 0.035,
                detail_size: 0.7,
                bury: 0.2,
                seed: 11,
                ..Default::default()
            }],
            subdivisions: vec![6],
            look: granite(0.55, 0.6, 0.4, 0.25, 11),
            lods: big_lods,
            heights: big_heights,
            texture: 2048,
            collider: true,
            footprint: 2.4,
        },
        Rock {
            name: "Boulder Split",
            stones: vec![Stone {
                radii: Vec3::new(1.7, 1.45, 1.4),
                squareness: 2.6,
                lumps: 0.15,
                lump_size: 1.6,
                facets: 6,
                facet_keep: [0.7, 0.9],
                facet_round: 0.18,
                facet_min_y: -0.1,
                flakes: 0.02,
                cracks: 2,
                crack_depth: 0.18,
                crack_width: 0.05,
                detail: 0.02,
                detail_size: 0.5,
                bury: 0.2,
                seed: 23,
                ..Default::default()
            }],
            subdivisions: vec![6],
            look: granite(0.35, 0.75, 0.6, 0.22, 23),
            lods: big_lods,
            heights: big_heights,
            texture: 2048,
            collider: true,
            footprint: 1.55,
        },
        Rock {
            name: "Boulder Round",
            stones: vec![Stone {
                radii: Vec3::new(1.1, 1.0, 1.0),
                lumps: 0.18,
                lump_size: 0.9,
                facets: 3,
                facet_keep: [0.85, 0.95],
                facet_round: 0.4,
                flakes: 0.025,
                cracks: 1,
                crack_depth: 0.05,
                crack_width: 0.025,
                detail: 0.015,
                detail_size: 0.35,
                bury: 0.25,
                seed: 37,
                ..Default::default()
            }],
            subdivisions: vec![6],
            look: granite(0.7, 0.45, 0.3, 0.18, 37),
            lods: big_lods,
            heights: big_heights,
            texture: 1024,
            collider: true,
            footprint: 1.05,
        },
        Rock {
            name: "Outcrop Layered",
            stones: vec![Stone {
                radii: Vec3::new(4.2, 1.2, 2.7),
                squareness: 6.0,
                lumps: 0.2,
                lump_size: 2.2,
                facets: 5,
                facet_keep: [0.7, 0.9],
                facet_round: 0.15,
                facet_min_y: -0.05,
                cracks: 2,
                crack_depth: 0.1,
                crack_width: 0.04,
                detail: 0.02,
                detail_size: 0.5,
                strata: Some(Strata {
                    thickness: 0.3,
                    dip: 0.15,
                    recess: 0.06,
                }),
                bury: 0.3,
                settle: 0.2,
                seed: 51,
                ..Default::default()
            }],
            subdivisions: vec![6],
            look: Look {
                stone: Kind::Sandstone,
                relief: 0.008,
                tint: Vec3::ONE,
                moss: 0.6,
                lichen: 0.5,
                stain: 0.5,
                band: 0.22,
                seed: 51,
            },
            lods: big_lods,
            heights: big_heights,
            texture: 2048,
            collider: true,
            footprint: 3.6,
        },
        Rock {
            name: "Rock Small",
            stones: vec![Stone {
                radii: Vec3::new(0.42, 0.3, 0.36),
                squareness: 2.3,
                lumps: 0.14,
                lump_size: 0.4,
                facets: 7,
                facet_keep: [0.6, 0.85],
                facet_round: 0.03,
                detail: 0.006,
                detail_size: 0.12,
                bury: 0.2,
                seed: 67,
                ..Default::default()
            }],
            subdivisions: vec![5],
            look: granite(0.3, 0.5, 0.2, 0.07, 67),
            lods: small_lods,
            heights: small_heights,
            texture: 1024,
            collider: true,
            footprint: 0.4,
        },
        Rock {
            name: "Cobbles",
            stones: cobbles,
            subdivisions: vec![4],
            look: Look {
                stone: Kind::Cobble,
                relief: 0.0015,
                tint: Vec3::ONE,
                moss: 0.15,
                lichen: 0.25,
                stain: 0.0,
                band: 0.04,
                seed: 79,
            },
            lods: small_lods,
            heights: small_heights,
            texture: 1024,
            collider: false,
            footprint: 0.8,
        },
    ]
}

fn project() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../SampleProject")
}

/// The rock's material (colour = the bake's tint, so it is checked against the files).
pub fn material(rock: &Rock, tint: [f32; 3]) -> Material {
    Material {
        color: [tint[0], tint[1], tint[2], 1.0],
        metallic: 0.0,
        smoothness: 1.0,
        albedo_map: Some(rock.albedo_path()),
        normal_map: Some(rock.normal_path()),
        normal_scale: 1.0,
        metallic_map: Some(rock.mask_path()),
        occlusion_map: Some(rock.mask_path()),
        occlusion_strength: 1.0,
        ..Default::default()
    }
}

/// The rock's prefab: a LOD Group over LOD 0 (the dense, virtual mesh) and LOD 1–3.
pub fn prefab(rock: &Rock) -> Prefab {
    let mut s = Scene::new("Prefab");
    let root = s.spawn(rock.name, None);
    let mut group = LodGroup {
        lods: Vec::new(),
        ..Default::default()
    };
    for level in 0..4 {
        let e = s.spawn(&format!("LOD{level}"), Some(root));
        let mesh = match level {
            0 => format!("{}#0", rock.mesh_path()),
            k => format!("{}#{}", rock.lods_path(), k - 1),
        };
        s.add(
            e,
            MeshFilter {
                mesh: Some(MeshRef::Asset(mesh)),
            },
        );
        s.add(
            e,
            MeshRenderer {
                material: Some(rock.material_path()),
                ..Default::default()
            },
        );
        if level == 2 && rock.collider {
            s.add(e, MeshCollider::default());
        }
        group.lods.push(LodLevel {
            screen_height: rock.heights[level],
            renderers: vec![e],
        });
    }
    s.add(root, group);
    Prefab::from_scene(&s, root)
}

/// Give `rel` a meta file with a GUID when it has none.
fn ensure_meta(root: &Path, rel: &str) {
    let abs = root.join(rel);
    if misty_engine::asset_db::meta::read_guid(&abs).is_none() {
        misty_engine::asset_db::meta::write_guid(&abs, misty_engine::asset_db::Guid::new())
            .unwrap();
    }
}

/// Re-read the project's GUIDs (so saved JSON assets list the GUIDs of what they reference).
fn refresh_guids(root: &Path) {
    misty_engine::asset_db::install(misty_engine::asset_db::GuidDatabase::read(root));
}

const README: &str = include_str!("README.md");
const LICENSE: &str =
    "Misty Engine sample rocks (Assets/Rocks): meshes, textures, materials and prefabs.

Written by the Misty Engine authors, generated by code (crates/misty_editor/src/sample_rocks,
tools/rock_textures.py); no scanned, photographed or third-party material is used.

CC0 1.0 Universal (Public Domain Dedication)

To the extent possible under law, the authors have waived all copyright and related or
neighboring rights to these assets. You can copy, modify, distribute and use them, even for
commercial purposes, all without asking permission.
https://creativecommons.org/publicdomain/zero/1.0/
";

/// Tint of each rock's material, as baked (the material colour, sRGB).
fn tints(root: &Path) -> Vec<[f32; 3]> {
    rocks()
        .iter()
        .map(|r| {
            Material::load(&root.join(r.material_path()))
                .map(|m| [m.color[0], m.color[1], m.color[2]])
                .unwrap_or([1.0; 3])
        })
        .collect()
}

#[test]
#[ignore = "writes into SampleProject (needs tools/rock_textures.py output); run on purpose"]
fn write_rocks() {
    let root = project();
    let sources = std::env::var("MISTY_ROCK_TEXTURES")
        .map(PathBuf::from)
        .unwrap_or_else(|_| {
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/rock-textures")
        });
    let sets = bake::Sets::load(&sources).expect("run tools/rock_textures.py first");
    let only = std::env::var("MISTY_ROCK").unwrap_or_default();
    for dir in ["", "/Meshes", "/Textures", "/Materials", "/Prefabs"] {
        std::fs::create_dir_all(root.join(format!("{DIR}{dir}"))).unwrap();
        if !dir.is_empty() {
            ensure_meta(&root, &format!("{DIR}{dir}"));
        }
    }
    ensure_meta(&root, DIR);
    std::fs::write(root.join(format!("{DIR}/README.md")), README).unwrap();
    std::fs::write(root.join(format!("{DIR}/LICENSE.txt")), LICENSE).unwrap();
    ensure_meta(&root, &format!("{DIR}/README.md"));
    ensure_meta(&root, &format!("{DIR}/LICENSE.txt"));
    let mut tints = tints(&root);
    for (i, rock) in rocks().iter().enumerate() {
        if !only.is_empty() && !rock.name.contains(&only) {
            continue;
        }
        let started = std::time::Instant::now();
        let mesh = shape::build(&rock.stones, &rock.subdivisions);
        let levels: Vec<_> = rock
            .lods
            .iter()
            .map(|&r| shape::lod(&mesh.data, r))
            .collect();
        std::fs::write(
            root.join(rock.mesh_path()),
            glb::write(&[("LOD0", &mesh.data)]),
        )
        .unwrap();
        let named: Vec<(String, _)> = levels
            .iter()
            .enumerate()
            .map(|(k, m)| (format!("LOD{}", k + 1), m))
            .collect();
        let refs: Vec<(&str, _)> = named.iter().map(|(n, m)| (n.as_str(), *m)).collect();
        std::fs::write(root.join(rock.lods_path()), glb::write(&refs)).unwrap();
        let baked = bake::bake(&sets, &rock.look, &mesh.data, &mesh.cavity, rock.texture, 2);
        let mut file = std::fs::File::create(root.join(rock.albedo_path())).unwrap();
        misty_engine::image::codecs::jpeg::JpegEncoder::new_with_quality(&mut file, 92)
            .encode_image(&baked.albedo)
            .unwrap();
        baked.normal.save(root.join(rock.normal_path())).unwrap();
        baked.mask.save(root.join(rock.mask_path())).unwrap();
        for rel in [
            rock.mesh_path(),
            rock.lods_path(),
            rock.albedo_path(),
            rock.normal_path(),
            rock.mask_path(),
        ] {
            ensure_meta(&root, &rel);
        }
        ModelImporter {
            generate_lightmap_uvs: false,
            virtual_geometry: true,
            ..Default::default()
        }
        .save(&root.join(rock.mesh_path()))
        .unwrap();
        ModelImporter {
            generate_lightmap_uvs: false,
            virtual_geometry: false,
            ..Default::default()
        }
        .save(&root.join(rock.lods_path()))
        .unwrap();
        tints[i] = baked.tint.to_array().map(bake::linear_to_srgb);
        eprintln!(
            "{}: {} triangles (LODs {:?}), footprint {:.2} m, top {:.2} m, tint {:?}, {:.1} s",
            rock.name,
            mesh.data.indices.len() / 3,
            levels
                .iter()
                .map(|l| l.indices.len() / 3)
                .collect::<Vec<_>>(),
            mesh.footprint,
            mesh.top,
            tints[i],
            started.elapsed().as_secs_f32()
        );
    }
    refresh_guids(&root);
    for (rock, tint) in rocks().iter().zip(&tints) {
        material(rock, *tint)
            .save(&root.join(rock.material_path()))
            .unwrap();
        ensure_meta(&root, &rock.material_path());
    }
    refresh_guids(&root);
    for rock in rocks() {
        prefab(&rock).save(&root.join(rock.prefab_path())).unwrap();
        ensure_meta(&root, &rock.prefab_path());
    }
    refresh_guids(&root);
}

#[test]
fn rocks_match_their_generator() {
    let root = project();
    for (rock, tint) in rocks().iter().zip(tints(&root)) {
        for rel in [
            rock.mesh_path(),
            rock.lods_path(),
            rock.albedo_path(),
            rock.normal_path(),
            rock.mask_path(),
        ] {
            assert!(root.join(&rel).exists(), "{rel} (run write_rocks)");
            assert!(
                misty_engine::asset_db::meta::read_guid(&root.join(&rel)).is_some(),
                "{rel}.meta"
            );
        }
        let dense = ModelImporter::load(&root.join(rock.mesh_path()));
        assert!(
            dense.virtual_geometry && !dense.generate_lightmap_uvs,
            "{}",
            rock.name
        );
        assert!(!ModelImporter::load(&root.join(rock.lods_path())).generate_lightmap_uvs);
        let on_disk = Material::load(&root.join(rock.material_path())).unwrap();
        crate::test_util::assert_close(&on_disk, &material(rock, tint), rock.material_path());
        let saved = Prefab::load(&root.join(rock.prefab_path())).unwrap();
        crate::test_util::assert_json_close(
            &saved.to_json(),
            &prefab(rock).to_json(),
            rock.prefab_path(),
        );
        // The meshes load: a dense level 0 and three smaller levels.
        let dense = misty_engine::models::load(&root.join(rock.mesh_path())).unwrap();
        let lods = misty_engine::models::load(&root.join(rock.lods_path())).unwrap();
        let mut tris = dense.meshes[0].data.indices.len() / 3;
        assert!(tris > 20_000, "{}: {tris}", rock.name);
        assert_eq!(lods.meshes.len(), 3);
        for m in &lods.meshes {
            let t = m.data.indices.len() / 3;
            assert!(t < tris, "{}: {t} vs {tris}", rock.name);
            tris = t;
        }
    }
}

// ---- placement in MeadowDemo ---------------------------------------------------------------

/// Index of a rock in [`rocks`].
const LARGE: usize = 0;
const SPLIT: usize = 1;
const ROUND: usize = 2;
const OUTCROP: usize = 3;
const SMALL: usize = 4;
const COBBLES: usize = 5;

/// A group of rocks around a point given from the camera: `bearing` degrees right of the view
/// (+Z), `distance` metres, members spread within `spread` metres (small ones gather round
/// the first big one).
struct Group {
    bearing: f32,
    distance: f32,
    spread: f32,
    members: &'static [usize],
}

const fn group(bearing: f32, distance: f32, spread: f32, members: &'static [usize]) -> Group {
    Group {
        bearing,
        distance,
        spread,
        members,
    }
}

const GROUPS: &[Group] = &[
    // Near, just left of the first view: the close-up group.
    group(-68.0, 20.0, 6.0, &[LARGE, SMALL, SMALL, COBBLES, COBBLES]),
    // Behind the player and to the sides (seen when turning round).
    group(165.0, 55.0, 14.0, &[OUTCROP, SPLIT, SMALL, COBBLES]),
    group(-150.0, 38.0, 8.0, &[ROUND, COBBLES, SMALL]),
    group(80.0, 70.0, 10.0, &[SPLIT, SMALL, COBBLES]),
    group(205.0, 150.0, 12.0, &[LARGE, SMALL, COBBLES]),
    // The open field's sides, middle distance.
    group(34.0, 140.0, 10.0, &[ROUND, SMALL, COBBLES]),
    group(27.0, 95.0, 8.0, &[OUTCROP, SMALL, COBBLES]),
    group(-30.0, 115.0, 8.0, &[LARGE, SMALL, COBBLES]),
    group(-44.0, 235.0, 12.0, &[SPLIT, ROUND, SMALL]),
    group(55.0, 320.0, 16.0, &[LARGE, ROUND, SMALL]),
    group(-95.0, 160.0, 12.0, &[LARGE, COBBLES, SMALL]),
    group(120.0, 210.0, 14.0, &[OUTCROP, SMALL]),
    group(-125.0, 260.0, 14.0, &[SPLIT, ROUND, COBBLES]),
    // The foot of the mountains: outcrops and boulders.
    group(-20.0, 600.0, 40.0, &[OUTCROP, LARGE, SPLIT, SMALL]),
    group(18.0, 640.0, 40.0, &[OUTCROP, ROUND, SMALL]),
    group(60.0, 560.0, 40.0, &[LARGE, SPLIT, SMALL]),
    group(-65.0, 560.0, 40.0, &[OUTCROP, SPLIT, SMALL]),
    group(110.0, 480.0, 40.0, &[OUTCROP, LARGE]),
    group(-110.0, 500.0, 40.0, &[LARGE, ROUND, SMALL]),
    group(160.0, 430.0, 40.0, &[OUTCROP, SPLIT]),
    group(-165.0, 420.0, 40.0, &[OUTCROP, LARGE, SMALL]),
];

/// A placed rock (world space).
#[derive(Clone, Debug, PartialEq)]
pub struct Placed {
    pub rock: usize,
    pub position: Vec3,
    pub rotation: Quat,
    pub scale: f32,
}

/// Terrain normal at local (x, z) from central differences.
fn terrain_normal(d: &TerrainData, x: f32, z: f32) -> Vec3 {
    let e = 1.5;
    let hx = d.sample_height(x + e, z) - d.sample_height(x - e, z);
    let hz = d.sample_height(x, z + e) - d.sample_height(x, z - e);
    Vec3::new(-hx, 2.0 * e, -hz).normalize()
}

/// The meadow's rocks (deterministic): every group's members on ground that isn't too steep,
/// clear of trees, each other, the player and the open view ahead; tilted towards the slope,
/// sunk so no edge floats. `corner` = the terrain's world corner, `eye` = the camera's
/// terrain-local (x, z), `player` = the player's world position.
pub fn meadow_rocks(d: &TerrainData, corner: Vec3, eye: Vec2, player: Vec3) -> Vec<Placed> {
    let kinds = rocks();
    let size = Vec2::new(d.size.x, d.size.z);
    let trees: Vec<Vec2> = d
        .trees
        .instances
        .iter()
        .map(|t| t.position * size)
        .collect();
    let mut out: Vec<Placed> = Vec::new();
    // Local positions and footprints of the rocks placed so far.
    let mut taken: Vec<(Vec2, f32)> = Vec::new();
    let mut n = 0u32;
    for (g, grp) in GROUPS.iter().enumerate() {
        let a = grp.bearing.to_radians();
        let centre = eye + Vec2::new(a.sin(), a.cos()) * grp.distance;
        let mut anchor: Option<(Vec2, f32)> = None;
        for &kind in grp.members {
            let rock = &kinds[kind];
            let big = rock.is_big();
            for attempt in 0..40 {
                n += 1;
                let r = |salt| rand(0x5eed_0000 + n, salt);
                let scale = if big {
                    0.8 + 0.45 * r(1)
                } else {
                    0.75 + 0.6 * r(1)
                };
                let fp = rock.footprint * scale;
                let p = match anchor {
                    // Small rocks gather round the group's first big one.
                    Some((at, afp)) if !big => {
                        let ang = r(2) * std::f32::consts::TAU;
                        at + Vec2::new(ang.cos(), ang.sin())
                            * (afp + fp * 0.6 + 2.2 * r(3) + 0.2 * attempt as f32)
                    }
                    _ => {
                        let ang = r(2) * std::f32::consts::TAU;
                        let dist = if anchor.is_none() && attempt < 4 {
                            grp.spread * 0.3 * r(3)
                        } else {
                            grp.spread * r(3).sqrt()
                        };
                        centre + Vec2::new(ang.cos(), ang.sin()) * dist
                    }
                };
                let rel = p - eye;
                if rel.x.atan2(rel.y).to_degrees().abs() < 16.0 && rel.length() < 450.0 {
                    continue;
                }
                if p.cmplt(Vec2::splat(20.0)).any() || p.cmpgt(size - 20.0).any() {
                    continue;
                }
                let world_xz = Vec2::new(corner.x, corner.z) + p;
                if world_xz.distance(Vec2::new(player.x, player.z)) < 8.0 + fp {
                    continue;
                }
                let slope = misty_engine::terrain::trees::slope_at(d, p.x, p.y);
                let max_slope = match kind {
                    OUTCROP => 34.0,
                    _ if big => 30.0,
                    _ => 26.0,
                };
                if slope > max_slope {
                    continue;
                }
                let clear_of_trees = if big { 4.5 + fp } else { 2.0 + fp };
                if trees.iter().any(|t| t.distance(p) < clear_of_trees) {
                    continue;
                }
                let overlap = taken
                    .iter()
                    .any(|(q, qfp)| q.distance(p) < (fp + qfp) * if big { 1.1 } else { 0.75 });
                if overlap {
                    continue;
                }
                // Tilt towards the slope (big rocks less), a little random lean.
                let normal = terrain_normal(d, p.x, p.y);
                let align = if big { 0.45 } else { 0.8 };
                let lean = Vec3::new(r(4) - 0.5, 0.0, r(5) - 0.5) * 0.08;
                let up = (Vec3::Y.lerp(normal, align) + lean).normalize();
                let yaw = r(6) * std::f32::consts::TAU;
                let rotation = Quat::from_rotation_arc(Vec3::Y, up) * Quat::from_rotation_y(yaw);
                // Sink until no edge of the footprint floats above the ground.
                let h = d.sample_height(p.x, p.y);
                let mut gap: f32 = 0.0;
                for k in 0..8 {
                    let ang = k as f32 * std::f32::consts::FRAC_PI_4;
                    let off = Vec2::new(ang.cos(), ang.sin()) * fp;
                    let ground = d.sample_height(p.x + off.x, p.y + off.y);
                    let plane = h - (up.x * off.x + up.z * off.y) / up.y;
                    gap = gap.max(plane - ground);
                }
                let sink = gap + 0.04 * fp + 0.03;
                let position = corner + Vec3::new(p.x, h - sink, p.y);
                out.push(Placed {
                    rock: kind,
                    position,
                    rotation,
                    scale,
                });
                taken.push((p, fp));
                if anchor.is_none() && big {
                    anchor = Some((p, fp));
                }
                let _ = g;
                break;
            }
        }
    }
    out
}

/// The rocks under a "Rocks" object (prefab instances), appended to `s`.
pub fn add_to_scene(s: &mut Scene, placed: &[Placed]) {
    let kinds = rocks();
    let parent = s.spawn("Rocks", None);
    let prefabs: Vec<Prefab> = kinds
        .iter()
        .map(|r| {
            Prefab::load(&project().join(r.prefab_path())).expect("rock prefab (run write_rocks)")
        })
        .collect();
    for p in placed {
        let rock = &kinds[p.rock];
        let e = s.instantiate_prefab(&prefabs[p.rock], Some(&rock.prefab_path()), Some(parent));
        let mut t = Transform {
            position: p.position,
            scale: Vec3::splat(p.scale),
            ..Default::default()
        };
        t.set_rotation(p.rotation);
        s.set_transform(e, t);
    }
}
