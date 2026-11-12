use glam::{IVec2, Vec2, Vec3};

use super::chunks::{self, CHUNK};
use super::collider::{self, Shape};
use super::edit::{self, Brush};
use super::palette::TilePalette;
use super::tile::{self, Neighbor, RuleOutput, RuleTransform, TileAsset, TileKind, TilingRule};
use super::*;
use crate::scene::{Scene, Transform};

const SQUARE: &str = "Builtin/Square.png";
const CIRCLE: &str = "Builtin/Circle.png";
const TRIANGLE: &str = "Builtin/Triangle.png";

/// A tile path registered in memory for one test.
fn tile_path(test: &str, name: &str) -> String {
    format!("Assets/Tests/{test}/{name}.tile")
}

fn plain(test: &str, name: &str, sprite: &str) -> String {
    let p = tile_path(test, name);
    tile::register(&p, TileAsset::sprite(sprite));
    p
}

fn at(x: i32, y: i32) -> IVec2 {
    IVec2::new(x, y)
}

/// Ground Rule Tile: surface (nothing above), left/right edges and a fill sprite.
fn ground_rules(test: &str) -> String {
    let p = tile_path(test, "Ground");
    let mut top = TilingRule::pattern(["?.?", "?x?", "???"], "Top");
    top.collider = ColliderType::Grid;
    let rules = vec![
        // Corners first: nothing above and nothing to one side.
        TilingRule::pattern(["?.?", ".x#", "???"], "TopLeft"),
        TilingRule::pattern(["?.?", "#x.", "???"], "TopRight"),
        top,
        TilingRule::pattern(["???", ".x?", "???"], "Left"),
        TilingRule::pattern(["???", "?x.", "???"], "Right"),
    ];
    tile::register(
        &p,
        TileAsset {
            kind: TileKind::RuleTile,
            sprite: Some("Fill".into()),
            rules,
            ..Default::default()
        },
    );
    p
}

fn sprite_at(map: &Tilemap, c: IVec2) -> Option<String> {
    tile::resolve(map, c, 0.0).and_then(|r| r.sprite)
}

#[test]
fn rule_tile_picks_sprites_from_neighbours() {
    let ground = ground_rules("rules");
    let other = plain("rules", "Other", SQUARE);
    let mut map = Tilemap::default();
    // A 4×2 block of ground.
    for y in 0..2 {
        for x in 0..4 {
            map.set(at(x, y), Some(&ground), TileTransform::default());
        }
    }
    assert_eq!(sprite_at(&map, at(0, 1)).as_deref(), Some("TopLeft"));
    assert_eq!(sprite_at(&map, at(1, 1)).as_deref(), Some("Top"));
    assert_eq!(sprite_at(&map, at(3, 1)).as_deref(), Some("TopRight"));
    assert_eq!(sprite_at(&map, at(0, 0)).as_deref(), Some("Left"));
    assert_eq!(sprite_at(&map, at(3, 0)).as_deref(), Some("Right"));
    assert_eq!(
        sprite_at(&map, at(1, 0)).as_deref(),
        Some("Fill"),
        "default"
    );
    // A different tile above is "not this": the cell below becomes a surface again.
    map.set(at(1, 2), Some(&other), TileTransform::default());
    assert_eq!(sprite_at(&map, at(1, 1)).as_deref(), Some("Top"));
    // The same tile above turns the surface into fill.
    map.set(at(1, 2), Some(&ground), TileTransform::default());
    assert_eq!(sprite_at(&map, at(1, 1)).as_deref(), Some("Fill"));
    // The matched rule's collider type wins over the tile's.
    let r = tile::resolve(&map, at(2, 1), 0.0).unwrap();
    assert_eq!(r.collider, ColliderType::Grid);
}

