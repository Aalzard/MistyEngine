//! First Person Player: the `Player ▸ HeadBobVertical ▸ HeadBobHorizontal ▸ Camera` rig with a
//! Character Controller, `FirstPersonController.lua` (locomotion + mouse look) and the three
//! head bob scripts (vertical, horizontal, rotation), each adding its offset on top of the
//! pose the controller sets.
//!
//! GameObject ▸ First Person Player builds it in any project (the scripts are written into
//! `Assets/Scripts` when missing). The sample project also has it as
//! `Assets/Prefabs/First Person Player.prefab` and the `FirstPersonDemo` scene (a test course
//! with ramps, stairs, platforms, a bridge and a low tunnel), both written by the ignored
//! `write_first_person_demo` test here; the other tests check the files still match and play
//! the rig.

use std::path::Path;

use glam::Vec3;
use misty_engine::hecs::Entity;
use misty_engine::physics::layers;
use misty_engine::scene::{
    AudioListener, Camera, CharacterController, Scene, ScriptRef, ScriptValue, Scripts, Transform,
};

use crate::state::EditorState;

pub const CONTROLLER: &str = "Assets/Scripts/FirstPersonController.lua";
pub const BOB_VERTICAL: &str = "Assets/Scripts/HeadBobVertical.lua";
pub const BOB_HORIZONTAL: &str = "Assets/Scripts/HeadBobHorizontal.lua";
pub const BOB_ROTATION: &str = "Assets/Scripts/HeadBobRotation.lua";
pub const PREFAB: &str = "Assets/Prefabs/First Person Player.prefab";
#[cfg(test)]
pub const SCENE: &str = "Assets/Scenes/FirstPersonDemo.scene";
pub const NAME: &str = "First Person Player";

/// The scripts as shipped: the sample project's files are the source.
pub const SCRIPTS: [(&str, &str); 4] = [
    (
        CONTROLLER,
        include_str!("../../../SampleProject/Assets/Scripts/FirstPersonController.lua"),
    ),
    (
        BOB_VERTICAL,
        include_str!("../../../SampleProject/Assets/Scripts/HeadBobVertical.lua"),
    ),
    (
        BOB_HORIZONTAL,
        include_str!("../../../SampleProject/Assets/Scripts/HeadBobHorizontal.lua"),
    ),
    (
        BOB_ROTATION,
        include_str!("../../../SampleProject/Assets/Scripts/HeadBobRotation.lua"),
    ),
];

/// Eye height above the feet (the camera's rest position).
pub const EYE_HEIGHT: f32 = 1.7;
pub const CAPSULE_HEIGHT: f32 = 1.8;
pub const CAPSULE_RADIUS: f32 = 0.35;

/// Head bob as tuned in the MeadowDemo (script field name, value).
const BOB_VERTICAL_FIELDS: [(&str, f64); 3] = [
    ("breathingRate", 0.2),
    ("runAmplitude", 0.025),
    ("walkAmplitude", 0.015),
];
const BOB_HORIZONTAL_FIELDS: [(&str, f64); 3] = [
    ("runAmplitude", 0.0175),
    ("strafeLean", 0.025),
    ("walkAmplitude", 0.01),
];
const BOB_ROTATION_FIELDS: [(&str, f64); 1] = [("pitchBob", 3.0)];

fn script(path: &str, fields: &[(&str, f64)]) -> Scripts {
    Scripts {
        list: vec![ScriptRef {
            path: path.into(),
            fields: fields
                .iter()
                .map(|(k, v)| ((*k).to_owned(), ScriptValue::Number(*v)))
                .collect(),
            ..Default::default()
        }],
    }
}

/// Build the rig under `parent` (feet at the origin) and return the Player.
pub fn build(scene: &mut Scene, parent: Option<Entity>) -> Entity {
    let player = scene.spawn("Player", parent);
    scene.game_object_mut(player).tag = "Player".into();
    scene.game_object_mut(player).layer = layers::name_to_layer("Player") as u8;
    scene.add(
        player,
        CharacterController {
            height: CAPSULE_HEIGHT,
            radius: CAPSULE_RADIUS,
            center: Vec3::new(0.0, CAPSULE_HEIGHT * 0.5, 0.0),
            step_offset: 0.35,
            slope_limit: 50.0,
            ..Default::default()
        },
    );
    scene.add(player, script(CONTROLLER, &[]));

    let vertical = scene.spawn("HeadBobVertical", Some(player));
    scene.set_transform(
        vertical,
        Transform::from_position(Vec3::new(0.0, EYE_HEIGHT, 0.0)),
    );
    scene.add(vertical, script(BOB_VERTICAL, &BOB_VERTICAL_FIELDS));

    let horizontal = scene.spawn("HeadBobHorizontal", Some(vertical));
    scene.add(horizontal, script(BOB_HORIZONTAL, &BOB_HORIZONTAL_FIELDS));

    let camera = scene.spawn("Camera", Some(horizontal));
    scene.game_object_mut(camera).tag = "MainCamera".into();
    scene.add(
        camera,
        Camera {
            // Close to walls: the capsule radius is 0.35 m.
            near: 0.1,
            ..Default::default()
        },
    );
    scene.add(camera, AudioListener::default());
    scene.add(camera, script(BOB_ROTATION, &BOB_ROTATION_FIELDS));
    player
}

