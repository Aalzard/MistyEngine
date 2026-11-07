# Test health (branch test-health)

Goal: `cargo test --workspace --release` green every time, and a golden-shot gate for look
changes. Started from main 52765ec, where one test failed on every run and two failed now and
then under load, plus an occasional access violation in the engine's GPU tests.

## 1. `foliage_lighting_demo_matches_its_generator` (failed on every run)

The scene data says grass density 600, the generator 1500.

- `FoliageLightingDemo.terrain` (and the Tall Grass overlay) were committed once, in 11cf089
  (Nov 5, 20:08), with density 600. Nobody edited them since.
- The generator never named a number: `density: details::MAX_DENSITY`. `MAX_DENSITY` was 600
  then. 08e831c (Grass realism, Nov 5, 21:37) raised it to 1500 for MeadowDemo and regenerated
  MeadowDemo only, so the foliage generator silently started saying 1500.

Neither side was a deliberate change to this demo. The committed data is what the demo was
made, tuned and shot with, and what the editor renders, so the **generator** was the side that
drifted: it now says `density: 600.0` with a note. No data changed.

## 2. `scripted_meadow_run_has_no_hitches` (flaky under load)

Causes found:

- **The verdict used wall time.** The run already retried once, but a frame counted as a
  hitch whenever its wall time was over 2.5× the median and the GPU was ordinary. On a loaded
  machine the render thread (and the rayon workers `Render.FrameLists` waits for) queue for a
  core: with 14 busy-loop processes on 16 cores, frames of 25–64 ms showed up with
  `Render.FrameLists` at 13–47 ms while the render thread's own CPU work was 0.9–1.7× its median
  — the profile the earlier failures had ("spikes in `Render.Culling` and `Trees.Prepare`").
- **Global state shared with other tests.** The profiler's recording switch, frame counter and
  history are process-wide; the profiler's own tests switched recording off, ended frames and
  filled the history while a timing test ran (its GPU times vanished or landed on the wrong
  frames). And `headless()` switched grass streaming off *before* waiting for the GPU lock, so a
  test queueing for the GPU turned streaming off under the running hitch test, which then
  scattered every grass chunk on the render thread — a real-looking hitch.

Changes:

- Each frame records the render thread's own CPU time (`QueryThreadCycleTime` on Windows,
  `CLOCK_THREAD_CPUTIME_ID` on Linux; it does not grow while the thread waits for a core) and
  the machine's busy share during the frame (`GetSystemTimes`, `/proc/stat`).
- A slow frame (wall over 2.5× the median, GPU ordinary) **fails** when the render thread's work
  was over 2.5× its median (our CPU work: a scatter or a shader compile on the render thread),
  or when the thread was off its core on a machine under 85 % busy (a wait of ours: a lock, a
  file, a blocking call). It is **excused** and printed only when the work was ordinary and the
  machine was at least 85 % busy. Both attempts must have a hitch to fail. The slow-frame lines
  now show `work N x median, machine N % busy`.
- `profiler::test_serial()`: one lock for the profiler tests, the hitch test and the grass flight
  timing test.
- `headless()` (engine and editor) switches streaming off only once it holds the GPU lock.

Checked: alone, 3 runs pass (work ratios of slow frames 1.9–3.2×, all on a machine under 65 %
busy and under the wall limit). Under 14, 16 and 20 busy loops it passes: the stalls are
excused (work 0.9–1.7×, machine 100 % busy) or come with GPU spikes. The first run after the
build had 5 real hitches of 25–39 ms in its first attempt (cold file cache) and passed on the
second, as before.

## 3. `the_watcher_walks_on_a_thread_and_reports_new_files` (flaky under load)

Causes found:

- **A race in `poll_now`.** The first `poll_now` ends by starting a walk. When the test then
  writes `Blue.mat` while that walk is still listing (likely on a loaded machine: the thread
  starts late), the next `poll_now` waited for *that* walk, which never saw the file, compared
  "no change", and returned `None`: "a rescan for the new file" failed.
- **The process-wide database.** The test installed its temp project as the global GUID
  database, which every other test in the process reads (GUID tables of saved scenes, asset
  listings) while it ran.

Changes: `poll` is now `poll_in(db, walk)` on a given database slot and walk (the editor's
`poll()` passes the globals). The test uses its own slot and walk, never the global ones.
`poll_now_in` waits for a walk already under way and drops it (it may predate the caller's
change), then walks afresh and compares; each wait is on the walk finishing with a 60 s
timeout, not a fixed sleep. The test also covers the walk-already-running case on purpose and
the editor's one-poll-late path. 40 runs in a row and 144 runs 24 at a time: no failure.

