use std::path::PathBuf;

use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use misty_engine::assets::Material;
use misty_engine::render::RenderingMode;
use misty_engine::scene::{LodGroup, LodLevel, MeshFilter, MeshRef, MeshRenderer};

use super::*;
use crate::actions::Action;
use crate::project::ProjectBrowser;

/// An OBJ grid of `n` × `n` quads (2 n² triangles).
fn grid(n: usize) -> String {
    let mut s = String::new();
    for z in 0..=n {
        for x in 0..=n {
            let h = ((x * 7 + z * 13) % 5) as f32 * 0.02;
            s += &format!("v {} {h} {}\n", x as f32 * 0.1, z as f32 * 0.1);
        }
    }
    let at = |x: usize, z: usize| z * (n + 1) + x + 1;
    for z in 0..n {
        for x in 0..n {
            let (a, b, c, d) = (at(x, z), at(x + 1, z), at(x + 1, z + 1), at(x, z + 1));
            s += &format!("f {a} {b} {c}\nf {a} {c} {d}\n");
        }
    }
    s
}

/// A project with dense (5 000+ triangle) and small models. Mesh references use absolute paths:
/// independent of the process-wide project root other tests set.
struct Project {
    s: EditorState,
    root: PathBuf,
}

impl Project {
    fn new(name: &str) -> Self {
        let root = std::env::temp_dir().join(format!("misty_vgeo_{name}_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("Assets")).unwrap();
        for (file, n) in [
            ("Dense.obj", 60),
            ("Foliage.obj", 60),
            ("Small.obj", 4),
            ("Lod0.obj", 60),
            ("Lod1.obj", 60),
        ] {
            std::fs::write(root.join("Assets").join(file), grid(n)).unwrap();
        }
        let mut s = EditorState::new(ProjectBrowser::open(root.clone()));
        let leaf = Material {
            rendering_mode: RenderingMode::Cutout,
            ..Material::default()
        };
        s.assets.update_material("Assets/Leaf.mat", leaf).unwrap();
        Self { s, root }
    }

    /// Absolute path of a model (its key in mesh references).
    fn model(&self, file: &str) -> String {
        self.root
            .join("Assets")
            .join(file)
            .to_string_lossy()
            .replace('\\', "/")
    }

    /// An object drawing `file`'s mesh with `material`.
    fn object(&mut self, file: &str, material: Option<&str>) -> Entity {
        let e = self.s.scene.spawn(file, None);
        let mesh = MeshRef::asset(&self.model(file), 0);
        self.s.scene.add(e, MeshFilter { mesh: Some(mesh) });
        self.s.scene.add(
            e,
            MeshRenderer {
                material: material.map(str::to_owned),
                ..Default::default()
            },
        );
        e
    }

    /// Dense, foliage (alpha-tested), small and a LOD Group of two dense levels.
    fn scene(&mut self) -> Entity {
        let dense = self.object("Dense.obj", None);
        self.object("Foliage.obj", Some("Assets/Leaf.mat"));
        self.object("Small.obj", None);
        let group = self.s.scene.spawn("Rock", None);
        let lod0 = self.object("Lod0.obj", None);
        let lod1 = self.object("Lod1.obj", None);
        self.s.scene.set_parent(lod0, Some(group), None, false);
        self.s.scene.set_parent(lod1, Some(group), None, false);
        let lods = vec![
            LodLevel {
                screen_height: 0.5,
                renderers: vec![lod0],
            },
            LodLevel {
                screen_height: 0.1,
                renderers: vec![lod1],
            },
        ];
        self.s.scene.add(
            group,
            LodGroup {
                lods,
                ..Default::default()
            },
        );
        dense
    }

    fn virtual_on(&self, file: &str) -> bool {
        ModelImporter::load(std::path::Path::new(&self.model(file))).virtual_geometry
    }
}

impl Drop for Project {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

#[test]
fn materials_and_meshes_decide_eligibility() {
    let m = |f: fn(&mut Material)| {
        let mut m = Material::default();
        f(&mut m);
        material_skip(&m)
    };
    assert_eq!(m(|_| {}), None, "Standard, Opaque, single-sided");
    assert_eq!(
        m(|m| m.rendering_mode = RenderingMode::Cutout),
        Some(Skip::AlphaTested)
    );
    assert_eq!(
        m(|m| m.rendering_mode = RenderingMode::Fade),
        Some(Skip::Transparent)
    );
    assert_eq!(
        m(|m| m.rendering_mode = RenderingMode::Transparent),
        Some(Skip::Transparent)
    );
    assert_eq!(m(|m| m.double_sided = true), Some(Skip::DoubleSided));
    assert_eq!(m(|m| m.shader = "Unlit/Color".into()), Some(Skip::Unlit));
    assert_eq!(
        m(|m| m.shader = "Assets/Water.shader".into()),
        Some(Skip::CustomShader)
    );
    let dense = MeshFacts {
        triangles: 80_000,
        vertices: 40_000,
        ..Default::default()
    };
    assert_eq!(dense.skip(DEFAULT_MIN_TRIANGLES), None);
    let skinned = MeshFacts {
        skinned: true,
        ..dense
    };
    assert_eq!(skinned.skip(DEFAULT_MIN_TRIANGLES), Some(Skip::Skinned));
    let morph = MeshFacts {
        blend_shapes: true,
        ..dense
    };
    assert_eq!(morph.skip(DEFAULT_MIN_TRIANGLES), Some(Skip::BlendShapes));
    let tiny = MeshFacts {
        triangles: 12,
        vertices: 8,
        ..Default::default()
    };
    assert_eq!(tiny.skip(DEFAULT_MIN_TRIANGLES), Some(Skip::Small(12)));
    assert_eq!(tiny.skip(0), None, "no threshold");
    // Every reason reads as a sentence fragment.
    assert!(Skip::Small(12).reason(5000).contains("5000"));
    assert!(Skip::AlphaTested.reason(5000).contains("foliage"));
}

#[test]
fn scene_conversion_skips_with_reasons_and_is_one_undo_step() {
    let mut p = Project::new("scene");
    p.scene();
    let roots = p.s.scene.roots().to_vec();
    let plan = plan_objects(&p.s, &roots, DEFAULT_MIN_TRIANGLES);
    let names =
        |v: Vec<String>| -> Vec<String> { v.iter().map(|r| file_name(r).to_owned()).collect() };
    assert_eq!(
        names(plan.models.iter().map(|m| m.rel.clone()).collect()),
        ["Dense.obj", "Lod0.obj"]
    );
    assert_eq!(plan.objects, 2);
    assert!(
        plan.models
            .iter()
            .all(|m| m.triangles == 7200 && !m.already)
    );
    let skipped: Vec<(String, Skip)> = (plan.skipped.iter())
        .map(|(r, why, _)| (file_name(r).to_owned(), why.clone()))
        .collect();
    assert_eq!(
        skipped,
        [
            ("Foliage.obj".to_owned(), Skip::AlphaTested),
            ("Lod1.obj".to_owned(), Skip::LodLevel),
            ("Small.obj".to_owned(), Skip::Small(32)),
        ]
    );
    let summary = plan.summary(Some(Scope::Scene));
    assert!(
        summary.starts_with("2 models, 2 objects, est. "),
        "{summary}"
    );

    let quality = quality_levels_to_enable(&p.s.project_settings.quality);
    let names_q: Vec<&str> = (quality.iter())
        .map(|&i| p.s.project_settings.quality.levels[i].name.as_str())
        .collect();
    assert_eq!(names_q, ["High", "Ultra"]);
    let objects = p.s.scene.len();
    let n = apply(
        &mut p.s,
        Scope::Scene.title(),
        &plan.to_change(),
        true,
        &quality,
    );
    assert_eq!(n, 2);
    assert!(p.virtual_on("Dense.obj") && p.virtual_on("Lod0.obj"));
    assert!(!p.virtual_on("Foliage.obj") && !p.virtual_on("Small.obj"));
    assert!(!p.virtual_on("Lod1.obj"));
    let q = &p.s.project_settings.quality;
    assert!(q.levels[2].virtual_geometry && q.levels[3].virtual_geometry);
    assert!(!q.levels[0].virtual_geometry && !q.levels[1].virtual_geometry);
    assert!(p.s.vgeo.progress.is_some(), "the builds are tracked");
    assert_eq!(p.s.scene.len(), objects, "the scene doesn't change");
    // The re-imported model is virtual.
    assert!(models::is_virtual(&MeshRef::asset(
        &p.model("Dense.obj"),
        0
    )));
    // A second conversion finds them on already: nothing to change.
    let again = plan_objects(&p.s, &roots, DEFAULT_MIN_TRIANGLES);
    assert!(again.to_change().is_empty());
    assert_eq!(again.models.iter().filter(|m| m.already).count(), 2);

    // One undo step puts the import settings and the Quality levels back; redo again.
    assert_eq!(p.s.undo.steps(), [Scope::Scene.title()]);
    assert_eq!(p.s.undo_step().as_deref(), Some(Scope::Scene.title()));
    assert!(!p.virtual_on("Dense.obj") && !p.virtual_on("Lod0.obj"));
    assert!(!p.s.project_settings.quality.levels[2].virtual_geometry);
    let saved = misty_engine::settings::ProjectSettings::load(&p.root).unwrap();
    assert!(!saved.quality.levels[3].virtual_geometry, "saved too");
    assert!(!models::is_virtual(&MeshRef::asset(
        &p.model("Dense.obj"),
        0
    )));
    p.s.redo_step();
    assert!(p.virtual_on("Dense.obj") && p.virtual_on("Lod0.obj"));
    assert!(p.s.project_settings.quality.levels[3].virtual_geometry);
}

#[test]
fn project_conversion_covers_every_model() {
    let mut p = Project::new("project");
    // The project's models by absolute path (see `Project`).
    p.s.assets.models.clear();
    for f in [
        "Dense.obj",
        "Foliage.obj",
        "Small.obj",
        "Lod0.obj",
        "Lod1.obj",
    ] {
        let rel = p.model(f);
        p.s.assets.models.insert(rel, Ok(vec![f.into()]));
    }
    let plan = plan_project(&p.s, DEFAULT_MIN_TRIANGLES);
    let models: Vec<&str> = plan.models.iter().map(|m| file_name(&m.rel)).collect();
    // By the models' own materials (OBJ without one: Default-Material).
    assert_eq!(models, ["Dense.obj", "Foliage.obj", "Lod0.obj", "Lod1.obj"]);
    assert_eq!(plan.skipped.len(), 1);
    assert_eq!(plan.skipped[0].1, Skip::Small(32));
    assert!(!plan.summary(Some(Scope::Project)).contains("object"));
    // A higher threshold leaves them all out.
    let none = plan_project(&p.s, 100_000);
    assert!(none.models.is_empty() && none.skipped.len() == 5);
    let n = apply(
        &mut p.s,
        Scope::Project.title(),
        &plan.to_change(),
        true,
        &[],
    );
    assert_eq!(n, 4);
    assert!(p.virtual_on("Lod1.obj") && !p.virtual_on("Small.obj"));
    // No Quality levels asked for: unchanged.
    assert!(!p.s.project_settings.quality.levels[2].virtual_geometry);
    p.s.undo_step();
    assert!(!p.virtual_on("Lod1.obj") && !p.virtual_on("Dense.obj"));
}

#[test]
fn enable_and_disable_on_the_selection() {
    let mut p = Project::new("selection");
    let dense = p.scene();
    let small = p.s.scene.find_by_name("Small.obj").unwrap();
    let group = p.s.scene.find_by_name("Rock").unwrap();
    p.s.selection.set_many(vec![dense, small, group]);
    assert_eq!(selection_state(&p.s), (true, false));
    run(&mut p.s, Command::EnableSelected);
    // The LOD Group's LOD 0 (a child of the selected root) and the dense mesh; the small one is
    // under the threshold, LOD 1 is left to LOD 0.
    assert!(p.virtual_on("Dense.obj") && p.virtual_on("Lod0.obj"));
    assert!(!p.virtual_on("Small.obj") && !p.virtual_on("Lod1.obj"));
    assert!(p.s.project_settings.quality.levels[2].virtual_geometry);
    let step = p.s.undo.undo_label().unwrap().to_owned();
    assert!(step.starts_with("Enable Virtual Geometry on"), "{step}");
    assert_eq!(
        selection_state(&p.s),
        (true, true),
        "Small and Lod1 stay off"
    );
    // Disable turns off only the models that have it on.
    p.s.selection.set(dense);
    run(&mut p.s, Command::DisableSelected);
    assert!(!p.virtual_on("Dense.obj") && p.virtual_on("Lod0.obj"));
    assert_eq!(selection_state(&p.s), (true, false));
    p.s.undo_step();
    assert!(p.virtual_on("Dense.obj"));
    // Nothing virtual in the selection: Disable says so and changes nothing.
    p.s.selection.set(small);
    let steps = p.s.undo.steps().len();
    run(&mut p.s, Command::DisableSelected);
    assert_eq!(p.s.undo.steps().len(), steps);
}

#[test]
fn progress_imports_and_counts_the_builds() {
    let p = Project::new("progress");
    let rel = p.model("Dense.obj");
    let failed = set_models(&p.s, std::slice::from_ref(&rel), true);
    assert!(failed.is_empty());
    let mut progress = Progress::new(vec![rel]);
    let (f, text, done) = progress.step();
    assert!(text.starts_with("Importing Dense.obj") && !done && f > 0.0);
    let started = std::time::Instant::now();
    loop {
        let (f, text, done) = progress.step();
        if done {
            assert!((f - 1.0).abs() < 1e-6, "{f} {text}");
            break;
        }
        assert!(text.starts_with("Building clusters"), "{text}");
        assert!(started.elapsed().as_secs() < 60, "the build never finished");
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
}

#[test]
fn tools_menu_opens_the_dialog_which_converts() {
    let mut p = Project::new("dialog");
    p.scene();
    let s = std::mem::replace(
        &mut p.s,
        EditorState::new(ProjectBrowser::open(p.root.clone())),
    );
    // Tools ▸ Rendering ▸ Convert Scene to Virtual Geometry…
    let mut h = Harness::builder()
        .with_size([900.0, 600.0])
        .build_ui_state(|ui, s: &mut EditorState| crate::menu_bar::show(ui, s), s);
    h.run();
    h.get_by_label("Tools").click();
    h.run();
    h.get_by_label("Rendering ⏵").hover();
    h.run();
    assert!(
        h.query_by_label("Convert Project to Virtual Geometry...")
            .is_some()
    );
    h.get_by_label("Convert Scene to Virtual Geometry...")
        .click();
    h.run();
    let actions = std::mem::take(&mut h.state_mut().actions);
    assert!(
        matches!(
            actions.as_slice(),
            [Action::VirtualGeometry(Command::ConvertDialog(
                Scope::Scene
            ))]
        ),
        "{actions:?}"
    );
    let mut s = h.into_state();
    run(&mut s, Command::ConvertDialog(Scope::Scene));
    assert!(s.vgeo.dialog.is_some());

    // The dialog: threshold, summary, Quality, what is skipped; Convert applies it.
    let mut h = Harness::builder()
        .with_size([480.0, 420.0])
        .build_ui_state(|ui, s: &mut EditorState| dialog_contents(ui, s), s);
    h.run();
    h.get_by_label("Min. triangles");
    assert!(
        h.query_by_label_contains("2 models, 2 objects, est.")
            .is_some()
    );
    h.get_by_label_contains("Turn on Quality ▸ Virtual Geometry for High and Ultra");
    h.get_by_label("Skipped (3)").click();
    h.run();
    assert!(h.query_by_label("Foliage.obj (1 object)").is_some());
    h.get_by_label_contains("alpha-tested material");
    h.get_by_label("Convert").click();
    h.run();
    let s = h.into_state();
    assert!(s.vgeo.dialog.is_none(), "closed");
    assert!(p.virtual_on("Dense.obj") && p.virtual_on("Lod0.obj"));
    assert!(s.project_settings.quality.levels[3].virtual_geometry);
    assert_eq!(s.undo.steps(), [Scope::Scene.title()]);
}

#[test]
fn search_lists_the_commands() {
    let p = Project::new("search");
    let cmds = crate::search::menu_items::all(&p.s);
    for title in [
        "Convert Scene to Virtual Geometry",
        "Convert Project to Virtual Geometry",
        "Enable Virtual Geometry",
        "Disable Virtual Geometry",
    ] {
        assert!(cmds.iter().any(|c| c.title == title), "{title}");
    }
}
