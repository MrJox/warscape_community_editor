# One home for saves: the projects folder

The editor had two places where it wrote the user's work: the **projects folder** (`<data>/projects/<game>/`, from
`Settings::projects_dir`) and the **output folder** (`output_dir`, default `Documents\AttilaWeatherEditor\out` for Attila,
`Documents\WCE\<game>\out` for the others). The output folder came from the old weather editor, from before the projects folder existed.
This plan dropped it: **Save** always writes into the project, everything written for the game or for TEd goes under **File > Export**, and
throw-away build files get a folder of their own.

State (2026-11-22): planned 2026-11-08 with the user, steps 1 to 10 done the same day, in master (how it works now: [editor.md,
Saving and exporting](../editor.md#saving-and-exporting)). Nothing is left open. The Outliner (the other half of that discussion)
was out of scope here; it went later with [outliner_removal.md](outliner_removal.md).

## Why

Two homes for the user's work with different rules (`<projects>/loose` is mounted before every pack, the output folder was not, so a
saved weather file was invisible to every loader but the weather preview); the output folder mixed real work with build output; the save
actions wrote to five kinds of place without the menus saying which; the setting's default named an app that no longer exists. Before
this plan, weather files, the weather database, UI XML exports, exported tiles, the terrain export, launch staging and dry runs went to
the output folder, and the file dialogs started there.

## Where they are written now

| Action | Writes to |
|---|---|
| Tile, Map, Prefab > Save | `<projects>/assembly_kit/raw_data/...` (unchanged) |
| Weather file, weather database, UI layout > Save (opened from the packs) | `<projects>/loose/<path in the packs>` |
| UI skins, saved layouts, pictures | `<projects>/loose/...` (unchanged) |
| **File > Revert > To the game's version...** (new) | deletes the loose copy, the packs' file shows again |
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

## As built (2026-11-08)

1. The VFS sees a save into `loose/` (and a revert) at once, without a remount.
2. to 4. Weather files, the weather database (`loose/warscape_asset_variation_db/`) and UI layouts opened from the packs save into
   `loose/` (`XmlFileDocument::default_save_path`, `UiDocument::default_save_path`); the Weather tool and the weather preview read through
   the VFS alone. A document's source label says "project" when the file came from `loose/`.
5. Revert to the game's version (`project_prompts.rs`): asks first, deletes the copy, refreshes the VFS, reopens from the packs.
6. Launch staging and dry runs go into `<projects>/build/`: throw-away, nothing reads it back between runs.
7. File > Export: For Assembly Kit, As exported tile..., To mod pack..., XML... (UI), the terrain export; each asks for its destination
   and remembers the last one per game. The script commands kept their names and take the destination as their argument.
8. The `output_dir` setting, `WCE_OUTPUT_DIR` and the `output_dir` command are gone. Open file and Save as start in the projects folder.
9. The one-time migration of the old output folder into `loose/` was removed on 2026-11-21 for the release, with the other migrations.
10. Scripts (`cross_tool.txt`, `weather_smoke.txt`) and docs/editor.md updated.

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

- None. **Several tiles drawn at once** (from the Outliner discussion): not wanted; one document per tool, and the Outliner went
  ([outliner_removal.md](outliner_removal.md)).
