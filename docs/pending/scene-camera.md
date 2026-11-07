# Scene camera feel (branch scene-camera)

User: "The camera moving inside the editor for the scene tab is laggy a little bit, or maybe it
needs smoothed-out movement. Even when looking around."

## What was measured

`MISTY_CAMERA_BENCH=<frames>` (scene_camera/bench.rs) flies the Scene camera through a
screenshot run: 90 frames still, then forward with W while turning 30°/s, through the same
smoothing code as right mouse + W. It prints frame pacing, editor CPU, Scene render CPU and GPU
time, grass uploads, the camera's speed per frame and the **input → GPU done** latency (from the
start of the frame, where input is read, until the GPU finished that frame's Scene image; a
polling thread notices within ~0.25 ms; present and scan-out come on top).

```
MISTY_SCREENSHOT=out.png MISTY_SCENE=Assets/Scenes/MeadowDemo.scene MISTY_TAB=Scene
MISTY_CAMERA_BENCH=600 MISTY_CAPTURE_FRAME=760 MISTY_FRAME_LATENCY=1|2 MISTY_VSYNC=0|1
target/release/misty SampleProject
```

### Results (MeadowDemo, docked Scene view, RTX 4060, release build)

The machine was running five other cargo builds during these runs, so CPU times and hitch
counts are inflated and vary run to run; the comparisons below are within that noise.

| 600 flying frames | latency 2, no VSync (before) | latency 1, no VSync | latency 2, VSync | latency 1, VSync |
|---|---|---|---|---|
| frame interval median | 11.2 ms (89 FPS) | 12.8 ms (78 FPS) | 15.0 ms | 17.6 ms |
| p90 / p99 | 26.7 / 79.2 ms | 26.9 / 58.9 ms | 77.6 / 470 ms | 29.5 / 47.3 ms |
| frames > 1.5× median | 165 | 132 | 205 | 125 |
| input → GPU done, median | 18.3 ms (1.6 frames) | 20.1 ms (1.6 frames) | 15.4 ms | 37.0 ms |
| editor ui() CPU, median | 8.3 ms | 8.0 ms | | |
| Scene render CPU / GPU, median | 6.3 / 6.6 ms | 6.2 / 7.0 ms | | |

- **The Scene view is CPU-bound while flying**: the editor's frame (8 ms, of which the Scene
  render's CPU side is 6 ms) is as long as the GPU frame (7 ms), so the swapchain queue never
  fills. One frame of latency instead of two measured no lower input → GPU latency (18–20 ms,
  about 1.6 frames, either way), so the window keeps egui's two frames
  (`MISTY_FRAME_LATENCY` overrides it).
- **Hitches**: 22–27 % of flying frames take over 1.5× the median. In those frames the Scene
  render's CPU time nearly doubles (10 ms against 5.5 ms) and the wait outside ui() grows; only
  about a quarter of them have more than 2 ms of grass upload. **Grass**: one chunk upload alone
  went over the 2 ms `UPLOAD_BUDGET` in 66–73 of 600 frames (up to 27–28 ms in one frame): the
  first chunk of a frame always uploads, so a large chunk blows the budget. That is in the grass
  renderer (left to its owner); flying at full accelerated speed (20 m/s) streams chunks every
  frame.
- **VSync on** caps at the 60 Hz display and drops frames to 33 ms whenever one misses 16.7 ms
  (p99 470 ms in one run); the default stays off.
- **Fly speed per frame**: variation 0.01 % (distance per frame follows the real frame time), so
  dt jitter does not cause the judder.

## Causes (code reading, checked by tests)

- **No frame behind**: the Scene panel reads input and moves the camera before it renders, and
  the Scene image is submitted before egui's pass in the same frame. Not a cause.
- **Repaint**: the view repainted while a mouse button was held or a transition ran, idle
  otherwise. Correct, kept (now also while smoothing or a glide is still moving).
- **Mouse look used the cursor position** (`pointer.delta()`): Windows pointer acceleration,
  whole-pixel steps at slow speeds and a hard stop at the screen edge. The Game view's cursor
  lock already used raw motion.
