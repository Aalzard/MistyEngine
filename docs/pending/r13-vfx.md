# Round 13: Visual Effects (GPU particles + Effect Graph)

## Decisions

1. **Names.** The component is **Visual Effect**, the asset an **Effect Graph** (`.effect`, JSON,
   GUID table via `asset_db::embed_json`, `.meta` like every asset), the window **Effect Graph**.
   Behaviour follows the reference engine's GPU effect graphs; no code, assets or names copied.
2. **One system per graph, contexts in a fixed flow.** A graph holds one Spawn, one Initialize
   Particle, at most one Update Particle and one or more Output Particle contexts; the flow
   (Spawn → Initialize → Update → Outputs) is implied by the context types and drawn as arrows,
   not wired by hand. Extra Spawn/Initialize/Update contexts are compile errors on the extra
   context. Reason: covers every effect in the task with far less UI and validation than
   free-form system wiring; several systems = several Visual Effects.
3. **Fixed 64-byte particle layout** (position, age, velocity, lifetime, linear colour + alpha,
   size, angle, seed, alive). Reason: one struct for every graph keeps the compiler, readbacks
   and the renderer simple; 1M particles = 64 MB. Attributes beyond these are left for later.
4. **Spawn runs on the CPU** (constant rate with carried fractions, single burst, periodic
   burst), like the reference engine. Spawn inputs accept constants, properties and + − × ÷ of
   them (evaluated on the CPU); per-particle operators are refused when linking and are a compile
   error on the block if forced. Deterministic for a given dt sequence (tested).
5. **GPU data flow** (`render/vfx`, kernels in `vfx/codegen.rs`): per instance a particle buffer,
   a dead-index list with an atomic count, two alive-index lists swapped every step, atomic
   counters (dead, alive×2, spawned, reaped) and an indirect buffer. A step: `cs_reset` (after
   Reinit), `cs_emit` (pops dead indices — underflow is detected with an `i32` atomic and undone —
   and appends to the current alive list), `cs_args` (writes the indirect dispatch), `cs_update`
   (`dispatch_workgroups_indirect`, only live particles; reaped indices go back on the dead list,
   survivors to the other list), `cs_finish` (writes one `draw_indirect` argument block per
   output). Drawing reads the survivors' list in the vertex shader; the CPU never touches
   particles. Newly emitted particles are updated in their first step (dt included).
6. **No sorting** (decision on the optional camera-distance sort): alpha-blended outputs draw in
   alive-list order. Reason: the samples are additive (order independent); a bitonic sort per
   frame of up to 1M keys costs more than the whole simulation and is left as a follow-up.
7. **Simulation once per step, only when visible.** The CPU instance bumps a step counter; the
   renderer simulates an instance when a view that sees its bounds renders with a new counter, so
   the Scene and Game views don't double-step and culled effects pause (the reference engine's
   default culling). Bounds are the Initialize context's box in the object's space.
8. **`Renderer::render_with_effects`** (new entry point) instead of a new `Frame` field. Reason:
   ~20 test files build `Frame` literals; parallel branches touch them too. `GameView` gained an
   `effects` list; the editor views and the player call the new method.
9. **Random numbers** are PCG hashes of (seed, step, emit index) at birth and (particle seed,
   step) in update, so the same seed gives the same particles whatever slots they land in (the
   atomics make slot order nondeterministic; the test compares sorted particle sets).
