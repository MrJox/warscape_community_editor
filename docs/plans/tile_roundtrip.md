# Tile round trip: a game-neutral tile model with per-game readers and writers

Branch `tile-roundtrip`. Status: **Phase 1 done** (2026-10-22), waiting for review; Phase 2 not started.

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

## Phase 2 (outline; detailed plan after Phase 1)

Exported binary tile = BOB's working_data output for a variation (`mesh.rigid_model_v*`, `outfield_mesh`, `hf_height_map.data`,
`blend/index/normal.dds`, `ground_types.dds`, `*_procedural_bmd_data.bin`, `bmd_data_escalation*.bin`, `*.tree_list.bin`,
`default.grass_list.bin`, `civilian_*.bin`, `building_list`, `*.markers`, `terrain_outlines.xml`, `tile_database.bin` rows).
1. Binary reader into the model (layers, names, procedural vegetation and the camera do not exist in the binary: a reconstruction step makes
   one layer per export group, and the exactness target is the data, not the TEd text).
2. Binary writer from the model = the BOB pipeline (vegetation, tile database and the battle map fill are already ported; the terrain mesh,
   `hf_height_map.data` and the DXT5 maps are not).
3. Tests: binary -> model -> binary byte for byte; raw -> binary against BOB's output (the oracle kit at `~/wce_re/bobkit`);
   raw -> binary -> raw keeps every value the binary carries.
