# UI round: Hierarchy and Project trees (branch ui-trees)

## Progress
- Hierarchy: alternating row stripes, soft "folder" tint and semi-bold brighter names on
  parents, filled ▼/▶ triangles, file-explorer connection lines (├ / └, vertical runs from
  each open parent). Rows are 20 px with no gap; indent 16 px.
- Project folder tree (new `panels/project_tree.rs`): ▼/▶ foldouts for folders with
  subfolders, the same lines, stripes and parent tint, 22 px rows. Assets starts open;
  opening a folder opens the folders above it.
- Hierarchy Folders: GameObject ▸ Create Folder (also in the Hierarchy "+" and right-click
  menus, and the Ctrl+K palette). Folder icon, always the folder tint, Alt+right-click opens a
  pastel colour wheel with a None reset; the colour tints the row and icon. Undoable.
- Object right-click menu: Move To View, Align With View, Align View to Selected.
- Capture hooks: `MISTY_EXPAND_ALL=1`, `MISTY_FOLDER_DEMO=1|picker`.
- Screenshots: docs/screenshots/ui-round/trees_hierarchy.png, trees_folder_colors.png,
  trees_folder_picker.png.

## Decisions
- One shared module `tree_lines.rs` holds the guide computation (`guides`, a pure function of
  row depths, `None` = scene header break), the painters (lines, triangles, background) and
  the colours. Reason: both panels look the same and the logic is unit-tested in one place.
- Colours are translucent overlays (stripe, parent tint, hover, lines), not fixed greys.
  Reason: the theme is being reworked in parallel; overlays keep the contrast on any panel
  colour without touching theme.rs.
- Hover became an overlay too, so a hovered parent still reads lighter than a hovered leaf.
- Connection lines start at depth 1 (children of an object); top-level objects do not connect
  to their scene header. Reason: that would need a deeper indent for every root row.
- While searching the list is flat, so no lines and no triangles (as before).
- Rows in the Hierarchy have no vertical item spacing so stripes and lines are continuous; the
  virtual-scroll step reads the spacing, so it stays correct.
- `HierarchyFolder { color: Option<[f32; 3]> }` (sRGB 0..1) lives in misty_engine
  `scene/hierarchy_folder.rs`, registered as "Hierarchy Folder" with no Add Component
  category. It is inserted without an entry in the component order, so the Inspector does not
  list it (like the Prefab link) while scene files, prefabs and undo snapshots still carry it
  (they serialise every registered component). Old scenes load unchanged (absent = not a
  folder).
- The colour wheel is limited to saturation 0.58 and lightness 0.72 (rim) to 0.88 (inner
  edge), so every pick is a soft pastel; the row wash is the colour at 22 % so text stays
  readable. Alt+right-click replaces the context menu only on folder rows.
- Align/Move To View reuse the existing GameObject-menu actions (already undoable); "Create
  Folder" is one line in `create_menu_items` (menu_bar.rs), shared by the GameObject menu and
  both Hierarchy menus.
