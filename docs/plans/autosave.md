# Autosave

Copies of the modified documents written every few minutes into an autosave folder, never over the project's files, and offered back on
the next launch after a crash.

State: planned 2026-11-14 with the user and built the same day (branch `autosave-plan`); how it works now:
[editor.md, Autosave](../editor.md#autosave). Replaces the Autosave item of [editor_todo.md](editor_todo.md). Left: prefab stages (below),
and a try of the timer and the dialog in an interactive session (the scripted checks cover the copies and the restores, not those two).

## Decisions

- **Where:** `<projects>/autosave/` (from `Settings::projects_dir`, so per game, like everything else the user's work writes; confirmed by
  the user). Not mounted by the VFS and not read by any loader: only the recovery reads it.
- **Never over the project:** an autosave writes a copy; it never marks the document saved, never touches its history's saved point and
  never changes where Save writes.
- **Which documents:** tiles, battle maps, UI layouts, weather files and the weather database. Not prefab stages (open point below).
- **Retention:** the last N copies per document, rotated (N a setting, default 3). Recovery lets the user pick which one.
- **Recovery:** offered at the next interactive launch (and after a game switch, for that game's folder). A clean quit deletes the
  session's copies; a document saved, or closed with Discard, loses its copies at once (its file has everything then, or the user threw
  the changes away). Whatever is left belongs to a session that did not end cleanly.
- **Setting:** app-wide (not per game): Settings > General, "Autosave" on by default, every 5 minutes (1 to 60), 3 copies kept (1 to 10).
- **When:** only documents modified since their last copy, never while a gesture, a mouse button, an unsaved-changes prompt or a write
  (an export, an update) is under way, and never in a scripted run (`--script`, `--shot`, `--capture`): scripts use `autosave_now`.

## Folder layout

```
<projects>/autosave/
  <session>/                  one per editor process: <start time>_<pid>
    session.lock              locked by the running editor (std's File::try_lock): a session whose lock can be taken ended without cleaning up
    session.toml              the editor's version, the pid, the start time
    <slot>/                   a document's: its key made file-safe, and a hash of the key
      meta.toml               tool, kind, title, where its Save writes, what reopens it, its file in a copy, the copies (time, folder)
      1/ 2/ 3/                the copies, oldest removed first
    restored/<n>/             copies taken back into this session's documents (they read their files there until the session ends)
```

- **One folder per process:** two editors may run at once (the settings already merge each other's writes); they never share a slot.
  A running editor's session is never offered or cleaned: its lock is held. A lock instead of a pid: a crash lets it go by itself, and a
  pid can be reused.
- **The document key:** where the document's Save writes (a game file's place in `loose/`, a tile's folder or the project a kit tile's
  first save makes, a map's projects folder). Recovery tells from it whether that file was written after the copy (the copy is then
  offered with a warning, not hidden).
- **Each copy is written into a temporary folder and renamed** into place, so a crash mid-write leaves the previous copies whole.

## What a copy holds

`Document::autosave_meta`, `Document::autosave_stamp` and `Document::autosave(&mut self, dir)` (by default not autosaved). `&mut self`
because the tile's copy goes through its save code, which notes things on the way; what it notes (the maps' unsaved marks, the entry,
the heights' stored range) is put back after the copy, so the next real Save is what it would have been.

| Document | Copy |
|---|---|
| `XmlFileDocument` (weather file) | the file as Save writes it |
| `UiDocument` | the layout as Save writes it (binary, or XML for a `.xml` file) |
| `MapDocument` | the files the edits made again (`attila::write` / `built::write`'s bytes), and `refs.txt` naming the files that were not edited (an LF is large; copying it every few minutes would stall the editor) |
| `TileDocument` | the tile in the kit's layout: `raw_data/terrain/tiles/.../<variation>/` written as a Save into another folder writes it, its entry (`_tile_database/TILES/<name>.xml`, as it is in memory) and `_settings.xml`. No sidecar format: the copy opens as a kit tile with its entry |
| The weather database (the picker's working copy, not a document: `Tool::autosave_work`) | `warscape_asset_variation_db.xml` |

## Recovery

1. `wce_editor_core::autosave::stale` lists the slots of sessions whose lock can be taken; empty sessions are removed on the way.
2. The dialog lists their documents: title, kind, its target, a warning when the target was written after the copy, the copy (the newest
   by default, the others in a drop-down), **Restore**, **Discard**; **Discard all** and **Later** below.
3. **Restore** moves the copy into this session (`restored/<n>`) and the tool takes it back (`Tool::restore`):
   - weather file, UI layout: the file opened again from where it is, the copy laid over it as one undoable edit (Undo goes back to the
     file);
   - tile: opened from the copy, its Save redirected to the tile's target (`TileDocument::set_restored`): the first Save writes every
     file there, as a kit tile's first save goes into its project, then the document is that folder's;
   - map: the copy made whole (`MapDocument::complete_autosave`: the files it names copied in, every file dated now so that Save writes
     them all), read on a worker as the map, its Save into the map's projects folder as always;
   - weather database: the picker's working copy, edited.
   The document opens modified. A restore that fails puts the copy back (`autosave::give_back`) and says why; it is offered again.
4. A session left with no documents is deleted. Also reachable as File > Recover autosaves....

## Checked (2026-11-14)

Scripted runs on the hidden desktop with a scratch projects folder: edit, `autosave_now`, `autosave_abandon` (a crash's leftovers), Save
(the direct save); the saved files moved aside; a second run `autosave_restore 0`, Save. The restored save is byte-identical to the
direct save for a weather file of the packs (`med_sp_land_day_dry.environment`, an attribute and a structural edit), a UI layout
(`settlement_captured`), a kit tile's first save into a new project (`dense_forest_rugged/1x1/aa`: a moved building and a raised
height, its entry identical too), a project tile restored over its folder (the same files; the only difference: a `blend_map.tif.bak`, as
the restored Save writes every file), a kit battle map (`settlement_eastern_city_a_minor`, an LF stroke: its copy held the LF alone and
`refs.txt`) and the weather database (an added climate value, `.xml` and `.bin`). The autosave folder was empty after each second run.
Unit tests: `wce_editor_core::autosave` (rotation, a failed copy, a clean end, a gone session offered, taken, given back, discarded) and
`XmlDoc::parse_over`.

## Open points

- **Prefab stages** are not autosaved: a stage is a tile the Prefab tool makes for a prefab, and what its Save writes is the prefab
  (`StageSave`). A copy would be the stage's tile, and taking it back needs the Prefab tool to make the stage around it (`open_prefab`
  with the copy's project instead of `prefab_stage::make`, without `begin_stage` laying the prefab's ground again).
- Large tiles: a tile's copy is all its files (35 ms for a 1x1 tile). If a large one stalls a frame, write it on a worker from a
  snapshot of the bytes.
- A map's copy refers to the files it did not edit where they are: when one of those is changed or deleted before the copy is restored
  (a kit map edited in TEd), the restore takes the file as it is then, or fails and says which.
- Disk use: N copies of a large map's edited LF. No cap planned.
