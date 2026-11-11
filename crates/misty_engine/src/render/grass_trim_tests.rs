//! What the grass holds stays bounded while the camera travels (round 35): chunks left behind
//! are trimmed to the blades their distance draws, the blade pages never spill, and a field
//! too dense for them goes coarser instead.

use std::sync::Arc;

use glam::{Vec2, Vec3};

use super::{
    CHUNK, Grass, LEVELS, Pressure, RELAX_AFTER, TRIM_SLACK, level_counts, reach, set_grass_budget,
    thin, trim_level, wanted,
};
use crate::render::gpu_tests::headless;
use crate::render::grass_cull::record;
use crate::render::grass_pool::BLADE_BYTES;
use crate::terrain::TerrainData;
use crate::terrain::details::{self, Blade};

/// A flat `side` m field covered in grass: `density` blades per square metre, full density
/// up to `lod` m, drawn up to `range` m. With seed heads and wildflowers (their blades sort
/// among the grass's).
fn meadow(side: f32, density: f32, lod: f32, range: f32) -> Arc<TerrainData> {
    let texels = (side as usize).next_power_of_two().min(1024);
    let mut d = TerrainData::new(65, Vec3::new(side, 10.0, side), texels);
    d.details.enabled = true;
    d.details.density = density;
    d.details.lod_distance = lod;
    d.details.draw_distance = range;
    d.details.seed_heads = 0.2;
    d.detail_map = vec![255; texels * texels];
    d.flowers.enabled = true;
    // (Only scattered here, never drawn: the texture is not read.)
    d.flowers.kinds = vec![crate::terrain::flowers::FlowerKind {
        texture: "Assets/Flowers/Daisy.png".into(),
        amount: 1.0,
        ..Default::default()
    }];
    d.flowers.density = 2.0;
    d.flowers.coverage = 0.5;
    d.flowers.scattered = 0.05;
    Arc::new(d)
}

/// Puts the device's own blade budget back when a test ends (also when it fails).
struct Budget;

impl Budget {
    fn of(blades: u64) -> Self {
        set_grass_budget(blades);
        Budget
    }
}

impl Drop for Budget {
    fn drop(&mut self) {
        set_grass_budget(0);
    }
}

/// Bytes `range` of `buffer`.
fn read(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    buffer: &wgpu::Buffer,
    range: std::ops::Range<u64>,
) -> Vec<u8> {
    let size = range.end - range.start;
    let copy = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("misty grass trim readback"),
        size,
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    let mut encoder = device.create_command_encoder(&Default::default());
    encoder.copy_buffer_to_buffer(buffer, range.start, &copy, 0, size);
    queue.submit([encoder.finish()]);
    let slice = copy.slice(..);
    slice.map_async(wgpu::MapMode::Read, |_| {});
    let _ = device.poll(wgpu::PollType::wait_indefinitely());
    slice.get_mapped_range().expect("mapped").to_vec()
}

/// The blades chunk `k` holds on the GPU, and its cull records.
fn held_blades(
    g: &Grass,
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    k: (u32, u32),
) -> (Vec<Blade>, Vec<[u32; 4]>) {
    let c = g.chunks.get(&k).expect("chunk held");
    let slot = c.store.expect("blades");
    let pool = g.pool.as_ref().expect("pages");
    let at = slot.at as u64 * BLADE_BYTES;
    let bytes = read(
        device,
        queue,
        pool.buffer(slot.page),
        at..at + c.count as u64 * BLADE_BYTES,
    );
    let records = match (c.arena, &g.arena) {
        (Some(at), Some(arena)) => {
            let at = at as u64 * 16;
            let bytes = read(device, queue, &arena.buffer, at..at + c.count as u64 * 16);
            bytemuck::pod_collect_to_vec(&bytes)
        }
        _ => Vec::new(),
    };
    (bytemuck::pod_collect_to_vec(&bytes), records)
}