#[test]
fn rotated_and_mirrored_rules_turn_the_sprite() {
    let p = tile_path("rotated", "Pipe");
    // An end cap pointing right: "this" only on the left.
    let mut cap = TilingRule::pattern([".#.", "#x.", ".#."], "Never");
    cap.neighbors = [Neighbor::NotThis; 8];
    cap.neighbors[3] = Neighbor::This;
    cap.sprites = vec!["Cap".into()];
    cap.transform = RuleTransform::Rotated;
    tile::register(
        &p,
        TileAsset {
            kind: TileKind::RuleTile,
            sprite: Some("Fill".into()),
            rules: vec![cap],
            ..Default::default()
        },
    );
    let mut map = Tilemap::default();
    // Vertical pair: the top one has "this" below = the rule turned 90° counter-clockwise.
    map.set(at(0, 0), Some(&p), TileTransform::default());
    map.set(at(0, 1), Some(&p), TileTransform::default());
    let top = tile::resolve(&map, at(0, 1), 0.0).unwrap();
    assert_eq!(top.sprite.as_deref(), Some("Cap"));
    assert_eq!(
        top.transform.rotation(),
        1,
        "a quarter turn: its left side faces down"
    );
    let bottom = tile::resolve(&map, at(0, 0), 0.0).unwrap();
    assert_eq!(bottom.transform.rotation(), 3);

    // Mirror X: a left-facing rule also matches on the right side, flipped.
    let m = tile_path("rotated", "Mirror");
    let mut edge = TilingRule::pattern(["???", ".x#", "???"], "Edge");
    edge.transform = RuleTransform::MirrorX;
    tile::register(
        &m,
        TileAsset {
            kind: TileKind::RuleTile,
            rules: vec![edge],
            ..Default::default()
        },
    );
    let mut row = Tilemap::default();
    row.set(at(0, 0), Some(&m), TileTransform::default());
    row.set(at(1, 0), Some(&m), TileTransform::default());
    let left = tile::resolve(&row, at(0, 0), 0.0).unwrap();
    let right = tile::resolve(&row, at(1, 0), 0.0).unwrap();
    assert!(!left.transform.flip_x() && right.transform.flip_x());
    assert_eq!(right.sprite.as_deref(), Some("Edge"));
}

#[test]
fn random_and_animated_outputs() {
    let p = tile_path("random", "Grass");
    let mut any = TilingRule::pattern(["???", "?x?", "???"], "A");
    any.sprites = vec!["A".into(), "B".into(), "C".into()];
    any.output = RuleOutput::Random;
    tile::register(
        &p,
        TileAsset {
            kind: TileKind::RuleTile,
            rules: vec![any],
            ..Default::default()
        },
    );
    let mut map = Tilemap::default();
    for x in 0..30 {
        map.set(at(x, 0), Some(&p), TileTransform::default());
    }
    let picks: std::collections::HashSet<String> =
        (0..30).filter_map(|x| sprite_at(&map, at(x, 0))).collect();
    assert_eq!(picks.len(), 3, "all variants appear: {picks:?}");
    assert_eq!(
        sprite_at(&map, at(7, 0)),
        sprite_at(&map, at(7, 0)),
        "stable"
    );

    let water = tile_path("random", "Water");
    tile::register(
        &water,
        TileAsset {
            kind: TileKind::AnimatedTile,
            frames: vec!["W0".into(), "W1".into(), "W2".into()],
            speed: 2.0,
            ..Default::default()
        },
    );
    let mut pool = Tilemap::default();
    pool.set(at(0, 0), Some(&water), TileTransform::default());
    let frame = |pool: &Tilemap, t: f32| tile::resolve(pool, at(0, 0), t).unwrap();
    assert!(frame(&pool, 0.0).animated);
    assert_eq!(frame(&pool, 0.0).sprite.as_deref(), Some("W0"));
    assert_eq!(frame(&pool, 0.6).sprite.as_deref(), Some("W1"));
    assert_eq!(frame(&pool, 1.2).sprite.as_deref(), Some("W2"));
    assert_eq!(frame(&pool, 1.6).sprite.as_deref(), Some("W0"), "loops");
    pool.animation_frame_rate = 2.0;
    assert_eq!(
        frame(&pool, 0.3).sprite.as_deref(),
        Some("W1"),
        "frame rate scales"
    );
}

