# Timeline (sequencing cutscenes)

Progress: `.timeline` assets (JSON, GUID'd like every asset) with Activation, Animation, Audio
and Signal tracks, track groups and mute / lock; a Playable Director component that plays them
in Play mode (play on awake, wrap Hold / Loop / None, initial time, Game / Unscaled / Manual
time, one binding per track); a Timeline window (Window ▸ Sequencing ▸ Timeline) with track
headers, clip blocks you move and trim, blends drawn diagonally, a scrubbable ruler that
previews the scene in edit mode, transport and frame stepping, zoom / pan, snapping and a
markers row; Lua `PlayableDirector` (`Play`, `Pause`, `Resume`, `Stop`, `Evaluate`, `time`,
`duration`, `state`, `stopped:AddListener`); and the TimelineDemo sample.

Decisions:
- Engine code lives in `misty_engine::timeline` — `asset` (format), `evaluate` (weights and
  posing, shared by Play mode and the editor preview so they can't disagree), `tracks/` with one
  module per track kind, and `director` (Play mode). A new track kind is a `TrackKind` variant
  plus a module, so the virtual-camera work can add a Camera track without touching the rest.
- Tracks have stable ids ("t1", "t2", …, the next free number) and the director's bindings are
  `(track id, object)` pairs registered as object references in `scene/file.rs`, so they survive
  save / load, duplication and prefab remapping like joint and button targets. Ids are
  deterministic (not random GUIDs) so sample generators produce identical files.
- Track groups are flat: a track names its group's id. Grouped tracks are inserted right after
  their group; removing a group removes its tracks; mute and lock apply through the group.
  Simpler than nested track lists for the window, undo and the file.
- Blending: overlapping clips cross-fade over the overlap with a smoothstep curve; because
  `s(x) + s(1 - x) = 1` the two weights always sum to one, like the reference. Ease in / out on
  a lone clip fade from the values the object had before the timeline first touched it (the
  "baseline", captured lazily per property), and gaps return to that baseline. Discrete
  properties (Is Active) take the strongest clip's value. Clips are evaluated half-open
  `[start, end)`; at (or past) the timeline's end the evaluation time is nudged just inside the
  last frame so Hold shows clips that end with the timeline.
- Post-extrapolation: only None (default) and Hold. Loop / PingPong / Continue were left out to
  keep the clip inspector short; clips whose source `.anim` has Loop Time still loop inside the
  clip.
- "Recorded" keys: an Animation track may carry an embedded clip (`recorded`) that plays when
  the track has no clips. Evaluated and drawn, but the window has no record button yet.
- Activation Post-playback State defaults to Leave As Is; Revert uses the baseline.
- Audio tracks play through the bound Audio Source (its volume, 3D settings and mixer output):
  the source's clip is swapped for the timeline clip only for the `play()` call and put back
  at once, so the component is unchanged. Joining a clip late seeks to the right offset. There
  is no per-clip volume (the source's is used) — avoids a second volume control.
- Signals: a marker calls `method(parameter)` on the bound object's scripts with SendMessage
  semantics (warning if nothing defines it); markers on the timeline's markers row go to the
  director's own object. A marker fires once when playback passes it (`prev < t ≤ cur`; looping
  splits the frame in two spans), so a marker at 0 fires on the first frame. Retroactive markers
  also fire when playback starts after them. No Signal *asset* type: the method name lives on
  the marker, which is all the Lua side needs.
- `stopped` reuses the UI event object (`director.stopped:AddListener(fn)`) rather than a new
  callback mechanism; the listener gets the director. `Stop()` from a script fires it with the
  next update so listeners never run inside the call that stopped it.
- `Stop()` resets time to 0 and the next `Play()` starts at Initial Time; `time` set while
  stopped is kept. `state` is "Playing" / "Paused" / "Stopped" (strings, like other enums here).
- Timelines run after Animation and Animator in the frame, so they win for shared properties
  (the reference's timeline also overrides).
- Editor: timelines are edited in memory and saved after every edit (like Animator
  Controllers), with undo through the clip snapshot (`ClipSnapshot::Timeline`); one drag = one
  undo step. The preview backs up everything it touches and restores it on stop, play, save
  and scene changes, using the same hook list as the Animation window. Audio and signals don't
  run in the edit-mode preview.
- The window follows the selection but keeps its director when the selection moves to an
  object without one (selecting a clip clears the scene selection so the Inspector can show the
  clip, like Animator states). Double-clicking a `.timeline` opens it on a director that plays
  it. In Play mode the playhead follows the running director and editing is off.
- Look: no outlines — rounded header cards with a thin colour accent per kind, rounded clip
  blocks in the same colour, selection = a lighter fill; blends and eases are a shaded triangle
  with a light diagonal. Mute / lock are dim icons that brighten when on. The only toolbar
  extras are Preview, transport, the frame field and a Snap toggle.
- Snapping: to frames, then to clip edges of the same track, 0 and the playhead within 8 px.
  Mouse wheel zooms around the pointer and Shift+wheel pans (same as the Animation window);
  middle-drag pans; the wheel over the headers scrolls the track list.
- No Camera Cut / Control track: left to the virtual-camera work.

Screenshots: docs/screenshots/round11/timeline_*.png.
