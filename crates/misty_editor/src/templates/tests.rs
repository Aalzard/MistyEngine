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
    for alias in ["MeadowDemo", "meadow demo", "meadowdemo"] {
        assert_eq!(
            Template::from_name(alias),
            Some(Template::MeadowDemo),
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
    for t in Template::ALL.into_iter().filter(|t| t.available()) {
        let root = temp_project(&format!("Render{}", t.name()));
        create(&root, t).unwrap();
        let (scene, _) = Scene::load(&root.join(t.scene())).unwrap();
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
        let (scene, _) = Scene::load(&root.join(t.scene())).unwrap();
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

/// Installed editors copy the samples from `Examples/SampleProject` next to the exe (a plain
/// folder, not git). The staged settings go into `Library/`, which a fresh project doesn't
/// have yet (user report: "The system cannot find the file specified. (os error 2)").
#[test]
fn include_samples_from_a_shipped_folder() {
    let base = temp_project("Shipped");
    let shipped = base.parent().unwrap().join("Examples/SampleProject");
    std::fs::create_dir_all(shipped.join("Assets/Scenes")).unwrap();
    std::fs::create_dir_all(shipped.join("Assets/Scripts")).unwrap();
    std::fs::create_dir_all(shipped.join("ProjectSettings")).unwrap();
    let (demo, _) = Scene::load(&{
        let root = temp_project("ShippedTemplate");
        create(&root, Template::Standard).unwrap();
        root.join(SCENE)
    })
    .unwrap();
    demo.save(&shipped.join("Assets/Scenes/ShippedDemo.scene"))
        .unwrap();
    std::fs::write(shipped.join("Assets/Scripts/Hello.lua"), "-- hello\n").unwrap();
    misty_engine::settings::ProjectSettings::default()
        .save(&shipped)
        .unwrap();

    let root = base;
    super::create_template(&root, Template::Standard).unwrap();
    super::samples::add(&root, &super::samples::Source::Folder(shipped.clone())).unwrap();
    assert!(root.join("Assets/Scenes/ShippedDemo.scene").is_file());
    assert!(root.join("Assets/Scripts/Hello.lua").is_file());
    assert!(!root.join("Library/SampleProjectSettings.json").exists());
    let build = BuildSettings::load(&root).unwrap();
    assert_eq!(build.scenes[0].path, SCENE);
    assert!(
        build
            .scenes
            .iter()
            .any(|s| s.path == "Assets/Scenes/ShippedDemo.scene" && !s.enabled)
    );
    let settings = misty_engine::settings::ProjectSettings::load(&root).unwrap();
    assert_eq!(settings.player.unwrap().product_name, root_name(&root));
    let _ = std::fs::remove_dir_all(root.parent().unwrap());
}

/// A failure names the action and the path (a user's bare "os error 2" told us nothing).
#[test]
fn create_errors_name_the_path() {
    let base = temp_project("Blocked");
    let parent = base.parent().unwrap();
    std::fs::create_dir_all(parent).unwrap();
    // A file where the project's parent folder should be: nothing can be created under it.
    let blocker = parent.join("not a folder");
    std::fs::write(&blocker, "x").unwrap();
    let err = create(&blocker.join("My Game"), Template::Standard).unwrap_err();
    assert!(err.starts_with("Could not create the folder"), "{err}");
    assert!(err.contains("not a folder"), "{err}");
    let _ = std::fs::remove_dir_all(parent);
}

// ---- MeadowDemo -------------------------------------------------------------------------------

use super::meadow::{self, SampleFiles};
use super::samples::Source;

/// The repo's `SampleProject` folder (laid out like the release package's
/// `Examples/SampleProject`).
fn sample_folder() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../SampleProject")
}

/// The sample files the template copies from in this build, or `None` (skipped) without any.
fn meadow_source() -> Option<Source> {
    let source = super::samples::source();
    if source.is_none() {
        eprintln!("no sample project here: skipped");
    }
    source
}

fn json(root: &Path, rel: &str) -> serde_json::Value {
    let text = std::fs::read_to_string(root.join(rel)).unwrap_or_else(|e| panic!("{rel}: {e}"));
    serde_json::from_str(&text).unwrap_or_else(|e| panic!("{rel}: {e}"))
}

/// Horizontal angle (degrees) between two directions.
fn yaw_between(a: glam::Vec3, b: glam::Vec3) -> f32 {
    let (a, b) = (
        glam::Vec2::new(a.x, a.z).normalize(),
        glam::Vec2::new(b.x, b.z).normalize(),
    );
    a.dot(b).clamp(-1.0, 1.0).acos().to_degrees()
}

/// Every referenced path exists in `root`, and every `asset_guids` entry names the GUID that
/// path's meta file has there.
fn assert_refs_resolve(root: &Path, rel: &str) {
    use misty_engine::asset_db::{meta, refs};
    let v = json(root, rel);
    let mut strings = Vec::new();
    refs::strings(&v, &mut strings);
    for s in strings.into_iter().filter_map(refs::asset_path) {
        assert!(root.join(s).is_file(), "{rel} references {s}, not copied");
    }
    for (path, guid) in refs::table(&v) {
        assert_eq!(
            meta::read_guid(&root.join(&path)),
            Some(guid),
            "{rel}: GUID of {path}"
        );
    }
}

/// New Project ▸ MeadowDemo: the scene and everything it uses, byte for byte with the same
/// GUIDs, every reference of the scene, prefab, terrain and tree recipes resolving in the new
/// project, the sample's input axes and layers, and the views turned towards the sun.
#[test]
fn meadow_demo_template_copies_the_scene_and_its_dependencies() {
    use misty_engine::asset_db::meta;
    use misty_engine::scene::Terrain;
    use misty_engine::terrain::TerrainData;
    use misty_engine::trees::TreeAsset;
    let Some(source) = meadow_source() else {
        return;
    };
    let root = temp_project("Meadow");
    let start = std::time::Instant::now();
    create(&root, Template::MeadowDemo).unwrap();
    eprintln!("MeadowDemo: created in {:?}", start.elapsed());

    let src = SampleFiles::open(&source).unwrap();
    let files = meadow::files(&src).unwrap();
    eprintln!("MeadowDemo: {} files", files.len());
    for rel in [
        meadow::SCENE,
        "Assets/Scenes/MeadowDemo.scene.meta",
        "Assets/Terrains/MeadowDemo.terrain",
        "Assets/Terrains/MeadowDemo.terrain.meta",
        "Assets/Prefabs/First Person Player.prefab",
        "Assets/Prefabs/First Person Player.prefab.meta",
        "Assets/Scripts/FirstPersonController.lua",
        "Assets/Scripts/HeadBobVertical.lua",
        "Assets/Scripts/HeadBobHorizontal.lua",
        "Assets/Scripts/HeadBobRotation.lua",
        "Assets/Trees/Oak.tree",
        "Assets/Trees/Birch.tree.meta",
        "Assets/Trees/README.md",
        "Assets/Trees/LICENSE.txt",
        "Assets/Trees/Textures/OakBark.png",
        "Assets/Textures/Meadow/MeadowThatch.png",
        "Assets/Textures/Meadow/MeadowGround.png",
        "Assets/Textures/Terrain/Cliff.png",
        "Assets/Textures/Terrain/Scree.png",
        // Folders keep their GUIDs.
        "Assets/Trees.meta",
        "Assets/Trees/Textures.meta",
        "Assets/Scenes.meta",
    ] {
        assert!(files.contains(rel), "{rel} not in the copy list");
        assert!(root.join(rel).is_file(), "{rel}");
    }
    // Byte for byte (meta files and their GUIDs included), except the scene: its player was
    // turned towards the sun.
    for rel in &files {
        let ours = std::fs::read(root.join(rel)).unwrap();
        if rel != meadow::SCENE {
            assert!(ours == src.read(rel).unwrap(), "{rel} differs");
        }
        if !rel.ends_with(".meta") {
            assert!(
                meta::read_guid(&root.join(rel)).is_some(),
                "{rel} has no GUID"
            );
        }
    }
    // Nothing else: no Standard leftovers, no other sample scenes.
    assert!(!root.join(SCENE).exists());
    assert!(!root.join("Assets/Materials").exists());
    assert!(!root.join("Assets/Scenes/VehicleDemo.scene").exists());

    // Every reference resolves in the new project (paths and GUIDs).
    let deps = misty_engine::asset_deps::collect(&root, &[meadow::SCENE.to_owned()]);
    assert!(deps.missing.is_empty(), "{:?}", deps.missing);
    let mut checked = vec![
        meadow::SCENE.to_owned(),
        "Assets/Prefabs/First Person Player.prefab".to_owned(),
        "Assets/Terrains/MeadowDemo.terrain".to_owned(),
    ];
    let data = TerrainData::load(&root.join("Assets/Terrains/MeadowDemo.terrain")).unwrap();
    assert!(data.trees.instances.len() > 100);
    assert_eq!(data.trees.prototypes.len(), 5);
    for p in &data.trees.prototypes {
        let tree = TreeAsset::load(&root.join(&p.tree)).unwrap();
        for tex in tree.textures() {
            assert!(root.join(tex).is_file(), "{}: {tex}", p.tree);
        }
        checked.push(p.tree.clone());
    }
    for tex in data.layers.iter().filter_map(|l| l.diffuse.as_ref()) {
        assert!(root.join(tex).is_file(), "{tex}");
    }
    for rel in &checked {
        assert_refs_resolve(&root, rel);
    }

    // Project settings: one enabled scene, the sample's axes and layers, the new name.
    let build = BuildSettings::load(&root).unwrap();
    assert_eq!(build.scenes.len(), 1);
    assert_eq!(build.scenes[0].path, meadow::SCENE);
    assert!(build.scenes[0].enabled);
    let settings = misty_engine::settings::ProjectSettings::load(&root).unwrap();
    for axis in ["Run", "Crouch", "Horizontal", "Vertical", "Jump", "Mouse X"] {
        assert!(settings.input.axes.iter().any(|a| a.name == axis), "{axis}");
    }
    assert_eq!(settings.tags_and_layers.name_to_layer("Player"), 5);
    assert_eq!(settings.player.unwrap().product_name, root_name(&root));
    assert!(root.join(PROJECT_THUMBNAIL).is_file());
    assert!(!root.join(super::samples::SAMPLE_SETTINGS).exists());

    // The scene: a Player linked to its prefab and a Terrain, the player facing the sun.
    let (scene, report) = Scene::load(&root.join(meadow::SCENE)).unwrap();
    assert!(report.warnings.is_empty(), "{:?}", report.warnings);
    let player = scene.find_by_name("Player").expect("Player");
    let player_json = &json(&root, meadow::SCENE)["objects"]
        .as_array()
        .unwrap()
        .iter()
        .find(|o| o["components"]["GameObject"]["name"] == "Player")
        .unwrap()["components"]["Prefab"]["path"]
        .clone();
    let link = player_json
        .as_str()
        .expect("the Player is a prefab instance");
    assert!(root.join(link).is_file(), "{link}");
    let terrain = scene.find_by_name("Terrain").expect("Terrain");
    assert!(scene.world.get::<&Terrain>(terrain).is_ok());
    let sun = scene.find_by_name("Directional Light").unwrap();
    let (_, sun_rot, _) = scene.world_matrix(sun).to_scale_rotation_translation();
    let to_sun = -(sun_rot * glam::Vec3::Z);
    let (_, player_rot, feet) = scene.world_matrix(player).to_scale_rotation_translation();
    let facing = player_rot * glam::Vec3::Z;
    assert!(facing.y.abs() < 1e-4, "the player stays upright: {facing}");
    let off = yaw_between(facing, to_sun);
    eprintln!("player yaw is {off:.1}° off the sun");
    assert!(off <= 30.5, "player faces the sun: {off}°");
    let trees: Vec<glam::Vec3> = data
        .trees
        .instances
        .iter()
        .map(|t| {
            scene.world_position(terrain)
                + glam::Vec3::new(t.position.x * data.size.x, 0.0, t.position.y * data.size.z)
        })
        .collect();
    let yaw = facing.x.atan2(facing.z).to_degrees();
    assert!(
        !meadow::blocked(yaw, feet + glam::Vec3::Y * 1.7, &trees),
        "a tree right in front of the player"
    );

    // The editor starts on MeadowDemo, the Scene view looking towards the sun, a little down.
    let editor = crate::scene_files::EditorSettings::load(&root);
    assert_eq!(editor.recent_scenes, [meadow::SCENE]);
    let view = editor.scene_view.expect("a starting Scene view");
    let look = glam::Quat::from_array(view.rotation) * glam::Vec3::Z;
    assert!(yaw_between(look, facing) < 0.5, "same way as the player");
    assert!(look.y < -0.02 && look.y > -0.25, "slightly down: {look}");
    assert!(view.position[1] > feet.y + 1.0, "above the ground");
    cleanup(&root);
}

/// The release package copies from a plain `Examples/SampleProject` folder: the same files as
/// from git, and a missing folder is an error naming its path.
#[test]
fn meadow_demo_template_from_a_shipped_folder() {
    let Some(source) = meadow_source() else {
        return;
    };
    let folder = Source::Folder(sample_folder());
    let ours = meadow::files(&SampleFiles::open(&folder).unwrap()).unwrap();
    let theirs = meadow::files(&SampleFiles::open(&source).unwrap()).unwrap();
    assert!(ours.is_superset(&theirs), "{:?}", theirs.difference(&ours));

    let root = temp_project("MeadowShipped");
    std::fs::create_dir_all(&root).unwrap();
    misty_engine::settings::ProjectSettings::default()
        .save(&root)
        .unwrap();
    let view = meadow::create_from(&root, &folder).unwrap();
    assert!(view.position.iter().all(|v| v.is_finite()));
    for rel in &theirs {
        assert!(root.join(rel).is_file(), "{rel}");
    }
    let settings = misty_engine::settings::ProjectSettings::load(&root).unwrap();
    assert!(settings.input.axes.iter().any(|a| a.name == "Crouch"));

    let gone = root.parent().unwrap().join("No Such Package/SampleProject");
    let err = meadow::create_from(&root.join("Other"), &Source::Folder(gone)).unwrap_err();
    assert!(err.contains("No Such Package"), "{err}");
    cleanup(&root);
}

/// Include sample scenes on top of MeadowDemo: MeadowDemo stays the starting scene (listed
/// once), files already copied are kept (the player still faces the sun), and copying the
/// template's files a second time writes nothing.
#[test]
fn meadow_demo_with_sample_scenes() {
    let Some(source) = meadow_source() else {
        return;
    };
    let root = temp_project("MeadowSamples");
    super::create_with(&root, Template::MeadowDemo, true).unwrap();
    assert!(root.join("Assets/Scenes/VehicleDemo.scene").is_file());
    let build = BuildSettings::load(&root).unwrap();
    assert_eq!(build.scenes[0].path, meadow::SCENE);
    assert!(build.scenes[0].enabled);
    let listed = build
        .scenes
        .iter()
        .filter(|s| s.path == meadow::SCENE)
        .count();
    assert_eq!(listed, 1);
    assert!(build.scenes[1..].iter().all(|s| !s.enabled));
    let before = std::fs::read(root.join(meadow::SCENE)).unwrap();
    let src = SampleFiles::open(&source).unwrap();
    assert!(
        before != src.read(meadow::SCENE).unwrap(),
        "turned scene kept"
    );
    assert_eq!(meadow::copy(&src, &root).unwrap(), 0, "second copy");
    assert_eq!(std::fs::read(root.join(meadow::SCENE)).unwrap(), before);
    cleanup(&root);
}

/// The editor opens a MeadowDemo project on MeadowDemo (Player, Terrain with trees), and Play
/// runs its scripts on the terrain without an error: the player lands and stands.
#[test]
fn meadow_demo_opens_and_plays() {
    use misty_engine::audio::Audio;
    use misty_engine::runtime::{Runtime, Time};
    use misty_engine::scene::Terrain;
    use misty_engine::scripting::{Input, LogKind};
    if meadow_source().is_none() {
        return;
    }
    let root = temp_project("MeadowPlays");
    create(&root, Template::MeadowDemo).unwrap();
    let mut s = crate::state::EditorState::new(crate::project::ProjectBrowser::open(root.clone()));
    crate::scene_files::open_initial(&mut s);
    assert_eq!(
        s.scene.scene_path(s.scene.active_scene()).as_deref(),
        Some(meadow::SCENE)
    );
    assert!(s.scene.find_by_name("Player").is_some());
    let terrain = s.scene.find_by_name("Terrain").unwrap();
    assert!(s.scene.world.get::<&Terrain>(terrain).is_ok());
    // The Scene view opens where the template put it (towards the sun).
    crate::scene_files::restore_scene_view(&mut s);
    let saved = crate::scene_files::EditorSettings::load(&root)
        .scene_view
        .unwrap();
    let eye = glam::Vec3::from(saved.position);
    assert!((s.scene_view.camera.position() - eye).length() < 1e-3);
    drop(s);

    let (mut scene, _) = Scene::load(&root.join(meadow::SCENE)).unwrap();
    // Terrain data made absolute: the global project root other tests use is left alone.
    let terrain = scene.find_by_name("Terrain").unwrap();
    let data_path = {
        let mut t = scene.world.get::<&mut Terrain>(terrain).unwrap();
        t.terrain_data = root.join(&t.terrain_data).to_string_lossy().into_owned();
        PathBuf::from(&t.terrain_data)
    };
    let data = misty_engine::terrain::TerrainData::load(&data_path).unwrap();
    assert!(!data.trees.instances.is_empty(), "a terrain with trees");
    let settings = misty_engine::settings::ProjectSettings::load(&root).unwrap();
    let (audio, _) = Audio::null(root.clone());
    let mut rt = Runtime::start_with_audio(&mut scene, root.clone(), &settings, audio);
    let mut time = Time::new(&settings.time);
    let player = scene.find_by_name("Player").unwrap();
    let start = scene.world_position(player);
    let mut logs = Vec::new();
    for _ in 0..60 {
        time.advance(1.0 / 60.0);
        rt.update(&mut scene, &time, Input::default());
        logs.extend(rt.take_logs());
    }
    let problems: Vec<_> = logs
        .iter()
        .filter(|(k, _)| matches!(k, LogKind::Error | LogKind::Warning))
        .collect();
    assert!(problems.is_empty(), "{problems:?}");
    let now = scene.world_position(player);
    assert!(
        (now - start).length() < 1.0,
        "the player stands: {start} -> {now}"
    );
    drop(rt);
    cleanup(&root);
}

/// The yaw nudge: straight at the sun when clear, else the nearest clear direction.
#[test]
fn meadow_view_steers_around_trees() {
    let eye = glam::Vec3::new(0.0, 1.7, 0.0);
    assert_eq!(meadow::clear_yaw(25.0, eye, &[]), 25.0);
    // A trunk 20 m ahead along yaw 25°.
    let (s, c) = 25f32.to_radians().sin_cos();
    let trunk = glam::Vec3::new(s * 20.0, 0.0, c * 20.0);
    assert!(meadow::blocked(25.0, eye, &[trunk]));
    let yaw = meadow::clear_yaw(25.0, eye, &[trunk]);
    assert!(yaw != 25.0 && (yaw - 25.0).abs() <= 30.0, "{yaw}");
    assert!(!meadow::blocked(yaw, eye, &[trunk]));
    // Behind the eye or far away doesn't count.
    assert!(!meadow::blocked(25.0 + 180.0, eye, &[trunk]));
    assert!(!meadow::blocked(25.0, eye, &[trunk * 10.0]));
    // The sun's yaw from a light's rotation (its light travels along +Z).
    let light = misty_engine::camera::euler_to_quat(glam::Vec3::new(30.0, 205.0, 0.0));
    assert!((meadow::toward_sun_yaw(light) - 25.0).abs() < 0.01);
}

/// Render `scene` at `size`, from `eye` (position, rotation) when given, else its camera
/// (terrain data made absolute; set the project root first for textures and trees).
fn render_view(
    root: &Path,
    scene: &Scene,
    size: (u32, u32),
    eye: Option<(glam::Vec3, glam::Quat)>,
    frames: u32,
) -> misty_engine::image::RgbImage {
    let gpu = crate::sample_render::headless().unwrap();
    for e in scene.active_with::<Terrain>() {
        let mut t = scene.world.get::<&mut Terrain>(e).unwrap();
        t.terrain_data = root.join(&t.terrain_data).to_string_lossy().into_owned();
    }
    let assets = misty_engine::assets::AssetCache::open(root);
    let view = misty_engine::render_scene::GameView::new(scene, &assets).unwrap();
    let mut camera = view.camera;
    if let Some((position, rotation)) = eye {
        camera.position = position;
        camera.rotation = rotation;
    }
    let mut frame = view.frame();
    frame.camera = &camera;
    let (device, queue) = &gpu;
    let mut renderer = misty_engine::render::Renderer::new(device);
    let target = renderer.create_target(device, size.0, size.1);
    for _ in 0..frames {
        renderer.render(device, queue, &target, &frame);
    }
    let pixels = crate::project_thumbnail::read_pixels(device, queue, &target);
    let rgba = misty_engine::image::RgbaImage::from_raw(size.0, size.1, pixels).unwrap();
    misty_engine::image::DynamicImage::ImageRgba8(rgba).to_rgb8()
}

/// The first views of a new MeadowDemo project, for the docs: the Scene view as the editor
/// opens it, and the player's camera (what Play shows first).
#[test]
#[ignore = "writes docs/screenshots/gfx/meadow_template_*.png; run on purpose"]
fn write_meadow_template_start_shots() {
    let root = temp_project("MeadowShot");
    create(&root, Template::MeadowDemo).unwrap();
    misty_engine::assets::set_project_root(&root);
    let (scene, _) = Scene::load(&root.join(meadow::SCENE)).unwrap();
    let view = crate::scene_files::EditorSettings::load(&root)
        .scene_view
        .unwrap();
    let eye = (
        glam::Vec3::from(view.position),
        glam::Quat::from_array(view.rotation),
    );
    let out = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../docs/screenshots/gfx");
    render_view(&root, &scene, (1280, 720), Some(eye), 24)
        .save(out.join("meadow_template_start.png"))
        .unwrap();
    render_view(&root, &scene, (1280, 720), None, 24)
        .save(out.join("meadow_template_player.png"))
        .unwrap();
    cleanup(&root);
}
