//! Template creation: every template makes a complete project whose scene loads with the
//! promised objects, and renders on a GPU. `write_template_thumbnails` (ignored) renders the
//! thumbnails shipped in `assets/templates/`.

use std::path::{Path, PathBuf};

use misty_engine::build::BuildSettings;
use misty_engine::scene::{PostProcessVolume, Scene, Terrain};
use misty_engine::water::WaterSurface;

use super::scenes::VOLUME;
use super::{PROJECT_THUMBNAIL, SCENE, Template, create};

fn temp_project(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("misty_template_{tag}_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    dir.join(format!("My {tag} Game"))
}

fn volume(scene: &Scene) -> PostProcessVolume {
    let e = scene.find_by_name(VOLUME).expect("post-process volume");
    (*scene.world.get::<&PostProcessVolume>(e).unwrap()).clone()
}

/// The `active` flag of every override, by name.
fn overrides(v: &PostProcessVolume) -> Vec<(&'static str, bool)> {
    vec![
        ("tonemapping", v.tonemapping.active),
        ("bloom", v.bloom.active),
        ("color adjustments", v.color_adjustments.active),
        ("vignette", v.vignette.active),
        ("fog", v.fog.active),
        ("sun shafts", v.sun_shafts.active),
        ("ambient occlusion", v.ambient_occlusion.active),
        ("contact shadows", v.contact_shadows.active),
        ("clouds", v.clouds.active),
    ]
}

fn is_on(v: &PostProcessVolume, name: &str) -> bool {
    overrides(v).iter().any(|(n, on)| *n == name && *on)
}

/// Create the template's project and check the files every project gets.
fn create_checked(template: Template) -> (PathBuf, Scene) {
    let root = temp_project(template.name());
    let start = std::time::Instant::now();
    create(&root, template).unwrap();
    eprintln!("{}: created in {:?}", template.name(), start.elapsed());
    for rel in [
        "Assets/Scenes",
        "ProjectSettings/ProjectSettings.json",
        "ProjectSettings/BuildSettings.json",
        "Library/EditorSettings.json",
        PROJECT_THUMBNAIL,
        SCENE,
    ] {
        assert!(root.join(rel).exists(), "{}: {rel}", template.name());
    }
    let build = BuildSettings::load(&root).unwrap();
    assert_eq!(build.scenes.len(), 1);
    assert_eq!(build.scenes[0].path, SCENE);
    assert!(build.scenes[0].enabled);
    let settings = misty_engine::settings::ProjectSettings::load(&root).unwrap();
    assert_eq!(settings.player.unwrap().product_name, root_name(&root));
    let (scene, report) = Scene::load(&root.join(SCENE)).unwrap();
    assert!(report.warnings.is_empty(), "{:?}", report.warnings);
    for name in ["Main Camera", "Directional Light", VOLUME] {
        assert!(scene.find_by_name(name).is_some(), "{name}");
    }
    (root, scene)
}

fn root_name(root: &Path) -> String {
    root.file_name().unwrap().to_string_lossy().into_owned()
}

fn cleanup(root: &Path) {
    let _ = std::fs::remove_dir_all(root.parent().unwrap());
}

#[test]
fn empty_template_has_camera_light_and_disabled_volume() {
    let (root, scene) = create_checked(Template::Empty);
    let v = volume(&scene);
    assert!(!v.enabled);
    assert!(
        overrides(&v).iter().all(|(_, on)| !on),
        "{:?}",
        overrides(&v)
    );
    // Nothing but the camera, the light and the volume.
    assert_eq!(scene.walk().len(), 3);
    cleanup(&root);
}

#[test]
fn standard_template_has_platform_and_disabled_volume() {
    let (root, scene) = create_checked(Template::Standard);
    let platform = scene.find_by_name("Platform").unwrap();
    let (scale, _, pos) = scene.world_matrix(platform).to_scale_rotation_translation();
    assert!(
        (scale - super::scenes::PLATFORM_SIZE).length() < 1e-4,
        "{scale}"
    );
    assert!(pos.y < 0.0);
    assert!(root.join(super::scenes::PLATFORM_MAT).is_file());
    assert_eq!(scene.active_with::<WaterSurface>().len(), 0);
    let v = volume(&scene);
    assert!(!v.enabled && overrides(&v).iter().all(|(_, on)| !on));
    let sun = scene.find_by_name("Directional Light").unwrap();
    let light = scene.world.get::<&misty_engine::scene::Light>(sun).unwrap();
    assert_eq!(light.shadows, misty_engine::scene::Shadows::Soft);
    drop(light);
    cleanup(&root);
}

#[test]
fn ocean_template_has_water_and_post_processing() {
    let (root, scene) = create_checked(Template::Ocean);
    assert!(scene.find_by_name("Platform").is_some());
    assert_eq!(scene.active_with::<WaterSurface>().len(), 1);
    let v = volume(&scene);
    assert!(v.enabled);
    for o in ["tonemapping", "bloom", "fog"] {
        assert!(is_on(&v, o), "{o}");
    }
    assert!(!is_on(&v, "ambient occlusion"));
    cleanup(&root);
}

#[test]
fn landscape_template_has_terrain_and_everything_on() {
    let start = std::time::Instant::now();
    let (root, scene) = create_checked(Template::Landscape);
    let took = start.elapsed();
    // < 5 s in release builds; debug test builds are slower.
    assert!(took.as_secs_f32() < 30.0, "{took:?}");
    let terrain = scene.find_by_name("Terrain").unwrap();
    let data = scene
        .world
        .get::<&Terrain>(terrain)
        .unwrap()
        .terrain_data
        .clone();
    assert!(root.join(&data).is_file(), "{data}");
    for stem in misty_engine::world_gen::biome::TEXTURES {
        let tex = root.join(misty_engine::world_gen::biome::texture_path(stem));
        assert!(tex.is_file(), "{stem}");
    }
    let v = volume(&scene);
    assert!(v.enabled);
    for o in [
        "clouds",
        "fog",
        "sun shafts",
        "ambient occlusion",
        "bloom",
        "tonemapping",
    ] {
        assert!(is_on(&v, o), "{o}");
    }
    assert!(v.fog.volumetric, "volumetric fog");
    // The camera is above the ground it stands on.
    let d = misty_engine::terrain::TerrainData::load(&root.join(&data)).unwrap();
    let cam = scene.world_position(scene.find_by_name("Main Camera").unwrap());
    let half = super::landscape::SIZE[0] * 0.5;
    assert!(cam.y > d.sample_height(cam.x + half, cam.z + half) + 5.0);
    cleanup(&root);
}

#[test]
fn create_refuses_a_folder_with_files() {
    let root = temp_project("Occupied");
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(root.join("notes.txt"), "keep me").unwrap();
    assert!(create(&root, Template::Empty).is_err());
    assert!(root.join("notes.txt").is_file());
    cleanup(&root);
}

#[test]
fn template_names_round_trip() {
    for t in Template::ALL {
        assert_eq!(Template::from_name(t.name()), Some(t));
        assert!(!t.description().is_empty());
        let png = misty_engine::image::load_from_memory(t.thumbnail_png()).unwrap();
        assert!(png.width() >= 4, "{}", t.name());
    }
    assert_eq!(
        Template::from_name(" landscape "),
        Some(Template::Landscape)
    );
    assert_eq!(Template::from_name("nope"), None);
}

/// The editor opens a new project on its starting scene.
#[test]
fn editor_opens_the_template_scene() {
    let root = temp_project("Opens");
    create(&root, Template::Standard).unwrap();
    let mut s = crate::state::EditorState::new(crate::project::ProjectBrowser::open(root.clone()));
    crate::scene_files::open_initial(&mut s);
    assert_eq!(
        s.scene.scene_path(s.scene.active_scene()).as_deref(),
        Some(SCENE)
    );
    assert!(s.scene.find_by_name("Platform").is_some());
    cleanup(&root);
}

/// Render the template's scene headlessly (terrain data made absolute, so the global project
/// root other tests use is left alone).
fn render(root: &Path, scene: &Scene) -> crate::sample_render::Image {
    let gpu = crate::sample_render::headless().unwrap();
    for e in scene.active_with::<Terrain>() {
        let mut t = scene.world.get::<&mut Terrain>(e).unwrap();
        t.terrain_data = root.join(&t.terrain_data).to_string_lossy().into_owned();
    }
    let assets = misty_engine::assets::AssetCache::open(root);
    let view = misty_engine::render_scene::GameView::new(scene, &assets).unwrap();
    crate::sample_render::render(&gpu, &view.frame(), 12)
}

/// On a GPU: every template renders a lit, non-flat image.
#[test]
fn templates_render_headless() {
    if crate::sample_render::headless().is_none() {
        eprintln!("no GPU adapter: skipped");
        return;
    }
    for t in Template::ALL {
        let root = temp_project(&format!("Render{}", t.name()));
        create(&root, t).unwrap();
        let (scene, _) = Scene::load(&root.join(SCENE)).unwrap();
        let img = render(&root, &scene);
        let lum: Vec<f32> = img
            .pixels
            .chunks(4)
            .map(|p| (p[0] as f32 + p[1] as f32 + p[2] as f32) / 3.0)
            .collect();
        let mean = lum.iter().sum::<f32>() / lum.len() as f32;
        let spread = lum.iter().map(|l| (l - mean).abs()).sum::<f32>() / lum.len() as f32;
        eprintln!("{}: mean {mean:.1}, spread {spread:.1}", t.name());
        assert!(mean > 20.0 && mean < 250.0, "{}: {mean}", t.name());
        assert!(spread > 2.0, "{}: flat image ({spread})", t.name());
        cleanup(&root);
    }
}

#[test]
#[ignore = "writes assets/templates/*.png; run on purpose to re-render the thumbnails"]
fn write_template_thumbnails() {
    let out = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("assets/templates");
    for t in Template::ALL {
        let root = temp_project(&format!("Thumb{}", t.name()));
        create(&root, t).unwrap();
        // Alone on purpose: terrain textures resolve against the project root.
        misty_engine::assets::set_project_root(&root);
        let (scene, _) = Scene::load(&root.join(SCENE)).unwrap();
        let img = render(&root, &scene);
        let path = out.join(format!("{}.png", t.name().to_lowercase()));
        let rgba = misty_engine::image::RgbaImage::from_raw(
            crate::sample_render::WIDTH,
            crate::sample_render::HEIGHT,
            img.pixels,
        )
        .unwrap();
        misty_engine::image::DynamicImage::ImageRgba8(rgba)
            .to_rgb8()
            .save(&path)
            .unwrap();
        cleanup(&root);
    }
}
