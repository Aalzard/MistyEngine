# Round 20C: deeper VFX (branch r20-vfx)

Goal: a lighter take on the GPU effect graphs of the big engines (Niagara-style event handlers,
sub-graphs, ribbons) on our Effect Graph, plus the Shuriken-style modules our CPU Particle
System was missing. Behaviour follows the familiar engines; code and formats are our own.

## GPU Effect Graph

| Feature | How it works | Where |
|---|---|---|
| Back-to-front sorting | Output setting **Sort: Auto / On / Off** (Auto = Alpha Blend outputs). A bitonic sort of keys + particle indices on the GPU (`render/vfx/sort.wgsl`): `cs_sort_args` turns the alive count into indirect dispatches (padded to a power of two, at least one 512-element block), `cs_sort_init` writes inverted distance keys, `cs_sort_local` sorts 512-element blocks in workgroup memory, then each larger stage runs global compare-exchange steps for the long distances and finishes in workgroup memory. Stages above the alive count get zero workgroups, so the cost follows the live particles, not the capacity. Sorted every view (it follows the camera); the output draws from the sorted list. | `render/vfx/mod.rs` (`SortGpu`) |
| Mesh output | Output kind **Mesh** with a built-in primitive (cube, sphere, capsule, cylinder, plane, quad). The triangle list is a storage buffer read by vertex index; the particle's Size scales it, **Orient** chooses tumbling around a random axis by Angle, along the velocity, or upright. Lit by the sun with a flat ambient term; Opaque meshes write depth. | `vfx/codegen.rs` (`render_vertex`) |
| Depth buffer collision | Update block **Collide with Depth Buffer** (Bounce, Friction, Lifetime Loss, Surface Thickness). The simulation reads the last opaque scene depth a view copied (the scene copy water and soft particles use) with that view's camera; a particle behind the surface but less than the thickness deep (along the surface normal, from neighbouring depth texels) is put back on the surface and its velocity reflected. The renderer asks for the scene copy while such an effect is drawn. One frame of latency. | `SIM_HELPERS` (`vfx_depth_collide`) |
| GPU events | **GPU Event System** (canvas menu, or Create Node ▸ GPU Event System): a GPU Event context (On Die / On Collide, Count per event, parent system) starting a child system with its own Initialize, Update and Outputs. The parent's update appends event records (position, size, velocity, lifetime) to a queue in the instance's events buffer; the child's `cs_event_args` makes the indirect emit dispatch and `cs_emit` spawns Count particles per record. Initialize blocks **Inherit Source Velocity / Size**; shape blocks are relative to the event position. Impacts slower than 1 m/s don't count as collisions (resting contact). | `vfx/codegen.rs` (`SimConfig`), `render/vfx` |
| Sub-graphs | `.vfxoperator` assets (Assets ▸ Create ▸ Visual Effects ▸ Effect Sub-graph): operators, Blackboard properties as the node's inputs and **Output** nodes as its outputs. The node appears under Create Node ▸ Sub-graph. Graphs are flattened before compiling (`vfx/subgraph.rs`): the sub-graph's operators are copied with fresh ids, its property reads replaced by what feeds the node (or constants of the inline values), links from the node's outputs start at what feeds the Output nodes. Errors inside land on the node; sub-graphs may nest (8 deep, loops refused); editing one recompiles the effects using it. | `vfx/subgraph.rs` |
| Trails and ribbons | **Trail** output: every particle keeps a ring of its last N positions (2–32, one per step, filled at birth), drawn as a camera-facing tapering strip. **Ribbon** output: one strip through all particles youngest first (an age sort; births are spread over the step so no two particles share an age). | `vfx/codegen.rs`, `render/vfx` |

Several systems in one graph: each system compiles to its own simulation shader and buffers
(particles, dead list, two alive lists, counters, indirect arguments, trail ring); they step in
index order, parents first, in one encoder. Old `.effect` files load unchanged (new fields have
defaults; old Alpha Blend outputs now sort).

## CPU Particle System modules

Every number is a **MinMax** curve (Constant, Curve, Random Between Two Constants, Random
Between Two Curves; the ▾ menu in the Inspector) and every start colour a **MinMaxColor**
(Color, Gradient, Random Between Two Colors / Gradients). Start values and Rate over Time follow
their curves over the system's cycle; module curves over each particle's life. Per-particle
random picks come from a per-particle seed.

