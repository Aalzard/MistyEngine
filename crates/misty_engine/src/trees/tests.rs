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
