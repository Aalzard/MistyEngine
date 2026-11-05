use std::path::{Path, PathBuf};

use super::*;
use crate::scene::{Scene, ScriptRef, Scripts};

/// A throwaway project with `files` (project-relative path, contents) and a saved Level scene
/// whose "Mover" walks right while D is held.
fn project(name: &str, files: &[(&str, &str)]) -> PathBuf {
    let root = std::env::temp_dir().join(format!("misty_testrunner_{name}_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(root.join("Assets/Tests")).unwrap();
    std::fs::create_dir_all(root.join("Assets/Scripts")).unwrap();
    std::fs::write(
        root.join("Assets/Scripts/Mover.lua"),
        "local Mover = { speed = 5 }
function Mover:Update()
    if Input.GetKey(\"d\") then
        local p = self.transform.position
        self.transform.position = Vector3.new(p.x + self.speed * Time.deltaTime, p.y, p.z)
    end
end
return Mover
",
    )
    .unwrap();
    let mut level = Scene::new("Level");
    let e = level.spawn("Mover", None);
    level.add(
        e,
        Scripts {
            list: vec![ScriptRef {
                path: "Assets/Scripts/Mover.lua".into(),
                enabled: true,
                fields: Default::default(),
            }],
        },
    );
    level.save(&root.join("Assets/Scenes/Level.scene")).unwrap();
    for (rel, text) in files {
        let path = root.join(rel);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, text).unwrap();
    }
    root
}

fn context(root: &Path) -> RunContext {
    let mut scene = Scene::new("Edit");
    scene.spawn("Main Camera", None);
    scene.spawn("Cube", None);
    RunContext {
        root: root.to_path_buf(),
        settings: Default::default(),
        build: Default::default(),
        scene,
    }
}

fn run_all(root: &Path) -> Vec<(TestCase, TestResult)> {
    let files = discover(root);
    let mut run = TestRun::new(context(root), &files, |_| true);
    run.run_to_end();
    run.results().to_vec()
}

fn result<'a>(results: &'a [(TestCase, TestResult)], name: &str) -> &'a TestResult {
    &results
        .iter()
        .find(|(c, _)| c.name == name)
        .unwrap_or_else(|| panic!("no test {name}: {results:?}"))
        .1
}

const HOOKS: &str = r#"
beforeAll(function() print("root beforeAll") end)
afterAll(function() print("root afterAll") end)
beforeEach(function() print("root beforeEach") end)
describe("Outer", function()
    beforeAll(function() print("outer beforeAll") end)
    afterAll(function() print("outer afterAll") end)
    beforeEach(function() print("outer beforeEach") end)
    afterEach(function() print("outer afterEach") end)
    it("first", function() print("first") end)
    describe("Inner", function()
        beforeEach(function() print("inner beforeEach") end)
        afterEach(function() print("inner afterEach") end)
        it("second", function() print("second") end)
    end)
end)
it("third", function() print("third") end)
"#;

#[test]
fn hooks_run_in_order_around_each_test() {
    let root = project("hooks", &[("Assets/Tests/Hooks.test.lua", HOOKS)]);
    let results = run_all(&root);
    let logs: Vec<Vec<String>> = results.iter().map(|(_, r)| r.logs.clone()).collect();
    assert_eq!(
        logs,
        [
            vec![
                "root beforeAll",
                "outer beforeAll",
                "root beforeEach",
                "outer beforeEach",
                "first",
                "outer afterEach",
            ],
            vec![
                "root beforeEach",
                "outer beforeEach",
                "inner beforeEach",
                "second",
                "inner afterEach",
                "outer afterEach",
                "outer afterAll",
            ],
            vec!["root beforeEach", "third", "root afterAll"],
        ]
    );
    assert!(results.iter().all(|(_, r)| r.status == Status::Passed));
    let ids: Vec<&str> = results.iter().map(|(c, _)| c.id.as_str()).collect();
    assert_eq!(
        ids,
        [
            "Assets/Tests/Hooks.test.lua::Outer > first",
            "Assets/Tests/Hooks.test.lua::Outer > Inner > second",
            "Assets/Tests/Hooks.test.lua::third",
        ]
    );
    let _ = std::fs::remove_dir_all(root);
}

