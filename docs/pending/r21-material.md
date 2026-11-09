# Round 21 (17D): Standard Lit v2, the material model's optional features

Branch `r21-material`, cut from main at 432f979. Asked for: detail maps, parallax occlusion
mapping, clear coat, translucency / subsurface and anisotropy as optional Standard material
features that cost nothing when unused; glTF extension import; Shader Graph master stack
outputs; preview shapes in the material Inspector; existing materials unchanged; a
MaterialDemo2 sample; the GPU cost of each feature.

## How the features stay free (`render/material_v2.rs`)

- Each feature is a bit: Detail 1, Parallax 2, Clear Coat 4, Translucency 8, Anisotropy 16,
  plus Specular 32 (the `KHR_materials_specular` tint, cheap to add).
- mesh.wgsl has `const MAT_FEATURES: u32 = 0u;` and every feature's code sits behind
  `if ((MAT_FEATURES & FEAT_X) != 0u)`. That is a constant, so the base shaders compile no
  feature code at all. The terrain, grass, trees, water and custom shaders include mesh.wgsl too
  and keep 0.
- A material with features gets a variant compiled for exactly its bits: `lit_source(bits)` is
  mesh.wgsl with that one line replaced, plus `shaders/mesh_lit.wgsl` (entry points `fs_lit`
  and `fs_lit_cutout`). Sources are built once per mask. Pipelines are built on first use like
  the other variants (`PipelineKey.features`), and the depth prepass variant follows.
- Group 2 of a feature variant (`lit_layout`) is the six Standard bindings plus a 128-byte
  uniform of the feature values (binding 6) and six feature maps (7–12: detail albedo, detail
  normal, detail mask, height, coat normal, thickness; 1×1 white / flat when empty). Bind groups
  are keyed by textures + values (`LitSet`) in the texture cache, next to the plain sets. The
  plain set is still prepared for these materials' shadow, AO-prepass and debug draws.
- `MaterialParams.extras: Option<Arc<LitExtras>>` is `None` unless a feature is on, so plain
  materials take exactly the old path: same pipeline key, same bind group, same draw data
  (`Draw` is unchanged). Virtual geometry skips feature materials; they draw through the normal
  lists.
- The lighting hooks in `shade` (feature-guarded): `feature_light` replaces the per-light lobe
  when a shading feature is on (anisotropic GGX, wrap diffuse, the coat lobe with base energy
  loss), `coat_ambient` adds the coat's sky / probe reflection, the reflection vector bends for
  anisotropy, and F0 takes the specular tint. Area lights keep the plain lobe.

## The features (`material_features.rs`, mesh_lit.wgsl)

Stored in the `.mat` file as sections that are left out while at their defaults (old files
re-save byte for byte, as `old_sample_materials_re_save_unchanged` checks). Each has an
**Enabled** toggle. Switching it off keeps the values and compiles nothing.

| Section | Fields | Shader |
|---|---|---|
| Detail Maps | detail albedo ×2, detail normal + scale, detail mask (R), tiling, offset | albedo × detail × 4.59 (×2 in sRGB, grey = no change), whiteout normal blend, both masked |
| Parallax | height map (R, white = high), scale (uv units), steps (4–64) | parallax occlusion mapping: steps from steps/4 head-on to steps at grazing, linear interpolation of the hit; every map is then read at the hit uv |
| Clear Coat | strength, smoothness, optional normal map + scale | Schlick F (F0 = 4%) × GGX coat lobe per light, coat reflection of the sky / probes, base × (1 − F) |
| Translucency | colour, strength, wrap, thickness map (R) + thickness | wrap diffuse tinted by the colour; colour × strength × (1 − thickness) feeds the existing thin-surface transmission |
| Anisotropy | strength (−1..1), direction (Horizontal U, Vertical V, Radial), rotation | KHR-style anisotropic GGX (αt = mix(α, 1, s²)), bent reflection normal |
| Specular (Advanced) | specular, specular colour | F0 = 4% × colour × specular for dielectrics |

The UV Rotation field (Main Maps) is new too. It comes from `KHR_texture_transform`.

## glTF import (`models/gltf_materials.rs`)

