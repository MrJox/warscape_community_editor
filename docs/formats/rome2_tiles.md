# Rome 2's tiles

Rome 2's battle and campaign tiles are read and written by the Attila formats (`attila::TileBinary`, the decompiler and the compiler)
with Rome 2's conventions (`wce_tile_io::conventions::TileConventions::ROME_2`). Its Assembly Kit has no tiles, no TEd and no BOB
tile, vegetation or terrain processors: the packs are the only source, mod packs the only output (docs/plans/rome2.md).

## The packs

- The tiles are in `tiles.pack` .. `tiles4.pack`, their patches `tiles*_rome2.pack`, and `gaul.pack`, `greeks.pack`, `punic.pack`.
  `terrain*.pack` hold the battle maps (`terrain/battles`), the campaign and the ground textures.
- The patch packs (header type 2) replace single files of a folder, not folders, and an empty file in one removes the file of the
  packs below it (1,027 `.fxc`, about 30 tile files). `wce_vfs` puts the patch packs first and drops removed files.
- `data/manifest.txt` is stale (no `*_rome2.pack`, sizes wrong, packs that do not exist): the vanilla packs are rpfm's list
  (`GameProfile::ROME_2`).

## The tile database: XML in the packs

`terrain/tiles/<battle|campaign>/_tile_database/`: there is no `tile_database.bin`. `rome2::tile_database::Rome2TileDatabase` reads it
as rpfm's `TileDatabase`, which the decompiler and the editor read, and writes a tile back into its set's file
(`rome2_tile_database_check`: all 1,437 tiles give their file back byte for byte).

