# Round 14 — Video Player and Rich Text

## Progress

### Video

- **Video Clip assets** (`misty_engine::video`): Motion-JPEG AVI (`.avi`, MJPG / JPEG / AVRn /
  dmb1 frames or uncompressed 24/32-bit RGB, 8/16-bit PCM audio, OpenDML `AVIX` parts) and
  animated GIF (`.gif`, per-frame delays). The AVI reader (`video::avi`) walks the `movi` lists
  itself (no `idx1` needed) and inserts the standard Huffman tables into MJPEG frames that leave
  them out. `.mp4 .m4v .mov .webm .mkv .ogv .wmv .flv .mpg` are recognised as video and explain
  they can't be played, with the ffmpeg command that converts them. **Import settings** in the
  clip's `.meta` (`VideoImporter`: Import Audio, Transcode Quality); the Inspector shows codec,
  size, fps, frame count, length and audio format, a preview with scrub bar, and **Transcode to
  Motion-JPEG AVI** for clips that aren't MJPEG (GIF / uncompressed → AVI, pure Rust).
- **Video Player component** (Add Component ▸ Video): Source (Video Clip / URL = local file path,
  absolute or project-relative or `file://`), Play On Awake, Wait For First Frame, Loop, Skip On
  Drop, Playback Speed (0.01–10), Render Mode (Camera Far Plane, Camera Near Plane, Render
  Texture, Material Override, API Only) with Camera + Alpha + Aspect Ratio (No Scaling, Fit
  Vertically / Horizontally / Inside / Outside, Stretch), Target Texture name, or Renderer +
  Material Property (Main Texture / Emission Map) + element; Audio Output Mode (None, Direct
  with Volume, Audio Source) and Mute. Object references are remapped in scene files
  (`VIDEO_REFS`). Inspector: all of the above plus the clip preview and scrub bar.
- **Playback** (`VideoPlayback`, in the script host's world next to the timeline player): a
  worker thread per player decodes frames in order into a 4-frame queue (`video::decoder`);
  seeking bumps a generation number so stale frames are dropped; looping continues the frame
  sequence across the loop point. The game thread advances the clock by `dt × speed`, shows the
  newest decoded frame not after the clock (or every frame in turn without Skip On Drop),
  counts dropped frames and fires events: `prepareCompleted` (first frame decoded), `started`,
  `loopPointReached` (once per pass, looping or not), `errorReceived`.
- **Frames on the GPU**: frames are published as runtime textures (`video://N` per player,
  `runtime://<name>` for Render Texture targets; `video::textures`). The material texture cache
  and the UI renderer keep one GPU texture per name (`render::runtime_tex`) and re-upload it when
  a new frame is published, so any material texture slot or Raw Image can show video by name.
  Material Override writes the player's texture into the renderer's per-object overrides
  (`MaterialOverrides::main_texture` / `emission_texture`, new, left out of files when unset)
  while playing and removes it on Stop. Camera planes are drawn by the UI renderer before the
  canvases: Near Plane over the image, Far Plane through a second pipeline that reads the scene
  depth buffer and only draws where it is still at its clear value (the sky).
- **Audio**: the clip's PCM track is turned into an in-memory WAV and handed to the sound
  backend under the clip path `<clip>#audio` (`AudioBackend::preload`, new, default no-op);
  it plays through the chosen Audio Source (its volume, 3D settings and mixer group) or as a
  direct 2D voice that follows the playback speed; it restarts at each loop point and follows
  seeks, pause and stop.
- **Lua** `GetComponent("VideoPlayer")`: `Play() Pause() Stop() Prepare()`, `time` / `frame`
  (writable = seek), `length`, `frameCount`, `frameRate`, `isPlaying`, `isPaused`,
  `isPrepared`, `isLooping` (writable), `playbackSpeed`, `playOnAwake`, `clip`, `url`,
  `texture`, `width`, `height`, `gameObject`, events `started`, `loopPointReached`,
  `prepareCompleted`, `errorReceived` (`vp.started:AddListener(function(vp) … end)`; the error
  listener also gets the message).
- **Builds**: clips referenced by scenes are packed like other assets; `.avi` / `.gif` are
  stored without LZ4 (already compressed).

### Rich text ("Misty Rich Text")

- **Markup** in UI Text (`ui::rich::parse`), on with the new **Rich Text** checkbox (default on,
  also for older scenes, left out of files while on): `<b> <i> <u> <s> <color=#RGB[A]|#RRGGBB[AA]|name>
  <size=N|N%|+N|-N> <alpha=#AA> <mark=#RRGGBB[AA]> <sup> <sub> <br> <sprite name=… | index=… |
  =N [tint=1] [color=…]> <link="id"> <align=left|center|right>`, `<noparse>…</noparse>` and
  `&lt; &gt; &amp;`. Unknown or malformed tags stay in the text as typed; closing tags pop their
  own kind only.
- **Layout** (`ui::rich::layout`): per-character size, super/subscript, bold advance, kerning,
  character and word spacing (hundredths of an em), word wrapping, line spacing, per-line
  `<align>`, overflow modes **Overflow / Ellipsis / Truncate / Page** (with Page number),
  **Auto Size** between min and max (binary search for the largest size that fits), inline
  sprites from the text's **Sprite Asset** (a Multiple-mode sprite texture), underline /
  strikethrough / mark rectangles merged per word, and link boxes per line.
- **Right-to-left**: Unicode bidi levels (unicode-bidi) reorder each line for display (rule L2);
  right-to-left runs are shaped with rustybuzz (Arabic joining forms, marks).
- **Fonts**: the text's **Font** (a project `.ttf` / `.otf`, listed in the Inspector, packed in
  builds) or the built-in font, then the built-in font, then operating-system fonts with wide
  script coverage (Segoe UI / Arial / Tahoma on Windows, DejaVu / Noto on Linux, Arial Unicode
  on macOS) for characters the others lack — like dynamic OS fonts in the reference engine.
