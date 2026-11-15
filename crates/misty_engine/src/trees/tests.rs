use glam::Vec3;

use super::generate::{self, TreeModel};
use super::impostor::{self, BakeTextures};
use super::*;

fn model(t: &TreeAsset) -> TreeModel {
    generate::build(t, &BakeTextures::default())
}

#[test]
fn asset_round_trips_and_sanitizes() {
    let t = TreeAsset::birch();
    assert_eq!(TreeAsset::from_json(&t.to_json()).unwrap(), t);
    let mut bad = TreeAsset {
        height: f32::NAN,
        branches: 0,
        leaf_size: 100.0,
        ..TreeAsset::oak()
    };
    bad.sanitize();
    assert!(bad.height.is_finite() && bad.branches >= 1 && bad.leaf_size <= 4.0);
    // Older or hand-written files with only a few fields load with the defaults.
    let partial = TreeAsset::from_json(r#"{ "height": 9.0 }"#).unwrap();
    assert_eq!(partial.height, 9.0);
    assert_eq!(partial.branches, TreeAsset::oak().branches);
}

#[test]
fn generation_is_deterministic_and_fits_its_budget() {
    for t in [TreeAsset::oak(), TreeAsset::linden(), TreeAsset::birch()] {
        let a = model(&t);
        let b = model(&t);
        assert_eq!(a.lods[0].vertices, b.lods[0].vertices);
        let (l0, l1) = (a.lods[0].triangles(), a.lods[1].triangles());
        // Budgets: a full tree stays well under 30 k triangles, the far mesh under a third.
        assert!(l0 > 2000 && l0 < 30_000, "LOD0 {l0}");
        assert!(l1 * 3 < l0 + 3000, "LOD1 {l1} vs {l0}");
        assert!(!a.lods[0].leaves.is_empty() && !a.lods[0].bark.is_empty());
        // About the recipe's height (clusters stick out of the crown a little), foot at the
        // origin.
        assert!(
            (a.max.y - t.height).abs() < t.height * 0.2,
            "{} vs {}",
            a.max.y,
            t.height
        );
        assert!(a.min.y < 0.0 && a.min.y > -1.0);
        // The crown is about as wide as asked.
        let width = (a.max.x - a.min.x).max(a.max.z - a.min.z);
        let want = t.height * t.crown_width;
        assert!(
            width > want * 0.6 && width < want * 1.4,
            "{width} vs {want}"
        );
        // A different seed gives a different tree of the same kind.
        let c = model(&TreeAsset {
            seed: t.seed + 1,
            ..t.clone()
        });
        assert_ne!(a.lods[0].vertices, c.lods[0].vertices);
    }
}

#[test]
fn wind_weights_grow_outwards_and_bark_does_not_flutter() {
    let m = model(&TreeAsset::oak());
    let lod = &m.lods[0];
    for &i in &lod.indices[lod.bark.start as usize..lod.bark.end as usize] {
        let v = lod.vertices[i as usize];
        assert_eq!(v.wind[2], 0.0);
        assert_eq!(v.extra[2], 0);
    }
    // Trunk vertices (near the axis, low) don't sway; leaves out on the crown do.
    let foot = lod
        .vertices
        .iter()
        .filter(|v| v.position[1] < 1.0)
        .map(|v| v.wind[0])
        .fold(0.0f32, f32::max);
    assert_eq!(foot, 0.0);
    let leaves = &lod.indices[lod.leaves.start as usize..lod.leaves.end as usize];
    let mean = leaves
        .iter()
        .map(|&i| lod.vertices[i as usize].wind[0])
        .sum::<f32>()
        / leaves.len() as f32;
    assert!(mean > 0.05, "{mean}");
}

#[test]
fn leaf_normals_face_out_of_the_crown() {
    let m = model(&TreeAsset::linden());
    let lod = &m.lods[0];
    let mut outward = 0;
    let mut total = 0;
    for &i in &lod.indices[lod.leaves.start as usize..lod.leaves.end as usize] {
        let v = lod.vertices[i as usize];
        let p = Vec3::from(v.position) - m.crown_center;
        total += 1;
        outward += usize::from(Vec3::from(v.normal).dot(p) > 0.0);
    }
    assert!(outward as f32 > total as f32 * 0.85, "{outward} of {total}");
}

#[test]
fn clumping_gathers_the_leaves_into_lit_lumps() {
    let even = TreeAsset::oak();
    let lumpy = TreeAsset {
        clumping: 0.8,
        ..TreeAsset::oak()
    };
    // Off: no card belongs to a lump (the crown as before).
    assert!(
        generate::skeleton(&even)
            .cards
            .iter()
            .all(|c| c.lump.is_none())
    );
    let sk = generate::skeleton(&lumpy);
    let cards = &sk.cards;
    assert!(cards.iter().all(|c| c.lump.is_some()));
    // Each card sits near its lump's ball.
    let near = cards
        .iter()
        .filter(|c| {
            let (centre, r) = c.lump.unwrap();
            c.anchor.distance(centre) < r * 1.6
        })
        .count();
    assert!(
        near as f32 > cards.len() as f32 * 0.9,
        "{near} of {}",
        cards.len()
    );
    // The lumps' undersides are darker than their tops: more contrast between up- and
    // down-facing leaves than in the evenly spread crown (sunlit lumps, dark hollows).
    let contrast = |t: &TreeAsset| {
        let m = model(t);
        let lod = &m.lods[0];
        let (mut up, mut down) = ((0.0, 0), (0.0, 0));
        for &i in &lod.indices[lod.leaves.start as usize..lod.leaves.end as usize] {
            let v = lod.vertices[i as usize];
            let ao = v.extra[0] as f32 / 255.0;
            if v.normal[1] > 0.4 {
                up = (up.0 + ao, up.1 + 1);
            } else if v.normal[1] < -0.4 {
                down = (down.0 + ao, down.1 + 1);
            }
        }
        assert!(up.1 > 0 && down.1 > 0);
        up.0 / up.1 as f32 - down.0 / down.1 as f32
    };
    let (a, b) = (contrast(&even), contrast(&lumpy));
    assert!(b > a + 0.03, "contrast even {a}, clumped {b}");
}

/// The point of the polyline `points` nearest to `p`.
fn nearest_on(points: &[Vec3], p: Vec3) -> Vec3 {
    let mut best = points[0];
    for w in points.windows(2) {
        let d = w[1] - w[0];
        let s = ((p - w[0]).dot(d) / d.length_squared().max(1e-9)).clamp(0.0, 1.0);
        let q = w[0] + d * s;
        if q.distance(p) < best.distance(p) {
            best = q;
        }
    }
    best
}

/// The bark is seen from outside: every bark triangle winds so that `cross(b - a, c - a)`
/// (the engine's front face, culled from behind) agrees with its vertex normals, and the
/// trunk's faces point away from the trunk's axis. Leaf cards face out of the crown too.
#[test]
fn bark_faces_point_out_of_the_limbs() {
    for t in [TreeAsset::oak(), TreeAsset::linden(), TreeAsset::birch()] {
        let sk = generate::skeleton(&t);
        let trunk = &sk.limbs[0].points;
        let m = model(&t);
        for (li, lod) in m.lods.iter().enumerate() {
            let tri = |i: usize, range: &std::ops::Range<u32>| {
                let idx = &lod.indices[range.start as usize + i * 3..][..3];
                idx.iter()
                    .map(|&k| lod.vertices[k as usize])
                    .collect::<Vec<_>>()
            };
            let mut trunk_faces = 0;
            for i in 0..lod.bark.len() / 3 {
                let v = tri(i, &lod.bark);
                let p: Vec<Vec3> = v.iter().map(|v| Vec3::from(v.position)).collect();
                let face = (p[1] - p[0]).cross(p[2] - p[0]);
                if face.length_squared() < 1e-14 {
                    continue;
                }
                let n: Vec3 = v.iter().map(|v| Vec3::from(v.normal)).sum();
                assert!(
                    face.dot(n) > 0.0,
                    "LOD{li} bark triangle {i}: winding {face:?} against normals {n:?}"
                );
                // The trunk: no sway at any corner (limbs sway from their second ring on).
                if v.iter().all(|v| v.wind[0] == 0.0) {
                    trunk_faces += 1;
                    let c = (p[0] + p[1] + p[2]) / 3.0;
                    let out = c - nearest_on(trunk, c);
                    assert!(
                        face.dot(out) > 0.0 && n.dot(out) > 0.0,
                        "LOD{li} trunk triangle {i} faces the axis"
                    );
                }
            }
            assert!(trunk_faces > 50, "LOD{li}: {trunk_faces} trunk faces");
            // Leaf cards: front faces mostly towards the outside of the crown.
            let (mut out, mut total) = (0, 0);
            for i in 0..lod.leaves.len() / 3 {
                let v = tri(i, &lod.leaves);
                let p: Vec<Vec3> = v.iter().map(|v| Vec3::from(v.position)).collect();
                let face = (p[1] - p[0]).cross(p[2] - p[0]);
                total += 1;
                out += usize::from(face.dot((p[0] + p[1] + p[2]) / 3.0 - m.crown_center) > 0.0);
            }
            assert!(
                out as f32 > total as f32 * 0.8,
                "LOD{li} leaves: {out} of {total}"
            );
        }
    }
}

#[test]
fn impostor_covers_the_crown_in_every_view() {
    let m = model(&TreeAsset::oak());
    let imp = &m.impostor;
    assert_eq!(imp.albedo.width(), impostor::CELL * impostor::VIEWS);
    for view in 0..impostor::VIEWS {
        let mut covered = 0;
        for y in 0..impostor::CELL {
            for x in 0..impostor::CELL {
                covered += u32::from(imp.albedo.get_pixel(view * impostor::CELL + x, y)[3] > 127);
            }
        }
        let share = covered as f32 / (impostor::CELL * impostor::CELL) as f32;
        assert!(share > 0.2 && share < 0.9, "view {view}: {share}");
    }
    // Transparent texels carry colour (no black fringes in the mips).
    let p = imp.albedo.get_pixel(2, 2);
    assert_eq!(p[3], 0);
    assert!(p[1] > 0);
}

/// Triangle counts and sizes of the presets (prints only).
#[test]
#[ignore = "prints only"]
fn tree_stats() {
    for (name, t) in [
        ("oak", TreeAsset::oak()),
        ("linden", TreeAsset::linden()),
        ("birch", TreeAsset::birch()),
    ] {
        let m = model(&t);
        for (i, l) in m.lods.iter().enumerate() {
            eprintln!(
                "{name} LOD{i}: {} tris (bark {}, leaves {}), {} vertices",
                l.triangles(),
                l.bark.len() / 3,
                l.leaves.len() / 3,
                l.vertices.len()
            );
        }
        eprintln!("{name}: bounds {:?} .. {:?}", m.min, m.max);
    }
}

/// Impostor atlases of the sample trees with their real textures (`MISTY_TREE_DUMP=<dir>`).
#[test]
#[ignore = "writes images; run on purpose"]
fn dump_impostors() {
    let Ok(dir) = std::env::var("MISTY_TREE_DUMP") else {
        return;
    };
    let root = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../SampleProject");
    crate::assets::set_project_root(&root);
    for name in ["Oak", "Oak Broad", "Linden", "Birch", "Young Oak"] {
        let t = TreeAsset::load(&root.join(format!("Assets/Trees/{name}.tree"))).unwrap();
        let b = super::store::build(t);
        let dir = std::path::PathBuf::from(&dir);
        b.model
            .impostor
            .albedo
            .save(dir.join(format!("imp_{name}.png")))
            .unwrap();
        b.model
            .impostor
            .normal
            .save(dir.join(format!("imp_{name}_n.png")))
            .unwrap();
    }
}

/// A small model tree as an OBJ (two levels, a pack-style billboard card) in a temporary
/// project; returns the project root and the recipe.
fn model_tree(tag: &str) -> (std::path::PathBuf, TreeAsset) {
    let root = std::env::temp_dir().join(format!("misty_model_tree_{tag}_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(root.join("Assets/T")).unwrap();
    let mtl = "newmtl Bark\nKd 1 1 1\nnewmtl Leave\nKd 1 1 1\nnewmtl Billboard\nKd 1 1 1\n";
    std::fs::write(root.join("Assets/T/tree.mtl"), mtl).unwrap();
    // Trunk: a 4 m three-sided column (LOD 0: two rings of sides, LOD 1: one). Leaves: two
    // separate cards out on the +X side (LOD 1: one). Billboard: a big card to leave out.
    let mut obj = String::from("mtllib tree.mtl\n");
    let mut n = 0;
    let mut object = |name: &str, material: &str, tris: &[[[f32; 3]; 3]]| {
        obj.push_str(&format!("o {name}\nusemtl {material}\n"));
        for t in tris {
            for p in t {
                obj.push_str(&format!("v {} {} {}\nvt 0.5 0.5\n", p[0], p[1], p[2]));
            }
            obj.push_str(&format!("f {0}/{0} {1}/{1} {2}/{2}\n", n + 1, n + 2, n + 3));
            n += 3;
        }
    };
    let column = |y0: f32, y1: f32| -> Vec<[[f32; 3]; 3]> {
        let ring = [[0.1, 0.0], [-0.05, 0.087], [-0.05, -0.087]];
        (0..3)
            .flat_map(|i| {
                let (a, b) = (ring[i], ring[(i + 1) % 3]);
                [
                    [[a[0], y0, a[1]], [b[0], y0, b[1]], [b[0], y1, b[1]]],
                    [[a[0], y0, a[1]], [b[0], y1, b[1]], [a[0], y1, a[1]]],
                ]
            })
            .collect()
    };
    let card = |x: f32, y: f32| [[x, y, 0.0], [x + 0.5, y, 0.0], [x, y + 0.5, 0.2]];
    let mut trunk0 = column(0.0, 2.0);
    trunk0.extend(column(2.0, 4.0));
    object("Trunk_LOD0", "Bark", &trunk0);
    object("Crown_LOD0", "Leave", &[card(1.0, 3.0), card(1.6, 2.0)]);
    object("Trunk_LOD1", "Bark", &column(0.0, 4.0));
    object("Crown_LOD1", "Leave", &[card(1.0, 3.0)]);
    object(
        "Billboard_LOD2",
        "Billboard",
        &[[[-9.0, 0.0, 0.0], [9.0, 0.0, 0.0], [0.0, 9.0, 0.0]]],
    );
    std::fs::write(root.join("Assets/T/tree.obj"), obj).unwrap();
    let t = TreeAsset {
        model: "Assets/T/tree.obj".into(),
        bark_texture: String::new(),
        bark_normal: String::new(),
        bark_roughness: String::new(),
        bark_occlusion: String::new(),
        bark_height: String::new(),
        leaf_texture: String::new(),
        leaf_normal: String::new(),
        leaf_roughness: String::new(),
        leaf_translucency: String::new(),
        ..TreeAsset::oak()
    };
    (root, t)
}

#[test]
fn lod_levels_are_read_from_names() {
    use super::from_model::lod_level;
    assert_eq!(lod_level("Forest_Tree1_LOD0"), Some(0));
    assert_eq!(lod_level("tree lod 2"), Some(2));
    assert_eq!(lod_level("LOD_3_bark"), Some(3));
    assert_eq!(lod_level("Melody"), None);
    assert_eq!(lod_level("Lodge"), None);
}

#[test]
fn a_tree_can_come_from_a_model_file() {
    let (root, t) = model_tree("build");
    crate::assets::set_project_root(&root);
    let built = store::try_build(t.clone()).expect("model tree");
    let m = &built.model;
    // LOD 0: 12 bark triangles and 2 leaf cards; LOD 1: 6 and 1; the billboard card is out.
    assert_eq!(m.lods[0].bark.len(), 36);
    assert_eq!(m.lods[0].leaves.len(), 6);
    assert_eq!(m.lods[1].bark.len(), 18);
    assert_eq!(m.lods[1].leaves.len(), 3);
    assert_eq!(m.lods[0].bark.end, m.lods[0].leaves.start);
    assert!((m.height - 4.0).abs() < 1e-4 && (m.max.y - 4.0).abs() < 1e-4);
    assert!(m.max.x.abs().max(m.min.x.abs()) < 3.0, "billboard left out");
    // The recipe the renderer reads carries the model's height (the wind scales by it).
    assert_eq!(built.asset.height, m.height);
    let lod0 = &m.lods[0];
    let vertex = |k: u32| &lod0.vertices[lod0.indices[k as usize] as usize];
    // Bark: no leaf mark and no flutter; the trunk does not sway with the limbs.
    for k in lod0.bark.clone() {
        let v = vertex(k);
        assert_eq!(v.extra[2], 0);
        assert_eq!(v.wind[2], 0.0);
        assert!(v.wind[0] < 0.01, "trunk sway {}", v.wind[0]);
        assert!((0.0..=1.0).contains(&v.wind[3]));
    }
    // Leaves: marked, fluttering, swaying out on the limb; each card its own colour variation.
    let (a, b) = (vertex(lod0.leaves.start), vertex(lod0.leaves.start + 3));
    for v in [a, b] {
        assert_eq!(v.extra[2], 255);
        assert!(v.wind[2] > 0.0 && v.wind[0] > 0.05);
    }
    assert_eq!(a.extra[1], vertex(lod0.leaves.start + 1).extra[1]);
    assert_ne!(a.extra[1], b.extra[1]);
    // The crown is what the leaves fill; the impostor shows the tree.
    assert!(m.crown_center.x.abs() > 0.5 && m.crown_center.y > 2.0);
    assert!(m.impostor.albedo.pixels().any(|p| p[3] > 0));
    assert!((m.impostor.height - 4.05).abs() < 0.01);
    // Same file, same tree.
    let again = store::try_build(t).unwrap();
    assert_eq!(again.model.lods[0].vertices, m.lods[0].vertices);
    std::fs::remove_dir_all(root).ok();
}

#[test]
fn a_missing_model_is_reported_and_the_generated_tree_stands_in() {
    let (root, mut t) = model_tree("missing");
    crate::assets::set_project_root(&root);
    t.model = "Assets/T/gone.obj".into();
    assert!(store::try_build(t.clone()).is_err());
    let stand_in = store::build(t.clone());
    assert!(stand_in.model.lods[0].triangles() > 2000);
    // The field stays out of files that do not use it.
    assert!(!TreeAsset::oak().to_json().contains("model"));
    assert!(!TreeAsset::oak().to_json().contains("gloss_maps"));
    let json = t.to_json();
    assert_eq!(
        TreeAsset::from_json(&json).unwrap().model,
        "Assets/T/gone.obj"
    );
    std::fs::remove_dir_all(root).ok();
}

/// Models that are no use as trees (no triangles, nothing but the pack's card, not a model at
/// all, broken numbers) give a clear error or a tree, never a panic; a model without leaves
/// or without bark is a tree.
#[test]
fn odd_models_make_trees_or_clear_errors() {
    let (root, t) = model_tree("odd");
    let _scope = crate::assets::scoped_root(&root);
    let write = |name: &str, text: &str| {
        std::fs::write(root.join("Assets/T").join(name), text).unwrap();
        TreeAsset {
            model: format!("Assets/T/{name}"),
            ..t.clone()
        }
    };
    let tri = |name: &str, material: &str, y: f32| {
        format!(
            "o {name}\nusemtl {material}\nv 0 {y} 0\nv 1 {y} 0\nv 0 {} 1\nf -3 -2 -1\n",
            y + 2.0
        )
    };
    let header = "mtllib tree.mtl\n";
    // Bark alone (a dead tree) and leaves alone (a bush of cards).
    let bark = write(
        "bark.obj",
        &format!("{header}{}", tri("Trunk", "Bark", 0.0)),
    );
    let built = store::try_build(bark).expect("bark only");
    assert!(built.model.lods[0].leaves.is_empty() && !built.model.lods[0].bark.is_empty());
    let leaves = write(
        "leaves.obj",
        &format!("{header}{}", tri("Crown", "Leave", 0.5)),
    );
    let built = store::try_build(leaves).expect("leaves only");
    assert!(built.model.lods[0].bark.is_empty() && !built.model.lods[0].leaves.is_empty());
    // Errors that say what is wrong.
    let cases = [
        ("empty.obj", String::new(), ""),
        ("points.obj", "v 0 0 0\nv 1 1 1\nv 0 2 0\n".to_owned(), ""),
        (
            "card.obj",
            format!("{header}{}", tri("Billboard", "Billboard", 0.0)),
            "no meshes",
        ),
        (
            "flat.obj",
            "o Flat\nv 0 0 0\nv 1 0 0\nv 0 0 1\nf 1 2 3\n".to_owned(),
            "no height",
        ),
        (
            "garbage.fbx",
            "Kaydara FBX Binary  \0garbage".to_owned(),
            "",
        ),
        (
            "text.fbx",
            "; FBX 7.4.0 project file\nnot really\n".to_owned(),
            "",
        ),
    ];
    for (name, text, says) in cases {
        let e = store::try_build(write(name, &text)).err();
        let e = e.unwrap_or_else(|| panic!("{name} made a tree"));
        assert!(e.contains(name), "{name}: the error names the file: {e}");
        assert!(e.contains(says), "{name}: {e}");
    }
    // A texture in the model slot, and a file that is gone.
    let png = TreeAsset {
        model: "Assets/T/tree.mtl".into(),
        ..t.clone()
    };
    assert!(store::try_build(png).is_err());
    // Broken numbers in the file: a tree or an error, no panic.
    let nan = write(
        "nan.obj",
        &format!(
            "{header}{}o Crown\nusemtl Leave\nv nan 1 0\nv inf 2 0\nv 0 3 -inf\nf -3 -2 -1\n",
            tri("Trunk", "Bark", 0.0)
        ),
    );
    let _ = store::try_build(nan);
    let huge = write(
        "huge.obj",
        &format!("{header}o Trunk\nusemtl Bark\nv 0 0 0\nv 1e30 0 0\nv 0 1e30 1\nf 1 2 3\n"),
    );
    let _ = store::try_build(huge);
    drop(_scope);
    std::fs::remove_dir_all(root).ok();
}