#[test]
fn coarser_levels_are_prefixes_of_a_chunk() {
    let d = meadow(64.0, 120.0, 8.0, 60.0);
    let (lo, hi) = (Vec2::splat(16.0), Vec2::splat(32.0));
    let full = details::scatter_level(&d, lo, hi, 0);
    assert!(full.len() > 20_000, "{} blades", full.len());
    assert!(full.iter().any(Blade::flower) && full.iter().any(|b| b.seed_head()));
    let counts = level_counts(&full, 0);
    assert_eq!(counts[0] as usize, full.len());
    for level in 1..LEVELS {
        let coarse = details::scatter_level(&d, lo, hi, level as u32);
        // (A seed head's stored key is rounded: the count may be a blade off at the edge.)
        let n = counts[level] as usize;
        assert!(
            n.abs_diff(coarse.len()) <= 2,
            "level {level}: {n} != {}",
            coarse.len()
        );
        let common = n.min(coarse.len());
        assert!(coarse[..common] == full[..common], "level {level}");
        // About half the blades of the level before (flowers stay at every level).
        assert!(counts[level] < counts[level - 1] && counts[level] > counts[level - 1] / 3);
        // Counted on a coarser scatter, the levels below it hold the same blades.
        let again = level_counts(&coarse, level as u32);
        assert!(again[..=level].iter().all(|c| *c as usize == coarse.len()));
        for l in level + 1..LEVELS {
            assert!(again[l].abs_diff(counts[l]) <= 2);
        }
    }
}

#[test]
fn chunks_are_trimmed_with_a_level_to_spare() {
    // Up to a level finer than needed: left alone (stepping back and forth costs nothing).
    let slack = TRIM_SLACK;
    assert_eq!(trim_level(0, 0, slack), None);
    assert_eq!(trim_level(0, slack, slack), None);
    assert_eq!(trim_level(3, 3 + slack, slack), None);
    // Finer than that: down to one level finer than needed.
    assert_eq!(trim_level(0, 2, slack), Some(2 - slack));
    assert_eq!(trim_level(0, 6, slack), Some(6 - slack));
    assert_eq!(trim_level(4, 6, slack), Some(6 - slack));
    // Never finer than it is, never coarser than needed.
    for slack in 0..=2 {
        for level in 0..=6 {
            for want in 0..=6 {
                if let Some(to) = trim_level(level, want, slack) {
                    assert!(to > level && to <= want);
                }
            }
        }
    }
    // Out of room: down to just what is needed.
    assert_eq!(trim_level(0, 1, 0), Some(1));
    // A chunk coarser than wanted is the scatter's business, not the trim's.
    assert_eq!(trim_level(5, 2, slack), None);
}

#[test]
fn a_moving_cameras_trail_counts_as_a_few_places() {
    // 90 frames of walking at 5 m/s, then a second view 40 m away.
    let mut cameras: Vec<Vec3> = (0..90)
        .map(|i| Vec3::new(100.0 + i as f32 * 5.0 / 60.0, 2.0, 50.0))
        .collect();
    cameras.push(Vec3::new(140.0, 30.0, 90.0));
    let few = thin(&cameras, 2.0);
    assert!(few.len() <= 5, "{} places", few.len());
    // The latest camera is first, and every camera has a kept one within reach.
    assert_eq!(few[0], *cameras.last().unwrap());
    for c in &cameras {
        assert!(few.iter().any(|f| f.distance(*c) <= 2.0));
    }
}

#[test]
fn pressure_rises_at_once_and_falls_slowly() {
    let budget = 1_000_000;
    let mut p = Pressure::default();
    // Room to spare and no bias: nothing happens.
    for _ in 0..RELAX_AFTER * 3 {
        assert!(!p.settle(100_000, budget));
    }
    assert_eq!(p.bias, 0);
    p.raise(true);
    assert_eq!(p.bias, 1);
    // Not while a finer level (twice the blades) would not fit with room left over.
    for _ in 0..RELAX_AFTER * 40 {
        assert!(!p.settle(450_000, budget));
    }
    assert_eq!(p.bias, 1);
    // With room: a level finer after a wait (twice as long after one strike).
    let mut waited = 0;
    while !p.settle(300_000, budget) {
        waited += 1;
        assert!(waited < RELAX_AFTER * 4);
    }
    assert!(waited >= RELAX_AFTER);
    assert_eq!(p.bias, 0);
    // Running out again and again: each wait is longer than the one before, up to a limit.
    let mut waits = Vec::new();
    for _ in 0..8 {
        p.raise(true);
        let mut waited = 0u32;
        while !p.settle(300_000, budget) {
            waited += 1;
        }
        waits.push(waited);
    }
    assert!(waits.windows(2).all(|w| w[1] >= w[0]), "{waits:?}");
    assert!(waits[7] > waits[0] && waits[7] <= RELAX_AFTER << 5);
    // The bias never passes the coarsest level.
    for _ in 0..20 {
        p.raise(false);
    }
    assert_eq!(p.bias, details::MAX_LEVEL);
    // Over the budget (no chunk was refused): no longer wait afterwards.
    let mut p = Pressure::default();
    p.raise(false);
    let mut waited = 0;
    while !p.settle(300_000, budget) {
        waited += 1;
    }
    assert_eq!(waited + 1, RELAX_AFTER);
}