#[test]
fn transforms_compose() {
    let t = TileTransform::default().rotated(true);
    assert_eq!(t.rotation(), 1);
    assert_eq!(t.rotated(false), TileTransform::default());
    assert_eq!(t.rotated(true).rotated(true).rotated(true).rotation(), 0);
    let f = TileTransform::default().flipped_x();
    assert!(f.flip_x() && !f.flipped_x().flip_x());
    // Flipping both ways is a half turn.
    let both = TileTransform::default().flipped_x().flipped_y();
    let half = TileTransform(2);
    assert!(
        (both.matrix() - half.matrix())
            .abs()
            .to_cols_array()
            .iter()
            .all(|v| *v < 1e-5)
    );
    assert_eq!(TileTransform(1).then(TileTransform(1)).rotation(), 2);
}

#[test]
fn grid_cell_math_round_trips() {
    let g = Grid {
        cell_size: Vec2::new(2.0, 1.0),
        ..Default::default()
    };
    assert_eq!(g.cell_center(at(0, 0)), Vec2::new(1.0, 0.5));
    assert_eq!(g.local_to_cell(Vec2::new(-0.1, 0.2)), at(-1, 0));
    let iso = Grid {
        layout: GridLayout::Isometric,
        cell_size: Vec2::new(1.0, 0.5),
        ..Default::default()
    };
    for c in [at(0, 0), at(3, -2), at(-4, 5)] {
        assert_eq!(g.local_to_cell(g.cell_center(c)), c);
        assert_eq!(iso.local_to_cell(iso.cell_center(c)), c);
    }

    // World ↔ cell through the object's transform.
    let mut s = Scene::new("Cells");
    let grid = s.spawn("Grid", None);
    s.add(grid, Grid::default());
    s.set_transform(grid, Transform::from_position(Vec3::new(10.0, 0.0, 0.0)));
    let tm = s.spawn("Tilemap", Some(grid));
    s.add(tm, Tilemap::default());
    assert_eq!(world_to_cell(&s, tm, Vec3::new(10.5, 2.5, 0.0)), at(0, 2));
    assert_eq!(cell_to_world(&s, tm, at(1, 1)), Vec3::new(11.5, 1.5, 0.0));
    let hit = ray_to_plane(&s, tm, Vec3::new(12.2, 3.7, -10.0), Vec3::Z).unwrap();
    assert_eq!(world_to_cell(&s, tm, hit), at(2, 3));
}

