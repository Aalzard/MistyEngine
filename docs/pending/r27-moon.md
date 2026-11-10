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
  Date and Time: the real sun; Manual: lit as the sky would for the slider's phase, from the
  west while waxing and the east while waning). Lommel–Seeliger/Lambert mix (the full moon is evenly bright),
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
- **Moonlit clouds**: once the sun's own light on the clouds is gone (4.6° down), the cloud
  march and the cloud shadow map take the moon as their light (`moon::cloud_light`, two hooks in
  clouds/uniforms.rs), so clouds get moonlit edges and cast moon shadows.
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

## Night sky (second part of the round, asked after the moon screenshots)
- Lighting ▸ Environment ▸ **Night Sky** (Procedural sky; `lighting.night_sky`, left out while
  default; every default reproduces the first part's night): Night Sky Darkness (darkens the
  sky colour, the stars stay), Sky Time (sidereal hours; Date and Time mode uses the date), and
  collapsible groups:
  - **Stars**: Brightness, Density, Magnitude Spread (a few bright stars among many faint ones),
    Glow (halo + bloom on the bright ones), Color Variation (orange to blue-white), Twinkle and
    Twinkle Speed (stronger near the horizon), Fade in Bright Sky (moonlight, twilight and town
    glow wash out the faint stars and the Milky Way).
  - **Milky Way**: Visibility, Core, Dust Lanes, Width, Tint. Galactic band from the real
    galactic pole and centre on the celestial sphere (turns with the latitude and sidereal time
    like the stars): widening, warmer core towards Sagittarius, star clouds, the Great Rift.
  - **Light Pollution**: Amount, Color (horizon glow; also lifts the sky light and washes stars).
  - **Aurora**: Visibility, Color, Top Color, Height, Thickness (km), Curtains (1–6),
    Waviness, Ray Detail, Direction, Distance (far = low on the sky), Speed, Pulse, Scene Light.
    Ray-marched emissive slab ~100 km up (24 steps High, 10 Low, per-pixel jitter smoothed by
    TAA): folded curtains that wave and drift, vertical rays, sharp lower edge, colour gradient to
    the top, pulsing; it tints the sky light (ambient) a little.
  - **Meteors**: Per Minute, Brightness: streaks with a bright head along great circles, bloom.
- Animated on the renderer's clock (the grass clock, fixed by `Environment::grass_time` in
  tests). Quality ▸ Volumetric Clouds Low / Off take the cheap path (fewer aurora steps and noise
  octaves).
- Lua: `RenderSettings.nightSky`, `nightSkyDarkness`, `stars`, `starDensity`,
  `starMagnitudeSpread`, `starGlow`, `starColorVariation`, `starTwinkle`, `starTwinkleSpeed`,
  `starFade`, `skyTime`, `milkyWay`, `milkyWayCore`, `milkyWayDust`, `milkyWayWidth`,
  `milkyWayTint`, `lightPollution`, `lightPollutionColor`, `aurora`, `auroraColor`,
  `auroraTopColor`, `auroraHeight`, `auroraThickness`, `auroraCurtains`, `auroraWaviness`,
  `auroraRays`, `auroraAzimuth`, `auroraDistance`, `auroraSpeed`, `auroraPulse`,
  `auroraAmbient`, `meteorRate`, `meteorBrightness`.
- Sample `Scenes/NightSkyDemo.scene` (same generator as MoonDemo): the valley looking north under
  a thin setting crescent, Milky Way, glowing twinkling stars, an animated aurora, meteors, a
  faint town glow.
- Tests: the sky frame (Polaris north at the latitude's height, westward turn, galactic centre
  where Sagittarius is, Meeus sidereal time), settings round trip, the controls (darkness keeps
  the stars, town glow on the horizon, aurora sky light), packed layout, and a GPU test (aurora
  green in the north and moving, Milky Way and town glow brighten, a meteor streak).

## Screenshots (docs/screenshots/r27/)
- `moon_phases.jpg` (strip) and `moon_waxing_crescent.jpg`, `moon_first_quarter.jpg`,
  `moon_waxing_gibbous.jpg`, `moon_full.jpg`, `moon_waning_gibbous.jpg`, `moon_last_quarter.jpg`
  (telephoto, MoonDemo's sky).
- `moonlit_night_full.jpg`, `moonlit_night_crescent.jpg` (MoonDemo), `twilight_moonrise.jpg`
  (sun 4° down, nearly full moon rising). Rendered by `sample_moon::render_moon_screenshots`.

## Not done yet
- No moon shafts (sun shafts only look for the sun) and no lens flare / halo for the moon.
- No real star catalogue (procedural stars; the Milky Way is placed correctly, the stars are
  not real constellations).
- The aurora is not reflected by the water (water reflects the analytic sky colour, which only
  carries the aurora's faint sky light); no aurora or meteor events for Lua / Script Graphs; no
  moon halo / corona in thin cloud; twinkle does not look at the exposure.
- When r26's "Intensity from Sun Height" lands, its air-mass transmittance and `moon::
  transmittance` / `moon::air_mass` should become one function.
- Follow Sun keeps the moon on the sun's declination (plus 5°), not the ecliptic: a summer full
  moon rides as high as the summer sun.
- No eclipses (the moon does not hide the sun disc, no Earth shadow on the moon).
- The night sky colours are hand-tuned, not a scattering model.
