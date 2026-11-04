# Scene view Gizmos dropdown (icon size and color)

Progress: the Scene view toolbar's Gizmos toggle now has a ▾ dropdown with Icon Size
(0.5×–2×), Icon Color (tint picker + Reset) and 3D Icons. Settings are saved per project.

Decisions:
- New small module `scene_icons.rs` holds `IconSettings` and the dropdown body — keeps
  scene.rs from growing and gives the settings one home.
- Tint multiplies the default light grey (235), so white = today's look exactly; the
  selection highlight stays orange regardless of tint, so selection is always visible.
- Icon click area scales with icon size (radius = 2/3 of the side, 16 px at 1× as before)
  — a bigger icon that can't be clicked at its edge would feel broken.
- 3D Icons included (default off): full size within 10 units of the Scene camera, shrinking
  to a quarter beyond; it was a one-line factor in the size function.
- Persisted in `Library/EditorSettings.json` (`scene_icons`, `#[serde(default)]`), saved with
  the Scene view camera on close; the camera is still only saved for 3D views, icons always.
- Only the billboard icons (`draw_icons`) use these settings; line gizmos (light ranges,
  colliders, probes) are unchanged as asked.
- Screenshot hooks: `MISTY_GIZMOS_MENU=1` opens the dropdown, `MISTY_ICON_SIZE` /
  `MISTY_ICON_COLOR=r,g,b` set the values.

# Scene view Quality dropdown

Progress: a "Quality ⏷" button in the Scene view toolbar (left of the 2D toggle) opens Quality
Level, Anti-aliasing (FXAA), Render Scale (25–200%) and Upscale Filter (Bilinear / Nearest /
Sharpen). Project Settings ▸ Quality got the same FXAA and Upscale Filter rows.

Decisions:
- The dropdown edits the previewed level of Project Settings ▸ Quality (`quality::level()`),
  saved through `project_settings::changed` exactly like the settings page; switching the level
  calls `quality::install` like the Game view's picker. So Render Scale, FXAA and the filter
  apply to the Scene view, the Game view and built players alike — one set of values.
- The Scene view now honours Render Scale too (it used to ignore it): the user wants the fast
  shortcut to show its effect where they are looking.
- Anti-aliasing: the renderer's MSAA is fixed at 4x (`render::SAMPLE_COUNT`); the level's
  MSAA choice is still saved-but-unused (its tooltip already says so). Making MSAA switchable
  means rebuilding every pipeline and custom shader per sample count, so instead FXAA was
  added as a new post step (`render/finish.rs`, `shaders/finish.wgsl`) that runs after the
  composite. The dropdown shows only the FXAA toggle so no dead control is offered there.
- Upscale Filter: Bilinear/Nearest are just the sampler that stretches the image (egui texture
  filter in the editor's `viewport.rs`, the blit sampler in `misty_player/src/gfx.rs`).
  Sharpen = our own contrast-adaptive sharpening (CAS-style cross filter) in the same finish
  pass, then bilinear. FXAA + Sharpen run in one pass (`fs_both`).
- The finish pass only runs when FXAA or Sharpen is on; otherwise the composite writes the
  output as before (no cost). Each target has one extra 8-bit image for it.
- No DLSS / "Nvidia AI enhance" option: DLSS needs NVIDIA's proprietary SDK (and RTX
  hardware), which can't be used here. No fake option was added. Tell the user Sharpen at a
  lower Render Scale is the closest built-in alternative.
- Screenshot hooks: `MISTY_QUALITY_MENU=1` opens the dropdown; `MISTY_RENDER_SCALE`,
  `MISTY_UPSCALE=Nearest|Bilinear|Sharpen`, `MISTY_FXAA=1` change the previewed level in
  memory only (never saved to ProjectSettings.json).
- The player's splash uses the same blit sampler, so with Nearest it is sampled nearest too
  (only visible if the splash is scaled; left as is).
