# Round 65: a game made only with nodes

A short 2D platformer ("Lumo's Leap") built with Script Graphs only: no Lua, no Rust gameplay
code. The point is to prove the nodes a real game needs, and to list what is broken or missing.

## Status (2026-11-14): done; branch `r65-node-game`, main merged in (round 66), never pushed

- All checks green after the round 66 merge: `cargo fmt --all --check`, clippy (`--workspace
  --all-targets --release`) 0 warnings, `cargo test --workspace --release --no-fail-fast`: one
  failure, `panels::project::dock_tests::right_click_on_empty_space_opens_the_folder_menu_everywhere`
  ("Create ⏵" not found), which passes alone: a flake under load, not this branch.
- The game uses the round 66 fixes: On Trigger Enter / Exit 2D (Enemy Touch, Goal), the menu
  through the buttons' On Click (mouse click tested; Enter presses the selected Play button),
  native Physics2D Raycast / OverlapCircleAll, Friction 0.4 on the ground again.
- Added this session: a dash cooldown line on the HUD ("Dash: ready" / "Dash: ..."), the
  Round 65 section of `docs/progress.md`, built player screenshots. The test file compiled
  and passes (the stage test needed the project root set for tile colliders and short hops).
- Sounds (jump, dash, hit, hurt, win) and a smoothed camera were already in.

### History (first handover)


### Done

- **The game plays end to end, nodes only**, checked headlessly with main's binary
  (`misty --run-tests` on a temp project): menu, 3 stages, run / jump (coyote time, jump
  buffer, variable height), E dash (white glow + particle trail), Brute (2 hits, flash, knock
  back, burst prefab), Puff (pushed, fades, destroyed), patrol between ledges and walls, hearts,
  pits, stage restart, goal loads the next stage, win screen back to the menu. An `Autopilot`
  graph beats all three stages and reaches the win scene (about 24 s of game time).
- **Files:** `SampleProject/Assets/NodeGame/` (13 graphs in `Graphs/`, 5 scenes in `Scenes/`,
  `Prefabs/Burst.prefab`); the 5 scenes were added to
  `SampleProject/ProjectSettings/BuildSettings.json` (graphs load scenes by name). No `.meta`
  files (like `Assets/VisualScripting` and `Assets/Guides`). The game was only ever run in a
  temp project made from the 2D Platformer template, never inside SampleProject itself.
- **Generator:** `python tools/node_game/node_game.py <project folder> [--demo]` writes the
  graphs, scenes and prefab (`vgraph.py` = graph builder with auto layout, `graphs.py` = the
  graphs, `scenes.py` = scenes from text maps). `--demo` puts Autopilot on the player in place
  of Player Input. The committed sample files are its output for `SampleProject`.
- **Player build:** `misty --build <temp project> <out>` succeeded (graphs, prefab, audio
  packed). The built game was not run.
- **Test (written, NEVER COMPILED):** `crates/misty_editor/src/sample_node_game.rs`
  (registered in `main.rs`): only-graphs check, stage 1 played with simulated keys (move, dash
  kills the Puff, Brute takes two, stage 2 loads), menu and win screen.
- **User Guide chapter 9:** `docs/guide/en/09-node-game.md` and `ar/`, index entries, registered
  in `panels/guide/content.rs` with `docs/guide/images/guide-node-game.jpg`; the node-name test
  range in `panels/guide/tests.rs` now includes chapter 09. Guide tests NOT run.
- **Screenshots:** `docs/screenshots/r65/node-game-{menu,stage1,stage2,stage3,dash,win}.jpg`
  (editor Play captures; stages and dash from the Autopilot demo). The stage 2, stage 3 and
  win pictures were saved without being looked at.

### Left to do (first handover; all done now)

1. Get a build slot; `cargo test --release -p misty_editor sample_node_game` and
   `... guide`; fix what does not compile or pass (the test file has never been built; the
   guide names nodes in bold `Owner: Member` form that must match real node titles).
2. When the fixer's batch 1 is on main: merge main, set `TRIGGERS = True` in
   `tools/node_game/graphs.py` (Enemy Touch and Goal then use On Trigger Enter / Exit; both
   versions are written, the trigger one is untested), regenerate, re-test. The menu buttons
   already carry On Click entries (`Play` / `Quit` on the Menu object) and the Menu graph has
   the matching Custom Events; check that a click works. The guide text describes this final
   design, so it is ahead of the committed graphs until then.
3. If native 2D physics nodes land (batch 2), consider using them; if the friction combine is
   fixed, Friction 0 on the ground tilemap and side walls in `scenes.py` can go back to 0.4.
4. Run the built player once (window, screenshot). Look at the unchecked screenshots.
5. Round 65 section in `docs/progress.md` (not written).
6. Checks: `cargo fmt --all --check`, clippy (0 warnings), `cargo test --workspace --release
   --no-fail-fast`. None were run: no build slot was obtained in about an hour, and the target
   `E:\MistyTargets\r65-node-game` is empty.
