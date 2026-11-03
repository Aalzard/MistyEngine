//! Procedural Platformer2D scene in `SampleProject`: pixel-art sprites made in code (player, a
//! sliced tile sheet, a coin) imported as Point-filtered sprites at 16 pixels per unit, tile
//! platforms with Box Collider 2D, a player with a Rigidbody 2D moved by `PlayerController2D.lua`
//! (A/D to run, Space to jump using a ground raycast), coins as triggers that disappear and
//! count up a UI Text, and an orthographic camera following the player (`CameraFollow2D.lua`).
//! Regenerate with `cargo test -p misty_editor -- --ignored write_platformer_2d`; the other tests
//! check the committed files still match and that the scene plays.

use std::path::PathBuf;

use glam::{Vec2, Vec3};
use misty_engine::hecs::Entity;
use misty_engine::image::{Rgba, RgbaImage};
use misty_engine::scene::{
    BoxCollider2D, Camera, CircleCollider2D, ClearFlags, Collider2DCommon, PhysicsMaterial2D,
    RectTransform, Rigidbody2D, Scene, ScriptRef, Scripts, SpriteRenderer, Transform, UiText,
};
use misty_engine::sprites::slice::{self, GridSlice};
use misty_engine::sprites::{FilterMode, SpriteMode, TextureImporter, builtin};

use crate::actions::{Action, Create};
use crate::project::ProjectBrowser;
use crate::state::EditorState;
use crate::ui_create::UiElement;

const SCENE: &str = "Assets/Scenes/Platformer2D.scene";
const PLAYER_PNG: &str = "Assets/Sprites/Player.png";
const TILES_PNG: &str = "Assets/Sprites/Tiles.png";
const COIN_PNG: &str = "Assets/Sprites/Coin.png";
const CONTROLLER: &str = "Assets/Scripts/PlayerController2D.lua";
const FOLLOW: &str = "Assets/Scripts/CameraFollow2D.lua";
const PPU: f32 = 16.0;
const PLAYER_LAYER: u8 = 5;
const IGNORE_RAYCAST_LAYER: u8 = 2;

const CONTROLLER_LUA: &str = r#"-- PlayerController2D: A/D (or the arrow keys) run, Space jumps while standing on something.
-- Ground check: three short rays down from the feet against the Default layer.
-- Coins are triggers: touching one destroys it and updates the "Score Text" UI Text.
local PlayerController2D = {
    speed = 6,
    jumpSpeed = 14,
    groundDistance = 0.1,
}

function PlayerController2D:Start()
    self.rb = self.gameObject:GetComponent("Rigidbody2D")
    self.sprite = self.gameObject:GetComponent("SpriteRenderer")
    self.ground = LayerMask.GetMask("Default")
    self.coins = 0
    self.jump = false
    local label = GameObject.Find("Score Text")
    if label ~= nil then
        self.label = label:GetComponent("Text")
    end
end

function PlayerController2D:IsGrounded()
    local p = self.transform.position
    for _, dx in ipairs({ -0.3, 0, 0.3 }) do
        local feet = Vector2.new(p.x + dx, p.y - 0.45)
        if Physics2D.Raycast(feet, Vector2.down, self.groundDistance, self.ground) ~= nil then
            return true
        end
    end
    return false
end

function PlayerController2D:Update()
    if Input.GetKeyDown("space") then
        self.jump = true
    end
end

function PlayerController2D:FixedUpdate()
    local h = Input.GetAxis("Horizontal")
    local v = self.rb.velocity
    local vy = v.y
    if self.jump and self:IsGrounded() then
        vy = self.jumpSpeed
    end
    self.jump = false
    self.rb.velocity = Vector2.new(h * self.speed, vy)
    if h ~= 0 then
        self.sprite.flipX = h < 0
    end
end

function PlayerController2D:OnTriggerEnter2D(other)
    if other.name:sub(1, 4) == "Coin" then
        Destroy(other.gameObject)
        self.coins = self.coins + 1
        if self.label ~= nil then
            self.label.text = "Coins: " .. self.coins
        end
    end
end

return PlayerController2D
"#;

