# Audio: mixer routing and spatial depth (round 19D, branch r19-audio)

Goal: the familiar engine's Audio Mixer and AudioSource behaviour — sends, receives and ducking,
more effects with editable chains, nested mixers, weighted snapshot blends, graph edits while
playing, binaural 3D sound, custom rolloff curves and streamed clips. The behaviour follows the
familiar engine; the code, algorithms and file formats are our own.

## The mixer graph

Sound is no longer mixed by kira's tracks. `audio/engine.rs` is our own graph; kira only opens
the device and plays the graph as one endless sound (`EngineSound`). The same graph renders
offline (`GraphBackend::offline`, `Audio::offline`, `Audio::render`) so every test checks real
rendered buffers without a sound device. The silent `NullBackend` still records calls for the
bookkeeping tests.

| Part | How it behaves | Where |
|---|---|---|
| Voices | Read a decoded clip or a stream; 4-point Hermite resampling at pitch × clip rate / output rate; the reference engine's equal-power pan law; binaural rendering for spatialized sources; click-free pause, resume and stop (5 ms gate). | `engine.rs` (`Voice`, `Reader`) |
| Tracks | Mixer groups and per-source filter chains. Each 256-frame block a track runs its input (voices + child tracks) through its chain, applies its fader (smoothed) and adds into its parent. | `engine.rs` (`Track`, `process_track`) |
| Order | Tracks run in dependency order (children and senders before parents and receivers), so a send lands in the same block. A loop of sends is cut where it closes and delivers one block late. | `dependency_order` |
| Commands / status | Main thread → audio thread through a lock-free ring (`rtrb`); levels, gain reduction, positions and finished voices come back through a status block updated with `try_lock` (the audio thread never waits). The backend mirrors what the graph has and sends only changes. | `graph_backend.rs` |

### Routing units

| Unit | Behaviour | Parameters |
|---|---|---|
| Send | Copies the signal at its place in the chain to a Receive or Duck Volume on another group of the same mixer. | Send level (dB, -80 = off; the reference default) |
| Receive | Mixes what Sends deliver into its group at its place in the chain. | — |
| Duck Volume | A side-chain compressor: the level it is sent (mixed with its own signal by Sidechain mix) turns its group down. Peak envelope with attack / release, soft knee, ratio. | Threshold, Ratio, Attack, Release, Make up gain, Knee, Sidechain mix |

Send targets are effect ids (`MixerEffect::target`); removing a receiver clears the sends to it.

### Effects

Every effect has a place in an editable chain (drag a slot, or Move Up / Down), a bypass and a
wet mix level (`"<group>/<effect>/Wet"`, 0–100 %, in snapshots and exposable).

| Effect | Algorithm | Parameters |
|---|---|---|
| Lowpass / Highpass | RBJ biquad; Resonance 1 = flat (Butterworth) corner | Cutoff freq, Resonance |
| ParamEQ | RBJ peaking biquad; gain at the centre, bandwidth in octaves | Center freq, Octave range, Frequency gain (×) |
| Echo | Damped feedback delay; delay changes glide (50 ms) instead of jumping | Delay, Decay, Dry mix, Wet mix |
| Reverb | Schroeder–Moorer network: 8 lowpass-feedback combs + 4 allpasses per side | Room size, Damping, Width, Wet mix |
| Distortion | tanh soft clip | Level |
| Compressor | Feed-forward peak compressor | Threshold, Ratio, Attack, Release, Make up gain |
| Chorus | Three modulated delay taps (left, right, both) at 120° LFO phases | Dry mix, Wet mix tap 1–3, Delay, Rate, Depth |
| Flange | Short swept delay, right side a quarter cycle behind | Dry mix, Wet mix, Depth, Rate |
| Pitch Shifter | Phase vocoder: Hann frames, true-frequency estimate per bin from the phase advance, bins moved to `pitch ×` their frequency and resynthesised; a steady tone comes out at exactly the shifted frequency | Pitch, FFT size, Overlap |

### Live editing

Edits in the Mixer window (or by script) change the running graph in place; no source restarts:

- inserted effects fade in, removed ones fade out where they were (reverb and echo tails ring on);
- effects that keep their order keep their state;
- a reordered chain crossfades from the whole old chain (still ringing) to the new one;
- bypass and wet changes are ramps;
- a rerouted group or moved voice feeds both destinations for 20 ms;
- voices of a removed group move to its nearest surviving parent.

