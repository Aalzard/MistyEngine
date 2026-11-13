# Autosave and crash recovery

Computers crash and power goes out. Misty Engine keeps a backup of your unsaved work every few
minutes, so a crash costs you minutes, not hours. Your own files are only changed when you
save: backups go to a separate folder.

## What is backed up

Every 5 minutes (you can change this), the editor backs up each open document that has
unsaved changes:

- the open scenes, including a scene you have never saved;
- the prefab open in Prefab Mode, and the scenes or prefabs you opened it from;
- the Script Graph, Shader Graph and Effect Graph open in their windows;
- the scripts open in the Code Editor;
- documents that haven't changed since their last backup are skipped.

Animator controllers, Timelines, animation clips and terrain edits don't need a backup: the
editor writes them to their files right after each change.

The status bar shows **Autosaved 14:32** after each backup. Hover it to see how many documents
were backed up.

## Recovering after a crash

When the editor closes normally, it deletes its backups, because you already chose to save or
not. If it crashes or the power goes out, the backups stay. The next time you open the project:

1. The **Recover Unsaved Work** window lists each document whose backup is newer than its
   file, with the time of the backup and the time of the file.
2. **Restore All** or **Restore Selected** opens the backups. Each one opens as a changed
   document, marked with `*`, exactly as if you had just made the changes.
3. Look at what came back, then save it with **File ▸ Save** (or **Ctrl+S**) to keep it.
4. **Discard** deletes the backups. **Decide Later** keeps them and asks again the next time
   you open the project.

> [!WARNING]
> Restoring never writes your files. If you close the editor without saving a restored
> document, the editor asks whether to save it, like any other change.

## Crash reports

If the editor itself fails, it writes an emergency backup of your unsaved work and a crash
report to the project's `Library/Logs` folder before it closes. The report holds the error,
where it happened, the editor version, your graphics card and the last Console lines. The
next time you open the project, **Open crash log folder** shows it. Nothing is ever sent
anywhere: if you want help, you choose whether to share the file.

## Settings

Open **Edit ▸ Preferences ▸ Autosave** to turn backups off or to choose how often they run,
from every minute to every hour. The setting is yours, for every project.

> [!PROGRAMMERS]
> Backups live in `Library/Autosave/<session>/<document>/`, three per document, each written
> to a temporary file, flushed to the disk and then renamed, on the background saver thread.
> The first line of a backup file describes the document; the rest is the document exactly as
> its own file would hold it. `Library/session.lock` records the editor process and whether it
> closed cleanly. It is separate from the project lock that stops two editors from opening the
> same project.