- **Distance field glyphs** (`ui::rich::sdf`): each glyph is rendered once at 42 px per em (4×
  supersampled coverage → exact Euclidean distance transform, 8 px spread) into a 1024² R8
  atlas; the UI shader draws them with screen-space anti-aliasing, so text stays crisp at any
  size. **Effects** (Text ▸ Effects): Outline (width, colour), Shadow (offset, colour,
  softness) and Gradient (four corner colours) — drawn from the same atlas entry by moving and
  widening the distance threshold; bold is a thicker threshold, italic a skewed quad.
- **Links**: clicking a `<link>` (press and release over the same link) calls
  `OnLinkClicked(id)` on the text object's scripts that define it and runs listeners added with
  `text.onLinkClicked:AddListener(function(id) … end)` (`UiEvent::LinkClicked`, new). Lua Text
  also gained `richText` and `page`.
- **Compatibility**: a text only takes the rich path when it uses markup, effects, a font asset,
  spacing, an overflow mode, auto-size or a right-to-left character; everything else renders
  through the old bitmap path exactly as before. The new UiText fields are left out of files
  at their defaults, so existing scenes load and save byte-for-byte unchanged.

### Demo

**VideoTextDemo** (`sample_video_text.rs`): a television in a dim room plays the generated test
clip (`Assets/Video/TestPattern.avi`: 256×144, 20 fps, 3 s, scrolling colour bars, a progress
block, the frame number as ten black/white squares and a beep at the start of each second) via
Material Override with its audio through the screen's Audio Source; a UI panel shows the rich
text features (gradient + outline + shadow title, colours, styles, sizes, mark, sup/sub,
inline sprites from `Assets/Video/RichIcons.png`, a link logged by `LinkLogger.lua`, a faded
right-aligned line, an Arabic + Hebrew line, an ellipsis box), and a Raw Image shows the clip
again through a Render Texture player at half speed.

## Decisions

1. **Motion-JPEG AVI + GIF as the video formats.** They decode with pure-Rust code already in
   the dependency tree (the image crate's JPEG decoder; the `gif` crate via the image crate's
   `gif` feature), seek exactly and cheaply (every frame is a key frame), and ffmpeg and most
   editors can write MJPEG AVI. Modern codecs were considered: H.264/HEVC have no mature
   pure-Rust decoder (openh264 is C++ and ships prebuilt binaries), and the AV1 options are
   either C (dav1d) or large ports with assembly (rav1d), so they would break the "pure Rust /
   bundled source" rule and cross-compilation. Unsupported containers explain how to convert.
2. **Transcoder only where a pure-Rust path exists** (GIF / uncompressed AVI → MJPEG AVI). There
   is no in-editor converter for .mp4 etc.; the message gives the ffmpeg command instead.
3. **Decoding on one worker thread per player with a 4-frame bounded queue**: enough to hide
   JPEG decode jitter at 30–60 fps without holding many frames in memory; generations make
   seeks cheap and race-free.
4. **Video clock is master**, audio follows it at start, seek, pause and loop points (no
   continuous drift correction): deterministic in tests and good enough for clips of normal
   length; the audio backend gets the whole track as one in-memory WAV.
