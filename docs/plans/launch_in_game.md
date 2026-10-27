# Launch in game (one click)

Tile > **Launch in game**: the open tile, exported, in a battle that the game starts straight into, with the packs the editor has
enabled. The command line it relies on: [docs/re/game_command_line.md](../re/game_command_line.md).

## Status (2026-10-27)

- **Gate passed:** `Attila.exe game_startup_mode battle <battle xml>;` skips the menu and loads the battle (vanilla, the Londinium
  benchmark's XML from data.pack).
- Not checked yet (test 2, one launch): a battle XML on disk or loose rather than in a pack; `add_working_directory` for the exported
  tile and whether it wins over the mod packs; `mod "<pack>";` on the command line; `gfx_fullscreen 0;` and `show_frontend_movies 0;`.
- Design below; the open questions at the end are the user's.

## Design (proposed)

1. **Export.** The existing "Save as exported tile" (`TileTool::save_exported`: the tile's packed files, and with procedural walls their
   buildings in a pack) into a staging folder of its own, `<output>/launch/`, cleared each time.
2. **Deliver**, one of (question 2):
   - *loose:* the staging folder passed as `add_working_directory "<staging>";` (what the launcher does for `assembly_kit/working_data/`):
     nothing is written into the game folder, no packing, nothing to clean up;
   - *pack:* the staging folder packed (rpfm_lib) as `wce_launch.pack` into the chosen data folder and listed first among the mods.
3. **The battle XML**, written as `<staging>/wce_launch.xml` (the benchmark's format):
   - the map: a placement of the tile from the Map context (`wce_tile::battle_map`): the map's folder as `<name>`, the placement's centre
     as `<tile_map_position>` (fractions of the tile map); the building level / escalation as `<tile_upgrade>`;
   - the type from the tile: `land_normal` for a field tile, `settlement_standard` / `settlement_unfortified` for a settlement tile,
     `<battlefield_owner_alliance_index>` the defender;
   - the weather chips: `<environment_key>` (the file lighting the tile), `<season>` / `<Season>`, `<time_of_day>`,
     `<precipitation_type>`, `<Climate>`;
   - two armies from a preset (question 4), `<duration>` long, no battle script.
4. **Mods**, one of (question 3): `mod "<pack>";` for each pack enabled in the editor that exists in the game's `data/` (the editor's
   order), plus the launch pack; or `used_mods.txt` rewritten with a backup restored when the game exits (and on the editor's next start,
   should the editor die first).
5. **Launch:** `Attila.exe <mods> [add_working_directory] game_startup_mode battle "<xml>";` from the game folder, after checks: the game
   is not already running (it refuses a second instance), Steam is running, the export succeeded. The game takes focus: this is the
   user's click, not an unattended run.
6. **UI:** Tile > Launch in game (enabled while a tile is open), a command `tile_launch_game` for scripts (`--dry-run` writes the
   staging folder and logs the command line without starting the game), status and log lines for each step. Options in Settings > Game
   (*Launch in game*): the delivery, windowed, skip intro movies, the army preset.

## Open questions

1. **Which map** for a tile painted in several places (the Map context's choice, or the first), and for a new tile painted nowhere (a
   generated one-tile map is the tilemap scope, `tile.md`).
2. **Loose or pack.**
3. **Mods on the command line or `used_mods.txt`.**
4. **The armies:** factions and units (fixed preset, from the editor's packs, an empty battle with one unit to walk around).
