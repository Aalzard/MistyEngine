# Round 18D: asset pipeline

Branch `r18-assets`. FBX import, Unity-like model import settings, Generate LODs, and GPU
texture compression (BC1, BC3, BC5, BC7, BC6H) cached in `Library/TextureCache` and packed in
builds. Unity's behaviour is the model; no Unity code.

## Libraries

- **FBX: `ufbx` 0.11.5** (the official Rust binding of ufbx 0.23, MIT or public domain). ufbx is
  a single C file built with `cc` (the workspace already builds C through `mlua`'s vendored Lua,
  so Windows MSVC and the Linux CI job need nothing new). It reads binary and ASCII FBX 6.x–7.x,
  meshes with any polygon size, materials (FBX classic and the PBR models of Maya, 3ds Max,
  Blender), skins, blend shapes, animation stacks and layers, and does unit and axis conversion,
  geometric pivots and inherit-mode compensation for us. Its author maintains both the C library
  and the binding; it is fuzzed and tested against a large corpus of real exporter files.
  The pure-Rust crates (`fbxcel`, `fbx`, `fbx_direct`, …) are DOM readers: skin clusters, pivots,
  pre/post rotations, animation curves and their baking would all be ours to write.
- **BC encoders: `block_compression` 0.11** (MIT, a pure-Rust port of the Intel ISPC texture
  compressor kernels). CPU encoders and decoders for BC1–BC5, BC6H and BC7 (its optional wgpu
  compute path is left out). Pure Rust, so every target cross-compiles; it tracks wgpu 30 like we
  do. `texpresso` has BC1–BC5 only; `intel_tex_2` ships prebuilt ISPC libraries (x86 only).

## FBX import (`models/fbx_import.rs`)

- ufbx converts the scene to glTF's space (right-handed, Y up; metres with Convert Units),
  bakes unit scale and geometric pivots into the geometry (`ModifyGeometry`) and compensates
  inherit modes. From there the file follows the glTF path: X mirrored into engine space, one
  mesh per material part (like glTF primitives, built together by Virtual Geometry), the node
  hierarchy, skins (one per skin deformer, inverse binds from the clusters' geometry-to-bone
  matrices, root = the joint without a joint parent), blend shapes (channel names, default
  weights, position and normal offsets per control point) and every animation stack baked by
  ufbx and resampled at 30 fps into a clip (rotations as continuous Euler curves, DeformPercent
  as blend shape curves).
