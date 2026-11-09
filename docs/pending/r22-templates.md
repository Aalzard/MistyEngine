# Round 22 — project templates: MeadowDemo, First Person, Racing, Floating Platform

The Hub's New Project page now offers exactly four templates (user's request), in this order:

| Template | What it is | Needs the sample project |
|---|---|---|
| **MeadowDemo** | Unchanged: the walkable meadow with the first-person player. | yes |
| **First Person** | An endless flat white plain, the First Person Player prefab, its head camera as the Main Camera. | yes (the prefab and its scripts) |
| **Racing** | An arcade racer on a 1.2 km circuit, all game logic in Lua. | no |
| **Floating Platform** | The old Standard template (platform and cube in the sky), now with post-processing. | no |

**PP Ready is gone** (it was the water scene: the platform above an Ocean surface at sunset). Its
scene code, thumbnail, tests and the ignored tonemapper comparison render that used it
(`sample_cycles_room::render_template_tonemappers`) were removed. The comparison images it wrote
stay in `docs/screenshots/gfx/template_pp_ready_*.png` as history.

## Decisions

1. **Every template starts with post-processing.** `scenes::sky_volume()` is the shared global
   volume: AgX tonemapping (Punchy look), bloom (0.4, threshold 1), volumetric clouds (Sparse
   preset), screen-space sun shafts (0.6), a little exposure and vignette. First Person and Racing
   add a light non-volumetric haze so the far ground and hills fade out. MeadowDemo keeps its own
   volume, which already has bloom, ACES, clouds and sun shafts; a test now checks all four for
   every template.
2. **Standard became Floating Platform, not a new template.** The platform and the 1 m cube stay;
   the sun moved ahead and to the right of the camera (15° up) so its glow and rays are in the
   first view, and the volume is on. `Template::from_name("Standard")` and a serialized
   `"Standard"` still give Floating Platform (`--new standard` keeps working). It is the default
   template and the Tutorial Playground's.
3. **First Person reuses the sample's First Person Player**, copied with its dependencies, meta
   files and GUIDs by the same machinery as MeadowDemo (`meadow::dependencies` /
   `meadow::copy_files`, generalised from the MeadowDemo-only `files`/`copy`), together with the
   sample's project settings (Run and Crouch axes, the Player layer). One controller to maintain,
   and the template is unavailable (dimmed card) only where MeadowDemo is too.
   - The scene's own Main Camera is removed; the prefab's head camera (already tagged MainCamera)
     is renamed "Main Camera" and sees 3 km. Both are ordinary prefab overrides. One camera, so
     the Game view and `Camera.main` agree.
   - The ground is a 6 km × 6 km slab, top at y = 0, with a near-white material. It has a faint
     generated grid texture (major lines every 4 m): without any texture a white plain gives no
     sense of movement at all. Clearing the material's Albedo map gives plain white.
   - Its collider is 36 Box Colliders of 1 km × 1 km under "Ground Colliders": with one 6 km box
     the Character Controller fell straight through it (the capsule shape cast loses precision
     against a cuboid that large; a 200 m box and 1 km tiles both hold).
   - The player spawns 0.1 m above the ground: a Character Controller starting exactly on a
     surface slipped through it (rapier ignores contacts it starts inside).
   - The player and the starting Scene view face the sun's azimuth.
