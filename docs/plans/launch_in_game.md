# Launch in game (one click)

Tile > **Launch in game**: the open tile, exported, in a battle that the game starts straight into, with the packs the editor has
enabled. The command line it relies on: [docs/re/game_command_line.md](../re/game_command_line.md).

## Status (2026-10-27)

- **Gate passed:** `Attila.exe game_startup_mode battle <battle xml>;` skips the menu and loads the battle; with
  `mod "<pack>";` arguments (TDD loaded, `used_mods.txt` untouched) and the XML loose under `add_working_directory`, it loaded and played
  (results: [game_command_line.md](../re/game_command_line.md), "Launch results").
- The first two launches crashed in the GPU driver: a stray **movie-type** pack in `data\` (it loads whatever the mod list says) shadowed
  the mods' buildings. Hence the stray-pack check in step 5.
- Open (test 3): does a loose file beat a mod pack's file of the same path? The loose `.environment` test showed normal colours, which
  is also explained by the battle not using `<environment_key>`. Test with a file the battle surely reads (a tile file of the map).
- Design below; the decisions at the end are the user's.

## Design

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
   - two armies from a preset (question 4) of **vanilla** factions and units (a mod's units may need packs that are not enabled),
     `<duration>` long, no battle script.
4. **Mods**, one of (question 3): `mod "<pack>";` for each pack enabled in the editor that exists in the game's `data/` (the editor's
   order), plus the launch pack; or `used_mods.txt` rewritten with a backup restored when the game exits (and on the editor's next start,
   should the editor die first).
5. **Launch:** `Attila.exe <mods> [add_working_directory] game_startup_mode battle "<xml>";` from the game folder, after checks: the game
   is not already running (it refuses a second instance), Steam is running, the export succeeded, and no **non-vanilla movie-type pack**
   is in `data\` (read from each pack's header; listed in a warning the user can override). The game takes focus: this is the user's
   click, not an unattended run. The staging folder is the only thing a launch writes; it is removed when the game exits (or at the next
   launch, should the editor be closed first). Any asset the test needs beyond the tile goes there too.
6. **UI:** Tile > Launch in game (enabled while a tile is open), a command `tile_launch_game` for scripts (`--dry-run` writes the
   staging folder and logs the command line without starting the game), status and log lines for each step. Options in Settings > Game
   (*Launch in game*): the delivery, windowed, skip intro movies, the army preset.

## Decisions (user, 2026-10-27)

1. **Map:** the placement picked in the Tile panel's Map context, else the first found. A tile painted nowhere cannot launch yet (a
   generated one-tile map is the tilemap scope, `tile.md`).
2. **Delivery:** loose (`add_working_directory`); a pack only if test 3 shows loose files lose to the mod packs.
3. **Mods on the command line** (`mod "<pack>";`); `used_mods.txt` is never touched.
4. **Armies:** a minimal preset, one small unit per side far apart and a long duration; factions and units editable in Settings; vanilla units by default; extra assets go loose in staging, removed after every launch.
