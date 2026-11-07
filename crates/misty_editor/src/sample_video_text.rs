//! VideoTextDemo sample scene in `SampleProject`: a screen in a dim room playing a generated
//! test clip (scrolling colour bars, frame-number squares and a beep each second, Motion-JPEG
//! AVI with PCM audio) through a Video Player, and a UI panel showing rich text — colours,
//! bold / italic / underline / strike, sizes, marks, super- and subscript, inline sprites,
//! outline, shadow and gradient, a link that logs when clicked, an ellipsis box and an Arabic /
//! Hebrew line — next to a Raw Image showing the clip through a Render Texture.
//!
//! Regenerate with `cargo test -p misty_editor -- --ignored write_video_text_demo`; the other
//! tests check the committed files still match and render the demo headlessly.

use std::path::PathBuf;

use glam::{Vec2, Vec3};
use misty_engine::assets::Material;
use misty_engine::hecs::Entity;
use misty_engine::scene::render_settings::{AmbientSource, SkyboxKind};
use misty_engine::scene::{
    AudioSource, Camera, Canvas, CanvasScaler, Light, LightType, MeshFilter, MeshRenderer,
    PrimitiveMesh, RectTransform, Scene, ScriptRef, Scripts, Shadows, TextAnchor, TextEffects,
    TextOverflow, Transform, UiImage, UiRawImage, UiScaleMode, UiText,
};
use misty_engine::sprites::{SpriteMode, SpriteRect, TextureImporter, TextureType};
use misty_engine::video::generate::TestPattern;
use misty_engine::video::{VideoAudioOutput, VideoPlayer, VideoRenderMode};

const SCENE: &str = "Assets/Scenes/VideoTextDemo.scene";
const CLIP: &str = "Assets/Video/TestPattern.avi";
const ICONS: &str = "Assets/Video/RichIcons.png";
const SCRIPT: &str = "Assets/Scripts/LinkLogger.lua";
const SCREEN: &str = "Assets/Materials/Video Screen.mat";
const BEZEL: &str = "Assets/Materials/Screen Bezel.mat";
const ROOM: &str = "Assets/Materials/Screening Room.mat";

fn project() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../SampleProject")
}

/// The test clip: 256×144, 20 fps, 3 s, 22.05 kHz beep.
pub fn clip() -> TestPattern {
    TestPattern {
        width: 256,
        height: 144,
        fps: 20,
        seconds: 3,
        sample_rate: 22050,
        jpeg_quality: 80,
        ..Default::default()
    }
}

const LINK_SCRIPT: &str = r#"-- Logs the id of a rich text <link> when it is clicked.
local LinkLogger = {}

function LinkLogger:OnLinkClicked(id)
    print("Link clicked: " .. id)
end

return LinkLogger
"#;

/// Icon names and colours of the sprite sheet, one 32×32 cell each.
const ICON_CELLS: [(&str, [u8; 3]); 4] = [
    ("Star", [255, 205, 60]),
    ("Heart", [235, 70, 90]),
    ("Coin", [240, 170, 40]),
    ("Check", [90, 210, 110]),
];

/// A 128×32 sprite sheet: star, heart, coin and check mark drawn with signed distances.
fn icons() -> misty_engine::image::RgbaImage {
    misty_engine::image::RgbaImage::from_fn(128, 32, |x, y| {
        let cell = (x / 32) as usize;
        let (px, py) = ((x % 32) as f32 - 15.5, (y as f32) - 15.5);
        let r = (px * px + py * py).sqrt();
        let a = py.atan2(px);
        let inside = match cell {
            // Five-pointed star.
            0 => {
                let k = (a * 2.5).cos().abs();
                r < 7.0 + 7.0 * k.powf(6.0)
            }
            // Heart.
            1 => {
                let (u, v) = (px / 13.0, -py / 13.0 + 0.25);
                let q = u * u + v * v - 0.5;
                q * q * q - u * u * v * v * v < 0.0
            }
            // Coin with a rim.
            2 => r < 13.0 && !(9.5..11.0).contains(&r),
            // Check mark: two thick strokes.
            _ => {
                let d1 = ((px + 4.0) - (py - 6.0)).abs() / 2f32.sqrt();
                let d2 = ((px + 4.0) + (py - 6.0)).abs() / 2f32.sqrt();
                (d1 < 2.8 && px < -4.0 && px > -12.0) || (d2 < 2.8 && px > -5.5 && px < 12.0)
            }
        };
        let c = ICON_CELLS[cell].1;
        if inside {
            misty_engine::image::Rgba([c[0], c[1], c[2], 255])
        } else {
            misty_engine::image::Rgba([0, 0, 0, 0])
        }
    })
}

