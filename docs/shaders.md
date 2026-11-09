# Writing shaders

Misty materials can use your own shaders. A shader is a `.shader` file in `Assets`, written in
WGSL. It has a short comment header that declares its properties and render state, then one
function. The engine wraps that function in its own pipeline. A **surface shader** only says what
the surface looks like (colour, normal, gloss, glow), and the engine lights it with every light,
shadow, ambient and sky reflection and the fog. This works like surface shaders in the familiar
editor.

Create one with **Assets ▸ Create ▸ Shader ▸ Surface Shader** (or **Unlit Shader**). Select it and
use **Assets ▸ Create ▸ Material from Shader**, or pick it from any material's **Shader** dropdown.
The material's Properties panel then shows one field for each property you declared. Values are saved in
the `.mat` file under `"properties"`.

Edits recompile live. While you type in the Code Editor, the shader is checked with naga after a
short pause. Errors show as squiggles on their line, and the scene updates as soon as the shader
compiles. Saving also recompiles it, and so does changing the file in another editor. If a shader
does not compile, objects using it turn **magenta**, and the error (`file.shader:line:column`)
appears in the Console, where you can click it to open the Code Editor.

## A surface shader

```wgsl
// @shader surface
// @property _Color color "Color" = (1, 1, 1, 1)
// @property _MainTex texture2d "Albedo" = white
// @property _Smoothness range(0, 1) = 0.5   // Tooltip shown in the Properties panel

fn surface(input: SurfaceInput) -> SurfaceOutput {
    var o = default_output(input);
    let tex = textureSample(_MainTex, tex_sampler, input.uv);
    o.albedo = material._Color.rgb * tex.rgb;
    o.alpha = material._Color.a * tex.a;
    o.smoothness = material._Smoothness;
    return o;
}
```

`SurfaceOutput` fields:

| Field | Meaning |
|---|---|
| `albedo: vec3<f32>` | Linear base colour |
| `normal: vec3<f32>` | World-space unit normal (`default_output` uses the mesh normal) |
| `metallic`, `smoothness: f32` | Like the Standard shader |
| `emission: vec3<f32>` | Linear emitted light; values above 1 bloom |
| `alpha: f32` | Used when the shader blends |
| `occlusion: f32` | Ambient occlusion, 1 = none |

To discard a pixel (cut-outs), call `discard;` inside `surface`.

### Vertex displacement

Add `fn vertex(v: VertexData) -> VertexData` to move vertices in object space. `VertexData` has
`position`, `normal`, `uv`, `tangent` and `draw`. Shadows use the undisplaced mesh.

```wgsl
fn vertex(v: VertexData) -> VertexData {
    var o = v;
    o.position += v.normal * sin(globals.time.x * 3.0 + v.position.y * 8.0) * 0.05;
    return o;
}
```

## An unlit shader

With `// @shader unlit`, write `fn fragment(input: SurfaceInput) -> vec4<f32>` and return the final
linear colour and alpha. No lighting is applied. Call `apply_fog(color, input.world_pos)` if you
want fog: it applies the Lighting window's fog, then the volumetric / height fog and the clouds' haze of
the Post-process Volume, like the built-in shaders.

## Header syntax

Header lines are comments that start with `// @`. They can go anywhere, but normally sit at the
top of the file.

| Line | Values | Default |
|---|---|---|
| `// @shader surface` | `surface` (lit) or `unlit` | `surface` |
| `// @blend alpha` | `off`, `alpha` (Fade), `premultiply` (Transparent: reflections stay), `additive` | `off` |
| `// @cull off` | `back`, `front`, `off` (both sides; back faces get flipped normals) | `back` |
| `// @zwrite off` | `on`, `off` | `on` when opaque, `off` when blended |
| `// @queue transparent+10` | `background` 1000, `geometry` 2000, `alphatest` 2450, `transparent` 3000, `overlay` 4000, or a number, with an optional `+n`/`-n` | 2000 when opaque, 3000 when blended |
| `// @property NAME TYPE ["Label"] = DEFAULT // tooltip` | see below | |

Property types:

| Type | WGSL | Properties | Default value |
|---|---|---|---|
| `float` | `material.NAME: f32` | number field | a number (0) |
| `range(min, max)` (or `float range(…)`) | `f32` | slider | a number (min) |
| `color` | `vec4<f32>`, **linear** RGB + alpha | colour picker (sRGB) | `(r, g, b[, a])`, alpha 1 |
| `vector` | `vec4<f32>` | four numbers | `(x, y[, z, w])`, the rest 0 |
| `texture2d` (or `texture`) | `NAME: texture_2d<f32>` | texture slot | `white`, `black`, `grey`, `normal` |

Textures are sampled with `tex_sampler` (bilinear, repeat, mipmapped, anisotropic). Colour
textures are decoded from sRGB. A texture whose default is `normal` is read as linear data and
fits `unpack_normal`.

Property names must be valid WGSL identifiers (`_Color` is fine) and must be unique. If a
material has no value for a property, the property's default is used, so adding a property
never breaks existing materials.

## Built-in inputs and helpers

`SurfaceInput` (passed to `surface` and `fragment`):

| Field | |
|---|---|
| `world_pos`, `world_normal`, `world_tangent` | World space; the normal is flipped on back faces |
| `uv` | Mesh UV |
| `view_dir` | Unit vector from the surface to the camera |
| `screen_uv`, `screen_pos`, `depth` | 0..1 across the view (top left = 0), pixels, device depth |
| `front_facing`, `draw`, `time` | `draws[input.draw].model` is the object matrix |