#[test]
fn chunks_batch_by_texture_and_split_animated_tiles() {
    let a = plain("chunks", "A", SQUARE);
    let b = plain("chunks", "B", CIRCLE);
    let water = tile_path("chunks", "Water");
    tile::register(
        &water,
        TileAsset {
            kind: TileKind::AnimatedTile,
            frames: vec![SQUARE.into(), TRIANGLE.into()],
            ..Default::default()
        },
    );
    let mut map = Tilemap::default();
    for x in 0..20 {
        map.set(at(x, 0), Some(&a), TileTransform::default());
    }
    map.set(at(3, 1), Some(&b), TileTransform::default());
    map.set(at(4, 1), Some(&water), TileTransform::default());
    let grid = Grid::default();
    let first = chunks::build_chunk(&map, &grid, at(0, 0));
    assert_eq!(first.meshes.len(), 2, "one mesh per texture");
    let quads: Vec<usize> = first.meshes.iter().map(|m| m.quads()).collect();
    assert_eq!(quads, vec![CHUNK as usize, 1]);
    assert_eq!(first.animated, vec![at(4, 1)]);
    let second = chunks::build_chunk(&map, &grid, at(1, 0));
    assert_eq!(second.meshes[0].quads(), 20 - CHUNK as usize);
    assert_eq!(chunks::chunk_of(at(-1, 0)), at(-1, 0));
    // Quads cover their cells (built-in sprites are one unit, the cell size).
    let (lo, hi) = first.meshes[1].data.bounds();
    assert_eq!(
        (lo, hi),
        (Vec3::new(3.0, 1.0, 0.0), Vec3::new(4.0, 2.0, 0.0))
    );

    // Scene draws: two chunks of A, one mesh of B, one quad for the animated tile.
    let mut s = Scene::new("Chunks");
    let grid_e = s.spawn("Grid", None);
    s.add(grid_e, Grid::default());
    let tm = s.spawn("Tilemap", Some(grid_e));
    s.add(tm, map.clone());
    s.add(tm, TilemapRenderer::default());
    let draws = chunks::scene_draws(&s);
    assert_eq!(draws.len(), 4);
    assert!(
        draws
            .iter()
            .all(|d| d.material.lit_2d && d.material.mode.is_blended())
    );
    let keys: Vec<_> = draws.iter().map(|d| d.mesh.clone()).collect();
    // Unchanged tiles reuse the same chunk meshes; painting rebuilds only that chunk.
    assert_eq!(
        chunks::scene_draws(&s)
            .iter()
            .map(|d| d.mesh.clone())
            .collect::<Vec<_>>(),
        keys
    );
    s.world
        .get::<&mut Tilemap>(tm)
        .unwrap()
        .set(at(19, 0), None, TileTransform::default());
    let after: Vec<_> = chunks::scene_draws(&s)
        .into_iter()
        .map(|d| d.mesh)
        .collect();
    assert_eq!(after[0], keys[0], "chunk 0 untouched");
    assert_ne!(after[3], keys[3], "chunk 1 rebuilt");
    // A change on the border also rebuilds the neighbour (Rule Tiles look across).
    s.world
        .get::<&mut Tilemap>(tm)
        .unwrap()
        .set(at(16, 1), Some(&a), TileTransform::default());
    let border: Vec<_> = chunks::scene_draws(&s)
        .into_iter()
        .map(|d| d.mesh)
        .collect();
    assert_ne!(border[0], after[0], "chunk 0 sees its neighbour's edge");
    // Individual mode: one draw per tile.
    s.world.get::<&mut TilemapRenderer>(tm).unwrap().mode = TilemapRenderMode::Individual;
    assert_eq!(chunks::scene_draws(&s).len(), 22);
}

#[test]
fn collider_merges_cells_into_boxes() {
    let mut cells = Vec::new();
    // A 6×2 floor and a 1×3 pillar on it, apart from a lone cell.
    for y in 0..2 {
        for x in 0..6 {
            cells.push(at(x, y));
        }
    }
    for y in 2..5 {
        cells.push(at(2, y));
    }
    cells.push(at(9, 9));
    let rects = collider::merge(&cells);
    assert_eq!(rects.len(), 3, "{rects:?}");
    assert!(rects.contains(&(at(0, 0), at(5, 1))));
    assert!(rects.contains(&(at(2, 2), at(2, 4))));
    let covered: i32 = rects
        .iter()
        .map(|(lo, hi)| (hi.x - lo.x + 1) * (hi.y - lo.y + 1))
        .sum();
    assert_eq!(covered as usize, cells.len());

    // Through the Tilemap: Collider Type None tiles are skipped, composite off = one box each.
    let solid = plain("collider", "Solid", SQUARE);
    let ghost = tile_path("collider", "Ghost");
    tile::register(
        &ghost,
        TileAsset {
            collider: ColliderType::None,
            ..TileAsset::sprite(SQUARE)
        },
    );
    let mut map = Tilemap::default();
    for c in &cells {
        map.set(*c, Some(&solid), TileTransform::default());
    }
    map.set(at(20, 0), Some(&ghost), TileTransform::default());
    let grid = Grid::default();
    assert_eq!(collider::shapes(&map, &grid, true).len(), 3);
    assert_eq!(collider::shapes(&map, &grid, false).len(), cells.len());
    match &collider::shapes(&map, &grid, true)[0] {
        Shape::Box { min, max } => {
            assert_eq!((*min, *max), (Vec2::ZERO, Vec2::new(6.0, 2.0)));
        }
        s => panic!("{s:?}"),
    }
    // Cell gaps keep cells apart.
    let gapped = Grid {
        cell_gap: Vec2::splat(0.1),
        ..Default::default()
    };
    assert_eq!(collider::shapes(&map, &gapped, true).len(), cells.len());
}

