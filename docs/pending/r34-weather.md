# Round 34 — Weather (rain, snow, storms) — branch r34-weather

Status: work in progress (resumable). Target dir `D:/MistyTargets/r34-weather`; build only through
`/d/MistyTargets/r34-scratch/cargo.sh <log> <cargo args>` (takes a shared build slot, `-j 4`).

## Design (written)
- `crates/misty_engine/src/weather/`: `WeatherParams` (every value) + 13 presets (`mod.rs`), the
  `Weather` component = weather volume, Global (follows camera) or Area Box/Sphere with blend
  distance, priority, weight, timeline (Sequence/Random weighted), surfaces, effects, audio,
  Preview in Edit Mode (`component.rs`); runtime `WeatherWorld` on `Scene.weather` (timelines,
  priority blend at the camera, script layer for `Weather.SetState`, wind with gusts, wetness /
  puddles / snow cover accumulation and decay, lightning, events) (`state.rs`); lightning strikes,
  branching bolts, thunder delay = distance / 343 m/s (`lightning.rs`); renderer input
  `WeatherFrame` + post adjustments (Drive Clouds, fog) (`frame.rs`); generated rain/wind/thunder
  WAVs and the Play-mode audio driver through mixer groups (`audio.rs`); CPU tests (`tests.rs`).
- `crates/misty_engine/src/render/weather/`: view group bindings 24/25 (globals + top-down
  occlusion depth map), occlusion pass after the shadow maps, splash compute from the scene-copy
  depth, composite after TAA into the HDR image (haze, far sheets, storm curtains, mist, sky flash,
  procedural GPU rain streaks with refraction, snow flakes with DoF/tumble/swirl, splashes, bolts
  (+ glow image for bloom), lens drops), sun/sky dimming of the lights uniform.
  Lit-shader hook: `weather_surface` in `mesh.wgsl` `shade()`, `weather_foliage` in
  `shade_grass` / `shade_tree`. Quality ▸ `weather: Tier` (counts, splash slots, occlusion size).
- Lua `Weather` table (`scripting/weather_api.rs`) and events OnLightning / OnThunder /
  OnWeatherChanged as broadcast custom events (graphs + Lua). Runtime updates weather before
  scripts; audio driver after.
- Editor: `weather_preview.rs` (edit-mode tick), `panels/inspector/weather_editors.rs` (Inspector).

## Left to do
1. First compile of engine + editor (not yet built). Register the editor pieces: inspector
   module in `panels/inspector/mod.rs` + `editors.rs` dispatch (icon/enabled/edit),
   `weather_preview::tick` in `app.rs` next to `vfx_preview::tick`, Create menu entry
   (actions.rs/commands.rs/menu_bar.rs), Quality settings row, code_editor/api.rs Lua table rows,
   `[weather]` tables in locales tr.toml / ar.toml.
2. Run tests: `weather::tests`, `render::weather::tests` (naga validation), `weather_api` tests.
3. GPU render test (`render/weather/gpu_tests.rs`: rain/snow render, no rain under a roof).
4. WeatherDemo generator (`misty_editor/src/sample_weather.rs`) + Lua key switching + test.
5. GPU timings at 1080p High, screenshots to `docs/screenshots/r34/`, golden check under the
   gpu-timing.lock, docs/progress.md section, docs/api regeneration, fmt/clippy/full tests.