/// Walk `g`'s camera from `from` to `to` (terrain-local) in steps of `step` metres, every
/// chunk scattered in the frame that asks for it; the most blades held on the way.
fn walk(
    g: &mut Grass,
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    d: &Arc<TerrainData>,
    (from, to): (Vec3, Vec3),
    step: f32,
) -> u64 {
    let steps = (from.distance(to) / step).ceil().max(1.0) as usize;
    let mut most = 0;
    for i in 0..=steps {
        let camera = from.lerp(to, i as f32 / steps as f32);
        g.update(device, queue, d, camera, 1.0);
        most = most.max(g.stats.blades);
    }
    most
}

#[test]
fn a_long_walk_holds_no_more_than_standing_still() {
    let Some((device, queue)) = headless() else {
        return;
    };
    // (Like MeadowDemo's grass at a quarter of its density: full density only close by.)
    let d = meadow(1024.0, 200.0, 6.5, 120.0);
    let eye = |x: f32, z: f32| Vec3::new(x, 1.7, z);
    let standing = |at: Vec3| {
        let mut g = Grass::default();
        g.update(&device, &queue, &d, at, 1.0);
        g
    };
    let (start, far) = (eye(200.0, 200.0), eye(800.0, 760.0));
    let fresh_start = standing(start).load();
    // (The walk runs faster than the clock: only the camera's place of the frame counts.)
    let mut g = Grass {
        camera_memory: std::time::Duration::ZERO,
        ..Default::default()
    };
    let most = walk(&mut g, &device, &queue, &d, (start, far), 1.5);
    let walked = g.load();
    let fresh = standing(far);
    let fresh_far = fresh.load();
    eprintln!(
        "standing {} blades in {} pieces, {} pages; after 820 m {} blades in {} pieces, {} pages (most {most}); standing there {} blades",
        fresh_start.blades,
        fresh_start.pieces,
        fresh_start.pages,
        walked.blades,
        walked.pieces,
        walked.pages,
        fresh_far.blades
    );
    // Every chunk a fresh look from here holds is held at least as fine, and none is much
    // finer than it need be.
    for (k, c) in &fresh.chunks {
        let held = g.chunks.get(k).unwrap_or_else(|| panic!("{k:?} missing"));
        assert!(held.level <= c.level, "{k:?} coarser than needed");
        assert!(
            held.count >= c.count.saturating_sub(2),
            "{k:?} short of blades"
        );
    }
    for (k, c) in &g.chunks {
        let want = wanted(k.0, k.1, far, 6.5, 1.0, 0);
        // (Levels are picked a little nearer than the camera is: a level more at most.)
        assert!(
            c.level + TRIM_SLACK + 1 >= want,
            "{k:?} at level {} where {want} is needed, {} m away",
            c.level,
            reach(k.0, k.1, far)
        );
    }
    // The trail behind the camera costs no more than a little slack: at no point of the walk
    // more than half as many blades again as standing still (it was dozens of times more).
    assert!(
        most as f64 <= fresh_start.blades.max(fresh_far.blades) as f64 * 1.5,
        "{most}"
    );
    assert!(walked.blades as f64 <= fresh_far.blades as f64 * 1.5);
    // The pages hold it without growing past what standing still takes, and nothing was
    // refused or made coarser.
    assert!(walked.page_blades <= fresh_far.page_blades.max(fresh_start.page_blades) * 2);
    assert_eq!((walked.coarser, walked.unculled), (0, 0));
    // Walking on and back again: still the same.
    let most = walk(&mut g, &device, &queue, &d, (far, start), 1.5);
    assert!(
        most as f64 <= fresh_start.blades.max(fresh_far.blades) as f64 * 1.5,
        "{most}"
    );
    assert!(g.load().page_blades <= walked.page_blades);
}