- **Nothing was smoothed**: fly speed jumped from 0 to full on key press and back to 0 on
  release, Shift jumped ×4, the wheel zoomed in egui's scroll-area smoothing (90 % in 0.1 s, not
  frame-rate independent) and always towards the pivot.
- **Fly dt**: `stable_dt` is the real interval while repainting, so distance per frame follows
  the frame time; the bench's speed-per-frame variation confirms it. Not a cause.
- **Frame queue**: egui's `HIGH_THROUGHPUT` surface (two frames in flight). Measured: no effect
  here, the view is CPU-bound (see above). Kept.
- **Shift+D while flying** deselected everything (Shift + strafe right); Shift+F toggled Lock
  View.

## What changed

- `scene_camera/smooth.rs`: exact critically damped springs (SmoothDamp-style), frame-rate
  independent: `Chase` (input moves a goal, the view follows; increments applied, so look keeps
  the eye fixed and orbit the pivot exactly) and `Velocity` (fly velocity eased, distance
  integrated exactly).
- `SceneCamera` (scene_camera/mod.rs):
  - right mouse look / Alt + left orbit / middle pan / Alt + right zoom go through `Chase` with
    **Look Smoothing** (default 0.012 s: a steady drag is followed 12 ms behind, under one
    60 FPS frame);
  - right mouse + WASD/QE fly eases in and glides out with **Damping** (default 0.08 s, the
    glide after letting go is speed × 0.08 s), speed builds up while held (**Acceleration**,
    ×4 after 2 s), Shift ×4, right mouse + wheel changes the speed and shows it briefly in the
    view;
  - wheel zooms towards the point under the cursor (unsmoothed wheel events, eased with Damping);
  - look and orbit use raw mouse motion when the platform sends it; flying hides the cursor,
    confines it to the window and puts it back where the drag started;
  - F (Frame Selected) and the other transitions ease in and out over 0.3 s;
  - the flight uses the view direction halfway through each frame, so turning while flying
    takes the same path at any frame rate.
- Camera popover (📷 in the Scene toolbar): Field of View, clipping, then Navigation: Camera
  Speed, Acceleration, Easing (off = raw input), Damping, Look Smoothing, Reset. Saved per
  project in `Library/EditorSettings.json` (`scene_camera`).
- `MISTY_FRAME_LATENCY=<1..3>` sets the window's frames in flight for measuring (default 2, as
  before).
- Shift + letter shortcuts are ignored while flying.

## Defaults

| setting | default |
|---|---|
| Field of View | 60° |
| Dynamic Clipping | on (near 0.03, far 10 000 when off) |
| Camera Speed | 1 (5 m/s), range 0.01–50 |
| Acceleration | on (×1 → ×4 over 2 s) |
| Shift | ×4 |
| Easing | on |
| Damping | 0.08 s |
| Look Smoothing | 0.012 s |
| Wheel zoom | ~10 % per notch, towards the cursor |
| Frame Selected transition | 0.3 s ease in-out |

## Tests

- smooth.rs: step response identical at 30 / 60 / 144 FPS; streamed mouse input agrees within
  half a 30 FPS lump and lands exactly; steady lag = smooth time − half a frame; a held key
  flies the same distance at every rate (and easing in/out costs no distance); comes to rest.
- mod.rs: a 1 s turning flight + glide at 30 / 60 / 144 FPS ends within 2.4 cm (60 vs 30) and
  5.4 cm (144 vs 30) of 8.5 m; look lag under a 60 FPS frame; Easing off is exact; glide =
  speed × damping then rest (no more repaints); wheel zoom keeps the point under the cursor on
  screen; F eases and lands exactly; right mouse + wheel changes speed, not zoom.
- panels/scene.rs (egui_kittest): right drag look, RMB + W / Q / E, Shift faster, raw
  `MouseMoved` turns without cursor motion, RMB + scroll speed, scroll zooms towards the
  cursor and settles, MMB pan, Alt + LMB orbit, the popover shows the navigation settings.

## Not done

- The grass first-chunk upload going over budget, and the Scene render's CPU time while flying,
  are renderer work for the grass / culling branches.

- Shift+D while flying is fixed in `app.rs` shortcuts but has no test (no app-level harness).
- Cursor wrap at the screen edge is not needed with raw motion; on platforms without raw motion
  the look still stops at the edge.
