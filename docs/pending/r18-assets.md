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

## Measurements

MeadowDemo, every texture the scene uses, at the default quality level (Windows, Normal
Quality defaults; `sprites/compress/meadow.rs`, ignored test):

| Textures | Count | RGBA8 + mips | Compressed | Saved | Path |
|---|---|---|---|---|---|
| Rocks and other material textures | 18 | 125.8 MB | 21.0 MB | 83% | compressed now |
| Trees (bark, leaves) | 27 | 151.0 MB | 21.0 MB | 86% | own loader, not switched |
| Grass and flowers | 16 | 22.4 MB | 3.5 MB | 84% | own loader, not switched |
| Terrain layers and other | 6 | 21.0 MB | 2.6 MB | 87% | own loader, not switched |

- Material textures (the rocks' albedo BC1, masks BC3, normals BC5) save **104.8 MB** of GPU
  memory in MeadowDemo now. The other 46 textures would save another 167 MB once the tree,
  grass and terrain-layer uploaders (renderer code: `trees.rs`, `terrain_grass.rs`,
  `terrain_layers.rs`) use the same blocks.
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

## Samples

`SampleProject/Assets/Models/Tentacle.fbx` (Z-up, centimetres, four bones, Bulge/Pinch, Bend
take; imports to the same pose as `Tentacle.glb`), `Crate.fbx` (two materials, `Wood.png` next to
it, embedded normal map, a child node). Made in code by `models/fbx_sample.rs` with the small
binary FBX writer `models/fbx_write.rs`:
`cargo test -p misty_engine --release regenerate_sample_fbx -- --ignored`.

## Not done yet

- Tree textures, grass/flower atlases and terrain layer arrays still upload RGBA8 (their
  loaders are renderer code owned by another workstream); the helpers (`compress::encode`,
  `cached`, `store`, `upload_compressed`) are ready for them. HDRI skies don't use BC6H yet.
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
