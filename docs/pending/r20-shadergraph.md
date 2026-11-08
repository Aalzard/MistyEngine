# Shader Graph depth: production graphs (round 20D, branch r20-shadergraph)

Goal: the Shader Graph good enough for production use, at parity with the reference engine's
Shader Graph: Sub Graphs, keywords compiled as variants, a Custom Function node, groups and
sticky notes, copy / paste between graphs, an unsaved-changes prompt, vertex colour and vertex
offset inputs, a master stack for the full lit material model and a Decal target, example graphs,
and compile errors on the node that caused them. Behaviour follows the reference; names, code and
data are ours.

## What works

### Graph features

| Feature | How it behaves | Where |
|---|---|---|
| Sub Graphs | `.shadersubgraph` assets (Assets ▸ Create ▸ Shader Graph ▸ Sub Graph; double-click opens them in the Shader Graph window). Their Blackboard properties are the inputs (port name = display name, default = the property's default; texture inputs must be connected), their Output node's slots the outputs (Graph Settings becomes "Sub Graph Outputs": name, type, add, remove). A Sub Graph node caches the interface and refreshes it from the asset when a graph opens, on paste and with Node Settings ▸ Refresh; connections to ports that went away are dropped. Code generation inlines the Sub Graph's nodes (prefixed with the node's id, only the nodes its outputs need). A Sub Graph inside itself (at any depth, max 8) is an error. The node previews evaluate Sub Graphs on the CPU too. Its own main preview shows the first output unlit. The Create Node search lists the project's Sub Graphs. | `shader_graph/codegen.rs` (`sub_graph_node`), `ShaderGraph::sub_graph_interface` / `refresh_sub_graphs`, `eval.rs`, `panels/shader_graph/search.rs` |
| Keywords | Blackboard ▸ + ▸ Boolean Keyword / Enum Keyword (reference name like `_DETAIL_ON`, display name, default, Enum entries). A Keyword node has one input per value (Boolean: On, Off) and a dynamic output. Each combination of values is its own variant, generated with only the nodes that variant reads (the Keyword node passes its selected input). The variant count is capped at 128 (the reference's default limit); past it the graph errors on a Keyword node. Materials store a keyword's value under its reference like a property (Inspector: a checkbox or a dropdown); the material picks the variant, the renderer compiles each variant once (`path#_A=1`). The toolbar shows "N variants (limit 128)". Keywords of Sub Graphs are added to the shader's. Hand-written `.shader` files get keywords too: `// @keyword _A toggle = 0` / `enum(Low, High) = 1` with `#if _A`, `#if _Q == High`, `#elif`, `#else`, `#endif` lines (blanked per variant, line numbers kept). | `Keyword`, `variant_key`, `codegen::needed` / `keyword_node`, `shader_asset/parse.rs` (`preprocess`), `shader_asset/library.rs` (`load_variant`), `render/custom_shader` (`CustomMaterial::variant`) |
| Custom Function | Utility ▸ Custom Function. Node Settings: name, Type String (inline body) or File (a `.wgsl` in the project), inputs (any type, textures included) and outputs (any non-texture type). Inline: the body goes into `fn sg_fn_<name>_<hash>(inputs) -> struct of outputs` with each output a `var`, so it reads inputs and assigns outputs by name; identical functions are emitted once. File: the file is included once and `name(inputs…)` is called; it returns the output (one) or a struct with fields named after the outputs. Names must be WGSL identifiers. WGSL errors in the body or file are reported on the node. The file is a dependency: editing it hot-reloads materials. No CPU node preview (WGSL). | `NodeKind::CustomFunction`, `codegen::collect_functions`, `panels/shader_graph/node_settings.rs` |
| Groups | Select nodes ▸ right-click ▸ Group Selection (Ctrl+G). A tinted frame around its nodes with a title bar: drag it to move the group, click it to select the members, double-click to rename; right-click ▸ Rename / Ungroup; a node's menu has Remove From Group. A node is in at most one group; deleting nodes leaves the rest grouped, an empty group goes. | `Group`, `ShaderGraph::group` / `ungroup`, `canvas.rs` (`groups`) |
| Sticky notes | As before (right-click ▸ Create Sticky Note); the examples use them. | `canvas.rs` (`notes`) |
| Copy, Cut, Paste, Duplicate | Ctrl+C / Ctrl+X / Ctrl+V / Ctrl+D and the node and canvas menus. The clipboard keeps the nodes, the connections among them, their groups and the properties and keywords they read; it outlives the open graph and goes to the system clipboard as text, so nodes paste into another graph (or another editor). Pasting adds the properties and keywords the target graph lacks (one of the same name and type is reused; a same-named one of another type stays and the copy is renamed). Repeated pastes step along; canvas ▸ Paste puts the nodes at the click. One undo step each. | `Clipboard`, `ShaderGraph::copy` / `paste`, `ShaderGraphWindow::copy_selected` / `paste` |
| Unsaved changes | Closing the Shader Graph tab, opening another graph or quitting the editor with unsaved changes asks "Save changes?" (Save, Don't Save, Cancel). Don't Save goes back to the file's graph. | `AfterPrompt`, `may_close_tab`, `cancel_close`, `show_windows`, `PanelViewer::on_close`, `app.rs` |
| Errors on nodes | Every error that has a cause is on its node (red outline, a "!" badge with the message): missing properties or keywords, bad Custom Function names, WGSL errors in a Custom Function or a Sub Graph (on the Sub Graph node, "in <name>: …"), the variant cap (on a Keyword node), loops, type mismatches, unconnected texture inputs, object-space nodes in a Decal graph. | `codegen.rs` (`GraphError`, line → node map, `Gen::blame`) |

### Inputs and master stack

| Feature | How it behaves | Where |
|---|---|---|
| Vertex colour | Meshes carry vertex colours (`MeshData::colors`, linear RGBA8): glTF `COLOR_0` is imported (kept through smoothing remaps and lightmap unwraps); meshes without colours read white. `.shader` and Shader Graph pipelines take a second vertex buffer (location 5); the renderer binds the mesh's colours or a shared white buffer. The Vertex Color node reads them in both stages (`v.color`, `SurfaceInput.vertex_color`), so a vertex offset can be weighted by colour. Built-in shaders are untouched. | `render/mesh.rs`, `models/gltf_import.rs`, `render/mod.rs` (`GpuMesh::colors`, `white_colors`), `custom_shader/wrap.rs` (`CustomOut`) |
| Vertex position offset | The Vertex block's Position Offset (object space), as before, now with vertex colour available. | `codegen.rs` |
| Lit master stack | Base Color, Normal, Metallic, Smoothness, Emission, **Ambient Occlusion**, **Transmission** (thin-surface light from behind, the lit shader's transmission term), Alpha, Alpha Clip Threshold; Graph Settings ▸ **Normal Space** Tangent / Object / World. The 17D material-model inputs (clear coat, anisotropy, detail maps, parallax) are not in main yet, so there was nothing more to expose: the stack covers every input `shade()` takes. | `nodes.rs` (`MASTER_PORTS`), `prelude.wgsl` (`SurfaceOutput::transmission`) |
| Decal target | Graph Settings ▸ Material ▸ Decal (or Create ▸ Decal Shader Graph). Ports: Base Color, Alpha, Normal (tangent, decal X right / Y up), Metallic, Smoothness, Ambient Occlusion, Emission. A material using it goes on a Decal Projector (new Material field): the decal pass compiles the graph's function (per keyword variant, up to 8 properties) into its compute shader and calls it per pixel instead of sampling the projector's maps; the projector's opacity, fades, Affects flags, normal strength and priority still apply. Texture properties join the decal texture array; inputs are the decal UV (tiling and offset), world position and normal, view direction, screen UV and time. The pass is rebuilt when the set of graph variants in view changes; a graph that fails to compile is skipped with a warning. Decals without a material are unchanged. | `codegen::generate_decal`, `render/decals/mod.rs` (`GraphAssets`, `GraphPass`, `module_source`), `decals.wgsl` (`DecalInput`, `DecalOutput`, `dc_graph`) |
| Triplanar node | Texture ▸ Triplanar: a texture projected along the world (or object) axes, blended by the normal (Tile, Blend); CPU preview too. | `nodes.rs`, `codegen.rs`, `eval.rs` |

### Examples (`SampleProject/Assets/Examples/Shader Graph`)

| Graph | What it shows |
|---|---|
| Dissolve Glow (+ Dissolve Edge Sub Graph) | Two keywords (Pattern: Gradient / Cells, Animate: On / Off, 4 variants), the Sub Graph for alpha and the glowing edge, groups and a sticky note. Two materials (default, and Cells + Animate with a blue edge). |
| Triplanar Rock | Rock and moss textures projected with Triplanar nodes, moss on upward faces through an inline Custom Function. |
| Wind Sway | Grass blades (a small glTF with vertex colours) swaying by vertex colour red in the Vertex block, coloured root to tip, Transmission. |
| Puddle Decal | Decal target: an irregular wet patch with a ripple normal behind a keyword (an inline Custom Function). Used by the scene's Decal Projector. |

`ShaderGraphExamples.scene` shows them all. Regenerate with
`cargo test -p misty_editor -- --ignored write_shader_graph_examples`. Tests: the files match
their generator, every variant of every example compiles (from the files on disk), the Dissolve
variants contain only their nodes, the plant imports with vertex colours, and on a GPU the scene
renders without the error shader, the puddle darkens the floor and the blades' tips are lighter
than their roots.

## Tests

- Engine (`shader_graph/feature_tests.rs`): keyword variants with only their nodes, the cap, keys;
  variants through the shader library and materials; inline and file Custom Functions, errors on
  the node, missing files; Sub Graphs inlined with bound inputs, CPU evaluation, errors on the Sub
  Graph node, self-use refused, refresh dropping stale connections; groups and copy / paste across
  graphs with property merging; the master stack per target and normal spaces; vertex colour in
  both stages; Decal graphs (header for materials, the decal function, all variants, object space
  refused). `shader_asset/parse.rs`: `@keyword` parsing and `#if` preprocessing. Decal module
  validates with and without graphs.
- Editor (`panels/shader_graph/feature_tests.rs`): copy in one graph and paste in another (system
  clipboard text and the window clipboard), cut and undo; Ctrl+G groups and dragging a group;
  Blackboard keywords dropped as Keyword nodes and the variant count; Custom Function Node
  Settings and errors on the node; the unsaved-changes prompt for closing the tab, opening another
  graph (Save) and quitting (Cancel); Sub Graph assets opening and appearing in the search; keyword
  fields in the material Inspector picking the variant.

## Not done yet

- Clear coat, anisotropy, detail maps and parallax in the master stack: they arrive with round 17D's
  material model (not merged); the stack exposes everything the current lit shader takes.
- Custom Functions have no CPU node preview (the main preview shows them); per-node GPU previews
  would fix that.
- Shadows and the depth prepass of custom-shader materials still use the built-in variants, so a
  vertex offset does not move shadows (as before this round).
- Sub Graph keywords are merged into the parent's; a parent cannot rename them.
- Decal graphs: at most 8 properties, no vertex colour or object space, errors reported on the
  graph rather than a node; graph decals draw only in views where decals run (the screen-space
  prepass), like texture decals.
- Skinned meshes and FBX / OBJ imports have no vertex colours yet (white).

## Screenshots

`docs/screenshots/r20/`: `shadergraph_dissolve_keywords_subgraph.png` (Dissolve Glow: Keyword
nodes, the Dissolve Edge Sub Graph node, groups, a sticky note, "4 variants"),
`shadergraph_triplanar_rock_custom_function.png`, `shadergraph_wind_sway_vertex_color.png`,
`shadergraph_puddle_decal_target.png`, `shadergraph_sub_graph_outputs.png` (a Sub Graph with its
outputs card) and `shadergraph_examples_scene.png` (ShaderGraphExamples in the Game view).

## Checks

`cargo fmt`, `cargo clippy --workspace --all-targets --release` (0 warnings), `cargo test
--workspace --release --no-fail-fast` (three failures under load, all passing alone:
`opening_a_scene_lifts_a_view_left_under_the_terrain` (known flaky),
`real_udp_sockets_on_localhost`, and the examples render test, since fixed to load its model by
absolute path), golden shots unchanged (all 20 ok).
