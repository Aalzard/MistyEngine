# Misty Virtual Geometry (branch vgeo)

Virtualized geometry for opaque static meshes: a dense mesh is cut into small clusters of
triangles with a hierarchy of simplified clusters, and the GPU picks the level of detail per
cluster, so a scene of film-density rocks draws about as many triangles as it has pixels. This
is the geometry half of the "our own Lumen-like GI, faster at the same quality" plan: the GI
side wants cheap, dense, pixel-sized geometry and a visibility buffer to shade from.

This document is the design (what the full feature looks like) and the status of the prototype
on this branch (what exists, what was measured, what is left).

## What it is for, and what not

**Helps**: dense static opaque meshes — scanned or sculpted rocks, cliffs, ruins, statues,
props, kitbashed architecture — many instances of them, seen near and far. LOD switching goes
away (no popping, no artist-made LOD chain, no per-object LOD budget), small or distant parts of
a large mesh cost almost nothing, and occlusion culling works per cluster, not per object.

**Does not help** (keeps the existing paths):

- **Alpha-tested foliage and grass.** A cluster hierarchy simplifies surfaces; it cannot
  simplify a cloud of thin cards (simplifying leaves removes coverage, and alpha testing defeats
  depth-based occlusion and a visibility buffer's single opaque sample). Grass stays on its
  instanced blade path, trees on their instanced LODs and impostors (meadow work).
- Skinned, morphing or vertex-animated meshes (the hierarchy is built offline from rest
  positions). World-position-offset style wind is the same problem.
- Transparent and Fade materials, double-sided thin surfaces (cull-off), custom `.shader`
  materials (their vertex stage is theirs).
- Low-poly meshes: under a few thousand triangles there is nothing to cluster; the ordinary
  instanced path is as fast.
- Meshes that are mostly large flat faces (a box room): a cluster per 128 triangles of a wall
  that two triangles describe is waste.

## Workflow (Unity-like)

- **Model import settings ▸ Virtual Geometry** (checkbox, next to Generate Lightmap UVs; stored
  in the model's `.meta` as `"virtual_geometry": true`). Turning it on builds the clusters at
  import, like Nanite's per-mesh "enable" checkbox. Skinned models are never built.
- **Quality ▸ Virtual Geometry** (per Quality level, off by default while it is a prototype) and
  **Virtual Geometry Error** (pixels, default 1). With it off, or on hardware that can't run it,
  virtual meshes draw as ordinary meshes, so a project never breaks.
- Nothing changes in the scene: a Mesh Filter + Mesh Renderer with a virtual mesh is drawn by
  the cluster path when its material qualifies (Standard lit, Opaque, single-sided) and by the
  ordinary path otherwise. LOD Groups on a virtual mesh still work (a fading level uses the
  ordinary path for its dither); they become unnecessary.
- Virtual meshes stay out of static batching (merging would copy the clusters per object).
- The import Inspector shows the build: clusters, levels, source triangles and build time
  ("from Library/VirtualGeometry" when it came from the cache; "building clusters (n of m
  meshes done)…" while the worker runs).
- Planned: "Clusters" and "Triangles" Scene view debug views (the prototype has the cluster
  colours as a renderer switch: `Renderer::set_virtual_geometry_debug`); Lua
  `QualitySettings.virtualGeometry`.

## Offline build (`crates/misty_engine/src/vgeo/`)

### Clusters (`cluster.rs`)

At most **128 triangles** per cluster (`MAX_TRIANGLES`): 384 vertex invocations on the GPU, a
good size for a visibility-buffer id, small enough that culling is precise. Greedy growth on
edge adjacency (welded positions, so UV seams don't split clusters): a seed (next in Morton
order, or a triangle left on the previous cluster's frontier) grows by the frontier triangle
that closes a hole, else the one nearest the cluster's centre, so clusters stay connected and
round. Growth stops at 120 triangles; the scraps a greedy growth strands between full clusters
are then handed, triangle by triangle, to neighbours with room, and small clusters that fit are
merged into a neighbour. Clusters are at least 80 % full on average on the test rocks (asserted
by the tests; greedy growth alone left 71 %, with stranded 1–20-triangle scraps).

### Hierarchy (`dag.rs`, `simplify.rs`)

Repeat until one cluster is left or a level keeps more than 95 % of its triangles:

1. **Group** neighbouring clusters, about 4 per group (`GROUP_SIZE`), by shared border edges
   (greedy from Morton-ordered seeds; a lone cluster joins the neighbour group it shares most
   edges with). The full design wants a real graph partitioner (METIS-style, balanced, minimum
   cut) — fewer locked border edges means better simplification, see "Left to do".
2. **Simplify** each group's triangles to half with quadric error metrics, **locking the group's
   border** (every position on an edge used once in the group, or by more than two triangles)
   and UV/normal seams. Half-edge collapses only: a vertex moves onto a neighbour, so every
   level indexes the *source* vertex buffer and no vertex data is added. Collapses are rejected
   when they would fold a triangle, break the link condition (non-manifold result), or join two
   border vertices that are not already joined in the group (they may already share an edge in
   the neighbouring group, which would then have four triangles — the only crack source found
   by the tests).
3. **Split** the simplified group into new clusters (same builder).

The result is a DAG, not a tree: a group's new clusters replace all of its old ones at once, and
the next level regroups across the old group borders, so borders don't stay locked forever
(which would leave long seams of fine triangles).

### Error metric and seamless selection

Every cluster stores two (sphere, error) pairs:

- **own**: the group that made it (level 0: error 0, its own bounding sphere),
- **parent**: the group it was simplified into (roots: `ROOT_ERROR`).

Group error = max(children's errors) + the simplification's own error (square root of the largest
quadric error of an accepted collapse, in object units) — strictly larger than every child's.
Group sphere = a sphere holding every child's own sphere. Errors and spheres therefore only grow
towards the roots.

At runtime, error `e` measured from sphere `(c, r)` projects to `e · scale / max(|c − camera| − r, ε)`
pixels (`scale` = screen height / (2 tan(fov/2)); orthographic: `e · pixels per unit`). A cluster
is drawn when **own ≤ threshold < parent**. Along any path from a source triangle to a root the
projected values never decrease (bigger error, and a containing sphere is never farther), and a
cluster's parent value is *exactly* (same inputs, same code) the own value of every cluster its
group made. So for every camera exactly one cluster on each path passes — one level covers each
spot, no overlap and no hole — and since a group switches as a whole and its border never moved,
neighbours at different levels share their border edges exactly: **no cracks, no stitching**.
Tests check this directly (`vgeo/tests.rs`): every group's border is identical before and after
simplification, and cuts for cameras at 0.5 m–1 km and 0.5–4 px are watertight (every directed
edge matched by its reverse exactly once) on a closed 82 k-triangle rock.

Clusters are stored sorted by parent error, roots last: for an instance at distance `d` no
cluster whose parent error is below `threshold · d / scale` can pass, so the GPU skips all of
them with one binary search (the finest levels of far instances are never even read).

### Storage: built at import, cached on disk (`models/virtual_build.rs`, `vgeo/file.rs`)

Importing a model with Virtual Geometry on (or changing the setting, which re-imports it)
starts its build: each mesh's content hash (FNV-1a of every position and index) is looked up in
`Library/VirtualGeometry/<guid>.vgeo`; matching meshes are read synchronously (a few
milliseconds), the rest are built on a worker thread, meshes in parallel (rayon), and the file
is rewritten when they land. Until a mesh's build is ready it draws through the ordinary mesh
path, so neither an import nor a scene load stalls a frame; the renderer looks again when a
build finishes (`virtual_build::ready_generation`). The file is little-endian binary with a
magic and a format version (`file::VERSION`, bumped whenever the layout or the builder's output
changes); another version, a damaged file or another hash means a rebuild. Player builds bake
every packed model's file (waiting for its worker if needed) and pack it under the same
`Library/VirtualGeometry/<guid>.vgeo` key, so players read the cache from the `.pak`; a player
without it (or with a stale one) builds on its worker exactly like the editor. The source mesh
is still packed too (the fallback path and physics read it).

GPU pool layout (`render/vgeo/mod.rs`), one buffer with 256-byte aligned sections:

| section | per | bytes |
|---|---|---|
| vertices | source vertex | 32: position 3 × f32, normal and tangent octahedral 2 × snorm16 each (bitangent sign in the tangent's lowest bit), uv 2 × f32, lightmap uv 2 × unorm16 (was 56) |
| local indices | cluster corner | 2 (u16; the index buffer of the indexed path, read as storage elsewhere; was 4 + a 4-byte global index) |
| cluster vertices | cluster vertex | 4 (source vertex id) |
| clusters | cluster | 80 (spheres, errors, lists, normal cone) |

Positions stay f32: a cluster group's 16-bit quantization would save another 6 bytes per
vertex, but the depth prepass and shadow passes must reproduce the main pass's positions
exactly, and f32 keeps every path bit-identical to the mesh path (the GPU test checks it).
8-bit local indices are not possible for the hardware index buffer (WebGPU's smallest index
format is 16-bit); the visibility resolve and the pulled path read the same u16 list.

## Runtime (`crates/misty_engine/src/render/vgeo/`)

### Pool and instances

All virtual meshes share one pool (vertices, cluster triangles, cluster records, mesh records),
so the draw needs no per-mesh state. Each frame, the draws of virtual meshes (lit opaque
single-sided) leave the CPU lists and become instances (draw slot, mesh, batch, casts-shadow
flag); a batch is one material texture set (group 2), everything else per object comes from the
existing `draws[]` buffer through the slot, so transforms, colours, material values, lightmap
scale-offset and motion vectors work unchanged.

### Culling and LOD selection on the GPU (`vgeo_cull.wgsl`)

Per view, four small compute passes:

| pass | threads | does |
|---|---|---|
| `cs_instances` | one per instance | frustum (sphere); Hi-Z in the second phase; binary search for the first candidate cluster; queue chunks of 64 candidates |
| `cs_prepare` | one | write each batch's draw arguments (its region is a fixed share of the view's list) and the cluster dispatch size |
| `cs_clusters` | one workgroup per chunk | LOD test (own ≤ T < parent), frustum, normal cone and Hi-Z test of the cluster sphere, append (draw, cluster) to the batch's list and write the cluster's indexed draw arguments |
| `cs_finish` | one per batch | clamp each batch's count to its region |

Views: the main camera in **two phases** — instances visible last frame (frustum + LOD only),
then after the opaque depth of phase one is turned into the Hi-Z pyramid, every instance and
cluster not drawn yet against it (this is `render/gpu_culling`'s pyramid and occlusion test,
shared: when GPU culling is on both use the same pyramid; when it is off virtual geometry builds
it anyway). Then up to 6 **shadow maps** (frustum only; level of detail = the coarser of the
main camera's and one texel of the cascade, so far casters are coarse and near cascades don't
rasterize sub-texel detail).

**Normal cones**: each cluster stores the cone of its triangles' face normals (axis and cutoff,
the meshoptimizer formulation with the bounding sphere); a cluster whose every triangle faces
away from the view's eye — the camera, or for orthographic shadow views the light direction —
is dropped before the Hi-Z test and before drawing. Only under a uniform, unmirrored transform
(the cone isn't preserved otherwise). Bumpy rocks spread their normals widely, so this catches
about an eighth of all clusters in the unit test's mix of eyes and levels; smoother props catch
more.

Budgets: 256 k visible clusters (33 M triangles) and 128 k chunks (8 M candidate clusters) per
view; overflow is counted (`VgeoStats::dropped`) and clusters are dropped rather than
corrupting memory. wgpu's default limit of 8 storage buffers per shader stage shaped the
bindings: counters, limits and draw arguments share one buffer, the visible lists and the
chunks another; the cluster dispatch size is copied out of the counters because a buffer can't
be bound for writing and be the indirect buffer of the same dispatch.

### Drawing (prototype: hardware rasterization)

Two paths, chosen by device features:

- **Indexed** (adapters with `MULTI_DRAW_INDIRECT_COUNT`, i.e. desktop Vulkan / DX12): every
  cluster has its own vertex list (source vertex ids) and local indices (an index buffer). The
  culling pass writes one `DrawIndexedIndirectArgs` per visible cluster (first index = its local
  indices, base vertex = visible entry × 384, first instance = the entry), and each batch is one
  `multi_draw_indexed_indirect_count` whose count is the batch's cluster count. The vertex
  shader (`vs_cluster_indexed`) turns `vertex_index − instance × 384` into the cluster-local
  vertex and fetches it; the post-transform vertex cache works within each cluster.
- **Pulled** (fallback): one `draw_indirect` per batch, 384 vertices per visible cluster, the
  index read from storage; corners past a cluster's triangle count collapse onto one vertex.
  No vertex reuse: measured about twice as slow (Measurements).

Both write the same outputs as the mesh shader's `vs_main`, so the fragment stage is the
ordinary `fs_main` — every lighting feature (cascades, soft shadows, probes, lightmaps, SSGI
inputs, fog) applies unchanged. The same fetch feeds the shadow maps (`vs_cluster_depth*`) and
the depth + normal prepass of AO and contact shadows (`vs_cluster_prepass*`, first-phase
clusters). Each batch owns a fixed, equal share of a view's visible list (the indexed draw needs
its region's offset on the CPU); many materials therefore shrink each batch's budget — a
per-frame GPU-side compaction into one list per material is the production answer.

### Visibility buffer and material resolve (implemented: `render/vgeo/vis.rs`)

Forward shading of pixel-sized triangles wastes most of its work: the hardware shades 2 × 2
quads, so a 1-pixel triangle costs up to 4 fragment invocations, each running the full lit
shader with every light, shadow and probe lookup. What the branch does, per main-pass phase:

1. The phase's ordinary opaque draws, then the main pass is ended.
2. **Visibility raster** (`vgeo_vis.wgsl`, own render pass): the phase's clusters, same
   indexed sub-draws and the main pass's MSAA depth (test and write), write two ids per sample
   into two multisampled R32Float targets — `0x60000000 | cluster << 7 | triangle` and
   `0x40000000 | batch << 20 | draw` (raw bits; 0 = none). The triangle comes from
   `@builtin(primitive_index)` (`PRIMITIVE_INDEX`, requested when the adapter has it). The
   fragment stage is two stores.
3. The main pass resumes (loading everything) and the **resolve** (`vgeo_resolve.wgsl`) draws
   a full-screen triangle twice per batch: layer 0 shades the object of the pixel's first
   covered sample and writes the samples showing it (`sample_mask`), layer 1 the other covered
   samples (the next object found). The resolve rebuilds the triangle at the pixel centre from
   the pool (one storage binding: the lit fragment stage already uses 7 of the 8 WebGPU
   guarantees), with perspective-correct barycentrics and their screen derivatives, so
   textures get explicit gradients (`standard_surface_grad`, mesh.wgsl's Standard inputs with
   `textureSampleGrad` and the same mip bias), and calls the same `shade` as `fs_main`. It
   writes colour, glow and motion vectors; depth is already in place. Everything drawn after
   (terrain, grass, trees, transparents) tests against it as before.

Shading once per *object* per pixel instead of once per triangle per quad keeps the look of
MSAA: edges between objects keep their coverage, and inside one surface neighbouring triangles
share their shading (the surface is continuous across them). Three or more objects in one pixel
give the third the second's shading. The GPU test renders the rock through the visibility
buffer against the mesh path: mean difference 0.07 / 255, 0.02 % of the pixels off by more
than the threshold (the lit cluster path is bit-identical).

**Id format, shared with the grass visibility buffer** (branch `grass-vis`,
grass-study.md B4): both use raw bits in R32Float (core WebGPU can't multisample integer
targets) with bit 30 always set and bit 28 clear, so no id is denormal, infinite or NaN; bit 29
tells virtual geometry (set) from grass (clear: grass packs terrain, page and blade index
below bit 27). A merged resolve could therefore read one id target for both; the two branches
keep their own targets for now (grass resolves in compute with its own lighting variant, after
the MSAA resolve). Sharing the targets (66 MB at 1080p 4× each) is a follow-up once both land.

Limits and fallbacks: the resolve adds two sampled textures and one storage buffer to the lit
fragment stage, so device creation now asks for the adapter's per-stage limits
(`render::device_limits`, capped at 32 sampled textures / 16 storage buffers; desktop Vulkan,
DX12 and Metal all have them). Without `PRIMITIVE_INDEX` or the indexed path, with more than
512 batches, in the debug view, or if the device refuses the pipelines (validation scope),
clusters draw lit as before. `Renderer::set_virtual_geometry_visibility(false)` forces the lit
path for A/B measurements.

The original plan, for reference (a compute resolve with tile classification):

1. **Visibility pass**: clusters write a 32-bit id per pixel — 25 bits visible-list entry
   (cluster instance) + 7 bits triangle — into an `R32Uint` target with depth (hardware path),
   or via 64-bit atomics (software path, below). Phase one / Hi-Z / phase two as now. The pixel
   shader is trivial, so quad overshading costs almost nothing.
2. **Material classification**: a compute pass bins 8 × 8 tiles by the material(s) they contain
   (material id = the batch of the instance). Each material then runs one full-screen-ish pass
   (or indirect dispatch over its tiles).
3. **Resolve**: per pixel, fetch the three vertices of the triangle, compute barycentrics
   analytically from the pixel's ray (and their screen derivatives for texture LOD — no hardware
   derivatives in a compute resolve), interpolate attributes, and run the existing `shade`
   function. Output into the HDR target and the velocity target, then the forward passes
   (terrain, grass, trees, transparents) draw on top with the visibility pass's depth.

What changed from that plan: the engine renders MSAA, so the ids are per sample in the main
pass's own depth (no depth copy, edges against forward geometry stay anti-aliased), and the
resolve is a fragment pass inside the main pass (it can write the multisampled targets with a
sample mask and reuse the lit bind groups as they are). Per-material tile classification is not
done yet: each batch's resolve covers the screen and discards other pixels after reading the
ids (cheap per pixel, but it scales with materials × pixels — "Left to do"). This is also the
input the planned GI wants (a G-buffer-like view of the surfaces, built once).

### Software rasterizer for tiny triangles (design, with an honest cost/benefit for wgpu)

Hardware rasterizers are built for triangles covering many pixels: setup and the 2 × 2 quad rule
make sub-pixel triangles expensive (Epic reported their compute rasterizer ~3× faster than the
hardware path for pixel-sized triangles). Design: in `cs_clusters`, clusters whose projected
triangles are all under ~4 px² go to a software list; one workgroup per cluster rasterizes its
triangles by bounding box / edge functions and writes `depth << 32 | id` with
`atomicMax` on a 64-bit-per-pixel storage buffer; larger clusters keep the hardware path; both
write the same visibility image.

In wgpu today this is the weakest part of the plan:

- **64-bit atomics** are an optional feature (`SHADER_INT64_ATOMIC_MIN_MAX` /
  `SHADER_INT64_ATOMIC_ALL_OPS`): Vulkan with `shaderBufferInt64Atomics` and DX12 SM 6.6 on
  recent desktop GPUs, Metal only partially, nothing on WebGPU or GL. Without them the
  fallbacks are a two-pass depth-then-id scheme with 32-bit atomics (twice the raster work) or
  packing reduced depth (e.g. 20 bits) with a 12-bit id into 32 bits plus a second payload pass —
  both eat most of the gain.
- WGSL has no wave/quad intrinsics in the baseline (subgroups are an optional feature), so
  cluster-level raster loops are less efficient than HLSL ones.
- Our scenes are forward-shaded: the software rasterizer only pays once the visibility buffer
  exists (it cannot run `fs_main`).

**Verdict**: worth it only after the visibility buffer, and only on adapters with 64-bit atomics
(detected; hardware path otherwise). Expected gain: large in dense distant geometry where the
1-pixel error threshold produces ~1-pixel triangles — exactly the situation virtual geometry
creates — and none in close-up views dominated by larger triangles. The prototype measures how
much of the frame the cluster raster takes today (pass timers, below) so the decision can be
made on numbers.

### Streaming (design)

The prototype keeps every cluster resident. For large worlds:

- **Pages**: the cluster groups of each level are packed into fixed-size pages (~128 KB) in
  spatial order; the root pages of every mesh are always resident (so something is always
  drawable).
- **Requests**: `cs_clusters` already knows when a cluster passes its own LOD test but its
  children would be wanted (own error too big *and* the finer group not resident): it writes the
  page id into a request buffer with a priority (projected error). The CPU reads it back a frame
  later (no stall), loads pages asynchronously (`.vgeo` files or the `.pak`), uploads into a GPU
  page pool, and patches a page table; the cluster test then treats non-resident children as
  "not available" and keeps drawing the parent (still crack-free: whole groups switch).
- **Eviction**: LRU over pages not requested for N frames, with a memory budget per Quality
  level.

### Shadows

- **Prototype**: cascades (and spot/point layers) are fed by clusters, culled per map, with the
  main camera's level of detail. Cheap and consistent, but distant cascades rasterize coarse
  clusters for every caster in their frustum.
- **Design**: **virtual shadow maps** — one 16 k × 16 k virtual map per directional light (clipmap
  levels), 128 × 128 pages allocated only where the depth buffer needs them (page marking from
  the visibility/depth pass), static pages cached between frames (only pages touched by moving
  objects or a moving sun are redrawn), clusters culled and LOD-selected *per page* at the
  page's texel size. This is what makes pixel-accurate shadows of pixel-sized geometry
  affordable; it needs the same request/feedback machinery as streaming, so it comes after it.

### Fallbacks

| missing | effect |
|---|---|
| `INDIRECT_FIRST_INSTANCE` (or no compute) | virtual geometry off: virtual meshes draw as ordinary meshes (full detail — a production build should also keep a coarse fallback mesh, e.g. a mid level of the DAG, for this case and for WebGPU/GL) |
| 64-bit atomics | no software rasterizer: everything on the hardware path |
| mesh shaders | not used. wgpu has experimental mesh shaders; they would replace vertex pulling (no 384-vertex padding, per-cluster culling in the task stage) where available, behind a feature check |
| `MULTI_DRAW_INDIRECT_COUNT` | the pulled-vertex path (one `draw_indirect` per batch): correct, about twice the main-pass cost |
| subgroups | the software rasterizer and prefix sums get slower, still correct |
| 8 storage buffers per stage (default limit) | already worked around (shared control buffer) |
| Frame Debugger capture | off while capturing (the debugger lists CPU draws) |

## Phase 2: terrain as virtual geometry (design, after objects work)

The user asked for this ("Yes, add terrain"). It comes after the object path is solid and must
not slow it down. The reason is **quality, not speed**: today the terrain is a smooth heightfield
(one vertex per heightmap sample, `terrain/mesh.rs`; chunks with distance LOD from shared index
patterns, `render/terrain_lod.rs`), so mountains and cliffs read as soft, stretched slopes even
with triplanar texturing (`terrain/cliffs.rs`). In MeadowDemo the terrain costs ~2.6 ms on High
(meadow-perf measurements), mostly shading, not geometry. Virtual geometry terrain should land
at about the same cost while showing real rock detail: crags, ledges, strata, overhangs.

### Which representation

Two candidates:

- **Tiled heightfield cluster hierarchy**: tile the heightmap into fixed 8 × 8-cell patches
  (128 triangles = one cluster), build the coarser levels by merging 2 × 2 tiles and
  simplifying with the tile border locked, exactly like mesh groups. Cheap to build (a tile is
  regular), cheap to rebuild under a brush, and maps 1:1 to the heightmap the rest of the
  engine reads. But a heightfield has one height per (x, z): no overhangs, and steep cliffs
  still get few, stretched triangles per metre of wall.
- **Mesh-backed terrain**: the terrain *is* an arbitrary mesh (sculpted or generated), built
  like any virtual mesh. Full freedom (caves, arches, overhangs), but every system that reads
  the heightmap (grass, trees, physics, painting, height queries in scripts) would need a mesh
  query instead.

**Choice: the tiled heightfield hierarchy as the base, plus displacement on steep tiles, plus
optional mesh "cliff pieces"** (ordinary virtual meshes placed on the terrain for overhangs).
Concretely:

1. **Base**: the heightfield tiles as above. Flat and gently sloped ground simplifies hard (a
   meadow tile at distance becomes a couple of triangles), so the cluster count stays low where
   the grass already covers the ground.
2. **Displacement on cliffs**: tiles steeper than the cliff angle get *tessellated and displaced
   offline*: the tile is subdivided (up to 8× per cell edge) and each new vertex pushed along the
   surface normal by a displacement map — the cliff layer's height texture, or procedural ridged
   noise like `vgeo::rock` — scaled by slope so flat ground is untouched. That turns a stretched
   cliff quad into real geometry that the cluster hierarchy then simplifies back down for
   distance. The displaced surface is still a single-valued surface over the tile's
   parameterisation (displacement along the normal may produce gentle overhangs, but not caves).
3. **Overhangs and caves**: placed virtual meshes (rock walls, arches) that blend into the terrain
   with the existing height-blend layer shading. This is the kitbash workflow the object path
   already supports, and it keeps the heightmap the single source of truth for everything else.

### Seamless LOD and tile borders

The terrain has the same crack rules as meshes: a tile's border vertices are locked when a
group is simplified, and groups switch as a whole, so neighbouring tiles at different levels
share their border edges exactly — no skirts, no edge-snapping variants (today's 16 edge
patterns per LOD go away). Two terrain-specific points:

- **Between terrains** (neighbouring terrain assets): their shared edge has the same samples, so
  locking the outer border of each terrain's hierarchy keeps them matched; the outermost level
  keeps the full border resolution (a long thin seam of small triangles along the edge — the
  same trade-off as Nanite's locked mesh borders; acceptable at terrain scale).
- **Displaced tiles next to flat ones**: displacement fades to zero at tile borders that touch a
  non-displaced tile (computed from the neighbour's slope class at build time), so both sides
  share the undisplaced border vertices.

### Editing: sculpting and painting keep working

- **Painting** (splat weights, `brush::paint`) changes only shading inputs: the splat map is a
  texture read by the material resolve, so no cluster rebuild at all.
- **Sculpting** (`brush::raise`, `set_height`, `smooth`): the store already reports the dirty
  region of each stroke (`store::Region`, which today drives the partial GPU upload in
  `render/terrain.rs`). The tiles under that region are rebuilt on a worker thread: level 0
  tiles directly from the heightmap (regular grid: microseconds per tile), then their ancestors
  up the hierarchy (each level only for the groups containing a dirty tile; the locked borders
  mean untouched neighbours never change). Until the rebuild lands (a frame or two), the edited
  tiles draw from the plain heightmap path (the existing chunk renderer stays as the fallback),
  so a stroke is visible immediately. Displacement is re-applied to rebuilt steep tiles.
- Cost estimate: a 20 m brush on a 1 m-spacing terrain dirties ~16 tiles; their rebuild with
  ancestors is ~50 small simplifications — well under 10 ms on a worker per stroke step.

### Grass, trees and physics

They all stay on the **heightmap**, which remains the source of truth:

- **Grass** (`terrain_grass.rs`) samples heights and the splat map; it never sees the clusters.
  Displacement is limited to steep tiles, where grass doesn't grow (slope fade), so blades don't
  float above or sink into displaced rock. Where they meet, the displacement fade at the cliff
  foot keeps the ground at heightmap height.
- **Trees** (`render/trees.rs`) stand on heightmap heights: same argument; trees on displaced
  cliffs would need a displaced-height query (the displacement is a pure function of position,
  so it can be evaluated on the CPU).
- **Physics** keeps the heightfield collider (`physics/colliders.rs`): rapier heightfields are
  fast and the displaced detail is visual-scale (centimetres to a metre). Characters walking on a
  displaced cliff would sink or float by up to the displacement amplitude; for climbable rock,
  place mesh cliff pieces with mesh colliders. Optionally later: per-tile trimesh colliders built
  from a coarse DAG level of displaced tiles only.
- **Shadows, AO prepass, GI**: the terrain clusters go through the same views as objects, so
  shadows of crags come for free; today's terrain shadow pass (`vs_terrain_depth`) is replaced.

### Honest benefit

- **Detail**: real geometric relief on cliffs and mountains at any distance, no LOD popping, no
  visible chunk rings, shadows and AO from the relief itself.
- **Cost**: geometry was never the terrain's bottleneck (shading is: 4 splat layers with
  triplanar and height blend). Clusters keep the triangle count near pixel density, so the
  geometry side stays small; with the visibility buffer the terrain's heavy shading runs once
  per pixel instead of per quad fragment, which is where a real win could come from — but only
  after phase 1's visibility buffer exists. Expect "same frame time, much more detail", not a
  big speed-up.
- **Order**: objects path solid → visibility buffer → heightfield tiles (no displacement) at
  parity with today's look → displacement on cliffs → editing rebuilds → cliff pieces.

## The prototype on this branch

- `vgeo/` (CPU): clusters, simplifier, DAG, LOD selection reference, a dense procedural rock
  (`vgeo::rock::rock`, icosphere + ridged noise) and whole-mesh LOD levels made by the same
  simplifier (the LOD Group comparison).
- `vgeo/file.rs` (the `.vgeo` format) and `models/virtual_build.rs` (builds at import on a
  worker, the `Library/VirtualGeometry` cache, `bake` for player builds; the build pipeline
  packs the files).
- `render/vgeo/` (GPU): pool, culling passes (with normal cones), indexed and pulled cluster
  pipelines for the main pass, shadow maps and the screen-space prepass, the visibility
  buffer (`vis.rs`, `vgeo_vis.wgsl`, `vgeo_resolve.wgsl`), cluster colour debug view, stats
  readback.
- Settings: `ModelImporter::virtual_geometry`, `QualityLevel::virtual_geometry` /
  `virtual_geometry_error` with their inspector and Project Settings rows,
  `models::runtime::register_virtual` for generated meshes.
- Tests: `vgeo::tests` (cluster sizes and coverage, hierarchy depth, error/sphere monotonicity,
  group borders identical after simplification, watertight cuts, seams and open borders kept,
  normal cones only cull clusters whose every triangle faces away, `.vgeo` round trip and
  rejection of damaged / other-version files), `models::virtual_build::tests` (an import
  builds on the worker and writes the cache, the next import reads it synchronously, an
  edited mesh is rebuilt), `render::vgeo::tests` (shaders validate, struct sizes),
  `render::vgeo::gpu_tests::virtual_rock_matches_the_mesh_path` (a rock through clusters at
  0.1 px matches the mesh render, shadows included — through the visibility buffer, with and
  without its edge layer, and lit directly, which is bit-identical; at 4 px fewer triangles,
  still close), and the ignored demo `vgeo_field` (shots + timings).

## Measurements

`vgeo_field` (ignored GPU test, release): 3000 rocks over 320 × 320 m (3 generated rock
shapes, 81 920 triangles each, scales 0.25–5), a ground plane and a shadowing sun, 1920 × 1080,
RTX 4060, median of 90 frames after 30. Compared paths, all on the same rocks:

- **full detail**: every rock at 82 k triangles (instanced);
- **LOD Groups**: four levels per rock made by the same simplifier (82 k, 20 k, 5 k, 1.3 k
  triangles) with the default switch heights (60 %, 30 %, 10 %, last level never culled) — an
  *ideal* LOD chain (every level well simplified, all instances of 3 meshes batched), which is
  the hardest baseline virtual geometry will ever face;
- **LOD Groups + GPU culling** (the existing two-phase Hi-Z path);
- **Virtual Geometry** at the default 1 px error, and at 2 px.

Cluster build (now at import, on the worker, three meshes in parallel): 1.4–2.0 s for the three
82 k meshes; read back from the `.vgeo` cache in milliseconds. Pool: 8.7 MB for the three
meshes (all levels).

### Round 2 (this branch now): cache, normal cones, 32-byte vertices, visibility buffer

Same scene and machine, `gpu-timing.lock` held, full detail left out (`MISTY_VGEO_FULL=0`).
The machine was still shared: rows whose path-independent passes (sky, TAA) ran inflated
were discarded, so each cell is from a clean run, and comparisons are within one run.

| Quality / view | LOD Groups | VG 1 px, lit | **VG 1 px** (visibility buffer) | VG 1 px, edge layer | VG 2 px |
|---|---|---|---|---|---|
| High / field | 2.63 (round 1) | 2.80–2.99 | **2.77** | 2.97 | **2.11** |
| High / close | **2.66** | 2.99–3.31 | 2.88–2.93 | 3.06–3.10 | **2.23–2.26** |
| High / aerial | 2.28 | **2.08** | 2.14 (two layers) | – | **1.97** |
| Medium / field | 1.98 (round 1) | 2.50 | 2.20 | 2.38 | **1.80** |
| Medium / close | 2.07 (round 1) | 2.65 | 2.30 | 2.49 | **1.93** |
| Medium / aerial | 1.85 | – | – | 2.15 | **1.76** |

GPU ms, 1920 × 1080; "lit" = clusters drawn with the lit shader directly (the round 1 path,
now with cones and the small pool). Triangles: VG 1 px 2.1–2.3 M, VG 2 px 0.8–0.9 M, LOD
Groups 7.0–7.4 M (High).

Parts at High / close (one clean run, ms):

| part | LOD Groups | VG 1 px, lit | VG 1 px | VG 1 px, edge layer | VG 2 px |
|---|---|---|---|---|---|
| shadow culling | – | 0.11 | 0.11 | 0.11 | 0.11 |
| shadow maps | 0.84 | 0.77 | 0.77 | 0.77 | 0.33 |
| main geometry | 1.03 | 0.85 | 0.42 raster + 0.27 resolve | 0.42 + 0.38 | 0.20 + 0.27 |
| second phase Hi-Z + culling | – | 0.14 | 0.15 | 0.15 | 0.15 |

### What the numbers say (round 2)

- **Normal cones and the 32-byte pool** took the lit cluster path from 3.06 to 2.80 ms at
  High / field (main geometry 0.88 → 0.73, shadows 0.79 → 0.66); smaller elsewhere.
- **The visibility buffer** saves 0.2–0.4 ms over the lit cluster path in every clean
  comparison at 1 px: the raster of 2 M pixel-sized triangles costs ~0.42 ms with a two-store
  fragment stage, and the resolve shades each covered pixel once for ~0.2–0.27 ms (a fixed
  part: it reads every sample's ids over the whole screen). The edge layer (a second shade
  where two rocks of one material meet inside a pixel) costs another ~0.11 ms, so it is off by
  default; edges against everything else keep their MSAA coverage without it. With a single
  sun and a plain material the shading is cheap here: the gain grows with shading cost (more
  lights, probes, GI) and shrinks with fewer pixels covered.
- **Against an ideal LOD chain**: at 1 px virtual geometry is now within +0.1–0.3 ms of LOD
  Groups at High (round 1: +0.4–0.8), faster from above, with 3× fewer triangles, ~40 % less CPU
  and no LOD authoring or popping; at 2 px it is 0.4–0.5 ms faster everywhere. The "small
  instanced rocks" scene still favours LOD Groups (see round 1's notes below); the dominant
  remaining costs are the shadow maps (0.77 ms at 1 px: cascades take the camera's detail
  near the viewer) and the fixed resolve.
- **CPU**: unchanged from round 1 (5–7 ms vs 9–13 ms for LOD Groups).

### Round 1 (before this round), for reference

#### Frame time (GPU ms) and triangles

| Quality / view | full detail | LOD Groups | LOD + GPU culling | Virtual Geometry 1 px | Virtual Geometry 2 px |
|---|---|---|---|---|---|
| High / field | 81 ms, 469 M | **2.63**, 7.41 M | 4.06, 7.41 M | 3.06–3.10, 2.3–2.6 M | 2.76, 0.88 M |
| High / close | 73 ms, 422 M | **2.66**, 6.99 M | 5.16, 6.99 M | 3.05, 2.1–2.3 M | **2.49**, 0.9 M |
| High / aerial | 67 ms, 389 M | 2.28, 6.07 M | 4.05, 6.07 M | **2.07**, 1.0–1.3 M | – |
| Medium / close | 41 ms, 235 M | **2.07**, 3.90 M | – | 2.85, 2.3 M | **2.07**, 0.86 M |
| Medium / aerial | 43 ms, 246 M | 1.84, 3.84 M | – | **1.66–1.74**, 1.1 M | – |

CPU time of `Renderer::render`: LOD Groups 9–13 ms, Virtual Geometry 5–8 ms (thousands of draws
→ one instance list; the LOD selection moved to the GPU).

Parts at High / field (pass timers, ms):

| part | LOD Groups | Virtual Geometry 1 px (pulled vertices) | Virtual Geometry 1 px (indexed) |
|---|---|---|---|
| shadow culling | – | 0.12 | 0.12 |
| shadow maps | 0.84 | 2.60 | 0.79 |
| main pass geometry | 1.02 | 1.80 | 0.88 |
| second phase Hi-Z + culling | – | 0.14 | 0.14 |
| **GPU frame** | **2.63** | **6.01** | **3.06** |

**Measuring caveat**: this machine was shared with other sessions' builds and GPU tests while
measuring; contaminated runs show up as 5–10× inflated sky / TAA / bloom passes (which don't
depend on the path) and were discarded. Rows marked "–" had no clean run. The numbers are
consistent across the clean runs to about ±0.1 ms.

#### What the numbers say

- **The first version lost** (High / field 6.0 ms vs 2.6): pulling 384 vertices per cluster
  from storage buffers with a non-indexed draw gets no post-transform vertex reuse (≈3
  invocations per triangle against ≈0.6 for an indexed mesh), and the shadow maps used the
  camera's 1-pixel detail for every caster. Two changes fixed most of it:
  1. **indexed cluster draws**: each cluster has its own vertex list and local indices; the
     culling pass writes one `DrawIndexedIndirectArgs` per visible cluster (base vertex =
     entry × 384) and each batch is one `multi_draw_indexed_indirect_count` with the count
     from the GPU (`MULTI_DRAW_INDIRECT_COUNT`, now requested when the adapter has it; the
     pulled path remains as the fallback): main-pass geometry 1.80 → 0.88 ms;
  2. **shadow detail per cascade**: a cascade takes the coarser of the camera's level of
     detail and one of its own texels (the minimum of two projections that both grow towards
     the roots, so cuts stay crack-free): shadows 2.60 → 0.79 ms.
- **Now**: at 1 px virtual geometry is at parity with an ideal LOD chain (−0.2 to +0.8 ms),
  with 3–7× fewer triangles, ~40 % less CPU, no LOD authoring and no popping; at 2 px it is
  equal or faster in every clean measurement. From above (many small, distant rocks) it wins.
- **Where it should win more**: unique dense meshes (kitbash, scanned cliffs) where nobody
  authors LOD chains, and large meshes partly near and partly far (one LOD Group level covers
  the whole object; clusters refine only the near part) — this demo of small instanced rocks
  favours LOD Groups. And the next steps attack the remaining cost directly: the visibility
  buffer (quad overshading of the small triangles), per-cluster backface culling (about half of
  every closed rock's clusters face away), a persistent GPU instance buffer.
- **LOD Groups + GPU culling** is slower than plain LOD Groups here (as in the meadow/stress
  measurements): with few, cheap, mostly visible batches the extra passes cost more than they
  save. Virtual geometry's own culling passes cost 0.2–0.3 ms per frame in total.

### Visual quality

The field and aerial shots are indistinguishable between the paths (`vgeo_field_*`,
`vgeo_aerial_*`). Close up (`vgeo_close_lodgroup.png` vs `vgeo_close_virtual.png`) the near
rock looks **smoother** with virtual geometry at 1 px: its geometric error is under a pixel, but
the vertices simplified away carried high-frequency *normal* detail, which per-vertex shading
loses. Nanite-style content relies on normal maps for this; the fix here is either a normal map
baked from the source onto each level, or normal deviation in the simplification error
(attribute-aware quadrics, "Left to do" 3), or a lower threshold for the nearest clusters.

### Screenshots (`docs/screenshots/gfx/`)

Retaken in round 2: the `*_virtual` shots are drawn through the visibility buffer (single
layer) and look the same as round 1's lit clusters.

- `vgeo_field_lodgroup.png`, `vgeo_field_virtual.png`, `vgeo_field_clusters.png`: standing in the
  field (High).
- `vgeo_close_*`: next to a group of rocks; the cluster view shows small clusters on the near
  rock and coarse ones behind.
- `vgeo_aerial_*`: the whole field from above.

## Checks

Round 2 (release profile, CARGO_INCREMENTAL=0): `cargo fmt`, `cargo clippy --release --workspace
--all-targets` (0 warnings), `cargo test --workspace --release`: engine 888 passed, editor 561
passed, player 9 passed. Failing: `foliage_lighting_demo_matches_its_generator` (known, on main
too).

Round 1: engine 860, editor 542, player 9 passed, the same known failure.

## Left to do before production

Honest list, roughly in the order it should happen (done in this round: build at import with
the `.vgeo` cache, the visibility buffer and material resolve, normal cones, the smaller pool):

1. **Visibility resolve cost**: the resolve is a full-screen pass per material batch that
   reads every sample's ids before discarding; with many virtual materials that scales with
   materials × pixels. Needed: per-tile material classification (a compute pass bins 8 × 8
   tiles by the batches they contain, then each batch resolves only its tiles, indirect),
   and sharing the id targets with the grass visibility buffer once both are merged (same id
   rule; 66 MB at 1080p 4× each today). The resolve is worth more the heavier the shading
   (many lights, probes, GI): in the rock field with one sun its gain is small (Measurements).
2. **Better partitioning.** Greedy cluster growth and greedy grouping work but leave more
   locked border edges than a balanced minimum-cut partitioner (METIS-style); that is what
   limits how far groups simplify (stuck groups) and how full clusters are.
3. **Attribute-aware simplification**: UV and normal error in the quadrics, and collapses along
   seams instead of locking every seam vertex (meshes with many UV islands simplify poorly
   today), and normal deviation in the error so close-up shading keeps its detail (see Visual
   quality). Per-group 16-bit position quantization would take vertices from 32 to 24 bytes
   if the depth-matching passes all decode the same way.
4. **Software rasterizer** for tiny triangles, only on adapters with 64-bit atomics (see the
   cost/benefit above). The visibility raster's id format is what it would write.
5. **Streaming**: pages, GPU request feedback, async loads, eviction, a memory budget per
   Quality level; pool memory reported in the Profiler's Memory module (it isn't yet;
   `VgeoStats::pool_bytes` has it).
6. **Shadows**: virtual shadow maps with cached static pages. Today: cascades fed by clusters
   with the camera's / a texel's level of detail and normal cones from the light, no
   occlusion culling in shadow views, and only the first 6 shadow maps get virtual casters.
7. **Culling refinements**: cluster-level occlusion for the instances drawn in the first
   phase (only instance-level today); a persistent GPU instance buffer updated incrementally
   (instances are re-uploaded from the CPU draw list every frame — fine to ~100 k instances,
   not to millions); normal cones under non-uniform scale (skipped today).
8. **Materials and batches**: each material batch owns a fixed equal share of a view's
   visible list (scenes with many virtual materials need GPU-side compaction per material);
   only Standard lit Opaque single-sided surfaces take the cluster path. Cutout on clusters
   needs an alpha test in the visibility raster; multi-material meshes need a per-cluster
   material id (the id format has room in the batch field).
9. **Fallback mesh**: without the GPU features a virtual mesh draws its full source mesh; a
   production build should keep a coarse fallback (a mid level of the DAG) for those devices,
   and could then stop packing the full source mesh (physics would read a level of the DAG).
10. **Tools and stats**: the Game view Stats don't count cluster triangles (GPU-side counts
    are read back only on request), the Frame Debugger doesn't list cluster draws (virtual
    geometry is off while capturing), the Scene view has no Clusters / Triangles debug views
    yet, no Lua. Rebuilding a model's `.vgeo` by hand (Reimport) works through the import
    settings; there is no "Clear Virtual Geometry cache" menu item.
11. **Robustness**: the crack-free guarantee relies on the GPU computing a cluster's own error
    and its children's parent error identically (same inputs, same function); a compiler
    contracting the two call sites differently could give a one-frame overlap or hole exactly
    at a threshold crossing. Computing the decision once per group (stored per group, read by
    both) removes the dependency. Non-uniform scale uses the largest axis scale (conservative).