### Nested mixers and snapshots

- A mixer's **Output** (mixer Inspector, or `mixer.outputAudioMixerGroup`) is a group of another
  mixer; its Master plays into that group's track and group pitches multiply through. Loops are
  refused with a warning.
- **Snapshot weights**: `TransitionToSnapshots(snapshots, weights, time)` moves every value
  linearly to `Σ wᵢ·vᵢ / Σ wᵢ` (weights ≤ 0 count for nothing; all zero keeps the current
  values). `TransitionTo` is the one-snapshot case.

## Spatial audio

| Feature | Behaviour | Where |
|---|---|---|
| Spatialize (HRTF) | A spherical-head model (the structural model from the spatial-audio literature, implemented from its equations; no measured data): per-ear delay round a 8.75 cm head (ITD up to 0.66 ms), a head-shadow shelf from +6 dB facing the ear to -20 dB shadowed (corner c/a ≈ 620 Hz), and a duller top end behind the listener. Delays glide across blocks so moving sources don't click. The 3D share (spatial blend) is binaural, the 2D share panned. | `dsp/hrtf.rs`, `spatial::mix` |
| Spread | Narrows the pan as before; for spatialized sources it moves the sideways part of the direction to the front, so 180° is centred and 360° mirrored. | `spatial::mix` |
| Doppler | Unchanged maths, now checked on rendered audio: a tone approaching at 34.3 m/s comes out at 343 / 308.7 × its pitch. | `render_tests::doppler_shifts_the_rendered_pitch` |
| Custom rolloff | `AudioRolloff::Custom` with a curve of (distance as a share of Max Distance, volume) keys joined by a monotone cubic (no overshoot). The Inspector's rolloff graph becomes a curve editor (drag keys, double-click adds, right-click deletes). | `spatial::RolloffCurve`, `inspector/audio_editors.rs` |

## Clips

Import settings live in the clip's `.meta` (`audio::clip::AudioImporter`) and show in the clip
Inspector with format, rate, channels and length:

- **Decompress On Load** (default): decoded into memory when first played.
- **Compressed In Memory**: the encoded file stays in memory; each voice decodes it on a worker
  thread while playing.
- **Streaming**: read from disk and decoded on a worker thread into a lock-free ring (≈1.4 s at
  48 kHz); seeks and loop points are handled by the worker; frames carry a generation number so a
  seek drops what was decoded before it. Inside a packed build it streams the packed bytes.
- **Force To Mono**.

Formats are the decoders kira already brings through symphonia (MPL-2.0): WAV/PCM, OGG Vorbis
and MP3. Vorbis was already there, so no `lewton` was added; symphonia (same version and
features as kira's) and `rtrb` (MIT / Apache-2.0, already a kira dependency) are now direct
dependencies.

## Lua

`AudioMixer`: `SetFloat` / `GetFloat` / `ClearFloat` on exposed parameters (volumes, pitches,
any effect parameter, Send levels, wet levels), `FindSnapshot`, `TransitionTo`,
`TransitionToSnapshots(snapshots, weights, time)`, `outputAudioMixerGroup` (nested mixers),
`FindMatchingGroups`. `AudioSource`: `spatialize`, `rolloffMode = "Custom"`,
`SetCustomCurve("CustomRolloff", keys)` / `GetCustomCurve`.

## Editor

- **Audio Mixer window**: slots say where a Send goes (`Send → Music`) and how many feed a
  Receive / Duck Volume (`Duck Volume ← 1`); routing units are tinted; in Play mode a Duck Volume
  or Compressor slot shows an orange gain-reduction bar; drag a slot onto another to reorder;
  Add... lists the routing units after a separator.
- **Mixer Inspector**: a Send's Receive picker (the other groups' Receive / Duck Volume units),
  "Sends from" on receivers, Wet Mix Level on every signal effect, live gain reduction, and a
  response curve for Lowpass, Highpass and ParamEQ. The mixer summary has the Output picker.
- **Audio Source**: Spatialize; the rolloff graph edits Custom curves.
- **Audio Clip**: format line, Load Type, Force To Mono.

## Sample: AudioDemo2

`Assets/Scenes/AudioDemo2.scene` (generator `sample_audio2.rs`, CC0 synthesised clips):
music (a 12 s score imported as Streaming) ducked by the radio's dialogue through a Send to a
Duck Volume; Dialogue and SFX send to a Reverb bus; the camera walks into a cave (Audio Reverb
Zone) and `AudioDemo2.lua` blends Outdoor and Indoor snapshots with `TransitionToSnapshots` by
how far inside it is (music muffled, reverb sends up); a spatialized buzzer orbits the listener's
head; the radio fades along a Custom rolloff curve. Keys: D dialogue, Space clap, M music, W/S
walk.

