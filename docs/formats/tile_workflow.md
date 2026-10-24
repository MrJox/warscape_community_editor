# Opening and saving exported tiles in the editor

`wce_tile_io::attila::workflow` ties the round trip together for the editor (the tile tool's `open_exported` / `save_exported` commands
and the Tile menu's "Open exported tile..." / "Save as exported tile").

## The game

`workflow::Game::load(<game folder>)` reads, once:

- from the packs (`attila::game_data::GameData`): the tile databases (a mod's before the game's; per kind, the first is the one in
  effect), the buildings' destruction levels (`<key>_tech.cs2.parsed`, else the models' `_destructNN` names) and the
  `battlefield_buildings` tables (category, `destruct_thresholds`; through rpfm's schema, `~/.config/rpfm/schemas/schema_att.ron`);
- the packs' file index (`stage_b::PackIndex`: the water planes' models, whether a tree model exists, an exported tile's files);
- the kit's vegetation parameters (`assembly_kit/working_data`).

## Opening

`open_exported(game, files, folder, workspace)`: the exported tile (a folder on disk under `.../terrain/tiles/`, or the packs' folder
`terrain/tiles/<kind>/.../<variation>`) is read (`attila.tile_binary`), decompiled (docs/formats/tile_decompile.md) and written as a TEd
project into the workspace, a kit-like tree: `<workspace>/raw_data/<folder>` and the entry in
`<workspace>/raw_data/terrain/tiles/<kind>/_tile_database/TILES`, with the kit's `_settings.xml` next to it. The editor then opens it
like a kit tile. The workspace is `workspace` in the settings' `[tools.tile]` (or `$WCE_TILE_WORKSPACE`), else `<data folder>/tile_workspace` (see "Settings and first run" in docs/editor.md). The decompile's notes go to the
log.

## Saving

`save_exported(game, project folder, out)`: the editor saves the project first (tile.ted, the height map), then the project is compiled
with stages A, B (the kit's `raw_data` for BOB's inputs: the tile database settings, the linking tiles' heights, the buildings' height
meshes) and C, and written to `<out>/<folder>`; the default `out` is the editor's output folder. The tile database in effect for the
tile's kind gets the project's entry (`merge_entries`: the row of the same location replaced, or a new one where BOB's order puts it,
larger tiles first; a missing tile set added) and is written to `<out>/terrain/tiles/<kind>/tile_database.bin`: packs replace a file by
its path, so the mod's database must hold every tile. While a decompiled project's terrain is unchanged the row keeps the database's
`min_height`, `scale` and `raw_data_tri_density` (the decompiled entry's are its height map's range). Files already in the output folder
that the compile did not write are noted, not removed.

`save_exported_with(game, project folder, scene, out, base_of)` is the same for a project with buildings the game's packs don't have yet
(generated walls, docs/plans/procedural_walls.md): `base_of` names the building a key was made from, whose destruction levels and
thresholds the compile uses for it.

## Checks

- `tile_workflow_check -- <game> <scratch> <tile folder>...`: open and save as the editor does, then the saved folder against the packs'
  file by file and the database against the one in effect. farm_western/1x1/aa: 29 of 31 files and the database identical (two stage-A
  files within the text's precision); a campaign river tile all 17 and the database; western_city_b/small 41 of 59 (the stage-A files,
  the text's precision and TEd's garbage bytes). `WCE_WORKFLOW_EDIT=1` moves a tree and raises the terrain before saving: the terrain
  and vegetation are made again, the textures carried, the database row takes the new height range.
- `scripts/tile_exported.txt` (the app, `--script`): open from the packs, move a tree, paint, `save_exported`, the files exist.

## The Tiles panel

Its source combo lists the kits, then "game packs (exported tiles)": the tile databases in effect (battle and campaign) as rows. A
double-click or a variation button opens the tile through `open_exported` from the packs. A tile decompiled before opens the
workspace's copy, edits included; `open_exported <folder> --fresh` decompiles it again. `tile_browse <kit number | packs> [filter]` sets
the source and filter from a script.