const FOLLOW_LUA: &str = r#"-- CameraFollow2D: eases the camera towards the target in X/Y, keeping its own depth.
local CameraFollow2D = {
    target = "Player",
    smoothing = 6,
    height = 1,
}

function CameraFollow2D:Start()
    self.followed = GameObject.Find(self.target)
end

function CameraFollow2D:LateUpdate()
    if self.followed == nil then
        return
    end
    local p = self.transform.position
    local t = self.followed.transform.position
    local k = Mathf.Clamp(self.smoothing * Time.deltaTime, 0, 1)
    local x = p.x + (t.x - p.x) * k
    local y = p.y + (t.y + self.height - p.y) * k
    self.transform.position = Vector3.new(x, y, p.z)
end

return CameraFollow2D
"#;

fn project() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../SampleProject")
}

fn rgba(hex: u32) -> Rgba<u8> {
    let b = hex.to_be_bytes();
    Rgba([b[1], b[2], b[3], 255])
}

/// 16×16 hero: dark outline, blue suit, skin face with an eye looking right.
fn player_png() -> RgbaImage {
    const ART: [&str; 16] = [
        "................",
        ".....oooooo.....",
        "....osssssso....",
        "....ossssswo....",
        "....osssssko....",
        "....osssssso....",
        ".....oooooo.....",
        "...oobbbbbboo...",
        "..obbbbbbbbbbo..",
        "..obobbbbbbobo..",
        "..ososbbbbsoso..",
        "....obbbbbbo....",
        "....obboobbo....",
        "....obbo.obbo...",
        "...ooooo.ooooo..",
        "................",
    ];
    pixel_art(&ART, |c| match c {
        'o' => Some(rgba(0x1b1b2f)),
        's' => Some(rgba(0xf2c29b)),
        'w' => Some(rgba(0xffffff)),
        'k' => Some(rgba(0x222222)),
        'b' => Some(rgba(0x3b6fd8)),
        _ => None,
    })
}

/// 8×8-cell pattern scaled into a 16×16 coin: gold disc, dark rim, light glint.
fn coin_png() -> RgbaImage {
    const ART: [&str; 16] = [
        "................",
        ".....oooooo.....",
        "...ooyyyyyyoo...",
        "..oyyyhhyyyyyo..",
        "..oyyhhyyyyyyo..",
        ".oyyhyyyddyyyyo.",
        ".oyyyyyyddyyyyo.",
        ".oyyyyyyddyyyyo.",
        ".oyyyyyyddyyyyo.",
        ".oyyyyyyddyyyyo.",
        ".oyyyyyyddyyyyo.",
        "..oyyyyyyyyyyo..",
        "..oyyyyyyyyyyo..",
        "...ooyyyyyyoo...",
        ".....oooooo.....",
        "................",
    ];
    pixel_art(&ART, |c| match c {
        'o' => Some(rgba(0x8a5a00)),
        'y' => Some(rgba(0xffc62e)),
        'h' => Some(rgba(0xfff3b0)),
        'd' => Some(rgba(0xd99a00)),
        _ => None,
    })
}

/// 32×16 sheet: grass-topped tile, then a plain dirt tile.
fn tiles_png() -> RgbaImage {
    RgbaImage::from_fn(32, 16, |x, y| {
        let (tile, lx) = (x / 16, x % 16);
        // A few deterministic pebbles.
        let pebble = (lx * 7 + y * 13 + tile * 5) % 23 == 0;
        let dirt = if pebble {
            rgba(0x6b4423)
        } else {
            rgba(0x9b6a3c)
        };
        if tile == 0 && y < 4 {
            // Grass with a jagged lower edge.
            if y == 3 && lx % 3 == 1 {
                dirt
            } else if y == 0 {
                rgba(0x7ad957)
            } else {
                rgba(0x4caf3c)
            }
        } else {
            dirt
        }
    })
}

fn pixel_art(rows: &[&str], color: impl Fn(char) -> Option<Rgba<u8>>) -> RgbaImage {
    RgbaImage::from_fn(rows[0].len() as u32, rows.len() as u32, |x, y| {
        let c = rows[y as usize].as_bytes()[x as usize] as char;
        color(c).unwrap_or(Rgba([0, 0, 0, 0]))
    })
}

