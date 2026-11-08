# Ragdolls and cloth (round 19C, branch r19-physics)

Goal: the familiar engine's joint, ragdoll and cloth workflow on rapier: Configurable and
Character Joints with break forces, Physics Material assets, a Ragdoll Wizard that turns a
skeleton into a ragdoll you can switch on and off while playing, XPBD cloth on skinned and plain
meshes, and a Physics Debug view. The behaviour follows the familiar editor's; the code and file
formats are our own.

## What works

| Feature | How it behaves | Where |
|---|---|---|
| Configurable Joint | Per axis (X, Y, Z, Angular X, Y, Z) **Locked / Limited / Free**. Joint frame: X = Axis, Y = Secondary Axis (made perpendicular), Z = X × Y; the start pose is zero on every axis. Linear Limit (each Limited linear axis moves ± the limit), Low / High Angular X Limit, Angular Y / Z Limit (±). A limit without a spring is a hard rapier limit; with a **limit spring** it is a restoring spring-damper past the limit (rapier's joint softness would soften the locked axes too). **Drives**: X / Y / Z Drive towards Target Position / Target Velocity, Angular X and YZ Drives (or Slerp Drive) towards Target Rotation (Euler degrees) / Target Angular Velocity; Position Spring, Position Damper, Maximum Force (force-based rapier motors). **Projection** (Position and Rotation): after each step a body further than Projection Distance outside its locked or limited range is pulled back, and with every rotation locked, turned back to within Projection Angle. | `scene/physics_components.rs`, `physics/joints.rs` (`configurable_joint`, `apply_soft_limits`, `project_joints`) |
| Character Joint | A ball joint for limbs: twist about Axis between Low and High Twist Limit, Swing 1 about Swing Axis and Swing 2 about the third axis up to their limits; Twist / Swing Limit Springs make them soft; optional projection. | `physics/joints.rs` (`character_joint`) |
| Break Force / Break Torque | On every joint (Fixed, Hinge, Spring, Configurable, Character); infinity = unbreakable (written as `null`). After each step the force and torque the joint used (from rapier's per-substep joint impulses; the torque about the anchor also counts the joint force's moment about the body) are compared with the limits; a joint over either is removed. Spring Joints break on their spring force. | `JointRecord`, `check_breaks`, `joint_force` |
| `OnJointBreak(breakForce)` | Called on the scripts of the joint's object right after the step that broke it; then the joint component is removed from the object (like the familiar engine). | `scripting/mod.rs` (`joint_breaks`) |
| Physics Material asset | `.physicmat`: Dynamic Friction, Static Friction, Bounciness, Friction Combine, Bounce Combine (Average < Minimum < Multiply < Maximum; the higher of the two colliders' rules wins, matching rapier's own order). Every collider has a **Material** slot that replaces its own friction / bounciness. rapier has one friction coefficient, so a contact hook picks the combined **static** friction while the bodies slide slower than 5 cm/s and the **dynamic** one otherwise (only colliders with a material run the hook). A missing file warns in the Console and keeps the collider's own values. | `physics/material.rs`, `physics/colliders.rs` |
| Ragdoll Wizard | GameObject ▸ 3D Object ▸ **Ragdoll…** (also the Ragdoll component's button). Thirteen bones (Pelvis, Left/Right Hips, Knee, Foot, Arm, Elbow, Middle Spine, Head) plus optional hands, **auto-filled** from the Animator's humanoid Avatar (r18) or, without one, by mapping the bone names the same way Avatar ▸ Create does. Total Mass, Strength (joint break force; 0 = unbreakable), Flip Forward. Creates eleven bodies (pelvis, thighs, calves, upper arms, forearms, chest, head) with masses from typical human segment proportions of the total; **capsules** along each limb as long as the bone (to the hand, or as long as the upper arm without one), a **box** for the pelvis (hip-wide, up to the spine) and the chest (shoulder-wide), a **sphere** for the head; **Character Joints** to the body above with humanlike limits (knees and elbows bend one way only: the twist range's sign is worked out from the limb direction and the character's facing). Running it again replaces its earlier work. One undo step. | `physics/ragdoll.rs` (`auto_fill`, `create`), `misty_editor/src/ragdoll_wizard.rs` |
| Ragdoll on / off | The wizard puts a **Ragdoll** component on the character (Enabled On Start, Blend Time, its bones). Off: the bones are kinematic and follow the animation (they still push other bodies). On: they turn dynamic (keeping the animation's velocity) and their poses are written into the bones after the Animator each frame, parents first. Off again: kinematic, and for Blend Time seconds each bone blends from the pose the ragdoll left it in to the animation. Lua `Ragdoll.Enable(go)`, `Ragdoll.Disable(go [, blendTime])`, `Ragdoll.IsEnabled(go)` (`go` = the root or any bone); the Inspector has Go Ragdoll / Back to Animation buttons while playing. | `Physics::set_ragdoll`, `after_animation`, `scripting/physics_api.rs` |
| Cloth | A **Cloth** component on an object with a Skinned Mesh Renderer (skinned, morphing or plain imported mesh, or a built-in primitive; a Mesh Filter + Mesh Renderer object gets a "Convert to Skinned Mesh Renderer" button). XPBD with small sub-steps (Solver Frequency per second, one constraint pass each): **distance** constraints on the edges (Stretching Stiffness), **bending** constraints across each shared edge (Bending Stiffness), **long-range attachments** (Use Tethers: no free vertex gets further from its nearest pinned vertex than in the mesh), **max distance** per vertex from where the skinned mesh puts it (0 pins it, negative = free), **sphere and capsule colliders** listed on the component (push-out with friction, Thickness), gravity, external and random acceleration, damping, the scene's **Wind Zone** (direction, Main × 8 m/s × Wind Scale, Turbulence as swirls, Pulse Magnitude / Frequency as gusts) as aerodynamic drag across and along each triangle, World Velocity Scale (how much of the object's own movement the cloth lags), optional **self-collision** (spatial hash). Vertices welded at seams. | `scene/cloth_component.rs`, `physics/cloth/solver.rs` |
| Cloth runtime | One job per cloth per frame on the background pool (`background.rs`); the main thread only collects a finished job, snapshots the inputs (skin matrices, colliders, wind) and starts the next; a job still running is left alone (its time goes into the next one, clamped to 1/15 s). The first job builds the topology on the worker too. The result is a `ClothPose` component on the object, drawn through the **skinning hook**: the renderer's pose carries the vertices and the GPU skinning pass uploads them into the drawn buffer instead of skinning. See "CPU or GPU" below. | `physics/cloth/mod.rs`, `skinning/mod.rs` (`SkinPose::vertices`), `render/skinning.rs` |
| Constraint painting | Cloth ▸ **Edit Constraints**: the vertices show as dots in the Scene view (red pinned, yellow → green how far they may move, grey free); the left button paints the brush's max distance (Shift paints free) within the brush circle; Brush Size, Max Distance and Paint Free in the Inspector; one undo step per stroke. **Max Distance From Vertex Colours** reads a glTF COLOR_0 channel (R / G / B / A) × Colour Scale (0 pins), matched to the imported vertices by position (the import mirrors X and may reorder vertices when it makes lightmap UVs). Free All. | `misty_editor/src/cloth_tools.rs`, `physics/cloth/flag.rs` (`max_distances_from_colors`) |
| Physics Debug view | Scene view draw mode **Physics Debug**: every collider as a wireframe (static green, dynamic blue, sleeping grey, kinematic purple, trigger yellow), contact points with their normals (red), joints (the line between the two joint frames and their axes) and Spring Joints, cloth particles, a legend with counts. While playing it reads the running world; in Edit mode it builds a world from the scene (at most twice a second while the mode is on), so it shows the colliders and joints exactly as Play will. | `physics/debug.rs`, `misty_editor/src/physics_debug_view.rs` |
| Gizmos | Selected Configurable Joints: anchor, primary and secondary axes, angular limit arcs, linear limit ranges. Character Joints: axes, twist arc and the two swing arcs, the line to the connected body. | `misty_editor/src/physics_gizmos.rs` |

### CPU or GPU for cloth

The cloth runs on the CPU, on the background pool. Game cloth is small (hundreds to a few thousand
particles: capes, flags, skirts), it collides with scene colliders and reacts to wind that both
live on the CPU, and gameplay may want to read it. The XPBD constraint pass is sequential per cloth
(Gauss-Seidel), so the parallelism that fits is one job per cloth spread over the pool's workers,
which also keeps the main thread out of it entirely. A compute shader would need the colliders and
wind uploaded every frame, a graph-coloured constraint schedule to parallelise within one cloth,
and either a readback for gameplay or a render-side integration, and its time would land on the GPU
frame the renderer already budgets (the golden and timing gates). The skinning hook already uploads
the result into the buffer the GPU skinning pass would have written, so a GPU solver could later
replace the job without touching the draw path.

## Lua

`OnJointBreak(breakForce)`; `Ragdoll.Enable(go)`, `Ragdoll.Disable(go [, blendTime])`,
`Ragdoll.IsEnabled(go)`.

## File format (backward compatible)

- Joints: new `break_force`, `break_torque` (`null` = infinity, the default; old scenes load
  unbreakable). New components `Configurable Joint`, `Character Joint`, `Ragdoll`, `Cloth` (object
  references `connected_body`, `bones`, `sphere_colliders`, `capsule_colliders` are remapped like
  the others).
- Colliders: `material` (path of a `.physicmat`), written only when set.
- `.physicmat`: `{ "dynamic_friction", "static_friction", "bounciness", "friction_combine",
  "bounce_combine" }` (combine: `Average`, `Minimum`, `Multiply`, `Maximum`).
- Cloth `max_distances`: one number per mesh vertex (negative = free); empty = all free.

## Sample

`Assets/Scenes/PhysicsDemo2.scene` (generator and checks: `sample_physics2`):

- **Dummy**: a jointed character of primitives playing an idle clip, made a ragdoll by the wizard
  (60 kg). Left click throws a ball where the mouse points (`BallThrower.lua`); a ball that hits a
  bone switches the ragdoll on and shoves it (`RagdollBall.lua`); **R** blends back to the idle
  clip, **K** knocks it over (`RagdollGetUp.lua`).
- **Flag**: `Assets/Models/Flag.glb` (made in code, red vertex colour 0 along the pole) with a
  Cloth whose max distances come from that channel, streaming in a Wind Zone.
- **Bridge**: nine sagging planks on Hinge Joints and a Configurable Joint at the far end, each
  breaking at 6000 N; a 400 kg anvil falls on it and snaps it (`BridgePlank.lua` logs each
  `OnJointBreak`).
- **Ramp**: an ice block (`Ice.physicmat`: 0.02 / 0.05, Minimum) slides off, a rubber block
  (`Rubber.physicmat`: 1.0 / 1.2, Maximum) grips, a rubber ball bounces (0.75, Maximum).

## Tests

- `physics::joint_config_tests`: Configurable angular limits, free / locked axes, soft limits past
  their range, linear drive to its target with the spring sag, Maximum Force capping a drive,
  angular velocity drive, Character twist and swing limits, joint force reading the weight it holds,
  break force (held under, broken over, the break reported once), break torque, Spring Joint breaks,
  projection (distance and locked rotation, then the solver closes the rest), file round trip with
  infinity; Physics Materials: ice slides and rubber grips on 25°, static friction holds until
  pushed then dynamic can't stop it, bounciness with Maximum, a missing material warns.
- `physics::material::tests`: combine priorities, static / dynamic choice, file round trip.
- `physics::ragdoll::tests`: auto-fill by names, missing bones refused, the wizard's bodies (masses
  add up, distribution), capsule lengths and axes, boxes, head sphere, joint parents, knee and
  elbow bending directions, re-running replaces, a disabled ragdoll stands and an enabled one falls
  with its joints holding and nothing through the floor, blending back to the animation.
- `physics::cloth::solver::tests`: welding and constraint counts, pinned vertices follow moving
  targets exactly, a hanging cloth stays under 10 % stretch and settles, energy never grows without
  forces, a cloth at rest stays at rest, sphere and capsule push-out (draped over a sphere),
  max distance bounds, wind blows a flag downwind, self-collision separates layers.
- `physics::cloth::runtime_tests`: a Plane cloth hangs from its pinned edge through the background
  jobs and the renderer's pose carries its vertices, the main thread only starts jobs, the Wind
  Zone blows the cloth, flag vertex colours give the max distances.
- `scripting::physics_api::joint_ragdoll_tests`: `OnJointBreak` runs once and the joint is removed;
  `Ragdoll.Enable` / `Disable` / `IsEnabled` from Lua.
- kittest: Configurable and Character Joint editors, joint gizmos, Cloth editor (convert, Edit
  Constraints, brush), Ragdoll editor (bones, opens the wizard), Ragdoll Wizard (auto-fill,
  create, undo label; missing bones reported), Physics Material inspector; cloth painting; the
  Physics Debug view in Edit mode.
- `sample_physics2::physics_demo2_matches_its_generator`, `physics_demo2_plays` (the dummy stands,
  falls when switched on, gets back up; the anvil snaps the bridge; ice slides, rubber grips; the
  flag streams downwind).

## Performance

Measured with `cargo test --release -p misty_engine --lib cloth_and_ragdoll_timings -- --ignored
--nocapture` (holding the timing lock; the machine was also building for two other branches, so
the numbers are upper bounds and varied between runs).

| Case | Result |
|---|---|
| Cloth, the sample flag (400 particles, 1119 stretch + 1041 bend constraints) | topology 0.38 ms once; **0.19 ms** per 60 Hz frame (4 sub-steps, wind, a sphere) |
| Cloth, 64 × 64 grid (4225 particles, 12416 + 12160 constraints) | topology 4.6 ms once; **2.4 ms** per frame |
| Cloth runtime, 8 cloths, main thread (collect, snapshot, start) | **0.06 ms** per frame; the frame never waits (a job still running is skipped: on the busy machine the below-normal-priority workers were late on 70 % of frames, which only delays the cloth by a frame) |
| Ragdolls, physics step at 50 Hz (11 bodies each, all falling) | 1: 0.04–0.06 ms, 10: 0.3–1.3 ms, 30: 0.85–3.8 ms |
| Ragdolls, writing the poses into the bones | 1: 0.01 ms, 10: 0.08–0.48 ms, 30: 0.28–1.1 ms |

The joint additions cost nothing for scenes without breakable joints, soft limits or materials
(those loops skip; only colliders with a Physics Material run the contact hook).

## Screenshots

`docs/screenshots/r19/`: `physics_demo2_play` (Game view: the flag streaming, the snapped bridge,
ice slid off the ramp, rubber still on it), `physics_debug_view` (Physics Debug while playing:
kinematic dummy bones, dynamic planks, contacts, joints, legend), `physics_ragdoll_wizard`
(auto-filled bones), `physics_character_joint` (knee joint Inspector and its arcs),
`physics_configurable_joint` (the bridge's far-end joint axes), `physics_cloth_paint` (Edit
Constraints: the pole edge pinned in red), `physics_cloth_flag` (the flag in the wind),
`physics_material_asset` (Ice.physicmat Inspector).

## Not done yet

- Configurable Joint: no "Configured In World Space", "Swap Bodies", mass scales or Linear Limit
  bounciness / contact distance (stored, not used by the solver); the Slerp Drive is applied per
  axis (rapier has no slerp motor); angular projection acts only when every rotation is locked.
- Break torque is an estimate (joint torque plus the joint force's moment about the body's centre
  of mass); rapier does not report the constraint wrench about the anchor directly.
- Ragdolls: the root object doesn't follow the fallen body (blending back snaps the character to
  where it stood); no "get up" animation matching; no per-bone ragdoll layers or partial ragdolls.
- Cloth: no continuous collision (fast colliders can pass through thin cloth), no collision with
  mesh, box or terrain colliders, no tearing, no GPU solver; vertex colours are read from glTF files
  only (not OBJ); the painting brush works in Edit mode only.
- Physics Debug: mesh, convex and heightfield colliders draw as their bounding boxes.
