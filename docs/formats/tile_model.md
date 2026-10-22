# The tile model and its formats

A tile is read into one game-neutral model (`wce_tile_model`) and written out of it by format modules (`wce_tile_io`). One game's
format is one `TileFormat` (id `<game>.<format>`): `read(&TileFiles) -> Tile` and `write(&Tile) -> TileFiles`. Plan and status:
`docs/plans/tile_roundtrip.md`.

## The model (`wce_tile_model`)

- **`Tile`**: `scene` (a node tree), `rivers` (node trees with an index), `maps` (rasters by kind and index), `entry` (the tile database
  entry as a node tree, with its file name), `documents` (the other typed files by name: markers, building lists, BMD data ...),
  `variation`, `files` (everything else of the folder, untouched: `OpaqueFile` = relative path
  + blob), `source_format` and `source` (that format's trivia per part, see below).
- **`Node`**: a class (`Sym`, an interned name), its properties in order (`Prop` = name + `Value`), its child nodes. Wrappers (`children`,
  `points`, `modules`, `scene`) are nodes without properties; `wrapped("children")` / `wrapped_mut` reach their nodes.
- **`Value`**: `Bool`, `Int`, `Float` (f64), `Vec2`, `Vec3`, `Euler` (radians), `Colour` (r, g, b, a), `Flags` (names), `Str`, and
  for the binary formats `Floats` / `Ints` (matrices, points, columns of records) and `Bytes` (fields not understood yet). A text a format
  could not type stays `Str`. Equality is meaning: two NaNs are equal (TEd writes `lf_offset="nan"`).
- **`Raster`**: width, height, channels, `Samples` (u8 / u16 / f32), row-major from the file's first row, channels interleaved.
- **`MapKind`**: `Height` (index n of `height_map_<n>`, or the region of a CHMF), `Blend`, `GroundType`, `Nogo`, `LfInfluence`, `Water`,
  and the exported tile's decoded textures `BlendTexture`, `IndexTexture`, `NormalTexture`.

## Trivia: how a format gives its own bytes back

What only the format a tile was read from needs, kept out of the meaning:

- **Per node** (`Trivia`, generic): `verbatim` = the original text of a value the format would not write back from the value (expected
  to be rare; `Node::set` drops it, so an edited value is always written from the value), and `layout` = bytes between tokens that differ
  from the document's style (position codes are the format's). A node without either has no trivia (no allocation).
- **Per part** (`Tile::source`, keyed by the part's file name, `Arc<dyn Any>`): the document style of an XML file, the encoding of an image.
  A writer uses it only when `source_format` is its own id; other formats ignore it.
- `tile.semantic()` compares two tiles without trivia: read(write(tile)) must be the same tile.

## Rules for a format

1. **Nothing is dropped.** A file the format does not model is an `OpaqueFile`; a file of a modelled kind that does not read is an
   `OpaqueFile` too, and the read report says why.
2. **Read then write gives the same bytes** for every file the format models, or the file is reported as `Kept` (an image whose encoder
   is not reproduced, written from its original bytes while the pixels are unchanged).
3. **Values are typed by a schema** (`xml::values::Schema`): (class, attribute) -> `Kind` (and its number style). Unknown attributes read
   as `Str` and are counted; the gate requires 0.
4. **An edit changes only what it edits**: setting a value back to what it was gives the original bytes; a new node is written in the
   format's style.
5. Every format registers in `wce_tile_io::formats()` and gets a corpus gate and kit sample tests like `attila.ted_project`'s.

## `attila.ted_project`

The variation folder of a tile in Attila's Assembly Kit and its entry (`_tile_database/TILES/<name>.xml`, found through `TileIndex`).

| Part | In the model | Codec |
| --- | --- | --- |
| `tile.ted` | `scene` | XML, `TedSchema` (docs/formats/tile_ted.md "Attribute kinds") |
| `river_spline.xml`, `river_spline<n>.xml` | `rivers` | XML, `RiverSchema` (all `%g`) |
| `height_map_<n>.png`, `ground_type_map.png`, `nogo.png`, `lf_influence_map.png` | `maps` | PNG, the kit's encoder; ancillary chunks: kept |
| `blend_map.tif` | `maps` | TIFF, uncompressed: TEd's 242-byte header rebuilt from the size, any other header kept while the size is the same |
| the entry | `entry` | XML, `EntrySchema` (`%.6f`, tabs, single quotes) |
| everything else (BOB outputs, artists' files, legacy `.markers` and lists, sub folders) | `files` | untouched |

**The XML codec** (`wce_tile_io::xml`) reads the shape of TEd's family of files: one element per line, a fixed indent, attributes on the
element's line, leaves self-closed, no text or comments. `DocStyle` (prolog, epilog, line end, indent, quote) is detected per document; the
writer predicts every byte between tokens from it and the node's layout trivia holds the exceptions (none in the kit). Number styles:
`Ted` (float32, `%.6f` rounded half away from zero as MSVC does, trailing zeros removed), `G` (`%g` of the float32), `Fixed6` (`%.6f` of the
double, zeros kept).

**Verification** (Attila Assembly Kit, 2026-10-22, `tile_roundtrip_check`, `docs/baseline.md`): 1,380 tiles, every modelled file
byte-identical, 222 images kept (Photoshop / World Machine PNGs), 3 PNGs that do not read carried along (`plains/3x1/3x1_aa`, their LF bytes
were converted to CRLF), 7 of 83.6 million values kept as text (flags without TEd's leading space on 5 root nodes of old files, two denormal
`lf_offset`s that `%g` does not give back), 0 unknown attributes, 0 layout trivia.

## `attila.tile_binary`

An exported tile: a variation folder as BOB writes it into `working_data` and the packs ship it (docs/formats/tile_binary.md).

| Files | In the model | Codec |
| --- | --- | --- |
| `hf_height_map.data`, `hf_water_map.data` | a `CHMF` document (header, `region` children: rect, tag) + a `Height` / `Water` raster per region | `wce_formats::chmf`, the engine's encoder; a short block table: kept |
| `*.markers` | a document per file: the class as root, `record` children (the layout's named fields), `groups` | `wce_formats::markers` |
| `building_list*`, `far_building_list` | `BATTLEFIELD_BUILDING_LIST` document, `building` children | rpfm_lib ESF, `attila::building_list` |
| `*.tree_list.bin`, `*grass_list.bin` | `TREE_LIST_BIN` / `GRASS_LIST_BIN`: a `type` child per type, the trees as columns | rpfm_lib |
| `*.tree_list`, `*.grass_list` (older) | `TREE_LIST` / `GRASS_LIST`: `type` children, a `records` child with columns | `wce_formats::old_lists` |
| BMD binaries, civilian lists, `tile_database.bin`, other ESF | `BMD`, `CIVILIAN_*`, `TILE_DATABASE`, `ESF` documents | rpfm_lib through `serde_node` |
| `blend.dds`, `index.dds`, `normal.dds` | decoded RGBA rasters | kept while unchanged (no compressor yet) |
| `ground_types.dds` | a `GroundType` raster | header + pixels |
| XML (`definition.xml`, `zones.xml`, outlines ...) | documents, values as text, `#text` / `#comment` children | the XML codec |
| meshes, loading screens, `tile.ted_binary`, anything that does not read | `files` | untouched |