/// Pixel-art sprite import: Nearest Neighbor filter, 16 pixels per unit.
fn importer(sprites: Vec<misty_engine::sprites::SpriteRect>) -> TextureImporter {
    TextureImporter {
        pixels_per_unit: PPU,
        filter_mode: FilterMode::Nearest,
        sprite_mode: if sprites.is_empty() {
            SpriteMode::Single
        } else {
            SpriteMode::Multiple
        },
        sprites,
        ..TextureImporter::sprite()
    }
}

fn textures() -> Vec<(&'static str, RgbaImage, TextureImporter)> {
    let grid = GridSlice {
        cell: [16, 16],
        skip_empty: false,
        ..Default::default()
    };
    vec![
        (PLAYER_PNG, player_png(), importer(Vec::new())),
        (COIN_PNG, coin_png(), importer(Vec::new())),
        (
            TILES_PNG,
            tiles_png(),
            importer(slice::grid(32, 16, &grid, "Tiles", None)),
        ),
    ]
}

fn script(path: &str) -> Scripts {
    Scripts {
        list: vec![ScriptRef {
            path: path.into(),
            ..Default::default()
        }],
    }
}

fn sprite(s: &mut Scene, e: Entity, sprite: String, order: i32) {
    s.add(
        e,
        SpriteRenderer {
            sprite: Some(sprite),
            order_in_layer: order,
            ..Default::default()
        },
    );
}

/// A solid block of tiles `w`×`h` with its top-left tile centre at (x, y): one Box Collider 2D
/// on the parent, one sprite per tile (grass on the top row).
fn platform(s: &mut Scene, name: &str, x: f32, y: f32, w: u32, h: u32) {
    let center = Vec2::new(x + (w as f32 - 1.0) * 0.5, y - (h as f32 - 1.0) * 0.5);
    let p = s.spawn(name, None);
    s.set_transform(p, Transform::from_position(center.extend(0.0)));
    s.add(
        p,
        BoxCollider2D {
            size: Vec2::new(w as f32, h as f32),
            ..Default::default()
        },
    );
    s.game_object_mut(p).is_static = true;
    for row in 0..h {
        for col in 0..w {
            let tile = s.spawn(&format!("Tile {col},{row}"), Some(p));
            let at = Vec2::new(x + col as f32, y - row as f32) - center;
            s.set_transform(tile, Transform::from_position(at.extend(0.0)));
            let which = if row == 0 { 0 } else { 1 };
            sprite(s, tile, format!("{TILES_PNG}#Tiles_{which}"), 0);
        }
    }
}

fn coin(s: &mut Scene, i: usize, at: Vec2) {
    let c = s.spawn(&format!("Coin {i}"), None);
    s.set_transform(c, Transform::from_position(at.extend(0.0)));
    sprite(s, c, COIN_PNG.into(), 1);
    s.add(
        c,
        CircleCollider2D {
            common: Collider2DCommon {
                is_trigger: true,
                ..Default::default()
            },
            radius: 0.35,
        },
    );
    // Ground rays only look at Default, so coins never count as ground.
    s.game_object_mut(c).layer = IGNORE_RAYCAST_LAYER;
}

/// Background shape from a built-in sprite, tinted and drawn behind everything.
fn backdrop(s: &mut Scene, name: &str, shape: &str, at: Vec3, size: f32, color: [f32; 4]) {
    let e = s.spawn(name, None);
    let mut t = Transform::from_position(at);
    t.scale = Vec3::new(size, size, 1.0);
    s.set_transform(e, t);
    s.add(
        e,
        SpriteRenderer {
            sprite: Some(builtin::path(shape)),
            color,
            order_in_layer: -10,
            ..Default::default()
        },
    );
}

fn create(s: &mut EditorState, what: Create) -> Entity {
    crate::commands::apply(s, Action::Create { what, parent: None });
    s.selection.active().expect("created object is selected")
}

