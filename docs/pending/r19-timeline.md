# Timeline and cameras: cutscene tooling (round 19E, branch r19-timeline)

Goal: the familiar engine's Timeline and virtual-camera package behaviour for cutscenes — Camera
(Cinemachine-style) and Control tracks, sub-timelines, record mode, an edit-mode preview that
shows the shot in the Game view, signals with Lua receivers, plus Free Look rigs, impulses, group
framing, custom blend curves and a confiner. Behaviour follows the reference; names, code and
data are ours.

## What works

### Timeline

| Feature | How it behaves | Where |
|---|---|---|
| Camera track | Bound to a Camera Brain. Each clip shows a virtual camera (the clip's reference). Two overlapping clips blend from the earlier camera to the later one with the timeline's smoothstep mix (weights always sum to one); a clip's ease in / out blends with the brain's own live camera, so a cutscene can ease in from and out to normal play. Between clips the brain is left alone. In Play mode the track hands the brain a `CameraOverride` (from, to, weight) each frame; the rig shows it instead of the priority pick and, when the override ends, blends from the camera it showed to the live camera with the brain's blends (blend list asset first). While overridden, `activeVirtualCamera` is the camera with the most weight and `isBlending` / `blendWeight` follow the override. Paused directors keep their override. | `timeline/tracks/camera.rs`, `vcam/rig.rs` (`CameraOverride`, `update_brain`) |
| Clip references | Camera and Control clips point at scene objects through the director (assets can't hold scene objects): the clip stores a reference name (`r1`, `r2`, … — deterministic like track ids) and the Playable Director a `references` list (name → object), remapped on load / duplicate / prefab like bindings. | `TimelineClip::reference`, `PlayableDirector::references`, `scene/file.rs` |
| Control track | Each clip controls its object: Activation (on only while a clip covers the time; Post-playback State on stop, default Revert), Sub-Timeline (the object's Playable Director timeline is posed at the clip's local time), Particles (its and its children's systems restart at the clip start and stop at the end), Audio (its Audio Source plays during the clip), Impulse (its Impulse Source fires at the clip start). Particles, audio and impulses only run in Play mode. | `timeline/tracks/control.rs`, `director.rs` (`sync_control`) |
| Sub-timelines | Local time = `clip_in + (t − start) × speed`; past the nested timeline's end it holds its last frame, or wraps when that director's Wrap Mode is Loop. Nested timelines evaluate with their own director's bindings and references, may hold Camera and Control tracks themselves (shots bubble up), and nest up to 8 deep (a timeline that plays itself stops there). In Play mode their markers fire at the mapped time (the part of each frame span inside the clip). The nested director should have Play On Awake off (the parent drives it). | `evaluate::apply_nested`, `control::sub_time`, `director::signals` |
| Record mode | ⏺ on an Animation track arms it (the preview turns on, the toolbar shows "⏺ Recording"). Any change to the bound object or its children — Scene view gizmos, Inspector fields, anything — becomes a key at the playhead in the track's recorded keys: after the preview poses the scene each frame the recordable values (transform, light, active) are remembered and next frame whatever differs is keyed; keys made during one drag are one undo step. Leaving the preview puts the scene back (the change lives on as keys). Recording writes to tracks without clips (the track's own recorded clip, drawn as one "Recorded" block); tracks with clips can't record. | `panels/timeline/record.rs`, `headers::record_button` |
| Ripple / Mix | Toolbar "Ripple": moving a clip shifts every clip that started after it by the same amount, trimming a clip's end shifts the later clips by the change, deleting a clip closes the gap. Off (Mix, the default) clips move alone and blend where they overlap. | `ops::ripple`, `lanes::interact` |
| Edit-mode preview | Scrubbing or playing in the window outside Play mode poses Activation, Animation and Control tracks (with sub-timelines), and Camera tracks pose the brain camera itself — transform and lens, cameras solved without damping, the highest-priority virtual camera standing in for "live" during eases — so the Scene and Game views show the shot. Everything touched (values, active flags, brain transforms and lenses) is backed up the first time and put back when the preview stops (Preview off, Play, save, scene change); a brain no shot covers any more returns to its own pose at once. | `panels/timeline/preview.rs`, `tracks::camera::preview`, `Baseline::cameras` |
| Signals | Markers on Signal tracks (and the markers row) call `method(parameter)` on the bound object's scripts as before, and now also run the director's `signalReceived` listeners `(director, method, parameter)` — a receiver that doesn't need a script method per marker. | `director::signals`, `scripting/mod.rs` (`director_events`) |

### Virtual cameras

| Feature | How it behaves | Where |
|---|---|---|
| Free Look | Orbital body with Orbits = Three Rings (GameObject ▸ Camera ▸ Free Look Camera): top, middle and bottom rings (height above the target, radius; defaults 4.5 / 1.75, 2.5 / 3, 0.4 / 1.3). The vertical axis (0 bottom … 0.5 middle … 1 top) slides along straight lines between the rings bent towards the parabola through all three by the Spline Curvature (0.2); the horizontal axis is the heading. The camera looks at the orbit centre unless an aim takes over. Scene view gizmo: the three rings and the vertical path at the current heading. | `Orbital::ring_at`, `solve.rs`, `vcam_gizmos.rs` |
| Input System axes | Each orbit axis takes an Input Manager axis (as before) or an Input System action (`Input Action`, "Map/Action" or "Action") read from the first playing Player Input with it enabled — x for the horizontal axis, y for the vertical; the action wins. | `OrbitAxis::input_action`, `solve::action_value` |
| Impulse | Impulse Source component: shape (Bump — a push along the velocity and back; Explosion — a ringing kick with a random sideways jolt; Rumble — fast 6D shaking), amplitude / frequency gain, default velocity, attack / sustain / decay envelope (smooth, reaching exactly zero), impact radius (full strength) and dissipation distance (fades to zero beyond it), channel bits, trigger Manual or Collision Enter (with a layer mask). Fired by scripts (`GenerateImpulse`), signals (script → `Impulse.GenerateImpulse`), Control clips and collisions. Virtual cameras with an Impulse Listener (gain, channel mask, 2D distance) add the summed shake of every live impulse to their output (the camera object keeps its raw pose). Deterministic: a pure function of time and a per-impulse seed. | `vcam/impulse.rs`, `CameraRig::solve_cameras` |
| Group framing | Target Group component (members: object, weight, radius; Position Mode Group Center or Group Average): while playing the group object sits on the group's position, so any body can follow it. Bounds: members with weight in 0..1 are pulled towards the weighted average, their radii pad the box, the bounding sphere's centre and radius feed the Group Composer aim, which aims like the Composer at the centre and then dollies along the line to the centre and / or zooms so the sphere fills Framing Size of the screen (Horizontal / Vertical / the tighter of both); Dolly Only, Zoom Only or Dolly Then Zoom with dolly limits, distance limits, FOV limits and damping. The object keeps the body's position (only the shown camera dollies). Gizmo: the group's bounding sphere. GameObject ▸ Camera ▸ Target Group Camera makes a group of the selection and a camera framing it. | `vcam/group.rs`, `solve::frame_group` |
| Blends | Blend style Custom: a cubic Bézier from (0, 0) to (1, 1) through two control points (inspector with a curve preview). Blend list assets (`.blends`, JSON, a list of from → to blends by camera name) are set on the brain ("New" moves the inline blends into an asset; its entries are edited in the brain's Inspector) and checked before the brain's own custom blends (exact pair > any → to > from → any > any → any in each list). | `BlendDefinition::weight`, `bezier_ease`, `BlendList`, `CameraBrain::blend_with` |
| Confiner | Bounding Volume: an object's Box Collider (oriented, scaled), else its Sphere Collider, else its unit cube. The body's position is moved to the nearest point inside before the aim, with optional damping. | `solve::confine` |

### Lua

- `PlayableDirector`: `Play`, `Pause`, `Resume`, `Stop`, `Evaluate`, `time`, `duration`, `state`
  (as before), `SetGenericBinding(track, object)` and `GetGenericBinding(track)` (track by name or
  id; object a GameObject, Transform or nil; an unknown track errors), events `played`, `stopped`
  and `signalReceived` (`function(director, method, parameter)`).
- `VirtualCamera`: `xAxis`, `yAxis` (Free Look / orbital axis values, clamped or wrapped like the
  axis), `xAxisInput`, `yAxisInput` (Input Manager axis names), `inputAction` (both axes).
- `GetComponent("ImpulseSource")`: `GenerateImpulse()`, `GenerateImpulse(force)`,
  `GenerateImpulse(velocity)`, `GenerateImpulseWithForce(force)`.
- `Impulse.GenerateImpulse(gameObject [, force or velocity])` (false without a source),
  `Impulse.GenerateImpulseAt(position, velocity)`.
- `GetComponent("TargetGroup")`: `AddMember(object [, weight [, radius]])`, `RemoveMember(object)`,
  `memberCount`, `radius`.

### Editor

- Timeline window: Add ▸ Control Track / Camera Track; lane menus "Add Camera Shot ▸ <virtual
  camera>" and "Add Control Clip ▸ <object>" (objects with a director, particles, audio or an
  impulse source first); clips whose object is gone say "(no object)". The clip Inspector picks
  the Virtual Camera / Source Object (kept on the window's director) and the Control options.
  ⏺ record buttons, the Ripple toggle.
- Inspector: Free Look rings and curvature, Input Action fields, Group Composer settings (and its
  composition), Confiner, Impulse Listener, Target Group (targets list, bounds radius), Impulse
  Source, Custom blend curves, Blend List field.
- GameObject ▸ Camera: Virtual Camera, Free Look Camera (three rings), Target Group Camera,
  Impulse Source.
- `MISTY_TIMELINE=record=<track>,clip=<track>:<index>` (with `MISTY_TIMELINE_TIME`) and
  `MISTY_CAPTURE_PLAY_TIME=<seconds>` for
  screenshots.

## Sample: CutsceneDemo

`Assets/Scenes/CutsceneDemo.scene` from `sample_cutscene.rs` (`write_cutscene_demo`; the test
`cutscene_demo_matches_its_generator_and_plays` checks the files and plays it headlessly). An 18 s
cutscene: Wide Cam (0–4.5 s), cut to Hero Cam following the Hero as he walks in (Animation
track), blend to Group Cam (Group Composer on the Trio target group) over 7.5–8.5 s, cut to
Explosion Cam at 12 s, blend back to Wide Cam over 14.5–15.5 s. At 12 s the "OnExplosion" signal
makes `CutsceneDemo.lua` call `Impulse.GenerateImpulse(Barrel)` (an Explosion-shape source; every
camera listens), a Control clip switches the explosion sphere on (reverting at stop) and another
plays `CutsceneLights.timeline` as a sub-timeline flickering the Flash light. "Ally Turn" holds
recorded keys (the Ally turns to greet the Hero). When the director stops the brain blends to Free
Look Cam (priority 20, held back by the timeline until then) with the Custom curve from
`Assets/Cameras/CutsceneBlends.blends`.

## Decisions

1. **Camera tracks override, they don't change priorities.** The reference's track drives the
   brain directly; we do the same through a per-frame override list the directors hand to the rig
   after they update (directors run before LateUpdate, cameras after), so a stopped or destroyed
   director simply stops sending one.
2. **Clip references live on the director.** Like the reference's exposed references: the asset
   stays scene-independent, the director maps names to objects. Names are `r<n>` (next free), not
   GUIDs, so generators are deterministic.
3. **No separate Cinemachine Shot clip type.** A Camera clip is an ordinary clip with a reference;
   eases and overlaps reuse the timeline's blend maths, so the Timeline window draws camera blends
   like animation blends.
4. **Control clips control what the object has.** Rather than per-feature sub-options with lists
   (the reference has prefab instancing and ITimeControl), five toggles decide what is driven;
   prefab instancing is left out.
5. **Sub-timelines are evaluated, not played.** The parent poses the nested timeline at the mapped
   time each frame (and fires its markers); the nested director's own playback isn't started —
   one clock, no drift.
6. **Record mode records by difference.** Instead of hooking every property setter, the preview
   remembers the recordable values it left and keys what differs next frame — any edit path
   (gizmo, Inspector, script in edit mode) records. Recording targets the track's own recorded
   clip; recording into `.anim` clips under the playhead isn't done.
7. **Edit-mode Camera preview solves without memory** (`preview_pose`): no damping, no noise, so a
   scrub shows the settled shot. The highest-priority camera stands in for the brain's "live"
   camera during eases (the reference's preview does the same with its active camera).
8. **Free Look stays an Orbital body** with a ring style instead of a new body kind (the inspector
   and solver share the axes, binding and damping). The vertical spline is piecewise linear blended
   towards a quadratic by Spline Curvature — exact on the rings, smooth between.
9. **Impulses are stateless signals.** An event stores its start time and settings; listeners
   evaluate envelope × shape × falloff at their own position, so many listeners and sources cost
   nothing to keep. The shake goes on the output only (the object keeps its raw pose, as noise
   does).
10. **Group framing dollies the shown camera only.** Keeping the object at the body's position
    stops Do Nothing bodies from creeping and keeps the dolly limits relative to the body.
11. **Custom blends are Bézier curves** (two control points) rather than free curves: Copy, four
    numbers in the file, enough for slow-in / fast-out shapes.

## Tests

- Engine (`timeline/tests_r19.rs`): camera shots (cuts, overlap weights, eases from the live
  camera), the rig showing overrides and blending back when they end, sub-timeline time mapping
  (clip in, speed, hold, loop; nested activation through evaluation), Control activation with
  Revert on stop and the impulse event, sub-timeline signals at the mapped time, the edit-mode
  camera preview putting the brain back, file round trip.
- Engine (`vcam/tests_r19.rs`): Free Look ring interpolation (rings exact for any curvature,
  monotone heights, solved position), Input System action axes, impulse envelope / falloff /
  channels / gain / decay to nothing and deterministic shapes, listeners shaking the brain, group
  framing keeping all three members (with radii) on screen while they spread and wander, group
  bounds weighting, zoom-only, the confiner, Bézier curves and a blend list asset winning over
  inline blends.
- Lua (`scripting/timeline_tests.rs`, `vcam_api.rs`): `SetGenericBinding` / `GetGenericBinding`,
  `played`, `signalReceived`, `stopped`; Free Look axes and inputs, `GenerateImpulse` (component and
  `Impulse` table), Target Group members.
- Editor kittest (`panels/timeline/tests_r19.rs`): Add ▸ Camera Track, lane menu ▸ Add Camera Shot,
  scrubbing poses the Game camera (transform and FOV) and activates Control objects, Preview off
  puts everything back; ⏺ arms recording, moving the bound object writes keys at 1 s and 2 s
  (saved), scrubbing plays them, leaving restores; Ripple drag and delete.
- Sample: `cutscene_demo_matches_its_generator_and_plays` (shot order, blend, group in view, shake,
  flicker, signal logs, blend to Free Look).

## Screenshots

`docs/screenshots/r19/`: `timeline_camera_track.png` (CutsceneDemo in the Timeline window at
8 s: Camera, Animation, recorded, two Control and a Signal track; the Game view shows the
edit-mode preview half way through the Hero → Group blend), `timeline_record.png` (Ally Turn
armed: red ⏺ and "Recording"), `timeline_control_clip.png` (12.4 s: the explosion shot with the
sphere and flash switched on by the Control clips; the Flash Flicker clip's Control options in
the Inspector), `timeline_freelook_inspector.png` (Free Look rings in the Inspector and the Scene
view gizmo), `timeline_group_inspector.png` (Group Composer settings and the group's bounding
sphere), `timeline_play_group.png` (Play mode at 12 s: the group shot with all three framed, the
playhead following), `timeline_play_explosion.png` (Play mode just after the explosion signal:
the shaken explosion shot), `timeline_play_freelook.png` (after the cutscene: the Free Look
camera). Captured with `MISTY_SCENE`, `MISTY_SELECT`, `MISTY_TAB`, `MISTY_TIMELINE_TIME`,
`MISTY_TIMELINE` and `MISTY_PLAY` + `MISTY_CAPTURE_PLAY_TIME` (new: holds the shot until Play mode has run that many seconds of game time — frame counts drift with machine load).

## Left

- Recording into `.anim` clips (tracks with clips), Replace edit mode, nested audio tracks.
- Control clip prefab instancing; particle systems aren't simulated in the edit-mode preview.
- Dolly track / path cameras, lookahead, Free Look per-ring aims.
- Signal assets with a Signal Receiver component (the method name on the marker plus
  `signalReceived` cover the Lua side).
