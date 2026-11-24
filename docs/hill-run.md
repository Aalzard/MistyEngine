# Hill Run (C# sample game)

A small two-player split-screen game in `SampleProject/Assets/HillRun`, written only
in C#. It exists to show the C# scripting API working together in a real game, and its play
tests guard the engine features it uses.

## Open and play

1. Install the .NET SDK (8 or later) and open `SampleProject` in the editor.
2. Open `Assets/HillRun/HillRun.scene` and press Play.
3. In the start menu, pick **Play** with the arrow keys / d-pad and Enter / A.
4. Join: any key joins Player 1 on the keyboard (WASD or arrows to walk, Space to jump); any
   button on a gamepad joins Player 2 (left stick or d-pad, A to jump). The screen splits in two
   when the second player joins.
5. Race for the six gold pickups: one on the flat start, one high up (jump for it), two up the
   ramp and on the top of the hill, one on the landing at the top of the steps and one next to
   the steep ramp (too steep to walk up). The last pickup ends the race.
6. Esc or Start pauses (`Time.timeScale = 0`); the pause menu works with the keyboard, a
   gamepad or the mouse.

The scene is also in Build Settings, so a built player can start it (`Main Menu` reloads it by
name).

## What is in it

| File | What it does |
| --- | --- |
| `HillRun.scene` | The level (ramp, steps, steep ramp, pickups), the menus, the Event System and the `Game` object with the Player Input Manager (Runner.prefab, two players, split screen). |
| `Runner.prefab` | A Character Controller (slope limit 45°, step offset 0.3, Slope Speed 0.5) with a Player Input (Send Messages), the jointed body of the CharacterAnimatorDemo with an Animator, and a camera behind it. |
| `Runner.controller` | Idle / Walk / Run on a `Speed` float; the Base Layer has IK Pass on. |
| `HillRun.inputactions` | Player map: Move, Jump, Pause, for Keyboard&Mouse and Gamepad. |
| `Scripts/GameManager.cs` | Menus, joining (`EnableJoining`, `OnPlayerJoined`), spawns, scores, pause, win screen. |
| `Scripts/HillRunner.cs` | `CharacterController.Move` with gravity and jumping, `OnJump` / `OnPause` messages, turns the body, sets `Speed`. |
| `Scripts/FootIK.cs` | `OnAnimatorIK`: rays down from the feet, `SetIKPosition` / `SetIKRotation`, look-at. |
| `Scripts/Pickup.cs` | Spins and bobs; `OnTriggerEnter` scores for the runner that touched it. |
| `Scripts/MenuButton.cs` | `OnSelect` / `OnDeselect` highlight of the selected button. |

## Regenerating and tests

The files are made by `crates/misty_editor/src/sample_hill_run.rs`:

```sh
cargo test -p misty_editor --bin misty -- --ignored write_hill_run   # rewrite the sample
cargo test -p misty_editor --bin misty hill_run                      # check and play it
```

`hill_run_matches_its_generator` checks the committed files. The play tests (skipped without
.NET) load the scene through the runtime and inject keys and a fake gamepad: menu navigation
and highlight, joining with split-screen camera rects, walking up the ramp (slower, grounded),
the steep ramp blocking, steps, jumping, pickups and scores, the win screen, pause with
`Time.timeScale`, animator states following the speed, and foot IK keeping the soles on the
ground.
