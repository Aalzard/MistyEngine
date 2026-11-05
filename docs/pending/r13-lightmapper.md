# Round 13 — Lightmapper (baked global illumination)

## Progress

Baked GI like the reference engine's progressive lightmapper:

- **Lightmap UVs** (`misty_engine::lightmap::uv`): a second UV set on every vertex
  (`Vertex::uv2`, vertex attribute 4). `unwrap` grows planar charts by flood fill across shared
  edges (matched by position) while face normals stay within ~45° of the chart's first triangle,
  projects each chart onto its plane, turns it wider than tall and shelf-packs the charts into
  [0, 1]² with a binary search on the scale, `MARGIN` (1/32) between charts and half of it at the
  border. Plane, Quad and Sphere keep their (already non-overlapping) UVs inset by half the
  margin; Cube, Capsule and Cylinder are unwrapped. Imported models: **Generate Lightmap UVs**
  import setting (Inspector on the model asset, `.meta`, on by default, re-imports when changed);
  a glTF `TEXCOORD_1` is read when generation is off.
- **Light Mode** (Realtime / Mixed / Baked) on the Light component (serde default Realtime, left
  out of files). **Mesh Renderer ▸ Lighting**: Contribute Global Illumination, Receive Global
  Illumination (Lightmaps / Light Probes), Scale In Lightmap, the baked lightmap shown read-only.
  Static objects with Contribute GI are baked.
- **Lighting window ▸ Scene**: Mixed Lighting (Baked Global Illumination, Lighting Mode = Baked
  Indirect), Lightmapping Settings (Samples, Bounces 0–4, Lightmap Resolution in texels per
  unit, Padding, Max Lightmap Size, Indirect Intensity), **Generate Lighting** (background bake,
  progress bar with texel count, Cancel) and **Clear Baked Data**. Settings are per scene
  (`RenderSettings::lightmapping`), undoable.
- **Baker** (`lightmap::bake`): renderers get squares sized for the texel density (world area
  over uv2 area × resolution × Scale In Lightmap), packed into the smallest power-of-two
  lightmap that holds them (or several of Max Lightmap Size) with Padding texels between them.
  Texels are rasterised from uv2 triangles (world position + normal), then a multi-threaded
  (rayon) path tracer over a BVH of the static triangles computes per sample: direct light of
  Baked lights (shadow rays; point / spot sources jittered over their shadow radius, rectangle
  lights sampled over their area) and one cosine-weighted path — sky on a miss, else emission +
  albedo × (direct light of Baked and Mixed lights + next bounce). The indirect part is denoised
  by an edge-aware À-trous blur (position and normal guided), direct + indirect × Indirect
  Intensity, and empty texels around charts are dilated (4 + padding texels).
- **Output**: `Assets/Scenes/<Scene>/Lightmap-N.lightmap` (HDR, f16 RGB planes + zlib, header
  `MLMP`), listed in the scene's lightmapping settings; each renderer stores
  `lightmap_index` + `lightmap_scale_offset` in the scene file. Files are loaded through the VFS
  (pak in player builds) and referenced as asset paths, so builds pack them.
- **Renderer**: lightmaps are an rgba16f texture array (mesh group bindings 25–26); the per-draw
  uniform grew to 256 bytes (`lightmap_st`, `lightmap`). Lightmapped surfaces take their diffuse
  ambient / bounce light from the lightmap instead of the realtime ambient and the reflection
  probes' diffuse SH (specular reflections stay). Once a scene has lightmaps, Baked lights stop
  lighting in realtime; Mixed lights keep their realtime direct light and shadows. Static
  batching folds each renderer's scale-offset into the merged uv2, so renderers sharing a
  lightmap still merge.
- **Scene view draw modes** "Baked Lightmap" (lightmap only, others dark grey) and "UV Charts"
  (lightmap texel checkerboard tinted per lightmap).
- **Lightmap asset Inspector**: size and a tonemapped preview.
- **LightmapDemo** (`sample_lightmap.rs`): Cornell-box room (red / green walls, white floor,
  ceiling, back wall, two blocks), a Baked rectangle light in the ceiling, an emissive orange
  panel, a Mixed point light and a dynamic sphere lit in realtime; its 6 texels-per-unit
  lightmap is committed.

## Decisions

1. **CPU path tracer, not GPU.** The bake runs on a background thread while the editor keeps
   rendering: a CPU bake never competes with the editor's wgpu queue or risks a driver timeout
   on long dispatches, it reuses the probe placement BVH (now reporting the triangle index), it
   is deterministic (per-texel seeded PCG) and testable without a GPU, and rayon uses every
   core. Scenes this engine targets bake in seconds at the default 10 texels per unit. A wgpu
   compute port can reuse the same inputs (`BakeInput`) later.
2. **uv2 lives in the vertex** (56-byte `Vertex`, attribute 4) rather than a side buffer: every
   mesh pipeline already binds one vertex buffer, skinning copies it through, static batching
   transforms it, and other passes ignore the attribute.
3. **Lightmap values are irradiance in the realtime light units**: `albedo × texel` is the
   diffuse light, the same convention as realtime lights (`albedo × colour × N·L × falloff`,
   no 1/π) and the ambient. Baked direct light uses the realtime point / spot falloff and the
   rectangle lights' window, and the sky is the realtime ambient gradient, so a light switched
   from Realtime to Baked looks the same.
4. **Baked lights stay realtime until the scene is baked**, so a scene is lit while it is being
   set up; after Generate Lighting they only live in the lightmaps (dynamic objects then rely on
   ambient / light probes, like the reference engine).
5. **Per-renderer data in the scene file**, the lightmap list in the scene's lighting settings
   (`RenderSettings::lightmapping`); defaults are left out of files so old scenes are unchanged.
6. **Mesh-level uv2 with a fixed margin** (1/32 of the uv square): charts keep their relative
   sizes, renderers get squares in the atlas, the bake's dilation fills chart gaps. Small
   renderers have a minimum of 8 texels per side.
7. **Mixed Lighting Mode is Baked Indirect only**; Subtractive / Shadowmask need per-light
   flags in the light uniform and shadow-mask textures and are left for later.
8. **Lightmaps share one size** per bake (one texture array); up to 32 lightmaps per scene.
9. **Model import default: Generate Lightmap UVs on** (a model can't know whether its objects
   will be static); skinned meshes and meshes with blend shapes keep their own uv2 since their
   vertices are indexed by the skin.

## Tests

- uv2: primitive charts inside [0, 1] and at least the margin apart; cube = 6 equal planar
  charts; a bumpy grid unwraps into many non-overlapping charts.
- atlas: squares never overlap and keep padding; side follows texel density.
- bake: a point light over a plane matches the realtime falloff (within 6 %); the padding ring
  is dilated; a red wall tints the floor (r/g > 1.1 × the white-wall case); an emissive panel
  lights the floor under it and the sky reaches open areas; cancel stops the bake.
- files: lightmap round trip at half precision; scene round trip of lightmap index /
  scale-offset / GI settings (defaults left out).
- LightmapDemo: matches its generator; the committed lightmap bleeds red / green onto the
  floor; headless render is lit, not flat, differs from the same frame without lightmaps and
  shows a reddish floor by the red wall.

## Left for later

- GPU (compute) baker; Subtractive and Shadowmask mixed modes; directional lightmaps.
- Light probes for dynamic objects (parallel branch) — Baked lights don't reach dynamic objects
  until then.
- Lightmap compression (BC6H), per-renderer chart packing at bake time (exact texel padding).
