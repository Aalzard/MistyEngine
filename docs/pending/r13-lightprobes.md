# Round 13: Light Probes (pending merge notes)

## Progress

Light probes: baked indirect light for moving objects.

- **Light Probe Group** component (Rendering ▸ Light Probe Group, GameObject ▸ Light ▸ Light Probe
  Group): local probe positions, default 8 on a 2 m cube.
- **Scene view editing** (Inspector ▸ Edit Light Probes): click / Shift / Ctrl select, drag probes
  or the Move handle, Delete removes, Ctrl+D duplicates in place, Add Probe (view centre), Select
  All, Delete / Duplicate Selected, selected probe position field, **Fill Box** (grid with a
  spacing over a box). Each action is one undo step. Tetrahedra wireframe while editing; probes
  drawn as spheres (yellow before baking, shaded by their baked SH after).
- **Bake** (Lighting ▸ Scene ▸ Light Probes ▸ Bake Light Probes, Bounces 1/2, Cancel, progress
  bar): captures a 32 px cube per probe with the reflection probe capture renderer over the static
  scene, projects onto L2 SH, saves `Assets/Scenes/<Scene>/LightProbes.probes` (JSON) and stores
  the path in the scene's Lighting settings (`lighting.light_probes`). Runs a few probes per
  editor frame within a 30 ms budget. Clear Baked Light Probes removes it.
- **Runtime**: own 3D Delaunay (Bowyer–Watson) over all probes; each flagged mesh draw walks from
  its last tetrahedron to the one containing its bounds centre, blends the four probes'
  irradiance SH barycentrically (outside the hull: closest point of the nearest hull face) and the
  shader uses it instead of the ambient light. Mesh Renderer ▸ Light Probes: Off / Blend Probes.
- **Lua**: `LightProbes.GetInterpolatedProbe(position)` → `{coefficients, color, baked,
  Evaluate(direction)}` (ambient light when nothing is baked).
- **LightProbesDemo** sample (red / blue walls, 126-probe grid, swinging sphere with a script,
  a Light Probes Off sphere for comparison), baked file committed.
- Tests: SH projection vs analytic coefficients, irradiance evaluation, Delaunay validity
  (positive volumes, symmetric adjacency, empty circumspheres, convex hull, coverage without
  overlap on random points and grids), exact interpolation at probes, linear reproduction,
  outside-hull fallback, few-probe fallback, stale-hint walk, file round-trip, Lua API, editor
  editing/undo through the Scene panel, bake preconditions, headless render (sphere by the red
  wall redder than by the blue wall), sample generator match + baked tint.

## Decisions

1. **Per-draw SH rides in the reflection-probe list buffer (binding 23)** after the draws'
   probe lists (an index per draw, then 9 coefficient blocks as f32 bits), with the draw count in
   the free `probes.header.w`. Reason: no new binding (fragment storage-buffer count is close to
   the default limit of 8) and nothing uploaded when no draw uses probes.
2. **The flag is `MaterialParams::light_probes`, not a `MeshDraw` field.** `MeshDraw` is built by
   literal in ~45 places (many tests of parallel branches); `MaterialParams` already carries
   per-object overrides (`uv_rotation`) and has `Default`. The draw cache sets it for non-static
   renderers with Blend Probes and non-unlit materials.
3. **Static objects never blend probes** (they stand for lightmapped geometry); the second bounce
   bakes again with the statics lit by the first result, which is how probes get two bounces.
4. **Probes store indirect light only**: lights stay realtime, so the captured sky leaves out the
   sun disk (otherwise direct sunlight would be counted twice). Ambient intensity does not scale
   probe light (it is already in what the probes saw).
5. **Light-probe objects keep the probe diffuse inside reflection-probe boxes**; reflection probes
   then only add their reflections.
6. **Sampling point = world bounds centre**, like other engines; one SH per draw.
7. **Delaunay jitter**: grids are full of cospherical points; a 1e-5 deterministic jitter breaks
   ties, topology is built in f64 on the jittered points, weights use the real positions (jittered
   ones for slivers that are flat in reality), so interpolation is exact at probes. Enclosing
   tetrahedron 1e4 × the layout size (1e6 lost points to f64 precision; smaller sizes leave the
   hull visibly short of convex). A reflex-edge filling pass was tried and dropped: on grid hulls it
   made overlapping slivers.
8. **File is JSON** (positions + radiance SH): small (a few KB per 100 probes) and diffable; the
   field (tetrahedralization, convolved SH) is built at load and cached per file (`store`).
9. **Gizmo spheres are unlit + light_probes**: `fs_unlit` multiplies by the probe SH when a draw
   has one, so the spheres show exactly the baked SH without direct light; the draw cache never
   flags unlit user materials, so nothing else changes.
10. Lighting window: own small section after the reflection probes (one added line in
    `scene_page`) so the lightmapper's edits there don't conflict. No Light `mode` field was
    needed.

## Left / follow-ups

- Skinned Mesh Renderers, terrain and particles don't use probes yet (ambient as before).
- No "Use Proxy Volume" (LPPV) option; no anchor override.
- Gizmo shows interpolated SH at moved probes until re-baked (no "stale bake" warning).
