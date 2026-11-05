# Round 12: Shader Graph

## Decisions

1. **A graph is a shader asset.** `.shadergraph` files are JSON graphs; `shader_asset::load` generates
   the `.shader` text from them (`shader_graph::generate`) and parses it like a hand-written shader.
   `is_shader_path` accepts both extensions, so materials, the Shader dropdown, Material from Shader,
   batching, hot reload and the shader_material Inspector work for graphs without new code paths.
   Reason: one compile path and one material model; graphs get every renderer feature custom shaders
   have (lighting, fog, transparency queues) for free.
2. **Per-line node map for errors.** The generator emits each node's code on its own `let` lines and
   remembers which node produced each line; a naga error's line maps back to the node, which gets a
   red badge. Graph-level problems (unconnected texture input, missing property, loops) are checked
   before WGSL and carry their node directly.
3. **Typed float literals (`1.0f`).** naga (this version) rejects builtins on all-abstract operands
   outside constants (`mix(0.0, 1.0, 0.5)`), so generated constants are concrete f32.
4. **Implicit conversions like the reference editor**: floats splat, wider vectors truncate,
   narrower vectors pad with zeros; textures only connect to textures. Dynamic ports resolve to the
   widest connected dynamic input (float when nothing is connected). Codegen and the CPU evaluator
   share the same rules (tested).
5. **Ports identified by name in the file** (`"Base Color"`, `"UV"`), nodes by a `"type"` tag with
   their settings inline, unconnected values only stored when changed. Reason: readable diffs, and
   adding ports or nodes later keeps old graphs loading.
6. **Master stack is a node (id 0)** that cannot be deleted; which blocks show depends on the target
   (Unlit hides Normal/Metallic/Smoothness, Alpha Clip Threshold only with Alpha Clipping). Hidden
   blocks keep their connections. Position Offset is an object-space offset (the task asked for an
   offset), Normal is tangent space.
7. **Vertex stage generated separately**: nodes feeding Position Offset are emitted again inside
   `fn vertex` with vertex-stage inputs (`v.uv`, world position from the draw matrix,
   `textureSampleLevel` instead of `textureSample`).
8. **Object-space position/normal in the fragment stage** are reconstructed from the draw matrix
   (projection onto its axes; exact for rotation + scale, no shear) since the wrapper has no inverse
   model matrix.
9. **Node previews on the CPU** (`shader_graph::eval`, 64×64): no pipeline per node, instant, works
   headless and in kittests. The hash/noise functions are integer-based so CPU and GPU agree. Nodes
   reading normals/positions/view direction (or fed by them) preview on a sphere, others on a UV
   square. Re-evaluated when the graph's content changes (positions excluded) and at 10 Hz when it
   reads Time.
10. **Main preview renders the unsaved graph** under a private path
    (`Library/ShaderGraphPreview/<name>.shadergraph`, never a file) through `set_source`; the scene and
    materials only change on **Save Asset**, like the reference editor. Material previews now key on
    the shader's load generation, so a recompiled shader re-renders the Inspector preview too.
11. **Window-local undo/redo** (snapshots of the graph, drags/typing grouped into one step) instead of
    the scene undo stack: graph edits are not scene edits and must not interleave with them. While the
    window has focus the editor's global Delete/Ctrl+Z/Y/S/D and tool keys step aside.
12. **Boolean properties** became a shader header type (`// @property _X toggle = 1`, an f32 0/1 with
    a checkbox in the Inspector), small and additive in `shader_asset`.
13. **Sample Texture 2D without a connected texture is an error** (no hidden auto-properties):
    textures come from Blackboard Texture2D properties, so materials control them.
14. Reserved names (WGSL keywords, prelude globals, `sg_` helpers) are refused as property references.

## Progress

- Engine: `misty_engine::shader_graph` (`mod.rs` model + editing, `nodes.rs` catalog of 50 nodes,
  `codegen.rs`, `eval.rs`, tests); `shader_asset` loads graphs, `toggle` property kind.
- Editor: `panels/shader_graph` (canvas, search, blackboard, previews, window + tests), Tab
  `Shader Graph` (Window ▸ Rendering), double-click opens, Assets ▸ Create ▸ Shader Graph ▸
  Lit/Unlit, Plane shape + graph slot in `material_preview`, `MISTY_SHADERGRAPH` capture hook.
- Samples: `Dissolve`, `Hologram`, `Water Ripple` graphs + materials and `ShaderGraphDemo.scene`
  (`sample_shader_graph.rs`, generator + match/compile/headless-render tests).
- Docs: Shader Graph section in `docs/shaders.md`.

## Left / ideas

- No Save prompt when closing the tab or switching graphs with unsaved changes (the working copy
  stays in the window while the tab is closed; opening another graph discards it).
- No copy/paste between graphs, no groups, no sub-graphs, no custom function node, no keywords.
- Normals are not recomputed after Position Offset (the Water Ripple colours by height instead).
- Meshes have no vertex colours yet (Vertex Color reads white).
