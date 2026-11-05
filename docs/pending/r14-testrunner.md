# Round 14: Test Runner (project tests in Lua)

## Decisions

1. **Behaviour, not code.** The window, the EditMode/PlayMode split and the command line follow the
   reference engine's test runner; the Lua API (`describe`/`it`/hooks/`assert`/`expect`) follows
   the common Lua and JavaScript test frameworks. Nothing is copied, no NUnit names.
2. **Files by suffix under `Assets/Tests/**`.** `*.test.lua` = EditMode, `*.playtest.lua` =
   PlayMode. Reason: no assembly definitions or attributes in Lua; the name says the mode, and
   game scripts (`*.lua` elsewhere) are never picked up. Assets ▸ Create ▸ Testing puts new files
   in `Assets/Tests` (or the current folder when it is already inside it).
3. **The framework is Lua** (`testing/framework.lua`, `include_str!`): it collects a tree of
   suites/tests and runs one test with its hooks (`__misty.run(index, next)`) inside a coroutine,
   returning `{status, message, traceback}`. Rust only drives coroutines and sessions. Reason:
   hooks, error levels and `xpcall` tracebacks are natural in Lua; yields across `xpcall` work in
   Lua 5.4, so hooks and tests can both wait for frames.
4. **One Lua state per test file per run, separate from the game's.** The file is evaluated once;
   its tests share file-level locals like a test class. Game scripts keep their own Lua state.
   To use the game API, the runner *lends* the game's `World` (scene, physics, input, logs…) to the
   test state between frames (`ScriptHost::lend_world`, `Runtime::lend_world`, additive). Reason:
   `loadScene` can restart the play session (new `ScriptHost`, new Lua) without killing the test
   coroutine, and `beforeAll` state survives across tests. The API is Lua-state agnostic
   (userdata wrap entities; only UI listeners live in a Lua registry — listeners a test adds are
   not called, noted below).
5. **`World::new`** (additive) builds the world the host starts with; `start_with_physics` uses it
   and EditMode tests get one around a scratch scene.
6. **EditMode isolation:** every test gets a fresh `World` around a fresh copy of the active scene
   (the editor's open scene; the CLI's first enabled Build Settings scene, else a default scene),
   with physics built for queries and nothing playing. `loadScene` swaps in a scene file.
7. **PlayMode isolation:** every test gets its own `Runtime` (silent audio) on an empty
   "Test Scene", like the reference engine's init test scene; `loadScene(name)` restarts the
   session on that scene (Awake now, Start on the next frame). Game-script `LoadScene` requests
   do the same; additive load/unload use `apply_scene_request`; `Quit` is ignored. The session is
   dropped when the test ends. In PlayMode `beforeAll` runs inside the first test's session
   (documented: find objects in `beforeEach`).
8. **Fixed 1/60 s frames** for PlayMode tests, whatever the machine; `yield(seconds)` counts scaled
   game time (`Time.time`), `yieldFrames(n)` frames. Reason: deterministic CI. Default per-test
   limit 30 s of game time (`setTimeout`); `waitUntil` has its own timeout (10 s).
9. **Runaway Lua** (`while true do end`) is stopped by an instruction-count hook with a 10 s
   wall-clock deadline per resume (1 s in unit tests); the test fails, `afterEach` still gets a
   second, the run goes on. Timed-out PlayMode tests run their `afterEach`/`afterAll` via
   `__misty.abort` outside the coroutine.
10. **Logged errors fail tests** (game script errors, `Debug.LogError`), like the reference engine's
    log assertions; `ignoreErrorLogs()` opts out. Reason: a script error in the game under test
    should not pass silently.
11. **`debug` library**: test states are created with `StdLib::DEBUG` (`unsafe_new_with`) so the
    framework can read test lines and tracebacks; it keeps `getinfo`/`traceback` as locals and
    removes the global before any test code runs, so tests see the game scripts' environment.
12. **Failure location** = the first `<test file>:<line>` in the message, else in the traceback,
    else any `Assets/…lua:<line>`; assertions raise at level 2 so it is the test's line. The
    message is stored without that prefix; JUnit and the CLI print `file:line: message`.
    Tracebacks drop framework frames.
13. **A file that fails to load** shows as one failing stand-in test `(file failed to load)` at
    the error line, so CI fails on syntax errors.
14. **Runs step in slices** (`TestRun::step(budget)`): opening a file, starting a test or one game
    frame per unit; the editor gives it 12 ms per frame (`test_runner::tick` from `game_frame`),
    so PlayMode tests visibly step and the editor stays responsive. The CLI steps to the end.
15. **Separate from the editor's Play mode**: test sessions never touch `s.scene`/`s.runtime`, so
    unsaved edits are safe; runs are refused while the editor plays (the play clock and audio are
    global). The Game view doesn't show test sessions (left for later).
16. **Window ▸ General ▸ Test Runner** is an on-demand tab (`Tab::ON_DEMAND` grows to 13) that
    docks with the Scene/Game views (the tree and the failure details want height; the bottom
    Project/Console area cut the traceback off in the first screenshots). Rerun on save polls test-file mtimes once a second.
    Failure links always open the built-in Code Editor (an external editor can't jump to a line).
17. **CLI**: `misty --run-tests <project> [--filter X] [--mode EditMode|PlayMode] [--results x.xml]`
    lives in `testing::cli` (engine) so it is unit-tested; `main.rs` only dispatches. Exit 0/1/2.
    JUnit: one `<testsuite>` per file, `classname` = path (without `Assets/` and suffix) + suites,
    `file`/`line` attributes, `<failure>` with message + traceback, `<skipped/>`, `<system-out>`.
18. **Screenshot hook** `MISTY_RUN_TESTS=EditMode|PlayMode|1` runs both modes to the end at startup
    and opens the window on that mode; `MISTY_TEST_SELECT=<test name>` selects a test.

## Progress

- Engine: `testing` module (framework.lua, discovery/collection, `TestRun` driver with play
  sessions and simulated input, JUnit writer, CLI), `World::new`, `ScriptHost::lend_world`,
  `Runtime::lend_world`.
- Editor: Test Runner window (tabs, tree with icons/durations, Run All/Selected/Failed, Cancel,
  Rerun on save, search, failure details with file:line links and traceback, summary bar),
  Assets ▸ Create ▸ Testing ▸ EditMode Test / PlayMode Test, `--run-tests`, `MISTY_RUN_TESTS`.
- SampleProject: `Assets/Tests/MathTests.test.lua`, `PlayerControllerTests.playtest.lua`,
  `VehicleTests.playtest.lua`.
- Docs: docs/scripting.md "Testing (Test Runner)".
- Tests: framework semantics (hook order, failures don't stop others, throws/near/expect,
  only/skip/pending, failing hooks, load errors, runaway loops), EditMode scratch scenes,
  PlayMode yields/frames/loadScene/input/timeouts/log errors, slicing, CLI XML + exit codes,
  templates; editor kittests (menu, Run All, search filter, failure link → Code Editor, Run Failed,
  PlayMode tab), tree rows, Create menu, and the sample project's tests passing.

## Left for later

- Show a running PlayMode test in the Game view; per-test "Run in Play mode of the editor".
- UI listeners (`AddListener`) added from test code are stored in the test's Lua state and not
  called by the game.
- EditMode `Destroy` is queued but not applied (no frame runs); physics queries see the scene as
  it was when the test started.
- `LogAssert.Expect`-style expected messages; parameterised tests; code coverage.
