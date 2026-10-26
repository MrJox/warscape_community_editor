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
| rpfm_lib ESF (CAAB) | building lists, `far_building_list`, `bmd.grass_list`, `bmd.tree_list` | the node tree; **building lists typed** (`attila::building_list`, below); the campaign `bmd.*_list` not mapped yet |
| rpfm_lib FASTBIN lists, BMD, tile database | tree / grass / civilian bins, BMD binaries | yes |
| rpfm_lib rigid model | terrain meshes | vertices and indices |
| `wce_formats::chmf` | `hf_height_map.data`, `hf_water_map.data` | **decoded** (heights; see below) |
| `wce_formats::markers` | `*.markers` | **decoded** (records split and named per class and version; see below) |
| `wce_formats::image_containers::DdsFile` | `*.dds` | container; DXT5 pixels decoded by `dds_decode`, the compressor not reproduced |
| lossless XML | the XML files | text |
| `wce_formats::old_lists` | older `.tree_list` / `.grass_list` | **decoded** (below) |
| lossless XML | `*.outlines` (XML text: `OUTLINE_LIST` > `OUTLINE` > `position x y`) | text |
| none | `tile.ted_binary`, `.jpg` | opaque: `tile.ted_binary` is a camera (below), the jpg a loading screen |

## CHMF (`hf_height_map.data`, `hf_water_map.data`): decoded and encoded (2026-10-22, `wce_formats::chmf`)

The engine's `WARSCAPE::COMPRESSED_HEIGHT_MAP` (`Warscape.AssemblyKit.dll`, exported names: `load`, `value` 0x104a9150, the block
decoder 0x104a9790 and its mode table at 0x107b68e8; the game's reader `empire.retail.dll` fn 0x11857a00).

- Header: `"CHMF"`, u32 version (4 or 5), u32 width, height (samples; a battle tile has `cells * per_cell + 1`: 257 x 129 for a 2 x 1 tile at
  1 m, 961 x 961 for mont_blanc's 30 x 30 at 4 m), u32 `cells_x`, `cells_y` (the unit of the region rectangles, in samples: 1 in battle
  tiles, 8 or 32 in TDD campaign tiles), f32 min, f32 max (metres; the engine's `min_height`, `max_height`), version 5: u32 spacing (metres per sample).
- u32 region count, per region `x0, y0, x1, y1` (cells, end exclusive); then per region u16 x 2 (0, 0xFFFF in battle tiles; meaning not
  read), u32 block count, u32 data size, the block offsets (u32), the block data.
- Blocks of 16 x 16 samples, row by row; the last block of a row / column holds the rest (961 = 60 x 16 + 1: 61 x 61 = 3,721 blocks). A
  table can stop early (545 TDD campaign files): the game reads a missing block as 0.
- A block's first byte `k` picks the mode (the table at 0x107b68e8): 0 = one u16 for the block; 1 ..= 127 = a palette of `k + 1` u16, then
  one `ceil(log2(k + 1))`-bit index per sample; 128 ..= 142 = a u16 base, then `k - 127` bits per sample added to it; 143 = one u16 per
  sample. Bits are packed from the low bit of each byte (the game reads them through unaligned u16 / u32 loads).
- A sample is `u16 / 65535`; the height `min + (max - min) * sample` (BOB's `COMPRESSED_HEIGHT_MAP::convert` writes
  `(h - min) / (max - min) * 65535` truncated). Row y of the grid is the tile's z (no flip, checked against the kit's
  `final_heights.dds` of mont_blanc: the pack was built from an older raw tile, so only 57 % of the samples agree within 1 cm).
- Checked: the 1,911 extracted CHMF files (vanilla and TDD, battle and campaign, versions 4 and 5) parse, decode and write back
  byte-identically; on mont_blanc the steps across block borders are the same as inside blocks (mean 1.08 against 1.11 m: no seams).