#[test]
fn tilemap_collider_stops_a_falling_body() {
    use crate::physics2d::Physics2D;
    use crate::scene::{BoxCollider2D, Rigidbody2D};
    let solid = plain("physics", "Solid", SQUARE);
    let mut s = Scene::new("Fall");
    let grid = s.spawn("Grid", None);
    s.add(grid, Grid::default());
    let tm = s.spawn("Ground", Some(grid));
    let mut map = Tilemap::default();
    edit::box_fill(
        &mut map,
        at(-5, -2),
        at(5, -1),
        &Brush::single(&solid, Default::default()),
    );
    s.add(tm, map);
    s.add(tm, TilemapCollider2D::default());
    let ball = s.spawn("Box", None);
    s.set_transform(ball, Transform::from_position(Vec3::new(0.5, 3.0, 0.0)));
    s.add(ball, Rigidbody2D::default());
    s.add(
        ball,
        BoxCollider2D {
            size: Vec2::ONE,
            ..Default::default()
        },
    );
    let mut p = Physics2D::build(&s, &Default::default(), &Default::default());
    assert_eq!(p.collider_count(), 2, "one merged compound + the box");
    for _ in 0..180 {
        p.step();
    }
    p.sync_out(&mut s);
    let y = s.world_position(ball).y;
    assert!((y - 0.5).abs() < 0.05, "rests on the tiles: {y}");

    // Erasing tiles while playing rebuilds the collider: the box falls through the hole.
    {
        let mut map = s.world.get::<&mut Tilemap>(tm).unwrap();
        edit::box_erase(&mut map, at(-1, -2), at(1, -1));
    }
    p.refresh_tilemaps(&s);
    for _ in 0..60 {
        p.step();
    }
    p.sync_out(&mut s);
    assert!(s.world_position(ball).y < -1.0, "fell through the hole");
}

/// A box sliding along a tile floor, with no friction, keeps its speed and stays on the floor:
/// it no longer catches on the seams between tiles (round 2's B3: with one box per cell it
/// stopped dead within 8 cells). Both collider modes.
#[test]
fn bodies_slide_across_tile_seams_without_catching() {
    use crate::physics2d::Physics2D;
    use crate::scene::{BoxCollider2D, Collider2DCommon, PhysicsMaterial2D, Rigidbody2D};
    let solid = plain("seams", "Solid", SQUARE);
    for composite in [false, true] {
        let mut s = Scene::new("Slide");
        let grid = s.spawn("Grid", None);
        s.add(grid, Grid::default());
        let tm = s.spawn("Floor", Some(grid));
        let mut map = Tilemap::default();
        // A floor with a step-free top and uneven cells underneath.
        edit::box_fill(
            &mut map,
            at(-2, -1),
            at(60, -1),
            &Brush::single(&solid, Default::default()),
        );
        for x in (-2..60).step_by(3) {
            map.set(at(x, -2), Some(&solid), TileTransform::default());
        }
        s.add(tm, map);
        let ice = Collider2DCommon {
            material: PhysicsMaterial2D {
                friction: 0.0,
                bounciness: 0.0,
            },
            ..Default::default()
        };
        s.add(
            tm,
            TilemapCollider2D {
                common: ice,
                used_by_composite: composite,
            },
        );
        let slider = s.spawn("Slider", None);
        s.set_transform(slider, Transform::from_position(Vec3::new(0.5, 0.5, 0.0)));
        s.add(
            slider,
            Rigidbody2D {
                freeze_rotation: true,
                ..Default::default()
            },
        );
        s.add(
            slider,
            BoxCollider2D {
                common: ice,
                size: Vec2::ONE,
                ..Default::default()
            },
        );
        let mut p = Physics2D::build(&s, &Default::default(), &Default::default());
        for _ in 0..30 {
            p.step();
        }
        p.set_velocity(slider, Vec2::new(12.0, 0.0));
        let (mut slowest, mut highest) = (f32::MAX, f32::MIN);
        for _ in 0..200 {
            p.step();
            let v = p.velocity(slider).unwrap();
            slowest = slowest.min(v.x);
            highest = highest.max(v.y.abs());
        }
        p.sync_out(&mut s);
        let x = s.world_position(slider).x;
        assert!(
            slowest > 11.5 && highest < 0.5,
            "composite {composite}: caught on a seam (slowest {slowest:.2} m/s, vertical {highest:.2} m/s, at x {x:.1})"
        );
        assert!(x > 35.0, "composite {composite}: slid to {x}");
    }
}

