# Profiling and debugging depth (round 20E, branch r20-profiler)

Goal: find problems without reading logs. The Profiler gains a GPU module with per-pass
timings, Physics / Audio / UI modules, a Timeline with a row per thread, capture files and a
connection to a running Player; the Frame Debugger lists indirect draws, shows bindings and the
depth, normals, GI and emission buffers and steps through draws; the Memory Profiler splits GPU
and CPU memory and lists the CPU asset caches.

## Profiler (Window ▸ Analysis ▸ Profiler, Ctrl+7)

| Module | Chart | Details pane |
|---|---|---|
| CPU Usage | Main Thread time stacked by category, GPU time as a line | Hierarchy (merged call tree) or Timeline |
| GPU Usage | GPU time stacked by pass group: Shadows, Depth / Culling, Opaque, Grass & Trees, Transparent & Effects, Lighting / GI / Volumes, Post-processing, Other | Hierarchy: each view (Game, Scene) and its passes with group, share and ms, then per-group totals |
| Rendering | Batches | Batches, SetPass, triangles, culling, virtual geometry counters |
| Memory | GPU MB | Objects, meshes, textures, buffer and texture estimates |
| Physics | Physics time | Physics / step / sync / ragdoll+cloth times, fixed steps, bodies (active, sleeping, kinematic, static), colliders, triggers, contact pairs, joints, Character Controllers |
| Audio | Audio.Update + audio-thread mixing time | Both times, playing sources, virtual voices, graph voices, mixer groups, effects, streams, DSP load (% of one core), underruns |
| UI | UI time | Layout / render / events times, canvases, elements, texts, interactive elements, batches, vertices |

### GPU timings

