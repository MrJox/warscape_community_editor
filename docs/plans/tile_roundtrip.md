# Tile round trip: a game-neutral tile model with per-game readers and writers

Branch `tile-roundtrip`. Status: **Phase 1 done** (2026-10-22), waiting for review; Phase 2 steps 1 and 2 done (2026-10-22), step 3 next.

## Phase 1 result

- Crates `wce_tile_model` (the model) and `wce_tile_io` (the `TileFormat` trait, the registry, the XML codec of TEd's file family,
  `attila.ted_project`); TEd's `%.6f` formatter moved to `wce_formats::c_num` (`printf_f_msvc`, `trim_fraction`). Model and rules:
  `docs/formats/tile_model.md`; attribute kinds: `docs/formats/tile_ted.md`.
- The whole kit round-trips (`docs/baseline.md`, "Tile round trip"): every modelled file byte-identical, 222 Photoshop-style PNGs kept,
  3 broken PNGs carried along, 7 of 83.6 M values kept as text, 0 unknown attributes; edits, re-reads and the cross check against
  `TedScene` pass.
- Tests: `wce_tile_io` unit tests and `tests/ted_project.rs` (synthetic, always run), `tests/kit_samples.rs` (25 kit tiles with
  `WCE_ATTILA_KIT`), gates `tile_roundtrip` and `tile_roundtrip_all` (`--corpus`).
- Deviations from the plan: legacy `.markers` exports (19 folders) and `custom_normal.*` are carried along as files rather than modelled
  (their payload is not decoded; they are not TEd sources); "files per folder" is in the survey, not a separate doc.

## Goal

A tile is read into one in-memory model (`wce_tile_model`) and written out of it by format modules. One game's format = one reader + one
writer. Phase 1: Attila's TEd project (raw tile) both ways, byte for byte. Phase 2: Attila's exported binary tile (what BOB writes and the
packs ship) into the same model, and from the model back to binary.

## Decisions (2026-10-21)

- Phase 1 scope: TEd's files go into the model; BOB outputs and artist leftovers are carried along untouched.
- Scene nodes: a typed property bag per TEd class (schema per class), not one struct per class.
- The editor stays on `TedScene` in Phase 1; moving it onto the model is a separate step after the corpus passes.
- Images we cannot re-encode exactly: unchanged pixels write the original bytes (reported as "kept"); edited ones are re-encoded in TEd's style.

## What exists (survey, 2026-10-21)

- **Byte-exact per file, but no shared model.** `TedScene` (tile.ted on the lossless `XmlDoc`, edits are text splices), `TileEntry`
  (`TILES/*.xml`, lossless), `PngFile` (6,122 / 6,122 non-ancillary PNGs re-encoded exactly), `TiffFile` / `DdsFile` / `TgaFile` (header
  fields + raw payload), rpfm_lib for the binaries (`bmd`, `fastbin_lists`, `tile_database`). `TileProject` ties tile.ted, the height map
  and the entry together and saves only tile.ted and `height_map_0.png`. The editor (`wce_tile_editor::TileDocument`) edits through
  `TedScene`'s splices.
