# r44: first open of a project and shader compiles off the main thread

Branch `r44-first-open` (from main 9320362d). Target `E:\MistyTargets\r44-first-open`,
TMP/TEMP `C:\MistyTry\r44tmp`, editor runs on copies under `C:\MistyTry\r44\`.

## What was measured before (main 9320362d)

Fresh copy of SampleProject, `Library/` removed, `MISTY_WINDOW_SIZE=1100x650`,
`MISTY_SCREENSHOT` capture run with `MISTY_PROFILE`; each run with a freshly named copy of the
exe (NVIDIA's driver cache is per executable, so a new name is a cold driver cache, as for a
user's first launch of a new build). RTX 4060, Vulkan. Two other agents' builds were running.

| Run | First editor frame | Main-thread frame 0 | Of it |
| --- | --- | --- | --- |
| SampleScene, run 1 | 12.3 s | 11.5 s | `Renderer.New` 7.2 s (36 pipelines: terrain shade 2.5 s, terrain 2.3 s, grass 1.4 s), scene render 3.2 s (`Pipeline: mesh` 2.5 s), asset DB 0.3 s, state 0.3 s |
| SampleScene, run 2 | 10.2 s | 9.4 s | `Renderer.New` 5.7 s, scene render 2.7 s |
| SampleScene, run 3 (new exe name) | 8.7 s | 8.0 s | `Renderer.New` 5.4 s, scene render 1.8 s |
| MeadowDemo | 10.3 s | 9.6 s | `Renderer.New` 2.7 s, scene render 6.0 s (`Render.PrepareAssets` 3.9 s, two `mesh` variants 2.9 s) |

All of it before the first frame: the window showed the splash and then nothing (Windows:
Not Responding). After the first frame no frame was over 100 ms in these scenes except the
second meadow frame (313 ms). Texture block compression was already on a worker
(`sprites::compress::Mode::Background`); texture decodes, mip chains and model imports were
not.

## Changes

See "Round 44" in `docs/progress.md`.

## Measured after

(to fill)

## Left

(to fill)