/// The rig as a prefab asset.
#[cfg(test)]
pub fn prefab() -> misty_engine::scene::prefab::Prefab {
    use misty_engine::scene::prefab::Prefab;
    let mut scene = Scene::new("prefab");
    let root = build(&mut scene, None);
    Prefab::from_scene(&scene, root)
}

/// Write the scripts the rig needs into the project when they are missing; returns what was
/// written.
pub fn ensure_scripts(root: &Path) -> Result<Vec<String>, String> {
    let mut written = Vec::new();
    for (rel, source) in SCRIPTS {
        let path = root.join(rel);
        if path.exists() {
            continue;
        }
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
        }
        std::fs::write(&path, source).map_err(|e| format!("{rel}: {e}"))?;
        written.push(rel.to_owned());
    }
    Ok(written)
}

/// GameObject ▸ First Person Player: the project's prefab when it has one, else the rig built
/// here (with its scripts added to the project), at the Scene view pivot or under `parent`.
pub fn create(s: &mut EditorState, parent: Option<Entity>) {
    let parent = parent.filter(|p| s.scene.contains(*p));
    if s.project.root.join(PREFAB).is_file() {
        if crate::prefabs::instantiate(s, PREFAB, parent).is_some() {
            s.console.info(format!("Created '{NAME}' from {PREFAB}."));
        }
        return;
    }
    match ensure_scripts(&s.project.root) {
        Ok(written) if !written.is_empty() => {
            s.refresh_assets();
            s.console
                .info(format!("Added {} to the project.", written.join(", ")));
        }
        Ok(_) => {}
        Err(e) => {
            s.console.warn(format!("Could not write the scripts: {e}"));
            return;
        }
    }
    s.record(&format!("Create {NAME}"));
    let e = build(&mut s.scene, parent);
    if parent.is_none() {
        let pivot = s.scene_view.camera.pivot().round();
        s.scene.set_world_position(e, pivot);
    }
    s.select(e);
    s.console.info(format!(
        "Created '{NAME}': WASD move, Shift run, Space jump, Ctrl crouch, Esc releases the cursor."
    ));
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use glam::Vec2;
    use misty_engine::audio::Audio;
    use misty_engine::input::InputAxis;
    use misty_engine::runtime::{Runtime, Time};
    use misty_engine::scene::prefab::Prefab;
    use misty_engine::scene::{
        BoxCollider, MeshFilter, MeshRenderer, PrimitiveMesh, RectTransform, Rigidbody, UiText,
    };
    use misty_engine::scripting::{CursorLockMode, Input, LogKind};
    use misty_engine::settings::ProjectSettings;

    use super::*;
    use crate::actions::{Action, Create};
    use crate::project::ProjectBrowser;
    use crate::ui_create::UiElement;

    const CHECKER: &str = "Assets/Materials/Checker.mat";
    const WOOD: &str = "Assets/Materials/Wood.mat";
    const STEEL: &str = "Assets/Materials/Blue Steel.mat";
    const CRATE: &str = "Assets/Materials/AO Crate.mat";
    const RED: &str = "Assets/Materials/Red.mat";

    fn project() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../SampleProject")
    }

    /// A cube with a Box Collider.
    fn block(s: &mut Scene, name: &str, t: Transform, mat: &str) -> Entity {
        let e = s.spawn(name, None);
        s.set_transform(e, t);
        s.add(e, MeshFilter::primitive(PrimitiveMesh::Cube));
        s.add(
            e,
            MeshRenderer {
                material: Some(mat.into()),
                ..Default::default()
            },
        );
        s.add(e, BoxCollider::default());
        e
    }

    fn scaled(pos: Vec3, scale: Vec3) -> Transform {
        let mut t = Transform::from_position(pos);
        t.scale = scale;
        t
    }

    /// A ramp rising towards +z from `start` over `length` metres at `angle` degrees.
    fn ramp(s: &mut Scene, name: &str, start: Vec3, length: f32, angle: f32) {
        let rad = angle.to_radians();
        let thickness = 0.3;
        let center = start
            + Vec3::new(
                0.0,
                length * 0.5 * rad.sin() - thickness * 0.5 * rad.cos(),
                length * 0.5 * rad.cos(),
            );
        block(
            s,
            name,
            scaled(center, Vec3::new(3.0, thickness, length))
                .with_euler(Vec3::new(-angle, 0.0, 0.0)),
            WOOD,
        );
        // A landing at the top.
        let rise = length * rad.sin();
        block(
            s,
            &format!("{name} Top"),
            scaled(
                start + Vec3::new(0.0, rise - 0.15, length * rad.cos() + 1.5),
                Vec3::new(3.0, 0.3, 3.0),
            ),
            WOOD,
        );
    }

    /// The test course: floor, 15° and 30° ramps, stairs, platforms to jump between, a narrow
    /// bridge, a low tunnel to crouch through, crates to push, the player prefab and a hint.
    fn demo_scene() -> Scene {
        let mut st = EditorState::new(ProjectBrowser::open(
            std::env::temp_dir().join("misty_first_person_demo_build"),
        ));
        st.scene = Scene::default_3d("FirstPersonDemo");
        let s = &mut st.scene;
        // The player prefab brings its own camera.
        if let Some(cam) = s.find_by_name("Main Camera") {
            s.despawn(cam);
        }
        block(
            s,
            "Floor",
            scaled(Vec3::new(0.0, -0.5, 10.0), Vec3::new(60.0, 1.0, 60.0)),
            CHECKER,
        );
        // Ramps on the left.
        ramp(s, "Ramp 15", Vec3::new(-8.0, 0.0, 2.0), 8.0, 15.0);
        ramp(s, "Ramp 30", Vec3::new(-13.0, 0.0, 2.0), 6.0, 30.0);
        // Stairs on the right: 20 cm risers, 45 cm treads (a 0.35 m capsule needs treads about
        // as deep as its radius to rest on them).
        for i in 0..8 {
            let h = 0.2 * (i + 1) as f32;
            block(
                s,
                &format!("Stair {}", i + 1),
                scaled(
                    Vec3::new(8.0, h * 0.5, 3.0 + i as f32 * 0.45),
                    Vec3::new(3.0, h, 0.45),
                ),
                WOOD,
            );
        }
        // The landing at the top of the eighth step (1.6 m up, past z = 6.375).
        block(
            s,
            "Stairs Top",
            scaled(Vec3::new(8.0, 1.6 - 0.15, 8.375), Vec3::new(3.0, 0.3, 4.0)),
            WOOD,
        );
        // Platforms ahead with growing gaps (1.8, 2.4, 3.0 m): walk, then run and jump.
        let mut z = 12.0;
        for (i, gap) in [0.0_f32, 1.8, 2.4, 3.0].into_iter().enumerate() {
            z += gap + if i == 0 { 0.0 } else { 2.0 };
            block(
                s,
                &format!("Platform {}", i + 1),
                scaled(Vec3::new(0.0, 0.6, z), Vec3::new(2.5, 0.3, 2.0)),
                STEEL,
            );
        }
        // A narrow bridge between two towers on the right.
        for (name, z) in [("Tower A", 14.0), ("Tower B", 24.0)] {
            block(
                s,
                name,
                scaled(Vec3::new(14.0, 0.75, z), Vec3::new(3.0, 1.5, 3.0)),
                STEEL,
            );
        }
        block(
            s,
            "Bridge",
            scaled(Vec3::new(14.0, 1.4, 19.0), Vec3::new(0.6, 0.2, 7.0)),
            WOOD,
        );
        block(
            s,
            "Tower Ramp",
            scaled(Vec3::new(14.0, 0.6, 10.9), Vec3::new(3.0, 0.3, 4.0))
                .with_euler(Vec3::new(-22.0, 0.0, 0.0)),
            WOOD,
        );
        // A low tunnel on the left: 1.4 m of headroom, crouch to get through.
        block(
            s,
            "Tunnel Roof",
            scaled(Vec3::new(-8.0, 1.5, 16.0), Vec3::new(3.0, 0.2, 4.0)),
            RED,
        );
        for (name, x) in [("Tunnel Wall L", -9.6), ("Tunnel Wall R", -6.4)] {
            block(
                s,
                name,
                scaled(Vec3::new(x, 0.8, 16.0), Vec3::new(0.2, 1.6, 4.0)),
                RED,
            );
        }
        // Crates to shove.
        for (i, p) in [(3.0, 0.4, 6.0), (3.9, 0.4, 6.5), (3.4, 1.2, 6.2)]
            .into_iter()
            .enumerate()
        {
            let c = block(
                s,
                &format!("Crate {}", i + 1),
                scaled(Vec3::from(p), Vec3::splat(0.8)),
                CRATE,
            );
            s.add(c, Rigidbody::default());
        }
        let player = s.instantiate_prefab(&prefab(), Some(PREFAB), None);
        s.set_world_position(player, Vec3::new(0.0, 0.05, -4.0));

        // Controls hint in the top-left corner.
        crate::commands::apply(
            &mut st,
            Action::Create {
                what: Create::Ui(UiElement::Text),
                parent: None,
            },
        );
        let text = st.selection.active().expect("created text is selected");
        st.scene.game_object_mut(text).name = "Hint Text".into();
        let w = &mut st.scene.world;
        *w.get::<&mut RectTransform>(text).unwrap() = RectTransform {
            anchor_min: Vec2::new(0.0, 1.0),
            anchor_max: Vec2::new(0.0, 1.0),
            pivot: Vec2::new(0.0, 1.0),
            anchored_position: Vec2::new(20.0, -20.0),
            size_delta: Vec2::new(900.0, 80.0),
        };
        {
            let mut t = w.get::<&mut UiText>(text).unwrap();
            t.text =
                "WASD move   Shift run   Space jump   Ctrl crouch   Mouse look   Esc cursor".into();
            t.font_size = 24.0;
            t.color = [1.0; 4];
        }
        st.scene
    }

    /// The sample project's input axes with Run and Crouch (left shift / left ctrl, left stick
    /// press / B on a gamepad).
    fn with_fps_axes(mut p: ProjectSettings) -> ProjectSettings {
        let defaults = misty_engine::input::InputSettings::default();
        for name in ["Run", "Crouch"] {
            if p.input.axes.iter().any(|a| a.name == name) {
                continue;
            }
            let extra: Vec<InputAxis> = defaults
                .axes
                .iter()
                .filter(|a| a.name == name)
                .cloned()
                .collect();
            p.input.axes.extend(extra);
        }
        p
    }

    #[test]
    #[ignore = "writes into SampleProject; run on purpose to regenerate"]
    fn write_first_person_demo() {
        let root = project();
        // The prefab file may carry a GUID table the editor added: keep it when the rig is
        // the same.
        let same = std::fs::read_to_string(root.join(PREFAB))
            .is_ok_and(|text| without_guids(&text) == without_guids(&prefab().to_json()));
        if !same {
            prefab().save(&root.join(PREFAB)).unwrap();
        }
        demo_scene().save(&root.join(SCENE)).unwrap();
        with_fps_axes(ProjectSettings::load(&root).unwrap())
            .save(&root)
            .unwrap();
    }

    /// A JSON asset file without its GUID table (added on save in the editor).
    fn without_guids(text: &str) -> serde_json::Value {
        let mut v: serde_json::Value = serde_json::from_str(text).expect("json");
        if let Some(o) = v.as_object_mut() {
            o.remove(misty_engine::asset_db::refs::KEY);
        }
        v
    }

    #[test]
    fn first_person_demo_matches_its_generator() {
        let root = project();
        let text = std::fs::read_to_string(root.join(PREFAB)).unwrap();
        assert_eq!(
            without_guids(&text),
            without_guids(&prefab().to_json()),
            "regenerate with write_first_person_demo"
        );
        let text = std::fs::read_to_string(root.join(SCENE)).unwrap();
        assert_eq!(
            text.replace("\r\n", "\n"),
            demo_scene().to_json(),
            "regenerate with write_first_person_demo"
        );
        let (_, report) = Scene::from_json("FirstPersonDemo", &text).unwrap();
        assert!(report.warnings.is_empty(), "{:?}", report.warnings);
        let p = ProjectSettings::load(&root).unwrap();
        for name in ["Run", "Crouch", "Mouse X", "Mouse Y", "Jump"] {
            assert!(
                p.input.axes.iter().any(|a| a.name == name),
                "the sample project has a {name} axis"
            );
        }
        for (rel, source) in SCRIPTS {
            let on_disk = std::fs::read_to_string(root.join(rel)).unwrap();
            assert_eq!(
                on_disk.replace("\r\n", "\n"),
                source.replace("\r\n", "\n"),
                "{rel}"
            );
        }
    }

    #[test]
    fn prefab_round_trips_with_the_rig() {
        let p = prefab();
        let (loaded, report) = Prefab::from_json(&p.to_json()).unwrap();
        assert!(report.warnings.is_empty(), "{:?}", report.warnings);
        assert_eq!(loaded, p);
        assert_eq!(
            loaded.outline(),
            [
                ("Player".to_owned(), 0),
                ("HeadBobVertical".to_owned(), 1),
                ("HeadBobHorizontal".to_owned(), 2),
                ("Camera".to_owned(), 3),
            ]
        );
        let roots = loaded.root_components();
        assert!(
            roots.iter().any(|c| c == "Character Controller"),
            "{roots:?}"
        );
        let mut scene = Scene::new("t");
        let e = scene.instantiate_prefab(&loaded, Some(PREFAB), None);
        let cam = scene.find_by_name("Camera").unwrap();
        assert!(scene.world.get::<&Camera>(cam).is_ok());
        assert!(scene.world.get::<&AudioListener>(cam).is_ok());
        assert_eq!(scene.game_object(cam).tag, "MainCamera");
        assert_eq!(scene.prefab_root(cam), Some(e));
        let eye = scene.world_position(cam).y - scene.world_position(e).y;
        assert!((eye - EYE_HEIGHT).abs() < 1e-5, "eye height {eye}");
    }

    #[test]
    fn create_builds_the_rig_and_writes_the_scripts() {
        let root = std::env::temp_dir().join(format!("misty_fps_create_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let mut s = EditorState::new(ProjectBrowser::open(root.clone()));
        create(&mut s, None);
        let player = s.selection.active().expect("the Player is selected");
        assert_eq!(s.scene.name(player), "Player");
        assert!(s.scene.world.get::<&CharacterController>(player).is_ok());
        assert_eq!(s.scene.children(player).len(), 1);
        for (rel, source) in SCRIPTS {
            assert_eq!(std::fs::read_to_string(root.join(rel)).unwrap(), source);
        }
        s.undo_step();
        assert!(!s.scene.contains(player), "undoable");
        // A second one keeps the scripts as they are (never overwritten).
        std::fs::write(root.join(CONTROLLER), "-- mine").unwrap();
        create(&mut s, None);
        assert_eq!(
            std::fs::read_to_string(root.join(CONTROLLER)).unwrap(),
            "-- mine"
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    /// Lua-level unit tests of the locomotion, look and bob maths (no scene).
    fn lua(chunk: &str) {
        let scripts: Vec<(&str, &str)> = SCRIPTS
            .iter()
            .map(|(rel, src)| {
                let name = Path::new(rel).file_stem().unwrap().to_str().unwrap();
                (name, *src)
            })
            .collect();
        if let Err(e) = misty_engine::scripting::test_scripts(&scripts, chunk) {
            panic!("{e}");
        }
    }

    const LUA_PRELUDE: &str = r#"
        local T = FirstPersonController
        local L = T.Locomotion
        local dt = 1 / 60
        local function fresh() return L.new(T) end
        -- Free movement: the measured velocity is the intended one.
        local function walk(l, frames, move, run, grounded, jump)
            local last
            for i = 1, frames do
                local vx, vy, vz = l:step({ move = move, forward = move.z, run = run, jump = (jump == i) }, dt, grounded)
                l:afterMove(grounded, { x = vx, y = vy, z = vz }, dt)
                last = { vx = vx, vy = vy, vz = vz }
            end
            return last
        end
        local FWD = { x = 0, z = 1 }
        local STILL = { x = 0, z = 0 }
    "#;

    #[test]
    fn lua_speed_blends_to_walk_and_run_without_overshoot() {
        lua(&format!(
            r#"{LUA_PRELUDE}
            local l = fresh()
            local peak = 0
            for i = 1, 60 do
                walk(l, 1, FWD, false, true)
                peak = math.max(peak, l.speed)
            end
            assert(math.abs(l.speed - 4) < 0.05, "walks at 4 m/s: " .. l.speed)
            assert(peak <= 4.0001, "no overshoot: " .. peak)
            assert(l.state == "Walk", l.state)
            -- The first frames accelerate gently (SmoothDamp), not a step.
            local l2 = fresh()
            walk(l2, 1, FWD, false, true)
            assert(l2.speed > 0 and l2.speed < 1.5, "eases in: " .. l2.speed)
            -- Shift: run.
            peak = 0
            for i = 1, 60 do
                walk(l, 1, FWD, true, true)
                peak = math.max(peak, l.speed)
            end
            assert(math.abs(l.speed - 7) < 0.05, "runs at 7 m/s: " .. l.speed)
            assert(peak <= 7.0001, "no overshoot: " .. peak)
            assert(l.state == "Run", l.state)
            -- Stop: decelerates smoothly to Idle.
            local before = l.speed
            walk(l, 1, STILL, false, true)
            assert(l.speed < before and l.speed > before - 3, "decelerates smoothly: " .. l.speed)
            walk(l, 40, STILL, false, true)
            assert(l.speed < 0.05, "stops: " .. l.speed)
            assert(l.state == "Idle", l.state)
            -- Air control: slower acceleration in the air.
            local air = fresh()
            walk(air, 10, FWD, false, false)
            local ground = fresh()
            walk(ground, 10, FWD, false, true)
            assert(air.speed < ground.speed * 0.6, "air control " .. air.speed .. " vs " .. ground.speed)
            "#
        ));
    }

    #[test]
    fn lua_coyote_time_and_jump_buffer() {
        lua(&format!(
            r#"{LUA_PRELUDE}
            -- Coyote: a jump 0.08 s after leaving the ground still works.
            local l = fresh()
            walk(l, 30, FWD, false, true)
            walk(l, 5, FWD, false, false)
            local vx, vy = l:step({{ move = FWD, jump = true }}, dt, false)
            assert(vy > 3, "coyote jump: " .. vy)
            assert(l.jumped and l.state == "Jump", l.state)
            -- ... but not 0.2 s after.
            l = fresh()
            walk(l, 30, FWD, false, true)
            walk(l, 12, FWD, false, false)
            vx, vy = l:step({{ move = FWD, jump = true }}, dt, false)
            assert(vy < 0, "too late: " .. vy)
            -- Buffer: a press 0.1 s before landing jumps on the landing frame.
            l = fresh()
            walk(l, 30, FWD, false, false)
            l:step({{ move = FWD, jump = true }}, dt, false)
            l:afterMove(false, {{ x = 0, y = l.vy, z = 0 }}, dt)
            walk(l, 5, FWD, false, false)
            vx, vy = l:step({{ move = FWD }}, dt, true)
            assert(vy > 3, "buffered jump: " .. vy)
            -- ... a press 0.25 s before landing is forgotten.
            l = fresh()
            walk(l, 30, FWD, false, false)
            l:step({{ move = FWD, jump = true }}, dt, false)
            l:afterMove(false, {{ x = 0, y = l.vy, z = 0 }}, dt)
            walk(l, 15, FWD, false, false)
            vx, vy = l:step({{ move = FWD }}, dt, true)
            assert(vy <= 0, "forgotten: " .. vy)
            -- Gravity stops at the terminal velocity.
            l = fresh()
            walk(l, 600, STILL, false, false)
            assert(l.vy >= -40 - 1e-4 and l.vy < -39, "terminal: " .. l.vy)
            assert(l.state == "Fall", l.state)
            -- Jump height: apex near 1.2 m.
            l = fresh()
            walk(l, 10, STILL, false, true)
            l:step({{ move = STILL, jump = true }}, dt, true)
            local y, apex = 0, 0
            for i = 1, 120 do
                local _, vy = l:step({{ move = STILL }}, dt, false)
                y = y + vy * dt
                apex = math.max(apex, y)
                l:afterMove(false, {{ x = 0, y = vy, z = 0 }}, dt)
            end
            assert(apex > 1.0 and apex < 1.4, "apex " .. apex)
            "#
        ));
    }

    #[test]
    fn lua_phase_advances_by_distance_and_landing_is_measured() {
        lua(&format!(
            r#"{LUA_PRELUDE}
            local l = fresh()
            -- 4 m/s for 0.8 s = 3.2 m = two 1.6 m steps = one stride = 2π.
            local total = 0
            local last = 0
            for i = 1, 48 do
                l:step({{ move = FWD }}, dt, true)
                l:afterMove(true, {{ x = 0, y = 0, z = 4 }}, dt)
                local d = (l.phase - last) % (2 * math.pi)
                total = total + d
                last = l.phase
            end
            assert(math.abs(total - 2 * math.pi) < 0.05, "one stride: " .. total)
            assert(l.phaseRate > 7 and l.phaseRate < 9, "rate " .. l.phaseRate)
            -- Standing still: the phase holds (no sliding bob).
            local held = l.phase
            for i = 1, 30 do
                l:step({{ move = STILL }}, dt, true)
                l:afterMove(true, {{ x = 0, y = 0, z = 0 }}, dt)
            end
            assert(l.phase == held, "phase holds")
            assert(l.phaseRate == 0)
            -- In the air the phase holds too.
            for i = 1, 10 do
                l:step({{ move = FWD }}, dt, false)
                l:afterMove(false, {{ x = 0, y = -5, z = 4 }}, dt)
            end
            assert(l.phase == held, "no bob in the air")
            -- Landing from a fall at 8 m/s: half impulse (4..12 m/s).
            l.vy = -8
            l:afterMove(true, {{ x = 0, y = -8, z = 0 }}, dt)
            assert(math.abs(l.landing - 0.5) < 0.01, "impulse " .. l.landing)
            assert(l.state == "Land", l.state)
            local s = l:publish(0.5, 1, 90, -10)
            assert(s.landing == l.landing and s.strafe == 0.5 and s.turnRate == 90 and s.pitch == -10)
            assert(s.grounded and s.speed01 == 0)
            -- A soft landing (3 m/s) makes no impulse.
            l.grounded = false
            l.vy = -3
            l:afterMove(true, {{ x = 0, y = -3, z = 0 }}, dt)
            assert(l.landing == 0)
            "#
        ));
    }

    #[test]
    fn lua_mouse_look_yaw_wraps_and_pitch_clamps() {
        lua(r#"
            local yaw, pitch = FirstPersonController.look(350, 0, 10, 0, 2, -85, 85)
            assert(math.abs(yaw - 10) < 1e-6, "wraps: " .. yaw)
            yaw, pitch = FirstPersonController.look(0, 0, -5, 100, 2, -85, 85)
            assert(math.abs(yaw - 350) < 1e-6, "negative wraps: " .. yaw)
            assert(pitch == -85, "looking up clamps: " .. pitch)
            yaw, pitch = FirstPersonController.look(0, 0, 0, -100, 2, -85, 85)
            assert(pitch == 85, "looking down clamps: " .. pitch)
            yaw, pitch = FirstPersonController.look(0, 0, 0, -10, 2, -85, 85)
            assert(math.abs(pitch - 20) < 1e-6, "sensitivity: " .. pitch)
        "#);
    }

    const BOB_PRELUDE: &str = r#"
        local dt = 1 / 60
        local function bob(class)
            local b = setmetatable({}, { __index = class })
            b:_init()
            return b
        end
        local function signal()
            return { state = "Idle", speed = 0, speed01 = 0, walkSpeed = 4, runSpeed = 7, grounded = true,
                airTime = 0, phase = 0, phaseRate = 0, landing = 0, strafe = 0, forward = 0, turnRate = 0,
                crouch = 0, pitch = 0, jumped = false }
        end
        -- Walk the signal at `speed` for `frames`.
        local function advance(sig, speed, frames, f)
            for i = 1, frames do
                sig.speed = speed
                if sig.grounded then sig.phase = (sig.phase + speed / 3.2 * 2 * math.pi * dt) % (2 * math.pi) end
                f()
            end
        end
    "#;

    #[test]
    fn lua_vertical_bob_blends_with_speed_and_settles_when_stopping() {
        lua(&format!(
            r#"{BOB_PRELUDE}
            local v = bob(HeadBobVertical)
            local sig = signal()
            local peak, maxJump, last = 0, 0, 0
            advance(sig, 4, 120, function()
                local o = v:Evaluate(sig, dt)
                peak = math.max(peak, math.abs(o))
                maxJump = math.max(maxJump, math.abs(o - last))
                last = o
            end)
            assert(peak > 0.025 and peak < 0.035, "walk amplitude ~3 cm: " .. peak)
            assert(sig.verticalBob ~= nil and sig.verticalBobVelocity ~= nil, "published")
            -- Running: larger, still smooth.
            peak = 0
            advance(sig, 7, 120, function()
                local o = v:Evaluate(sig, dt)
                peak = math.max(peak, math.abs(o))
                maxJump = math.max(maxJump, math.abs(o - last))
                last = o
            end)
            assert(peak > 0.045 and peak < 0.055, "run amplitude ~5 cm: " .. peak)
            assert(maxJump < 0.025, "never jumps: " .. maxJump)
            -- Stop: back to (nearly) zero within half a second, no hard reset.
            local first = true
            advance(sig, 0, 30, function()
                local o = v:Evaluate(sig, dt)
                if first then
                    assert(math.abs(o - last) < 0.012, "no reset on stop: " .. (o - last))
                    first = false
                end
                last = o
            end)
            assert(math.abs(last) < 0.008, "settled: " .. last)
            -- In the air: no bob.
            local a = bob(HeadBobVertical)
            local air = signal()
            air.grounded = false
            local airPeak = 0
            advance(air, 7, 60, function() airPeak = math.max(airPeak, math.abs(a:Evaluate(air, dt))) end)
            assert(airPeak < 1e-6, "no bob in the air: " .. airPeak)
            -- Crouching lowers the eyes.
            air.crouch = 1
            assert(math.abs(a:Evaluate(air, dt) + 0.6) < 0.01, "crouch drop")
            "#
        ));
    }

    #[test]
    fn lua_landing_impulse_dips_and_decays() {
        lua(&format!(
            r#"{BOB_PRELUDE}
            local v = bob(HeadBobVertical)
            local sig = signal()
            sig.landing = 1
            local lowest, t = 0, 0
            local o = v:Evaluate(sig, dt)
            sig.landing = 0
            for i = 1, 90 do
                o = v:Evaluate(sig, dt)
                if o < lowest then lowest = o; t = i * dt end
            end
            assert(lowest < -0.06 and lowest > -0.16, "dip about 10 cm: " .. lowest)
            assert(t < 0.12, "dips quickly: " .. t)
            assert(math.abs(o) < 0.003, "settled after 1.5 s: " .. o)
            -- The rotation kick does the same on the pitch.
            local r = bob(HeadBobRotation)
            sig.landing = 1
            local pitch = r:Evaluate(sig, dt)
            sig.landing = 0
            local top = 0
            for i = 1, 90 do
                pitch = r:Evaluate(sig, dt)
                top = math.max(top, pitch)
            end
            assert(top > 1.0 and top <= 5, "pitch kick: " .. top)
            assert(math.abs(pitch) < 0.1, "springs back: " .. pitch)
            "#
        ));
    }

    #[test]
    fn lua_horizontal_sway_and_rotation_stay_within_clamps() {
        lua(&format!(
            r#"{BOB_PRELUDE}
            local h = bob(HeadBobHorizontal)
            local sig = signal()
            local peak = 0
            advance(sig, 4, 120, function() peak = math.max(peak, math.abs(h:Evaluate(sig, dt))) end)
            assert(peak > 0.015 and peak < 0.025, "sway ~2 cm: " .. peak)
            -- Strafing leans the head that way.
            sig.strafe = 1
            local lean
            advance(sig, 0, 60, function() lean = h:Evaluate(sig, dt) end)
            assert(math.abs(lean - 0.03) < 0.005, "strafe lean: " .. lean)
            -- Rotation: everything at once stays inside the clamps.
            local r = bob(HeadBobRotation)
            local wild = signal()
            wild.strafe = 1
            wild.turnRate = 100000
            wild.verticalBobVelocity = 50
            local maxRoll, maxPitch = 0, 0
            for i = 1, 120 do
                wild.landing = (i % 20 == 0) and 1 or 0
                wild.phase = (wild.phase + 7 / 4.4 * 2 * math.pi * dt) % (2 * math.pi)
                wild.speed = 7
                local p, ro = r:Evaluate(wild, dt)
                maxRoll = math.max(maxRoll, math.abs(ro))
                maxPitch = math.max(maxPitch, math.abs(p))
            end
            assert(maxRoll <= 6 and maxRoll > 1, "roll clamped: " .. maxRoll)
            assert(maxPitch <= 5 and maxPitch > 1, "pitch clamped: " .. maxPitch)
            -- Calm again: offsets return to 0.
            local calm = signal()
            local p, ro
            for i = 1, 120 do p, ro = r:Evaluate(calm, dt) end
            assert(math.abs(p) < 0.05 and math.abs(ro) < 0.05, "calm: " .. p .. " " .. ro)
            "#
        ));
    }

    /// Plays the demo with injected input: the rig stands, walks, runs with a wider field of
    /// view, bobs while moving and settles when stopped, looks around with the mouse, jumps,
    /// crouches, and holds the cursor until Escape.
    #[test]
    fn first_person_demo_plays() {
        let root = project();
        let (mut scene, _) = Scene::load(&root.join(SCENE)).unwrap();
        let settings = with_fps_axes(ProjectSettings::load(&root).unwrap());
        let (audio, _) = Audio::null(root.clone());
        let mut rt = Runtime::start_with_audio(&mut scene, root, &settings, audio);
        let mut time = Time::new(&settings.time);
        let player = scene.find_by_name("Player").unwrap();
        let vertical = scene.find_by_name("HeadBobVertical").unwrap();
        let horizontal = scene.find_by_name("HeadBobHorizontal").unwrap();
        let camera = scene.find_by_name("Camera").unwrap();
        let mut frame = |rt: &mut Runtime, scene: &mut Scene, input: &Input| {
            time.advance(1.0 / 60.0);
            rt.update(scene, &time, input.clone());
            let errors: Vec<_> = rt
                .take_logs()
                .into_iter()
                .filter(|(k, _)| *k == LogKind::Error)
                .collect();
            assert!(errors.is_empty(), "{errors:?}");
        };
        let held = |keys: &[&str]| {
            let mut i = Input::default();
            for k in keys {
                i.held.insert((*k).into());
            }
            i
        };
        let idle = Input::default();
        for _ in 0..30 {
            frame(&mut rt, &mut scene, &idle);
        }
        assert_eq!(rt.cursor().lock, CursorLockMode::Locked, "locked on start");
        assert!(!rt.cursor().visible);
        let start = scene.world_position(player);
        assert!(start.y.abs() < 0.1, "standing on the floor: {start}");

        // Walk forward: ~4 m/s after easing in, the head bobs.
        let w = held(&["w"]);
        let (mut lo, mut hi) = (f32::MAX, f32::MIN);
        let (mut xlo, mut xhi) = (f32::MAX, f32::MIN);
        let mut roll_max: f32 = 0.0;
        for _ in 0..90 {
            frame(&mut rt, &mut scene, &w);
            let y = scene.transform(vertical).position.y;
            lo = lo.min(y);
            hi = hi.max(y);
            let x = scene.transform(horizontal).position.x;
            xlo = xlo.min(x);
            xhi = xhi.max(x);
            roll_max = roll_max.max(scene.transform(camera).euler().z.abs());
        }
        let walked = scene.world_position(player).z - start.z;
        assert!(walked > 4.2 && walked < 6.5, "walked {walked} m in 1.5 s");
        // (The tuned walk amplitude is 1.5 cm.)
        assert!(
            hi - EYE_HEIGHT > 0.01 && EYE_HEIGHT - lo > 0.01,
            "vertical bob {lo}..{hi}"
        );
        // (The tuned walk sway is 1 cm.)
        assert!(xhi > 0.006 && xlo < -0.006, "horizontal sway {xlo}..{xhi}");
        let fov = scene.world.get::<&Camera>(camera).unwrap().fov;
        assert!((fov - 60.0).abs() < 1.0, "walking keeps the FOV: {fov}");
        assert!(roll_max > 0.3 && roll_max < 6.0, "stride roll {roll_max}");

        // Run: faster and a wider field of view.
        let run = held(&["w", "left shift"]);
        let z0 = scene.world_position(player).z;
        for _ in 0..60 {
            frame(&mut rt, &mut scene, &run);
        }
        let ran = scene.world_position(player).z - z0;
        assert!(ran > 5.5 && ran < 7.5, "ran {ran} m in 1 s");
        let fov = scene.world.get::<&Camera>(camera).unwrap().fov;
        assert!(fov > 64.0, "sprint FOV kick: {fov}");

        // Stop: the bob settles back to the base pose, the FOV returns.
        for _ in 0..60 {
            frame(&mut rt, &mut scene, &idle);
        }
        let y = scene.transform(vertical).position.y;
        assert!((y - EYE_HEIGHT).abs() < 0.008, "vertical settled: {y}");
        let x = scene.transform(horizontal).position.x;
        assert!(x.abs() < 0.003, "horizontal settled: {x}");
        let fov = scene.world.get::<&Camera>(camera).unwrap().fov;
        assert!((fov - 60.0).abs() < 0.5, "FOV back: {fov}");
        let e = scene.transform(camera).euler();
        assert!(e.z.abs() < 0.2 && e.x.abs() < 0.2, "rotation settled: {e}");

        // Mouse look: 100 px right turns the body ~20° (Mouse X 0.1 × sensitivity 2), 50 px
        // up pitches the camera.
        let look = Input {
            mouse_delta: Vec2::new(100.0, -50.0),
            ..Default::default()
        };
        frame(&mut rt, &mut scene, &look);
        for _ in 0..30 {
            frame(&mut rt, &mut scene, &idle);
        }
        let yaw = scene.transform(player).euler().y;
        assert!((yaw - 20.0).abs() < 1.0, "yaw {yaw}");
        let pitch = scene.transform(camera).euler().x;
        assert!((pitch + 10.0).abs() < 1.0, "pitch {pitch} (up is negative)");
        let look_back = Input {
            mouse_delta: Vec2::new(-100.0, 50.0),
            ..Default::default()
        };
        frame(&mut rt, &mut scene, &look_back);

        // Jump: up and back down.
        let mut jump = held(&["space"]);
        jump.pressed.insert("space".into());
        let ground = scene.world_position(player).y;
        frame(&mut rt, &mut scene, &jump);
        let mut apex = ground;
        for _ in 0..90 {
            frame(&mut rt, &mut scene, &idle);
            apex = apex.max(scene.world_position(player).y);
        }
        assert!(apex > ground + 0.9, "jumped {}", apex - ground);
        assert!(
            (scene.world_position(player).y - ground).abs() < 0.1,
            "landed"
        );

        // Crouch: the capsule shrinks and the eyes lower; standing up restores both.
        let crouch = held(&["left ctrl"]);
        for _ in 0..45 {
            frame(&mut rt, &mut scene, &crouch);
        }
        let cc = scene.world.get::<&CharacterController>(player).unwrap();
        assert!(
            (cc.height - 1.2).abs() < 0.05,
            "crouched capsule {}",
            cc.height
        );
        drop(cc);
        let y = scene.transform(vertical).position.y;
        assert!(y < EYE_HEIGHT - 0.5, "eyes lowered: {y}");
        for _ in 0..45 {
            frame(&mut rt, &mut scene, &idle);
        }
        let cc = scene.world.get::<&CharacterController>(player).unwrap();
        assert!((cc.height - 1.8).abs() < 0.05, "stood up {}", cc.height);
        drop(cc);

        // Escape releases the cursor; a click takes it back.
        let mut esc = held(&["escape"]);
        esc.pressed.insert("escape".into());
        frame(&mut rt, &mut scene, &esc);
        assert_eq!(rt.cursor().lock, CursorLockMode::None);
        assert!(rt.cursor().visible);
        let click = Input {
            mouse_pressed: [true, false, false],
            ..Default::default()
        };
        frame(&mut rt, &mut scene, &click);
        assert_eq!(rt.cursor().lock, CursorLockMode::Locked);
    }
}