## Tests (offline, rendered buffers)

- Effects (`dsp/fx_tests.rs`): ParamEQ gain at its centre and band edge, filter corners
  (-3 dB, 12 dB/octave, resonance), compressor gain reduction (-6 dBFS through -20 dB 4:1 →
  -16.5 dBFS), soft knee, pitch shift ratio by frequency detection (×0.75, 1, 1.5, 2 within 1 %),
  echo repeats, reverb tail vs room size, distortion harmonics, chorus / flange taps.
- Graph (`graph_tests.rs`): send / receive levels (-6 dB send = 0.25, with the dry path 0.75,
  receiving fader, bypass), sends to later tracks in the same block and send loops, the ducking
  envelope (depth -21.6 dB at 10:1, 10 dB of attack within ~13 ms, back within 1 dB 0.79 s after
  the dialogue stops), chain and routing edits while playing (largest sample jump below a sine's
  own slope; no restart), removing a track, streamed and compressed clips sample-identical to
  decoded ones (plain, looping, resampled, after a seek), clip info and Force To Mono.
- Scenes (`render_tests.rs`): nested mixer level and pitch, nested loops refused, HRTF level and
  time difference for a side source (ILD > 6 dB, ITD = 0.66 ms ± 1.5 samples), spread centring,
  Doppler on rendered pitch, custom rolloff volume, mixer edits while playing without clicks.
- Snapshot weights (`mixer_state.rs`), the Lua API (`scripting/audio_tests.rs`), kittests for the
  Mixer window with sends, drag reordering and wet levels, the clip import settings, Spatialize
  and the curve editor; AudioDemo2 matches its generator and plays headlessly (ducks, blends).

## CPU cost

Measured with `cargo test --release -p misty_engine -- --ignored mixer_cost_report --nocapture`
(this machine, 48 kHz stereo, 10 s rendered offline): audio-thread CPU per second of sound, and
as a share of one core.

| What | µs per second | % of a core |
|---|---|---|
| Graph overhead (one empty track) | 348 | 0.035 |
| Voice, 2D, pitch 1 | 920 | 0.092 |
| Voice, 2D, resampled | 1081 | 0.108 |
| Voice, spatialized (HRTF) | 2148 | 0.215 |
| Lowpass / Highpass / ParamEQ | 241–244 | 0.024 |
| Echo | 478 | 0.048 |
| Duck Volume | 521 | 0.052 |
| Compressor | 744 | 0.074 |
| Distortion | 1341 | 0.134 |
| Flange | 1699 | 0.170 |
| Reverb | 1960 | 0.196 |
| Chorus | 2332 | 0.233 |
| Pitch Shifter (FFT 1024, overlap 4) | 27879 | 2.788 |

So 32 spatialized voices take about 7 % of one core; the phase vocoder is by far the dearest
unit (two FFTs per hop per side). Streamed voices decode on their own worker threads (not in
these numbers).

## Screenshots (`docs/screenshots/r19/`)

- `audio_mixer_sends.png`: AudioDemo2's mixer: Sends (Dialogue → Duck Volume on Music and →
  Reverb), the Reverb bus's Receive ← 2, and the Send's Receive picker in the Inspector.
- `audio_play_ducking.png`: Play mode on the sound device: live meters, the Duck Volume reducing
  Music by 13 dB while the radio talks (orange bar on the slot, readout in the Inspector).
- `audio_play_indoor_blend.png`: inside the cave: Indoor 1.00 via `TransitionToSnapshots`, the
  Music lowpass sweeping down (response curve), the reverb bus loud.
- `audio_source_custom_rolloff.png`: Spatialize and the Custom rolloff curve editor.
- `audio_clip_streaming.png`: the score's import settings (Streaming) and format line.

## Not done yet

- Measured HRTF data sets (the head model has no elevation cues beyond the rear shelf).
- Encoding at import (the original file is kept; no transcoding to Vorbis/ADPCM), Preload Audio
  Data / Load In Background options.
- Mixer "views", effect presets, and spread curves / reverb-zone-mix curves over distance.
- Sends across mixers (as in the reference, sends stay inside one mixer).
