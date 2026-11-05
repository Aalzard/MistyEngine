# Round 11 — skinned meshes and blend shapes (branch `r11-skinning`)

## Progress

- **glTF skins import**: `JOINTS_0`/`WEIGHTS_0` (4 influences, weights normalised), morph targets
  (position + normal offsets, names from `mesh.extras.targetNames`, default weights), skins
  (joint nodes, inverse bind matrices, root joint) and animations. Everything is mirrored into
  engine space like the rest of the import (IBMs as `S·M·S`).
- **Skinned Mesh Renderer** component (Mesh category): mesh, materials, root bone, bones list,
  bounds (centre/extent in root bone space), Update When Offscreen, cast/receive shadows and
  blend shape weights (0–100). Dropping a skinned/morphing model makes one object per node,
  the Skinned Mesh Renderer on the mesh node bound to the joint objects, and an Animation on
  the model root playing the extracted clips.
- **GPU skinning**: a compute pass (`render/shaders/skin.wgsl`) applies blend shapes then
  linear blend skinning and writes a vertex buffer in the normal layout; every pass (shadows,
  depth prepass, GPU culling, main, outlines) draws it through the ordinary mesh path.
- **Animation import**: glTF animations become `.anim` clips extracted next to the model
  (`Tentacle_Bend.anim`), driving bones (Position/Rotation/Scale) and blend shape weights
  (`SkinnedMeshRenderer.blendShape.N`) through the existing Animation/Animator system.
  Animation window ▸ Add Property lists the blend shapes.
- **Lua**: `GetComponent("SkinnedMeshRenderer")` with `GetBlendShapeWeight(i)`,
  `SetBlendShapeWeight(i, w)`, `GetBlendShapeIndex(name)`, `GetBlendShapeName(i)`,
  `blendShapeCount`, `enabled`, `updateWhenOffscreen`, `rootBone`.
- **Inspector**: card with bounds, Update When Offscreen, mesh, root bone, bones list, bold
  "BlendShapes" sub-header with a 0–100 slider per shape, Materials and Lighting.
- **Gizmos**: the selected renderer's skeleton (bone lines + joint dots) painted over the Scene
  view so it shows through the mesh; its bounds box as world lines. Picking uses posed bounds.
- **Sample**: `Assets/Models/Tentacle.glb` (4-bone tentacle, "Bulge"/"Pinch" shapes, looping
  "Bend" clip) generated in code (`skinning::sample`), its material and clip, a
  `TentacleShapes.lua` script and the **SkinningDemo** scene (animated, Lua-driven and
  hand-posed tentacles), with a generator + check test (`sample_skinning.rs`).
- **Tests**: skin import (bones, normalised weights, bind poses, shapes, clip), CPU deform
  (bind pose identity, bent bone moves only its part, posed bounds contain the mesh), blend
  shape weights move vertices, clip drives bones and weights, Animation plays it in Play mode,
  scene round-trip and copy remap of bones, pose versioning, Lua API, GPU compute vs CPU
  reference vertex for vertex, and whole headless frames (default, depth prepass, GPU culling,
  with shadows) of the GPU-skinned mesh against the CPU-deformed mesh.

## Decisions

1. **Bones are object references** (`root_bone`, `bones: [id | null]`), stored and remapped by
   the scene/prefab/copy object-reference table like joint connected bodies. Reason: the
   reference engine's bones are Transform references, and anything that moves the objects
   (clips, scripts, IK constraints from the parallel rigging work) bends the mesh with no
   coupling between the systems.
2. **Bind poses live on the mesh** (`MeshSkin::bind_poses`, taken from the node's skin at
   import), like a mesh asset's bindposes, so a Skinned Mesh Renderer needs only its mesh and
   bone list.
3. **GPU skinning by compute into a per-renderer vertex buffer**, not in the vertex shaders.
   Reason: every pass (shadow maps, depth prepass, GPU-culled indirect draws, outlines,
   custom shaders) reads ordinary vertex buffers; posing once and drawing the result keeps all
   pipelines, shaders and `MeshDraw` unchanged, and non-skinned meshes pay nothing.
4. **Poses travel as mesh keys** `skinned://<view>/<object>#0`: the view's DrawCache poses each
   renderer and publishes the matrices/weights/bounds in a small registry; the renderer looks
   the key up like an imported mesh. Reason: no new field on `MeshDraw` (39 struct literals
   across the code base) and no change to batching or culling. Each DrawCache has its own
   namespace so scenes never share poses; poses unrefreshed for 10 s are dropped.
5. **Re-skin only on change**: a pose's version only bumps when its matrices or weights
   change, and the GPU dispatches only then — a still character costs one comparison a frame.
6. **Skin matrices in the renderer's space** (`inverse(renderer) · bone · bindPose`), so the
   draw keeps the renderer's world matrix and lighting, culling and shadows treat it like any
   mesh.
7. **Bounds**: default box (root bone space) is a cube around the root as large as the
   farthest vertex plus blend shape offsets, so bending about the root never leaves it; Update
   When Offscreen computes exact-ish posed bounds every frame (per-joint vertex boxes carried by
   the skin matrices). Unlike the reference, offscreen renderers are still skinned (simpler;
   skipping is a later optimisation).
8. **Imported animation keys are resampled at 30 fps with linear tangents**; rotations become
   continuous Euler curves (each key picks the Euler triple, of the two equivalent ones,
   closest to the previous key) and redundant keys are removed. Reason: clips animate Euler
   angles (with the Transform's Euler hint keeping axis writes consistent); resampling turns
   slerp/step/cubic-spline glTF keys into curves the Animation window can edit.
9. **Clips are extracted as `.anim` files next to the model** (never overwritten, like
   materials) and an **Animation** component plays them. Reason: the existing clip/animation
   pipeline (Animation window, Animator controllers, events) works on clip files unchanged.
10. **Blend shape weights are animated by shape index** (`SkinnedMeshRenderer.blendShape.N`)
    rather than by name, keeping `Property` a `Copy` enum; weights are 0–100 (the reference
    range), the GPU gets `weight / 100`.
11. **Bone gizmo painted with egui over the image** (not depth-tested world lines), so the
    skeleton inside the mesh is always visible, like the reference's bone handles.

## Left for later

- Picking tests against the posed bounds box, not posed triangles.
- Animation window Record mode doesn't key blend shape weights yet (Add Property does).
- A glTF mesh with several primitives gets one renderer per primitive; a weights animation
  reaches the first one only.
