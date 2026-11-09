# Round 27: the moon, moonlight and stars (branch r27-moon)

## Done
- **Moon settings**: Lighting window ▸ Environment ▸ **Moon** (shown for the Procedural and HDRI
  skyboxes), stored in the scene under `lighting.moon`, left out of the file while default, so
  existing scenes are byte-identical. On by default.
  - Position: **Follow Sun** (default; on the sun's daily circle, Phase × 360° east of it, 5°
    north at new moon so it does not eclipse the sun), **Manual** (azimuth, altitude, Phase
    slider), **Date and Time** (date, local time, UTC offset, latitude, longitude: the real sun
    and moon, `moon::ephemeris`, Meeus chapters 12, 13, 25, 47, 48, with the moon's parallax).
  - Size (× the real 0.52°, default 3), Brightness, Earthshine, Texture (None = built-in),
    Moonlight intensity and colour, Stars.
- **Phases from lighting**: the disc is a sphere lit from the sun's direction (Follow Sun and
  Date and Time: the real sun; Manual: the bright limb towards the sun, turned so the lit
  fraction matches the slider). Lommel–Seeliger/Lambert mix (the full moon is evenly bright),
  soft terminator, crater relief from the built-in height map near the terminator, earthshine on
  the dark side (strongest at new moon). Mip level from texels per pixel; anti-aliased limb.
- **In the sky** (sky.wgsl): drawn before the clouds and fog are blended over the sky, so clouds in
  front hide it, fog and the horizon haze dim it; hidden below the horizon; dimmed and reddened
  by the air mass (Kasten–Young air mass, per-channel transmittance scaled by Atmosphere
  Thickness); a little bloom. Also drawn over HDRI skies.
- **Night sky**: as the sun goes 1°–11.5° below the horizon the procedural sky blends to a dark
  blue night sky that brightens under a high, full moon (GPU `sky_night`, CPU `night_mix`), so
  sky ambient, reflections and the clouds' sky light follow. Unchanged by day.
- **Stars**: a fixed procedural star field (cube-face cells, at least a pixel wide so TAA keeps
  them still, energy-preserving), fading in with the night and towards the horizon, hidden by
  the moon disc, clouds and fog.
- **Moonlight**: reuses the main Directional Light and its shadow path (no second shadowed
  light, no extra cascades). In `Renderer::render`, `moon::night_lights` swaps the renderer's copy
  of the sun light: the sun fades out between 0° and −3.4°, then the moon fades in from −3.4° to
  −11.5° (direction switched while the light is at zero: no pop). Intensity = Moonlight × Allen's
  phase law (quarter ≈ 9 % of full, new ≈ 0) × air-mass transmittance × above-horizon fade;
  cool tint. Volumetric fog, contact shadows, Misty GI and probes see it like the sun.
  Solid Color skies (2D scenes) are never touched.
- **Texture**: `moon::texture_gen` makes the built-in `src/moon/assets/moon.png` (2048 × 1024
  equirectangular, luminance = albedo, alpha = height): the real near-side maria at their
  selenographic positions, highland fbm, ~30 000 power-law craters with bowls, rims, central
  peaks, ejecta, young rayed craters, and named landmarks (Tycho and Copernicus with long rays,
  Kepler, Aristarchus, dark-floored Plato and Grimaldi, Clavius, …). Deterministic (integer
  hashes); `moon_texture_matches_its_generator` checks the committed PNG, `write_moon_texture`
  (ignored) rewrites it.
- **Your own texture** (e.g. made with Codex / an image model): any project PNG/JPG in
  Moon ▸ Texture or `RenderSettings.moonTexture`. 2:1 = equirectangular whole moon (near side
  centred, longitude −180..180 left to right, north up; NASA's CGI Moon Kit colour map works);
  square = a photo of the near side touching the edges. Albedo only: full and evenly lit, no
  phase or shadow. It is re-read when the file changes. Project images are lit as a smooth
  sphere (no height map).
- **Lua**: `RenderSettings.moon`, `moonMode`, `moonPhase`, `moonAzimuth`, `moonAltitude`,
  `moonDate`, `moonTime`, `moonUtcOffset`, `moonLatitude`, `moonLongitude`, `moonSize`,
  `moonBrightness`, `moonEarthshine`, `moonLightIntensity`, `moonLightColor`, `stars`,
  `moonTexture`, read-only `moonIllumination`, `moonDirection`. docs/api regenerated.
- **Sample**: `Scenes/MoonDemo.scene` (generator `sample_moon`): a moonlit valley, standing
  stones casting moon shadows, a cabin with a warm window, dead trees, ground haze, stars;
  `Scripts/MoonCycle.lua` runs the phases round every 40 s.
- **Tests**: lit fraction vs sun–moon angle; phase law; Follow Sun placement; moonlight new vs
  full and by height; smooth twilight (fade functions and the swapped light's colour ×
  intensity in 0.0005 steps of sun height); Meeus reference values (JD, sidereal time, Venus
  alt/az, sun, moon λ/β/Δ/α/δ, lit fraction 0.6786) and the 2024-04-08 Dallas total eclipse
  (moon over the sun within 0.2° only with parallax), 2024 full / quarter moons; texture
  generator and maria placement; settings round trip; Lua; GPU render of the disc at full,
  quarter, crescent and new; sample generator, play test.

## Not done yet
- The clouds are still lit by the sun only: at night they get the dark sky light but no
  moonlight (no moonlit silver linings). Needs the cloud march to take the moon as its light
  while the sun is down (render/clouds/uniforms.rs `sun_light` / `sun_direction`); left out to
  keep clear of r26's sun-height work in the same code.
- No moon shafts (sun shafts only look for the sun) and no lens flare / halo for the moon.
- Stars do not turn with the sidereal time in Date and Time mode, and there is no Milky Way or
  real star catalogue.
- When r26's "Intensity from Sun Height" lands, its air-mass transmittance and `moon::
  transmittance` / `moon::air_mass` should become one function.
- Follow Sun keeps the moon on the sun's declination (plus 5°), not the ecliptic: a summer full
  moon rides as high as the summer sun.
- No eclipses (the moon does not hide the sun disc, no Earth shadow on the moon).
- The night sky colours are hand-tuned, not a scattering model.