10. **Colours** are sRGB (+ HDR above 1) in files, fields and properties, converted to linear for
    the GPU; gradients and curves are baked into 64-sample ramp rows per instance (gradient
    properties follow the instance's overrides).
11. **Over Life blocks**: in Update they overwrite the attribute (absolute value); in Output they
    multiply what is drawn without changing the stored particle — the reference engine's usual
    way to fade and shrink.
12. **Operators are inlined per block**: each block's inputs emit their operator chain inside the
    block's scope (memoised within the block), so attribute reads see the values earlier blocks
    wrote. Dynamic math ports resolve to the widest connected input (float → vector3 → colour);
    floats splat, colours truncate, vectors pad alpha 1. Gradient/Curve values only feed ramp
    inputs.
13. **Compile errors** carry the context, block or operator id: structure checks first, then
    generated-line → node mapping for naga errors; output shaders are validated against the
    renderer's view prelude (`render::vfx::validate_output`).
14. **Live editing**: the window compiles its working copy into the effect library in place of
    the asset (`library::set_working_copy`), so every Visual Effect using it shows unsaved edits
    in the Scene view; Save Asset writes the file and reloads it. Unchanged content keeps the
    library generation, so particles don't restart on every redraw.
15. **Edit mode**: all active Visual Effects simulate in the Scene view (the reference engine's
    GPU effects play in edit mode); the editor only keeps repainting continuously while an effect
    is selected or used by the open Effect Graph.
16. **Soft particles** (Quad outputs) fade against the opaque scene's depth copy (the scene copy
    the water uses), requested only when a drawn output wants it; otherwise a far 1×1 texture is
    bound.
17. **Window-local undo/redo** with grouped drags/typing, like the Shader Graph window; the
    canvas reuses the Shader Graph's view mapping, grid, wires and card style.

## Progress

- Engine `vfx`: `graph.rs` (model, editing, links/types), `blocks.rs` (Spawn: constant rate,
  single/periodic burst; Initialize: set attribute (+random, overwrite/add/multiply), position
  sphere/box/cone/circle (volume/surface), velocity from direction; Update: gravity, linear drag,
  turbulence (curl noise), conform to sphere, vortex, collide plane/sphere (inside), kill sphere,
  colour/size over life; Output: orient (face camera/along velocity/face up), over life,
  attributes), `operators.rs` (property, constants, time, get attribute, math, vector, random per
  evaluation/particle, noise, curl noise, sample gradient/curve), `codegen.rs`, `spawn.rs`,
  `library.rs` (cache, hot reload, working copies), `component.rs` (Visual Effect + instance),
  tests. Output kinds: Quad, Point, Line; blends: Additive, Alpha, Opaque; texture (asset or
  Texture property) with flipbook (over life / fps); soft particles.
- Renderer `render/vfx` (pipelines per effect hash, instance buffers, indirect dispatch/draw,
  async counter readback for `aliveCount`, culling, GC), GPU tests.
- Lua `VisualEffect`: Play, Stop, Reinit, SendEvent("OnPlay"/"OnStop"), SetFloat/SetVector3/
  SetColor, GetFloat, HasProperty, ResetOverride, aliveCount, isPlaying, seed (+ Code Editor docs).
- Editor: Effect Graph window (`panels/effect_graph`: canvas, Add Block / Create Node search,
  Blackboard, inline fields and gradient/curve popups, error badges, undo/redo, save, play
  controls for the previewed effects), Visual Effect Inspector (asset + Edit, seed, overrides,
  Play/Stop/Reinit, live count), bounds gizmo, GameObject ▸ Effects ▸ Visual Effect, Assets ▸
  Create ▸ Visual Effects ▸ Effect Graph, double-click opens, Window ▸ Rendering ▸ Effect Graph,
  `MISTY_EFFECTGRAPH=<asset>` capture hook, edit-mode preview (`vfx_preview.rs`).
- Samples: `Assets/Effects/{Fireflies, Sparks Fountain, Magic Portal, Million Particles}.effect`,
  `Assets/Scenes/VfxDemo.scene`, `PortalPulse.lua` (`sample_vfx.rs` generator + match, compile,
  script and headless render tests).

## Left / ideas

- No camera-distance sorting for alpha outputs; no mesh output or mesh-surface spawn; no depth
  buffer collision.
- Custom attributes, events beyond OnPlay/OnStop, GPU events, multiple systems per graph,
  sub-graphs, sticky notes in the canvas, copy/paste between graphs.
- Bounds are manual (no "record bounds" button).
- Colour fields in blocks edit LDR colour (HDR intensity kept when only the hue changes); the
  Blackboard and Inspector have an intensity field.