/// Two scenes drawn in turn whose tilemaps have the same entity id but different tiles keep
/// their chunk meshes: no chunk is rebuilt after the first frame of each (round 2's B6: the
/// cache was keyed by entity id alone, so each scene rebuilt the other's chunks every frame).
#[test]
fn two_scenes_with_the_same_tilemap_id_keep_their_chunks() {
    let solid = plain("two_scenes", "Solid", SQUARE);
    let scene = |cells: &[IVec2]| {
        let mut s = Scene::new("Map");
        let grid = s.spawn("Grid", None);
        s.add(grid, Grid::default());
        let tm = s.spawn("Tiles", Some(grid));
        let mut map = Tilemap::default();
        for c in cells {
            map.set(*c, Some(&solid), TileTransform::default());
        }
        s.add(tm, map);
        s.add(tm, TilemapRenderer::default());
        (s, tm)
    };
    // The same chunk with different tiles, and a chunk only the second scene has.
    let (a, ta) = scene(&[at(0, 0), at(1, 0)]);
    let (b, tb) = scene(&[at(0, 0), at(2, 3), at(CHUNK * 3, 1)]);
    assert_eq!(ta, tb, "same entity ids");
    let builds = || chunks::BUILDS.with(|b| b.get());
    let before = builds();
    for _ in 0..10 {
        assert!(!chunks::scene_draws(&a).is_empty());
        assert!(!chunks::scene_draws(&b).is_empty());
    }
    assert_eq!(builds() - before, 3, "one build per chunk of each scene");
    // Editing still rebuilds.
    let (c, _) = scene(&[at(0, 0)]);
    chunks::scene_draws(&c);
    assert_eq!(builds() - before, 4);
}

#[test]
fn paint_erase_and_flood_fill() {
    let a = plain("paint", "A", SQUARE);
    let b = plain("paint", "B", CIRCLE);
    let brush = Brush::single(&a, TileTransform::default());
    let mut map = Tilemap::default();
    assert_eq!(edit::paint(&mut map, at(0, 0), &brush), 1);
    assert_eq!(edit::paint(&mut map, at(0, 0), &brush), 0, "already there");
    assert_eq!(edit::paint_line(&mut map, at(0, 0), at(4, 2), &brush), 4);
    assert_eq!(edit::line(at(0, 0), at(4, 2)).len(), 5);
    // Box fill 5×3, then erase a column.
    assert_eq!(edit::box_fill(&mut map, at(0, 0), at(4, 2), &brush), 10);
    assert_eq!(map.len(), 15);
    assert_eq!(edit::box_erase(&mut map, at(2, 0), at(2, 2)), 3);
    // Flood fill the left part only (the erased column splits the region).
    assert_eq!(
        edit::flood_fill(&mut map, at(0, 0), Some(&b), Default::default()),
        6
    );
    assert_eq!(map.tile_at(at(1, 2)), Some(b.as_str()));
    assert_eq!(map.tile_at(at(3, 0)), Some(a.as_str()));
    // Flood filling empty cells stays inside the painted bounds.
    assert_eq!(
        edit::flood_fill(&mut map, at(2, 1), Some(&a), Default::default()),
        3
    );
    assert_eq!(map.len(), 15);
    // Flood erase.
    assert_eq!(
        edit::flood_fill(&mut map, at(4, 2), None, Default::default()),
        9
    );
    assert_eq!(map.len(), 6);

    // Pick a block and stamp it elsewhere; move a block.
    let picked = edit::pick(&map, at(0, 0), at(1, 1));
    assert_eq!(picked.size(), at(2, 2));
    assert_eq!(edit::paint(&mut map, at(10, 10), &picked), 4);
    assert_eq!(map.tile_at(at(11, 11)), Some(b.as_str()));
    assert_eq!(
        edit::move_cells(&mut map, at(10, 10), at(11, 11), at(0, 5)),
        8
    );
    assert!(map.get(at(10, 10)).is_none() && map.get(at(10, 15)).is_some());

    // Rotating and flipping a brush.
    let turned = brush.rotated(true);
    assert_eq!(turned.cells[0].transform.rotation(), 1);
    let line_brush = edit::pick(&map, at(0, 0), at(1, 0));
    let vertical = line_brush.rotated(true);
    assert_eq!(vertical.size(), at(1, 2));
    assert!(brush.flipped(true).cells[0].transform.flip_x());
}