`render/pass_timer.rs` has a second mode for the Profiler: a ring of 8 query sets, each render
writing a timestamp at every existing `mark` between passes (and inside the main pass when the
device has `TIMESTAMP_QUERY_INSIDE_PASSES`), resolved and mapped after submit and collected at
the start of a later render, never waiting. The parts go to `profiler::add_gpu_passes` for the
frame they were recorded in, named by view (`Renderer::set_view_label`: the editor's Game and
Scene views, the Player's Game). The renderer turns the timer on while the Profiler records and
drops it when recording stops; the Frame Debugger's capture and an explicit blocking pass timer
(`set_pass_timing`, the benchmarks) keep it off.

Fallback: without `TIMESTAMP_QUERY_INSIDE_ENCODERS` the frame records `GpuTiming::FrameOnly`
(the existing start/end timer still gives each render's whole GPU time, shown in the chart as
Other with a note); without timestamp queries at all, `Unsupported` and the module says so.

### Timeline rows

Samples now carry absolute times (ns since the profiler's clock started) and are made relative
to the frame's start (the previous `end_frame`). The thread that ends frames is the Main
Thread; on any other thread a finished outermost scope is moved into that thread's shared
buffer (one lock per outermost scope), which `end_frame` collects into `FrameData::threads`.
Rows: Main Thread, then **Render (GPU)** per view (the passes placed after the render's
`Render.Submit`, since rendering is encoded on the main thread here), then the audio thread
(`Audio.Mix`, named by `profiler::name_thread`), the background pool (`MeshSdf.Bake`,
`VirtualGeometry.Build`, `Navigation.BakeTile`; rows "Background N") and rayon's workers
(`Render.ShadowList` per shadow map, `Render.DrawPrep (job)` per rayon job; rows "Worker N").
A scope that began in an earlier frame is drawn from the frame's start.

### Captures and the Player

- Save… writes the frames shown (`.mprof`, one JSON line per frame: samples, counters, thread
  rows, GPU passes); Load… shows a file instead of the editor (older captures without the new
  fields still load). `MISTY_PROFILE=capture.mprof` still records from the first frame in the
  editor and the Player.
- Target ▸ Player listens on `127.0.0.1:54998`. A Player started with
  `MISTY_PROFILER_CONNECT=127.0.0.1:54998` connects, records and streams each frame (a sender
  thread serialises; when it falls behind frames are dropped, never the game). The window
  shows the Player's frames while the target is selected.
- Recording stops while the Profiler window is out of sight (closed or behind another tab) and
  while it shows a file or a Player; it starts again when shown.

## Frame Debugger

- **Indirect draws**: kind Draw Indirect for virtual geometry's cluster draws, GPU-culled grass,
  Visual Effect outputs and, with **GPU-driven** on, GPU culling's multi-draws ("Opaque
  (GPU-driven) ▸ First / Second Phase", one event per `multi_draw_indexed_indirect`, replayable
  like any other event). GPU-driven keeps GPU culling on during the capture (it used to be off
  so every draw was CPU-side).
- **Bindings** card: what groups 0–3, the vertex / index buffers, the instances and the indirect
  arguments hold for mesh draws, fullscreen steps, culled multi-draws, virtual geometry, grass
  and effects (descriptive; the engine's own layouts).
- **View** picker: Color (the event's target, as before), Depth (prepass depth, near bright),
  Normals (prepass world normals), GI (Misty GI's gathered irradiance), Emission (every material
  unlit showing its emission, sky off). Depth and Normals force the depth + normal prepass for
  the capture; GI needs Misty GI ("GI not in this frame" otherwise).
- **Draws only**: the slider and arrow keys step through draw calls, skipping clears, copies and
  compute.

## Memory Profiler

Every item is on the GPU or the CPU (`MemLocation`). Summary shows GPU memory by category, then
CPU memory by category; the Table has a Where column. New CPU items (`memory_snapshot::
cache_items`): each cached model's mesh data and embedded textures, runtime (video) texture
frames, Misty GI's mesh distance field bakes (new category), and in Play mode the clips the
mixer actually holds (decoded frames or compressed bytes) instead of the estimate.

## Overhead (MeadowDemo, `profiler_overhead_meadow`)

`cargo test --release -p misty_engine --lib profiler_overhead_meadow -- --ignored --nocapture`
renders MeadowDemo's wide view at 1920×1080 (High), the GPU waited for after every frame, and
times `Renderer::render` + `profiler::end_frame` in alternating blocks of 40 frames with the
Profiler off and recording (per-pass GPU timestamps on, as the window records). This machine:

| | median | mean |
|---|---|---|
| Off | 3.162 ms | 3.228 ms |
| Recording (32 scopes, 35 GPU passes per frame) | 3.263 ms | 3.311 ms |
| **Overhead while recording** | **0.101 ms** | **0.083 ms** |

Closed (not recording) a scope costs 0.74 ns (one relaxed atomic load, the name never built),
about 0.00002 ms per frame here; the pass timer is dropped, so no timestamps are written.

## Tests

- profiler: frame-start relative samples, worker / background / audio rows, rayon worker
  names, counters per frame, GPU passes reaching their frame and grouping, renderer labels
  mapped to groups, save / load (and old captures), Player → editor streaming over TCP.
- GPU: per-pass times reach their frames without waiting and recording off drops the timer;
  bindings on draws; GPU-driven captures list indirect draws and match the CPU-culled image;
  Depth / Normals / GI / Emission views.
- memory: GPU / CPU split, cached models listed, mixer clips named by project path.
- editor: GPU hierarchy per render, Timeline lanes, module counter rows, module names, file
  source round trip, Draws only stepping, view / GPU-driven changes starting a new capture,
  sorting by Where.

## Screenshots (`docs/screenshots/r20/`)

| File | Shows |
|---|---|
| `profiler_cpu_timeline.png` | MeadowDemo in Play mode: module charts, CPU Timeline with Main Thread, Render (GPU) · Game and worker rows |
| `profiler_gpu.png` | GPU Usage stacked by pass group, GPU Hierarchy of the Game view |
| `profiler_physics.png` | PhysicsDemo: Physics module times and counters |
| `profiler_audio.png` | AudioDemo2: Audio module (audio-thread mixing, voices, groups, effects, streams, DSP load) |
| `profiler_ui.png` | UIShowcase: UI module (layout / render / events, canvases, elements, batches) |
| `frame_debugger_bindings.png` | PhysicsDemo: a draw's Bindings card |
| `frame_debugger_indirect.png` | MeadowDemo: Draw Indirect grass event, replayed up to it |
| `frame_debugger_depth.png`, `frame_debugger_normals.png` | Depth and Normals views |
| `memory_profiler_cpu_gpu.png` | Memory Profiler Summary: GPU and CPU memory by category |

Screenshot hooks added: `MISTY_PROFILER_MODULE`, `MISTY_PROFILER_VIEW=Timeline`,
`MISTY_PROFILER_LOAD`, `MISTY_FRAME_DEBUGGER_VIEW`, `MISTY_FRAME_DEBUGGER_GPU`,
`MISTY_DOCK_SPLIT` (a taller bottom panel). `MISTY_FRAME_DEBUGGER=<text>` now selects the
first capture that has the event (streamed work shows up a few frames in).

## Not done

- A separate render thread doesn't exist; the "Render (GPU)" row shows the GPU queue's passes,
  aligned to the CPU submit only approximately (GPU and CPU clocks aren't correlated).
- Per-pass timings on devices without timestamps inside encoders (would need pass-level
  `timestamp_writes` on every pass).
- Indirect draws report the candidate (upper bound) instance count; the visible count stays on
  the GPU.
- The Player connection is one Player at a time, local by default, without discovery or a
  Build Settings "Autoconnect Profiler" toggle.
- Bindings are descriptions, not live resource handles; no pixel history / shader debugging.
