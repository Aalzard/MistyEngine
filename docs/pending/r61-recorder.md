# Round 61: the Recorder package (branch r61-recorder)

The ask: a Recorder that captures video, GIF and image sequences from the Scene Viewer or a
camera at a fixed frame rate with no dropped frames; an optional package, not installed by
default, that Remove takes out cleanly; a Recorder window, a Timeline Recorder track, Lua
`Recorder.Start/Stop`, docs and Turkish / Arabic strings.

## Status

| Part | State |
|---|---|
| Optional built-in packages (Package Manager) | done, tested |
| Recorder window (Window ▸ General ▸ Recorder, only when installed) | done, tested (kittest) |
| Capture time (fixed dt while recording) | done, tested |
| GPU readback ring + encoder threads | done, GPU test (`misty_recorder::gpu_tests`) |
| PNG / JPG sequences | done, tested (decoded back) |
| GIF | done, tested (frame count, size, timing) |
| MP4 (H.264) | done, tested (decoded back with OpenH264, frame count, error) |
| WebM (AV1) | done, tested (container parsed back: frame count, key frame, timestamps) |
| Timeline Recorder track / clip | done (clip data tested; Play-mode flow shares the manual path) |
| Lua `Recorder.Start([name])`, `Recorder.Stop()`, `Recorder.isRecording` | done, tested, docs/api regenerated |
| Audio capture | not done: videos are silent (see below) |
| User Guide chapter (en + ar), locales (tr + ar) | done |

## Decisions

- **Optional built-in packages** (`packages/optional.rs`): a second list next to the built-in
  modules. They show in the Package Manager's *Built-in* list marked *Optional* (✔ once
  installed) and, once installed, under *In Project*. **Install** writes
  `"com.misty.recorder": "*"` to `Packages/manifest.json`; resolution finds the engine's copy
  (origin built-in) and the lock file records it with source `builtin`. Nothing is copied
  into `Packages/`. **Remove** takes the manifest entry out and deletes the package's settings
  file (`ProjectSettings/Recorder.json`); recordings stay. Existing projects and templates
  have no manifest entry, so they don't get it. The editor caches the installed set
  (`EditorState::optional_packages`, read at project open and after Install / Remove); the
  Recorder's tab, its Window ▸ General item, the Timeline's Add ▸ Recorder Track and Add
  Recorder Clip, and the clip's settings only show when it's there. Removing it closes the
  tab and stops a recording. A folder / zip / git package can't take an optional package's
  name. `r63-behaviour-trees` adds another optional package: it can add a row to `PACKAGES`
  (and its settings file to `settings_files`).
- **Crate split**: the settings (`misty_engine::recorder`: source, format, resolution, frame
  rate, range, folder, pattern, take; the pattern expansion; the Lua request queue) live in
  the engine so Timeline clips and scripts can carry them. The capture and the encoders are in
  a new crate, `misty_recorder`, that only the editor links: built games don't carry
  OpenH264 or rav1e.
- **Capture time**: while a recording runs, `play::tick` advances the game by `1 / fps` of the
  fastest recording instead of the wall-clock frame time, so every editor frame is exactly one
  recorded frame. A slow machine records slower than real time; the file is smooth. Recorders
  at a lower rate than the fastest one keep only the frames on their own time grid.
- **No dropped frames**: each frame due is rendered into the recorder's own target at the
  output size (its own draw lists, the Scene Viewer's UI when Include UI is on, background
  loads waited for so nothing pops in), copied into a 4-buffer staging ring
  (`readback.rs`) and mapped asynchronously; finished copies go to the encoder a frame or two
  later. Only a full ring waits for the oldest copy. The encoder thread's queue holds 6
  frames; when it's full the main thread waits (capture time doesn't move meanwhile).
  PNG / JPG sequences encode on up to four threads. Finishing a file (joining the encoder,
  writing the MP4 index, flushing AV1) runs on a thread; the window says when it's written.
- **Sources**: Scene Viewer (the highest-Depth camera, as the Scene Viewer draws it),
  Camera (by object name, the same draws with that camera), Editor Scene (the Scene view's
  last image; a size change mid-recording is scaled to the first size). Scene Viewer and
  Camera recordings enter Play mode and leave it when they end (pressing Stop in Play ends
  them too). Editor Scene recordings run in Edit mode too, a frame per editor frame.
