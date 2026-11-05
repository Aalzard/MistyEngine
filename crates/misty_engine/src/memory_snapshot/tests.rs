//! Memory snapshots: scene totals from known allocations, renderer items, diff math, WAV sizes.

use super::*;
use crate::particles::{Particle, ParticleEmitter};
use crate::scene::{GameObject, Node, Scene, Transform};

fn item(c: MemCategory, name: &str, bytes: u64) -> MemItem {
    MemItem::new(c, "T", name, bytes)
}

#[test]
fn scene_totals_match_the_component_structs() {
    let mut scene = Scene::default();
    let before = scene_items(&scene);
    let count = |items: &[MemItem], name: &str| {
        items
            .iter()
            .find(|i| i.name == name)
            .map_or(0, |i| i.refs as u64)
    };
    let objects = count(&before, "GameObject");
    for i in 0..10 {
        scene.spawn(&format!("Object {i}"), None);
    }
    let items = scene_items(&scene);
    let go = items.iter().find(|i| i.name == "GameObject").unwrap();
    assert_eq!(go.refs as u64, objects + 10);
    assert_eq!(
        go.bytes,
        go.refs as u64 * std::mem::size_of::<GameObject>() as u64
    );
    let t = items.iter().find(|i| i.name == "Transform").unwrap();
    assert_eq!(
        t.bytes,
        t.refs as u64 * std::mem::size_of::<Transform>() as u64
    );
    let n = items.iter().find(|i| i.name == "Node").unwrap();
    assert_eq!(n.bytes, n.refs as u64 * std::mem::size_of::<Node>() as u64);

    // A particle system with room for 100 particles.
    let e = scene.spawn("Sparks", None);
    let mut emitter = ParticleEmitter::default();
    emitter.particles.reserve_exact(100);
    let expected = (emitter.particles.capacity() * std::mem::size_of::<Particle>()) as u64;
    scene.world.insert_one(e, emitter).unwrap();
    let snap = MemorySnapshot::new("test", scene_items(&scene));
    let sparks = snap.items.iter().find(|i| i.name == "Sparks").unwrap();
    assert_eq!(sparks.bytes, expected);
    assert_eq!(snap.category_total(MemCategory::Particles), expected);
    let components: u64 = snap
        .items
        .iter()
        .filter(|i| i.category == MemCategory::SceneObjects)
        .map(|i| i.bytes)
        .sum();
    assert_eq!(snap.total(), components + expected);
    assert_eq!(
        snap.totals().iter().map(|(_, b)| b).sum::<u64>(),
        snap.total()
    );
}

#[test]
fn diff_matches_items_by_category_type_and_name() {
    let a = MemorySnapshot {
        label: "A".into(),
        items: vec![
            item(MemCategory::Textures, "grass.png", 1000),
            item(MemCategory::Meshes, "tree", 500),
            item(MemCategory::Audio, "beep.wav", 300),
        ],
        working_set: Some(10),
    };
    let b = MemorySnapshot {
        label: "B".into(),
        items: vec![
            item(MemCategory::Textures, "grass.png", 4000),
            item(MemCategory::Meshes, "tree", 500),
            item(MemCategory::Meshes, "rock", 200),
            // Same name, other category: a different item.
            item(MemCategory::Textures, "beep.wav", 7),
        ],
        working_set: None,
    };
    let rows = diff(&a, &b);
    assert_eq!(rows.len(), 5);
    assert_eq!(rows[0].name, "grass.png", "largest change first");
    assert_eq!(rows[0].delta(), 3000);
    assert_eq!(rows[0].change(), Change::Grew);
    let find = |n: &str, c| {
        rows.iter()
            .find(|r| r.name == n && r.category == c)
            .unwrap()
    };
    assert_eq!(
        find("beep.wav", MemCategory::Audio).change(),
        Change::Removed
    );
    assert_eq!(find("beep.wav", MemCategory::Audio).delta(), -300);
    assert_eq!(find("rock", MemCategory::Meshes).change(), Change::Added);
    assert_eq!(find("tree", MemCategory::Meshes).change(), Change::Same);
    let net: i64 = rows.iter().map(|r| r.delta()).sum();
    assert_eq!(net, b.total() as i64 - a.total() as i64);
    let cats = category_diff(&a, &b);
    let tex = cats.iter().find(|c| c.0 == MemCategory::Textures).unwrap();
    assert_eq!((tex.1, tex.2), (1000, 4007));
    assert_eq!(a.largest(), vec![0, 1, 2]);
}

#[test]
fn wav_clips_count_decoded_stereo_float_frames() {
    // 0.5 s of 8 kHz mono 16-bit = 4000 frames → 32000 bytes decoded.
    let wav = crate::audio::wav::sine_beep(440.0, 0.5, 8000);
    assert_eq!(decoded_audio_bytes("Assets/a.wav", &wav), 4000 * 8);
    assert_eq!(decoded_audio_bytes("Assets/a.ogg", &[0; 100]), 1000);
    assert_eq!(format_bytes(1536), "1.5 KB");
    assert_eq!(format_bytes(3 * 1_048_576), "3.0 MB");
    assert_eq!(format_delta(-2048), "-2.0 KB");
}

#[test]
fn working_set_is_reported_on_windows() {
    if cfg!(windows) {
        assert!(working_set().unwrap() > 1_000_000);
    }
}

#[test]
fn renderer_items_match_known_allocations() {
    let Some((device, queue)) = crate::render::gpu_tests::headless() else {
        eprintln!("no GPU adapter: skipped");
        return;
    };
    let r = crate::render::Renderer::with_queue(&device, &queue);
    let items = r.memory_items();
    let prims = items
        .iter()
        .find(|i| i.name == "Built-in primitives")
        .unwrap();
    let expected: u64 = crate::scene::PrimitiveMesh::ALL
        .iter()
        .map(|p| {
            let m = crate::render::mesh::primitive(*p);
            (m.vertices.len() * std::mem::size_of::<crate::render::mesh::Vertex>()
                + m.indices.len() * 4) as u64
        })
        .sum();
    assert_eq!(prims.bytes, expected);
    let cascades = items
        .iter()
        .find(|i| i.name.starts_with("Shadow cascades"))
        .unwrap();
    let size = r.quality().shadow_map_size() as u64;
    let n = 4; // render::cascades::MAX_CASCADES
    assert_eq!(cascades.bytes, size * size * 4 * n);
    // A 64×32 view: 105 bytes per pixel plus the bloom mips (32×16, 16×8, 8×4 at 8 bytes).
    let target = r.create_target(&device, 64, 32);
    let bloom: u64 = (1..=4)
        .map(|i| ((64u64 >> i).max(1)) * ((32u64 >> i).max(1)) * 8)
        .take(target_levels(64, 32))
        .sum();
    assert_eq!(target.memory_bytes(), 64 * 32 * 105 + bloom);
}

/// Bloom levels a target of this size makes (render/target.rs).
fn target_levels(w: u32, h: u32) -> usize {
    6u32.min((w.min(h) as f32).log2().floor().max(2.0) as u32 - 1) as usize
}
