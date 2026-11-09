//! Template creation: every template makes a complete project whose scene loads with the
//! promised objects, starts with post-processing (bloom, tonemapping, volumetric clouds, sun
//! shafts; bloom for the 2D Platformer), opens in the editor, plays without a script error and
//! renders on a GPU.
//! `write_template_thumbnails` (ignored) renders the thumbnails shipped in `assets/templates/`.

use std::path::{Path, PathBuf};

use misty_engine::build::BuildSettings;
use misty_engine::scene::{Camera, PostProcessVolume, Scene, Terrain};

use super::scenes::VOLUME;
use super::{PROJECT_THUMBNAIL, SCENE, Template, create};

fn temp_project(tag: &str) -> PathBuf {
    let tag = tag.replace(' ', "");
    let dir = std::env::temp_dir().join(format!("misty_template_{tag}_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    dir.join(format!("My {tag} Game"))
}

/// The scene's global volume (any name: MeadowDemo calls it "Global Volume").
fn volume(scene: &Scene) -> PostProcessVolume {
    let e = *scene
        .active_with::<PostProcessVolume>()
        .first()
        .expect("post-process volume");
    (*scene.world.get::<&PostProcessVolume>(e).unwrap()).clone()
}

/// What every template starts with: tonemapping, bloom, volumetric clouds and sun shafts on.
fn assert_post_processing(name: &str, scene: &Scene) {
    let v = volume(scene);
    assert!(v.enabled, "{name}: volume off");
    for (what, on) in [
        ("tonemapping", v.tonemapping.active),
        ("bloom", v.bloom.active),
        ("clouds", v.clouds.active && v.clouds.enabled),
        ("sun shafts", v.sun_shafts.active),
    ] {
        assert!(on, "{name}: {what} off");
    }
    assert!(v.bloom.intensity > 0.0 && v.sun_shafts.intensity > 0.0);
}

/// The enabled camera tagged MainCamera, named "Main Camera".
fn main_camera(scene: &Scene) -> misty_engine::hecs::Entity {
    let cam = scene.find_by_name("Main Camera").expect("Main Camera");
    assert_eq!(scene.game_object(cam).tag, "MainCamera");
    assert!(scene.world.get::<&Camera>(cam).unwrap().enabled);
    cam
}

/// Create a generated template's project and check the files every project gets, and what
/// every 3D template's scene has (sun, camera, post-processing).
fn create_checked(template: Template) -> (PathBuf, Scene) {
    let (root, scene) = create_project_checked(template);
    for name in ["Directional Light", VOLUME] {
        assert!(scene.find_by_name(name).is_some(), "{name}");
    }
    main_camera(&scene);
    assert_post_processing(template.name(), &scene);
    (root, scene)
}

/// Create a generated template's project and check the files every project gets: settings with
/// the product name, Build Settings listing the scene, the thumbnail, a scene that loads without
/// a warning and every material it uses.
fn create_project_checked(template: Template) -> (PathBuf, Scene) {
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
    // Every material the scene uses was written, with the texture it names.
    for r in scene
        .world
        .query::<&misty_engine::scene::MeshRenderer>()
        .iter()
    {
        if let Some(m) = &r.material {
            let mat = misty_engine::assets::Material::load(&root.join(m)).unwrap();
            if let Some(tex) = &mat.albedo_map {
                assert!(root.join(tex).is_file(), "{m}: {tex}");
            }
        }
    }
    (root, scene)
}

fn root_name(root: &Path) -> String {
    root.file_name().unwrap().to_string_lossy().into_owned()
}

fn cleanup(root: &Path) {
    let _ = std::fs::remove_dir_all(root.parent().unwrap());
}

/// A project's scene in Play mode (scripts resolve against the project, not the global root).
struct Play {
    scene: Scene,
    rt: misty_engine::runtime::Runtime,
    time: misty_engine::runtime::Time,
}

impl Play {
    fn start(root: &Path, rel: &str) -> Self {
        use misty_engine::audio::Audio;
        use misty_engine::runtime::{Runtime, Time};
        let (mut scene, _) = Scene::load(&root.join(rel)).unwrap();
        let settings = misty_engine::settings::ProjectSettings::load(root).unwrap();
        let (audio, _) = Audio::null(root.to_path_buf());
        let rt = Runtime::start_with_audio(&mut scene, root.to_path_buf(), &settings, audio);
        Self {
            scene,
            rt,
            time: Time::new(&settings.time),
        }
    }

    /// `n` frames of 1/60 s holding `held`, with `pressed` pressed on the first; fails on a
    /// script error or warning.
    fn frames(&mut self, n: usize, held: &[&str], pressed: &[&str]) {
        use misty_engine::scripting::{Input, LogKind};
        for i in 0..n {
            let mut input = Input::default();
            input.held.extend(held.iter().map(|k| k.to_string()));
            if i == 0 {
                input.pressed.extend(pressed.iter().map(|k| k.to_string()));
                input.held.extend(pressed.iter().map(|k| k.to_string()));
            }
            self.time.advance(1.0 / 60.0);
            self.rt.update(&mut self.scene, &self.time, input);
            let problems: Vec<_> = self
                .rt
                .take_logs()
                .into_iter()
                .filter(|(k, _)| matches!(k, LogKind::Error | LogKind::Warning))
                .collect();
            assert!(problems.is_empty(), "{problems:?}");
        }
    }

    fn position(&self, name: &str) -> glam::Vec3 {
        self.scene
            .world_position(self.scene.find_by_name(name).unwrap())
    }

    fn text(&self, name: &str) -> String {
        let e = self.scene.find_by_name(name).unwrap();
        self.scene
            .world
            .get::<&misty_engine::scene::UiText>(e)
            .unwrap()
            .text
            .clone()
    }
}

/// Horizontal angle (degrees) between two directions.
fn yaw_between(a: glam::Vec3, b: glam::Vec3) -> f32 {
    let (a, b) = (
        glam::Vec2::new(a.x, a.z).normalize(),
        glam::Vec2::new(b.x, b.z).normalize(),
    );
    a.dot(b).clamp(-1.0, 1.0).acos().to_degrees()
}

// ---- Floating Platform ------------------------------------------------------------------------

#[test]
fn floating_platform_template_has_the_platform_in_the_sky() {
    let (root, scene) = create_checked(Template::FloatingPlatform);
    let platform = scene.find_by_name("Platform").unwrap();
    let (scale, _, pos) = scene.world_matrix(platform).to_scale_rotation_translation();
    assert!(
        (scale - super::scenes::PLATFORM_SIZE).length() < 1e-4,
        "{scale}"
    );
    assert!(pos.y < 0.0);
    assert!(scene.find_by_name("Cube").is_some());
    assert!(root.join(super::scenes::PLATFORM_MAT).is_file());
    // Nothing else: no water, no terrain.
    let water = scene.active_with::<misty_engine::water::WaterSurface>();
    assert!(water.is_empty());
    assert!(scene.active_with::<Terrain>().is_empty());
    let sun = scene.find_by_name("Directional Light").unwrap();
    let light = scene.world.get::<&misty_engine::scene::Light>(sun).unwrap();
    assert_eq!(light.shadows, misty_engine::scene::Shadows::Soft);
    drop(light);
    // The sun is ahead of the camera, so its glow and rays are in the first view.
    let to_sun = -(scene.world_rotation(sun) * glam::Vec3::Z);
    let look = scene.world_rotation(main_camera(&scene)) * glam::Vec3::Z;
    assert!(to_sun.dot(look) > 0.5, "sun {to_sun}, camera {look}");
    cleanup(&root);
}

#[test]
fn create_refuses_a_folder_with_files() {
    let root = temp_project("Occupied");
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(root.join("notes.txt"), "keep me").unwrap();
    assert!(create(&root, Template::FloatingPlatform).is_err());
    assert!(root.join("notes.txt").is_file());
    cleanup(&root);
}

#[test]
fn template_names_round_trip() {
    assert_eq!(
        Template::ALL.map(Template::name),
        [
            "MeadowDemo",
            "First Person",
            "Racing",
            "2D Platformer",
            "Floating Platform"
        ]
    );
    for t in Template::ALL {
        assert_eq!(Template::from_name(t.name()), Some(t));
        assert!(!t.description().is_empty());
        let png = misty_engine::image::load_from_memory(t.thumbnail_png()).unwrap();
        assert!(png.width() >= 4, "{}", t.name());
    }
    for (alias, t) in [
        ("first person", Template::FirstPerson),
        ("FirstPerson", Template::FirstPerson),
        (" racing ", Template::Racing),
        ("floatingplatform", Template::FloatingPlatform),
        // The Floating Platform's old name.
        ("Standard", Template::FloatingPlatform),
        ("meadow demo", Template::MeadowDemo),
        ("meadowdemo", Template::MeadowDemo),
        ("2d platformer", Template::Platformer2D),
        ("2D", Template::Platformer2D),
        // The sample scene it comes from.
        ("Platformer2D", Template::Platformer2D),
    ] {
        assert_eq!(Template::from_name(alias), Some(t), "{alias}");
    }
    // The water scene is gone.
    assert_eq!(Template::from_name("PP Ready"), None);
    assert_eq!(Template::from_name("nope"), None);
    assert_eq!(Template::default(), Template::FloatingPlatform);
    assert!(Template::Racing.available() && Template::FloatingPlatform.available());
    // Generated: every install can make it.
    assert!(Template::Platformer2D.available());
    let two_d: Vec<Template> = Template::ALL.into_iter().filter(|t| t.is_2d()).collect();
    assert_eq!(two_d, [Template::Platformer2D]);
}

/// The editor opens a new project on its starting scene.
#[test]
fn editor_opens_the_template_scene() {
    let root = temp_project("Opens");
    create(&root, Template::FloatingPlatform).unwrap();
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

/// Play runs the Floating Platform for a second without an error.
#[test]
fn floating_platform_plays() {
    let root = temp_project("PlatformPlays");
    create(&root, Template::FloatingPlatform).unwrap();
    let mut play = Play::start(&root, SCENE);
    play.frames(60, &[], &[]);
    drop(play);
    cleanup(&root);
}

// ---- First Person -----------------------------------------------------------------------------

/// New Project ▸ First Person: a big flat white ground with a collider, the First Person
/// Player prefab (and its scripts) at the origin facing the sun, its head camera as the Main
/// Camera, and the sample's input axes and Player layer.
#[test]
fn first_person_template_has_a_player_on_white_ground() {
    use super::first_person::{GROUND_MAT, GROUND_SIZE, PREFAB};
    if super::samples::source().is_none() {
        eprintln!("no sample project here: skipped");
        return;
    }
    let (root, scene) = create_checked(Template::FirstPerson);
    for rel in [
        PREFAB,
        "Assets/Prefabs/First Person Player.prefab.meta",
        "Assets/Scripts/FirstPersonController.lua",
        "Assets/Scripts/HeadBobVertical.lua",
        "Assets/Scripts/HeadBobHorizontal.lua",
        "Assets/Scripts/HeadBobRotation.lua",
        GROUND_MAT,
        super::first_person::GRID_TEXTURE,
    ] {
        assert!(root.join(rel).is_file(), "{rel}");
    }
    // Only the player and what it uses: no meadow, no other samples.
    assert!(!root.join("Assets/Terrains").exists());
    assert!(!root.join("Assets/Scenes/MeadowDemo.scene").exists());
    let deps = misty_engine::asset_deps::collect(&root, &[SCENE.to_owned()]);
    assert!(deps.missing.is_empty(), "{:?}", deps.missing);

    let ground = scene.find_by_name("Ground").unwrap();
    let (scale, _, pos) = scene.world_matrix(ground).to_scale_rotation_translation();
    assert!((scale - GROUND_SIZE).length() < 1e-2, "{scale}");
    assert!((pos.y + GROUND_SIZE.y * 0.5).abs() < 1e-4, "top at y = 0");
    // Its collider, in tiles covering the whole ground.
    let tiles = scene.children(scene.find_by_name("Ground Colliders").unwrap());
    let tile = super::first_person::COLLIDER_TILE;
    assert_eq!(tiles.len() as f32, (GROUND_SIZE.x / tile).powi(2));
    for t in tiles {
        let box_collider = scene.world.get::<&misty_engine::scene::BoxCollider>(t);
        assert!(box_collider.is_ok());
    }
    let mat = misty_engine::assets::Material::load(&root.join(GROUND_MAT)).unwrap();
    assert!(mat.color[..3].iter().all(|c| *c > 0.9), "white: {mat:?}");
    // Nothing else stands on it.
    let drawn = scene
        .world
        .query::<&misty_engine::scene::MeshRenderer>()
        .iter()
        .count();
    assert_eq!(drawn, 1, "only the ground is drawn");

    let player = scene.find_by_name("Player").unwrap();
    assert_eq!(scene.prefab_link(player).as_deref(), Some(PREFAB));
    let cam = main_camera(&scene);
    assert_eq!(scene.prefab_root(cam), Some(player), "the head camera");
    let far = scene.world.get::<&Camera>(cam).unwrap().far;
    assert_eq!(far, 3000.0);
    assert_eq!(scene.active_with::<Camera>().len(), 1, "one camera");
    // Facing the sun.
    let sun = scene.find_by_name("Directional Light").unwrap();
    let to_sun = -(scene.world_rotation(sun) * glam::Vec3::Z);
    let facing = scene.world_rotation(player) * glam::Vec3::Z;
    assert!(yaw_between(facing, to_sun) < 1.0, "{facing} {to_sun}");

    let settings = misty_engine::settings::ProjectSettings::load(&root).unwrap();
    for axis in ["Run", "Crouch", "Horizontal", "Vertical", "Jump", "Mouse X"] {
        assert!(settings.input.axes.iter().any(|a| a.name == axis), "{axis}");
    }
    assert_eq!(settings.tags_and_layers.name_to_layer("Player"), 5);
    let editor = crate::scene_files::EditorSettings::load(&root);
    assert!(editor.scene_view.is_some());
    cleanup(&root);
}

/// The editor opens a First Person project, and Play puts the player on the ground; holding W
/// walks it forwards, without a script error.
#[test]
fn first_person_opens_and_plays() {
    if super::samples::source().is_none() {
        eprintln!("no sample project here: skipped");
        return;
    }
    let root = temp_project("FirstPersonPlays");
    create(&root, Template::FirstPerson).unwrap();
    let mut s = crate::state::EditorState::new(crate::project::ProjectBrowser::open(root.clone()));
    crate::scene_files::open_initial(&mut s);
    assert_eq!(
        s.scene.scene_path(s.scene.active_scene()).as_deref(),
        Some(SCENE)
    );
    assert!(s.scene.find_by_name("Player").is_some());
    drop(s);

    let mut play = Play::start(&root, SCENE);
    let start = play.position("Player");
    play.frames(60, &[], &[]);
    let stood = play.position("Player");
    assert!(
        stood.y.abs() < 0.2 && (stood - start).length() < 0.5,
        "stands on the ground: {start} -> {stood}"
    );
    play.frames(90, &["w"], &[]);
    let walked = play.position("Player");
    let player = play.scene.find_by_name("Player").unwrap();
    let facing = play.scene.world_rotation(player) * glam::Vec3::Z;
    let moved = (walked - stood).dot(facing);
    assert!(moved > 2.0, "walked forwards: {stood} -> {walked}");
    drop(play);
    cleanup(&root);
}

// ---- Racing -----------------------------------------------------------------------------------

/// New Project ▸ Racing: the circuit (a closed spline with a road extrude and kerbs), props,
/// the car with four wheels and three bodies, the chase camera, the HUD and the Lua scripts
/// as shipped.
#[test]
fn racing_template_has_a_circuit_a_car_and_scripts() {
    use misty_engine::scene::{Rigidbody, Scripts, WheelCollider};
    use misty_engine::splines::{SplineContainer, SplineExtrude};
    let (root, scene) = create_checked(Template::Racing);
    for (rel, source) in super::racing::SCRIPTS {
        let on_disk = std::fs::read_to_string(root.join(rel)).unwrap();
        assert_eq!(on_disk, source, "{rel}");
    }
    for name in ["Track", "Kerb Left", "Kerb Right"] {
        let e = scene.find_by_name(name).unwrap();
        let c = scene.world.get::<&SplineContainer>(e).unwrap();
        assert!(c.splines[0].closed, "{name}: a loop");
        assert!(scene.world.get::<&SplineExtrude>(e).unwrap().enabled);
    }
    let path = super::racing::track_spline().path();
    assert!(
        path.length > 1000.0 && path.length < 1400.0,
        "{}",
        path.length
    );
    let track = scene.find_by_name("Track").unwrap();
    let width = scene.world.get::<&SplineExtrude>(track).unwrap().width;
    assert!(width >= 12.0, "wide enough: {width}");
    for name in ["Start Line", "Gantry Beam", "Grandstand", "Trees", "Hills"] {
        assert!(scene.find_by_name(name).is_some(), "{name}");
    }
    let posts = scene.find_by_name("Marker Posts").unwrap();
    assert!(scene.children(posts).len() > 60);
    let barriers = scene.find_by_name("Barriers").unwrap();
    assert!(!scene.children(barriers).is_empty(), "barriers in corners");

    let car = scene.find_by_name("Car").unwrap();
    assert!(scene.world.get::<&Rigidbody>(car).is_ok());
    for (name, _, _) in super::racing::WHEELS {
        let w = scene.find_by_name(name).unwrap();
        let model = scene.world.get::<&WheelCollider>(w).unwrap().wheel_model;
        assert_eq!(model, scene.find_by_name(&format!("{name} Model")));
    }
    let shown: Vec<bool> = super::racing::MODELS
        .iter()
        .map(|m| scene.game_object(scene.find_by_name(m).unwrap()).active)
        .collect();
    assert_eq!(shown, [true, false, false]);
    // The car waits on the grid just before the start line, on the road.
    let on = path.nearest(scene.world_position(car));
    assert!(on.distance < 1.0 && on.t > 0.95, "{} {}", on.distance, on.t);
    let scripts = |name: &str| -> Vec<String> {
        let e = scene.find_by_name(name).unwrap();
        let s = scene.world.get::<&Scripts>(e).unwrap();
        s.list.iter().map(|r| r.path.clone()).collect()
    };
    assert_eq!(scripts("Car"), [super::racing::RACE_CAR]);
    assert_eq!(scripts("Main Camera"), [super::racing::CHASE_CAMERA]);
    assert_eq!(
        scripts("Race"),
        [super::racing::LAP_TIMER, super::racing::RACE_SETTINGS]
    );
    for name in [
        "Lap Text",
        "Times Text",
        "Speed Text",
        "Boost Text",
        "Message Text",
        "Settings Text",
    ] {
        assert!(scene.find_by_name(name).is_some(), "{name}");
    }
    cleanup(&root);
}

/// Speed of the car (m/s), from the physics world.
fn car_speed(play: &Play) -> f32 {
    let car = play.scene.find_by_name("Car").unwrap();
    play.rt
        .with_physics(|p| p.velocity(car))
        .unwrap_or_default()
        .length()
}

/// Headless Play of the Racing template: a simple autopilot holds W (up to 30 m/s) and steers
/// with A/D towards a point 25 m ahead on the track spline. The car crosses the start line
/// (the lap timer starts) and drives well over a third of the loop on its wheels, never put
/// back by the reset, without a script error; the HUD shows speed and lap.
#[test]
fn racing_template_drives_round_the_loop() {
    let root = temp_project("RacingDrives");
    create(&root, Template::Racing).unwrap();
    let mut s = crate::state::EditorState::new(crate::project::ProjectBrowser::open(root.clone()));
    crate::scene_files::open_initial(&mut s);
    assert!(s.scene.find_by_name("Car").is_some());
    drop(s);

    let path = super::racing::track_spline().path();
    let mut play = Play::start(&root, SCENE);
    play.frames(60, &[], &[]);
    let rest = play.position("Car");
    assert!(
        (rest.y - super::racing::RIDE_HEIGHT).abs() < 0.08,
        "rests on its wheels: {rest}"
    );
    let mut travelled = 0.0;
    let mut last_t = path.nearest(rest).t;
    let mut top = 0.0f32;
    for frame in 0..60 * 40 {
        let car = play.scene.find_by_name("Car").unwrap();
        let p = play.scene.world_position(car);
        let forward = play.scene.world_rotation(car) * glam::Vec3::Z;
        let here = path.nearest(p);
        let d = (here.t * path.length + 25.0).rem_euclid(path.length);
        let ahead = path.position(path.t_at_distance(d));
        // Positive: the point ahead is to the right (+X when facing +Z).
        let side = forward.cross(ahead - p).y;
        let speed = car_speed(&play);
        top = top.max(speed);
        let mut keys = Vec::new();
        if speed < 30.0 {
            keys.push("w");
        }
        if side > 0.5 {
            keys.push("d");
        } else if side < -0.5 {
            keys.push("a");
        }
        play.frames(1, &keys, &[]);
        let mut dt = here.t - last_t;
        if dt < -0.5 {
            dt += 1.0;
        }
        travelled += dt;
        last_t = here.t;
        assert!(
            play.text("Message Text") != "Back on track",
            "reset at frame {frame}, {p}, {} m off the road",
            here.distance
        );
        let up = play.scene.world_rotation(car) * glam::Vec3::Y;
        assert!(up.y > 0.5, "on its wheels at frame {frame}: {up}");
    }
    eprintln!(
        "Racing: {:.0}% of the loop, top {:.1} m/s",
        travelled * 100.0,
        top
    );
    assert!(travelled > 0.4, "drove round the loop: {travelled}");
    assert!(top > 25.0, "fast: {top} m/s");
    let lap = play.text("Lap Text");
    assert!(lap.starts_with("LAP 1 "), "{lap}");
    let speed = play.text("Speed Text");
    assert!(speed.ends_with(" km/h") && speed != "0 km/h", "{speed}");
    drop(play);
    cleanup(&root);
}

/// The settings keys while playing: C switches the car body (and its tuning), G the gravity,
/// 1–6 mass, grip and top speed; the panel shows the values; Shift boosts; R resets.
#[test]
fn racing_settings_change_while_playing() {
    use misty_engine::scene::{Rigidbody, WheelCollider};
    let root = temp_project("RacingSettings");
    create(&root, Template::Racing).unwrap();
    let mut play = Play::start(&root, SCENE);
    play.frames(30, &[], &[]);
    let panel = play.text("Settings Text");
    assert!(
        panel.contains("Sport") && panel.contains("Normal"),
        "{panel}"
    );
    let active = |play: &Play| -> Vec<bool> {
        super::racing::MODELS
            .iter()
            .map(|m| {
                let e = play.scene.find_by_name(m).unwrap();
                play.scene.active_in_hierarchy(e)
            })
            .collect()
    };
    assert_eq!(active(&play), [true, false, false]);
    let car = play.scene.find_by_name("Car").unwrap();
    let mass = |play: &Play| play.scene.world.get::<&Rigidbody>(car).unwrap().mass;
    assert_eq!(mass(&play), 1150.0);

    play.frames(2, &[], &["c"]);
    assert_eq!(active(&play), [false, true, false]);
    assert_eq!(mass(&play), 1500.0, "the Muscle's mass");
    assert!(play.text("Settings Text").contains("Muscle"));

    play.frames(2, &[], &["g"]);
    let g = play.rt.physics_config().0;
    assert!((g.y + 18.0).abs() < 1e-3, "high gravity: {g}");
    assert!(play.text("Settings Text").contains("High"));
    play.frames(2, &[], &["g"]);
    let g = play.rt.physics_config().0;
    assert!((g.y + 3.7).abs() < 1e-3, "low gravity: {g}");

    play.frames(2, &[], &["2"]);
    assert_eq!(mass(&play), 1600.0);
    let wheel = play.scene.find_by_name("Wheel FL").unwrap();
    let grip = |play: &Play| {
        play.scene
            .world
            .get::<&WheelCollider>(wheel)
            .unwrap()
            .sideways_friction
            .stiffness
    };
    let before = grip(&play);
    play.frames(2, &[], &["4"]);
    assert!((grip(&play) - before - 0.1).abs() < 1e-4);
    play.frames(2, &[], &["6"]);
    let panel = play.text("Settings Text");
    assert!(
        panel.contains("1600 kg") && panel.contains("260 km/h"),
        "{panel}"
    );
    play.frames(2, &[], &["tab"]);
    assert_eq!(play.text("Settings Text"), "[Tab] Settings");

    // Boost: the meter drains while Shift is held.
    play.frames(60, &["w", "left shift"], &[]);
    let boost = play.text("Boost Text");
    assert!(boost.contains('.'), "boost used: {boost}");
    // R puts the car back on the track.
    play.frames(2, &[], &["r"]);
    assert_eq!(play.text("Message Text"), "Back on track");
    drop(play);
    cleanup(&root);
}

// ---- 2D Platformer ----------------------------------------------------------------------------

/// Use the project's own tile assets (Rule Tiles and the like) wherever a tilemap asks for them,
/// whatever global project root other tests set.
fn use_project_tiles(root: &Path) {
    use misty_engine::tilemap::tile::{self, TileAsset};
    let Ok(entries) = std::fs::read_dir(root.join("Assets/Tiles")) else {
        return;
    };
    for path in entries.flatten().map(|e| e.path()) {
        if path.extension().is_some_and(|e| e == "tile") {
            let rel = path.strip_prefix(root).unwrap().to_string_lossy();
            tile::register(&rel.replace('\\', "/"), TileAsset::load(&path).unwrap());
        }
    }
}

/// New Project ▸ 2D Platformer: the sprites, tiles, palette and scripts with their meta files
/// (every reference resolving by path and GUID), Sorting Layers for 2D, and a scene with an
/// orthographic camera following the player, the tilemap level (Rule Tile ground with its
/// collider and shadows, water, walls), the player, ten coins, 2D lights, the coin counter and
/// a bloom volume. No sun, no 3D meshes; the Scene view starts in 2D.
#[test]
fn platformer_2d_template_has_a_level_a_player_and_coins() {
    use super::platformer_2d::{self as p2d, art};
    use misty_engine::asset_db::meta;
    use misty_engine::light2d::{Light2D, ShadowCaster2D};
    use misty_engine::scene::{
        BoxCollider2D, CircleCollider2D, Rigidbody2D, Scripts, SpriteRenderer, UiText,
    };
    use misty_engine::tilemap::palette::TilePalette;
    use misty_engine::tilemap::tile::{TileAsset, TileKind};
    use misty_engine::tilemap::{Grid, Tilemap, TilemapCollider2D, TilemapRenderer};

    let (root, scene) = create_project_checked(Template::Platformer2D);
    for (rel, source) in p2d::SCRIPTS {
        let on_disk = std::fs::read_to_string(root.join(rel)).unwrap();
        assert_eq!(on_disk, source, "{rel}");
    }
    let sprites = [
        art::PLAYER_PNG,
        art::COIN_PNG,
        art::GROUND_PNG,
        art::WATER_PNG,
        art::PROPS_PNG,
    ];
    for rel in sprites {
        let path = root.join(rel);
        let img = misty_engine::image::open(&path).unwrap();
        assert!(img.width() >= 16, "{rel}");
        let importer = misty_engine::sprites::TextureImporter::try_load(&path).unwrap();
        assert_eq!(importer.pixels_per_unit, art::PPU, "{rel}");
        assert_eq!(
            importer.filter_mode,
            misty_engine::sprites::FilterMode::Nearest
        );
    }
    let tiles = [art::GROUND_TILE, art::WATER_TILE, art::WALL_TILE];
    for rel in sprites
        .into_iter()
        .chain(tiles)
        .chain([p2d::PALETTE, p2d::CONTROLLER, p2d::FOLLOW])
    {
        assert!(
            meta::read_guid(&root.join(rel)).is_some(),
            "{rel} has no GUID"
        );
    }
    let ground = TileAsset::load(&root.join(art::GROUND_TILE)).unwrap();
    assert_eq!(ground.kind, TileKind::RuleTile);
    assert!(ground.rules.len() > 10);
    let water = TileAsset::load(&root.join(art::WATER_TILE)).unwrap();
    assert_eq!(water.kind, TileKind::AnimatedTile);
    assert_eq!(water.frames.len(), 4);
    let palette = TilePalette::load(&root.join(p2d::PALETTE)).unwrap();
    let mut in_palette: Vec<&str> = palette.tiles.iter().map(|t| t.tile.as_str()).collect();
    in_palette.sort();
    let mut expected = tiles.to_vec();
    expected.sort();
    assert_eq!(in_palette, expected, "the palette holds every tile");
    let deps = misty_engine::asset_deps::collect(&root, &[SCENE.to_owned()]);
    assert!(deps.missing.is_empty(), "{:?}", deps.missing);
    for rel in [SCENE, art::GROUND_TILE, art::WATER_TILE, p2d::PALETTE] {
        assert_refs_resolve(&root, rel);
    }
    // Only what the template uses: no materials, no sample scenes.
    assert!(!root.join("Assets/Materials").exists());
    assert!(!root.join("Assets/Scenes/Platformer2D.scene").exists());

    let settings = misty_engine::settings::ProjectSettings::load(&root).unwrap();
    assert_eq!(settings.sorting_layers, p2d::SORTING_LAYERS);
    assert_eq!(settings.physics2d.gravity, glam::Vec2::new(0.0, -9.81));
    assert_eq!(settings.tags_and_layers.name_to_layer("Player"), 5);
    assert!(settings.input.axes.iter().any(|a| a.name == "Horizontal"));

    // The camera: orthographic, following the player, starting inside the level.
    let cam = main_camera(&scene);
    {
        let c = scene.world.get::<&Camera>(cam).unwrap();
        assert!(c.orthographic);
        assert_eq!(c.ortho_size, p2d::ORTHO_SIZE);
    }
    let scripts = |e| -> Vec<String> {
        let s = scene.world.get::<&Scripts>(e).unwrap();
        s.list.iter().map(|r| r.path.clone()).collect()
    };
    assert_eq!(scripts(cam), [p2d::FOLLOW]);
    let eye = scene.world_position(cam);
    assert!(eye.z < 0.0 && (eye.truncate() - p2d::CAMERA_START).length() < 1e-4);
    assert_eq!(scene.active_with::<Camera>().len(), 1, "one camera");
    // No 3D: no sun, no meshes.
    assert!(scene.find_by_name("Directional Light").is_none());
    let meshes = scene
        .world
        .query::<&misty_engine::scene::MeshRenderer>()
        .iter()
        .count();
    assert_eq!(meshes, 0);

    // The level.
    let grid = scene.find_by_name("Grid").unwrap();
    assert!(scene.world.get::<&Grid>(grid).is_ok());
    let names: Vec<String> = scene
        .children(grid)
        .into_iter()
        .map(|e| scene.game_object(e).name.clone())
        .collect();
    assert_eq!(names, ["Stone Wall", "Ground", "Water"]);
    let ground = scene.find_by_name("Ground").unwrap();
    assert!(scene.world.get::<&Tilemap>(ground).unwrap().len() > 150);
    assert!(scene.world.get::<&TilemapCollider2D>(ground).is_ok());
    assert!(scene.world.get::<&ShadowCaster2D>(ground).is_ok());
    let layer = |name: &str| {
        let e = scene.find_by_name(name).unwrap();
        let r = scene.world.get::<&TilemapRenderer>(e).unwrap();
        (r.sorting_layer.clone(), r.order_in_layer)
    };
    assert_eq!(layer("Stone Wall").0, "Background");
    assert_eq!(layer("Ground").0, "Default");
    assert_eq!(layer("Water").0, "Foreground");
    // Drawn in the right order even before the project's Sorting Layers are installed.
    assert!(layer("Stone Wall").1 < layer("Ground").1 && layer("Ground").1 < layer("Water").1);

    // The player.
    let player = scene.find_by_name("Player").unwrap();
    assert_eq!(scene.game_object(player).layer, 5);
    assert!(
        scene
            .world
            .get::<&Rigidbody2D>(player)
            .unwrap()
            .freeze_rotation
    );
    assert!(scene.world.get::<&BoxCollider2D>(player).is_ok());
    assert!(scene.world.get::<&SpriteRenderer>(player).is_ok());
    assert_eq!(scripts(player), [p2d::CONTROLLER]);
    let feet = scene.world_position(player);
    assert!((feet.truncate() - p2d::PLAYER_START).length() < 1e-4);

    // Coins: triggers on Ignore Raycast (the ground check never sees them).
    let coins = scene.children(scene.find_by_name("Coins").unwrap());
    assert_eq!(coins.len(), p2d::COINS.len());
    for c in coins {
        assert!(scene.game_object(c).name.starts_with("Coin "));
        assert!(
            scene
                .world
                .get::<&CircleCollider2D>(c)
                .unwrap()
                .common
                .is_trigger
        );
        assert_eq!(scene.game_object(c).layer, 2);
    }
    let score = scene.find_by_name("Score Text").unwrap();
    assert_eq!(scene.world.get::<&UiText>(score).unwrap().text, "Coins: 0");

    // 2D lights: daylight and five shadowed torches.
    let lights = misty_engine::light2d::collect(&scene);
    assert_eq!(lights.items.len(), 6, "global + 5 torches");
    let global = scene.find_by_name("Global Light 2D").unwrap();
    assert!(scene.world.get::<&Light2D>(global).unwrap().intensity >= 1.0);
    // Glow: bloom, no clouds or sun shafts (there is no sky to put them in).
    let v = volume(&scene);
    assert!(v.enabled && v.bloom.active && v.bloom.intensity > 0.0);
    assert!(!v.clouds.active && !v.sun_shafts.active);

    let editor = crate::scene_files::EditorSettings::load(&root);
    assert_eq!(editor.recent_scenes, [SCENE]);
    assert!(editor.scene_view.is_none());
    let view = editor.scene_view_2d.expect("starts in 2D");
    assert_eq!(view.center, p2d::CAMERA_START.to_array());
    assert!(view.size > p2d::ORTHO_SIZE);
    cleanup(&root);
}

/// The editor opens a 2D Platformer project on its scene with the Scene view in 2D mode (and
/// remembers a moved 2D view). Play: the player lands on the tile collider; holding Space
/// jumps high, tapping W hops, Up jumps too; running right with the arrow key collects the
/// first three coins (the UI Text counts them) and the camera follows, without a script error.
#[test]
fn platformer_2d_opens_in_2d_and_plays() {
    use super::platformer_2d as p2d;
    let root = temp_project("Platformer2DPlays");
    create(&root, Template::Platformer2D).unwrap();
    let mut s = crate::state::EditorState::new(crate::project::ProjectBrowser::open(root.clone()));
    crate::scene_files::open_initial(&mut s);
    assert_eq!(
        s.scene.scene_path(s.scene.active_scene()).as_deref(),
        Some(SCENE)
    );
    assert!(s.scene.find_by_name("Player").is_some());
    crate::scene_files::restore_scene_view(&mut s);
    assert!(s.scene_view.is_2d && s.scene_view.camera.is_2d());
    let (center, size) = s.scene_view.camera.view_2d();
    assert!(
        (center.truncate() - p2d::CAMERA_START).length() < 1e-4,
        "{center}"
    );
    assert!((size - (p2d::ORTHO_SIZE + 0.5)).abs() < 1e-3, "{size}");
    // Looking down +Z at the XY plane.
    let forward = s.scene_view.camera.rotation() * glam::Vec3::Z;
    assert!((forward - glam::Vec3::Z).length() < 1e-4, "{forward}");
    // A moved 2D view is where the project reopens.
    s.scene_view
        .camera
        .set_2d_view(glam::Vec3::new(12.0, 3.0, 0.0), 9.0);
    crate::scene_files::remember_scene_view(&s);
    drop(s);
    let mut again =
        crate::state::EditorState::new(crate::project::ProjectBrowser::open(root.clone()));
    crate::scene_files::open_initial(&mut again);
    crate::scene_files::restore_scene_view(&mut again);
    assert!(again.scene_view.is_2d);
    let (center, size) = again.scene_view.camera.view_2d();
    assert!((center - glam::Vec3::new(12.0, 3.0, 0.0)).length() < 1e-3);
    assert!((size - 9.0).abs() < 1e-3);
    // Turning 2D off goes back to a 3D view.
    again.scene_view.is_2d = false;
    again.scene_view.camera.set_2d(false);
    assert!(!again.scene_view.camera.is_2d());
    drop(again);

    use_project_tiles(&root);
    let mut play = Play::start(&root, SCENE);
    play.frames(30, &[], &[]);
    let ground_y = 0.475;
    let stood = play.position("Player");
    assert!(
        (stood.y - ground_y).abs() < 0.05 && (stood.x - p2d::PLAYER_START.x).abs() < 0.1,
        "stands on the tiles: {stood}"
    );
    // Highest point of the player's feet over `n` frames with `key` pressed on the first one
    // (and held for `hold` frames).
    let mut jump = |play: &mut Play, key: &str, hold: usize, n: usize| -> f32 {
        let mut top = f32::MIN;
        for i in 0..n {
            let held: &[&str] = if i < hold { &[key] } else { &[] };
            let pressed: &[&str] = if i == 0 { &[key] } else { &[] };
            play.frames(1, held, pressed);
            top = top.max(play.position("Player").y);
        }
        top - ground_y
    };
    let full = jump(&mut play, "space", 30, 90);
    let landed = play.position("Player");
    assert!(full > 2.8, "held Space: a full jump ({full} m)");
    assert!((landed.y - ground_y).abs() < 0.05, "landed: {landed}");
    let hop = jump(&mut play, "w", 1, 90);
    assert!(hop > 0.5 && hop < full * 0.6, "tapped W: a hop ({hop} m)");
    let up = jump(&mut play, "up", 30, 90);
    assert!(up > 2.8, "held Up: a full jump ({up} m)");

    // Run right along the ground: the three coins on the way are collected.
    play.frames(156, &["right"], &[]);
    let ran = play.position("Player");
    assert!(
        ran.x > 3.0 && ran.x < 6.6,
        "ran right, short of the pit: {ran}"
    );
    assert!((ran.y - ground_y).abs() < 0.05, "on the ground: {ran}");
    for i in 1..=3 {
        assert!(
            play.scene.find_by_name(&format!("Coin {i}")).is_none(),
            "Coin {i} collected"
        );
    }
    assert!(play.scene.find_by_name("Coin 4").is_some());
    assert_eq!(play.text("Score Text"), "Coins: 3");
    let cam = play.position("Main Camera");
    assert!(
        cam.x > p2d::CAMERA_START.x + 2.0,
        "the camera follows: {cam}"
    );
    assert!(
        cam.z == -10.0 && cam.y >= p2d::CAMERA_START.y - 1e-3,
        "{cam}"
    );
    drop(play);
    cleanup(&root);
}

// ---- Rendering --------------------------------------------------------------------------------

/// Render the template's scene headlessly (terrain data made absolute and the project's tiles
/// registered, so the global project root other tests use is left alone; spline extrudes
/// built first).
fn render(root: &Path, scene: &mut Scene) -> crate::sample_render::Image {
    let gpu = crate::sample_render::headless().unwrap();
    use_project_tiles(root);
    for e in scene.active_with::<Terrain>() {
        let mut t = scene.world.get::<&mut Terrain>(e).unwrap();
        t.terrain_data = root.join(&t.terrain_data).to_string_lossy().into_owned();
    }
    misty_engine::splines::update(scene);
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
        let (mut scene, _) = Scene::load(&root.join(t.scene())).unwrap();
        let img = render(&root, &mut scene);
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

/// `MISTY_TEMPLATE_THUMBS=Racing,First Person` renders only those.
#[test]
#[ignore = "writes assets/templates/*.png; run on purpose to re-render the thumbnails"]
fn write_template_thumbnails() {
    let out = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("assets/templates");
    let only = std::env::var("MISTY_TEMPLATE_THUMBS").unwrap_or_default();
    for t in Template::ALL {
        let picked = only.split(',').any(|n| Template::from_name(n) == Some(t));
        if !only.is_empty() && !picked {
            continue;
        }
        let root = temp_project(&format!("Thumb{}", t.name()));
        create(&root, t).unwrap();
        // Alone on purpose: terrain textures resolve against the project root.
        misty_engine::assets::set_project_root(&root);
        let (mut scene, _) = Scene::load(&root.join(t.scene())).unwrap();
        let img = render(&root, &mut scene);
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
    super::create_with(&root, Template::FloatingPlatform, true).unwrap();
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
        create(&root, Template::FloatingPlatform).unwrap();
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
    super::create_template(&root, Template::FloatingPlatform).unwrap();
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
    let err = create(&blocker.join("My Game"), Template::FloatingPlatform).unwrap_err();
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
    // Post-processing like every template: bloom, tonemapping, volumetric clouds, sun shafts.
    assert_post_processing("MeadowDemo", &scene);
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
    // The rocks' collider meshes by absolute path too (other tests move the project root).
    for f in scene
        .world
        .query::<&mut misty_engine::scene::MeshFilter>()
        .iter()
    {
        if let Some(misty_engine::scene::MeshRef::Asset(key)) = &mut f.mesh
            && key.starts_with("Assets/")
        {
            *key = root.join(key.as_str()).to_string_lossy().into_owned();
        }
    }
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
