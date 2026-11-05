# Round 11 — Virtual cameras (branch `r11-vcam`)

## Decisions

1. **Own names: "Camera Brain" and "Virtual Camera"** (GameObject ▸ Camera ▸ Virtual Camera /
   Free Look Camera). The behaviour follows the familiar package; names, code and data are ours.
2. **Engine module `vcam/`** (components, solve, rig, noise) instead of `scene/`: the components
   register in the scene registry like `world_gen`'s, so `scene/` only gets two registry lines and
   two object-reference entries (`follow`, `look_at`) for remapping on load/duplicate/prefab.
3. **Runs only in Play mode**, inside the script host after `LateUpdate` (or right after the
   physics steps when the first brain's Update Method is Fixed Update, advancing by
   steps × fixed timestep). Edit mode leaves the Main Camera where it was authored, so opening a
   scene never rewrites camera transforms and dirties it. Reason: no edit-mode writes to the
   scene; the Game view shows the vcam result as soon as Play starts.
4. **One pass for all cameras**: every active virtual camera is solved every frame (no standby
   round-robin) and the pass is chosen by the first active brain. Simple and deterministic; scenes
   have a handful of vcams.
5. **Live camera** = highest priority among enabled, active-in-hierarchy virtual cameras; ties go
   to the most recently enabled (activation stamps; at Play start later Hierarchy order counts as
   more recent). Changing a priority from script is not an activation.
6. **The vcam's Transform carries its raw pose** (body + aim, no dutch, collider or noise), and
   the brain's camera gets the final pose. Damping state therefore survives inspection in the
   Inspector, and Do Nothing bodies/aims read the Transform (scripts can animate it).
7. **Blends**: Cut / Ease In Out / Ease In / Ease Out / Linear with a duration; custom blends are
   matched by object *name* (empty = any camera; exact pair > any→to > from→any > any→any >
   default) so they need no reference remapping. A blend interrupted half way restarts from a
   frozen snapshot of the shown pose (no jump); otherwise the outgoing camera keeps moving during
   the blend.
8. **Damping** is time-based: a gap is ~99 % closed after the damping time (exponential,
   frame-rate independent). 0 = rigid.
9. **Composer** works in yaw/pitch angles from the camera position: the dead zone turns nothing,
   the soft zone turns with damping, the soft-zone edge is a hard clamp; the first frame snaps the
   target to the screen position. Exact horizontally, a close approximation vertically at large
   yaw offsets (tests check against the real projection).
10. **Third Person** body covers "framing / third person": shoulder offset, vertical arm, camera
    side, distance, per-axis damping of the tracked point and a dead zone in screen fractions at
    the camera distance; its rotation follows the target (used when the aim is Do Nothing).
11. **Orbital (Free Look)**: one sphere around the damped target with yaw/pitch axes driven by
    Input Manager axes ("Mouse X"/"Mouse Y" by default; value += input × speed × dt, wrap or
    clamp). Free Look = Orbital body + Composer aim. No three-rig spline; kept simple.
12. **Collider** casts a ray from the look-at point (else the follow target) to the camera,
    ignoring the camera, its targets and their children and an optional tag; it pulls the camera
    to `hit − radius`, never closer than Minimum Distance; separate damping for moving in
    (default instant) and back out.
13. **Noise** is a pure function of (profile, gains, seed, time): two layers of 1D gradient
    (Perlin) noise per position/rotation channel; presets Handheld (slow sway) and Shake (fast).
14. **Lua**: `GetComponent("VirtualCamera")` → `priority`, `follow`, `lookAt` (GameObject,
    Transform or nil), `enabled`, `fieldOfView`, `dutch`, `isLive`; `GetComponent("CameraBrain")`
    → `activeVirtualCamera`, `isBlending`, `blendWeight`.
15. **Gizmos**: selected vcam shows its lens frustum (with dutch) and lines to its targets in the
    Scene view; the Game view paints the Composer zones (red outside soft, blue soft, clear dead)
    plus a dot on the tracked point when a Composer vcam is selected.
16. **`MISTY_CAPTURE_FRAME=<n>`** (capture.rs) delays the self-screenshot, to catch blends.

## Progress

- Engine: `crates/misty_engine/src/vcam/` (components, solve, rig, noise, tests), host wiring in
  `scripting/mod.rs`, `Runtime::with_cameras`, Lua `scripting/vcam_api.rs`.
- Editor: Inspector editors (`panels/inspector/vcam_editors.rs`: Lens, Body, Aim, Noise,
  Collider sub-headers; live status while playing), create menu (`vcam_tools.rs`, adds a Camera
  Brain to the main camera), gizmos and Game view overlay (`vcam_gizmos.rs`).
- Sample: `VirtualCameraDemo.scene` from `sample_vcam.rs` with `PathRunner.lua` and
  `CameraDirector.lua`.
- Tests: priority/ties, blend weights and interrupted blends, custom blend lookup, transposer
  damping, composer soft zone, hard look-at, third person, orbital input, collider vs wall, noise
  determinism, scene round-trip, Lua API, demo plays through its shot list.

## Left

- Edit-mode preview of the live virtual camera in the Game view.
- Timeline "Camera Cut" track (after Timeline lands).
- Free Look with three orbit rigs, lookahead, group framing, impulse/shake events.