const ASSERTS: &str = r#"
describe("Asserts", function()
    it("passes", function()
        assert.equal({ 1, { a = 2 } }, { 1, { a = 2 } })
        assert.near(0.1 + 0.2, 0.3)
        assert.near(Vector3.new(1, 2, 3), Vector3.new(1, 2, 3.0001), 0.001)
        assert.contains("hello world", "lo w")
        assert.contains({ 1, 2, 3 }, 2)
        assert.isNil(nil)
        assert.notNil(false)
        assert.isTrue(true)
        assert.isFalse(false)
        local m = assert.throws(function() error("boom") end, "boom")
        assert.contains(m, "boom")
        expect(3).toBe(3)
        expect(4):toBeGreaterThan(3)
        expect({ 1 }).toEqual({ 1 })
        expect(nil).toBeNil()
        expect(1).never.toBe(2)
        expect(function() error("x") end).toThrow("x")
        assert(true, "plain assert still works")
    end)
    it("fails equal", function()
        assert.equal(1 + 1, 3, "sum")
    end)
    it("keeps going after a failure", function()
        assert.isTrue(true)
    end)
    it("fails near", function()
        assert.near(1.0, 1.5, 0.1)
    end)
    it("fails throws", function()
        assert.throws(function() end)
    end)
    it("fails expect", function()
        expect("a"):toBe("b")
    end)
    it("errors", function()
        local t = nil
        return t.field
    end)
    it.skip("is skipped", function() error("never runs") end)
    it("is pending")
end)
"#;

