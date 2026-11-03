//! The game without a window: project settings, the build scene list, the loaded scene and the
//! runtime that plays it. `main` feeds it input and draws what its camera sees.

use std::path::{Path, PathBuf};

use misty_engine::assets::AssetCache;
use misty_engine::build::{BuildSettings, SceneRef};
use misty_engine::render_scene::GameView;
use misty_engine::runtime::{Runtime, Time};
use misty_engine::scene::Scene;
use misty_engine::scripting::{Input, LogKind, Request};
use misty_engine::settings::ProjectSettings;

pub struct Game {
    root: PathBuf,
    pub settings: ProjectSettings,
    pub build: BuildSettings,
    pub assets: AssetCache,
    pub scene: Scene,
    /// Build index of the loaded scene.
    pub scene_index: usize,
    runtime: Option<Runtime>,
    /// Keeps running across scene loads.
    pub time: Time,
    /// `Application.Quit()` was called.
    pub quit: bool,
    logs: Vec<(LogKind, String)>,
}

impl Game {
    /// Open the project at `root` and start build scene 0.
    pub fn open(root: &Path) -> Result<Self, String> {
        let mut logs = Vec::new();
        let settings = ProjectSettings::load(root).unwrap_or_else(|e| {
            logs.push((LogKind::Warning, format!("ProjectSettings.json: {e}")));
            ProjectSettings::default()
        });
        let build = BuildSettings::load(root).map_err(|e| format!("BuildSettings.json: {e}"))?;
        if build.enabled().next().is_none() {
            return Err("Build Settings has no scenes: add one in File ▸ Build Settings…".into());
        }
        let mut game = Self {
            root: root.to_path_buf(),
            time: Time::new(&settings.time),
            assets: AssetCache::open(root),
            settings,
            build,
            scene: Scene::default(),
            scene_index: 0,
            runtime: None,
            quit: false,
            logs,
        };
        game.load_scene(&SceneRef::Index(0))?;
        Ok(game)
    }

    pub fn product_name(&self) -> String {
        self.build.product_name_or(&self.root)
    }

    /// Replace the current scene with a build scene and start its scripts (`Awake`).
    pub fn load_scene(&mut self, which: &SceneRef) -> Result<(), String> {
        let (rel, index) = self.build.resolve(which)?;
        let (mut scene, report) = Scene::load(&self.root.join(&rel))?;
        for w in report.warnings {
            self.logs.push((LogKind::Warning, format!("{rel}: {w}")));
        }
        // Old scripts and bodies go before the new scene's Awake runs.
        self.runtime = None;
        let runtime = Runtime::start(&mut scene, self.root.clone(), &self.settings);
        self.scene = scene;
        self.scene_index = index;
        self.runtime = Some(runtime);
        self.collect_logs();
        Ok(())
    }

    /// One game frame of `real_dt` seconds; then scene loads / quit asked for by scripts.
    pub fn frame(&mut self, real_dt: f32, input: Input) {
        self.time.advance(real_dt);
        let Some(runtime) = self.runtime.as_mut() else {
            return;
        };
        runtime.update(&mut self.scene, &self.time, input);
        let requests = runtime.take_requests();
        self.collect_logs();
        // Like the familiar API, the last LoadScene of a frame wins.
        let mut load = None;
        for r in requests {
            match r {
                Request::LoadScene(s) => load = Some(s),
                Request::Quit => self.quit = true,
            }
        }
        if let Some(scene) = load
            && let Err(e) = self.load_scene(&scene)
        {
            self.logs.push((LogKind::Error, e));
        }
    }

    /// What the game camera sees (`None` = no camera rendering).
    pub fn view(&self) -> Option<GameView> {
        GameView::new(&self.scene, &self.assets)
    }

    fn collect_logs(&mut self) {
        if let Some(r) = self.runtime.as_mut() {
            self.logs.extend(r.take_logs());
        }
    }

    pub fn take_logs(&mut self) -> Vec<(LogKind, String)> {
        std::mem::take(&mut self.logs)
    }
}

#[cfg(test)]
mod tests {
    use misty_engine::glam::Vec3;
    use misty_engine::scene::{
        Camera, MeshFilter, MeshRenderer, PrimitiveMesh, ScriptRef, Scripts,
    };

    use super::*;
    use misty_engine::build::BuildScene;

