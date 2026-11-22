# Opening and saving exported tiles in the editor

`wce_tile_io::attila::workflow` ties the round trip together for the editor (the tile tool's `open_exported` / `save_exported` / `save_to_pack`
commands, File > "Open exported tile...", File > Export > "As exported tile..." and "To mod pack...").

## The game

`workflow::Game::from_vfs(<mounted packs>, <kit>)` (the editor's packs: the release, the patches, the chosen mods; `Game::load(<game>,
<folder>)` for the checks mounts every pack of the folder) reads, once:

- from the packs (`attila::game_data::GameData`): the tile databases (Attila's `tile_database.bin`, every pack's that has one, a mod's
  first; Rome 2's XML database, docs/formats/rome2_tiles.md), the buildings' destruction levels (`<key>_tech.cs2.parsed`, else the
  models' `_destructNN` names) and the `battlefield_buildings` tables (category, `destruct_thresholds`; through rpfm's schema of the
  game, `wce_foundation::settings::rpfm_schema_path`: `$WCE_RPFM_SCHEMA`, the settings' DB schemas folder, the editor's downloaded
  schemas, else rpfm's own), the game's conventions (`TileConventions`);
- the kit's vegetation parameters (`assembly_kit/working_data`), when the kit has the tiles.

## Opening

`open_exported(game, files, folder, workspace)`: the exported tile (a folder on disk under `.../terrain/tiles/`, or the packs' folder
`terrain/tiles/<kind>/.../<variation>`) is read (`attila.tile_binary`), decompiled (docs/formats/tile_decompile.md) and written as a TEd
project into the workspace, a kit-like tree: `<workspace>/raw_data/<folder>` and the entry in
`<workspace>/raw_data/terrain/tiles/<kind>/_tile_database/TILES`, with the kit's `_settings.xml` next to it. The editor then opens it
like a kit tile. In the editor the workspace is the projects folder in the kit's layout (`<projects>/assembly_kit`, `$WCE_TILE_WORKSPACE`
for the tests; see "Settings and first run" in docs/editor.md): the project is at the tile's own folder (the kit's tile of the same name
shares it), its `tile.wce` marked `wce_extracted_tile="true"` (docs/formats/tile_wce.md). A project there already opens instead; the
Tiles panel and the menu ask first whether to extract over it (`open_exported ... --fresh`). The decompile's notes go to the log.

## Saving

`save_exported(game, project folder, out)`: the editor saves the project first (tile.wce, the terrain layers); the scene is converted for
TEd and the maps composed from the layers (`save_exported_with` takes the converted scene), then the project is compiled with stages A, B
(the kit's `raw_data` for BOB's inputs: the tile database settings, the linking tiles' heights, the buildings' height meshes) and C, and
written to `<out>/<folder>`; `out` is the folder File > Export asks for (`save_exported [folder]`: by default the entry's last one, else the
projects folder). The tile database in effect for the tile's kind gets the project's entry (`merge_entries`: the row of the same location
replaced, or a new one where BOB's order puts it, larger tiles first; a missing tile set added) and is written to
`<out>/terrain/tiles/<kind>/tile_database.bin`: packs replace a file by its path, so the mod's database must hold every tile. While a
decompiled project's terrain is unchanged the row keeps the database's `min_height`, `scale` and `raw_data_tri_density` (the decompiled
entry's are its height map's range). Files already in that folder that the compile did not write are noted, not removed. A combined project
(docs/formats/tile_wce.md) exports each ticked variation (`--variations a,b`) as a project of its own. Each exported project gets its
`loading_screen_radar.jpg` drawn by the editor, unless a hand-made one is there (docs/plans/radar.md).

`save_exported_with(game, project folder, scene, out, base_of)` is the same for a project with buildings the game's packs don't have yet
(generated walls, docs/plans/procedural_walls.md): `base_of` names the building a key was made from, whose destruction levels and
thresholds the compile uses for it.

## Saving into a mod pack

`save_exported_to_pack(game, project folder, scene, pack)` (File > Export > To mod pack..., `save_to_pack <pack>`): the same compile into a
scratch folder, then its files (the tile's folder, the tile database's: Attila's `tile_database.bin`, Rome 2's tile set file and
`_settings.xml` when a texture group was added) into the pack, created if missing; the pack's files below the tile's folder go first.
A game whose kit has no tiles (Rome 2) saves only so. An extracted tile is read without its extracted mark, as it was fingerprinted.

## Checks

- `tile_workflow_check -- <game> <scratch> <tile folder>...`: open and save as the editor does, then the saved folder against the packs'
  file by file and the database against the one in effect. farm_western/1x1/aa: 29 of 31 files and the database identical (two stage-A
  files within the text's precision); a campaign river tile all 17 and the database; western_city_b/small 41 of 59 (the stage-A files,
  the text's precision and TEd's garbage bytes). `WCE_WORKFLOW_EDIT=1` moves a tree and raises the terrain before saving: the terrain
  and vegetation are made again, the textures carried, the database row takes the new height range.
- `scripts/tile_exported.txt` (the app, `--script`): open from the packs, move a tree, paint, `save_exported`, the files exist.

## The Tiles panel

Its source combo lists the projects, the kits, then "game packs (exported tiles)": the tile databases in effect (battle and campaign) as
rows. A double-click or a variation button opens the tile through `open_exported` from the packs. A tile extracted before opens the
project, edits included, after asking (open it, or extract over it); `open_exported <folder> --fresh` extracts it again. `tile_browse
<kit number | packs> [filter]` sets the source and filter from a script.