- **The kit tiles are on this machine now**: `~/.local/share/Steam/steamapps/common/Total War Attila/assembly_kit/raw_data/terrain/tiles`
  (53 GB, battle + campaign, 1,380 `tile.ted`). That also unblocks the corpus check still owed by the Rust port (PLAN.md, "Before the port is
  closed").
- **tile.ted on this kit**: versions 1 (1 file), 6 (9), 8 (102), 9 (89), 11 (10), 12 (15), 14 (46), 15 (168), 16 (940); no v17 (TDD only).
  All CRLF, no BOM, no XML declaration, 4-space indent, double quotes, self-closed leaves.
  **Attribute order is TEd's per-class hash order**: for 95 of 97 (version, tag) pairs every element agrees with one order; v16
  `BUILDING_INSTANCE` (8 orders, 1.83 M elements in the dominant one) and v16 `SIMPLE_MESH_INSTANCE` (2 orders) do not. So the order has to be
  carried by the model, not regenerated from a table.
- Numbers: TEd's formatter (`format_ted_number`, `%g` for points, `argb` with one decimal) reproduced every value of the Windows corpus;
  to be re-confirmed on this kit.
- Files of a variation folder (kit-wide counts): TEd/BOB inputs `tile.ted`, `height_map_N.png` (1,383), `blend_map.tif` (1,487),
  `ground_type_map.png` (782), `nogo.png` / `lf_influence_map.png` (1,178), `river_splineN.xml` (255), `*.markers` (133), `custom_normal.*`;
  BOB outputs `final_{heights,alpha,lf_influence}.dds`, `protection_map.dds`, `tile_normal.tga`; and artist leftovers (`raw/`, World Machine
  `*_output.png`, `wm_in.tif`, `.tmd`, `.CS2`, `.max`, ...).

## Design

### Crate layout

- **`wce_tile_model`** (new, pure data, no I/O): the model below, no knowledge of any game's files.
- **`wce_tile_io`** (new): the `TileReader` / `TileWriter` traits, a registry keyed by format id (`attila.ted_project`, later
  `attila.binary`, `rome2.ted_project`, ...), and one module per format. Depends on `wce_formats` and rpfm_lib for the file codecs.
- `wce_tile` keeps `TedScene` / `TileProject` unchanged in Phase 1 (the editor keeps working); migrating the editor onto the model is a
  later, separate step (see questions).

```rust
pub trait TileReader { fn format(&self) -> FormatId; fn probe(&self, src: &TileSource) -> bool; fn read(&self, src: &TileSource) -> Result<Tile>; }
pub trait TileWriter { fn format(&self) -> FormatId; fn write(&self, tile: &Tile, dst: &mut dyn TileSink) -> Result<WriteReport>; }
```

`TileSource` / `TileSink` are file sets (a folder on disk, an in-memory map, later a pack through `wce_vfs`), so the tests round-trip in
memory and the corpus gate never writes into the kit.

### The model

```text
Tile
  entry: TileEntryData            the TILES/*.xml entry: location, set, mask, size, colours, texture groups, variations, link targets, links
  variation: Option<usize>        which variation this is
  scene: Scene                    modules (seeds), node tree, camera
  maps: TileMaps                  height (u16 grid + border/ppc), blend (16 x u16), ground_type (RGBA8), nogo, lf_influence, custom normal
  rivers: Vec<RiverSpline>
  derived: DerivedMaps            BOB outputs found next to the sources (final_*.dds, protection_map, tile_normal): decoded, flagged derived
  extra_files: Vec<OpaqueFile>    every other file of the folder, by relative path, bytes untouched
  fidelity: Fidelity              per-format "trivia" a writer of the same format uses to give the same bytes; ignored by other formats
```

**Scene nodes are TEd's reflected classes, typed.** `Node { class: ClassId, props: PropBag, children: Vec<Node>, points: Option<Vec<Point>> }`,
where `PropBag` holds typed values (`F32`, `Vec3`, `Euler`, `Argb`, `Flags`, `Seasons`, `Enum`, `Int`, `Bool`, `Str`) per a schema per class
(grown out of `wce_tile::properties`, which already lists TEd's classes, kinds and ranges). Typed accessors (`position()`, `key()`, ...) sit on
top for the editor. Classes or attributes the schema does not know are kept as `Str` (nothing is dropped; the gate counts them so they get
typed).

**Fidelity (Attila TEd):** per document an interned table of attribute layouts (an element stores a `u16` layout id, so 1.8 M buildings
cost 2 bytes each), the indent / line-end style, and a "verbatim" text for the rare value whose text TEd's formatter would not give back
(expected empty; kept so a stray value never breaks the round trip). Editing a value through the model drops its verbatim text; a new node
takes the canonical layout of its (version, class), learned from the file or from a built-in table.

**Images:** decoded pixels in the model; the fidelity keeps what the encoder needs (PNG: IDAT chunk size, ancillary chunks; TIFF: the IFD
layout when it is not TEd's 242-byte template). Unchanged pixels whose original encoding we cannot reproduce (Photoshop / World Machine
files) are written from the original bytes, and the gate reports them as "kept", separately from "re-encoded exactly".

### Phase 1 steps

1. **Baseline on this kit.** Run `scene_check` and `tile_entry_check --all` on the local kit; record in `docs/baseline.md` (closes the
   pending corpus item for `wce_tile`). Survey script for: number formatting misses, unknown classes / attributes, image layouts per file
   kind, files per folder. Result: the list of what the Attila format must cover, in `docs/formats/tile_ted.md`.
2. **Model + traits** (`wce_tile_model`, `wce_tile_io` skeleton, in-memory `TileSource` / `TileSink`).
3. **tile.ted reader / writer** through the model (own streaming XML writer with TEd's style, not `XmlDoc` splices; parse may reuse
   `XmlDoc`'s tokenizer). Old versions (v1 - v15) included from the start.
4. **Entry, images, rivers, markers, extra files** into the model and back.
5. **Tests** (below). Each step lands with its tests.
6. Docs: `docs/formats/tile_model.md` (the model and the rules a new format must follow), `docs/formats/tile_ted.md` updated.

### Tests (what keeps the round trip from breaking later)

- **Unit tests (`cargo test`, no kit needed):** synthetic tile.ted per version (1, 8, 15, 16) and per class, generated in the test
  (like `tile_checks.rs`): read -> write equals input; read(write(m)) == m; edit a value -> write -> read gives the edit and changes only
  that span; a new node gets the canonical layout; property-based tests over random property bags (proptest) for the formatter and parser.
- **Kit sample tests (`cargo test`, skipped with a note when `WCE_ATTILA_KIT` is not set):** a fixed list of about 15 variations
  covering every tile.ted version, battle and campaign, a settlement (largest file), a river, a sea coast, a tile with markers, a Photoshop PNG
  and a non-template TIFF. Whole-folder round trip in memory, file by file.
- **Corpus gate (`wce_tile_io --example tile_roundtrip_check -- <kit>`, in `gates.sh --corpus`):** every variation folder of both kinds:
  byte-exact per file, model idempotence, the counts per file kind (re-encoded exactly / kept / failed), unknown classes and attributes,
  and a random edit-then-revert pass per tile (bytes equal again). Output recorded in `docs/baseline/`.
- **Cross check against the existing code:** for every tile, the model's values equal `TedScene`'s (positions, names, flags, layer paths),
  so both readers agree before the editor ever moves to the model.

**Done when:** every TEd input file of every kit variation round-trips byte for byte (images that cannot be re-encoded counted as kept),
unknown classes and attributes are 0 or listed and accepted, and all three test levels pass.

## Phase 2: the exported binary tile (plan, 2026-10-22, waiting for review)

### What an exported tile is (survey)

The game reads a tile from `terrain/tiles/<kind>/<set>/.../<variation>/` in its packs; BOB writes the same folder into the kit's
`working_data`. A shipped battle tile (`atlantic_mountains/custom_mont_blanc/aa`, 25 files) and BOB's own output (`~/wce_re/bobkit`):

| File(s) | What it holds | Codec today |
| --- | --- | --- |
| `mesh.rigid_model_v2`, `outfield_mesh.rigid_model_v2` | the terrain mesh, HF heights in the vertices (decimated) | rpfm_lib `rigidmodel`; round trip of terrain meshes not checked; 11 header bytes are uninitialised memory (BOB run vs run) |
| `hf_height_map.data` | `CHMF` v5 block table of heights | `chmf` round trip (171 / 171, `wce_formats` `roundtrip`); meaning of the blocks not decoded |
| `blend.dds`, `index.dds`, `normal.dds` | DXT5, 11 mips: blend weights, the layer index per pixel, normals | container exact (`DdsFile`), pixels decoded (`dds_decode`); the compressor (nvtt) not reproduced |
| `ground_types.dds` | uncompressed, TEd/BOB template header | exact |
| `*_procedural_bmd_data.bin` / `.xml`, `definition.xml`, `bmd_data_escalation*.bin` | props, particle emitters, buildings, capture locations, outlines, deployment ... | rpfm_lib `bmd` (860 / 860) |
| `*.tree_list.bin`, `default.grass_list.bin`, `civilian_*.bin` | generated trees and grass per climate, civilian zones | rpfm_lib `fastbin_lists`, exact |
| `*.tree_list`, `default.grass_list` (no `.bin`, not FASTBIN: `u32 2`, ...) | the same, an older format still shipped next to the new one | **not decoded** |
| `*.markers` | stage-A exports (particle emitters, props, decals, AI hints ...) | framing exact, payload partly decoded (point lights, particle emitters in the renderer) |
| `terrain_outlines.xml`, `tile.agf`, `building_list` (0xABCA stream) | outlines, the tile's agf, buildings | XML exact; agf and the 0xABCA stream not decoded |
| the `tile_database.bin` row | the entry | rpfm_lib `tile_database`, exact |

### What crosses between raw and binary

| Model part | Raw (TEd project) | Binary | Loss binary -> raw |
| --- | --- | --- | --- |
| heights | `height_map_0.png` + entry min / scale | `hf_height_map.data`, mesh vertices | quantisation; the border (overlap) may not be in the binary |
| blend weights | `blend_map.tif`, 16 layers | `blend.dds` + `index.dds` (DXT5) | DXT5 error, and only the layers the index keeps per pixel |
| ground types | `ground_type_map.png` | `ground_types.dds` | none expected |
| buildings, props, decals, VFX, lights | scene nodes in layers | building list, bmd props / emitters, markers | layers, names, euler (from a quaternion), TEd-only properties; the z mirror and the half-metre scale are known |
| logic (capture, deployment, AI hints, zones, outlines, civilians) | polygon nodes | bmd sections, markers, civilian bins | layer structure, TEd-only properties |
| trees and grass | seeds (`modules`) + hand-placed instances | generated lists per climate | the seeds; the generated trees cannot tell which were hand placed |
| camera, nogo, LF influence, exclusion polygons | in the raw files | not in the binary | everything |

### Steps

1. **Inventory**: every tile file of Attila's packs through the VFS (and BOB's working_data), counted per kind and per tile kind
   (battle / campaign / settlement); `docs/formats/tile_binary.md`. Decode what is missing: the older `.tree_list` / `.grass_list`,
   the `markers` payloads, `tile.agf`, the 0xABCA building list, the CHMF blocks' meaning.
2. **`attila.tile_binary` format, byte for byte** (the Phase 1 discipline): read every file of an exported tile into the model and write
   it back identically (compressed images kept while unchanged; the 11 uninitialised RMV2 bytes as trivia). Gate over every tile of the
   packs, kit sample tests, synthetic tests.
3. **Binary -> raw** (decompile): an exported tile written as a TEd project that TEd and the editor open; the losses above listed in a
   report per tile. Check: every value the binary carries is in the raw tile.
4. **Raw -> binary** (compile): BOB's outputs from the model, one output kind at a time, each against BOB's own output in the oracle kit
   (vegetation, the tile database and the battle map fill are ported already). Check: raw -> binary equals BOB; binary -> raw -> binary
   keeps every value.

### Step 4 plan (raw -> binary, 2026-10-22; user: A first, TEd stays parked)

The binary is written in three stages (docs/plans/tile.md): **A** TEd's export of the logic, **B** BOB's Tile processor, **C** BOB's
Vegetation processor. Each output kind is one compiler module (`wce_tile_io::attila::compile`), from the model of a TEd project to the
model of `attila.tile_binary`, so the binary writer of step 2 gives the bytes.

| Stage | Outputs | Oracle | Already there |
| --- | --- | --- | --- |
| A | `building_list` (+ far, sprawl), the `.markers` (props, emitters, lights, AI hints, separators), `definition.xml`, `terrain_outlines` / `non_terrain_outlines.xml`, `zones.xml`, `deployment_areas.xml`, `capture_location_list.xml`, `bmd_data_escalation<n>.bin`, `civilian_*.bin`, siege graph | **binary -> raw -> binary** over the 1,654 tile folders of the packs (the decompiler is the inverse of A; what it cannot carry is listed per tile); the kit's raw tiles where the packs are their export | the frames and conventions of step 3, `building_db` |
| B | `blend.dds`, `index.dds`, `normal.dds` (DXT5, ATI's compressor), `ground_types.dds`, `hf_height_map.data` (final heights + building stamps), `hf_water_map.data`, `mesh.rigid_model_v2` / `outfield_mesh` (decimation), `final_*.dds`, `protection_map.dds`, `tile_normal.tga` | fresh BOB under Wine (`bob_tile.sh`), byte for byte (the 11 uninitialised RMV2 bytes masked) | the CHMF encoder, the blend reduction and ground type rule (docs/re/terrain.md), DDS containers |
| C | tree / grass lists, `*_procedural_bmd_data` | fresh BOB (`bob_veg.sh`) | the vegetation port (exact on 11 tiles; the grass bug keeps it on pass-through until fixed) |
| - | the tile database row and `tile_database.bin` | BOB | `tdb_build` (0 bytes differ) |

**Exactness (user, 2026-10-22):** the shipped binaries were exported from TEd's in-memory floats, finer than tile.ted's `%.6f` text
(an exact 180 degree turn comes from no euler text), so binary -> raw -> binary tolerates the float steps the text cannot carry (a few
f32 ulps for positions, 1e-6 for quaternions) and is byte-exact for everything else. The compiler follows TEd's arithmetic (Ghidra:
`Ted::BMDExporter::export_`, `Ted::transform_to_game_tile_space`, the quaternion `FUN_00474030`).

Order: A, then B one output at a time (blend / index / ground types first: their rule is known, the compressor is the work), then the
heights and meshes; C is wired to the existing port behind a switch once its bug is fixed.

### Progress

- **Step 1 done**: inventory and decoding (docs/formats/tile_binary.md): every kind of file of the exported tiles has a codec; CHMF heights
  (decoder and the engine's encoder), markers, building lists and the older lists decoded.
- **Step 2 done**: `attila.tile_binary` (`wce_tile_io::attila::tile_binary`): every tile folder of the packs (1,756) through the model and
  back byte for byte, with the DXT5 textures and 545 TDD height maps kept as original bytes and the terrain meshes carried as files. Tests:
  `tests/tile_binary.rs` (synthetic), `tests/pack_samples.rs` (8 pack folders with `WCE_ATTILA_GAME`), gates `tile_binary_roundtrip` and
  `tile_binary_roundtrip_all` (`--corpus`).
- **Step 3 in progress** (`attila::decompile`, check `tile_decompile_check`): entries, buildings, decals, props, VFX, lights, trees, logic
  and capture locations decompiled.
  - Heights: the CHMF is written as `height_map_0.png` and is exact by construction. The kit's `height_map_0.png` is **not** a reference.
    It is BOB's input: BOB writes `final_heights.dds` from it (already different, e.g. 9 cm of relief exported as 0), then the CHMF is
    `final_heights` plus raised building stamps (fresh BOB, farm_western/1x1/aa: 4% of the samples, up to +1 m, all positive). Many pack
    tiles are also not the kit's version: 167 ship a flat CHMF (range ~1e-13) and a flat mesh where the kit tile has relief, and
    rolling_valley tiles ship ~0.18 x the kit's relief. The check reports the kit agreement for information only.
  - Buildings (1,893,562 paired): orientation and toggleable all agree (older tile.ted files omit `toggleable`, TEd's default false);
    destruction levels come from the building's `<key>_tech.cs2.parsed` (fewest destructs over its pieces, `destruction_levels`), 20
    differ.
  - Simple meshes: the record's y and z rows are negated; 208 agree, the other 939 differ by a constant yaw per building group (90, 180,
    275 degrees ...: the groups differ between the kit and the packs).
  - Decals: 1,660,832 of the paired agree. The rest (farm_eastern / farm_western, arbitrary yaws) and the 256k unpaired at another height
    are in the families whose kit is not the shipped version (farm_*, town_greek_plains, roads_*, sea_coast); there the binary y looks
    terrain-relative (kit y ~ ground + 2 y, off by 0.1 - 0.7 m) while everywhere else it is absolute (2 y). Open: whether the exporter
    writes some decals relative to the ground (needs TEd's exporter).
  - Deployment zones: the binary's are clipped to the playable area (a reported loss).
  - Capture locations: from the escalation BMDs and, for the normal layers, `capture_location_list.xml` (1,288 in 169 packs' files).
    A circle (radius, one point) is placed as the kit has it: the instance at the centre, the flag at `flag_position`. 999 of the kit's
    999 boundaries pair, the instances all but one; `flag_facing` within 2e-6 (the binary's vector has 6 digits).
  - Not in the binary: vegetation exclusion polygons (applied to the tree lists; 4 tiles ship a `vegetation_exclusion.xml` bounding box
    that matches no kit polygon) and the river splines (the river tiles ship `hf_water_map.data` only, the water surface; a spline could
    only be fitted to it).
  - Lite building zones: not in the binary, but a building of the main list is `lite` exactly when its position is inside a kit zone
    (eastern_city_b/small 25,720 / 25,720, nordic_city_b/medium 12,781 / 12,781). The decompiler rebuilds zones from the flags (a
    quadtree of the lite buildings, touching rectangles merged): 4,722 zones on 102 tiles where the kit has 140, no building changing
    its flag. Fewer, outline-shaped zones would be nicer for an artist; the export is the same.
    Later: what the game does with a `lite` building is unknown (a cheaper mode for the outer city, likely). Find where the flag is
    consumed (the game's or BOB's code, static RE) before improving the zones' shape.
  - Tiles without a database row (18: mods' assembly_kit battles, gaul / river_ford / nile tiles): an entry made from the binary (size
    from the CHMF). Every tile folder of the packs decompiles. Mapping: docs/formats/tile_decompile.md.
  - Blend map from `blend.dds` / `index.dds` (BOB's reduction inverted; group weights 1.06 / 255 off the kit's over 782 tiles, the DXT5
    error), ground type map as the painted overrides BOB's rule needs (`ground_type_from_texture_group`, `ground_type_to_colour`, found
    in ToolDataBuilderDll): the rebuilt maps give the binary's ground types back on 99.99 % of the pixels.
  - Segmented splines (4 kit tiles): a road tool baked into the heights, nothing of it in the binary.
  - **Step 3 done** (2026-10-22): every tile folder of the packs decompiles and reads back (gates `tile_decompile`,
    `tile_decompile_all`); docs/formats/tile_decompile.md. For step 4: BOB writes its DDS with ATI's compressor
    (`TOOLDATABUILDER::write_dds_ATI_compressor`).
- **Step 4 in progress** (docs/formats/tile_compile.md): stage A's building lists compile from the decompiled projects of every tile
  folder of the packs within the text's precision (keys and flags identical over 4,063,006 buildings); the gates compare them. TEd's
  export read in Ghidra (`Ted::BMDExporter::export_`): destruction levels are the most destructs over a building's pieces, the starting
  damage level / max or the building's `destruct_thresholds`. Then the markers (props, emitters with their older layouts, lights, AI
  hints, separators, the siege graph; group order = TEd's MARKER_TABLE), definition.xml (the climate carried) and the go / no-go outline
  files, each within the text's precision on chosen tiles (western_city_b/small, a campaign river, dwarven_city_b, farms); zones,
  deployment, capture locations; the escalation BMDs with their XML twins and civilian_deployment.bin (TEd's garbage bytes counted
  apart; the BMDs' prop / emitter group order through a reconstructed insertion order). Tested on western_city_b/small, dwarven_city_b,
  eastern_port_constantinople, dunedain_city_a/medium, osgiliath, nordic_port_a/minor, farm_eastern, an encampment.
  - Capture building links: the buildings' `capture_location` names (user's hint; the kit's tiles give every binary's links).
  - Deployment: clipped to the tile less 32.5 m (Sutherland-Hodgman, TEd's side order), the orientation the facing + 1.570796; capture
    circles at their polygon's position. `WCE_COMPILE_KIT=1` compiles the kit's raw tiles: deployment and capture files as the packs'.
  - `vegetation_exclusion.xml` (4 encampments) is carried: TEd's box comes from the editor's loaded models (not reproduced).
  - Stage B owns `civilian_shelter.bin` (capacities not in tile.ted).
  - Stage A done but the carried `vegetation_exclusion.xml`.
- **Stage B in progress**: blend, index (ATI's DXT5 compressor ported, 2008 corrections from the binary) and ground types byte for byte
  against fresh BOB on 3 tiles (2 blocks of 133k off: BOB's unset stack). `normal.dds` (Sobel / 9 in f32) byte for byte too.
  Final heights (TEd's influence map) byte for byte against BOB's `final_alpha.dds` / `final_heights.dds`; the CHMF writer ported (its
  second float is the max, not a range: the decompiler's scale is now max - min). The CHMF is the decimated terrain mesh rasterised
  (plus building stamps), so the mesh comes first.
  Terrain mesh surface (grid, flags with the protection map, `TRIANGLE_MERGER`) exact on 2 tiles; rasterised, the CHMF is byte for
  byte on terrain_rolling_hills_med/2x2/ae (no stamps) and, with the buildings' height meshes stamped (a CS2 reader for the kit's
  raw models), on farm_western/1x1/aa.
  `mesh.rigid_model_v2` (fringe, merger, writer) byte for byte on both tiles but BOB's uninitialised shader name tail.
  Next: `outfield_mesh`, the water map. Still to wire: the CHMF into
  `textures()` (needs the kit's db and art for the stamps, and `_settings.xml`'s angle). The kit lacks most buildings' raw height-map meshes
  (`raw_data/art/.../*.cs2`), so BOB cannot finish tiles that use them: oracle tiles are those without such buildings.
  - User (2026-10-23): no corpus runs until the plan is finished; check each piece on a few tiles (`--filter`).
  - For later (the user can fetch them): TDD's raw tiles (tile.ted + maps, `_tile_database`), at least dwarven_city_b/medium and
    middle_men_city_hornburg/medium + small for the 6 non-unit building quaternions; TDD's working_data of a few tiles for stage B if its
    export setup differs from vanilla.

### Decisions (2026-10-22)

- **Binary -> raw first** (step 3 before step 4); raw -> binary follows.
- **Decompiled trees** are written as explicit `VEGETATION_INSTANCE`s in a layer of their own ("Decompiled vegetation"; the seeds are not in
  the binary, so regenerating would move them); grass lists are carried along as files.
- **Raw -> binary is reimplemented exactly** (terrain mesh decimation, the DXT5 compressor included), each output proven byte-identical
  against BOB in the oracle kit, as the vegetation port was.
- **Corpus**: every tile of Attila's packs (through the VFS), BOB's working_data (the oracle kit), and mod packs (TDD) when they are on this
  machine.
