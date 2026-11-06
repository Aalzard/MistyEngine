use std::path::PathBuf;

use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use misty_engine::testing::{RunContext, Status, TestMode, TestRun};

use super::*;
use crate::actions::{Action, AssetKind};
use crate::layout::Tab;
use crate::project::ProjectBrowser;

const MATH: &str = r#"describe("Math", function()
    it("adds", function() assert.equal(1 + 1, 2) end)
    it("breaks", function()
        assert.equal(2 * 2, 5)
    end)
end)
it("standalone", function() end)
"#;
const MATH_REL: &str = "Assets/Tests/Math.test.lua";

fn project(name: &str) -> (EditorState, PathBuf) {
    let root =
        std::env::temp_dir().join(format!("misty_test_runner_{name}_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(root.join("Assets/Tests/Play")).unwrap();
    std::fs::write(root.join(MATH_REL), MATH).unwrap();
    std::fs::write(
        root.join("Assets/Tests/Play/Frames.playtest.lua"),
        "it(\"waits\", function() yieldFrames(3) end)\n",
    )
    .unwrap();
    (EditorState::new(ProjectBrowser::open(root.clone())), root)
}

fn harness(s: EditorState) -> Harness<'static, EditorState> {
    Harness::builder()
        .with_size([900.0, 600.0])
        .build_ui_state(|ui, s: &mut EditorState| show(ui, s), s)
}

/// Step the run (as the editor does every frame) until it is over.
fn finish(h: &mut Harness<'static, EditorState>) {
    for _ in 0..10_000 {
        if !tick(h.state_mut()) {
            break;
        }
        h.run();
    }
    assert!(!h.state().test_runner.running());
    h.run();
}

#[test]
fn window_menu_opens_the_test_runner() {
    let (s, root) = project("menu");
    let mut h = Harness::builder()
        .with_size([900.0, 600.0])
        .build_ui_state(|ui, s: &mut EditorState| crate::menu_bar::show(ui, s), s);
    h.run();
    h.get_by_label("Window").click();
    h.run();
    h.get_by_label("General ⏵").hover();
    h.run();
    h.get_by_label("Test Runner").click();
    h.run();
    let actions = std::mem::take(&mut h.state_mut().actions);
    assert!(
        matches!(actions.as_slice(), [Action::FocusTab(Tab::TestRunner)]),
        "{actions:?}"
    );
    let mut dock = crate::layout::default_layout();
    crate::layout::focus_or_open(&mut dock, Tab::TestRunner);
    let at = dock.find_tab(&Tab::TestRunner).expect("opened");
    assert_eq!(at.node, dock.find_tab(&Tab::Scene).unwrap().node);
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn run_all_filter_and_failure_link_open_the_code_editor() {
    let (s, root) = project("window");
    let mut h = harness(s);
    h.run();
    // The tree lists the file, its describe block and tests before anything ran.
    h.get_by_label("Math.test.lua");
    h.get_by_label("Math");
    h.get_by_label("breaks");
    h.get_by_label_contains("3 not run");

    h.get_by_label("Run All").click();
    h.run();
    assert!(h.state().test_runner.running());
    finish(&mut h);
    h.get_by_label_contains("2 passed");
    h.get_by_label_contains("1 failed");
    let w = &h.state().test_runner;
    assert_eq!(
        w.status(&format!("{MATH_REL}::Math > adds")),
        Status::Passed
    );
    assert_eq!(
        w.status(&format!("{MATH_REL}::Math > breaks")),
        Status::Failed
    );
    assert!(
        h.state()
            .console
            .entries
            .iter()
            .any(|e| e.message.starts_with("Test Runner: 2 passed, 1 failed"))
    );

    // The search filters the tree.
    h.get_by_role(egui::accesskit::Role::TextInput).click();
    h.run();
    h.get_by_role(egui::accesskit::Role::TextInput)
        .type_text("ADDS");
    h.run();
    assert_eq!(h.state().test_runner.filter, "ADDS");
    h.get_by_label("adds");
    assert!(h.query_by_label("breaks").is_none());
    assert!(h.query_by_label("standalone").is_none());
    h.state_mut().test_runner.filter.clear();
    h.run();

    // Selecting the failure shows its message; its file:line link opens the Code Editor there.
    h.get_by_label("breaks").click();
    h.run();
    h.get_by_label_contains("expected 5, got 4");
    h.get_all_by_label("Math.test.lua:4")
        .next()
        .expect("failure location link")
        .click();
    h.run();
    let s = h.state();
    assert_eq!(s.code_editor.docs.len(), 1);
    assert_eq!(s.code_editor.docs[0].path, MATH_REL);
    assert_eq!(s.code_editor.docs[0].editor.caret().line, 3);
    assert!(
        s.actions
            .iter()
            .any(|a| matches!(a, Action::FocusTab(Tab::CodeEditor)))
    );

    // Run Failed reruns only the failing test.
    h.state_mut().actions.clear();
    h.get_by_label("Run Failed").click();
    h.run();
    let w = &h.state().test_runner;
    assert_eq!(
        w.status(&format!("{MATH_REL}::Math > breaks")),
        Status::Running
    );
    assert_eq!(
        w.status(&format!("{MATH_REL}::Math > adds")),
        Status::Passed
    );
    finish(&mut h);

    // PlayMode tab: its own tree and summary.
    h.get_by_label("PlayMode").click();
    h.run();
    h.get_by_label("Play/Frames.playtest.lua");
    h.get_by_label("Run All").click();
    h.run();
    finish(&mut h);
    h.get_by_label_contains("1 passed");
    let r = &h.state().test_runner.results["Assets/Tests/Play/Frames.playtest.lua::waits"];
    assert_eq!(r.frames, 3);
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn tree_rows_fold_and_aggregate() {
    let (mut s, root) = project("rows");
    scan(&mut s);
    let w = &mut s.test_runner;
    let labels = |w: &TestRunnerWindow| -> Vec<(usize, String)> {
        rows(w).into_iter().map(|r| (r.depth, r.label)).collect()
    };
    assert_eq!(
        labels(w),
        [
            (0, "Math.test.lua".to_owned()),
            (1, "Math".to_owned()),
            (2, "adds".to_owned()),
            (2, "breaks".to_owned()),
            (1, "standalone".to_owned()),
        ]
    );
    w.collapsed.insert(format!("{MATH_REL}::Math"));
    assert_eq!(labels(w).len(), 3);
    w.collapsed.insert(MATH_REL.to_owned());
    assert_eq!(labels(w).len(), 1);
    assert!(under(&w.files[0].tests[0], MATH_REL));
    assert!(under(&w.files[0].tests[0], &format!("{MATH_REL}::Math")));
    assert!(!under(&w.files[0].tests[2], &format!("{MATH_REL}::Math")));

    use Status::*;
    assert_eq!(aggregate(&[Passed, Failed]), Failed);
    assert_eq!(aggregate(&[Passed, Skipped]), Passed);
    assert_eq!(aggregate(&[Passed, NotRun]), NotRun);
    assert_eq!(aggregate(&[Skipped, Skipped]), Skipped);
    assert_eq!(aggregate(&[Running, Passed]), Running);
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn create_menu_makes_test_files_in_the_tests_folder() {
    let (mut s, root) = project("create");
    s.project.current = root.join("Assets");
    crate::commands::apply(
        &mut s,
        Action::CreateAsset(AssetKind::Test(TestMode::EditMode)),
    );
    crate::commands::apply(
        &mut s,
        Action::CreateAsset(AssetKind::Test(TestMode::PlayMode)),
    );
    let edit = root.join("Assets/Tests/NewEditModeTest.test.lua");
    let play = root.join("Assets/Tests/NewPlayModeTest.playtest.lua");
    assert!(edit.is_file() && play.is_file());
    assert!(
        std::fs::read_to_string(&play)
            .unwrap()
            .contains("describe(\"NewPlayModeTest\"")
    );
    assert_eq!(
        s.code_editor.docs.len(),
        2,
        "new tests open in the Code Editor"
    );
    scan(&mut s);
    assert_eq!(s.test_runner.files.len(), 4);
    let _ = std::fs::remove_dir_all(root);
}

/// The sample project's own tests (Assets/Tests) pass: pure math, the platformer's player, the
/// vehicle demo's car and the first-person player.
#[test]
fn sample_project_tests_pass() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../SampleProject");
    misty_engine::assets::set_project_root(&root);
    let files = misty_engine::testing::discover(&root);
    let names: Vec<&str> = files.iter().map(|f| f.rel.as_str()).collect();
    assert_eq!(
        names,
        [
            "Assets/Tests/FirstPersonTests.playtest.lua",
            "Assets/Tests/MathTests.test.lua",
            "Assets/Tests/PlayerControllerTests.playtest.lua",
            "Assets/Tests/VehicleTests.playtest.lua",
        ]
    );
    let mut run = TestRun::new(RunContext::load(&root).unwrap(), &files, |_| true);
    run.run_to_end();
    let failed: Vec<String> = run
        .results()
        .iter()
        .filter(|(_, r)| r.status != Status::Passed)
        .map(|(c, r)| {
            format!(
                "{}: {:?} {:?}\n{}",
                c.id,
                r.status,
                r.failure,
                r.logs.join("\n")
            )
        })
        .collect();
    assert!(failed.is_empty(), "{}", failed.join("\n\n"));
    assert!(run.results().len() >= 8, "{}", run.results().len());
}
