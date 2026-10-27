# Launch in game (one click)

Tile > **Launch in game**: the open tile, exported, in a battle that the game starts straight into, with the packs the editor has
enabled. The command line it relies on, and the launches that settled it: [docs/re/game_command_line.md](../re/game_command_line.md).

## Status (2026-10-27)

- **Command line settled** (five launches): `game_startup_mode battle <xml>;` starts the battle with no menu; `mod "<pack>";` loads mod
  packs (by file name, from `data\` only: an absolute path stops the game); a mod pack's file wins over a working directory's
  (`add_working_directory` serves only paths no pack has); the first two crashes were a stray movie-type pack in `data\`.
- Not started: the code. Untested: a single tile as `<battle_map_definition><name>` (TDD has one commented out), which would launch a
  tile painted on no map.

## Design

1. **Export.** The existing "Save as exported tile" (`TileTool::save_exported`: the tile's packed files, and with procedural walls their
   buildings) into a staging folder, `<output>/launch/`, cleared each time.
2. **The launch pack.** The staging folder packed (rpfm_lib, PFH4, type Mod) as `wce_launch.pack` in the game's `data\`: a mod pack
   wins over loose files, and the game takes mod packs from `data\` only. Mod type, so a pack left behind never loads unless named.
   The battle XML goes in it too (its path, `script/wce/wce_launch.xml`, is the editor's own).
3. **The battle XML** (the benchmark's format):
   - the map: the placement picked in the Map context (`wce_tile::battle_map`): the map's folder as `<name>`, the placement's centre as
     `<tile_map_position>` (fractions of the tile map); the building level / escalation as `<tile_upgrade>`;
   - the type from the tile: `land_normal` for a field tile, `settlement_standard` / `settlement_unfortified` for a settlement tile,
     `<battlefield_owner_alliance_index>` the defender;
   - the weather chips: `<environment_key>` (the file lighting the tile), `<season>` / `<Season>`, `<time_of_day>`,
     `<precipitation_type>`, `<Climate>`;
   - two armies from a preset of **vanilla** factions and units (a mod's units may need packs that are not enabled), `<duration>` long,
     no battle script.
4. **Mods:** `mod "wce_launch.pack";` first (first listed wins), then `mod "<pack>";` for each pack enabled in the editor that is in
   `data\`, in the editor's order. `used_mods.txt` is never touched.
5. **Launch:** `Attila.exe <mods> show_frontend_movies false; game_startup_mode battle script/wce/wce_launch.xml;` from the game folder,
   after checks: the game is not already running (it refuses a second instance), Steam is running, the export succeeded, and no
   non-vanilla **movie-type** pack is in `data\` (read from each pack's header; a warning that lists them, which the user can override).
   The game takes focus: this is the user's click, not an unattended run.
6. **Cleanup:** the launch writes only the staging folder and `data\wce_launch.pack`; both are removed when the game exits (the editor
   waits on the process), or at the next launch and the editor's next start, should the editor be closed first. A `wce_launch.pack`
   the editor did not write (a marker file inside it) is never touched.
7. **UI:** Tile > Launch in game (enabled while a tile is open), a command `tile_launch_game [--dry-run]` (dry run: the staging folder
   and the pack, the command line logged, no game), status and log lines for each step. Settings > Game, *Launch in game*: the army preset.

## Decisions (user, 2026-10-27)

1. **Map:** the placement picked in the Tile panel's Map context, else the first found. A tile painted nowhere cannot launch yet (a
   generated one-tile map is the tilemap scope, `tile.md`).
2. **Delivery:** loose, unless loose loses to the mod packs. It does (test 4 of game_command_line.md), so a pack.
3. **Mods on the command line** (`mod "<pack>";`); `used_mods.txt` is never touched.
4. **Armies:** a minimal preset, one small unit per side far apart and a long duration; vanilla units by default, editable in Settings.
   Extra assets go into the launch, removed after every launch.
