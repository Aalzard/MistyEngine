# First Person Player: controller, locomotion signal and head bob

## Decisions

1. **One rig, four scripts, one owner of the pose.** `Player ▸ HeadBobVertical ▸ HeadBobHorizontal ▸
   Camera`: `FirstPersonController.lua` on the Player (Character Controller) is the only script that
   *sets* a rotation (Player yaw, Camera pitch, every Update); the bob scripts store their base local
   pose at Start and write `base + offset` in LateUpdate. Mouse look and the bob never fight because
   they never write the same value: the camera's base pitch comes from the controller's signal, the
   rotation script adds its offset to that. Reason: additive layering is the only arrangement where
   each script can be removed or tuned on its own and nothing accumulates.
2. **Locomotion signal instead of cross-script calls.** The controller publishes a plain table
   (`signal`: state, measured planar speed, `speed01`, grounded, air time, stride phase, phase rate,
   landing impulse, strafe / forward input, turn rate, crouch amount, pitch). The bob scripts find it
   once by walking up the parents (`GetComponent("FirstPersonController")`, a new API: a script
   instance by its name). Any other mover can publish the same table and reuse the bob.
3. **Stride phase advanced by distance, not time.** `phase += speed / (2·step) · 2π · dt` on the
   ground only, one stride being two footsteps of `walkStepLength` 1.6 m → `runStepLength` 2.2 m
   (blended with the speed): 2.5 bounces/s walking, 3.2 running. The bob frequency then
   matches the footsteps at any speed and the phase freezes the moment the feet stop (no sliding bob
   when pushing into a wall: the *measured* Character Controller velocity drives it, not the input).
   Vertical bob is `sin(2·phase)` (two bounces per stride), horizontal sway and roll `sin(phase)`
   (left foot, right foot).
4. **SmoothDamp everywhere a target changes.** Planar velocity (`accelerationTime` 0.12 s,
   `decelerationTime` 0.08 s, ÷ `airControl` in the air), bob amplitudes (`blendTime` 0.25 s), strafe
   lean / tilt, turn tilt, the final roll and pitch offsets, the FOV kick and the crouch amount all use
   the critically damped SmoothDamp (never overshoots, frame-rate independent). Stopping therefore
   fades the amplitude while the phase holds, so the head settles from wherever it is instead of
   snapping to 0; leaving the ground fades the bob out the same way.
5. **Spring-damper impulses for landings.** The landing dip (vertical) and pitch kick (rotation) are a
   damped spring (frequency 7 / 6 Hz, damping ratio 0.9) kicked with a velocity proportional to the
   impulse, integrated in 1/240 s sub-steps so a long frame stays stable. A spring reads as weight
   where a SmoothDamp would read as a slide. The impulse is the fall speed mapped from 4 m/s (0) to
   12 m/s (1): stepping off a kerb gives nothing, a platform drop a visible dip.
6. **Conservative defaults.** 3 / 5 cm vertical, 2 / 3.5 cm sway, 0.8° / 1.6° roll, pitch bob capped
   at 2°, roll at 6°, pitch offsets at 5°; breathing 4 mm at 0.3 Hz while idle. Everything is a public
   field, so the Inspector tunes it per instance.
7. **Jump feel.** Coyote time 0.12 s, jump buffer 0.15 s, jump speed from `jumpHeight` (√(2gh)),
   gravity 20 m/s² with a 40 m/s terminal velocity, `-2 m/s` ground stick so `isGrounded` stays true
   on slopes and steps (the Character Controller's autostep and snap-to-ground do the rest).
8. **Crouch changes the capsule.** `CharacterController.height` / `center` / `radius` became settable
   (the physics capsule is rebuilt in place); the controller lowers the centre with the height so the
   feet stay put, and stands up only when a SphereCast above the crouched capsule finds room. The eye
   drop is one more additive offset in HeadBobVertical (`crouchDrop`), driven by the smoothed
   `signal.crouch`.
9. **Cursor lock is a request scripts make, windows apply.** `Cursor.lockState` / `Cursor.visible` are
   stored in the world; the editor sends `CursorGrab` / `CursorVisible` viewport commands while the
   Game view has the input (Escape, Stop, pause and a lost focus release it and reset the script-side
   state so a click can lock again), the player calls winit (`Locked`, falling back to `Confined` on
   Windows) and releases on lost focus. While locked, the mouse delta is the raw device motion
   (`egui::Event::MouseMoved` / `DeviceEvent::MouseMotion`), since the pointer no longer moves.
10. **Pure maths for unit tests.** `FirstPersonController.Locomotion` (step / afterMove / publish),
    `FirstPersonController.look` and `HeadBob*:Evaluate(signal, dt)` take no scene, so
    `misty_engine::scripting::test_scripts` (a new helper: script classes as globals + a Lua chunk,
    with the game API registered) drives them from Rust tests in `fps_player.rs`.
11. **Scripts are the source.** The editor embeds the sample project's `.lua` files with
    `include_str!`, so GameObject ▸ First Person Player writes exactly the shipped scripts into a
    project that lacks them (never overwriting) and the "matches its generator" test is trivially the
    same text.

## Tuning notes

- Amplitude fades in over the first half of walking speed and blends walk → run between the two
  speeds; raise `walkAmplitude` before `runAmplitude` if walking feels flat.
- Nausea comes from roll and pitch more than translation: keep `maxRoll` ≤ 6° and `pitchBob` small;
  `turnTilt` is per degree/second of mouse turn, so 0.004 × 500°/s = 2° (the cap).
- `blendTime` 0.25 s is the shortest blend that hides a start/stop; longer feels floaty.
- For a heavier character raise `landingDip` and lower `landingFrequency`; `landingDamping` below 0.8
  overshoots upwards once, which can read as a bounce.
- `mouseSmoothing` 0.02–0.05 s takes the jitter out of high-DPI mice without noticeable lag.

## Progress

- Engine: `Cursor` / `CursorLockMode`, `Input.mouseDelta`, `GetComponent("Camera")` with
  `fieldOfView` / clip planes / `enabled`, `transform.childCount` / `GetChild` / `Find`,
  `GetComponent("<ScriptName>")`, settable `CharacterController.height/radius/center`
  (`Physics::set_controller_size`), Run / Crouch default axes, `Input.simulateMouseDelta` for
  PlayMode tests, `scripting::test_scripts`.
- Editor: GameObject ▸ First Person Player (also in the Ctrl+K menu search), cursor grab while
  playing with raw mouse motion, `fps_player.rs` generator + tests.
- Player: cursor grab / visibility from `Cursor`, raw motion while locked.
- SampleProject: the four scripts, `Prefabs/First Person Player.prefab`, `Scenes/FirstPersonDemo.scene`,
  Run / Crouch axes, `Tests/FirstPersonTests.playtest.lua`.
- Docs: scripting.md "First Person Player" and the API table rows; screenshots in
  `docs/screenshots/fps/` (Play mode in the Game view, the course in the Scene view with the
  prefab instance selected).

## Left for later

- Footstep sounds (the signal's phase crossings are the hook) and a land sound scaled by the impulse.
- Gamepad look (right stick) in the controller; the axes exist, the script reads only Mouse X / Y.
- Slide / mantle states; moving platforms (the Character Controller doesn't inherit their motion).
- A `Cursor` setting for the editor's Game view to confine instead of lock.