#[test]
fn failures_do_not_stop_other_tests_and_point_at_their_line() {
    let rel = "Assets/Tests/Asserts.test.lua";
    let root = project("asserts", &[(rel, ASSERTS)]);
    let results = run_all(&root);
    assert_eq!(results.len(), 9);
    assert_eq!(
        result(&results, "passes").status,
        Status::Passed,
        "{results:?}"
    );
    assert_eq!(
        result(&results, "keeps going after a failure").status,
        Status::Passed
    );
    let line_of =
        |needle: &str| ASSERTS.lines().position(|l| l.contains(needle)).unwrap() as u32 + 1;
    for (name, needle, message) in [
        ("fails equal", "1 + 1, 3", "sum: expected 3, got 2"),
        ("fails near", "1.0, 1.5", "expected 1.5 ± 0.1, got 1.0"),
        (
            "fails throws",
            "assert.throws(function() end)",
            "expected the function to throw",
        ),
        ("fails expect", "toBe(\"b\")", "expected \"b\", got \"a\""),
        ("errors", "t.field", "attempt to index a nil value"),
    ] {
        let r = result(&results, name);
        assert_eq!(r.status, Status::Failed, "{name}");
        let f = r.failure.as_ref().unwrap();
        assert!(f.message.contains(message), "{name}: {f:?}");
        assert_eq!(
            f.location,
            Some((rel.to_owned(), line_of(needle))),
            "{name}: {f:?}"
        );
        assert!(f.traceback.contains(rel), "{name}: {}", f.traceback);
        assert!(!f.traceback.contains("test_framework"), "{}", f.traceback);
    }
    assert_eq!(result(&results, "is skipped").status, Status::Skipped);
    assert_eq!(result(&results, "is pending").status, Status::Skipped);
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn only_and_failing_hooks() {
    let root = project(
        "only",
        &[
            (
                "Assets/Tests/Only.test.lua",
                r#"
it("left out", function() end)
it.only("picked", function() end)
describe.only("Picked suite", function() it("also picked", function() end) end)
"#,
            ),
            (
                "Assets/Tests/Hooks.test.lua",
                r#"
describe("Broken setup", function()
    beforeAll(function() error("no database") end)
    it("a", function() end)
    it("b", function() end)
end)
describe("Broken each", function()
    beforeEach(function() error("bad each") end)
    afterEach(function() print("cleaned up") end)
    it("c", function() error("never runs") end)
end)
it("d", function() end)
"#,
            ),
            (
                "Assets/Tests/Broken.test.lua",
                "describe(\"x\", function() it(\"y\" end)",
            ),
        ],
    );
    let results = run_all(&root);
    assert_eq!(result(&results, "left out").status, Status::Skipped);
    assert_eq!(result(&results, "picked").status, Status::Passed);
    assert_eq!(result(&results, "also picked").status, Status::Passed);
    for name in ["a", "b"] {
        let f = result(&results, name).failure.clone().unwrap();
        assert!(
            f.message.contains("beforeAll failed: ") && f.message.contains("no database"),
            "{f:?}"
        );
    }
    let c = result(&results, "c");
    assert!(
        c.failure
            .as_ref()
            .unwrap()
            .message
            .contains("beforeEach failed")
    );
    assert_eq!(c.logs, ["cleaned up"], "afterEach still runs");
    assert_eq!(result(&results, "d").status, Status::Passed);
    let broken = result(&results, LOAD_ERROR);
    assert_eq!(broken.status, Status::Failed);
    assert_eq!(
        broken.failure.as_ref().unwrap().location,
        Some(("Assets/Tests/Broken.test.lua".into(), 1))
    );
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn edit_mode_tests_get_a_fresh_scratch_scene() {
    let root = project(
        "edit",
        &[(
            "Assets/Tests/Scene.test.lua",
            r#"
describe("Scratch scene", function()
    it("sees the active scene", function() assert.notNil(GameObject.Find("Cube")) end)
    it("can change it", function()
        GameObject.Find("Cube").name = "Renamed"
        assert.notNil(GameObject.Find("Renamed"))
    end)
    it("gets a fresh copy", function()
        assert.notNil(GameObject.Find("Cube"))
        assert.isNil(GameObject.Find("Renamed"))
    end)
    it("can load another scene", function()
        loadScene("Level")
        assert.notNil(GameObject.Find("Mover"))
        assert.isNil(GameObject.Find("Cube"))
    end)
    it("cannot wait for frames", function() yieldFrames(1) end)
    it("reports unknown scenes", function() loadScene("Nowhere") end)
end)
"#,
        )],
    );
    let results = run_all(&root);
    for name in [
        "sees the active scene",
        "can change it",
        "gets a fresh copy",
        "can load another scene",
    ] {
        assert_eq!(
            result(&results, name).status,
            Status::Passed,
            "{name}: {results:?}"
        );
    }
    let f = result(&results, "cannot wait for frames")
        .failure
        .clone()
        .unwrap();
    assert!(f.message.contains("only works in PlayMode tests"), "{f:?}");
    let f = result(&results, "reports unknown scenes")
        .failure
        .clone()
        .unwrap();
    assert!(f.message.contains("no scene called 'Nowhere'"), "{f:?}");
    let _ = std::fs::remove_dir_all(root);
}

const PLAY: &str = r#"
describe("Mover", function()
    it("walks right while D is held", function()
        loadScene("Level")
        local mover = GameObject.Find("Mover")
        assert.notNil(mover)
        local x0 = mover.transform.position.x
        Input.simulateKeyDown("d")
        yield(0.5)
        Input.simulateKeyUp("d")
        local x1 = mover.transform.position.x
        assert.greater(x1, x0 + 2)
        yield(0.2)
        assert.near(mover.transform.position.x, x1, 0.2, "stops once D is up")
    end)
    it("counts frames", function()
        local f = Time.frameCount
        yieldFrames(5)
        assert.equal(Time.frameCount, f + 5)
        local t = Time.time
        yield(0.5)
        assert.near(Time.time, t + 0.5, 0.02)
    end)
    it("starts every test in a fresh session", function()
        assert.isNil(GameObject.Find("Mover"))
    end)
    it("waits until", function()
        local start = Time.time
        waitUntil(function() return Time.time > start + 0.25 end, 2)
        waitUntil(function() return false end, 0.2)
    end)
    it("times out", function()
        setTimeout(0.5)
        yield(5)
    end)
    it("fails on logged errors", function()
        Debug.LogError("something broke")
        yieldFrames(1)
    end)
    it("can ignore logged errors", function()
        ignoreErrorLogs()
        Debug.LogError("expected")
    end)
end)
"#;

#[test]
fn play_mode_tests_step_through_frames() {
    let rel = "Assets/Tests/Mover.playtest.lua";
    let root = project("play", &[(rel, PLAY)]);
    let results = run_all(&root);
    for name in [
        "walks right while D is held",
        "counts frames",
        "starts every test in a fresh session",
        "can ignore logged errors",
    ] {
        assert_eq!(
            result(&results, name).status,
            Status::Passed,
            "{name}: {results:?}"
        );
    }
    assert!(result(&results, "walks right while D is held").frames >= 40);
    assert_eq!(result(&results, "counts frames").frames, 5 + 30);
    let f = result(&results, "waits until").failure.clone().unwrap();
    assert!(f.message.contains("not met within 0.2 s"), "{f:?}");
    let line = PLAY
        .lines()
        .position(|l| l.contains("return false"))
        .unwrap() as u32
        + 1;
    assert_eq!(f.location, Some((rel.to_owned(), line)));
    let f = result(&results, "times out").failure.clone().unwrap();
    assert!(f.message.contains("timed out after 0.5 s"), "{f:?}");
    let f = result(&results, "fails on logged errors")
        .failure
        .clone()
        .unwrap();
    assert!(f.message.contains("something broke"), "{f:?}");
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn runaway_loops_fail_instead_of_hanging() {
    let root = project(
        "loop",
        &[(
            "Assets/Tests/Loop.test.lua",
            "it(\"spins\", function() while true do end end)\nit(\"next\", function() end)",
        )],
    );
    let results = run_all(&root);
    let f = result(&results, "spins").failure.clone().unwrap();
    assert!(f.message.contains("without yielding"), "{f:?}");
    assert_eq!(result(&results, "next").status, Status::Passed);
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn step_runs_in_slices() {
    let root = project("slices", &[("Assets/Tests/Mover.playtest.lua", PLAY)]);
    let files = discover(&root);
    let mut run = TestRun::new(context(&root), &files, |c| c.name == "counts frames");
    assert_eq!(run.progress(), (0, 1));
    let mut steps = 0;
    while !run.step(std::time::Duration::ZERO) {
        steps += 1;
        assert!(steps < 1000);
    }
    assert!(steps > 30, "one frame per step: {steps}");
    assert_eq!(run.results()[0].1.status, Status::Passed);
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn cli_writes_junit_xml_and_exit_codes() {
    let root = project(
        "cli",
        &[
            (
                "Assets/Tests/Math.test.lua",
                "describe(\"Math\", function() it(\"adds\", function() assert.equal(1 + 1, 2) end) it(\"breaks <&>\", function() assert.equal(1, 2) end) end)",
            ),
            (
                "Assets/Tests/Mover.playtest.lua",
                "it(\"waits\", function() yieldFrames(2) end)",
            ),
        ],
    );
    let xml_path = root.join("out/results.xml");
    let args = |extra: &[&str]| {
        let mut a = vec![root.to_string_lossy().into_owned()];
        a.extend(extra.iter().map(|s| s.to_string()));
        a
    };
    let mut out = Vec::new();
    let code = cli::run(&args(&["--results", &xml_path.to_string_lossy()]), &mut out);
    let text = String::from_utf8(out).unwrap();
    assert_eq!(code, 1, "{text}");
    assert!(text.contains("2 passed, 1 failed, 0 skipped"), "{text}");
    assert!(
        text.contains("FAIL Assets/Tests/Math.test.lua > Math > breaks <&>"),
        "{text}"
    );

    let xml = std::fs::read_to_string(&xml_path).unwrap();
    let mut reader = quick_xml::Reader::from_str(&xml);
    let (mut cases, mut failures) = (0, 0);
    loop {
        use quick_xml::events::Event;
        match reader.read_event().expect("well-formed XML") {
            Event::Start(e) | Event::Empty(e) if e.name().as_ref() == b"testcase" => cases += 1,
            Event::Start(e) if e.name().as_ref() == b"failure" => failures += 1,
            Event::Eof => break,
            _ => {}
        }
    }
    assert_eq!(failures, 1, "{xml}");
    assert_eq!(cases, 3, "{xml}");
    assert!(
        xml.contains("<testsuites name=\"Misty Tests\" tests=\"3\" failures=\"1\""),
        "{xml}"
    );
    assert!(xml.contains("name=\"breaks &lt;&amp;&gt;\""), "{xml}");
    assert!(xml.contains("classname=\"Tests.Math.Math\""), "{xml}");

    // A filter that leaves only passing tests exits 0; a bad project or option exits 2.
    let mut out = Vec::new();
    assert_eq!(cli::run(&args(&["--filter", "ADDS"]), &mut out), 0);
    assert_eq!(cli::run(&args(&["--mode", "PlayMode"]), &mut Vec::new()), 0);
    assert_eq!(cli::run(&["Z:/no/such/project".into()], &mut Vec::new()), 2);
    assert_eq!(cli::run(&args(&["--bogus"]), &mut Vec::new()), 2);
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn cli_arguments() {
    let o = cli::parse(&[
        "P".into(),
        "--filter".into(),
        "Math".into(),
        "--mode".into(),
        "edit".into(),
    ])
    .unwrap();
    assert_eq!(o.project, PathBuf::from("P"));
    assert_eq!(o.filter.as_deref(), Some("Math"));
    assert_eq!(o.mode, Some(TestMode::EditMode));
    assert!(cli::parse(&[]).is_err());
    assert!(cli::parse(&["P".into(), "--results".into()]).is_err());
}

#[test]
fn templates_list_their_tests() {
    let root = project("templates", &[]);
    for (mode, rel) in [
        (TestMode::EditMode, "Assets/Tests/NewTest.test.lua"),
        (TestMode::PlayMode, "Assets/Tests/NewTest.playtest.lua"),
    ] {
        std::fs::write(root.join(rel), template(mode, "NewTest")).unwrap();
        let f = collect(&root, rel);
        assert!(f.error.is_none(), "{:?}", f.error);
        assert_eq!(f.tests.len(), 2);
        assert_eq!(f.mode, mode);
        assert_eq!(f.tests[0].suites, ["NewTest"]);
    }
    let results = run_all(&root);
    assert!(
        results.iter().all(|(_, r)| r.status == Status::Passed),
        "{results:?}"
    );
    assert_eq!(
        create_dir(&root, &root.join("Assets/Scripts")),
        root.join(TESTS_DIR)
    );
    let sub = root.join("Assets/Tests/Sub");
    assert_eq!(create_dir(&root, &sub), sub);
    let _ = std::fs::remove_dir_all(root);
}