4. **Racing is generated, and its scripts are real files.** The four Lua scripts live in
   `crates/misty_editor/src/templates/racing/*.lua` (embedded with `include_str!`) and are written
   to the project's `Assets/Scripts/`, so they are easy to read and edit in both places.
   - **Track**: a closed Auto Smooth spline of 32 knots sampled from a smooth outline (a radius with
     2nd, 3rd and 5th harmonics: fast sweepers, an S and two tighter corners of 60–80 m radius),
     extruded with the Road profile (14 m wide) in an asphalt texture with edge lines and a dashed
     centre line. The kerbs are two more extruded splines, offset 7.6 m each side, with a red and
     white band texture. The first knot is the start line, in the middle of the straightest stretch.
     Curves are flat: the car drives on the grass slab's collider, the road is visual and 2 cm
     above it. Banking would need a road collider from the runtime extrude mesh, which Mesh
     Collider doesn't take yet.
   - **Props for a sense of speed**: marker posts with glowing reflectors every 25 m both sides,
     red and white tyre barriers (with colliders) outside every corner tighter than 120 m, trees
     (trunks with colliders) kept 14 m clear of the road, a start gantry with a glowing banner, a
     chequered line, a grandstand on the start straight and low hills on the horizon.
   - **Car**: one Rigidbody on four Wheel Colliders (all-wheel drive), wheel models with a spoke so
     they visibly roll, and three primitive bodies under it: **Sport** (red, low, wing), **Muscle**
     (blue, long, white stripes), **Kart** (yellow, open wheels, driver). One is active at a time.
     The collider sits low so the centre of mass does (rapier puts it at the collider).
   - **RaceCar.lua**: throttle that fades out at the top speed, brakes then reverse, handbrake,
     speed-sensitive steering, downforce growing with speed², roll and pitch damping (arcade
     stability), boost (Left Shift; 3 s meter, refills in 7 s; also past the top speed), grass drag
     off the road, and reset to the nearest road point (R, or automatically after 1.5 s upside down
     or 4 s off the track) using the Track spline's `GetNearestPoint`/`EvaluateTangent`.
     `SetModel`, `ApplyTuning`, `mass`, `grip`, `topSpeed` are the settings hooks.
   - **ChaseCamera.lua**: behind and above on the ground plane, catching up smoothly, looking a bit
     ahead along the velocity, pulling back and widening the field of view with speed and boost.
   - **LapTimer.lua**: lap clock from the first crossing of the line; a lap counts only after every
     checkpoint (four evenly spread spline positions) was passed forwards; last and best laps.
   - **RaceSettings.lua**: keys listed on the HUD panel: C car, G gravity (Low 3.7 / Normal 9.81 /
     High 18 m/s²), 1/2 mass ±100 kg, 3/4 grip ±0.1, 5/6 top speed ±10 km/h, Tab hides the panel.
     Picking a car resets the tuning to that car's values.
5. **Three small Lua API additions for the settings**: `Physics.gravity` (get and set, wakes
   sleeping bodies; `Physics::set_gravity`), a settable `Rigidbody.mass` (component and rapier
   body; `Physics::set_mass`), and `WheelCollider.forwardStiffness` / `sidewaysStiffness` (the
   friction curves' grip multipliers). The wheel physics already read gravity, body mass and the
   curves every step, so the changes apply at once. Autocomplete and `docs/api/` list them.
6. **Template scenes are saved with the new project's own GUID table** (`templates::save_scene`),
   not the global asset database, which belongs to whatever project the editor has open (or to
   another test).
7. **Generated textures only** (`templates/textures.rs`): grid, grass, asphalt, kerb, chequer.
   Nothing downloaded, so everything in the templates is free to reuse.

## Tests

- `templates::tests`: every generated template is created and checked (files, Build Settings,
  scene loads without warnings, a MainCamera-tagged "Main Camera", the four post effects on,
  every material and texture written). MeadowDemo's test checks the post effects too.
- First Person: the ground (size, tiled collider covering it, white), only the ground drawn, the player linked to
  its prefab and facing the sun, one camera, the sample's axes and layer; Play: the player stands
  on the ground and walks forwards on W.
- Racing: closed splines, track length 1–1.4 km, width ≥ 12 m, props, wheels with models, Sport
  shown, the car on the grid just before the line, scripts as shipped, HUD texts.
  `racing_template_drives_round_the_loop` plays 40 s headlessly with a test autopilot (W up to
  30 m/s, A/D towards a point 25 m ahead on the spline): the car stays upright, is never reset,
  covers more than 40 % of the loop, and the HUD shows the lap and speed.
  `racing_settings_change_while_playing` presses C, G, 2, 4, 6, Tab, Shift and R and checks the
  model, mass, gravity, grip, panel text, boost meter and reset.
- `floating_platform_plays`, `meadow_demo_opens_and_plays`, `first_person_opens_and_plays`: Play
  without a script error or warning.
- `templates_render_headless` renders all four (spline extrudes built first); the ignored
  `write_template_thumbnails` re-renders the thumbnails (`MISTY_TEMPLATE_THUMBS=Racing,...` for
  some only).
- Engine: `gravity_and_mass_change_while_playing` (wheel loads follow) and
  `scripts_tune_grip_mass_and_gravity` (the Lua properties).

## Not done

- Banked curves (needs a Mesh Collider on the runtime spline mesh).
- The Racing car models share one wheelbase and collider; only the bodies differ.
- No opponents, no sound, no gamepad bindings in the Racing scripts (keyboard only).

## Screenshots

`docs/screenshots/r22/`: `hub_new_project.png` (the four cards), and for each template
`<template>_editor.png` (the editor as a new project opens) and `<template>_play.png` (Play mode,
Game view): `meadow_demo`, `first_person`, `racing`, `floating_platform`. The Racing HUD's canvas
scales from 1600 × 900, so it is small in the editor's docked Game view and full size in a player
window.