7. Clean up `C:\MistyTry\r65` (temp projects `NodeGame`, `NodeGameDemo`, `Build`, `bin` with a
   copy of main's verify `misty.exe` / `misty_player.exe`, `shots`) and `C:\MistyTry\r65tmp`.

### How to run it

- Temp project: `misty --new "2D Platformer" <folder>` (opens a window; set `MISTY_SCREENSHOT`
  so it exits), delete its Lua scripts and `Main.scene`, copy
  `Assets/Audio/{Blip,Beep,Clap,Chime,Buzz}.wav` from SampleProject, run the generator on it and
  list the 5 scenes in its `ProjectSettings/BuildSettings.json`. `C:\MistyTry\r65\NodeGame`
  (keyboard) and `C:\MistyTry\r65\NodeGameDemo` (Autopilot) are set up this way.
- Headless play: put `tools/node_game/dev/demo.playtest.lua` in `<project>/Assets/Tests/` and
  run `misty --run-tests <project> --mode PlayMode` (it dumps positions through a failing
  assert; use `yieldFrames`, not `yield(seconds)`: finding A).
- Captures: `tools/node_game/dev/shot.sh <project name> <scene> <frame> <out.png>` (takes the
  GPU lock; paths point at `C:\MistyTry\r65`). The dash shows around frame 470 of
  `NodeGameStage1` in the demo project. Capture windows open maximised (finding C).
- `tools/node_game/dev/cb.sh <log> <cargo args>` waits for a build slot (3 s poll) and runs
  cargo with this round's target and TMP.

### Issue status

Issues 1 to 3 (batch 1) and issue 4 with findings A to D and the `MISTY_SCRIPTGRAPH` capture
problem (batch 2) were reported to the coordinator and given to the fixer (branch
`r66-node-fixes`). All are merged on main (round 66) and merged into this branch (2026-11-14); findings B (friction) and E/C (captures) were fixed there too.

## Issues

Each issue: how to reproduce, the graph, expected and actual. Status is updated when a fix is
merged.

### 1. 2D triggers and collisions never reach Script Graphs

- **Repro:** an object with a Rigidbody 2D, a Box Collider 2D and a graph with
  `On Trigger Enter -> Debug.Log("hit")` (or `On Collision Enter`). Move it into another 2D
  collider (trigger or solid).
- **Graph:** `OnTriggerEnter.Out -> Api(Debug.Log).In`, `OnTriggerEnter.Other -> Message`.
- **Expected:** the event runs with `Other` = the other object, as it does for 3D colliders (and as
  Lua gets `OnTriggerEnter2D` / `OnCollisionEnter2D`).
- **Actual:** nothing runs. `ScriptHost::dispatch_2d`
  (`crates/misty_engine/src/scripting/mod.rs`) calls only Lua instances; the 3D `dispatch` calls
  `self.graphs.contact(...)` first, the 2D one never does. A 2D game cannot use the four contact
  events at all.
- **Workaround used:** distance checks and `Physics2D.OverlapCircleAll` each frame.
- **Status:** fixed in round 66 (On Trigger / Collision Enter / Stay / Exit 2D nodes); Enemy Touch and Goal now use On Trigger Enter / Exit 2D.

### 2. A UI Button's On Click cannot run a Script Graph

- **Repro:** a Button whose On Click entry targets an object that has a graph with
  `Custom Event: Play`, method `Play`. Click the button while playing.
- **Expected:** the graph's `Custom Event: Play` runs (the entry is "call `method` on the scripts
  of `target`", and graphs are scripts; `gameObject:SendCustomEvent` already reaches both).
- **Actual:** a Console warning, `On Click: no script on 'Menu' has a method 'Play'`, and nothing
  runs. `ScriptHost::send_message_with` only looks at Lua instances; the same holds for On Value
  Changed and On End Edit of the other widgets. There is no node to listen to a button either
  (`button.onClick:AddListener` needs a Lua function).
- **Workaround used:** the menu reads keys and the mouse with `Input` nodes.
- **Status:** fixed in round 66; the menu's Play button (selected first, so Enter presses it) runs the Menu graph's Custom Event *Play*. Tested with a mouse click.

### 3. API nodes of variadic Lua functions have no input pins

- **Repro:** in the node search pick `Physics2D: Raycast`, `Rigidbody2D: AddForce`,
  `Rigidbody2D: MovePosition`, `Input: GetButton`.
- **Expected:** pins for the arguments (origin, direction, distance, layer mask…).
- **Actual:** the node has no inputs: the binding's signature is `(...)`, and
  `signature_args` (`crates/misty_editor/src/panels/script_graph/search.rs`) drops `...`. The
  node can only be used after typing names into Graph Properties ▸ Arguments, which nothing
  hints at.
- **Workaround used:** `args` typed by hand (`origin, direction, distance, mask`).
- **Status:** fixed in round 66 (documented signatures, optional pins).

### 4. No typed 2D physics nodes (missing nodes)

- **Repro:** look for 2D versions of the native `Physics: Raycast` node (pure, with typed
  `Hit`, `Point`, `Normal`, `Distance`, `Collider` outputs) and of `Rigidbody: Get / Set
  velocity`.
- **Actual:** `Physics2D: Raycast`, `OverlapCircle`, `OverlapCircleAll`, `OverlapBox`,
  `OverlapPoint` and every `Rigidbody2D` member only exist as Lua-binding nodes: all pins are
  Any, the queries are action nodes with exec pins although they only read, and a raycast's
  result is an opaque Lua table (test it with `Not Equal` against nothing, read it with
  `Get Lua Field`).
- **Graph used:** `Physics2D: Raycast (origin, direction, distance, mask) -> Result`,
  `Result -> Not Equal (B empty) -> Branch`.
- **Expected:** native nodes like the 3D ones: `Physics2D: Raycast` pure with `Hit` (Bool),
  `Point`, `Normal`, `Distance`, `Collider` (GameObject); `Rigidbody2D: Get / Set velocity` as
  Vector3; the overlap queries pure, returning GameObject / List.
- **Workaround used:** the Lua-binding nodes above (they work).
- **Status:** fixed in round 66; the game uses the native Physics2D Raycast (Hit) and OverlapCircleAll and the typed Rigidbody2D velocity.

## Other findings (not node bugs)

Found while building the game; listed so they are not lost.

### A. PlayMode tests never end when the game itself loads scenes

- **Repro:** a `*.playtest.lua` that loops `yield(0.5)` over a game which calls
  `SceneManager.LoadScene` every few seconds (the node game restarting a stage when the player
  dies). `misty --run-tests <project> --mode PlayMode` ran at 100 % CPU for 5 minutes, until
  killed.
- **Cause:** a game-side `LoadScene` starts a new session whose `Time.time` restarts at 0
  (`testing/run.rs`, `game_request`). `yield(seconds)` waits for `Time.time >= target` of the
  old clock, and the test timeout compares the session's game time (`run.rs` ~l.548), so
  neither is reached while the game keeps loading scenes. There is no wall-clock limit.
- **Expected:** `yield` and the timeout count game time across scene loads.
- **Workaround:** `yieldFrames(n)`.

### B. A frictionless player still sticks to walls (2D friction is averaged)

- **Repro:** a Rigidbody 2D player whose Box Collider 2D has Friction 0, pushed sideways
  (velocity set each Fixed Update) against the side of a Tilemap Collider 2D or Box Collider 2D
  with the default Friction 0.4, in the air.
- **Actual:** it hangs on the wall and slides down very slowly: the two frictions are averaged
  (0.2), `physics2d/colliders.rs` sets `.friction(...)` only (Rapier's default combine rule).
- **Expected:** like the familiar 2D engines, the combined friction is the geometric mean (or
  the minimum), so one frictionless collider is enough.
- **Workaround used:** Friction 0 on the ground tilemap and the side walls too.

### C. `MISTY_WINDOW_SIZE` capture windows open maximised

- **Repro:** `MISTY_WINDOW_SIZE=1100x650 MISTY_SCREENSHOT=out.png misty <project>` (also with
  `800x500`). The screenshot is 1920x1032 and the window reports `IsZoomed = true`, rect
  `-8,-8,1928,1040`, on a 1920x1080 display.
- **Expected:** a 1100x650 window at the top left, as `main.rs` (~l.443) intends.

### D. The MCP bridge has no Script Graph tools

- `misty --mcp` can create objects, components, Lua scripts and materials, but not `.vgraph`
  files or their nodes and wires, and `scene.add_component` attaches "a Lua script by path".
  The game's graphs were written as files (`tools/node_game`).

### E. Captures never fire while a tab hides the scene views (`MISTY_SCRIPTGRAPH`)

- **Repro:** `MISTY_SCRIPTGRAPH=Assets/.../Door.vgraph MISTY_CAPTURE_FRAME=60
  MISTY_SCREENSHOT=out.png misty <project>`: the editor keeps drawing (4,000 frames in 25 s)
  but no screenshot is taken, with any graph.
- **Likely cause (read, not proven):** `app.rs` (~l.1177) holds the capture while
  `background_busy()` is true; with the Script Graph tab in front that seems never to clear.
- **Effect here:** no Script Graph window screenshots of the game's graphs, so the generator's
  auto layout has never been looked at in the editor.

### F. Built player: the menu capture timed out twice (not reproduced)

- With the round 66 binaries, `NodeGame.exe --scene .../NodeGameMenu.scene` with
  `MISTY_SCREENSHOT` did not reach the capture frame twice in a row (40 s and 90 s timeouts)
  while `cargo test --workspace` was loading the machine; the stage and win scenes captured
  fine in the same loop. Four later menu runs captured in 3 to 6 s. The menu is the only scene
  with an Event System `first_selected` button. Not reported: no reliable repro.

## Player build check (2026-11-14, after merging round 66)

`misty --build` of a temp project with the generated game, then the built `NodeGame.exe` with
`--scene <scene> --size 1100x620` and `MISTY_SCREENSHOT`: menu, the three stages (with the
"Dash: ready" HUD line) and the win screen render correctly.
Pictures: `docs/screenshots/r65/node-game-player-{menu,stage1,stage2,stage3,win}.jpg`.