#[test]
fn palette_round_trips_on_disk() {
    let dir = std::env::temp_dir().join("misty_tilemap_palette");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let mut p = TilePalette::default();
    assert_eq!(p.add("Assets/Tiles/A.tile"), at(0, 0));
    assert_eq!(p.add("Assets/Tiles/B.tile"), at(1, 0));
    assert_eq!(p.add("Assets/Tiles/A.tile"), at(0, 0), "no duplicates");
    p.set(at(3, 2), Some("Assets/Tiles/C.tile"));
    assert_eq!(p.extent(), at(4, 3));
    let path = dir.join("Ground.palette");
    p.save(&path).unwrap();
    let back = TilePalette::load(&path).unwrap();
    assert_eq!(back, p);
    assert_eq!(back.get(at(3, 2)), Some("Assets/Tiles/C.tile"));
    p.set(at(0, 0), None);
    assert_eq!(p.next_free(), at(0, 0));
    assert!(palette::is_palette("A/b.palette") && tile::is_tile("A/b.TILE"));

    // Tile assets round-trip too.
    let tile_file = dir.join("Ground.tile");
    let asset = TileAsset {
        kind: TileKind::RuleTile,
        sprite: Some(SQUARE.into()),
        rules: vec![TilingRule::pattern(["#.#", "?x?", "..."], CIRCLE)],
        ..Default::default()
    };
    asset.save(&tile_file).unwrap();
    assert_eq!(TileAsset::load(&tile_file).unwrap(), asset);
    let key = tile_file.to_string_lossy().replace('\\', "/");
    assert_eq!(tile::get(&key).as_deref(), Some(&asset));
    assert_eq!(asset.preview_sprite(), Some(SQUARE));
}

