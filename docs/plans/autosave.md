# Autosave

Copies of the modified documents written every few minutes into an autosave folder, never over the project's files, and offered back on
the next launch after a crash.

State: planned 2026-11-14 with the user, not started. Replaces the Autosave item of [editor_todo.md](editor_todo.md).

## Decisions

- **Where:** `<projects>/autosave/` (from `Settings::projects_dir`, so per game, like everything else the user's work writes). Not mounted
  by the VFS and not read by any loader: only the recovery reads it.
- **Never over the project:** an autosave writes a copy; it never marks the document saved, never touches its history's saved point and
  never changes where Save writes.
- **Which documents:** every document that can be saved: tiles (and prefab stages), terrain maps, UI layouts, weather files, the weather
  database.
- **Retention:** the last N copies per document, rotated (N a setting, default 3). Recovery lets the user pick which one.
- **Recovery:** offered at the next launch. A clean exit (or closing a document, saved or discarded) deletes that document's autosaves;
  whatever is left at startup belongs to a session that did not end cleanly.
- **Setting:** app-wide (not per game): Settings > General, "Autosave" on by default, every 5 minutes (1 to 60), and "Keep" 3 copies (1 to
  10).
- **When:** only documents modified since their last autosave (or save), never in the middle of a gesture (a drag, a paint stroke, a gizmo
  move: whatever has an open command), never during a scripted run (`--script`, `--shot`, `--capture`), and never while a modal (a save,
  an export, a launch) is running.

## Folder layout

```
<projects>/autosave/
  <session>/                  one per editor process: <start time>_<pid>
    session.toml              the editor's version, the game, the pid, the start time
    <document key>/           a document's slot, its key made file-safe (see below)
      meta.toml               kind, title, where its Save writes, the source it was opened from, one entry per copy (time, folder)
      1/ 2/ 3/                the copies, oldest removed first
```

- **One folder per process:** two editors may run at once (the settings already merge each other's writes); they never share a slot.
  A session whose pid is still running is never offered or cleaned.
- **The document key:** what identifies it across sessions: `source_key` where it has one (a game file's path), else where its Save
  writes (a tile's project folder, a map's `save_dir`), else the title. Recovery uses it to match an autosave with the document it came
  from and to tell whether that file was saved after the autosave (then the autosave is older and is offered as such, not hidden).
- **Each copy is written into a temporary folder and renamed** into place, so a crash mid-write leaves the previous copies whole.

## What a copy holds

A new `Document` method, `autosave(&self, dir: &Path) -> Result<(), String>` (default: not supported), writes what reopening needs:

| Document | Copy |
|---|---|
| `XmlFileDocument` (weather file), weather database | the file as Save would write it (minimal edit of its source bytes) |
| `UiDocument` | the layout's bytes as Save writes them |
| `MapDocument` | the files `wce_terrain_io` writes for Save (`attila::write` / `built::write`), into the copy's folder |
| `TileDocument` | `tile.wce` plus the tile's database entry (height range, size, colour, the rest of the entry) in a sidecar file, since a tile folder alone reopens without them (why Save as is blocked for tiles); a prefab stage: the prefab as its Save writes it |

The copy goes through the same writers as Save, so the bytes are what Save would have written. `&self`, not `&mut self`: an autosave must
not change the document (anything a save prepares in the model, e.g. a tile's polyfill for kit saves, is done on a copy or not at all).

## Recovery

At startup, after the settings and the game are loaded and before the last tool's documents reopen:

1. List the sessions in `<projects>/autosave/` whose pid is not running. None: nothing to show.
2. A dialog lists their documents: title, kind, the newest copy's time, and whether the document's own file was saved after it. Per
   document: **Restore** (newest, or an older copy from a drop-down), **Discard**, or leave it (**Later**: the dialog shows again next
   launch).
3. **Restore** opens the document from its source as usual, then applies the copy over it: the document opens **modified**, its Save
   still writes to the project, and the user decides whether to save. A document that cannot be restored (its source gone, the copy does
   not read) says why and keeps its autosave.
4. A session left with no documents is deleted.

Also reachable later as File > Recover autosaves... (the same dialog), for a user who chose Later.

## Steps

1. **Settings.** `AutosaveSettings { enabled, interval_minutes, keep }` in `Settings` (app-wide, with defaults; serde default so old
   files read). Settings > General gets the three fields.
2. **The autosave folder.** A small module in `wce_editor_core` (next to `document_manager.rs`): the session folder, the slots, the
   rotation, the temporary-then-rename write, `meta.toml`, deleting a slot, finding stale sessions (pid not running).
3. **`Document::autosave` and `Document::restore_autosave(dir)`**, implemented for the five documents (the table above). The tile's
   entry sidecar is the one new format; it is ours, a TOML of the entry's fields.
4. **The timer.** `Editor::frame` checks the interval (wall clock, not frames); when due, autosaves each modified document whose history
   moved since its last autosave, skipping the gesture/modal/script cases. A failure goes to the log and the status line once, not every
   interval.
5. **Clean-up.** Closing a document (saved or discarded) deletes its slot; a clean exit deletes the session folder. A save keeps the slot
   (its copies are older than the file then) until the next autosave or the close.
6. **Recovery dialog** at startup and File > Recover autosaves...
7. **Script commands** for tests: `autosave_now`, `assert_autosave <document> <copies>`; and a scripted run never autosaves on its own
   (only `autosave_now`).
8. **Tests and docs.** A core test (`editor_check.rs`, `NumberDoc`): rotation, the skip rules, a clean exit deleting the session. A round
   trip per document kind: autosave, restore, Save, and the saved bytes equal those of a direct Save. `docs/editor.md`: an Autosave
   section and the settings table.

## Open points

- Large tiles and maps: if a copy takes long enough to stall a frame, write it on a worker thread from a snapshot of the bytes (the
  serialisation on the main thread, the disk write off it). Measure first.
- Disk use: N copies of a large map. The dialog could show the folder's size; no cap planned.
