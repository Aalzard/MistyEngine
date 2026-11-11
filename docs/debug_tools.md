# Debug tools: Frame Debugger and Memory Profiler

Both windows live under **Window ▸ Analysis**, next to the Profiler, and dock beside
Asset Files so the Scene Viewer stays visible.

## Frame Debugger

Shows how the Scene Viewer's frame is built, one render event at a time.

1. Open **Window ▸ Analysis ▸ Frame Debugger** and press **Enable**. Play mode pauses (it
   resumes when you press **Disable**); in Edit mode the Scene Viewer simply shows the same frame.
2. The left side lists the frame's events grouped by pass, in the order they run:
   `Shadows ▸ Cascade N` / `Local Light Layer N`, `Ambient Occlusion`, `Depth Prepass`,
   `Clouds`, `Volumetric Fog`, `Sky`, `Opaque`, `Terrain`, `Scene Copy`, `Water`,
   `Transparent`, `Particles`, `Visual Effects`, `Gizmos`, `Halos & Flares`, `Sun Shafts`,
   `Post ▸ Bloom` / `Tonemap` / `Anti-aliasing / Sharpen`, and `UI`. Only passes that ran appear.
   Each group shows its event count; click the arrow to fold it. The search field filters
   events by name, object, mesh or material.
3. Select an event (click, the slider, ◀ ▶, or **←/→** while the pointer is over the window).
   The Scene Viewer replays the frame **up to that event** and shows the render target it wrote:
   the HDR scene colour (tonemapped) for main-pass draws, the depth of the shadow map layer for
   shadow casters, the bloom mip for bloom steps, the final image for the last events. Compute
   passes have no colour target and show a dark image.
4. The right side shows the event's details: object, mesh, material, submesh, vertex / index /
   instance counts, **batching** ("GPU instanced: N objects…", "Static batch…", or why it is
   not batched — different shader variant or textures, different mesh, transparent ordering,
   instancing off), the shader / pipeline, render target with size and format, blend / depth /
   cull state, bound textures with thumbnails, and the material's values.

Notes:

- While capturing, **GPU culling** and **GPU timing** are off, so every draw is a CPU-side
  event and a replay can stop anywhere. Everything else (instancing, static batching in Play
  mode, culling) is as normal; a full-frame capture is pixel-identical to an uncaptured frame.
- The capture costs nothing when the window is closed or disabled: the renderer's hooks check an
  empty `Option` and return.
- Engine API: `Renderer::begin_capture(CaptureRequest { stop_after, labels })`, render, then
  `Renderer::end_capture() -> Option<FrameCapture>`. Labels (object + material names per draw)
  come from `DrawCache::labels`.

## Memory Profiler

Press **Capture** to take a snapshot. Each snapshot lists items with an estimated size:

| Category | What is counted |
|---|---|
| Textures | Loaded material/particle/terrain textures (full mip chain, by format), reflection probe atlas, HDRI sky cubemaps |
| Meshes | Built-in primitives, imported meshes, combined static batches, skinned poses, terrain meshes and layers |
| Render Targets | Editor Scene / Scene Viewer targets (MSAA colour, emission, depth, resolves, bloom chain), shadow cascades and local shadow layers, fog and cloud volumes |
| Buffers | Per-draw values, draw slots, lines, uniforms, LTC table |
| Audio | Clips used by Audio Sources at their decoded size (WAV: frames × 8 bytes; compressed formats ≈ 10× file size) |
| Scripts | The Lua heap (Play mode) |
| Scene Objects | Components per type: count × struct size (heap data such as strings is not counted) |
| Particles / VFX | Live particle storage per system, particle instance buffer, Visual Effect buffers |
| Lightmaps | The baked lightmap texture array |

Views:

- **Summary** — totals per category as bars, the tracked total and the process **working set**
  (Windows; from the OS).
- **Table** — every item; click a column header (Name, Type, Category, Size, Refs) to sort, the
  search field filters. *Refs* counts users where known (material texture sets for textures,
  instances for components, Audio Sources for clips).
- **Tree Map** — squarified rectangles per category and item, sized by bytes; hover for details.
- **Compare** — the selected snapshot against another (the previous one by default): totals per
  category before / after / change, then items sorted by the size of their change (new, gone,
  grew, shrank).

Sizes are estimates from known formats and dimensions; driver overhead, alignment and CPU-side
copies are not included, which is why the working set is larger than the tracked total.
