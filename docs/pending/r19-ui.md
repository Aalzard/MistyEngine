# Round 19A — uGUI parity (branch r19-ui)

One runtime UI system: the canvas UI now covers the reference engine's uGUI feature set (behaviour copied,
no code), drawn and edited in the Scene view.

## Progress

- **Canvas render modes** (`scene/ui_components.rs`, `ui/canvas.rs`, `ui/tree.rs`): Screen Space - Overlay,
  **Screen Space - Camera** (Render Camera, Plane Distance: laid out like an overlay, placed on a plane that
  exactly fills the camera's view at that distance; unlit, depth-tested against the scene, so nearer objects
  cover it; without a camera it acts as an overlay, with the reference editor's note in the Inspector) and
  **World Space** (the Transform places the canvas, the Rect Transform sizes it in canvas units; switching a
  canvas to World Space in the Inspector keeps its on-screen size and gives it a 0.01 scale). Every canvas is
  laid out in its own canvas-pixel space; world-placed canvases carry a canvas → world and a canvas → clip
  matrix. Draw order: world-placed canvases (sort order, then far to near), then overlays (sort order).
- **Rendering** (`ui/draw.rs`, `render/ui.rs`, `shaders/ui.wgsl`): UI vertices carry a clip-space position
  (overlays z 0, w 1). Batches of world / camera canvases use `fs_world`, which hides fragments behind the
  scene's depth (MSAA depth sample 0, looked up through the clip position so upscaled views work), in the same
  UI pass as the overlays. `UiRenderer::render_view` draws any view (the Scene view passes its own camera);
  `render` keeps working unchanged for the Game view and the player.
- **Canvas Scaler**: Constant Pixel Size, Scale With Screen Size with **Match Width Or Height / Expand /
  Shrink**, **Constant Physical Size** (centimetres, millimetres, inches, points, picas; screen DPI reported by
  the Game view and the player, fallback DPI otherwise; sprite DPI), Reference Pixels Per Unit, and for World
  Space canvases Dynamic Pixels Per Unit (the resolution text and masks are made at).
- **Rect Transform rotation and scale**: the Transform's z rotation and x / y scale turn and scale a UI object
  about its pivot (`UiNode::xf`), drawn, clicked (`UiNode::contains`) and outlined that way; masks clip before
  the rotation.
- **Image types** (`ui/image_mesh.rs`): Simple, **Sliced** (9-slice from the sprite's border; borders shrink in
  proportion when the rect is smaller than both, Fill Center), **Tiled** (tiles at the sprite's size from the
  lower-left corner, the last ones cut with their uvs; with borders the edges stay and the middle repeats;
  bounded to 4096 tiles), **Filled** (Horizontal / Vertical from either side, Radial 90 about a corner, Radial
  180 about an edge's middle, Radial 360 about the centre, from each origin, clockwise or not; triangle fans,
  clipped to masks with Sutherland–Hodgman) and **Preserve Aspect** (Simple, Filled; shrinks towards the pivot).
  Pixels Per Unit Multiplier. Plain textures get their import settings' border too.
- **Sprite borders** (`sprites/import.rs`, `inspector/sprite_border.rs`): `border` (left, bottom, right, top
  pixels) on Single textures and on each sprite of a Multiple one, left out of `.meta` files while unset;
  edited in the texture import settings (a sprite picker for Multiple textures) with green lines on the
  preview.
- **Canvas Group** (alpha multiplies every graphic below, Interactable off disables the selectables below and
  shows them Disabled, Blocks Raycasts off lets the pointer through, Ignore Parent Groups; alpha 0 draws
  nothing). **Aspect Ratio Fitter** (Width Controls Height, Height Controls Width, keeping the pivot point;
  Fit In / Envelope Parent sized from the parent with the pivots lined up). The layout groups and Content Size
  Fitter keep their tests.
- **Raycasts** (`ui/input.rs`): the pointer is traced through each canvas' placement (inverting the plane's
  homography; rays meeting a plane edge-on or behind the camera miss), topmost first: overlays, then world and
  camera canvases near to far. The event system works in the canvas pixels of the canvas under the pointer
  (and of the pressed canvas while a drag lasts), so sliders, scroll rects and input fields work on world
  canvases.