#[test]
fn trimmed_chunks_keep_their_first_blades() {
    let Some((device, queue)) = headless() else {
        return;
    };
    let d = meadow(512.0, 150.0, 8.0, 150.0);
    let k = (10u32, 10u32);
    let centre = Vec3::new(168.0, 1.7, 168.0);
    let mut g = Grass {
        camera_memory: std::time::Duration::ZERO,
        ..Default::default()
    };
    g.update(&device, &queue, &d, centre, 1.0);
    let lo = Vec2::new(k.0 as f32, k.1 as f32) * CHUNK;
    let full = details::scatter_level(&d, lo, lo + CHUNK, 0);
    let (before, _) = held_blades(&g, &device, &queue, k);
    assert_eq!(g.chunks[&k].level, 0);
    assert!(
        before == full,
        "the chunk under the camera holds every blade"
    );
    // Step away: the chunk keeps a prefix, level by level, wherever its run moves to.
    let counts = level_counts(&full, 0);
    let mut last = 0;
    for metres in [14.0, 24.0, 40.0, 70.0, 120.0] {
        let camera = centre + Vec3::new(metres + 8.0, 0.0, 0.0);
        g.update(&device, &queue, &d, camera, 1.0);
        let c = &g.chunks[&k];
        let want = wanted(k.0, k.1, camera, 8.0, 1.0, 0);
        assert!(c.level >= last && c.level + TRIM_SLACK + 1 >= want && c.level <= want);
        assert_eq!(c.count, counts[c.level as usize], "level {}", c.level);
        let (blades, records) = held_blades(&g, &device, &queue, k);
        assert!(
            blades == full[..c.count as usize],
            "{metres} m: blades of level {}",
            c.level
        );
        if !records.is_empty() {
            let expected: Vec<[u32; 4]> = blades.iter().map(record).collect();
            assert!(records == expected, "{metres} m: cull records");
        }
        last = c.level;
    }
    assert!(last >= 4, "trimmed down to level {last}");
    // Coming back: the chunk is scattered afresh, every blade again.
    g.update(&device, &queue, &d, centre, 1.0);
    let (again, _) = held_blades(&g, &device, &queue, k);
    assert!(again == full);
    // Nothing leaked: what the pages count as used is what the chunks hold.
    let load = g.load();
    let used = load.page_blades - load.page_free;
    let rounded: u64 = (g.chunks.values())
        .filter(|c| c.store.is_some())
        .map(|c| (c.count.max(1) as u64).next_multiple_of(64))
        .sum();
    assert_eq!(used, rounded);
    if load.records > 0 {
        assert_eq!(load.records - load.records_free, rounded);
    }
}

#[test]
fn a_field_too_dense_for_its_budget_goes_coarser() {
    let Some((device, queue)) = headless() else {
        return;
    };
    let d = meadow(512.0, 400.0, 20.0, 120.0);
    let at = Vec3::new(250.0, 1.7, 250.0);
    // As it is: everything fits.
    let mut free = Grass::default();
    free.update(&device, &queue, &d, at, 1.0);
    let all = free.load();
    assert_eq!(all.coarser, 0);
    // With room for less than half of that: coarser levels, within the budget.
    let budget = all.blades * 9 / 20;
    let _guard = Budget::of(budget);
    let mut g = Grass {
        camera_memory: std::time::Duration::ZERO,
        ..Default::default()
    };
    for _ in 0..8 {
        g.update(&device, &queue, &d, at, 1.0);
    }
    let tight = g.load();
    eprintln!(
        "{} blades as scattered; budget {budget}: {} blades, {} levels coarser",
        all.blades, tight.blades, tight.coarser
    );
    assert!(
        tight.coarser >= 1 && tight.coarser <= 3,
        "{}",
        tight.coarser
    );
    assert!(
        tight.blades <= budget,
        "{} blades over {budget}",
        tight.blades
    );
    // Nothing went missing: every chunk is still there, only coarser.
    assert_eq!(tight.pieces, all.pieces);
    assert_eq!(tight.unculled, 0);
    // Walking on under the same budget stays within it (a little over while chunks land).
    let most = walk(
        &mut g,
        &device,
        &queue,
        &d,
        (at, at + Vec3::new(120.0, 0.0, 60.0)),
        2.0,
    );
    assert!(most as f64 <= budget as f64 * 1.25, "{most} over {budget}");
    // With room again, the grass goes back to full density after a while.
    set_grass_budget(0);
    let end = at + Vec3::new(120.0, 0.0, 60.0);
    for _ in 0..(RELAX_AFTER << 5) * 4 {
        g.update(&device, &queue, &d, end, 1.0);
        if g.stats.coarser == 0 && g.stats.pending == 0 {
            break;
        }
    }
    assert_eq!(g.load().coarser, 0);
    let mut fresh = Grass::default();
    fresh.update(&device, &queue, &d, end, 1.0);
    for (k, c) in &fresh.chunks {
        assert!(g.chunks.get(k).is_some_and(|h| h.level <= c.level), "{k:?}");
    }
}

