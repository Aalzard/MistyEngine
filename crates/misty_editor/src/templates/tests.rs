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
fn pp_ready_template_has_ocean_sunset_and_effects_on() {
    let (root, scene) = create_checked(Template::PpReady);
    // The same ground and cube as Standard.
    let platform = scene.find_by_name("Platform").unwrap();
    let (scale, _, _) = scene.world_matrix(platform).to_scale_rotation_translation();
    assert!((scale - super::scenes::PLATFORM_SIZE).length() < 1e-4);
    assert!(scene.find_by_name("Cube").is_some());
    assert_eq!(scene.active_with::<WaterSurface>().len(), 1);
    let v = volume(&scene);
    assert!(v.enabled);
    for o in [
        "clouds",
        "fog",
        "sun shafts",
        "bloom",
        "tonemapping",
        "vignette",
    ] {
        assert!(is_on(&v, o), "{o}");
    }
    assert!(v.fog.volumetric, "volumetric fog makes the god rays");
    // A low sun (sunset) and a camera at a corner of the platform.
    let sun = scene.find_by_name("Directional Light").unwrap();
    let dir = scene
        .world_matrix(sun)
        .transform_vector3(glam::Vec3::Z)
        .normalize();
    let elevation = (-dir.y).asin().to_degrees();
    assert!(
        elevation > 2.0 && elevation < 15.0,
        "sun elevation {elevation}"
    );
    let cam = scene.world_position(scene.find_by_name("Main Camera").unwrap());
    let half = super::scenes::PLATFORM_SIZE.x * 0.5;
    assert!(cam.x.abs() > half && cam.z.abs() > half, "{cam}");
    cleanup(&root);
}

#[test]
fn create_refuses_a_folder_with_files() {
    let root = temp_project("Occupied");
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(root.join("notes.txt"), "keep me").unwrap();
    assert!(create(&root, Template::Standard).is_err());
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
    for alias in [" pp ready ", "PPReady", "ppready"] {
        assert_eq!(
            Template::from_name(alias),
            Some(Template::PpReady),
            "{alias}"
        );
    }
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
    // A new project's Scene view starts looking through the Main Camera...
    crate::scene_files::restore_scene_view(&mut s);
    let cam = s.scene.find_by_name("Main Camera").unwrap();
    let eye = s.scene.world_position(cam);
    assert!((s.scene_view.camera.position() - eye).length() < 1e-3);
    // ...and reopens where it was left.
    let moved = eye + glam::Vec3::new(3.0, 1.0, -2.0);
    s.scene_view
        .camera
        .set_view(moved, glam::Quat::from_rotation_y(0.5));
    crate::scene_files::remember_scene_view(&s);
    let mut again =
        crate::state::EditorState::new(crate::project::ProjectBrowser::open(root.clone()));
    crate::scene_files::open_initial(&mut again);
    crate::scene_files::restore_scene_view(&mut again);
    assert!((again.scene_view.camera.position() - moved).length() < 1e-3);
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
        let path = out.join(format!("{}.png", t.file_stem()));
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

/// New Project ▸ Include sample scenes: the demo scenes and their assets come along, the
/// template's own scene stays first, and only committed sample files are copied.
#[test]
fn include_samples_copies_the_demo_scenes() {
    let Some(source) = super::samples::source() else {
        eprintln!("no sample project here: skipped");
        return;
    };
    let root = temp_project("Samples");
    super::create_with(&root, Template::Standard, true).unwrap();
    for rel in [
        SCENE,
        "Assets/Scenes/VehicleDemo.scene",
        "Assets/Scenes/LightmapDemo.scene",
        "Assets/Scripts/CarController.lua",
        super::scenes::PLATFORM_MAT,
    ] {
        assert!(root.join(rel).is_file(), "{rel}");
    }
    // Only committed sample files come along: never untracked or work-in-progress ones.
    if let super::samples::Source::Git(repo) = &source {
        let out = std::process::Command::new("git")
            .arg("-C")
            .arg(repo)
            .args(["ls-files", "SampleProject/Assets"])
            .output()
            .unwrap();
        let tracked: std::collections::HashSet<String> = String::from_utf8_lossy(&out.stdout)
            .lines()
            .map(|l| l.trim_start_matches("SampleProject/").to_owned())
            .collect();
        let ours = [
            SCENE,
            super::scenes::PLATFORM_MAT,
            super::scenes::ACCENT_MAT,
        ];
        let mut stack = vec![root.join("Assets")];
        while let Some(dir) = stack.pop() {
            for entry in std::fs::read_dir(dir).unwrap().flatten() {
                let path = entry.path();
                if path.is_dir() {
                    stack.push(path);
                    continue;
                }
                let rel = path
                    .strip_prefix(&root)
                    .unwrap()
                    .to_string_lossy()
                    .replace('\\', "/");
                assert!(
                    tracked.contains(&rel) || ours.contains(&rel.as_str()),
                    "{rel} is not a committed sample file"
                );
            }
        }
    }
    let build = BuildSettings::load(&root).unwrap();
    assert_eq!(build.scenes[0].path, SCENE);
    assert!(build.scenes[0].enabled);
    assert!(build.scenes.len() > 10, "{}", build.scenes.len());
    assert!(build.scenes[1..].iter().all(|s| !s.enabled));
    let settings = misty_engine::settings::ProjectSettings::load(&root).unwrap();
    assert_eq!(settings.player.unwrap().product_name, root_name(&root));
    // The project still opens on the template's scene.
    let (scene, _) = Scene::load(&root.join(SCENE)).unwrap();
    assert!(scene.find_by_name("Platform").is_some());
    cleanup(&root);
}
