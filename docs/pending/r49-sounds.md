# r49: sounds for the demo scenes, the weather and the First Person Player

Branch `r49-sounds` (from main 16543191). Target `E:\MistyTargets\r49-sounds`, TMP/TEMP
`C:\MistyTry\r49tmp`.

The user asked "is there supposed to be sounds?": the scenes were silent. Sound effects were
made for the project in ElevenLabs (48 kHz stereo WAV, loops generated as seamless loops):
meadow ambience, forest birds, breeze over grass, steady rain, heavy storm rain (30 s loops)
and 10 s of footsteps on grass.

## Assets (`SampleProject/Assets/Audio`)

Prepared offline by `tools/sample_sounds.py <sfx_dir> SampleProject` (numpy, scipy, PyAV):

| File | Source loudness | Seam check (HF energy over the seam / its 100 ms neighbourhood) | Size |
| --- | --- | --- | --- |
| `Ambience/meadow_ambience_loop.ogg` | -46.0 LUFS | 1.19, decoded 0.95 | 390 KiB |
| `Ambience/forest_birds_loop.ogg` | -38.2 LUFS | 1.12, decoded 1.08 | 404 KiB |
| `Ambience/breeze_grass_loop.ogg` | -45.7 LUFS | 1.44, decoded 1.23 | 400 KiB |
| `Weather/rain_steady_loop.ogg` | -41.0 LUFS | 1.20, decoded 1.13 | 441 KiB |
| `Weather/storm_heavy_rain_loop.ogg` | -18.3 LUFS | 0.90, decoded 0.86 | 416 KiB |
| `Footsteps/footstep_grass_01..06.wav` | | | 234 KiB (6 clips, 230-432 ms, mono) |

- **Seams**: none of the loops clicks (a click would be ~10x its neighbourhood; the script
  would then crossfade 40 ms equal-power at the seam). The raw jump over each seam is below
  0.01 and within the file's ordinary sample-to-sample steps.
- **Loudness**: every loop at -24 LUFS (BS.1770 gated) under a -1 dBFS look-ahead limiter run
  circularly (the gain is continuous over the seam). Only the steady rain touched the
  limiter (droplet peaks, -24.4 LUFS after it).
- **Format**: Ogg Vorbis (libvorbis via PyAV, 112 kb/s ambience, 128 kb/s rain), stereo kept
  (side/mid -1 to +0.6 dB: wide ambiences). 30 s x 5 loops: 28.8 MB of WAV -> 2.0 MB.
  Decoded again: exactly 1,440,000 frames each and the seam still clean. The engine side is
  tested too (`bundled_rain_loops_decode_seamlessly`: looping playback repeats sample-exact
  across the loop point, kira's full decode gives the same length). Import settings:
  Compressed In Memory (a 30 s loop decoded whole is 11 MB).
- **Footsteps**: onsets from a high-passed 5 ms envelope; the six loudest steps (at 2.96,
  3.44, 4.06, 5.16, 5.70, 7.20 s) cut 12 ms before the onset, ended before the next step or
  where the step has died away, 3 ms fade in / up to 60 ms fade out, folded to mono (the
  recording is nearly mono: side/mid -16 dB), -20 LUFS each, peaks under -3 dBFS.
- Repository size added: about 2.3 MB of audio.

## What plays where

- **NoonValley** and **MeadowDemo** (`sample_ambience.rs`, `Assets/Scripts/AmbienceZones.lua`,
  added by the scenes' generators): an "Ambience" object with the meadow loop (2D) and the
  forest birds (3D, Linear rolloff 25-160 m, spread 120). The script moves the birds to the
  closest point of the nearest forest area (box footprints from the generator) with a short
  glide, turns them 2D within 20 m (inside the wood they surround you) and fades the meadow
  loop to a quarter over 50 m as you go in. NoonValley: areas along the forested hillside's
  foot, the treeline wood ahead, the treeline behind (checked against the generator's
  `forest()` / `wood_depth()`); the start is ~90 m from the hillside. MeadowDemo: one area per
  grove, plus a quiet breeze loop (2D) over the windy field. MeadowDemo's scene keeps the
  user's edits: `write_meadow_demo` / `write_meadow_rocks` replace only the Rocks and Ambience.
- **Weather** (engine, `weather/audio.rs`): the bundled steady rain and storm recordings are
  built into the engine (written to `Library/Weather` on first use, as the generated wind and
  thunder already were). Volume follows the rain intensity: silent when dry, rising with it,
  steady rain crossfading (equal power) into the storm loop from intensity 0.45 to 0.9. Under
  cover it drops to Under Cover (0.35): nine rays cast up from the Audio Listener (Player layer
  and Ignore Raycast skipped) give the share of sky that is covered; only with Sheltered Areas
  on. Every level eases (0.6 s; cover 0.35 s), so the rain fades in and out with timeline
  transitions, when Play starts and when you step under a roof. Wind keeps more under a roof.
  Snow: silent except the existing wind loop in strong wind and blizzards.
- **Weather settings**: Audio ▸ Sound, Volume, Under Cover, Rain Loop, Storm Loop (empty =
  the built-in recordings), Wind Clip, Thunder Clip (empty = the generated thunder, which
  already arrives `distance / 343 m/s` after each flash), Output. New fields (`storm_clip`,
  `under_cover`) are left out of files at their defaults: old scenes load and re-save
  unchanged (tested). A scene that set its own Rain Clip now uses it for the steady loop
  only; the storm loop is the built-in one unless Storm Loop is set.
- **Footsteps** (`FirstPersonController.lua`): a random grass step (never the same clip twice
  in a row) at the feet on every step on the ground: each half stride of the controller's
  stride phase, so the rate follows the speed and the walk/run step lengths. Louder running,
  softer crouched, a thump on landing; each step's volume +-15 % and pitch +-6 %. Settings:
  Footsteps, Footstep Clips (comma-separated), Footstep Volume, Footstep Volume Variation,
  Footstep Pitch Variation. Played with `AudioSource.PlayClipAtPoint(clip, position, volume,
  pitch)` (new optional pitch argument), so the prefab and the scenes holding it are
  unchanged. GameObject ▸ First Person Player writes the clips into a project that lacks them;
  builds and templates pack them (the default list names each clip in its own literal).

## Checks

(filled in below as they run)

## Left

- No thunder recording yet: the generated thunder plays (a Thunder Clip slot takes one).
- Cover is from colliders only: terrain tree crowns (no colliders) don't muffle the rain.
- Footsteps play the grass clips on every surface (no per-surface sets).
