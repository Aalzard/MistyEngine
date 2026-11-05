# Round 14 — Frame Debugger and Memory Profiler

## Progress

- **Frame Debugger** (Window ▸ Analysis ▸ Frame Debugger, `panels/frame_debugger`): Enable
  pauses Play mode (resumed on Disable) and freezes the Game view on one frame. The left side is
  a tree of the frame's events grouped by pass (Shadows ▸ Cascade N / Local Light Layer N,
  Ambient Occlusion, Depth Prepass, Clouds, Volumetric Fog, Sky, Opaque, Terrain, Scene Copy,
  Water, Transparent, Particles, Visual Effects, Gizmos, Halos & Flares, Sun Shafts,
  Post ▸ Bloom / Tonemap / Anti-aliasing, UI) with counts, folding and a search filter. Each draw
  shows object, mesh, material, submesh, vertex / index / instance counts and the batch reason
  (GPU instanced, static batch, or why not batched). A slider, ◀ ▶ and ←/→ step through events;
  the Game view replays the frame up to the selected event and shows that event's render target
  (tonemapped HDR, shadow map depth, bloom mip, pre-FXAA composite, final image). The right side
  shows shader / pipeline, render target + size / format, blend / depth / cull, bound textures
  with thumbnails and material values.
- **Capture hook** (`render/frame_debug`): `Renderer::begin_capture(CaptureRequest)` /
  `end_capture() -> FrameCapture`; `Renderer::capture_event` for events drawn outside
  `render` (the Game view's canvas UI). Object and material names come from the new
  `DrawCache::labels`.
- **Memory Profiler** (Window ▸ Analysis ▸ Memory Profiler, `panels/memory_profiler`): Capture
  takes a `MemorySnapshot` (`misty_engine::memory_snapshot`) of items in nine categories
  (Textures, Meshes, Render Targets, Buffers, Audio, Scripts/Lua heap, Scene Objects,
  Particles/VFX, Lightmaps). Views: Summary (bars per category, tracked total, process working
  set), Table (sortable by name / type / category / size / refs, filter), Tree Map (squarified,
  nested per category, hover details), Compare (category before / after / change and items by
  size of change). Sources: `Renderer::memory_items`, `RenderTarget::memory_bytes`,
  `Gpu::view_target_items`, `memory_snapshot::{scene_items, audio_items, lua_item}`,
  `TextureCache::entries`, `Runtime::lua_memory`, `ComponentInfo::size`.
- Docs: `docs/debug_tools.md`. Screenshot hooks: `MISTY_FRAME_DEBUGGER=1|<n>|<name text>`,
  `MISTY_MEMORY_CAPTURE=<n>`, `MISTY_MEMORY_VIEW=<view>`. Screenshots in
  `docs/screenshots/round14/debug_*.png`.
- Tests: capture event sequence and counts per pass for a small scene (and a full-frame capture
  is pixel-identical to no capture), replay to N vs N+1 differ on a headless GPU, shadow events
  show greyscale depth, the last event equals the full frame, nothing recorded without a
  capture; snapshot totals equal count × struct size for a test scene and the particle capacity,
  renderer items match the primitive buffer / shadow arrays / a 64×32 target, diff and category
  diff math, WAV decoded size, working set; tree rows (grouping, folding, filter), Enable pausing
  Play, kittests of both windows (stepping with ←/→, details, capture, compare, views), the
  squarified tree map (areas proportional, no overlaps, aspect < 4) and table sorting.

## Decisions

1. **Replay re-renders the frame and skips draws after the selected event** (every event is
   still recorded, so the list does not change while stepping). Passes still begin and end so
   attachments stay valid; whole passes after the stop (bloom, glow, fog) are skipped. Reason:
   the renderer is immediate (no recorded command list to truncate), and re-rendering each
   frame keeps the editor's normal update path.
2. **The stopped event's render target is blitted into the output** with small dedicated
   pipelines (`debug_blit.wgsl`, `debug_depth.wgsl`, no shared WGSL prelude): HDR images are
   Reinhard-tonemapped, shadow maps shown as `sqrt(1 - depth)`, compute-only events show a dark
   image. Reason: the composite would mix in stale bloom from the last frame; showing the actual
   target is what a frame debugger is for.
3. **GPU culling and GPU timing are off while capturing.** GPU-culled batches are only known on
   the GPU (indirect draws), so they could not be listed or stopped individually; a replay may
   skip the pass that ends the timestamp query. Everything else (instancing, static batching,
   CPU culling) is unchanged, and a full capture is pixel-identical (tested).
4. **Capture slot is `Option<Mutex<Recorder>>` on the renderer**, hooks are free functions on
   that field (`frame_debug::record(&self.capture, ..)`), events built in closures. Reason: zero
   cost when closed (one `is_none`), works from `&self` draw code and inside `&mut self` passes
   (disjoint field borrows), and keeps `Renderer: Send + Sync`.
5. **Pass grouping is a path on each event** (`["Shadows", "Cascade 0"]`) set by the pass that
   records it; the editor tree groups consecutive events by path prefix. Sky comes before
   Opaque because the renderer draws the sky first (the reference renderer draws it after
   opaque geometry); the list shows the real order.
6. **Batch reasons are derived per list at capture time** from how many batches share a mesh /
   material state (pipeline key + textures): our batch key is mesh + shader variant + texture
   set, so "different material" is worded as "different shader variant or textures".
7. **Object / material names travel as a separate label list** (`CaptureRequest::labels`, from
   `DrawCache::labels`) rather than new `MeshDraw` fields. Reason: `MeshDraw` is built in ~20
   places across crates; labels are only needed while debugging.
8. **The capture runs only while the window is enabled and on screen** (drawn this or the
   previous UI pass); closing or hiding the tab disables it (and resumes Play). Both analysis
   windows dock beside the Project window so the Game view stays visible.
9. **Memory sizes are estimates from formats and dimensions**: textures by block size over the
   full mip chain, view targets from their attachments (105 B/pixel + bloom chain), components
   as count × `size_of` (no heap data), particles by allocated capacity, WAV clips at decoded
   stereo f32 (frames × 8 B), compressed clips at 10× file size. The process working set comes
   from `K32GetProcessMemoryInfo` via a two-function `extern` block (no new dependency;
   `None` off Windows).
10. **Snapshot items are matched by (category, type, name)** for Compare; duplicates are summed.
11. `Tab::ON_DEMAND` grew from 12 to 14 (FrameDebugger, MemoryProfiler).

## Left / ideas

- Per-draw events inside GPU-culled and Visual Effect indirect draws (one event each today).
- Showing the depth buffer / emission target of a main-pass event (only colour is shown).
- Terrain draws are one event per terrain (not per chunk).
- Memory: CPU-side asset caches (decoded model data, audio buffers held by the backend) are
  not walked; Lua heap only exists in Play mode.
