use glam::IVec2;
use misty_engine::scene::Scene;
use misty_engine::tilemap::edit::Brush;
use misty_engine::tilemap::tile::{self, TileAsset};
use misty_engine::tilemap::{GridLayout, Tilemap};

use super::*;
use crate::project::ProjectBrowser;

fn state(test: &str) -> EditorState {
    let root = std::env::temp_dir().join(format!("misty_tile_palette_{test}"));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(root.join("Assets")).unwrap();
    let mut s = EditorState::new(ProjectBrowser::open(root));
    s.scene = Scene::new("Tiles");
    s
}

fn tiles(s: &EditorState, e: Entity) -> Tilemap {
    (*s.scene.world.get::<&Tilemap>(e).unwrap()).clone()
}

#[test]
fn a_stroke_is_one_undo_step() {
    let mut s = state("stroke");
    let e = create_tilemap(&mut s, None, GridLayout::Rectangle);
    assert!(
        s.scene
            .world
            .get::<&Grid>(s.scene.parent(e).unwrap())
            .is_ok()
    );
    let path = "Assets/Tiles/Stroke.tile";
    tile::register(path, TileAsset::sprite("Builtin/Square.png"));
    s.tile_palette.brush = Brush::single(path, Default::default());
    let before = s.undo.steps().len();
    // One stroke dragged over three segments.
    let mut recorded = false;
    stroke(
        &mut s,
        e,
        IVec2::new(0, 0),
        IVec2::new(0, 0),
        PaletteTool::Paint,
        false,
        &mut recorded,
    );
    stroke(
        &mut s,
        e,
        IVec2::new(0, 0),
        IVec2::new(3, 0),
        PaletteTool::Paint,
        false,
        &mut recorded,
    );
    stroke(
        &mut s,
        e,
        IVec2::new(3, 0),
        IVec2::new(3, 2),
        PaletteTool::Paint,
        false,
        &mut recorded,
    );
    assert_eq!(tiles(&s, e).len(), 6);
    assert_eq!(s.undo.steps().len(), before + 1, "{:?}", s.undo.steps());
    assert_eq!(s.undo.undo_label(), Some("Paint Tiles"));
    // Painting where tiles already are changes nothing and records nothing.
    let mut again = false;
    stroke(
        &mut s,
        e,
        IVec2::new(0, 0),
        IVec2::new(3, 0),
        PaletteTool::Paint,
        false,
        &mut again,
    );
    assert!(!again && s.undo.steps().len() == before + 1);
    // Shift erases with the same stroke.
    let mut erase = false;
    stroke(
        &mut s,
        e,
        IVec2::new(0, 0),
        IVec2::new(1, 0),
        PaletteTool::Paint,
        true,
        &mut erase,
    );
    assert_eq!(tiles(&s, e).len(), 4);
    assert_eq!(s.undo.undo_label(), Some("Erase Tiles"));
    s.undo_step();
    assert_eq!(tiles(&s, e).len(), 6, "erase undone");
    s.undo_step();
    assert!(tiles(&s, e).is_empty(), "the whole stroke undone at once");
    s.redo_step();
    assert_eq!(tiles(&s, e).len(), 6);

    // Flood fill and box fill are one step each.
    let other = "Assets/Tiles/Other.tile";
    tile::register(other, TileAsset::sprite("Builtin/Circle.png"));
    let n = edit_map(&mut s, e, "Flood Fill Tiles", None, |m| {
        edit::flood_fill(m, IVec2::new(0, 0), Some(other), Default::default())
    });
    assert_eq!(n, 6);
    assert_eq!(s.undo.undo_label(), Some("Flood Fill Tiles"));
    s.undo_step();
    assert_eq!(tiles(&s, e).tile_at(IVec2::new(0, 0)), Some(path));
}

#[test]
fn creates_assets_and_fills_palettes_from_dropped_sprites() {
    use misty_engine::image::{Rgba, RgbaImage};
    use misty_engine::sprites::slice::{self, GridSlice};
    use misty_engine::sprites::{SpriteMode, TextureImporter};

    let mut s = state("drop");
    let root = s.project.root.clone();
    let dir = root.join("Assets");
    let tile_path = create_asset(&dir, TileCreate::RuleTile).unwrap();
    let rule = TileAsset::load(&tile_path).unwrap();
    assert_eq!(rule.kind, TileKind::RuleTile);
    let pal = create_asset(&dir, TileCreate::Palette).unwrap();
    assert!(pal.extension().is_some_and(|e| e == "palette"));
    let pal_rel = crate::asset_ops::rel(&root, &pal);
    s.tile_palette.open(&root, &pal_rel);

    // A 2-sprite sheet dropped on the palette: two tiles made and added.
    let sheet = dir.join("Sheet.png");
    RgbaImage::from_pixel(32, 16, Rgba([200, 100, 50, 255]))
        .save(&sheet)
        .unwrap();
    let grid = GridSlice {
        cell: [16, 16],
        skip_empty: false,
        ..Default::default()
    };
    TextureImporter {
        sprite_mode: SpriteMode::Multiple,
        sprites: slice::grid(32, 16, &grid, "Sheet", None),
        ..TextureImporter::sprite()
    }
    .save(&sheet)
    .unwrap();
    // Absolute texture path: tests leave the process-wide project root alone.
    let sheet_ref = sheet.to_string_lossy().replace('\\', "/");
    misty_engine::sprites::invalidate(&sheet_ref);
    let added = drop_on_palette(&mut s, &sheet_ref, None);
    assert_eq!(added.len(), 2, "{added:?}");
    assert_eq!(s.tile_palette.data.tiles.len(), 2);
    let back = TilePalette::load(&pal).unwrap();
    assert_eq!(back, s.tile_palette.data, "saved");
    let t = TileAsset::load(&root.join(&added[1])).unwrap();
    assert_eq!(t.sprite, Some(format!("{sheet_ref}#Sheet_1")));
    // Dropping it again reuses the tiles; a .tile is added as it is.
    let again = drop_on_palette(&mut s, &sheet_ref, None);
    assert_eq!(again, added);
    assert_eq!(s.tile_palette.data.tiles.len(), 2);
    let rel = crate::asset_ops::rel(&root, &tile_path);
    drop_on_palette(&mut s, &rel, Some(IVec2::new(5, 3)));
    assert_eq!(
        s.tile_palette.data.get(IVec2::new(5, 3)),
        Some(rel.as_str())
    );
    // Clicking a palette cell makes it the brush.
    s.tile_palette.pick_palette(IVec2::new(5, 3));
    assert_eq!(s.tile_palette.brush.first_tile(), Some(rel.as_str()));
    s.tile_palette.scan(&root, true);
    assert_eq!(s.tile_palette.palettes, vec![pal_rel]);
}
