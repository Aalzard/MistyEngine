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
        assert!((a.max.y - t.height).abs() < t.height * 0.2, "{} vs {}", a.max.y, t.height);
        assert!(a.min.y < 0.0 && a.min.y > -1.0);
        // The crown is about as wide as asked.
        let width = (a.max.x - a.min.x).max(a.max.z - a.min.z);
        let want = t.height * t.crown_width;
        assert!(width > want * 0.6 && width < want * 1.4, "{width} vs {want}");
        // A different seed gives a different tree of the same kind.
        let c = model(&TreeAsset { seed: t.seed + 1, ..t.clone() });
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
    for (name, t) in [("oak", TreeAsset::oak()), ("linden", TreeAsset::linden()), ("birch", TreeAsset::birch())] {
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
        b.model.impostor.albedo.save(dir.join(format!("imp_{name}.png"))).unwrap();
        b.model.impostor.normal.save(dir.join(format!("imp_{name}_n.png"))).unwrap();
    }
}