- **Ranges**: Manual; Frame Range `start..=end` of the recording's frames; Time Range
  `[start, end)` seconds → `round(start·fps) ..= round(end·fps) − 1`; Timeline Clip (set on a
  Timeline Recorder clip). Frames before a range run but aren't kept.
- **File names**: `<Scene>`, `<Take>` (3 digits), `<Frame>` (4 digits), `<Recorder>`,
  `<Project>`, `<Resolution>`, `<Source>`; characters a file name can't hold become `_`; image
  sequences get `_<Frame>` when the pattern has none. Auto-increment skips takes already on
  disk and saves take + 1.
- **Timeline Recorder track** (`TrackKind::Recorder`, not bindable; clips carry
  `recorder: RecorderSettings`, left out of the file when absent): in Play mode, a clip of a
  playing director records from the first frame the director's time is inside it to the
  first frame it isn't, so a 5 s clip at 30 fps makes 150 frames. Timelines with Recorder
  tracks load and play without the package (the track does nothing).

## Encoders and licences

| Format | Encoder | Licence | Notes |
|---|---|---|---|
| PNG / JPG sequence | `image` crate (already in the engine) | MIT / Apache-2.0 | |
| GIF | `image` → `gif` + NeuQuant (`color_quant`) | MIT / Apache-2.0 | delays in 1/100 s, alternated to keep the rate |
| MP4 (H.264) | OpenH264 (Cisco) compiled from the source bundled by the `openh264` crate | BSD-2-Clause (both) | Constrained Baseline, BT.601 limited, key frame every 2 s, frame skipping off; our own MP4 writer (`mp4.rs`) |
| WebM (AV1) | rav1e, pure Rust, without its nasm assembly | BSD-2-Clause (+ AOMedia patent licence) | speed preset 10; our own WebM writer (`webm.rs`) |

All are GPL-3 compatible and need nothing installed on Windows or Linux (the editor build
needs a C++ compiler for OpenH264, which every Rust toolchain on these platforms has: MSVC on
Windows, gcc / clang on Linux). nasm is used for OpenH264's assembly when it is on PATH and
skipped otherwise. H.264 is patent-encumbered: Cisco's royalty coverage applies to its own
binaries, not to OpenH264 built from source; distributors in countries with software patents
should know that (AV1 / WebM is the royalty-free choice).

## Not done / later

- **Audio**: kira mixes straight to the output device; capturing the mix means a tap in the
  audio thread (a kira effect on the main track writing into a ring) and an AAC (MP4) or Opus
  (WebM) encoder. Neither has a mature pure-Rust encoder; the checkbox is shown greyed out
  with "Sound is not recorded yet".
- The editor tests drive the capture loop without a GPU (black frames); the ring itself has a
  GPU test in the recorder crate. Not yet tried by hand in a running editor.
- Frame rates are whole numbers (no 29.97).
- OpenH264 refuses the non-real-time usage types; it runs as real-time camera video with
  frame skipping off and bit-rate rate control.

## Checks

Release build, target `E:/MistyTargets/r61-recorder`, commit 755ba415 (+ the GPU test after):

- `cargo fmt --all --check`: clean.
- `cargo clippy --workspace --all-targets --release`: 0 warnings.
- `cargo test --workspace --release --no-fail-fast`: all pass. Editor 952 passed (87 ignored),
  MCP end-to-end 1, engine 1608 passed (111 ignored), player 11, recorder 8.
- Recorder tests: exact N frames for frame and time ranges (crate and editor, the editor one in
  capture time with a 0.5 s "real" frame), PNG and JPG sequences decoded back, GIF frame count,
  size and timing, MP4 decoded back with OpenH264 (40/40 frames, mean error under 12/255),
  WebM parsed back (12 blocks, key frame first, timestamps, duration), install / remove,
  window and menu follow the package, Lua start / stop only with the package.
- No render code changed (the recorder renders through the existing renderer into its own
  target), so no golden shots.
- Note: the engine's GPU tests in the workspace run overlapped with r59 holding
  `gpu-timing.lock` (the lock was taken by r59 after my run had started; stopping my own test
  process was not permitted). The recorder GPU test ran under the lock.