5. **Runtime textures by name** instead of a Render Texture asset type: the existing texture
   paths (material slots, Raw Image) already route by string, so `video://` and `runtime://`
   names plug video into every consumer with one small cache per renderer. A real Render Texture
   asset (camera output) can reuse the same mechanism later.
6. **Camera planes in the UI pass**: the Far Plane reads the scene depth buffer to stay behind
   geometry; both planes are composited after post-processing (no bloom / fog on the video),
   like a background / overlay video.
7. **Rich text extends UI Text** rather than a second text component: one component keeps the
   Localized Text and layout code paths working; the system's own name is "Misty Rich Text".
   Rich Text defaults on (also for old scenes, which render the same unless they contain
   markup), and plain texts keep the old bitmap path so existing scenes are pixel-identical.
8. **SDF (single channel) generated at first use** from the font outlines with a 4×
   supersampled exact distance transform rather than MSDF: no extra dependency, good corners at
   UI sizes, and outline / shadow / bold come from the same field. Glyphs are generated on
   demand (the atlas is rebuilt when full) instead of a baked asset file, so any font asset and
   the OS fallbacks work without an import step.
9. **unicode-bidi + rustybuzz for right-to-left text**: both pure Rust, well known, and
   rustybuzz shares ttf-parser with ab_glyph. Shaping is only used for right-to-left runs, so
   Latin layout (and its metrics) is unchanged.
10. **OS font fallback** for scripts the bundled font lacks: shipping an Arabic/Hebrew font in
    the repository was avoided; the system's fonts are used at runtime (missing glyphs show as
    the font's missing-glyph box when none is installed).

## New dependencies

| Crate | Version | License | Why |
| --- | --- | --- | --- |
| unicode-bidi | 0.3.18 | MIT OR Apache-2.0 | Bidi levels for right-to-left text |
| rustybuzz | 0.20.1 | MIT | Shaping right-to-left runs (Arabic joining) |
| (rustybuzz deps) unicode-script, unicode-ccc, unicode-properties, unicode-bidi-mirroring, core_maths, smallvec, bitflags | — | MIT / Apache-2.0 | |
| gif (image crate feature `gif`) | 0.14 | MIT OR Apache-2.0 | Animated GIF clips (with weezl, color_quant: MIT/Apache) |

## Tests

- `video::tests`: generated clip round-trips (frame count, fps, frame-number squares and bar
  colours at frames 0/7/23/39), MJPEG frames without Huffman tables decode identically after the
  standard tables are inserted, audio lines up with video time (beep onset at 1 s ±2 ms = frame
  20), unsupported formats, GIF clips + transcoding, decoder thread order + seek, player
  prepare/play/loop with events fired once per loop and Material Override set and removed,
  non-looping end, seeking shows the requested frame, audio voice position follows seeks,
  missing clip error event, aspect ratio modes, Render Texture players publish under their
  target name (`runtime://<name>`, regression: they used their own `video://N` name, so Raw
  Images showing the target stayed black).
- `scripting::video_api` tests: Lua events and controls (prepared/started once, pause/seek/
  play, two loop events, stop); clicking a rich text link calls `OnLinkClicked` and listeners.
- `ui::rich::tests`: parser nesting / unknown tags / escapes / values, wrapping with spacing,
  per-line alignment and sizes, ellipsis / truncate / pages, auto-size, link hit testing,
  decoration merging, right-to-left reordering, Arabic shaping (when an OS font has Arabic),
  `needs_rich`, SDF glyph quality (IoU with the coverage > 0.9).
- `sample_video_text`: committed demo matches its generator, the demo clip decodes, and a
  headless render shows colour bars on the screen and rich text on the panel.

## Left / ideas

- Modern codecs (H.264/VP9/AV1) once a pure-Rust decoder is mature; streaming from http(s).
- A Render Texture asset (camera output) using the runtime texture mechanism.
- MSDF for sharper corners at very large sizes; a pre-baked font asset file for faster start.
- Text: justified alignment, `<font>`, `<voffset>`, `<cspace>`, `<indent>` and other tags;
  bidi bracket mirroring outside shaped runs.
- Video audio during editor pause: direct voices are not paused by the editor's Pause button
  (Audio Source voices are).
- Seen twice in this round: the workspace-built `misty_engine` test binary exited with
  0xc0000005 (access violation) during the GPU test phase (soft shadows / vfx / volumetrics
  were running; no new code there and no `unsafe` in the new modules). Two direct reruns of the
  same binary passed 753/753; looks like a driver / device-creation flake worth watching.