- **The encoder** (`encode_region`, the engine's `FUN_104aa1b0`): every block holds 256 samples, those past the last row / column clamped
  to it (the game's `value` reads an edge block with the remaining width as its stride, so it misreads the last partial column and row;
  the model holds the encoder's samples). Mode: one value -> 0; up to 128 values -> a palette unless the delta (bits of max - min) is
  smaller; more -> the delta unless it is 513 bytes or more (then raw). The region's two u16 are the smallest and largest sample, except in
  17 flat files (kept as data). Decode then encode gives 1,349 + 17 of the 1,911 files byte for byte; the other 545 (TDD campaign tiles)
  have short block tables, not the engine's encoder: written from their original bytes while their samples are unchanged.

## Markers (`*.markers`): decoded (2026-10-22, `wce_formats::markers`)

Written by `EMPIREUTILITY::BASE_MARKER_REPOSITORY::write` (`EmpireUtility.AssemblyKit.dll` 0x106e5c30; the old versions are read by each
class's `handle_backwards_compatibility`, e.g. `POINT_LIGHT_MARKER` 0x100a1520). `wstr` = u16 count + UTF-16, `str` = u16 count + bytes.

- Header: wstr `BASE_MARKER_REPOSITORY`; the repository id (16 bytes: 12 zero + u32 0x9D9BB); u32 1; u16 1; the class id (12 zero + u32);
  wstr class name; the class id again; u32 version; u32 count.
- The records (layouts below), then the **groups**, omitted in empty files: u16 group count; per group u16 key part count, the parts, u32
  index count, the u32 indices of the group's records. The key is a wstr for props and lights (the model path, the TEd layer name), a u32
  for the AI polylines and polyline lists (8 in the vanilla files: the hint type, to confirm), a str for the AI separators (`PALT_FORT`,
  `PALT_GENERIC`). This is `EMPIREUTILITY::MARKER_IB_NODE<T>` (reader 0x1009c7d0).
- Record layouts (`wce_formats::markers::layout`), every file of the extraction fits (records consumed exactly, the groups end the file):

| Class | Version | Files | Layout |
| --- | --- | --- | --- |
| `PROP_MARKER` | 5, 6, 8, 9 | 71, 97, 16, 375 | 4 x 4 matrix (row-major, translation last); u8 x 6 (byte 65 = is decal); v6+: u32 seasons (15 = all); v8: 3 bytes; v9: u8, then 4 u8 visible inside / outside snow / destruction zones; if decal: f32 parallax_scale, u8 tiled, f32 scale x, y, z (z negative) |
| `PARTICLE_EMITTER_MARKER` | 4, 5, 7, 8 | 675, 325, 75, 5 | matrix + 11, 15, 16, 18 bytes not named yet |
| `PARTICLE_EMITTER_MARKER` | 9 | 264 | matrix; 6 bytes; u8 autoplay; u8 use_in_outfield; the 4 visibility u8; f32 scale; u32 seasons |
| `POINT_LIGHT_MARKER` | 2, 3 | 40, 230 | f32 x, y, z, radius, r, g, b, colour_scale; v3: u8 (1); str animation type (`LAT_*`); f32 animation speed x, y, colour_min, random_offset; str falloff (`WPLFT_*`) |
| `AI_HINT_POLYLINE_MARKER` | 1 | 613 | u32 count + (x, z) f32 points |
| `AI_HINT_POLYLINE_LIST_MARKER` | 1 | 281 | u32 count + point lists |
| `AI_SEPARATOR_MARKER` | 1 | 175 | u32 count + points |
| `AI_HINT_DIRECTED_POINT`, `BUILDING`, `DECAL`, `GENERIC`, `TREE` | | 27, 27, 27, 11, 27 | no records in any shipped file |

The names of the prop and particle bytes come from their distributions (seasons is 15 in every record, the four visibility bytes are 1 in
all but 5 of 300k) and BMD's PROP / PARTICLE_EMITTER (docs/formats/fastbin_tile.md); the frames of positions against tile.ted (x, y doubled,
z mirrored for the lights: docs/re/renderer.md) are settled in step 3.

## Building lists (`building_list`, `far_building_list`, `building_list_sprawl`, `building_list_veg`): decoded (2026-10-22)

CAAB ESF (rpfm_lib), `BATTLEFIELD_BUILDING_LIST` (record version 12; 11 in three sprawl lists) > `BATTLEFIELD_BUILDING_LIST_BLOCK`, one
block per building of 17 values (15 in version 11): ascii key, vec3 position, f32 x 4 orientation (quaternion i, j, k, w), vec3 scale,
f32 starting_damage, bool on_fire, start_disabled, weak_point, ai_breachable, indestructible, dockable, u32 position_type (2 =
`lf_relative`), then (version 12) bool toggleable, lite. The names are the XML twin's (TEd's `building_list.xml`; `wce_tile_io::attila::
building_list`). Checked (`tile_building_list_check`): the 591 extracted lists read (4,064,620 buildings), and the 32 that ship with an XML
twin agree with it on every attribute of 6,698 buildings (numbers in the XML's `%.6f`, rounded half away from zero).

## Older tree and grass lists: decoded (2026-10-22, `wce_formats::old_lists`)

`<climate>.tree_list`, `default.tree_list`, `outfield_tents.tree_list`, `default.grass_list` (no `.bin`): u32 version, u32 type count, u32
record count; per type a u16-counted name and the u32 index of its first record; then the records. Trees v2 (3,196 files): 10 bytes, half
floats x, y, z (metres in the tile, up to 5,116), u8 0, u8 scale x 127, 2 bytes not named (the first 0 or 1, the second varies). Trees v0
(5 files): 28 bytes, f32 x, y, z, f32 (0, 1, 0), 3 bytes, u8 scale x 127. Grass v1 (703): 6 bytes, half floats x, y, z. Grass v0 (5): 12
bytes, f32 x, y, z. Every extracted file (3,909) reads with no byte left and writes back identically. Their climates are often ones Attila
does not have (`rome_alpine`): left from Rome II, the game reads `.tree_list` still (loader list above).

## `tile.ted_binary` (22 files, 76 bytes)

A camera: 8 doubles stored with their two 32-bit halves swapped, after 8 bytes (0, 0xC0000000): the vertical field of view (1.0472), a
point, the aspect ratio (1.71889), another point; then u32 0. The points do not match the kit's tile.ted cameras (the packs were built from
other raw states). The game's tile loader does not read it; carried as an opaque file.

## Step 1 status (2026-10-22)

Every kind of file of the exported tiles has a codec that writes it back byte for byte (`tile_binary_check` over 15,148 extracted files:
all identical). Decoded to values: CHMF heights, markers, building lists, the old lists, and (rpfm_lib) the FASTBIN lists, BMD binaries,
terrain meshes, the tile database. Still raw inside their codec: the bytes of the older particle emitter versions (4 to 8), a few prop
bytes, the campaign `bmd.grass_list` / `bmd.tree_list` (ESF nodes not mapped), `tile.ted_binary`.

## Step 2: the format `attila.tile_binary` (2026-10-22, `wce_tile_io::attila::tile_binary`)

An exported tile folder into the model and back (docs/formats/tile_model.md, "attila.tile_binary"). Gate `tile_binary_roundtrip_check`
over every tile folder of Attila's 31 packs, read from the packs (`docs/baseline/tile_binary_roundtrip_check.txt`, about 3 min): **1,756
folders, every file the format reads identical** (49 kinds: markers, building lists, tree / grass lists of both formats, BMD binaries, the
civilian lists, the tile databases, CHMF, every XML file including the ones with text and comments), the models read back from the written
files equal; kept as original bytes: 545 CHMF (short tables) and 4,438 DXT5 textures (`blend`, `index`, `normal`: the compressor is
step 4); carried as files: the terrain meshes (2,809), loading screens (194), `tile.ted_binary` (22), stray textures, `assembly_kit_rules.bob`.

Notes from the gate: rpfm_lib invents an `id` (u64, random) for trees and capture locations that is not in the files; the model sets it
to 0 (a re-read is the same model, the bytes prove no `id` is data). `building_list.xml` has the root of the binary list: the writer picks
the codec by the file's name first.

## Map-level files (not decoded; tilemaps are a later scope, docs/plans/tile.md)

What the 2026-10-06 survey found about the files BOB_Terrain, Terry and the campaign tools write per map:

- `tile_map.index`: u32 9, width, height. `terrain.tile_list`: u32 11, then a u16-prefixed path table (campaign: 13 MB, 41 sets, 371
  variations). `tile_map.tiles` embeds heap pointers, so BOB itself does not reproduce it: compare it structurally only.
- `lf_height_map.data`: CHMF (above); `lf_height_map.dds`, `lf_sea_height_map.dds`: TEd/BOB's uncompressed DDS template
  (docs/formats/image_encoders.md).
- `locations.blm` (+ `.mask0`, `.mask1`): `BLMF` version 8, holds the `ROME_*` climate names.
- `.terry`: XML, ECS entities (`ECBattleCatchmentArea`, deployment zones; 25 MB for TDD's battle map), written by Terry / TWeak, which
  also run BOB_Terrain's actions.
- `explicit_tiles.txt`: lines `x,y,tile_path,rotation`. Also per map: `config.xml`, `casloadersettings` (a boost archive), `.tmd`
  (World Machine), `<map>.agf`, `map_info.xml`.
- Campaign: `map.hex` (written by CA's CAIME tool; `map_data_config.xml` gives slots and ports) feeds BOB's campaign processor
  (`map_data.esf`, `pathfinding.ppd`, `trade_routes.ptd`, `borders.pbd`, lookup `.tga`, `dynamic_resources.esf`). Out of scope for tiles
  (user, 2026-10-08).