fn demo_scene() -> Scene {
    let mut st = EditorState::new(ProjectBrowser::open(
        std::env::temp_dir().join("misty_platformer_build"),
    ));
    st.scene = Scene::new("Platformer2D");
    let s = &mut st.scene;

    let cam = s.spawn("Main Camera", None);
    s.game_object_mut(cam).tag = "MainCamera".into();
    s.set_transform(cam, Transform::from_position(Vec3::new(-5.0, 1.5, -10.0)));
    s.add(
        cam,
        Camera {
            clear_flags: ClearFlags::SolidColor,
            background: [0.42, 0.72, 0.95, 1.0],
            orthographic: true,
            ortho_size: 5.0,
            ..Default::default()
        },
    );
    s.add(cam, script(FOLLOW));

    backdrop(
        s,
        "Sun",
        "Circle",
        Vec3::new(7.0, 6.0, 5.0),
        2.5,
        [1.0, 0.93, 0.55, 1.0],
    );
    backdrop(
        s,
        "Hill 1",
        "Circle",
        Vec3::new(-4.0, -1.5, 5.0),
        9.0,
        [0.55, 0.78, 0.45, 1.0],
    );
    backdrop(
        s,
        "Hill 2",
        "Circle",
        Vec3::new(6.0, -2.5, 5.0),
        11.0,
        [0.48, 0.7, 0.4, 1.0],
    );

    platform(s, "Ground", -9.0, -0.5, 22, 2);
    platform(s, "Wall", -9.0, 3.5, 1, 4);
    platform(s, "Platform A", 4.0, 2.0, 4, 1);
    platform(s, "Platform B", 9.0, 4.0, 3, 1);
    for (i, at) in [
        Vec2::new(-3.0, 0.5),
        Vec2::new(-2.0, 0.5),
        Vec2::new(-1.0, 0.5),
        Vec2::new(5.5, 3.1),
        Vec2::new(10.0, 5.1),
    ]
    .into_iter()
    .enumerate()
    {
        coin(s, i + 1, at);
    }

    let player = s.spawn("Player", None);
    s.game_object_mut(player).layer = PLAYER_LAYER;
    s.set_transform(player, Transform::from_position(Vec3::new(-6.0, 0.5, 0.0)));
    sprite(s, player, PLAYER_PNG.into(), 5);
    s.add(
        player,
        Rigidbody2D {
            gravity_scale: 3.0,
            freeze_rotation: true,
            ..Default::default()
        },
    );
    s.add(
        player,
        BoxCollider2D {
            // No friction, so the player slides along walls instead of sticking to them.
            common: Collider2DCommon {
                material: PhysicsMaterial2D {
                    friction: 0.0,
                    bounciness: 0.0,
                },
                ..Default::default()
            },
            size: Vec2::new(0.7, 0.95),
            ..Default::default()
        },
    );
    s.add(player, script(CONTROLLER));

    // Coin counter in the top-left corner.
    let text = create(&mut st, Create::Ui(UiElement::Text));
    st.scene.game_object_mut(text).name = "Score Text".into();
    let w = &mut st.scene.world;
    *w.get::<&mut RectTransform>(text).unwrap() = RectTransform {
        anchor_min: Vec2::new(0.0, 1.0),
        anchor_max: Vec2::new(0.0, 1.0),
        pivot: Vec2::new(0.0, 1.0),
        anchored_position: Vec2::new(20.0, -16.0),
        size_delta: Vec2::new(300.0, 44.0),
    };
    {
        let mut t = w.get::<&mut UiText>(text).unwrap();
        t.text = "Coins: 0".into();
        t.font_size = 30.0;
        t.color = [1.0; 4];
    }
    st.scene
}

fn scripts() -> [(&'static str, &'static str); 2] {
    [(CONTROLLER, CONTROLLER_LUA), (FOLLOW, FOLLOW_LUA)]
}

