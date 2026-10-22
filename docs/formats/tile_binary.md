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
4 `hf_height_map.data` of CHMF **version 4** (`roads*/2x2_straight_br_l*/bb`; read since the CHMF decoding). What each codec understands:

| Codec | Files | Content decoded |
| --- | --- | --- |
| rpfm_lib ESF (CAAB) | building lists, `far_building_list`, `bmd.grass_list`, `bmd.tree_list` | the node tree (names, values); the meaning of the building fields to map |
| rpfm_lib FASTBIN lists, BMD, tile database | tree / grass / civilian bins, BMD binaries | yes |
| rpfm_lib rigid model | terrain meshes | vertices and indices |
| `wce_formats::chmf` | `hf_height_map.data`, `hf_water_map.data` | **decoded** (heights; see below) |
| `wce_formats::markers` | `*.markers` | framing only: **payloads to decode** (point lights and particle emitters partly known, docs/re) |
| `wce_formats::image_containers::DdsFile` | `*.dds` | container; DXT5 pixels decoded by `dds_decode`, the compressor not reproduced |
| lossless XML | the XML files | text |
| none | older `.tree_list` / `.grass_list`, `tile.ted_binary`, `*.outlines`, `.jpg` | **to decode** (the jpg stays opaque) |

## CHMF (`hf_height_map.data`, `hf_water_map.data`): decoded (2026-10-22, `wce_formats::chmf`)

The engine's `WARSCAPE::COMPRESSED_HEIGHT_MAP` (`Warscape.AssemblyKit.dll`, exported names: `load`, `value` 0x104a9150, the block
decoder 0x104a9790 and its mode table at 0x107b68e8; the game's reader `empire.retail.dll` fn 0x11857a00).

- Header: `"CHMF"`, u32 version (4 or 5), u32 width, height (samples; a battle tile has `cells * per_cell + 1`: 257 x 129 for a 2 x 1 tile at
  1 m, 961 x 961 for mont_blanc's 30 x 30 at 4 m), u32 `cells_x`, `cells_y` (the unit of the region rectangles, in samples: 1 in battle
  tiles, 8 or 32 in TDD campaign tiles), f32 min, f32 range (metres), version 5: u32 spacing (metres per sample).
- u32 region count, per region `x0, y0, x1, y1` (cells, end exclusive); then per region u16 x 2 (0, 0xFFFF in battle tiles; meaning not
  read), u32 block count, u32 data size, the block offsets (u32), the block data.
- Blocks of 16 x 16 samples, row by row; the last block of a row / column holds the rest (961 = 60 x 16 + 1: 61 x 61 = 3,721 blocks). A
  table can stop early (545 TDD campaign files): the game reads a missing block as 0.
- A block's first byte `k` picks the mode (the table at 0x107b68e8): 0 = one u16 for the block; 1 ..= 127 = a palette of `k + 1` u16, then
  one `ceil(log2(k + 1))`-bit index per sample; 128 ..= 142 = a u16 base, then `k - 127` bits per sample added to it; 143 = one u16 per
  sample. Bits are packed from the low bit of each byte (the game reads them through unaligned u16 / u32 loads).
- A sample is `u16 / 65535`; the height `min + range * sample`. Row y of the grid is the tile's z (no flip, checked against the kit's
  `final_heights.dds` of mont_blanc: the pack was built from an older raw tile, so only 57 % of the samples agree within 1 cm).
- Checked: the 1,911 extracted CHMF files (vanilla and TDD, battle and campaign, versions 4 and 5) parse, decode and write back
  byte-identically; on mont_blanc the steps across block borders are the same as inside blocks (mean 1.08 against 1.11 m: no seams).
- Not exact against an oracle yet (the only BOB-made pair in `~/wce_re/bobkit` is a flat tile); the encoder BOB uses (`convert`,
  0x104aae80 / 0x104ab0d0 / 0x104ac240) is for step 4.
