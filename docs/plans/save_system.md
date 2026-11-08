# One home for saves: the projects folder

The editor has two places where it writes the user's work: the **projects folder** (`<data>/projects/<game>/`, from
`Settings::projects_dir`) and the **output folder** (`output_dir`, default `Documents\AttilaWeatherEditor\out` for Attila,
`Documents\WCE\<game>\out` for the others). The output folder comes from the old weather editor, from before the projects folder existed.
This plan drops it: **Save** always writes into the project, everything written for the game or for TEd goes under **File > Export**, and
throw-away build files get a folder of their own.

State: planned 2026-11-08 with the user, steps 1 to 10 done the same day (how it works now: [editor.md, Saving and
exporting](../editor.md#saving-and-exporting)). The Outliner (the other half of that discussion) is out of scope: it stays as it is until it
has been discussed further.

## Why

- Two homes for the user's work, with different rules: `<projects>/loose` is mounted before every pack (`Editor::overlay_folders`), the
  output folder is not. Only `Editor::output_copy` reads the output folder back (the weather preview and the Weather tool's open), so a
  saved weather file is invisible to every other loader.
- The output folder mixes real work (edited weather files, the weather database, UI layouts) with build output (launch staging, dry runs,
  exports).
- The save actions write to five kinds of place (the project, the output folder, a kit, a pack, the game's data folder), and the menus
  don't say which.
- Its default name refers to an app that no longer exists, and the setting is one more thing a new user has to understand.

## Where things were written (before this plan)

| Action | Writes to |
|---|---|
| Tile > Save, Map (terrain) > Save, Prefab > Save | `<projects>/assembly_kit/raw_data/...` |
| UI skins, saved layouts, pictures | `<projects>/loose/...` (mounted) |
| Weather file > Save (opened from the packs) | `<output>/<path in the packs>` (`XmlFileDocument::default_save_path`) |
| Weather database > Save | `<output>/warscape_asset_variation_db/warscape_asset_variation_db.{xml,bin}` (`picker.rs`) |
| UI layout > Save (opened from the packs) | `<projects>/loose/<path in the packs>` already (`UiDocument::default_save_path`), but not indexed again after a save |
| UI > Export XML | `<output>/ui_xml/<name>.uixml` |
| Tile > Save as exported tile, `save_exported` | `<output>/...` (exported tile folders and the tile database) |
| Terrain export | `<output>/...` (the map's folder and pack, `wce_terrain_tool::export`) |
| Launch staging, dry runs | `<output>/launch/<id>/game`, `<output>/launch/terrain`, `<output>/dry_*` |
| File > Open file / Save as | dialogs start in the output folder |
| Weather / UI Save of a file opened from disk | back in place |
| Save for Assembly Kit (tile, prefab) | the chosen kit's `raw_data` |
| Tile > Save to mod pack, UI > Export mod pack | the chosen `.pack` |
| Launch in game (launch pack) | `<game>/data/wce_launch.pack`, removed afterwards |

## Where they are written now

| Action | Writes to |
|---|---|
| Tile, Map, Prefab > Save | `<projects>/assembly_kit/raw_data/...` (unchanged) |
| Weather file, weather database, UI layout > Save (opened from the packs) | `<projects>/loose/<path in the packs>` |
| UI skins, saved layouts, pictures | `<projects>/loose/...` (unchanged) |
| **Revert to game's version** (new) | deletes the loose copy, the packs' file shows again |
| Launch staging, dry runs | `<projects>/build/...` (throw-away, no setting) |
| File > Export > For Assembly Kit | the chosen kit's `raw_data` |
| File > Export > As exported tile | a chosen folder |
| File > Export > To mod pack (tile, UI, terrain) | a chosen `.pack` |
| File > Export > XML (UI) | a chosen folder |
| Save / Save as of a file opened from disk | back in place, or the chosen path |
| Launch in game (launch pack) | `<game>/data/wce_launch.pack` (unchanged) |

Each Export entry asks for its destination and remembers the last one per game (as `default_mod_pack` does today).

## Decisions

- Projects stay **one folder per game** for now. Named projects per mod may come later; this plan must not get in their way (everything
  goes through `Settings::projects_dir`).
- **A saved weather file in `loose/` lights every tile**, as a loose file does in the game. The same goes for the weather database: once
  saved, it is the one the editor reads.
- Saves stay minimal edits: only where the bytes go changes. The weather gates (`cross_tool_check`, `span_diff.py`) keep comparing the
  saved file with the packs' file.

## Steps

1. **The VFS sees a save at once.** `Vfs::mount` indexes the files when it mounts (`Vfs::files`), so a new file in `loose/` is not seen
   until a remount. Add a way to add or drop one loose path in the index (or remount only the loose folders) and call it after every save
   into `loose/` and every revert. Whatever reads through the VFS (the weather preview, the tile tool's weather list, the launch's
   `needs`) then sees the edit.
2. **Weather files save into `loose/`.** `XmlFileDocument` gets the loose folder instead of the output folder. The Weather tool's `open`
   and `Editor::set_weather_preview` read through the VFS alone (`loose/` wins over the packs), so `output_copy` and `copy_to_output` go.
   The document's source label says "project" when the file came from `loose/`.
3. **The weather database saves into `loose/warscape_asset_variation_db/`.** The picker's Open / Save / Save As / Convert dialogs start
   there.
4. **UI layouts from the packs save into `loose/`** (`UiDocument::default_save_path`).
5. **Revert to game's version**: a File menu entry and an Outliner context entry for a document whose file is a loose copy of a packs'
   file. It asks first, deletes the copy, refreshes the VFS (step 1) and reopens the document from the packs.
6. **Build output into `<projects>/build/`**: the tile and terrain launch staging and the dry runs. It is throw-away: nothing reads it back
   between runs.
7. **File > Export submenu.** Move Save for Assembly Kit, Save as exported tile, Save to mod pack, Export mod pack, Export XML and the
   terrain export under it. Each asks for a destination (the kit prompt stays for kits). Their script commands keep their names and take
   the destination as their argument, as `save_exported [folder]` already does.
8. **Remove the setting.** `GameSettings::output_dir`, the top-level legacy field, `WCE_OUTPUT_DIR`, the Weather tool's Settings menu
   field, the `output_dir` command and the Folders page entry go. Open file and Save as start in the projects folder.
9. **Migration, once per game.** If the old output folder (the setting's, or the default) holds files under a packs' path, offer to move
   them into `loose/`. A file that `loose/` already has stays where it is and is listed in the log. Build leftovers (`launch/`, `dry_*`)
   are not moved. Until the release, like the other migrations in `settings/mod.rs`.
10. **Scripts and docs.** `scripts/cross_tool.txt` and `scripts/weather_smoke.txt` use the output folder; they move to the projects
    folder (`$WCE_SETTINGS_DIR` already gives scripts a throw-away one). Update `docs/editor.md`: the Weather tool section, Save All, the
    settings table, and the Save All row that still says tiles save as `tile.ted` with a `.bak` (tiles save as `tile.wce` in the project).

## Launch in game

Checked 2026-11-08: the battle XML names the weather file itself (`<environment_key>`, `launch::battle_xml`), and the launch carries that
file whenever the mounted files read it from a loose folder (`launch_prompt.rs`: in both deliveries; the loose delivery also carries every
other dependency read from a loose folder, `launch::Needs::overlay`). Before this plan a saved weather file never reached the game: the
output folder was not mounted, so the launch shipped nothing and the game used the packs' file. Now a weather file saved into `loose/`
is carried. The weather database is not carried: the battle names its environment file, so the game's own pick (the database) only
matters for a tile without a weather file.

## Found on the way (not changed)

- `scripts/weather_smoke.txt` failed at its first `assert_attr` on master too: the Tile tool's `assert_attr` (`a5e89232`) was
  registered after the Weather tool's and replaced it (the registry is a map by name). The Weather tool's is `assert_xml_attr` now, and
  `CommandRegistry::add` keeps the first command of a name and logs an error for a second one (fixed 2026-11-08).
- `scripts/cross_tool_check.sh` used a tile not every kit has (`khazad_wall_test`) and expected Save to write `tile.ted` in place: it
  uses the vanilla `dense_forest_rugged/1x1/aa` now (five of the game's rock buildings, no rivers; the kit found under `WCE_GAME`, or
  `WCE_KIT_TILES`), checks that Save All writes the project (`tile.wce`) and not the kit, and makes the `tile.ted` it compares with
  File > Export > For Assembly Kit into its scratch kit (`save_for_kit`). Passes, both runs identical (2026-11-08).

## Open points

- **Several tiles drawn at once** (from the Outliner discussion): the user is checking whether that is ever wanted. It doesn't change this
  plan.
