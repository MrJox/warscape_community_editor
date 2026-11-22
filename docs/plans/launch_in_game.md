# Launch in game (one click)

Tile > **Launch in game**: the open tile, exported, in a battle that the game starts straight into, with the mod packs it needs. The
command line it relies on, and the launches that settled it: [docs/re/game_command_line.md](../re/game_command_line.md). What the editor
does, in detail: docs/editor.md, "Launch in game".

## Status (2026-11-22)

- **Command line settled** (six launches): `game_startup_mode battle <xml>;` starts the battle with no menu; `mod "<pack>";` loads mod
  packs (by file name, from `data\` only: an absolute path stops the game); a mod pack's file wins over a working directory's
  (`add_working_directory` serves only paths no pack has); a single tile is a battle map on its own; the first two crashes were a stray
  movie-type pack in `data\`.
- **Built:** the `wce_launch` crate (`launch.rs`, `proton.rs`; shared with the Terrain tool), the Tile tool's `launch_prompt.rs` and
  `launch_climate.rs`; the menu item, `tile_launch_game [--variation <v>] [--map tile|context] [--dry-run] [--ignore-movie-packs]
  [--async]`, the settings, the checks, the cleanup, the work on a thread with a progress window and its breakdown, the launch pack
  (default) and loose delivery.
- **Since 2026-10-28:** a coast tile's battle names its sea surface (without it the game crashed loading a coast tile, 2026-10-30); a tile
  alone takes its own deployment zones, each army's unit inside its zone (2026-11-02); Linux, the Windows build through Proton (works end
  to end, 2026-11-01); a tile alone is in the editor's climate (2026-11-15, below); a combined project asks which variation. The Terrain
  tool (Launch in game at a battle catchment area, `terrain_launch`) and the Cinematic tool (the scene's battle, `scene_launch`) launch
  with the same crate.
- **Timing:** the user's first launch (2026-10-27) took 2 to 3 minutes, nearly all of it stage C; stage C now generates the climates and
  the grass in parallel (47 s -> 12 s on plains 8x8, the whole launch about 18 s), byte for byte the same output.
- **Loose files do not work for a tile** (2026-10-28, TDD's Cair Andros, game_command_line.md test 7): the release's `tiles.pack` has the
  tile database and wins over the loose one, the game finds no terrain and crashes. The shadowing check counts the release's packs, so a
  tile falls back to the pack; loose stays as a setting for files no pack has.
- **Other games:** the executable, data folder, pack format and campaign map are the game's profile's; the armies are kept per game
  (`launch_army`, empty: the game's vanilla keys) and the battle XML leaves out what the game's engine lacks (Rome 2: the alliances'
  indices and the tile upgrades). Rome 2 reads a battle map folder's tile map, so a tile launched alone comes up as open sea: Rome 2 needs
  the Map context's placement (the launch note says so). Rome 2 not launched yet.
- Seen in game: vanilla tiles, TDD tiles, a new settlement tile with procedural walls launched alone (Amon Calen, 2026-11-01; its crash
  was the siege AI over steep ground, [siege_ai.md](siege_ai.md)).
- The wait for the game runs in the Tile tool's update: with another tool active the launch's files are removed when the Tile tool is
  next active (or at the next start).

## Design

1. **Export** (`TileTool::export_inputs` on the main thread, `ExportInputs` run on the worker) into the launch's own folder,
   `<projects>/build/launch/<id>` (`d<id>` for a dry run).
2. **The battle XML** (the benchmark's format, `script/wce/wce_launch.xml`):
   - the map: by default **the tile alone** (its variation folder as `<name>`, `<playable_area>` from the tile's size, its deployment
     zones or the units across its centre), in the editor's climate: the game draws a tile alone in the `default` climate, so the launch
     pack gives `default` the ground textures, grass and trees of the climate and season the editor draws the ground with (choose
     `default` in the editor for the game's own look); as an option the Map context's placement in the game's campaign map
     (`<tile_map_position>` = the placement's centre / the map's size), with `<tile_upgrade>`s for a settlement tile set;
   - the tile's weather and its sea surface; one unit a side, vanilla keys by default, no script. No `<Climate>` / `<Season>` (the game
     reads neither).
3. **Delivery** (a setting):
   - **launch pack** (default): `data\wce_launch.pack` (type Mod, a marker inside) named first (first listed wins), then every mod
     pack chosen in the editor;
   - **loose files**: the files under `<id>/game`, `add_working_directory "<id>/game/";`, and only the mod packs chosen in the editor
     that the tile needs (`launch::needs`). A pack that loads (the release's or a needed mod) with a path the tile ships (always the tile
     database), procedural walls or no mounted files switch that launch to the pack, with the reason in the log.
4. **Launch:** `Attila.exe <mods> [add_working_directory] show_frontend_movies false; game_startup_mode battle script/wce/wce_launch.xml;`
   from the game folder (on Linux through Proton, via a batch file run by the prefix's `cmd`), after checks: the game is not running (it
   refuses a second instance), Steam is, and no non-vanilla movie-type pack is in `data\` (a question: *Launch anyway*). The game takes
   focus: this is the user's click, not an unattended run. The editor's own DXVK settings are not passed to the game.
5. **Cleanup:** the launch's folder and pack are removed when the game exits (the editor waits on the process); the other launches' folders
   only when no game runs (a running game reads its loose files there); what an editor closed meanwhile left goes at its next start. A
   `wce_launch.pack` without the marker is never touched.
6. **Progress:** steps with their times and the last launch's (`launch.step_seconds`), the step's pieces (`wce_foundation::activity`), the
   time left, Cancel (the work ends in its own folder; a new launch waits for it).

## Decisions (user)

1. **Map** (2026-10-27): the tile alone by default (any tile, painted on a map or not); the Map context's placement as an option.
2. **Delivery** (2026-10-28): the user asked for loose files by default, with the mod packs the tile needs; the first loose launch showed
   the release's packs win over loose files (test 7), so the pack is the default again and loose falls back to it for a tile.
3. **Mods on the command line** (`mod "<pack>";`); `used_mods.txt` is never touched.
4. **Armies:** a minimal preset, one unit a side far apart and a long duration; vanilla units by default, editable in Settings. Extra
   assets go into the launch, removed after every launch.
5. **Progress** (2026-10-28): a loader with each step and what it is doing, and how long to wait.
6. **Deployment** (2026-11-02): a tile alone uses the tile's own deployment zones.

## Open

- [ ] Not yet seen in game: the Map context mode; Rome 2 at a placement.