#[test]
fn full_pages_refuse_a_chunk_and_the_grass_goes_coarser() {
    let Some((device, queue)) = headless() else {
        return;
    };
    let d = meadow(512.0, 400.0, 20.0, 120.0);
    let at = Vec3::new(250.0, 1.7, 250.0);
    let mut free = Grass::default();
    free.update(&device, &queue, &d, at, 1.0);
    let all = free.load();
    // Pages with room for a third of the field's blades (the budget well above that: only
    // the pages say no).
    let mut pool = crate::render::grass_pool::BladePool::new(&device);
    pool.set_page_limit((all.blades / 3 / 4) as u32);
    let room = pool.room_limit();
    let _guard = Budget::of(u64::MAX / 8);
    let mut g = Grass {
        pool: Some(pool),
        ..Default::default()
    };
    let mut refused = 0;
    for _ in 0..12 {
        g.update(&device, &queue, &d, at, 1.0);
        refused += g.stats.refused;
    }
    let tight = g.load();
    eprintln!(
        "{} blades as scattered; pages for {room}: {} blades, {} levels coarser, {refused} chunks refused",
        all.blades, tight.blades, tight.coarser
    );
    assert!(refused > 0 && tight.coarser >= 1);
    assert!(tight.blades <= room && tight.page_blades <= room);
    // Every chunk is held in the pages with its cull records: the culled lists and the
    // visibility buffer can draw them all.
    assert_eq!(tight.pieces, all.pieces);
    assert_eq!(tight.unculled, 0);
    assert!(g.chunks.values().all(|c| c.count == 0 || c.store.is_some()));
    // Settled: further frames refuse nothing more.
    for _ in 0..4 {
        g.update(&device, &queue, &d, at, 1.0);
        assert_eq!(g.stats.refused, 0);
    }
}

/// MeadowDemo itself (800 blades per square metre, full density up to 6.5 m): the walk that
/// used to fill every blade page within 150 m.
#[test]
fn a_walk_through_meadow_demo_stays_in_its_pages() {
    let Some((device, queue)) = headless() else {
        return;
    };
    let view = crate::render::grass_walk_tests::scene_view("MeadowDemo");
    let d = &view.terrains[0].data;
    let eye = |x: f32, z: f32| Vec3::new(x, d.sample_height(x, z) + 1.7, z);
    let (start, far) = (eye(1022.0, 518.0), eye(1022.0, 918.0));
    let mut standing = Grass::default();
    standing.update(&device, &queue, d, start, 1.0);
    let still = standing.load();
    drop(standing);
    let mut g = Grass {
        camera_memory: std::time::Duration::ZERO,
        ..Default::default()
    };
    let most = walk(&mut g, &device, &queue, d, (start, far), 2.0);
    let walked = g.load();
    eprintln!(
        "MeadowDemo standing: {} blades in {} pieces, {} pages; after 400 m: {} blades in {} pieces, {} pages of {} blades (most held {most})",
        still.blades,
        still.pieces,
        still.pages,
        walked.blades,
        walked.pieces,
        walked.pages,
        walked.page_blades
    );
    // No more than half as many blades again as standing still, at any point of the walk
    // (before: 7.4 million, every page full, after 150 m).
    assert!(
        most as f64 <= still.blades as f64 * 1.5,
        "{most} blades held"
    );
    // Two pages are plenty (four are the limit), nothing was refused or thinned, and every
    // chunk has its cull records.
    assert!(
        walked.pages <= 2 && walked.pages <= still.pages + 1,
        "{} pages",
        walked.pages
    );
    assert_eq!((walked.coarser, walked.unculled), (0, 0));
    // The chunks around the camera hold every blade.
    let under = ((far.x / CHUNK) as u32, (far.z / CHUNK) as u32);
    assert_eq!(g.chunks[&under].level, 0);
}
