# Animator depth (round 18B, branch r18-animator)

Goal: the familiar engine's Mecanim behaviour for character animation: layers with avatar masks,
2D and direct blend trees, sub-state machines, transition interruption, root motion, and a Lua
API to drive all of it. The behaviour follows the familiar editor's; the code and the file
formats are our own.

## What works

| Feature | How it behaves | Where |
|---|---|---|
| Layers | Layer 0 is the Base Layer; more layers are listed in the controller and evaluated bottom to top. **Override** layers replace the pose below by their weight; **Additive** layers add each clip's motion relative to its first frame, scaled by the weight. A layer's weight is its start value; scripts change it. | `animation/controller.rs` (`Layer`), `animation/animator.rs` (`apply`) |
| Avatar Masks | `.mask` assets: a list of transform paths below the Animator's object, each on or off. A layer with a mask only animates the Transform curves of paths that are on (an empty list lets everything through; lights, blend shapes and the active flag always pass). | `animation/mask.rs` |
| Sync layers | A layer can reuse another layer's states and transitions with its own clip per state (states without one play nothing on that layer). It follows the source layer's current state, blend and normalised time. | `Layer::sync_source`, `sync_motions` |
| Blend trees | **1D**, **2D Simple Directional** (the two directions either side of the input, plus a centre motion; without one, normalised), **2D Freeform Directional** (gradient bands in polar space: several speeds per direction work), **2D Freeform Cartesian** (gradient bands in x/y) and **Direct** (each child's weight is its own parameter, optionally normalised; unnormalised weights below 1 leave the rest to the rest pose). Each child has a speed (time scale) that changes the blended length. | `animation/blend.rs`, `BlendTree::weights_at` |
| Sub-state machines | States belong to a machine path ("Combat", "Combat/Melee"); each machine has its own Entry, Exit, default state and graph position. A transition to a machine enters through its Entry: the first Entry transition whose conditions hold, else the default state. A transition to Exit leaves the machine: the parent's transitions from the machine node are checked in order (one without conditions always goes), else the parent's Entry decides; leaving the top machine starts it again. State names stay unique per layer, so `Play("Attack")` and full paths ("Base Layer.Combat.Attack") both work. | `animation/fsm.rs` (`enter`, `exit`, `resolve`), `StateMachine` |
| Any State | Checked first from every state of the layer, also during a transition (they can always interrupt). Never re-enters the state being played or entered unless **Can Transition To Self** is on. | `fsm::evaluate` |
| Interruption | Per transition: **Interruption Source** None / Current State / Next State / Current State Then Next State / Next State Then Current State, and **Ordered Interruption** (only the current state's transitions listed before this one count). When interrupted, the blend in progress is frozen where it is (sampled once) and blended out by the new transition. | `fsm::evaluate`, `fsm::interrupt` |
| Exit time, duration, offset | As before: exit times below 1 repeat every loop; durations in seconds or source lengths; the destination's start offset. Plus **Mute** and **Solo**. | `Transition` |
| State extras | Speed **Multiplier** parameter, **Tag** (`IsTag`). | `State` |
| Root motion | Animator ▸ **Apply Root Motion**, optional **Root Motion Bone**. The root's movement between each clip's previous and current time is measured in the clip's own root space (so walking forwards follows the character's facing), across loop wraps, weighted like the pose (Base Layer). With the object as root, its own position and Y rotation curves move it instead of being written. With a bone, its horizontal travel and heading move the object; the bone keeps its height and tilt and holds its first-frame horizontal position. With a Character Controller the move goes through it (collisions, steps). | `animation/root_motion.rs`, `scripting::root_motion`, `animator_api::apply_root_motion` |
| OnAnimatorMove | A script defining `OnAnimatorMove` takes over: the motion is not applied, the function runs right after the Animator updates and reads `animator.deltaPosition` / `deltaRotation` (and may call `ApplyBuiltinRootMotion()`). | `scripting/mod.rs` |
| Target matching | `MatchTarget(position, rotation, "Root", weights, start, end)` moves the object so it reaches the target at the end time of the current state, by per-axis position weights and a rotation weight. | `AnimatorPlayer::match_target` |
| Damped floats | `SetFloat(name, value, dampTime, deltaTime)` moves `dt / (damp + dt)` of the way each call. | `animator::damped` |
| Humanoid avatars | `.avatar` assets map a rig's bones to a standard 21-bone human skeleton by name (Hips / pelvis, Spine, a second spine as Chest, Neck, Head, shoulder / clavicle, upper arm / Arm, forearm / lower arm, hand, thigh / UpLeg, calf / Leg, foot, toes; sides from "Left", "l", "_r", "mixamorig:" prefixes stripped) and record its rest pose (each bone's rotation relative to the Animator and its parent's, the hips height). | `animation/humanoid.rs` |
| Retargeting | A clip made for one rig becomes a humanoid clip: per human bone, the rotation change from rest in the Animator's space (quaternion curves on "@LeftUpperArm", sampled at 30 fps, kept on one hemisphere), plus the hips' move in hips heights; the object's own curves (root motion) are kept. An Animator with an Avatar plays humanoid clips on its rig: each bone gets the same change from its own rest pose (so other names, bone axes and proportions work; both rigs must share the rest pose, e.g. arms down), bones between mapped ones stay at rest, the hips move scaled by the hips height. Masks apply to the target paths. | `humanoid_clip`, `retarget`, `animator::sample_run` |

### Editor

- **Animator window**: a **Layers** tab next to **Parameters** (weight sliders, M / A / S badges,
  ⚙ settings: name, weight, blending, mask, sync, delete; live weights in Play mode); the graph shows
  the selected layer. **Breadcrumb** "Base Layer ▸ Combat ▸ Melee" (click to go back, ⏶ up one
  level). Sub-state machines are stacked nodes; double-click (or right-click ▸ Open) enters them;
  inside, "(Up) Base Layer" stands for the parent: a transition to or from a state elsewhere is drawn
  from the sub-machine holding it or from (Up), and ending Make Transition on (Up) lists the states
  outside. Canvas menu ▸ Create Sub-State Machine; state menu ▸ Move to. Muted transitions are red.
  Synced layers show their source's graph read-only.
- **Blend tree view**: double-click a blend tree state (or Inspector ▸ Open in Animator Window):
  parameter fields, the diagram and the motions with their live weights.
- **Inspector, state**: Motion Type Clip / Blend Tree; Blend Type, Parameter (X / Y), Normalized
  Blend Values (Direct); the **diagram** (1D influence triangles, 2D motion points with weight
  circles and the selected motion's influence as a heat map, Direct bars) with the parameters as a red
  marker that can be dragged (defaults in Edit mode, live values in Play mode); per motion: weight
  now, clip, threshold / position / parameter, speed. Speed, Multiplier, Loop, Tag.
- **Inspector, transition**: Mute, Solo, Interruption Source, Ordered Interruption, Can Transition To
  Self (Any State). **Sub-state machine**: name, default state, its transitions.
- **Avatar Mask inspector**: Import skeleton from a scene object (objects with an Animator first;
  flags already set are kept), the transform tree with checkboxes (unticking a bone unticks
  everything below it), All / None / Clear, undo. Project ▸ Create ▸ Avatar Mask.
- **Animator component**: Apply Root Motion, Root Motion Bone (picked from the hierarchy), Avatar
  (slot plus **Create**: maps this object's rig and saves `Assets/<Name>.avatar`, warning about
  missing required bones).
- **Avatar inspector**: the human bones and the rig paths they map to (missing required bones in
  red), hips height, and **Convert to Humanoid** for a picked clip (writes "<clip> (Humanoid).anim"
  next to it).

### Lua

`SetFloat(name, v [, dampTime, deltaTime])`, `layerCount`, `GetLayerName(i)`, `GetLayerIndex(name)`,
`GetLayerWeight(i)`, `SetLayerWeight(i, w)`, `Play(state [, layer [, normalizedTime]])`,
`CrossFade` / `CrossFadeInFixedTime(state, d [, layer [, offset]])` (layer -1 = the first layer with
that state; a blend in progress is frozen and blended out), `GetCurrentAnimatorStateInfo(layer)` /
`GetNextAnimatorStateInfo(layer)` (now also `name`, `fullPath`, `tag`, `IsTag`; `IsName` accepts the
short name, "Layer.State" and the full path), `IsInTransition(layer)`, `applyRootMotion`,
`deltaPosition`, `deltaRotation` (Euler degrees: there is no Quaternion type), `velocity`,
`ApplyBuiltinRootMotion()`, `MatchTarget(...)`, `InterruptMatchTarget([complete])`,
`isMatchingTarget`; callback `OnAnimatorMove()`.

## File format (backward compatible)

- `.controller`: the base layer's machine stays at the top level (`states`, `transitions`,
  `default_state`, `nodes`), so older files load unchanged and the existing samples were not
  regenerated. New keys: `sub_machines`, `layers` (each: `name`, `weight`, `blending`, `mask`,
  `sync_source`, `sync_motions` and its own machine keys), per state `speed_parameter`, `tag`,
  `machine`; per transition `interruption_source`, `ordered_interruption`,
  `can_transition_to_self`, `mute`, `solo`, `machine`; per blend tree `blend_type`, `parameter_y`,
  `normalize`; per child `position`, `direct_parameter`, `time_scale`; `nodes.up`.
- `.mask`: `{ "transforms": [ { "path": "Hips/Spine", "active": true }, … ] }`.
- `.avatar`: `{ "bones": [ { "bone": "LeftUpperArm", "path": "…", "rest": [x, y, z, w],
  "parent_rest": […], "rest_position": […] } ], "hips_height": 0.95 }`.
- Humanoid clips are ordinary `.anim` files whose curves use the paths "@Bone" and the properties
  `Humanoid.rotation.x/y/z/w` and `Humanoid.position.x/y/z`.
- Animator component: `apply_root_motion`, `root_motion_bone`, `avatar`, written only when set
  (scenes with Animators are unchanged).

## Sample

`Assets/Scenes/CharacterAnimatorDemo.scene` (generator and check:
`sample_character_animator`): a jointed character of primitives with a Character Controller.
Base Layer: a 2D Freeform Directional Locomotion tree (Idle, Walk, Run, WalkBack, StrafeLeft,
StrafeRight on MoveX / MoveZ) with root motion, and an "Emotes" sub-state machine (Bow, Cheer; the
Entry picks Cheer when EmoteId is 1). "Upper Body": override layer with the `UpperBody.mask`
playing Wave (the script fades its weight). "Breathing": additive. `CharacterControl.lua`: WASD,
Shift, Left / Right, Q, 1 / 2. "Buddy", a smaller second rig with other bone names ("pelvis",
"upperarm_r"…) and arm bones turned half a turn, plays the Character's Wave retargeted through
`Character.avatar` → `Wave (Humanoid).anim` → `Buddy.avatar`.

## Tests

- `animation::blend_tests`: weights per blend type against known values (1D, Simple Directional with
  and without a centre, Freeform Cartesian, Freeform Directional, Direct, `BlendTree::weights`).
- `animation::layer_tests`: override layer with a mask, additive layer, synced layer, layer
  sanitising and file round trip; interruption (None, Current State with and without ordering, Next
  State with the frozen pose, both orders, Any State, mute / solo); sub-state machine Entry / Exit /
  machine transitions / full paths and editing (nested machines, rename, move, remove); root motion
  deltas across loops, reversed and clamped, composition, object and bone roots, accumulation over
  frames, off; target matching.
- `animation::humanoid_tests`: auto-mapping two differently named rigs (required bones found,
  optional ones may be missing, round trip), converting a clip to humanoid (root curves kept) and
  playing it on the other rig (half size, arm bones turned 90°): the arm points the same way, the
  hips move by the scaled height, unanimated bones stay at rest.
- `animation::animator_tests` (updated): the original behaviour is unchanged.
- `scripting::animator_api::tests`: layers and state info, damped SetFloat and layer cross-fades,
  root motion along the facing, `OnAnimatorMove`.
- kittest (`panels/animator/tests.rs`): Layers tab (add, settings), sub-state machines (create,
  transition to it, open, (Up), create inside, breadcrumb, Inspector), blend tree editor (type,
  add motion, diagram click sets parameters, 1D fields), blend tree view, Avatar Mask editor
  (import, toggle subtree, undo, flags kept), transition interruption Inspector.
- `sample_character_animator::character_animator_demo_matches_its_generator_and_plays`.

## Screenshots

`docs/screenshots/r18/animator_*.png`.

## Not done yet

- **Foot IK** for humanoids is not automatic: use the existing Two Bone IK constraint (Animation
  Rigging) on the legs, as RiggingDemo does; no "IK Pass" layer option or `OnAnimatorIK`.
- Humanoid retargeting is rotation-delta based, not muscle space: both rigs need the same rest pose,
  there are no muscle limits, no twist distribution, no mirror. The Avatar mapping can't be edited
  by hand yet (re-create it after renaming bones); humanoid body-part masks are not offered (masks
  list transform paths).
- Blend trees can't nest (a child is always a clip); no "Compute Positions" from clip velocities; no
  per-child mirror or cycle offset.
- Layers: the base layer has no mask; no per-layer IK pass; synced layers ignore "Timing".
- Transitions from an interrupted blend freeze the pose once (the blend tree parameters at that
  moment); state machine behaviours (`OnStateEnter` …) are not provided.
- Root motion comes from the Base Layer only and does not drive a Rigidbody (only a Character
  Controller or the Transform). `MatchTarget` supports the Root target only.
- Editing in the graph: no copy / paste of states, no multi-select, states can't be dragged onto a
  sub-machine node (use right-click ▸ Move to).
