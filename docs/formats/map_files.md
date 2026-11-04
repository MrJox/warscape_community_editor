# Map files: the Terrain tool's model and formats (2026-11-02)

The Terrain tool (docs/plans/terrain_editor.md) reads a map folder into a game-neutral model and writes it back. What the files are and
which tool writes them: docs/re/terry.md; BOB's outputs: docs/re/terry.md, "BOB_Terrain's outputs"; the built files' layouts:
docs/re/battle_maps.md.

## Crates

- `wce_terrain`: the model. `MapModel` holds the decoded files as `Item<T>` (path, value, the original `Blob`, an edited flag) and carries
  every other file as its blob. `PagedU16` (LF fields: pages of 64 x 64 samples, page after page, the file's own 16-bit values),
  `CellLayer` (a cell layer: a palette of every colour the image has and a u16 index per cell), `ExplicitTiles` (line by line, each line's
  ending kept), `TerryProject` (the `.terry` as a lossless `XmlDoc` with its entities indexed: id, name, components, children).
- `wce_terrain_io`: `MapFiles` (a folder as named blobs, `from_dir` / `write_to`) and `attila::read` / `attila::write`, in memory.
- The built files' readers and writers stay in `wce_tile::battle_map` (`tile_map`, `tile_list`, `locations`), which the tile tool's map
  context uses; `locations.blm` now keeps every record's last two values and the file's tail (cell count, climate table) and writes back.

## Attila's map folders (`raw_data/terrain/battles|campaigns/<map>`)

| File | Read as | Written when edited |
|---|---|---|
| `tile_map.png`, `climate_map.png`, `culture_map.png`, `snow_map[_<season>].png`, `season_mask_<season>_01.png`, `layout_map.png` (8-bit RGB / RGBA) | `CellLayer` | the kit's PNG encoder (`PngFile::encode`); a file with other chunks (Photoshop's `pHYs`, `iCCP`, `cHRM`) keeps them in place and only its IDAT run changes (`PngFile::encode_keeping_chunks`) |
| `lf_heights.tif`, `lf_sea_heights.tif` (16-bit grey, uncompressed, one strip; TEd's 242-byte layout or Photoshop's 19 to 22 entries) | `PagedU16` | the samples rewritten in place: header and bytes after the strip unchanged (a one-sample edit changes two bytes) |
| `explicit_tiles.txt` | `ExplicitTiles` | the edited lines only |
| `<map>.terry` | `TerryProject` | `XmlDoc`: only the edited spans |
| the folder's `*.xml` (`config.xml`, `masks.xml`, `map_info.xml`, a TEd map's battle XML) | `XmlDoc` | only the edited spans |
| anything else (`normal.dds`, `nogo.dds`, `.psd`, `rules.bob`, sub folders) and a file that does not read as its kind | carried | never |

A file not edited is written back as its original blob (a file on disk is copied, not read into memory).

## Gates

- `map_roundtrip_check <raw_data>... [--filter]`: every map folder read then written unedited (byte for byte), every decoded file made
  again from the model (TIFFs, text byte for byte; PNGs byte for byte when the kit's encoder wrote them, else the same pixels and the same
  other chunks), a one-sample LF edit (two bytes change) and a one-cell paint (only the IDAT chunks change). 2026-11-02, Attila's and
  Thrones' kits: **129 maps, 862 files, 0 failures**; made again 91 PNGs byte for byte, 168 Photoshop PNGs the same pixels, 232 TIFFs and
  151 text files byte for byte, 232 minimal edits; about 45 s. The campaign battle map reads in 1.3 s (164 MB decoded), the campaign map
  in 2.0 s (436 MB).
- `map_built_roundtrip_check [--game <dir> [--vanilla]] [<map folder>...]`: the packs' built files (every `terrain/battles/<map>`) and
  loose folders (BOB's outputs) parsed and written back. Attila with its enabled mods and BOB's two scratch outputs: 19 CHMF, 3 858
  deployment XML, 5 `locations.blm`, 17 tile lists, 5 tile maps, all byte for byte; Thrones (vanilla): 5, 691, 1, 4, 1, all byte for
  byte. Rome 2 (vanilla, `WCE_GAME_KEY=rome_2`, 2026-11-04): CHMF (111), deployment XML (8 168), `locations.blm` (7, version 7),
  `terrain.tile_list` (104: versions 7 to 9; Rome 2's loader fn 0x115b9940: 14-byte placements, a rectangle list, version 7's strings
  UTF-16, version 9's last section kept verbatim) and `tile_map` (7, version 6: the index's chunk table and the cells' unknown bytes
  kept), all byte for byte (`wce_tile::battle_map::tile_list` / `tile_map`).
- In `scripts/gates.sh`: `map_roundtrip` (one settlement map), `map_built_roundtrip` (the vanilla packs); `--corpus` adds
  `map_roundtrip_all`.