fn icon_importer() -> TextureImporter {
    let mut imp = TextureImporter::sprite();
    imp.texture_type = TextureType::Sprite;
    imp.sprite_mode = SpriteMode::Multiple;
    imp.sprites = ICON_CELLS
        .iter()
        .enumerate()
        .map(|(i, (name, _))| SpriteRect {
            name: (*name).into(),
            rect: [i as f32 * 32.0, 0.0, 32.0, 32.0],
            pivot: [0.5, 0.5],
        })
        .collect();
    imp
}

fn materials() -> [(&'static str, Material); 3] {
    [
        (
            SCREEN,
            Material {
                shader: "Unlit/Texture".into(),
                color: [1.0; 4],
                ..Default::default()
            },
        ),
        (
            BEZEL,
            Material {
                color: [0.05, 0.05, 0.06, 1.0],
                metallic: 0.6,
                smoothness: 0.7,
                ..Default::default()
            },
        ),
        (
            ROOM,
            Material {
                color: [0.16, 0.15, 0.18, 1.0],
                smoothness: 0.25,
                ..Default::default()
            },
        ),
    ]
}

/// The rich text of the panel.
pub const BODY: &str = "<color=#FFD24A>Colours</color>, <b>bold</b>, <i>italic</i>, \
<u>underline</u>, <s>strike</s>\n\
<size=130%>Big</size> and <size=75%>small</size>, <mark=#3AA0FF66>marked</mark>, \
x<sup>2</sup> H<sub>2</sub>O\n\
Sprites <sprite name=\"Star\"> <sprite name=\"Heart\"> <sprite name=\"Coin\"> \
<sprite name=\"Check\"> inline\n\
<link=\"docs\"><u><color=#7FD0FF>Click this link</color></u></link> to log a message\n\
<align=right><alpha=#99>right aligned, faded</align>\n\
\u{645}\u{631}\u{62d}\u{628}\u{627} \u{628}\u{627}\u{644}\u{639}\u{627}\u{644}\u{645} \u{2014} \
\u{5e9}\u{5dc}\u{5d5}\u{5dd} \u{5e2}\u{5d5}\u{5dc}\u{5dd}";

fn scaled(position: Vec3, scale: Vec3) -> Transform {
    Transform {
        position,
        scale,
        ..Default::default()
    }
}

fn mesh(s: &mut Scene, name: &str, parent: Option<Entity>, t: Transform, mat: &str) -> Entity {
    let e = s.spawn(name, parent);
    s.set_transform(e, t);
    s.add(e, MeshFilter::primitive(PrimitiveMesh::Cube));
    s.add(
        e,
        MeshRenderer {
            material: Some(mat.into()),
            ..Default::default()
        },
    );
    e
}

fn ui_rect(min: Vec2, max: Vec2, pos: Vec2, size: Vec2) -> RectTransform {
    RectTransform {
        anchor_min: min,
        anchor_max: max,
        pivot: Vec2::new(0.5, 0.5),
        anchored_position: pos,
        size_delta: size,
    }
}

fn demo_scene() -> Scene {
    let mut s = Scene::new("VideoTextDemo");
    let rs = &mut s.render_settings;
    rs.skybox.kind = SkyboxKind::Procedural;
    rs.environment_lighting.source = AmbientSource::Color;
    rs.environment_lighting.sky_color = [0.12, 0.12, 0.15];

    let cam = s.spawn("Main Camera", None);
    s.set_transform(
        cam,
        Transform::from_position(Vec3::new(-1.6, 1.6, -5.2)).with_euler(Vec3::new(4.0, 12.0, 0.0)),
    );
    s.add(cam, Camera::default());
    let light = s.spawn("Ceiling Light", None);
    s.set_transform(
        light,
        Transform::from_position(Vec3::new(0.0, 3.5, -2.0)).with_euler(Vec3::new(60.0, 20.0, 0.0)),
    );
    s.add(
        light,
        Light {
            kind: LightType::Directional,
            color: [1.0, 0.95, 0.88],
            intensity: 0.6,
            shadows: Shadows::Soft,
            ..Default::default()
        },
    );

    let floor = mesh(
        &mut s,
        "Floor",
        None,
        scaled(Vec3::new(0.0, -0.05, 0.0), Vec3::new(14.0, 0.1, 14.0)),
        ROOM,
    );
    let _ = floor;
    mesh(
        &mut s,
        "Back Wall",
        None,
        scaled(Vec3::new(0.0, 2.0, 1.2), Vec3::new(14.0, 4.0, 0.2)),
        ROOM,
    );

    // The screen: a 16:9 quad in a bezel, playing the clip with its audio.
    let tv = s.spawn("Television", None);
    s.set_transform(tv, Transform::from_position(Vec3::new(0.4, 1.7, 0.9)));
    mesh(
        &mut s,
        "Bezel",
        Some(tv),
        scaled(Vec3::new(0.0, 0.0, 0.08), Vec3::new(3.4, 1.98, 0.12)),
        BEZEL,
    );
    let screen = s.spawn("Screen", Some(tv));
    s.set_transform(
        screen,
        scaled(Vec3::new(0.0, 0.0, -0.0), Vec3::new(3.2, 1.8, 1.0)),
    );
    s.add(screen, MeshFilter::primitive(PrimitiveMesh::Quad));
    s.add(
        screen,
        MeshRenderer {
            material: Some(SCREEN.into()),
            cast_shadows: false,
            ..Default::default()
        },
    );
    s.add(
        screen,
        AudioSource {
            play_on_awake: false,
            ..Default::default()
        },
    );
    s.add(
        screen,
        VideoPlayer {
            clip: Some(CLIP.into()),
            looping: true,
            render_mode: VideoRenderMode::MaterialOverride,
            audio_output: VideoAudioOutput::AudioSource,
            ..Default::default()
        },
    );

    // UI: a panel of rich text on the left and the clip again through a Render Texture.
    let canvas = s.spawn("Canvas", None);
    s.add(canvas, Canvas::default());
    s.add(
        canvas,
        CanvasScaler {
            mode: UiScaleMode::ScaleWithScreenSize,
            reference_resolution: Vec2::new(1280.0, 720.0),
            match_width_or_height: 0.5,
            ..Default::default()
        },
    );
    let left = Vec2::new(0.0, 0.5);
    let panel = s.spawn("Rich Text Panel", Some(canvas));
    s.add(
        panel,
        ui_rect(left, left, Vec2::new(290.0, 20.0), Vec2::new(540.0, 560.0)),
    );
    s.add(
        panel,
        UiImage {
            color: [0.05, 0.06, 0.09, 0.82],
            raycast_target: false,
            ..Default::default()
        },
    );
    let top = Vec2::new(0.5, 1.0);
    let title = s.spawn("Title", Some(panel));
    s.add(
        title,
        ui_rect(top, top, Vec2::new(0.0, -46.0), Vec2::new(500.0, 60.0)),
    );
    s.add(
        title,
        UiText {
            text: "<b>Video</b> + <i>Rich Text</i>".into(),
            font_size: 40.0,
            color: [1.0; 4],
            alignment: TextAnchor::MiddleCenter,
            raycast_target: false,
            effects: TextEffects {
                outline_width: 1.5,
                outline_color: [0.05, 0.1, 0.25, 1.0],
                shadow: true,
                shadow_offset: [3.0, -3.0],
                shadow_color: [0.0, 0.0, 0.0, 0.6],
                shadow_softness: 0.4,
                gradient: true,
                gradient_colors: [
                    [1.0, 0.95, 0.6, 1.0],
                    [1.0, 0.75, 0.45, 1.0],
                    [0.45, 0.8, 1.0, 1.0],
                    [0.7, 0.55, 1.0, 1.0],
                ],
            },
            ..Default::default()
        },
    );
    let body = s.spawn("Body", Some(panel));
    s.add(
        body,
        ui_rect(top, top, Vec2::new(0.0, -270.0), Vec2::new(500.0, 360.0)),
    );
    s.add(
        body,
        UiText {
            text: BODY.into(),
            font_size: 24.0,
            line_spacing: 1.15,
            color: [0.92, 0.93, 0.96, 1.0],
            sprite_asset: Some(ICONS.into()),
            ..Default::default()
        },
    );
    s.add(
        body,
        Scripts {
            list: vec![ScriptRef {
                path: SCRIPT.into(),
                ..Default::default()
            }],
        },
    );
    let bottom = Vec2::new(0.5, 0.0);
    let small = s.spawn("Ellipsis Box", Some(panel));
    s.add(
        small,
        ui_rect(bottom, bottom, Vec2::new(0.0, 52.0), Vec2::new(500.0, 56.0)),
    );
    s.add(
        small,
        UiText {
            text: "Overflow: Ellipsis cuts text that does not fit the box, so this long \
                   sentence ends with three dots instead of spilling out of its panel."
                .into(),
            font_size: 20.0,
            color: [0.75, 0.8, 0.9, 1.0],
            overflow: TextOverflow::Ellipsis,
            character_spacing: 2.0,
            ..Default::default()
        },
    );

    let right = Vec2::new(1.0, 0.0);
    let picture = s.spawn("Render Texture Picture", Some(canvas));
    s.add(
        picture,
        ui_rect(
            right,
            right,
            Vec2::new(-150.0, 100.0),
            Vec2::new(256.0, 144.0),
        ),
    );
    s.add(
        picture,
        UiRawImage {
            texture: Some("runtime://Picture".into()),
            raycast_target: false,
            ..Default::default()
        },
    );
    s.add(
        picture,
        VideoPlayer {
            clip: Some(CLIP.into()),
            looping: true,
            playback_speed: 0.5,
            render_mode: VideoRenderMode::RenderTexture,
            target_texture: "Picture".into(),
            audio_output: VideoAudioOutput::None,
            ..Default::default()
        },
    );
    let caption = s.spawn("Caption", Some(picture));
    s.add(
        caption,
        ui_rect(
            bottom,
            bottom,
            Vec2::new(0.0, -16.0),
            Vec2::new(256.0, 24.0),
        ),
    );
    s.add(
        caption,
        UiText {
            text: "Render Texture · <b>½×</b> speed".into(),
            font_size: 18.0,
            color: [1.0; 4],
            alignment: TextAnchor::MiddleCenter,
            raycast_target: false,
            effects: TextEffects {
                shadow: true,
                ..Default::default()
            },
            ..Default::default()
        },
    );
    s
}

fn write_all(root: &std::path::Path) {
    std::fs::create_dir_all(root.join("Assets/Video")).unwrap();
    std::fs::write(root.join(CLIP), clip().encode()).unwrap();
    icons().save(root.join(ICONS)).unwrap();
    icon_importer().save(&root.join(ICONS)).unwrap();
    std::fs::write(root.join(SCRIPT), LINK_SCRIPT).unwrap();
    for (path, m) in materials() {
        m.save(&root.join(path)).unwrap();
    }
    demo_scene().save(&root.join(SCENE)).unwrap();
}

#[test]
#[ignore = "writes into SampleProject; run on purpose to regenerate"]
fn write_video_text_demo() {
    write_all(&project());
}

#[test]
fn video_text_demo_matches_its_generator() {
    let root = project();
    assert_eq!(
        std::fs::read(root.join(CLIP)).unwrap(),
        clip().encode(),
        "regenerate with write_video_text_demo"
    );
    let png = misty_engine::image::open(root.join(ICONS))
        .unwrap()
        .to_rgba8();
    assert_eq!(png, icons());
    let imp = TextureImporter::try_load(&root.join(ICONS)).unwrap();
    assert_eq!(imp, icon_importer());
    assert_eq!(
        std::fs::read_to_string(root.join(SCRIPT))
            .unwrap()
            .replace("\r\n", "\n"),
        LINK_SCRIPT
    );
    for (path, m) in materials() {
        crate::test_util::assert_close(&Material::load(&root.join(path)).unwrap(), &m, path);
    }
    let text = std::fs::read_to_string(root.join(SCENE)).unwrap();
    crate::test_util::assert_json_close(
        &text,
        &demo_scene().to_json(),
        "regenerate with write_video_text_demo",
    );
    let (scene, report) = Scene::from_json("VideoTextDemo", &text).unwrap();
    assert!(report.warnings.is_empty(), "{:?}", report.warnings);
    let players = scene.world.query::<&VideoPlayer>().iter().count();
    assert_eq!(players, 2);
}

/// The clip decodes back to its frames (frame number squares) and the beep lines up.
#[test]
fn demo_clip_decodes() {
    let src = misty_engine::video::VideoSource::open_file(&project().join(CLIP)).unwrap();
    let info = src.info();
    assert_eq!((info.width, info.height, info.frame_count), (256, 144, 60));
    assert_eq!(info.audio, Some((22050, 1)));
    for n in [0, 17, 59] {
        assert_eq!(clip().read_id(&src.frame(n).unwrap()), Some(n as u32));
    }
}

/// Play the demo headlessly: video frames reach the screen and the UI draws rich text.
#[test]
fn demo_renders_video_and_rich_text() {
    use misty_engine::render::Renderer;
    use misty_engine::render::ui::UiRenderer;
    struct Assets(PathBuf);
    impl misty_engine::render_scene::MaterialSource for Assets {
        fn params(&self, rel: Option<&str>) -> misty_engine::render::MaterialParams {
            rel.and_then(|r| Material::load(&self.0.join(r)).ok())
                .map(|m| m.params())
                .unwrap_or_default()
        }
    }
    let root = project();
    let text = std::fs::read_to_string(root.join(SCENE)).unwrap();
    let (mut scene, _) = Scene::from_json("VideoTextDemo", &text).unwrap();
    let (mut audio, _) = misty_engine::audio::Audio::null(root.clone());
    let mut video = misty_engine::video::VideoPlayback::new(root.clone());
    video.play_on_awake(&mut scene, &mut audio, None);
    video.wait_prepared(std::time::Duration::from_secs(20));
    video.update(&mut scene, &mut audio, 0.0);
    let screen = scene.find_by_name("Screen").unwrap();
    assert!(video.is_playing(screen));
    let Some(gpu) = crate::sample_render::headless() else {
        return;
    };
    let (device, queue) = &gpu;
    let materials = Assets(root.clone());
    let view = misty_engine::render_scene::GameView::new(&scene, &materials).unwrap();
    let mut renderer = Renderer::with_queue(device, queue);
    let target = renderer.create_target(device, 640, 360);
    renderer.render(device, queue, &target, &view.frame());
    let before_ui = crate::project_thumbnail::read_pixels(device, queue, &target);
    let mut ui = UiRenderer::new(device, queue);
    ui.render(device, queue, &target, &scene, None);
    let pixels = crate::project_thumbnail::read_pixels(device, queue, &target);
    let px = |img: &[u8], x: u32, y: u32| {
        let i = ((y * 640 + x) * 4) as usize;
        [img[i], img[i + 1], img[i + 2]]
    };
    // Colour bars on the screen: many distinct saturated colours in its middle row band.
    let mut colours = std::collections::HashSet::new();
    for x in (300..600).step_by(4) {
        let c = px(&before_ui, x, 140);
        colours.insert([c[0] / 48, c[1] / 48, c[2] / 48]);
    }
    assert!(colours.len() >= 4, "video on the screen: {colours:?}");
    // The panel's text: the left side varies a lot once the UI is drawn.
    let (mut lo, mut hi) = (255u8, 0u8);
    for y in (60..300).step_by(3) {
        for x in (40..300).step_by(3) {
            let v = px(&pixels, x, y)[0];
            lo = lo.min(v);
            hi = hi.max(v);
        }
    }
    assert!(hi as i32 - lo as i32 > 120, "rich text drawn: {lo}..{hi}");
    assert_ne!(before_ui, pixels);
}
