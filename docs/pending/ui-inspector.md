# UI round: Inspector cards and stroke-free theme (branch `ui-inspector`)

## Progress
- Inspector components are rounded cards (6 px radius, 5 px gap): a header bar with fold arrow,
  enable checkbox, icon, **bold** name, and the preset / ⋮ buttons on the right. Clicking the
  title folds the card; right-click still opens the component menu. Scripts and the inline
  material editor use the same card.
- One aligned label / value layout for the whole Inspector: labels in regular weight on the
  left (about 40 % of the width, truncated with a tooltip), values filling the rest; rows are
  22 px. Transform's X/Y/Z rows and nested grids line up on the same value column.
- Sub-sections inside components are bold sub-headers (Light ▸ Shadow Settings / Volumetrics,
  NavMesh Agent ▸ Steering, Probe ▸ Runtime Settings, Material ▸ Main Maps, ...); foldouts
  (Halo, Constraints, Spring, post-process overrides, ...) have bold titles.
- Theme without black outlines: buttons, combo boxes, fields, windows, tabs and panel splits
  are separated by soft shade differences instead of strokes; focus and selection keep the
  accent colour.
- Screenshots: docs/screenshots/ui-round/inspector_light.png, inspector_volume.png,
  inspector_mesh.png (LightingDemo).

## Decisions
- **Card + grid helpers live in `panels/inspector/card.rs`**: `card()` (component card),
  `grid()` / `PropertyGrid` (drop-in for the old `Grid::new(..).num_columns(2)...` chains, so
  every editor got the aligned layout without hand edits), `sub_header()` / `grid_sub_header()`.
  Reason: the user asked for one consistent look; central helpers keep editors unchanged.
- **The value column is an x position stored per frame in egui temp data**
  (`widgets::set_value_column`), set by each card body and cleared when the Inspector is done.
  Reason: nested and indented grids (overrides, post-process sections) then still align with
  the top-level rows, and panels outside the Inspector keep their old widths.
- **Fields fill the value column through spacing, not per widget**: inside a property grid the
  combo, text-edit and slider widths are set to the value width; number fields keep a compact
  width (30 % of the column, 48–76 px) because a slider's number box uses the same size.
  Hard-coded `.width(150.0)` on Inspector combo boxes was removed so they fill too. Colour
  swatches fill the remaining width (`card::wide`). Texture slots take 60 % so the colour /
  strength on the same row fits.
- **Real bold**: egui's `.strong()` only brightens the colour, so the Segoe UI Bold face is
  loaded as a `bold` font family (`theme::bold`); without it the regular font is used.
- **Strokes**: widget `bg_stroke` is `NONE` for inactive / hovered / active / open; buttons are
  a lighter grey than panels, fields darker, tool and tab bars darker. `noninteractive.bg_stroke`
  (separators, group frames, panel split lines) is a soft grey close to the panel colour instead
  of near-black; `BORDER` (dock separators and gaps) is a soft dark grey. Popups / menus use a
  slightly lighter fill and no outline. Keyboard focus and text-edit focus use the accent blue.