- `_settings.xml`: `CONVERSION_PARAMS` (`triangle_decimation_*_factor<n>`, Attila's `*_factors<n>`), `RENDER_PARAMS` (fewer than
  Attila's), `TEXTURE_GROUPS` > `GROUP name='group14' red='sand' green='forest' blue='grass' alpha='mud'`, `CLIMATES` > `CLIMATE` >
  `TEXTURE` (no `TEXTURES` wrapper; `const_smoothness<n>` is Attila's `lerp_smoothness<n>`; only what differs from the defaults:
  reflectivity 0.009, smoothness 0.1, repeat 40.1 m).
- One file per tile set, or several sets in one file: `TILE_DATABASE` > `TILE_SET name red green blue [also_place_tile_set link_as_set
  linking_tile shared_geometry]` > `TILE location masked width height texture_group red_channel .. alpha_channel
  custom_alpha_blend_texture [encampable requires_infield_lodding random_rotatable red green blue]`, with `NEW_MASK data` (the
  mask's bits, Attila's `mask` attribute), `LINK_TARGET x y target_set`, `VARIATION name [red green blue] scale min_height
  normal_strength overlap_border_size [raw_data_tri_density] [blend_common index_common] [normal_common]` and `LINK x y test name base_x
  base_y no_offline_blend is_entry blend_size` (`test` is `equals` / `not_equals`, Attila's `TLT_EQUALS` ...). An attribute at its
  default is left out (`random_rotatable` true, `raw_data_tri_density` 128, the others false, 0 or empty). Numbers are `%.6f` as MSVC
  rounds them (half away from zero); colours integers.
- `texture_group` names a settings group with the tile's four channels; a saved tile whose channels no group has adds one
  (`group<n>`) to `_settings.xml`.
- The workspace of a decompiled tile gets the settings in the kit's format (`rome2::kit_settings`), the tile's entry as the kit
  writes it, like an Attila tile.

## The tile folders

The same files as Attila's (CHMF heights and water, the DDS maps, RMV2 meshes, XML logic, markers, building lists), and:

- no FASTBIN anywhere: no `.tree_list.bin` / `.grass_list.bin`, procedural BMDs, escalation BMDs, civilian files or outfield tents.
  The vegetation is the older lists (`<climate>.tree_list`, `default.tree_list`, `default.grass_list`);
- `tile.ted_binary` (TEd's state, 76 bytes mostly) and `non_terrain_buildings.outlines` / `non_terrain_sprawl.outlines` (an
  `OUTLINE_LIST` per building list): carried into the project and written back as they are.

### Markers

| Class | Rome 2 versions | Notes |
| --- | --- | --- |
| PROP_MARKER | 3 (empty files only), 4, 5, 6 | 4: version 5 without the last flag (`unknown_69`, the water plane's): records of 69 / 86 bytes |
| PARTICLE_EMITTER_MARKER | 3 (empty), 4, 5 | as Attila's 4 and 5 |
| POINT_LIGHT_MARKER | 1 (empty), 2 | |
| AI hint polyline / list, separator | 1 | as Attila's |
| GENERIC_MARKER | 1 | one file with records (`custom_tile/2x2/front_end_scene_final`): not decoded |

A decal's scale (prop markers): in Attila from version 6 the fields are signed and the rows' lengths their magnitudes; in Rome 2, at
every version, the fields are magnitudes and the rows carry the sign (Attila's version 5). `TileConventions::signed_decal_fields`.

### Building lists

CAAB ESF `BATTLEFIELD_BUILDING_LIST` > `BATTLEFIELD_BUILDING_LIST_BLOCK`, one block per building:

| Version | Values | Fields past the six flags |
| --- | --- | --- |
| 10 | 14 | none (no `position_type`) |
| 11 | 15 | `position_type` |
| 12 (Rome 2) | 16 | `position_type`, f32 `burn_amount` |
| 12 (Attila) | 17 | `position_type`, bool `toggleable`, bool `lite` |

The f32 is 0 in 159,320 buildings, 1 in 137 and between in a few, never with `on_fire`; EmpireUtility's XML writer of Rome 2's kit
writes ` burn_amount='` right after ` dockable='`. It is TEd's `burn_amount` of the building instance. Every version is a TEd
list of Rome 2 (decompiled to instances); the compiler writes the decompiled file's version (else 12) with the game's layout.

Rome 2's TEd wrote each building's `scale` as the length of each axis of its matrix in memory: 1 within a few ulps, which the
project's text cannot carry (the compile writes 1; `tile_decompile_check` counts it apart).

Re-encoded with rpfm, a Rome 2 list has the same values but not always the same bytes: Rome 2 writes its ASCII string table in index
order (rpfm in the order of Attila's string hash map) and pads some block sizes to five bytes at other places than rpfm's model of
Attila's 1 MiB write buffer.

### Capture locations

`capture_location_list.xml` is Attila's, but an older file of Rome 2 has several `CAPTURE_LOCATION_LIST`s (alternative sets): two
tiles of the packs in effect (`egyptian_port_b/large`, `hb_carthage/large`) and older copies the patches replace. The first list is
decompiled, the others noted as lost.

## Saving (docs/formats/tile_workflow.md)

`Tile > Save to mod pack...` (`save_to_pack <pack>`) compiles the project and puts the tile's folder and its tile set's file
(`_tile_database/<set>.xml`, `_settings.xml` when a texture group was added) into the pack, replacing the pack's files of the folder.

- Stage A (the logic, markers, building lists): compiled from the project.
- Stage B (BOB's terrain files): carried while their inputs are unchanged. Made again, the textures and water work; the terrain needs
  the linking tiles' final heights and the buildings' raw height-map meshes, which no Rome 2 kit has: noted, the packs' files stay
  (a mod pack replaces files, so the tile keeps the game's terrain files).
- Stage C (vegetation): Rome 2 has no vegetation parameters: the lists are written as decompiled, noted when the project changed.

## Checks

- `rome2_tile_database_check -- <Rome 2>`: every tile written back into its set's file, byte for byte.
- `tile_decompile_check -- <Rome 2> [--filter ..]` with `WCE_GAME_KEY=rome_2`: decompile, read back, compile, compare.
- `tile_workflow_check -- <Rome 2> <scratch> <folder>..` with `WCE_GAME_KEY=rome_2`: the editor's open and save, file by file, the set
  file against the packs'.
- `scripts/gates.sh` runs them with `WCE_ROME2=<Rome 2 folder>`.