- **Navigation** (`ui/navigation.rs`, `ui/events.rs`): Selectable Navigation (None, Horizontal, Vertical,
  Automatic, Explicit with Select On Up / Down / Left / Right, Wrap Around for Horizontal / Vertical), stored
  flat in each selectable and left out of files at its defaults. Automatic scoring compares positions in image
  pixels, from the edge of the current rect, so it crosses canvases; non-interactable selectables (including
  Canvas Group ones) are skipped. Event System: First Selected, Submit, **Cancel** (closes an open dropdown,
  else `OnCancel()` on the selection's scripts), **Move Repeat Delay / Rate**.
- **Input System UI map** (`input/actions/ui_module.rs`): the Event System's **Actions Asset** "UI" map (or the
  default UI actions) runs as its own action set inside the Input System, stepped with the others, read every
  frame for Navigate / Submit / Cancel; held directions repeat. With Active Input Handling = Input Manager the
  arrow keys, Return / Space / Escape and the first joystick's d-pad / stick / South / East drive it instead.
- **Scene view** (`ui_gizmos.rs`, `ui_rect_tool.rs`, `viewport.rs`): every canvas is drawn in the Scene view
  (overlays standing in the world at one unit per pixel, camera canvases in front of their camera, world
  canvases where they are) with the scene's depth, in 3D and 2D mode. Canvas outlines and the selection's rect
  are painted on top; clicking a UI graphic selects it (unless a scene object is nearer). **Rect tool** on a UI
  object: corner / edge handles resize (Shift on a corner keeps the ratio), dragging inside moves, just outside a
  corner rotates about the pivot (Shift snaps to 15°), the pivot circle moves the pivot and the anchor triangles
  move the anchors, both keeping the rect in place (anchors that meet move together). A World Space root canvas
  resizes its own rect and moves its Transform. **Visualize Navigation** (beside a selectable's Navigation in the
  Inspector) draws each selectable's links. Frame Selected frames UI objects where they are drawn.
- **Font assets** (`ui/rich/font_asset.rs`, `inspector/font_asset_editor.rs`): import settings in the font's
  `.meta`: Rendering Mode **SDF** (default, unchanged output) or **Raster** (4×4 supersampled coverage, drawn as
  coverage), **Sampling Point Size** (12–128), **Padding** (SDF reach), **Fallback Font Assets** (tried after the
  font, before the built-in and system fonts). Used by Text and rich text (any text with a font asset takes the
  rich path). The Inspector previews the glyphs as the atlas will hold them; Apply reloads the font.
- **Scripting** (`scripting/ui_api.rs`): Image `type`, `fillAmount`, `fillMethod`, `fillOrigin`,
  `fillClockwise`, `fillCenter`, `preserveAspect`; `Canvas` (`renderMode`, `sortingOrder`, `planeDistance`);
  `CanvasGroup`; `AspectRatioFitter`. Documented in `docs/scripting.md`.
- **Sample**: `Scenes/UIShowcase.scene` (`sample_ui_showcase.rs`, `ui_showcase_matches_its_generator`,
  `ui_showcase_plays_with_a_gamepad`) with `Scripts/UIShowcase.lua` and generated sprites
  `Sprites/UIPanel.png` (16 px border), `UIButton.png` (12 px border), `UIRing.png`: an overlay pause menu
  (sliced panel in a Canvas Group, Vertical wrap-around navigation, First Selected), a Screen Space - Camera
  HUD (Radial 360 health ring, Horizontal stamina bar, both driven by `fillAmount`) and a World Space sign
  (sliced panel, rich text, a button clicked through the camera).
- **Editor capture switches**: `MISTY_TOOL=Rect`, `MISTY_FRAME_SELECTED=1`, `MISTY_UI_NAVIGATION=1`;
  `MISTY_2D=1` now switches the Scene view camera at once.
- **Tests**: fill geometry (linear fills, radial 90 / 180 / 360 angles and areas from every origin, both
  directions), sliced pieces and uvs (and squeezed borders), tiles and cut uvs, polygon clipping, Preserve
  Aspect; Canvas Scaler maths (Expand, Shrink, physical units, sprite pixels per unit); camera-plane round
  trips; World Space hit tests (centre, an off-centre child, outside, edge-on, behind) and depth batches;
  Screen Space - Camera placement and fallback; draw order; Canvas Group fading / blocking / interactable /
  ignore parents; Aspect Ratio Fitter modes; rotated and scaled rects; navigation order (grid walk, modes,
  wrap, explicit, None, disabled and group-blocked candidates) and move repeat; the default UI map (arrows,
  Return, d-pad, East, Input Manager fallback); font atlas rendering (distance field, coverage at two sizes,
  raster and custom-size SDF entries in the atlas row for row, fallback fonts, Text with a raster font asset);
  kittest for the Rect tool (corner resize, move, edge resize, pivot, anchors, rotate with snapping); Scene view
  UI bounds; the sample generator, play test (gamepad navigation through the default UI map, the sign clicked
  through the camera) and a headless render test (sign, HUD ring and bar, menu all reach the image).

