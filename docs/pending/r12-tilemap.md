# Round 12: Tilemap, Tile Palette and 2D lights

Progress: new `misty_engine::tilemap` module with **Grid** (cell size, gap, Rectangle or
Isometric layout), **Tilemap** (one `.tile` asset + rotation/flip per cell, tint, tile anchor,
animation frame rate), **Tilemap Renderer** (Chunk or Individual mode, Sorting Layer / Order) and
**Tilemap Collider 2D** (per tile, or "Used By Composite" merging cells into few boxes). Tile
assets: plain **Tile**, **Rule Tile** (tiling rules over the 8 neighbours with This / Not This /
Don't Care, Single / Random / Animation output, Fixed / Rotated / Mirror X / Mirror Y / Mirror XY
matching, per-rule collider) and **Animated Tile** (frames, fps, start time). **Tile Palettes**
(`.palette`) and the **Tile Palette window** (Window ▸ 2D ▸ Tile Palette) with Select, Move,
Paint, Box Fill, Pick, Erase and Flood Fill tools, `[` `]` rotate / Shift mirror, an Active
Tilemap dropdown, drag-and-drop of sprites / textures / tiles to make palette tiles, and painting
in the Scene view with a cell highlight and one undo step per stroke. New `misty_engine::light2d`
module with **Light 2D** (Global, Point, Spot: inner/outer radius, inner/outer angle, falloff
strength, colour, intensity, target Sorting Layers, shadows with strength/softness) and
**Shadow Caster 2D** (sprite rect or tilemap cells, optional self shadows). Lua: `Tilemap`
(SetTile, GetTile, HasTile, ClearAllTiles, GetUsedTilesCount, WorldToCell, CellToWorld,
GetCellCenterWorld, color) and `Light2D` (intensity, color, radii, enabled). Generated
**TilemapDemo** scene (night level, rule-tile ground, animated water, torches with shadows, dim
global light, the Platformer2D player).

Decisions:
- **One `.tile` format for all tile kinds** (`kind`: Tile / RuleTile / AnimatedTile). The editor
  converts between them in place from the Inspector; a separate extension per kind would only add
  asset-type plumbing. Palettes are their own small JSON (`.palette`: tile path per cell) rather
  than a prefab: we have no prefab-with-tilemap workflow and the palette only needs swatches.
- **Tilemap cells serialise compactly**: `{"palette": [tile paths], "cells": [[x, y, index,
  transform], …]}` sorted by (x, y), so a path appears once per tilemap and files diff stably.
  The GUID table and rename rewriting see the paths as ordinary strings (`tile` and `palette`
  were added to the referencing extensions of `asset_ops` and the build's `asset_deps`).
- **Cell transform = 4 bits** (quarter turns counter-clockwise after flip X / flip Y). Rule
  outputs compose their matched orientation with the cell's (`TileTransform::then`).
- **Chunked rendering through runtime meshes.** Static tiles of a 16×16-cell chunk are merged per
  (texture, tint) into one mesh registered with a new `models::runtime::register_new`: the mesh
  name contains the chunk's content hash, so a changed chunk gets a new key and no other GPU
  mesh is invalidated (the existing `register` bumps the global generation and re-uploads every
  imported mesh). Retired chunk meshes are dropped from the library at once and released from the
  GPU in batches (`retire` moves the generation every 256 retirements). A chunk's hash covers its
  cells plus the one-cell ring around it, because Rule Tiles look across chunk borders.
- **Animated tiles are not baked into chunks**: their sprite changes every few frames, which
  would rebuild (and leak a GPU mesh per frame of) the chunk. They draw as their own quads
  (instanced like sprites), so only those cells cost per frame.
- **Collider merge** is greedy: horizontal runs per row, then runs with the same x span stacked
  vertically, emitted as one compound collider of cuboids (one rapier collider per tilemap).
  Long ground tops become one box, which also avoids the player catching on cell seams. Merging is
  skipped with a cell gap (it would close the gaps); isometric cells are convex diamonds, unmerged.
  Collider Type Sprite uses the sprite's rect; a sprite that fills the cell counts as a full cell
  so it merges.
- **Runtime tile edits** (Lua SetTile while playing): `Physics2D::refresh_tilemaps` compares a
  content signature before each fixed step and rebuilds only that tilemap's collider, waking
  bodies so nothing keeps resting on removed tiles.
- **2D lighting is per pixel in the sprite shader, not a separate light texture pass.**
  Sprites/tiles carry a `lit_2d` material flag (Sprite-Lit-Default); the draw's Sorting Layer bit
  goes in the previously reserved `extra.w` of the per-draw uniform; `fs_unlit` multiplies the
  colour by the sum of the 2D lights read from one storage buffer (mesh group binding 30). A
  separate screen-space light texture would need new render targets and a pass ordering change
  in a renderer shared by many features; the per-pixel loop over ≤16 lights gives the same result
  for Multiply blending and keeps Sorting Layer targeting exact per draw. With no 2D light in the
  scene the light count is 0 and sprites stay unlit exactly as before (only the 16-byte header is
  uploaded).
- **Shadows are CPU polar maps** (256 directions per shadowed light, up to 16 lights): for each
  direction, where the nearest caster's shadow starts. Casters are convex outlines (sprite rects,
  merged tilemap boxes); each caster only fills the bins of its angular span, so cost is
  casters × covered bins. Without Self Shadows the shadow starts at the caster's far side (the
  caster itself stays lit, like the reference default); with it, at the near side. The shader
  blends a few neighbouring bins for soft edges. A light inside a caster's bounds ignores that
  caster.
- **Falloff**: `pow((outer − d) / (outer − inner), 0.5 + 2.5 × falloff strength)`; spot cones fade
  with a smoothstep between the inner and outer half angles; spots point along the object's up
  axis. Lights sort globals first, then brightest; past 16 the rest are dropped.
- **Target Sorting Layers**: None = Everything (the default), else a list; stored as a mask of
  layer values −16..15. Sorting Layers stay process-wide, so the demo uses Order in Layer on
  Default rather than adding project layers (ProjectSettings are shared with other work).
- **Painting UX**: tools only take the Scene view's left button while the Tile Palette window is
  open and a Tilemap exists (the Hand tool and Alt keep camera control). The stroke records its
  undo snapshot just before its first real change, so clicking on identical tiles adds no step.
  The active tilemap is the one chosen in the dropdown, else the selected Tilemap, else the first.
- **Isometric** layout is supported for cell math, painting, rendering and colliders; hexagon
  layouts are not implemented.

Not done / later:
- Freeform and Sprite 2D lights, normal maps for 2D lights, light blend styles other than
  Multiply, shadow volume intensity.
- Hexagonal grids, tilemap sorting per tile in Chunk mode (chunks sort by their object), the Tile
  Palette's own grid editing tools (move/erase inside the palette beyond right-click remove).
- GameObject brushes and tile "random brush" variants.