- `KHR_materials_clearcoat`: factor, roughness → smoothness, normal texture (extracted as
  `<model>_<material>_ClearCoatNormal.png`, `ModelMaterial.extra_maps`).
- `KHR_materials_anisotropy`: strength, rotation (tangent space CCW → the material's uv angle).
- `KHR_materials_specular`: factor and colour factor.
- `KHR_materials_diffuse_transmission`: factor and colour → Translucency.
- `KHR_materials_transmission` (+ `KHR_materials_volume` attenuation colour): see-through glass,
  so Transparent mode with alpha = 1 − transmission, tinted. Real refraction is not modelled.
- `KHR_texture_transform` on the base colour texture: tiling, offset and UV rotation, with the
  offset corrected for the engine's rotate-about-centre order (the test checks points against
  the glTF formula).
- gltf crate features `extensions`, `KHR_texture_transform` and `KHR_materials_transmission`
  are now on (files that *require* texture transform or transmission load).
- Not imported: the clear coat / roughness masks, the anisotropy direction texture, transmission
  and thickness textures.

## Shader Graph

- Graph Settings (Lit) has three new toggles: **Clear Coat** (Coat Mask, Coat Smoothness),
  **Anisotropy** (Anisotropy, Anisotropy Direction in tangent space) and **Subsurface**
  (Subsurface Color, Thickness, which scales Transmission). Off by default, so existing graphs and
  files don't change.
- Custom surface shaders get `o.coat`, `o.coat_smoothness`, `o.anisotropy`,
  `o.anisotropy_direction` and `o.subsurface` outputs. A shader that writes one is compiled with
  that lighting feature (`features_of_surface_source`). Others keep the plain lighting.

## Editor

- Material Inspector (Standard): Secondary Maps, Parallax, Clear Coat, Translucency and
  Anisotropy sections under Main Maps, each opened by its checkbox. Specular is under Advanced
  Options. The UV Rotation row is new too.
- Preview: Sphere / Cube / **Plane** buttons (the plane shows parallax and detail best).
  `MISTY_PREVIEW_SHAPE=plane` sets it for screenshots.

## MaterialDemo2 (`crates/misty_editor/src/sample_materials2.rs`)

`Assets/Scenes/MaterialDemo2.scene`, materials in `Assets/Materials/MaterialDemo2/`, textures in
`Assets/Textures/MaterialDemo2/` (all procedural, generated by the test, so no licence applies).
It has a clear-coat car-paint body and sphere over metallic flakes, a radial brushed disc and a
vertical-brushed pipe, a parallax brick wall, a detail-mapped rock (mask + ×2 albedo + detail
normal) and translucent cutout leaves back-lit by a warm lamp.
`material_demo2_matches_its_generator` checks the files, and
`material_demo2_materials_use_their_features` checks each material compiles only its feature.
Regenerate with `cargo test -p misty_editor -- --ignored write_material_demo2`.

## Tests

- `material_v2`: the uniform layout, variant sources set only their bits, and all 63 feature
  variants validate with naga (CPU).
- `material_features`: bits, values, defaults left out of files.
- `gltf_materials`: each extension, zero factors change nothing, glass, texture transform
  against the glTF formula, and a whole `.gltf` file with required texture transform.
- `material_v2_gpu_tests`: each feature changes the picture. A neutral detail (grey ×2, flat
  normal) draws the plain material: 0 pixels over 8 levels apart, mean 0.57 (the grey is 0.8%
  under 1 after sRGB). Plain materials render the same with a feature material in the frame.
- `lit_v2_blocks_follow_the_settings_and_compile_their_features` (Shader Graph).
- `pipeline_state_per_mode` covers the new entry points.

## Screenshots (`docs/screenshots/r21/`)

- `material_demo2.png`: the demo through its camera at 1280×720 (`render_material_demo2`,
  ignored test). Left to right: clear-coat paint over flakes, the radial brushed disc (its
  streak follows the key light) above the vertical-brushed brass pipe, the parallax wall behind,
  the detail rock, and the leaves glowing green against the back lamp.
- `editor_clear_coat_inspector.png`: the editor with MaterialDemo2 open and Clear Coat Paint in
  the Inspector (the feature sections, Clear Coat on). The Project thumbnails render the features.
