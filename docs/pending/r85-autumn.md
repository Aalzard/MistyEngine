# Round 85: autumn forest from a bought tree pack (branch r85-autumn)

The pack, everything made from it and the scene stay OUT of the repo: the project is
`E:\Misty Engine\Misty Engine Dev\MistyProjects\AutumnForest` (made with `misty --new "First
Person"`), the pack copied to `Assets/AutumnForestTrees`, shots in its `Shots` folder. Only
engine and editor work is committed here.

Disks: engine target `E:\MistyTargets\r85`, tool target `E:\MistyTargets\r85-tool`, TMP
`E:\MistyTargets\tmp\r85` (scripts `env.sh`, `gen.sh`, `shots.sh`; trial renders `s1`…).

## How the scene is made

`MistyProjects/AutumnForest/Tools/forest_tools` (a small crate outside the repo, path
dependency on this worktree's `misty_engine`): `forest_tools gen <project>` scans the project
(Unity metas taken over), writes 2K PNG copies of the pack's textures, the leaf litter ground
textures (the leaf atlas's twigs stamped a few thousand times, tileable), one `.tree` asset
per pack model (`model` = the FBX), the terrain with its trees and
`Assets/Scenes/AutumnForest.scene` (sun, haze, clouds, the four views as switched-off cameras
under "Views", the First Person Player). `forest_tools inspect <model>` prints what the
importer makes of a model.

Renders and timings: `render::project_shots_tests` (`project_scene_shots`,
`project_scene_times`; `MISTY_SHOT_PROJECT`, `MISTY_SHOT_SCENE`, `MISTY_SHOT_VIEWS`,
`MISTY_SHOT_DIR`), under the GPU lock.

## Milestone 1 (engine)

- TIFF textures (`image` feature `tiff`; `.tif` / `.tiff` in `TEXTURE_EXTENSIONS`); an
  "unspecified" fourth channel (Photoshop's alpha) is kept (`render::texture::tiff_alpha`).
- Unity `.meta` files are taken over by the asset scan (`asset_db::meta::unity_settings`).
- Tree assets from a model (`TreeAsset::model`, `trees::from_model`), `gloss_maps`.
- FBX: materials that name no textures take the ones named after them
  (`models::named_textures`), cut-out + two-sided when the albedo has clear texels,
  occlusion / gloss / roughness / translucency maps (converted on extraction); top nodes named
  `…_LOD<n>` become a LOD Group (`model_import`).

## What the pack is

15 FBX trees, each with nodes `Forest_Tree_LOD0..2` (bark + leaf parts) and `LOD3` (a crossed
billboard card). Inches in the file; the Unity meta's scale 1.17 (model 1 has no meta).
Heights 7.6–36.5 m, trunks 0.04–0.53 m. Materials `Bark`, `Leave`, `Billboard` without
textures (only `Billboard` names its `.tif`). Bark UVs tile down a bark atlas; leaves use an
atlas of six maple twigs. No ground texture in the pack.

## Open after milestone 1

- Ground: one litter texture pair in soft patches; no twigs, no bare soil, no fallen
  branches, no leaf meshes on the ground.
- Trees stand on a jittered grid; clumps and gaps would read more natural.
- Every tree asset packs its own copies of the shared textures (15 × 4 2K maps on the GPU).
- Far view: the terrain just ends (no hills behind the wood).

## Feedback pass after milestone 1 (scene only, no engine change)

Sun 46 degrees up, intensity 5.2, shadow angle 0.75 (crisp at the foot, softer with height);
ambient 1.25; eye adaptation held to -0.5..0.3. 33 tree assets (15 models in up to 5 colours:
plain, red, yellow, turning with the half-green leaves, green), drifts of colour by noise.
Glades and thickets (`wood_density`), terrain 1025 samples with small humps, a third "leaf
mould" ground layer, sparse dry grass (terrain grass, painted map), dry sprigs (the pack's
dry bush atlas as crossed cards, placed as terrain plants: 25 k), 258 fallen slim trees
(bark of the lightest level, a dead-wood material).

Tried and dropped: the sprigs as detail mesh layers (CPU render went to 800 ms a frame with
six layers over a 1024 map). Timing with 33 prototypes: every Renderer packs each asset's own
copies of the shared textures (a timing run of four views took 20 minutes under load).

Timings High 1080p (GPU partly busy with another job): overhead 15.0, eye level 18.9, into
sun 17.8, wide 16.0 ms. Milestone 1's scene, GPU idle: 11.6 / 14.3 / 14.2 / 12.9 ms.

Open: eye level and into-sun over 16 ms (trees 4.6-5.4 ms: sprigs and 33 prototypes);
clouds off (banding towards a low sun); not opened in the editor window yet.

## Milestone 2 (scene + three engine changes)

Engine: shared packed tree textures, Terrain Layer ▸ Relief (parallax), no trunk colliders
under 1.5 m, shots harness switches (see progress.md "Round 85, part 2").

"Cloud banding" was NOT a bug: the stripes were in how the 1080p PNG was previewed, the file
has none (sky steps of at most one level). Look at crops or half-size copies of big shots.

Scene (generator in the project's `Tools/forest_tools`): terrain 900 m, heightmap 2049,
mounds and root humps under trunks, litter layers with Relief 0.07 m and normal scale 2.2;
Misty GI on (High, intensity 2.0), ambient 2.0, SSR High full resolution, volumetric fog
(mean free path 100 m, max 320 m, anisotropy 0.6, sun volumetric ×1.6), sun shafts, clouds
on, a global Weather (Drizzle, sun and ambient dimming 0, clouds and haze not driven, wet
surfaces, puddles 0.25); bark tint ×1.5 against the wet darkening; stronger red / yellow
variants; sprigs fewer and taller (dry seedlings); LOD 20 m, impostors from 58 m.

Timings High 1080p (ms GPU): overhead 21.9, eye level 23.7, into sun 26.8, wide 21.3. Parts at
eye level: trees 5.3, SSR 3.4, shadow maps 2.2, shadow mask 2.0, terrain 1.6, Misty GI 1.3,
prepass 1.05, sun shafts 1.0, GTAO 0.6, TAA 0.5, weather 0.5, clouds 0.2, froxel fog 0.07.
CPU render 9–22 ms (1000–2250 draws: the fallen trees in every cascade).

Open: over 16 ms everywhere (the user put look first); trees 5–6 ms; CPU draw count; the
shade in the view from above is still deep; rain streaks barely read in stills.