Globals you can use anywhere:

- `globals.time`: x = seconds (follows `Time.time` in Play mode), y = sin, z = cos, w = frame delta.
- `globals.screen`: x, y = view size in pixels, zw = 1 / size.
- `view.view_proj`, `view.inv_view_proj`, `view.camera_pos` (xyz), `view.sun_dir`,
  `view.sun_color`, `view.fog_color`.
- `object_to_world(input)`, `tangent_to_world(input, n)`, `unpack_normal(sample, scale)`.
- `sky_color(dir)`, `apply_fog(color, world)`, `srgb_to_linear(c)`.
- **Opaque scene** (shaders in queue 3000 or higher): `scene_color_at(uv)` (linear HDR),
  `scene_depth_at(uv)`, `scene_world_pos(uv)`, `scene_distance(uv)`. Use them for refraction,
  soft edges and contact glows. Shaders in the opaque queue read a black colour and an empty
  depth.

## Batching

Objects that share a mesh and a material are drawn with one instanced call, the same as built-in
materials. Two materials with the same shader and the same values also share a bind group.

## Examples

`SampleProject/Assets/Shaders` has **Hologram** (lit, alpha blended, scrolling scan lines, rim
glow), **Wobble** (vertex displacement), **Scroll Gradient** (unlit) and **Force Field** (reads the
scene for distortion and contact glow). They are shown in `Assets/Scenes/ShaderDemo.scene`, and a
hologram beacon also appears in `WaterDemo`.

## Shader Graph

A **Shader Graph** (`.shadergraph`) builds a shader from nodes instead of WGSL. Create one with
**Assets ▸ Create ▸ Shader Graph ▸ Lit Shader Graph** (or **Unlit Shader Graph**) and double-click
it to open the **Shader Graph** window (also under **Window ▸ Rendering**). A graph is just another
shader asset: it shows up in every material's **Shader** dropdown and in **Material from Shader**,
and its Blackboard properties appear in the material's Properties panel like `@property` lines do.

- **Canvas**: wheel zooms, middle mouse or Alt+drag pans, F frames everything. Drag nodes to move
  them, click or drag a rubber band to select, Delete removes, Ctrl+D duplicates.
- **Create Node**: Space, or right-click ▸ Create Node, opens a search. Dragging a wire from a port
  and dropping it on empty canvas opens the same search and connects the new node.
- **Connections**: drag from an output to an input. Vectors convert implicitly (a float fills every
  component, a wider vector is truncated, a narrower one is padded with zeros); textures only go
  into texture inputs, and connections that would make a loop are refused. Drag a connected input
  to pick its wire up again; Alt+click a port to disconnect it. *Dynamic* ports (Add, Multiply,
  Lerp, …) take the widest vector connected to them.
- **Unconnected inputs** show their value in small fields left of the node (or the binding they
  read, such as `UV0` or `World Normal`).
- **Previews**: each node shows its first output on a small square (or a sphere for nodes reading
  normals, positions or the view direction), evaluated live on the CPU. The **Main Preview** shows
  the graph's material on a sphere, cube or plane (drag to rotate) with unsaved changes.
- **Blackboard**: "+" adds a Float, Range, Color, Vector, Texture2D or Boolean property. Drag one
  onto the canvas for a Property node. Select it to edit its label, reference name (`_Speed`, used
  in `.mat` files), default value and options.
- **Graph Settings**: Lit or Unlit, Opaque or Transparent (Lit uses premultiplied blending so
  reflections stay, Unlit uses alpha blending), Alpha Clipping (discards pixels whose Alpha is
  below the Alpha Clip Threshold) and Two Sided.
- **Master stack**: Position Offset (object space, vertex stage), then Base Color, Normal
  (tangent space), Metallic, Smoothness, Emission, Alpha and Alpha Clip Threshold. Unlit graphs
  output Base Color + Emission.
- **Undo/redo** (Ctrl+Z / Ctrl+Y) is the window's own. Nothing reaches the file, or the scene,
  until **Save Asset** (Ctrl+S).
- **Errors** (an unconnected texture input, a missing property, a WGSL problem) are shown on the
  offending node with a red badge, and in the toolbar.

Nodes: **Input** (Property, Color, Float, Vector 2/3/4, Time, UV, Position, Normal, View Direction,
Vertex Color, Screen Position), **Math** (Add, Subtract, Multiply, Divide, Power, Minimum, Maximum,
Lerp, Clamp, Saturate, One Minus, Negate, Remap, Step, Smoothstep, Sine, Cosine, Absolute, Floor,
Fraction, Dot, Cross, Normalize, Length, Split, Combine), **Texture** (Sample Texture 2D, its Normal
type, Tiling And Offset, Normal From Texture, Normal Strength), **Procedural** (Gradient Noise,
Voronoi, Checkerboard, Fresnel Effect) and **Utility** (Preview, sticky notes). Meshes have no
vertex colours yet, so Vertex Color reads white.

The samples `Assets/Shaders/Dissolve.shadergraph`, `Hologram.shadergraph` and
`Water Ripple.shadergraph` are shown in `Assets/Scenes/ShaderGraphDemo.scene`.

Shader headers also accept `// @property NAME toggle = 1`: a checkbox whose value the shader reads
as 0 or 1 (Shader Graph Booleans use it).