    /// A project with two scenes: "Menu" has a falling cube and a script that loads "Level2".
    fn project(name: &str) -> PathBuf {
        let root = std::env::temp_dir().join(format!("misty_player_{name}_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("Assets/Scripts")).unwrap();
        std::fs::write(
            root.join("Assets/Scripts/Loader.lua"),
            r#"
local Loader = { frames = 30 }
function Loader:Update()
    self.transform:Rotate(0, 90 * Time.deltaTime, 0)
    if Time.frameCount == self.frames then SceneManager.LoadScene("Level2") end
end
return Loader
"#,
        )
        .unwrap();
        std::fs::write(
            root.join("Assets/Scripts/Quitter.lua"),
            "local Q = {}\nfunction Q:Update() Application.Quit() end\nreturn Q\n",
        )
        .unwrap();

        let mut menu = Scene::new("Menu");
        let cam = menu.spawn("Main Camera", None);
        menu.add(cam, Camera::default());
        let cube = menu.spawn("Cube", None);
        menu.add(
            cube,
            MeshFilter {
                mesh: Some(PrimitiveMesh::Cube),
            },
        );
        menu.add(cube, MeshRenderer::default());
        menu.add_by_name(cube, "Box Collider");
        menu.add_by_name(cube, "Rigidbody");
        menu.set_world_position(cube, Vec3::new(0.0, 10.0, 0.0));
        let loader = menu.spawn("Loader", None);
        menu.add(loader, scripts("Loader.lua"));
        menu.save(&root.join("Assets/Scenes/Menu.scene")).unwrap();

        let mut level = Scene::new("Level2");
        let q = level.spawn("Quitter", None);
        level.add(q, scripts("Quitter.lua"));
        level
            .save(&root.join("Assets/Scenes/Level2.scene"))
            .unwrap();

        BuildSettings {
            scenes: vec![
                BuildScene::new("Assets/Scenes/Menu.scene"),
                BuildScene::new("Assets/Scenes/Level2.scene"),
            ],
            product_name: "Test Game".into(),
            ..Default::default()
        }
        .save(&root)
        .unwrap();
        root
    }

    fn scripts(file: &str) -> Scripts {
        Scripts {
            list: vec![ScriptRef {
                path: format!("Assets/Scripts/{file}"),
                enabled: true,
                fields: Default::default(),
            }],
        }
    }

    #[test]
    fn frames_run_physics_and_scripts_then_load_the_next_scene() {
        let root = project("frames");
        let mut game = Game::open(&root).unwrap();
        assert_eq!(game.product_name(), "Test Game");
        assert_eq!(game.scene.name, "Menu");
        assert_eq!(game.scene_index, 0);
        let view = game.view().expect("the scene has a camera");
        assert_eq!(view.meshes.len(), 1);

        for _ in 0..20 {
            game.frame(1.0 / 60.0, Input::default());
        }
        let cube = game.scene.find_by_name("Cube").unwrap();
        let y = game.scene.world_position(cube).y;
        assert!(y < 9.5, "the rigidbody fell: {y}");
        let loader = game.scene.find_by_name("Loader").unwrap();
        let (_, rot, _) = game
            .scene
            .world_matrix(loader)
            .to_scale_rotation_translation();
        assert!(rot.angle_between(Default::default()) > 0.1, "Update ran");
        assert!(!game.quit);

        for _ in 0..10 {
            game.frame(1.0 / 60.0, Input::default());
        }
        assert_eq!(game.scene.name, "Level2", "LoadScene(\"Level2\") ran");
        assert_eq!(game.scene_index, 1);
        assert_eq!(game.time.frame, 30, "time keeps running across loads");
        assert!(game.view().is_none(), "Level2 has no camera");

        game.frame(1.0 / 60.0, Input::default());
        assert!(game.quit, "Application.Quit() in Level2");
        let errors: Vec<_> = game
            .take_logs()
            .into_iter()
            .filter(|(k, _)| *k == LogKind::Error)
            .collect();
        assert!(errors.is_empty(), "{errors:?}");
    }

    #[test]
    fn unknown_scenes_are_errors_and_an_empty_build_list_fails_to_open() {
        let root = project("unknown");
        let mut game = Game::open(&root).unwrap();
        assert!(game.load_scene(&SceneRef::Name("Nope".into())).is_err());
        assert!(game.load_scene(&SceneRef::Index(5)).is_err());
        assert_eq!(game.scene.name, "Menu", "a failed load keeps the scene");

        BuildSettings::default().save(&root).unwrap();
        assert!(Game::open(&root).is_err());
    }
}