## Screenshots (`docs/screenshots/r19/`)

- `ui_showcase_play.png`: UIShowcase playing: the Screen Space - Camera HUD (radial ring, stamina bar), the World
  Space sign in the scene (the post in front of it hides its lower edge) and the overlay pause menu.
- `ui_scene_view_world.png`: the sign selected in the Scene view (3D), drawn with the scene.
- `ui_scene_view_hud.png`: the HUD canvas on its plane in front of the Main Camera; Canvas Inspector with
  Render Camera and Plane Distance, Canvas Scaler with Screen Match Mode.
- `ui_rect_tool.png`: 2D Scene view, Rect tool on a menu button (corner / edge handles, pivot, anchors), Sliced
  Image and Navigation in the Inspector.
- `ui_navigation.png`: Visualize Navigation on the menu (Vertical, wrapping), Canvas Group in the Inspector.
- `ui_image_filled.png`: the health ring's Filled / Radial 360 settings.
- `ui_canvas_camera.png`: the HUD's Canvas and Canvas Scaler.
- `ui_sprite_border.png`: the panel sprite's import settings with its 9-slice border lines.

## Decisions

1. **World-placed canvases draw in the UI pass, depth-tested by hand** (the fragment compares its depth with the
   scene's MSAA depth) instead of joining the forward pass: no change to the main pass or its golden images, and
   UI colours stay exactly as authored (not tonemapped). They are still hidden by nearer geometry and sorted with
   each other far to near. The main pass normally discards its MSAA depth when nothing after it reads it;
   `render::ui::set_keeps_depth(needs_depth(scene, …))` (called by the Game view, Scene view and player before
   each render) keeps it while a world-placed canvas or a far-plane video is in the scene, so scenes without
   them render exactly as before.
2. **Screen Space - Camera without a Render Camera acts as an overlay**, like the reference engine (the
   Inspector says so and offers the main camera).
3. **The Transform carries a UI object's rotation and scale** (z rotation, x / y scale about the pivot), since the
   Rect Transform here sits beside the Transform rather than replacing it. Position stays the Rect Transform's.
4. **Masks clip before an object's own rotation**: exact for rotated groups (clip and content turn together),
   approximate for a mask whose child turns on its own (the reference engine's RectMask2D has the same limit).
5. **Navigation is stored flat in each selectable** (`navigation`, `select_on_up` …), left out of files while
   Automatic with no targets, so older scenes re-save unchanged; explicit targets are remapped like other object
   references.
6. **The UI map runs as its own action set** (not one of the scripts' sets), so it never sends events to scripts
   and works without a Player Input. Space no longer submits under the Input System (the default UI actions
   submit with Return and South, like the reference ones); the Input Manager fallback keeps Space.
7. **Font asset atlases share the rich text atlas**, entries keyed by font and glyph; a font's settings are read
   when it loads, and Apply reloads it under a new id. The built-in font's settings are the old constants, so
   existing text is unchanged.
8. **Clicking UI in the Scene view picks the topmost drawn graphic** (Image, Raw Image or Text), whether or not
   it is a raycast target, unless a scene object is nearer to the camera.

## Not done

- Scripts can't set the selection (`EventSystem.current.SetSelectedGameObject`) or read it yet.
- No Graphic Raycaster "Blocking Objects" (3D colliders in front of a world canvas don't stop clicks), no
  per-canvas Event Camera (World Space canvases are clicked through the game camera).
- Rotated widgets: sliders, scrollbars and input fields map the pointer in their parent's unrotated frame.
- No sprite border handles to drag on the preview (fields and lines only); no Sliced + Filled combination.
- No TextMeshPro-style font asset file with a baked character set, no multi-atlas fonts; outlines and shadows
  of Raster font assets are limited to the coverage edge.
- The Rect tool doesn't snap edges or anchors to the parent / siblings, and has no Raw edit mode.