## 4. The occasional `0xc0000005` in the GPU test phase

Seen twice in round 14 (`docs/pending/r14-videotext.md`), the debug-built engine test binary,
both times passing on rerun. Reproduction attempts on this branch's release build, RTX 4060,
Vulkan, NVIDIA 610.88, holding the GPU timing lock:

| Run (before the instance change) | Result |
|---|---|
| Engine test binary, default threads, 4 runs | 4 × 951 passed, exit 0 (106–120 s) |
| Same, `--test-threads=1`, 2 runs | 2 × 951 passed, exit 0 (116–138 s) |
| Two engine test processes at once + 8 busy loops, 3 rounds | 6 × 951 passed, exit 0 |

No crash reproduced. What the code did that could cause one, and what changed:

- **An instance per test.** Every `headless()` made its own `wgpu::Instance`; the test's
  device was the last thing holding it, so every GPU test ended by destroying its Vulkan
  instance, and the Vulkan loader unloads the driver DLL with the last instance (sixty-odd
  unload/reload cycles per run). An unload racing a driver thread the previous device left
  winding down is a classic access violation in a module that is gone; it fits "occasional,
  passes on rerun, during GPU tests". Now one process-wide instance (`gpu_tests::instance()`,
  and the same in the editor's `sample_render`), never dropped: the driver loads once per run.
  Devices stay one per test.
- **Workers outliving their test.** With streaming on (the hitch and grass flight tests), a
  renderer starts `Prebuilt` workers that compile pipelines and upload cloud noise on its
  device; one not taken before the renderer is dropped kept building after the test ended,
  while the next test made and used its own device. `prebuilt::wait_idle` counts the running
  workers and `headless()` waits for them (up to 120 s, with a warning) before making the next
  device, so no two tests' devices are busy at once.
- The grass study's own device (ignored tests) now takes the GPU lock and the shared instance.
- Not the cause: every GPU test already went through the GPU lock; no test uses the global
  shader-module or pipeline caches (editor and player only); the engine's few `unsafe` blocks
  are unrelated to the GPU tests.

The disk-full builds of other branches also died with `0xc0000005` (rustc, `gfx-shadows-base`),
so a crash while E: was nearly full is another possibility for round 14.

## 5. Golden shots

    cargo test --release -p misty_engine --lib golden_shots -- --ignored --nocapture --test-threads=1

- 20 shots: MeadowDemo wide, close, behind and oak (the `meadow_perf_shots` views) and
  CyclesRoomDemo as saved, each at Low, Medium, High and Ultra. Rendered at 1600×900, 48 frames
  (wind clock fixed), box-filtered to 320×180 and compared with `docs/golden/<shot>.png`
  (2.1 MB in all; `docs/golden/adapter.txt` names the GPU and driver they were made on).
- A shot drifts when its mean channel difference is over 0.5 (of 255) or over 0.25 % of its
  pixels differ by more than 16 in a channel. Drifting shots are listed and the test fails;
  `target/golden/<shot>_now.png`, `_ref.png` and `_diff.png` (difference ×4) show where.
- Repeat runs on this machine are identical (every shot mean 0.00, 0 % changed, two runs).
  Neighbouring quality levels differ by mean 2–5 on MeadowDemo; CyclesRoom's levels differ by
  only 0.06–0.10 at this size, so a change there must be clearly visible to fail.
- `MISTY_GOLDEN=update` rewrites the references after a look change was checked and meant;
  `MISTY_GOLDEN_SHOTS=meadow_high,cycles` limits the run to matching shots. The comparison
  itself has a unit test (`golden_drift_tolerates_noise_and_catches_changes`).
- About 50 s. Every "flip a default" change runs it before merging; a drift is either the
  intended look change (update the references in the same commit, say which shots) or a bug.

## 6. Full runs

`cargo test --workspace --release --no-fail-fast`, twice in a row (all changes in, GPU timing
lock held):

| Run | misty_editor | misty_engine | misty_player | Failures |
|---|---|---|---|---|
| 1 (280 s) | 582 passed, 62 ignored | 951 passed, 61 ignored | 11 passed | 0 |
| 2 (196 s) | 582 passed, 62 ignored | 951 passed, 61 ignored | 11 passed | 0 |

Plus, on the final build: `golden_shots` passes (20 shots, all identical to the references) and
the hitch test alone passes.

## Checks

`cargo fmt --all`; `cargo clippy --workspace --all-targets --release`: 0 warnings.