#[test]
fn scene_round_trip_keeps_tilemaps() {
    let a = plain("scene", "A", SQUARE);
    let b = plain("scene", "B", CIRCLE);
    let mut s = Scene::new("Tiles");
    let grid = s.spawn("Grid", None);
    s.add(
        grid,
        Grid {
            cell_size: Vec2::new(1.0, 0.5),
            ..Default::default()
        },
    );
    let tm = s.spawn("Tilemap", Some(grid));
    let mut map = Tilemap {
        color: [0.5, 1.0, 1.0, 1.0],
        ..Default::default()
    };
    map.set(at(-3, 2), Some(&a), TileTransform(5));
    map.set(at(4, 0), Some(&b), TileTransform::default());
    map.set(at(5, 0), Some(&a), TileTransform::default());
    s.add(tm, map.clone());
    s.add(
        tm,
        TilemapRenderer {
            sorting_layer: "Ground".into(),
            order_in_layer: 3,
            ..Default::default()
        },
    );
    s.add(tm, TilemapCollider2D::default());
    s.add(tm, crate::light2d::ShadowCaster2D::default());
    let lamp = s.spawn("Lamp", None);
    s.add(
        lamp,
        crate::light2d::Light2D {
            shadows_enabled: true,
            target_sorting_layers: Some(vec!["Ground".into()]),
            ..Default::default()
        },
    );
    let json = s.to_json();
    // Compact: tile paths are listed once.
    assert_eq!(json.matches("scene/A.tile").count(), 1, "{json}");
    let (back, report) = Scene::from_json("Tiles", &json).unwrap();
    assert!(report.warnings.is_empty(), "{:?}", report.warnings);
    assert_eq!(back.to_json(), json);
    let e = back.find_by_name("Tilemap").unwrap();
    assert_eq!(*back.world.get::<&Tilemap>(e).unwrap(), map);
    assert_eq!(
        back.world
            .get::<&Tilemap>(e)
            .unwrap()
            .get(at(-3, 2))
            .unwrap()
            .transform,
        TileTransform(5)
    );
}

#[test]
fn drags_far_outside_a_tiny_grid_stay_bounded() {
    let g = Grid {
        cell_size: Vec2::splat(1e-6),
        ..Default::default()
    };
    let far = g.local_to_cell(Vec2::new(1e30, -1e30));
    assert_eq!(far, at(Grid::CELL_LIMIT, -Grid::CELL_LIMIT));
    let mut map = Tilemap::default();
    let brush = Brush::single("Assets/T.asset", TileTransform::default());
    let start = std::time::Instant::now();
    assert_eq!(edit::box_fill(&mut map, -far, far, &brush), 0);
    assert!(edit::pick(&map, -far, far).cells.is_empty());
    assert_eq!(edit::line(at(i32::MIN, 0), at(i32::MAX, 0)).len(), 1);
    assert!(start.elapsed().as_secs() < 5, "{:?}", start.elapsed());
    // Ordinary boxes still fill.
    assert_eq!(edit::box_fill(&mut map, at(0, 0), at(2, 1), &brush), 6);
}

#[test]
fn sprite_colliders_follow_the_tile_rotation() {
    // A 1×0.5 unit sprite (100×50 pixels at 100 pixels per unit) on a 1×1 cell.
    let dir = std::env::temp_dir().join(format!("misty_tile_rot_{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let png = dir.join("Wide.png");
    image::RgbaImage::from_pixel(100, 50, image::Rgba([255; 4]))
        .save(&png)
        .unwrap();
    let wide = png.to_string_lossy().replace('\\', "/");
    let p = tile_path("sprite_rot", "Wide");
    tile::register(
        &p,
        TileAsset {
            collider: ColliderType::Sprite,
            ..TileAsset::sprite(&wide)
        },
    );
    let mut map = Tilemap::default();
    map.set(at(0, 0), Some(&p), TileTransform::default());
    map.set(at(3, 0), Some(&p), TileTransform::default().rotated(true));
    let shapes = collider::shapes(&map, &Grid::default(), false);
    let boxes: Vec<(Vec2, Vec2)> = shapes
        .iter()
        .map(|s| match s {
            Shape::Box { min, max } => (*min, *max),
            s => panic!("{s:?}"),
        })
        .collect();
    let close = |a: Vec2, b: Vec2| (a - b).abs().max_element() < 1e-4;
    assert!(
        boxes
            .iter()
            .any(|(lo, hi)| close(*lo, Vec2::new(0.0, 0.25)) && close(*hi, Vec2::new(1.0, 0.75))),
        "{boxes:?}"
    );
    assert!(
        boxes
            .iter()
            .any(|(lo, hi)| close(*lo, Vec2::new(3.25, 0.0)) && close(*hi, Vec2::new(3.75, 1.0))),
        "the turned tile stands upright: {boxes:?}"
    );
    let _ = std::fs::remove_dir_all(dir);
}
