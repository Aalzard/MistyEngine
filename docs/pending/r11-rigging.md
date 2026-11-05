# Round 11: constraints and animation rigging

Progress: new `misty_engine::rigging` module with **Rig Builder**, **Rig** and five constraints —
**Two Bone IK**, **Multi-Aim**, **Damped Transform**, **Chain IK** (FABRIK / CCD) and
**Override Transform** — evaluated every frame after animation (Animation / Animator) and before
LateUpdate and rendering, in Play mode, in built players and in the Animation window's edit-mode
preview. Inspector editors (card grid, bold "Constrained Objects" / "Source Objects" / "Settings"
sub-headers, Two Bone IK "Auto Setup from Tip"), Scene-view gizmos (octahedral bones, target
boxes, hint diamonds, dashed effector lines), Lua access, and the generated **RiggingDemo** scene
(arm IK, head aim, tail chain, damped antenna; `RiggingDemo.lua` moves the targets).

Decisions:
- **No existing transform constraints to reuse.** The Inspector's "Constraints" foldouts are the
  Rigidbody / Rigidbody 2D freeze flags; nothing Position/Rotation/Aim-like existed, so the
  rigging module is new rather than an extension.
- **Everything is plain Transforms.** Bones, targets and hints are ordinary objects referenced by
  entity (same `entity_bits` serde + `OBJECT_REFS` remapping as joints, via `rigging::RIG_REFS`
  chained into `scene::file::all_refs`). The user moves targets/hints with the Move tool; skinned
  meshes (whose bones become transforms) work without changes.
- **Non-destructive evaluation (`RigState`).** Per driven bone the pose before and after the rig
  is remembered. Next frame, if the bone still holds exactly the rig's output (no curve, script or
  tool wrote it), the pre-rig pose is restored before solving. Without this, weights < 1 would
  accumulate every frame on bones no clip animates (Unity always re-reads the animated stream; we
  have no stream). It also lets the Animation window preview put the authored pose back.
- **Weights blend local transforms**: each constraint solves fully, then every driven bone's local
  position/rotation/scale is lerped/slerped from its pre-constraint value by `rig weight ×
  constraint weight`. Weight 0 skips the solve entirely, so it is exactly "no change".
- **Rig Builder layers** list rigs in order; with no layers, every Rig below the builder runs
  (convenient for hand-made rigs). Constraints run in hierarchy order under each rig (the rig
  object itself included), inactive objects are skipped.
- **Bind offsets** (maintain target offset, Multi-Aim maintain offset, Damped Transform offset)
  are captured the first time a constraint runs in a session, like Unity's bind at Rig Builder
  build time.
- **Two Bone IK** uses the law-of-cosines solve: bend the mid joint to the needed angle, swing the
  root onto the target line, then spin around root → target so the elbow faces the hint (signed
  angle, scaled by hint weight). Out-of-reach targets clamp to the chain length, so the arm
  straightens towards them.
- **Chain IK** offers FABRIK (default, spreads the bend) and CCD; root must be an ancestor of tip.
- **Multi-Aim** averages source positions by weight and builds the rotation from aim/up axis
  frames; World Up Type None / Scene Up / Object Up / Object Rotation Up / Vector. Constrained
  axes and min/max limits act on the local turn away from the input pose (YXZ Euler).
- **Damped Transform** closes `1 − damp^(dt·60)` of the gap each frame (frame-rate independent;
  0 = rigid, 1 = frozen). Maintain Aim re-aims along the source → object direction.
- **Override Transform** included (Pivot / Local / World space, or an override source object).
- **Edit mode:** rigs run only in Play mode and while the Animation window previews (rigs below
  the previewed object), matching Unity; preview end calls `RigState::restore`.
- **Lua:** `GetComponent("TwoBoneIKConstraint")` (also "RigBuilder", "Rig", "MultiAimConstraint",
  "DampedTransform", "ChainIKConstraint", "OverrideTransform", or the Inspector names). Fields
  mirror the Inspector in camelCase via the component's JSON (`weight`, `target`, `hint`,
  `hintWeight`, `constrainedObject`, `sourceObject`, `aimAxis`, `offset`, …); object fields take
  a GameObject/Transform/nil. Multi-Aim sources: `sourceCount`, `GetSource/SetSource`,
  `GetSourceWeight/SetSourceWeight`, `AddSource` (1-based). Generic by design so new fields need
  no API code.
- **Gizmos** draw for constraints related to the selection: the constraint object, its Rig or
  Rig Builder (also via layers), or any of its bones/targets/hints.
- Add Component category "Animation Rigging".

Screenshots: `docs/screenshots/round11/rigging_edit.png` (Two Bone IK Inspector),
`rigging_play.png` (Play mode, Creature selected: bone / target / hint gizmos),
`rigging_game.png` (Game view: arm on the target, head aiming, tail on its target, antenna),
`rigging_multi_aim.png` (Multi-Aim Inspector with weighted sources and limits).

Tests: `rigging::tests` (Two Bone IK reach / straighten, hint side, weight 0 and no weight
accumulation, animated input, Multi-Aim aim + limits, Chain IK convergence for FABRIK and CCD,
Damped lag/settle, Override, scene round-trip, Lua access) and
`sample_rigging::rigging_demo_matches_its_generator_and_plays` (600 frames of the demo).

Left for later: no Bone Renderer component (gizmos only show for the selection's constraints);
no Animation-window "record the rig into the clip" bake; object pickers are combo boxes (no
drag-from-Hierarchy yet, same as the other object fields).
