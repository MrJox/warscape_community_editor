# Exported (binary) tiles

What BOB writes into `working_data/terrain/tiles/<kind>/<set>/.../<variation>/` and the packs ship; Phase 2 of
`docs/plans/tile_roundtrip.md`. Work in progress: this file grows with step 1 (the inventory and the decoding).

## Inventory (2026-10-22, `wce_tile_io --example tile_pack_inventory`)

Attila's `data/` (31 packs, vanilla and TDD): tile files in `tiles.pack` (1,184 folders), `tiles2.pack` (110), `tiles3.pack` (146),
`tiles4.pack` (108), `tdd_pack2_battles` (171), `tdd_pack3_campaign` (33), `castle_hill.pack` and `doomon15lustrummarsh.pack` (2 each):
1,026 battle folders, 108 settlement folders, 622 campaign folders. The counts per kind of file are in the tool's output; the main ones:

- **Terrain** (BOB `Tile`): `mesh.rigid_model_v2`, `outfield_mesh.rigid_model_v2`, `custom_mesh.rigid_model_v2`, `hf_height_map.data`,
  `hf_water_map.data`, `blend.dds`, `index.dds`, `normal.dds`, `ground_types.dds`, `terrain_outlines.xml`.
- **Vegetation** (BOB `Vegetation`): `<climate>.tree_list.bin`, `default.grass_list.bin`, `<climate>_procedural_bmd_data.bin/.xml`,
  `outfield_tents.*`; campaign `bmd.grass_list`, `bmd.tree_list` (CAAB ESF). The older `<climate>.tree_list` and `default.grass_list`
  (no `.bin`, not FASTBIN) are still shipped in 670 / 708 folders, several for climates Attila does not have (`rome_alpine`).
- **TEd's stage-A export**: `building_list` (CAAB ESF), `far_building_list`, `building_list_sprawl`, `definition.xml`,
  `bmd_data_escalation<n>.bin/.xml`, `deployment_areas.xml`, `zones.xml`, `go_outlines.xml`, `non_terrain_outlines.xml`,
  `capture_location_list.xml`, `sound_emitter_list.xml`, `civilian_deployment.bin`, `civilian_shelter.bin`, `tree_random_seed.xml`, and the
  markers: `prop`, `particle_emitter` (and per climate), `point_light`, `decal`, `building`, `tree`, `generic`, `ai_hint_polyline`,
  `ai_hint_polyline_list`, `ai_hint_directed_point`, `ai_separator`.
- Others: `loading_screen_radar.jpg` (87 battle folders), `tile.ted_binary` (22, 76 bytes: a camera, partly decoded), `*.outlines`
  (`non_terrain_veg.outlines` ...), stray textures and copies (`normal - copy.dds`).

## What the game reads (battle tile definition loader, `empire.retail.dll` fn 0x1184eeb0)

In this order: `building_list.xml`, `building_list`, `building_list_sprawl`, `.tree_list`, `outfield_tents.tree_list.bin`,
`default.grass_list`, `nogo_outlines.xml`, `go_outlines.xml`, `terrain_outlines.xml`, `non_terrain_outlines.xml`, `deployment_areas.xml`,
`capture_location_list.xml`, `zones.xml`, the markers (`ai_hint_directed_point`, `ai_hint_polyline_list`, `ai_hint_polyline`,
`ai_separator`, `decal`, `generic`, `particle_emitter`, `point_light`, `prop`), `ground_types.dds`, `vegetation_exclusion.xml`,
`bmd_data.bin/.xml`, `lite_building_outlines.xml`, `<climate>_procedural_bmd_data.bin/.xml`, `.tree_list.bin/.xml`,
`outfield_tents_procedural_bmd_data.bin/.xml`, `default.grass_list.bin/.xml`, `civilian_deployment.bin`, `civilian_shelter.bin`,
`bmd_data_<n>`, `default.tree_list`. The heights, the mesh and the blend textures come through another path (the terrain system).
Which of a `.bin` / `.xml` / plain pair wins is not read yet.

## Codecs (`wce_tile_io::attila::binary`, `tile_binary_check`)

On 15,148 extracted files (every file of the kinds still to decode, five whole tiles), decode then encode is byte-identical for all but
4 `hf_height_map.data` of CHMF **version 4** (`roads*/2x2_straight_br_l*/bb`). What each codec understands:

| Codec | Files | Content decoded |
| --- | --- | --- |
| rpfm_lib ESF (CAAB) | building lists, `far_building_list`, `bmd.grass_list`, `bmd.tree_list` | the node tree (names, values); the meaning of the building fields to map |
| rpfm_lib FASTBIN lists, BMD, tile database | tree / grass / civilian bins, BMD binaries | yes |
| rpfm_lib rigid model | terrain meshes | vertices and indices |
| `wce_formats::chmf` | `hf_height_map.data`, `hf_water_map.data` | framing only (header, block offsets): **to decode** |
| `wce_formats::markers` | `*.markers` | framing only: **payloads to decode** (point lights and particle emitters partly known, docs/re) |
| `wce_formats::image_containers::DdsFile` | `*.dds` | container; DXT5 pixels decoded by `dds_decode`, the compressor not reproduced |
| lossless XML | the XML files | text |
| none | older `.tree_list` / `.grass_list`, `tile.ted_binary`, `*.outlines`, `.jpg` | **to decode** (the jpg stays opaque) |

## CHMF (`hf_height_map.data`)

Version 5, header of 16 u32: magic, version, then for `farm_eastern/2x1/aa` (a 2 x 1 tile): 257, 129 (samples: one per metre plus the
edge), 1, 1, 0, a float 1.477, 1, 1, 0, 0, 257, 129, 0xFFFF0000, 153 (blocks); then the data size and 153 offsets; blocks of 3 to about
200 bytes (compressed). Meaning to decode.
