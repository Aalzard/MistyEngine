# Round 12 — Splines

## Progress

Splines like the reference engine's Splines package:

- **Spline Container** (`misty_engine::splines`): one or more cubic Bézier splines of knots
  (position, rotation, tangent in/out, tangent mode, tension), open or closed. Modes: Auto
  Smooth, Linear, Bezier Mirrored / Continuous / Broken. `SplinePath` evaluates position,
  tangent, up vector, curvature, rotation, length, t at distance and nearest point, using a
  per-curve arc-length table.
- **Scene view editing** (`misty_editor::spline_tools`): the curve of every container is drawn
  (selected brighter); knots and the tangent handles of selected Bézier knots are drawn on top.
  Click / Shift / Ctrl select knots, drag moves them, the Move and Rotate tools show a handle on
  the selected knots, Ctrl+click on the curve inserts a knot (exact de Casteljau split for Bézier
  segments), Delete removes knots. **Draw Spline** (Inspector toggle) places knots on colliders,
  terrain or the ground plane: click = Auto Smooth knot, drag = Mirrored knot with tangents,
  clicking the first knot closes the loop, Enter/Esc finish; with no container selected the first
  click creates a "Spline" object. Every action is one undo step.
- **Spline Extrude**: tube (radius, sides, caps) or road strip (width) along every spline of the
  container, segments per unit, range, V tiling per unit, rebuilt live.
- **Spline Instantiate**: copies of prefabs or primitives (weighted items) by count or by spacing
  range, aligned to the spline / container / world, random position, rotation and scale ranges,
  seeded. Copies are children marked with **Spline Instance**.
- **Spline Animate**: moves its object by duration or speed, Once / Loop / Ping Pong, easing,
  alignment, start offset, Play On Awake; runs in Play mode and the player.
- **Lua**: `SplineContainer` (`EvaluatePosition/Tangent/UpVector/Curvature`, `CalculateLength`,
  `GetNearestPoint`, `KnotCount`, `GetKnotPosition`) and `SplineAnimate` (`Play/Pause/Restart`,
  `normalizedTime`, `elapsedTime`, `isPlaying`, …). Documented in `docs/scripting.md`.
- **SplinesDemo** (`sample_splines.rs`): winding road, lamp-post prefab along it, cones with
  random scale, a car ping-ponging along the road (`SplineRider.lua`, P pauses), a closed neon
  tube with an orb easing along it.

## Decisions

1. **t is by distance.** A spline's `t` maps to arc length (via a 48-sample table per curve), so
   `t = 0.5` is the middle of the length, matching how the reference package evaluates splines
   and making Spline Animate and Instantiate spacing even without extra work.
2. **Tangent modes are applied at evaluation**, not by rewriting stored data: Auto Smooth and
   Linear ignore the stored tangents (Auto Smooth uses the average of the unit directions to the
   neighbours times `tension` × distance; Linear points a third of the way to each neighbour so
   segments are straight with uniform speed). Mirrored/Continuous derive the in tangent from the
   out tangent; `Knot::enforce` keeps the stored values consistent after edits so switching to
   Broken keeps the shape.
3. **Up vectors** blend the knots' up vectors (smoothstep) and are made perpendicular to the
   tangent — simple and predictable; no rotation-minimising frames.
4. **Extruded meshes are runtime meshes keyed by a content hash** (FNV-1a of the splines and
   settings, stable across builds): `runtime://spline-extrude/<hash>#0` in the Mesh Filter.
   A saved scene names the same mesh it rebuilds on load, unchanged extrudes cost one hash per
   frame, and undo restores a key that is simply rebuilt if it was dropped. Meshes are in the
   container's local space.
5. **`models::runtime::register` only bumps the cache generation when replacing a key.** A new
   key can't be in a GPU cache (a lookup first leaves an error entry, which counts as replacing),
   so dragging a knot doesn't re-upload every imported mesh each frame. Replaced extrude meshes
   are collected and dropped in one batch once edits pause (or after 64), because dropping does
   clear GPU caches.
6. **Spline Instantiate copies are real children** marked with a non-removable Spline Instance
   component and rebuilt (deleted and recreated) when a hash of the inputs changes. The hash is
   not saved, so copies are rebuilt once after loading; the demo scene file holds no copies or
   meshes (they are generated on load), which keeps the generator test independent of the project
   root and prefab loading.
7. **Systems run from the frame loop**: `splines::update` (extrude + instantiate) from the
   editor's edit-mode frame and `Runtime::update`; Spline Animate from the script host's
   Animation step (after Update, so Lua `Play`/`Pause` apply the same frame).
8. **Knot selection lives in `EditorState::splines`**, not in the undo snapshots; it is
   validated against the container every frame so undo can't leave dangling indices. Delete is
   claimed by the knot selection only while the pointer is over the Scene view.
9. **Draw Spline raycasts against colliders** with a throw-away physics world built on click
   (only on click), then terrain, then the y = 0 plane.

## Left / ideas

- Knot links between splines, spline-profile extrude, Square profile.
- Selecting a container by clicking its curve (containers without a mesh are picked from the
  Hierarchy).
- Spline Animate preview scrubbing in Edit mode.