- Z-up files keep their conversion on the root objects (Unity does the same: a -90° X rotation
  on the model's roots); the skin and animation are consistent with it.
- Materials: base colour (PBR or Diffuse × factor), opacity, metalness, roughness → smoothness,
  emission; albedo and normal (or bump) textures. Embedded textures (FBX video content) are
  extracted as PNG; file textures are looked up by relative path, next to the file, in a
  `Textures` folder beside or above it, then by absolute path; images outside the project are
  read so extraction copies them in.
- FBX models are read-only prefabs with Skinned Mesh Renderers and an Animation, exactly like
  `.glb` ones (`model_import.rs` is shared).

## Model import settings (`models/settings.rs`, `models/process.rs`, Inspector `model_settings.rs`)

Inspector sections like Unity's Model Importer, with Revert / Apply:

- **Scene:** Scale Factor (meshes, blend shapes, node positions, bind poses and Position
  curves scale together), Convert Units (FBX), Axis Conversion (Automatic from the file, Y Up,
  Z Up: the roots turn -90° about X, their animation curves follow).
- **Meshes:** Read/Write (stored for parity; Misty keeps meshes readable on the CPU either way:
  physics, picking and baking read them), Generate Lightmap UVs (existed), Virtual Geometry
  (existed), Generate LODs with a level table (triangle share and screen height per level).
- **Geometry:** Normals (Import / Calculate with Smoothing Angle — angle-weighted, vertices split
  at sharper edges, skins and blend shapes follow — / None), Tangents (Calculate / Import: the
  file's directions with the engine's bitangent side / None).
- **Materials:** Location (folder) and **Extract Materials…** (folder picker, extracts now;
  existing `.mat` files are never overwritten).
- **Animation:** the file's takes, clips cut by frame range (name, take, start, end, loop) and a
  preview (skeleton and skinned vertices posed by a clip, scrubber, Play). Applying new clips
  rewrites the extracted `.anim` files.
- New fields stay out of the `.meta` while at their defaults: existing model metas are unchanged.

## Generate LODs

Per level after LOD 0, each static mesh is simplified with the vgeo quadric edge-collapse
simplifier (`vgeo::rock::simplified`: open borders and UV seams locked, so no cracks between
material parts). Instances get one child object per level (`Hull_LOD0`, `Hull_LOD1`, …) and a
LOD Group on the root with the levels' screen heights. Defaults: 100% / 50% / 20% at
50% / 20% / 3% screen height. Skinned and morphing meshes are left alone.

## GPU texture compression (`sprites/compress.rs`)

- Texture import setting **Compression**: None / Normal Quality / High Quality (unset = the
  type's default: Normal Quality for 3D textures and normal maps, None for sprites), plus
  **Platform Overrides** (Windows, Linux, macOS: Max Size and Compression).
- Formats: Normal Quality → BC1 (opaque) or BC3 (any alpha below 255); High Quality → BC7;
  normal maps (Texture Type Normal Map, or any texture in a material's normal slot) → BC5 (the
  standard shader and the custom-shader `unpack_normal` rebuild Z from X and Y); BC6H for HDR
  images (`encode_hdr`; no HDR texture type uses it yet).
- Mips are made as before (sRGB-aware box filter), every level padded to whole blocks.
  Textures whose size is not a multiple of 4 stay RGBA8.
- Cache: `Library/TextureCache/<key>.mtc`, key = SHA-256 of the source file's bytes and
  everything that changes the blocks (compression, use, size after Max Size and the quality
  level, mipmaps, encoder version). A hit skips decoding the PNG/JPG too.
- Upload: when the device has `TEXTURE_COMPRESSION_BC` (now requested with the other optional
  features) and the settings compress, blocks are uploaded; otherwise RGBA8 as before.
  Editor: blocks are made on a worker, the texture draws uncompressed until they are ready and
  then reloads silently. Players: cache only, never encode. Tests: encode on the spot.
- Builds: `build_pipeline/package.rs` bakes every packed texture for the target platform at each
  size the project's quality levels load it at, for the uses its materials make of it, and packs
  the `.mtc` entries; players read them through the pak like the editor reads `Library`.
- The Memory Profiler counts texture bytes by format now (block sizes for BC).

## Tree, grass and terrain textures (`sprites/compress/generated.rs`, branch tex-compress2)

The renderer's own textures go through the same blocks now: tree maps and impostors
(`render/trees.rs`), the grass atlas with its flower layers (`render/terrain_grass.rs`) and the
terrain layer arrays (`render/terrain_layers.rs`). They are packed from several images (spare
channels, atlas cells, array slices) and have coverage-keeping mips, so they are not file
textures:

- **Formats by role**, from the Compression setting of the role's main source file (an array
  takes None when any slice's file says None, High Quality when any says it; the built-in
  grass atlas and the impostors count as Normal Quality):

  | Role | Textures | Normal Quality | High Quality |
  |---|---|---|---|
  | Opaque colour | terrain layer albedo (alpha unused) | BC1 | BC7 |
  | Colour + alpha | bark albedo + height, leaf atlas, impostor albedo, grass atlas, flower albedo | BC7 | BC7 |
  | Four data channels | bark / leaf normal (x, y, roughness, occlusion / translucency), impostor normal (xyz, AO), terrain normal + height, flower material | BC7 | BC7 |

- Why: BC5 holds two channels, so a normal map with roughness and occlusion packed in would
  need BC5 + BC4 (two textures, 1.5 bytes a texel, new bindings and shader changes); BC7 keeps
  the four channels at 1 byte a texel and its normal error is 0.5–2.2° (BC3's 5:6:5 colour
  endpoints: 1.4–3.9°). For colour + alpha BC7 has 4–10 dB better colour than BC3 at the same
  size, with the alpha as good. The grass atlas and the flower layers share one array, so one
  format.
- **Cutouts**: the mips stay the coverage-keeping ones (made before encoding, each level
  encoded as it is). BC7 keeps each level's alpha-tested coverage within 0.42 % on levels of
  64 texels and more (0.08–0.42 %; up to 1.95 % on the 16–32 texel levels, a few texels).
  It rounds edge alpha up by about 0.6/255 on average, which alpha to coverage turns into
  slightly denser grass; cutout levels whose edges come out more than 0.2/255 off are encoded
  again with the bias taken out (dithered).
- **Quality gate**: blocks whose worst channel is under 30 dB PSNR are not used, the array
  stays RGBA8 (a `<key>.raw` marker in the cache remembers it). MeadowDemo's meadow thatch
  normal + height map is noise to 4×4 blocks (BC7: 15° mean normal error, height 22 dB; even
  BC5 is 6°) and moved the golden `meadow_low_close` shot over its tolerance (mean 0.62); with
  the gate the terrain's normal + height array stays RGBA8 and every other texture is
  compressed (each keeps 38 dB or more in every channel).
- **Cache**: `Library/TextureCache/<key>.mtc` like file textures; the key hashes the packed
  level-0 pixels with the recipe (how the mips are made), format, sRGB, cutout and the encoder
  settings. A hit skips the mips (the slow part) and the encode; textures shared between tree
  assets (the oaks' bark and leaves) share entries.
- **Encoder**: BC7 with the ISPC kernels' "alpha very fast" settings, within 0.1–1 dB of
  "alpha basic" at a third to a sixth of the time (`made_textures_bc7_settings`): all of
  MeadowDemo's made textures encode in about 12 s on the rayon pool on a first load (about
  40 s with "alpha basic"); later loads are cache hits.
- **Editor**: misses are encoded on the rayon pool while the textures draw RGBA8; the owner
  (tree, layer cache, flower atlas, built-in atlas) builds them again once the blocks are in
  the cache (`generated::waiting`), the old ones drawing meanwhile. **Players** read the cache
  only. **Tests** encode on the spot. Without `TEXTURE_COMPRESSION_BC` everything stays RGBA8.
- **Builds**: `pack_compressed_textures` also calls `render::bake_made_textures` for the packed
  `.tree` and `.terrain` files (trees are built as a player builds them, the impostor bake is
  deterministic) and packs those entries.
- The Memory module counts these textures under Textures now (they were in the terrain's
  buffer bytes); `Renderer::made_texture_bytes` gives them per category.

## Measurements

MeadowDemo, every texture the scene uses, at the default quality level (Windows, Normal
Quality defaults; `sprites/compress/meadow.rs`, ignored test):

| Textures | Count | RGBA8 + mips | Compressed | Saved | Path |
|---|---|---|---|---|---|
| Rocks and other material textures | 18 | 125.8 MB | 21.0 MB | 83% | compressed (round 18D) |
| Trees (bark, leaves) | 27 | 151.0 MB | 21.0 MB | 86% | source files; see below |
| Grass and flowers | 16 | 22.4 MB | 3.5 MB | 84% | source files; see below |
| Terrain layers and other | 6 | 21.0 MB | 2.6 MB | 87% | source files; see below |

- Material textures (the rocks' albedo BC1, masks BC3, normals BC5) save **104.8 MB** of GPU
  memory in MeadowDemo. The table's other rows are the source files; what the GPU holds of
  them is the renderer's packed textures, measured on the GPU below.

Renderer-made textures on the GPU (MeadowDemo, High, every tree, the terrain and the flowers
loaded; `made_textures_memory`, ignored test):

| Textures | RGBA8 + mips | Compressed | Saved |
|---|---|---|---|
| Trees (5 assets × bark, bark normal, leaves, leaf normal, impostor, impostor normal) | 139.8 MB | 35.0 MB | 75% |
| Terrain layers (albedo array BC1; normal + height array kept RGBA8 by the quality gate) | 44.7 MB | 25.2 MB | 44% |
| Grass atlas and flower layers (two 3-slice arrays) | 33.6 MB | 8.4 MB | 75% |
| **Total** | **218.1 MB** | **68.5 MB** | **69%** |

- All GPU texture memory in the Memory module: 403.3 → 253.7 MB.
- GPU frame time (`made_textures_times`: 1920×1080, High, uncompressed and compressed runs
  alternating, three each, medians of 60 frames after 60): wide 16.00 → 15.51 ms (−0.50),
  behind 17.28 → 16.59 ms (−0.70), oak 14.66 → 14.16 ms (−0.49). Less texture bandwidth in
  the passes that sample trees, grass and terrain: 3–4 % of the frame. (main before this
  branch, `meadow_perf_times`: wide 16.01 ms, behind 17.25 ms, the uncompressed numbers.)
- Golden check (`golden_shots`, all 20 shots, RTX 4060 reference): every shot within tolerance,
  references **not** updated. Meadow shots mean 0.15–0.39 (wide 0.19–0.26, close 0.28–0.39,
  behind 0.28–0.34, oak 0.15–0.20), changed pixels ≤ 0.03 %; CyclesRoomDemo 0.00. Before the
  quality gate the terrain's normal + height array was BC7 too and `meadow_low_close` drifted
  (mean 0.62 > 0.5): leaving out the grass atlas brought it to 0.40, the trees changed nothing,
  BC7 instead of BC1 for the terrain albedo changed nothing, leaving out the terrain normal +
  height array brought it to 0.39. Hence the gate.
- Close-ups (`docs/screenshots/gfx/texcomp_*.png`: uncompressed | compressed | difference ×8,
  High, 1600×900 telephoto views from the look-check spots, 400×300 crops at 2×): no visible
  difference in any. Mean absolute difference of the crops: bark 0.16, leaves 0.14, flowers
  0.74, terrain cliff 1.30 (BC1 albedo; the largest single difference 7/255, a smooth shift),
  terrain ground 1.13, meadow 0.83. Where the differences are larger they are single-pixel
  sparkle on blade and petal edges (alpha to coverage and TAA settling on slightly different
  samples), not blocks or colour shifts.
- Quality (PSNR of level 0 against the image): rock albedo BC1 37–43 dB, masks BC3 50–53 dB,
  rock normals BC5 40–54 dB; tree and flower colour maps 33–46 dB. Encoding all 67 textures
  takes about 2 s (rayon, release); later loads are cache hits.
- Golden check (`golden_shots`, all 20 shots, RTX 4060 reference): every shot within tolerance,
  mean difference 0.00–0.01, 0.00% changed pixels. The references were **not** updated.

## Tests

- FBX: static crate (parts, materials, embedded and file textures, hierarchy, winding, UVs),
  skinned tentacle compared with the glTF tentacle (bones, skin at rest and animated, blend
  shapes, take length and curves), Convert Units / Axis Conversion / Scale Factor, Smoothing
  Angle, clip splitting, settings round trip (old metas unchanged), Generate LODs, sample files
  up to date; editor: FBX instance like glTF, LOD Group spawn.
- Compression: BC1/BC3/BC5/BC7 error bounds, BC6H HDR error, mip padding, format choice
  (alpha, type, normal slot, sprites, overrides), cache file round trip and damage, cache key,
  bake writes once then hits, pak round trip, GPU upload without validation errors, build packing
  for material uses.
- Renderer-made textures (`generated::tests`, `render/made_textures_tests.rs`): formats by role
  and Compression, cache key (pixels, recipe, format, sRGB), encode once then hit, background
  encode with the RGBA8 fallback, quality gate and its marker, off-grid sizes and mixed arrays,
  BC7 keeping a cutout's coverage per mip, uploads of every kind (whole arrays and slice by
  slice) without validation errors, and a build baking a tree and MeadowDemo's terrain that a
  player then uploads as blocks. Ignored: `made_textures_quality` (PSNR, normal error,
  coverage drift per texture and format), `made_textures_bc7_settings`,
  `made_textures_memory`, `made_textures_times`, `made_textures_closeups`.

## Samples

`SampleProject/Assets/Models/Tentacle.fbx` (Z-up, centimetres, four bones, Bulge/Pinch, Bend
take; imports to the same pose as `Tentacle.glb`), `Crate.fbx` (two materials, `Wood.png` next to
it, embedded normal map, a child node). Made in code by `models/fbx_sample.rs` with the small
binary FBX writer `models/fbx_write.rs`:
`cargo test -p misty_engine --release regenerate_sample_fbx -- --ignored`.

## Not done yet

- HDRI skies don't use BC6H yet.
- Renderer-made textures: the terrain's normal + height array stays RGBA8 in MeadowDemo (the
  meadow thatch map fails the quality gate; BC5 normals + a BC4 height array would hold it
  better but need another binding); the three oak assets upload their identical bark and leaf
  textures once each (cache entries are shared, GPU textures are not); a player whose impostor
  bake differs in a bit from the editor's draws that impostor uncompressed; the Library keeps
  entries of textures that changed (no cache cleanup, as for file textures).
- Normal maps: the standard shader and the custom-shader prelude rebuild Z (needed for BC5);
  shader code that decodes normal maps by hand keeps reading Z from blue.
- No BC compression on adapters without `TEXTURE_COMPRESSION_BC` (most mobile/web): no
  ETC2/ASTC path. Textures not a multiple of 4 stay uncompressed (no padding to blocks).
- A build packs one sRGB/linear variant per material use; another use falls back to RGBA8 in
  players.
- FBX: no cameras, lights, constraints, NURBS or vertex colours; instanced meshes share the
  first instance's materials; animation of a multi-material mesh's extra parts (blend shapes)
  only drives the first part; no Bake Axis Conversion (Z-up conversion stays on the roots, as
  Unity does by default); no Humanoid/Avatar rig setup.
- Model import: Read/Write is stored but doesn't free CPU meshes; Generate LODs runs on the
  import thread (not cached in Library); the clip preview is a 2D front view of the skeleton
  and vertices, not a lit render; no per-clip root motion, events or masks.
