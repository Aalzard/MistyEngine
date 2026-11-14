# Round 62: autosave and crash recovery

Branch `r62-autosave` (worktree `MistyWork\r62-autosave`, from main b88e54c9). Cargo target
`E:\MistyTargets\r62-autosave` (`CARGO_INCREMENTAL=0`), TMP/TEMP `C:\MistyTry\r62tmp`, editor runs
on copies under `C:\MistyTry\r62\`.

Why: the user's PC crashed several times (power loss; a memory bugcheck from the failing D:
drive). Unsaved scene work must never be lost.

## What it does

Code: `crates/misty_editor/src/autosave/` (`mod.rs` timing + writing, `docs.rs` documents and the
backup format, `session.rs` marker and local time, `recovery.rs` scan + restore + dialog,
`crash.rs` panic path, `tests.rs`).

- **Autosave.** Edit ▸ Preferences ▸ Autosave: on/off, every 1/2/5/10/15/30/60 min (default on,
  5 min), saved per user in `autosave.json` next to `hub.json`. Each run backs up every open
  document with unsaved changes:
  - open scenes (several scenes: each changed one; a never-saved scene too);
  - Prefab Mode: the open prefab, the outer prefabs' stages and the scenes underneath;
  - Play mode: the scenes as they were before Play (Play changes are thrown away on Stop);
  - the Script / Shader / Effect Graph windows' graphs, scripts open in the Code Editor.
  - Not needed: Animator controllers, Timelines, Animation clips, terrain (written to their
    files right after each edit / stroke). Input Actions (has its own Auto Save) not covered.
- **Where.** `Library/Autosave/<session>/<kind-name-hash>/<slot>.autosave`, slots 0..3 rotating
  (last 3 per document). A backup = one JSON header line (kind, path, name, time, emergency) +
  the document exactly as its file would hold it (scene JSON with the GUID table, prefab JSON,
  graph JSON, script text). Self-describing, so a write cut short can't be paired with the wrong
  document. An unchanged document (same content hash as its last backup) isn't written again.
- **Threads.** The main thread only takes the snapshot: for a scene, `record_overrides` (like
  File ▸ Save) and a `Scene::clone`; the scene JSON + GUID table is written on a short-lived
  worker thread (`docs::start_snapshot`), the result picked up on a later frame. Small documents
  (graphs, scripts, prefabs) are serialised at once. Bytes go to `asset_db::saver::save_later`
  (round 40's writer thread; `write_atomic` = temp file, `sync_all`, rename). Spread: a run
  starts snapshots until 6 ms of the frame are used, the rest on the next frames. Defer: a due
  autosave waits while a mouse button is down or a text field has focus (max 60 s); when the
  last run's slowest main-thread step took > 20 ms (`big`), the next one waits until the user
  has been idle for 2 s (max one more interval). The idle editor wakes when the next autosave
  is due (not on a fixed tick).
- **Status bar.** "Autosaved 14:32" (local time, weak, rightmost); hover: document count, cost.
- **Session marker.** `Library/session.lock` JSON: pid, started_ms, session id, clean_exit,
  crashed, version, crash_log. Written on project open; `clean_exit: true` on a normal exit
  (`EditorApp::on_exit`, also when going back to the Hub). A live marker (pid running and started
  before the marker's time, so a reused pid after a reboot doesn't count) means another editor
  has the project: this one turns autosave off and leaves the marker alone.
- **Clean exit deletes the session's backups** (the save prompts were answered) and the backup
  folders of earlier sessions it restored. So anything left in `Library/Autosave` is from a
  crash / power loss / "Decide Later".
- **Recovery.** On open, `recovery::scan` reads the session folders that aren't this one's or a
  running editor's; per document the newest backup is offered when newer than its file (or the
  file is missing / never saved). Older ones are stale and deleted, as are folders with no
  readable backup. The **Recover Unsaved Work** modal lists document, backup time, file time
  (local), ⚠ on emergency backups; Restore All / Restore Selected (the rest stay listed) /
  Discard (asks "can't be undone") / Decide Later (kept, offered at the next start).
- **Restore = a modified document, never a file write.** Scene: `reload_scene` in place when that
  scene is open, else `load_additive`; untitled: replaces the open scenes (only when none has
  changes). `files.saved[id] = 0` + `scene_dirty` → title shows `*`, Save writes it. Prefab:
  `prefab_mode::open_restored` (Prefab Mode with the backup's contents, stage dirty). Graphs:
  window `open` + `edit("Restore Autosave")` (undoable, dirty). Scripts: Code Editor `open` +
  `editor.reload(backup)` (dirty). Order: scenes, graphs, scripts, prefabs. After a restore an
  autosave runs at once; the old folders go on this session's clean exit.
- **Panics.** `crash::install_hook` (replaces main.rs's hook; still flushes the saver ≤ 5 s): on
  the UI thread it writes `Library/Logs/crash-<local time>.txt` (message + location, backtrace,
  editor version, OS, GPU adapter, project) and marks the marker `crashed`. `launcher` runs the
  editor frame in `catch_unwind`; on a panic `EditorApp::crashed` → `crash::after_panic`: an
  emergency backup of every dirty document (each in its own `catch_unwind`, written directly
  with `write_atomic`, header `emergency: true`), the last 60 Console lines appended to the log,
  then `process::exit(101)`. Next start: the dialog (or a "closed unexpectedly" notice when
  nothing needs recovering) offers **Open crash log folder**. Nothing is sent anywhere.

- **`MISTY_CRASH_TEST=<seconds>`**: that long after the project opens, adds "Crash Test Object"
  to the scene and panics (to try the crash path on a test project). Off unless set.

## Project lock (round 60, merged from main e21f65b8)

- Separate files: `Library/session.lock` (this) vs `Library/ProjectLock*` (r60). No shared code.
- A read-only editor (another editor holds the lock: `project_lock.read_only()` or
  `asset_db::is_read_only(root)`) starts no autosave, writes no marker and shows no recovery
  dialog (`autosave::start`). The live-pid check in `session::start_as` stays as a fallback
  for when the lock couldn't be taken (`LockState::None`).
- The lock holder's writes (`Library/Autosave` through the saver, `Library/Logs`, the marker)
  go through `write_atomic`, which only refuses in a read-only project: they work. `on_exit`
  writes the clean marker before releasing the lock.
- Test: `a_read_only_editor_neither_backs_up_nor_recovers`.
- r60 open item: a validation error on a rayon worker aborts the process; the panic hook doesn't
  run then, so no crash log / emergency backup for that case (the 5-minute backups remain).

## Cost (release, this machine, other builds running)

| Scene | JSON (worker) | Main thread per autosave |
|---|---|---|
| NoonValley (235 KB) | 9.7 ms | 4.3 ms (clone 1.2 ms + prefab override records) |
| StressDemo (5.3 MB) | 95–118 ms | 20 ms (clone): counts as big, waits for an idle moment |

Before moving the JSON to a worker, NoonValley cost 9.5 ms of main-thread time per autosave.
Test: `autosave_cost_on_a_big_scene_stays_within_budget` (NoonValley < 12 ms main thread).