- `editor_parallax_plane_preview.png`: Parallax Bricks with the Inspector scrolled down to
  the Plane preview (`MISTY_PREVIEW_SHAPE=plane MISTY_INSPECTOR_SCROLL=600`).

The material asset Inspector now scrolls, because with the feature sections it is taller than
the panel. Before, the preview fell off the bottom.

## Results

### GPU cost per feature (`material_feature_costs`, ignored)

`cargo test --release -p misty_engine --lib material_feature_costs -- --ignored --nocapture`
(taken under the shared GPU lock). RTX 4060, Vulkan, 1600×900. A wall of 154 spheres and a plane
covers every pixel, all in one material, with the sun and a point light, no shadows. Each case
is the median of 60 frames, and the best of three interleaved rounds is kept. "Main: opaque
meshes" is the pass the material is drawn in.

| Material | Frame (ms) | vs plain | Where |
|---|---|---|---|
| plain Standard | 0.934 | — | opaque meshes 0.683 ms |
| + Detail Maps (albedo + normal + mask) | 0.958 | +0.025 | |
| + Parallax (32 steps, brick height) | 0.938 | +0.004 | opaque +0.007 |
| + Clear Coat | 0.945 | +0.011 | opaque +0.011 |
| + Translucency | 0.927 | −0.007 (noise) | |
| + Anisotropy | 0.927 | −0.007 (noise) | |
| + Specular | 0.931 | −0.003 (noise) | |
| all six together | 1.320 | +0.386 | opaque +0.368 (+54% of the pass) |

- On their own, the features cost between nothing measurable and 0.025 ms at 1.4 Mpx. That is
  within ±0.01 ms run-to-run noise, except detail and clear coat. Parallax is cheap here because
  the ray stops at the first step on the flat brick faces and only marches in the mortar. A
  rougher height field costs more, up to the step count at grazing angles.
- All six in one variant cost far more than the sum (+0.39 ms, 54% of the opaque pass). The
  combined shader holds many more values at once (the parallax loop, nine extra texture reads,
  the coat and anisotropic lobes per light), and fewer pixels can be shaded side by side. So
  turn on only what a material needs. Because each combination is its own variant, nothing
  else pays for it.
- Plain materials pay nothing: they keep the same pipeline, bind group and per-draw data, and
  the feature code in their shader sits behind a constant-false condition. The golden shots
  below confirm the pixels. I did not time against a main build, because that needs a second
  full build in the one allowed target dir.

### Golden shots

`cargo test --release -p misty_engine --lib golden_shots -- --ignored --nocapture --test-threads=1`
(under the GPU lock): **all 20 pass**. CyclesRoomDemo is identical at every level (mean 0.00,
0.00% changed). The MeadowDemo shots are at mean 0.15–0.39 with ≤ 0.03% changed, the same as
main's run-to-run spread recorded in progress.md (wind and temporal effects).

### Checks

- `cargo fmt --all`: clean. `cargo clippy --workspace --all-targets --release`: 0 warnings.
- `cargo test --workspace --release --no-fail-fast`: misty_editor 724 passed / 1 failed / 78
  ignored, misty_engine 1313 passed / 1 failed / 89 ignored, the rest ok. Both failures are
  environmental and pass when run alone:
  `auto_picks_one_phase_and_lit_for_a_few_rocks` (on the known-flaky list) and
  `hub::tests::new_project_page_shows_the_meadow_demo_card`, which failed with "a folder named
  Meadow already exists" in its temp folder while other suites ran.

## Not done / follow-ups

- glTF textures other than the clear coat normal (the clear coat and roughness masks, the
  anisotropy direction texture, transmission and thickness textures).
  `KHR_materials_transmission` is approximated as Transparent glass with no refraction.
- Parallax doesn't write depth (no pixel depth offset) and casts no self-shadows. Shadows and the
  depth prepass use the flat surface.
- Area (rectangle) lights shade feature materials with the plain lobe.
- Shader Graph: no Parallax Occlusion Mapping or Detail node yet (the master stack has the coat,
  anisotropy and subsurface outputs).
- The editor runs left `.meta` files next to the new MaterialDemo2 assets (and junk elsewhere).
  They are not committed. The scene references its materials by path.
