# Round 12 — Mesh Builder: in-editor mesh modelling (branch `r12-meshbuilder`)

## Decisions

1. **Name: Mesh Builder; component: Editable Mesh.** Same behaviour as the reference engine's
   in-editor modelling tool, own name and code.
2. **Face list, not half-edge.** `EditableMesh` stores shared positions plus polygons (any
   corner count) with a material index, a smoothing group and auto-UV settings per face. Edge
   and vertex adjacency is derived on demand (`edge_faces`, `vertex_faces`). Every operation
   rebuilds the few faces it touches and keeps winding consistent (each new face uses shared
   edges opposite to its neighbour), which the tests check with `is_manifold`/`is_closed`/
   signed volume. A half-edge structure would make walking cheaper but every operation and
   the file format much more complicated; meshes modelled by hand stay small.
3. **Saved in the scene, not a `.mesh` asset.** The component is serialised with the object
   like any other component: whole-scene undo snapshots cover every operation for free,
   prefabs/duplicates/copy-paste work unchanged, and each object is edited independently (the
   reference tool also keeps its meshes per object). Sharing one mesh between objects is what
   Export to Model is for.
4. **Baked into the runtime mesh cache under a content hash.** `mesh_builder::sync` bakes each
   Editable Mesh (per-corner vertices, flat or smoothing-group normals, projected UVs, ear-clipped
   polygons, tangents) and registers it as `runtime://editmesh/<hash>`; the Mesh Filter points
   at `…#0`. So the renderer, picking bounds, Mesh Colliders, static batching and wireframes
   need no changes. `sync` runs at the start of every `DrawCache::update` (editor views and the
   player) and after scene/prefab loading (`spawn_entries`), so colliders exist before the
   first frame. Same content → same key: saved files are deterministic. Old bakes are evicted
   past 4096 entries (re-baked if still used). Registering bumps the model cache generation,
   so a vertex drag re-uploads imported GPU meshes each frame — acceptable for editing.
5. **Per-face materials as submeshes.** Bake output mesh 0 holds every face, mesh `k + 1` the
   faces of material `k`. `DrawCache::build` draws Materials element `i` with `#(i+1)` for baked
   Editable Mesh keys only; any other mesh behaves as before. Set Material grows the Mesh
   Renderer's Materials list as needed.
6. **Shapes sit on the ground.** Generated shapes have their pivot at the centre of the base,
   so a New Shape drops onto the floor at the Scene view pivot. Converting a primitive keeps
   the primitive's own (centred) placement.
7. **Auto UVs.** Each face is projected onto the axis plane closest to its normal, 1 UV unit per
   metre (so textures tile evenly), then rotated, tiled and offset. Object space is anchored at
   the face's corner; World Space projects world positions (the bake then hashes the object's
   matrix too) so faces and separate objects line up.
8. **Handles reuse the Transform handles.** `handles.rs` asks `mesh_builder::handle_pivot` for
   the frame and routes translate/rotate/scale through `mesh_builder::drag` (applied to a copy
   of the mesh taken at drag start), so snapping, local/global and the undo grouping are shared.
9. **Edit modes and shortcuts.** Strip under the tools row (only while an Editable Mesh is
   selected): Object/Vertex/Edge/Face (keys 1–4), an Edit ▾ menu with every operation and its
   shortcut, and + for New Shape. Shortcuts: Ctrl+E Extrude, Alt+I Inset, Alt+Shift+B Bevel,
   Alt+S Subdivide, Alt+E Connect, Alt+B Bridge, Alt+C Merge, Alt+V Weld, Alt+D Detach,
   Alt+N Flip Normals, Alt+A Select All, Delete/Backspace delete elements (not the object).
   They run before the global shortcuts.
10. **Operation scope.** Extrude faces extrudes the selection as one region (shared edges stay
    joined, flat regions stay flat); edge extrude works on open edges in the face's plane.
    Inset is per face, clamped before the centre. Bevel chamfers edges shared by two faces
    (a corner triangle where more than three faces meet); edges whose faces an earlier edge in
    the same bevel already changed are skipped. Subdivide/Connect insert the new midpoint into
    unselected neighbours so no cracks appear. Connect with one edge is an edge loop cut;
    with several, it joins midpoints in faces holding exactly two of them.
11. **Export to Model** writes `Assets/Models/<Name>.obj` (one object per material, mirrored
    to the file's right-handed space like the importer expects) and swaps the Editable Mesh
    for that model: the object keeps mesh #0, extra materials become child objects (a model
    mesh has one material). One undo step (the file stays).
12. **MeshBuilderDemo** is generated in code (`sample_mesh_builder`): every part is built
    with the same operations the tools run (loop cuts, inset, extrude, delete, weld, bevel,
    set material) and the test checks every part is closed, outward and uses both materials.

## Progress

- Engine `mesh_builder`: `EditableMesh` component (Mesh category, requires Mesh Filter and
  Mesh Renderer), bake with smoothing groups and per-face UVs, ear-clipping triangulation,
  runtime cache registration and `sync`, submesh drawing, shapes (Cube, Plane, Cylinder,
  Stairs, Arch, Prism, Torus), operations (extrude faces/edges, inset, bevel, subdivide, loop
  cut, connect, bridge, merge, weld, delete, flip, detach, set material/smoothing, centre
  pivot, transform), conversion from primitives/triangle meshes, OBJ export.
- Editor: Scene view strip, element picking (click, Shift add, Ctrl toggle, box drag of
  visible elements), edge/face highlighting and vertex dots, Transform handles on elements,
  Edit menu, shortcuts, New Shape window (also GameObject ▸ 3D Object ▸ Mesh Builder Shape…),
  Editable Mesh Inspector (counts, modes, amounts, operation buttons, selected faces' material,
  smoothing, World Space UVs, offset, tiling, rotation), Mesh Filter ▸ Convert to Editable
  Mesh, Center Pivot, Freeze Transform, Export to Model; `MISTY_MESH_MODE=face[:i,j]` hook.
- Sample: `Scenes/MeshBuilderDemo.scene` and materials Builder Plaster / Builder Stone /
  Builder Grass.
- Tests: shapes closed/outward/triangulation covers each polygon, volumes, bake and submeshes,
  UVs, every operation's counts and manifoldness, window cut, scene round trip, draws per
  element, OBJ round trip; editor: each operation is one undo step (undo + redo), material
  slots, bridge, freeze, selection after undo, New Shape, convert, export; kittest: strip
  modes and menu, keys, face click + Shift, box select of visible vertices, face move handle.

## Left

- Bevel of edges sharing a vertex in one go (later edges are skipped), bevel segments.
- UV editor window (manual unwrap), vertex colours, select loop/ring, soft selection.
- Export writes OBJ only (no `.mesh` asset); undo does not remove the written file.