#[test]
#[ignore = "writes into SampleProject; run on purpose to regenerate"]
fn write_platformer_2d() {
    let root = project();
    std::fs::create_dir_all(root.join("Assets/Sprites")).unwrap();
    for (rel, img, imp) in textures() {
        let path = root.join(rel);
        img.save(&path).unwrap();
        imp.save(&path).unwrap();
    }
    for (rel, src) in scripts() {
        std::fs::write(root.join(rel), src).unwrap();
    }
    demo_scene().save(&root.join(SCENE)).unwrap();
}

#[test]
fn platformer_2d_matches_its_generator() {
    let root = project();
    for (rel, img, imp) in textures() {
        let path = root.join(rel);
        let on_disk = misty_engine::image::open(&path).unwrap().into_rgba8();
        assert_eq!(on_disk, img, "{rel}");
        assert_eq!(TextureImporter::try_load(&path).unwrap(), imp, "{rel}.meta");
    }
    for (rel, src) in scripts() {
        let on_disk = std::fs::read_to_string(root.join(rel)).unwrap();
        assert_eq!(on_disk.replace("\r\n", "\n"), src, "{rel}");
    }
    let (scene, report) = Scene::load(&root.join(SCENE)).unwrap();
    assert!(report.warnings.is_empty(), "{:?}", report.warnings);
    assert_eq!(scene.to_json(), demo_scene().to_json());
}

/// Playing: running right collects the ground coins (the UI Text counts them), the camera
/// follows, Space jumps, and the player lands back on the ground.
#[test]
fn platformer_2d_plays() {
    use misty_engine::audio::Audio;
    use misty_engine::runtime::{Runtime, Time};
    use misty_engine::scripting::{Input, LogKind};
    use misty_engine::settings::ProjectSettings;

    let root = project();
    misty_engine::assets::set_project_root(&root);
    let (mut scene, _) = Scene::load(&root.join(SCENE)).unwrap();
    let (audio, _) = Audio::null(root.clone());
    let mut rt = Runtime::start_with_audio(&mut scene, root, &ProjectSettings::default(), audio);
    let find = |s: &Scene, n: &str| s.find_by_name(n);
    let player = find(&scene, "Player").unwrap();
    let label = find(&scene, "Score Text").unwrap();
    let mut time = Time::default();
    let mut run = |scene: &mut Scene, seconds: f32, keys: &[&str], press: &[&str]| {
        for i in 0..(seconds * 60.0) as usize {
            let input = Input {
                held: keys.iter().map(|k| k.to_string()).collect(),
                pressed: if i == 0 {
                    press.iter().map(|k| k.to_string()).collect()
                } else {
                    Default::default()
                },
                ..Default::default()
            };
            time.advance(1.0 / 60.0);
            rt.update(scene, &time, input);
            let errors: Vec<_> = rt
                .take_logs()
                .into_iter()
                .filter(|(k, _)| *k != LogKind::Info)
                .collect();
            assert!(errors.is_empty(), "{errors:?}");
        }
    };
    // Settle on the ground.
    run(&mut scene, 0.5, &[], &[]);
    let start = scene.world_position(player);
    assert!(
        (start.y - 0.475).abs() < 0.05,
        "standing on the ground: {start}"
    );
    // Run right through the three ground coins.
    run(&mut scene, 1.2, &["d"], &[]);
    let p = scene.world_position(player);
    assert!(p.x > 0.0, "ran right: {p}");
    for i in 1..=3 {
        assert!(
            find(&scene, &format!("Coin {i}")).is_none(),
            "Coin {i} collected"
        );
    }
    assert!(find(&scene, "Coin 4").is_some());
    let text = scene.world.get::<&UiText>(label).unwrap().text.clone();
    assert_eq!(text, "Coins: 3");
    let cam = scene.world_position(find(&scene, "Main Camera").unwrap());
    assert!(cam.x > -3.0 && cam.z == -10.0, "the camera follows: {cam}");
    // Jump: up, then back down onto the ground.
    run(&mut scene, 0.3, &[], &["space"]);
    let up = scene.world_position(player);
    assert!(up.y > 2.0, "jumped: {up}");
    run(&mut scene, 1.5, &[], &[]);
    let down = scene.world_position(player);
    assert!((down.y - 0.475).abs() < 0.05, "landed: {down}");
}