| Module | What it does |
|---|---|
| Velocity over Lifetime | x / y / z curves added to the motion (not stored), Local / World, speed modifier. |
| Limit Velocity over Lifetime | Speed limit curve, Dampen (fraction of the excess removed per 1/30 s), Drag. |
| Force over Lifetime | x / y / z acceleration curves, Local / World. |
| Color over Lifetime | Gradient or random between two gradients. |
| Color by Speed, Size by Speed, Rotation by Speed | Gradient / curve over a speed range. |
| Size over Lifetime | A curve (older files keep their start → end line). |
| Rotation over Lifetime | Angular velocity curve (degrees / s); Start Rotation in Main. |
| Noise | Fractal gradient noise (1–4 octaves, multiplier, scale), strength curve, frequency, scroll speed, damping, position / rotation / size amounts. |
| Collision | Planes (world point + normal, one-sided) or World (the scene's box, sphere and capsule colliders, mesh colliders of primitives as their boxes; layer mask); dampen, bounce, lifetime loss, min / max kill speed, radius scale. |
| Sub Emitters | Birth / Collision / Death triggers emitting `count` particles from a child Particle System (by name) where the particle was; probability, inherit colour / size / velocity. A system used as a sub emitter doesn't emit by itself. |
| Texture Sheet Animation | Tiles, Whole Sheet / Single Row (fixed or random row), frame over time curve, start frame, cycles. |
| Trails | Particles mode (ratio, lifetime as a fraction of the particle's, minimum vertex distance, world space, die with particles) or Ribbon mode (all particles in birth order); width over trail, colour over lifetime / trail, inherit particle colour, own texture. |
| Renderer | Billboard, Stretched (speed / length scale), Horizontal, Vertical. |

New modules are left out of scene files while they hold their defaults, so older scenes keep
their exact text (the ParticlesDemo generator test still matches).

## Sample: VfxDemo2

`Assets/Scenes/VfxDemo2.scene` (generator `crates/misty_editor/src/sample_vfx2.rs`):
Fireworks (trails, death events into trailing sparks), Rain Splash (depth collision, collide
events, sorted splashes), Debris (lit mesh cubes), Ribbon (driven by the `Orbit` sub-graph),
Smoke (sorted flipbook), and the Particle Systems Embers (noise, trails, death sub emitter),
Sparks Shower (world collision on the floor and a crate, collision sub emitter, stretched
billboards) and Flipbook Smoke (texture sheet, curves). The smoke sheet is generated by the
generator (our own procedural texture, CC0).

## Cost: 100k GPU particles, sorted, with depth collision

Measured with `cargo test --release -p misty_engine --lib vfx_cost_report -- --ignored --nocapture`
(GPU lock held; 1920×1080, this machine's GPU, 99 166 particles alive at a steady state of
50 000 / s × 2 s, falling onto a floor in view). "Sim + sort" is GPU time from timestamps
around the simulation and sort passes; "frame" is a whole render, submitted and waited for.

| Case | Sim + sort (GPU) | Frame | Over the floor alone (2.35 ms) |
|---|---|---|---|
| 100k additive, no sort, no collision | 0.175 ms | 3.31 ms | +0.96 ms |
| 100k alpha, GPU sort every frame | 0.511 ms | 3.13 ms | +0.78 ms |
| 100k additive + depth collision | 0.176 ms | 4.00 ms | +1.65 ms |
| **100k alpha + sort + depth collision** | **0.501 ms** | **4.47 ms** | **+2.12 ms** |

The sort of 100k keys (a 131 072-key bitonic sort, 46 dispatches) costs about 0.34 ms of GPU
time; the depth test itself is free (≈ 0.002 ms), but depth collision needs the opaque depth
copy, which the frame pays for (≈ 0.7 ms at 1080p with MSAA). Frame timings are wall clock and
vary by ±0.2 ms between runs (the alpha case drawing faster than additive is within that).

## Tests

- `vfx::tests`: sort keys per output, mesh / trail vertex counts and trail kernels, depth
  collision flag, GPU Event kernels and queues, structure errors of systems, JSON round trip
  and round-13 files, sub-graph flattening and error mapping.
- `render::vfx::gpu_tests`: back-to-front order from the GPU sort (and a new order for another
  camera), ribbons youngest first with distinct ages, trail rings one step apart, mesh cubes
  drawn, death events (5 per death) and collision events (2 per impact) spawning children,
  particles resting on a floor through depth collision (and falling through without it),
  `vfx_cost_report` (ignored).
- `particles::tests`: curve / gradient modes, cycle curves, velocity / force / limit, noise,
  plane and world collisions, scene colliders, texture sheet frames, trails and ribbons,
  rotation / size / colour modules, sub emitters on birth / death / collision, render modes,
  older scenes keep their text.
- `sample_vfx2`: matches its generator, effects compile with their features, particle systems
  play (sub emitters, collisions), renders on a GPU.

## Not done (left for later)

- Mesh outputs of imported models (built-in primitives only); no per-particle mesh lighting
  beyond sun + flat ambient.
- Depth collision only sees what the last view saw (off-screen particles don't collide) and
  lags a frame; collision events have a fixed 1 m/s impact threshold.
- Sub-graph blocks (only operator sub-graphs); GPU events carry position / size / velocity
  (no colour) and only On Die / On Collide (no rate-over-time trigger events).
- Ribbons: one strip per system (no strip ids); trails have no texture modes besides stretch.
- CPU: no Inherit Velocity, Lights, External Forces or Custom Data modules, no Shape module
  additions (circle, edge, mesh), smooth (Hermite) curves are piecewise linear, World
  collisions use primitive bounds for mesh colliders and test the end position only (fast
  particles can tunnel through thin colliders). Collision Quality / Visualize options are absent.
