# Terrain tool: Terry's replacement (plan, 2026-11-02)

The Terrain tool edits a map: the battle map a game's battles are cut from (the campaign's `main_attila_map`, a mod's `rom_third_age_map`, a
historical battle, a settlement map), its low-frequency (LF) ground, the tiles painted on it and the metadata Terry keeps in the map's
`.terry` project. It replaces the Assembly Kit's Terry: Attila's crashes at launch, Rome 2's kit has none, Thrones' works but slowly (user,
2026-11-02). Attila first; every game fact behind the game profile. What Terry does: docs/re/terry.md. It takes over the map-level scope of
docs/plans/tile.md (items 1, 3, 4, 5) and the map context's open items that are about the map rather than the open tile
(docs/plans/map_context_todo.md, "Map-level terrain", "Large map contexts: performance").

Status: **Phases 0 and 1 done (2026-11-02); the user's decisions are in "Decisions" at the end.** Next: Phase 2 (the viewer).

## What Terry does and what the tool does with it

| Terry (docs/re/terry.md) | Terrain tool | Phase |
|---|---|---|
| Open / save a `.terry` project, the map's images next to it | a map document: the map folder (kit layout, projects first: `wce_tile::sources` order), every file read and written losslessly | 1 |
| 3D view of the map (engine scene), the tile window (real tiles only inside it), the water plane, the virtual-texture toggle | the map context's streamer and the game's shaders over the whole map; the near ring is the tile window; the sea as today | 2 |
| 2D tile map view | a Map panel: the cell layers as an image (one texel a cell), pan / zoom, the same tools as the 3D view; the overview map stays | 2 |
| Paint tile sets (layers, fill), place / move / rotate / remove explicit tiles | cell layer painting (tile set, tile / variation colours, climate, culture, snow per season), explicit tiles with the gizmo and the fit test (`test_final_tile_position`) | 4 |
| Paint LF heights and sea LF (falloff / image / disk / square brushes, size, aspect, rotation; one add / subtract operation; undo optional) | the tile tool's brushes and operations (raise, lower, smooth, flatten, level, ramp, noise, image brushes, falloff curves) on a paged LF field, always with undo; aspect ratio and rotation added | 3 |
| Apply (per-tile) LF patches | the custom-mesh tiles' `lf_mask.tif` into the LF (RE: what `apply_per_tile_lf_patches` does, docs/re/battle_maps.md) | 3 |
| Entities: battle catchment areas, deployment zones and regions, rivers, prop meshes, VFX, environment volumes, season masks; spot / circle / rectangle / polyline / spline creation; gizmo, clone, multi-selection, scene tree, properties, undo view | the `.terry` scene as a lossless ECS model; the editor's outliner, inspector (multi-selection), gizmo and creation modes; overlays draped on the ground | 5 |
| Generate battle catchment areas (`bca_generation_spec.xml`) | the generator ported, Thrones' Terry as the oracle | 6 |
| BOB_Terrain: `Terry file`, `Tilemap`, `Heights & Normals` | reimplemented, BOB as the oracle (docs/plans/tile.md decisions); pack output; Launch in game at a chosen location | 7 |
| Campaign projects (`terrain/campaigns/<map>`: props, VFX, season masks, the layout map) | a second document kind on the same model | 8 |
| Thrones (same plugin), Rome 2 (no Terry) and later games | profiles and per-game readers / writers | 9 |

## What exists to build on

| Piece | Where | Use |
|---|---|---|
| Kit map fill, exact against BOB on 69 maps | `wce_tile::battle_map::fill` | the placements after a tile map edit |
| `tile_map.index` / `.tiles` (v6, v7, v9), `terrain.tile_list` v11, `locations.blm` v7 / v8 readers, writers for the first two | `wce_tile::battle_map::{tile_map, tile_list, locations}` | load and build; `locations.blm` needs a writer |
| The battle tile database (sets, tiles, variations, links, colours, climates) | `wce_tile::battle_map::tile_db`, `wce_tile_editor::climate_db` | palettes, the fit test, validation |
| The map context: chunks of 8 x 8 cells at three levels, four workers, tile templates opened once and cached on disk (lz4), uploads off the render thread, removals by id range | `wce_tile_editor::tile_document::context::{stream, template, cache}` | the 3D view, as it is; its frame generalised (below) |
| The LF in the game's vertex shader (a 2048 window around the camera, one uv offset), LF normals on a torus, render origin, the sea, custom-mesh tiles, climates, snow regions | `wce_render::renderer::terrain`, `context::Frame`, `LfMap` | the ground; the window and the normals become updatable in parts |
| Brushes: falloff, falloff curves (LUT), image brushes; raise, noise, smooth, flatten, level, ramp; undo by touched rectangle (heights) or 64 x 64 blocks (`PixelLayer`) | `wce_tile::{height_field, falloff, brush_image, pixel_layer}`, `wce_tile_tool::{falloff_editor, image_brush, veg_brush}` | LF painting, cell painting |
| Overview map (the LF shaded, the camera, click to go) | `wce_tile_tool::minimap_panel`, the Map context's overview | the Map panel's start |
| Documents, commands, history, outliner, inspector, gizmo, shortcuts, scripts (`--script`, `--shot`) | `wce_editor_core`, `wce_editor` | the tool's frame; gates and captures |
| Launch in game (pack, loose, Proton) | `wce_tile_tool::{launch, launch_prompt}` | a battle at a location of the edited map |
| PNG (kit-exact encoder), TIFF, DDS templates, CHMF | `wce_formats` | the map's images and BOB's outputs |

## Architecture

### Crates (the tile tool's split)

- `wce_terrain` (model, no GPU, no UI): the game-neutral map model (`MapModel`: cell layers, LF fields, explicit tiles, the `.terry` scene,
  the map's settings), the edits and their undo records, the fill driver, the BCA generator, the builders of the game's files. Map-level
  formats move here from `wce_tile::battle_map` (`tile_map`, `tile_list`, `locations`, `context`; `wce_tile` re-exports them so the tile
  tool does not change).
- `wce_terrain_io`: per-game readers and writers (`attila`, later `rome2`, `thrones`), as `wce_tile_io`.
- The streamer stays in `wce_tile_editor` (its templates open whole `TileDocument`s, so a crate of its own would depend on the tile
  editor and the tile editor on it): `tile_document::context` becomes a public `map_stream` module whose API takes a frame and no open
  tile; the tile document is one client, the map document the other.
- `wce_terrain_editor` (depends on `wce_tile_editor`): the `MapDocument` (the model, the streamer, the scene layer, picking, the GPU-side
  dirty regions).
- `wce_terrain_tool`: the `Tool` (the placeholder `terrain` becomes it, behind a `terrain` feature of `wce_app`), panels, chips, commands.

### The frame

`context::Frame` is "the map seen from the open tile": the open tile's frame, the LF resampled into it (`lf_map`), the open tile's median
LF as the zero. The Terrain tool has no open tile: its frame is the map's own (the game's: origin at the map's centre, `tile_list`'s
`origin_offset`; x east, z north; world units as the `.terry` positions), the LF's lattice is the map's pixels (no resampling, no copy),
the sea at 0. Step one of Phase 2 makes the frame a parameter (`Frame::for_tile` = today's, `Frame::for_map`) with the tile tool's captures
unchanged (the gate below).

### Game seam

The profile gets `MapFacts`: cell size (1/8 of a settlement tile), LF pixels a cell per map kind, `lf_height` 3000 / `vertical_offset` 20,
the sea LF's zero, the file names and versions (`tile_map` 9 / 6, `blm` 8 / 7, `tile_list` 11), whether the kit has Terry projects, the
BCA spec's path. The model never names a game; `wce_terrain_io::<game>` maps files to it.

## Data structures, memory and frame time

Sizes to plan for (Attila): the campaign battle map is 2032 x 1437 cells (2.92 M), LF 8128 x 5748 (46.7 M samples), sea LF 4064 x 2874,
268 944 placements, 53 782 `.terry` entities; the campaign map's LF is 16256 x 11528 (187 M samples, 375 MB).

- **LF fields: paged u16.** Pages of 64 x 64 samples (8 KB, L1-sized), stored page after page (a brush's rows stay in one page, a
  dab touches a few pages). The file's 16-bit values are the truth (byte-identical round trip; no float copy of the map). A stroke keeps a
  float shadow of the pages it touches (its start values plus the brush's sum), written back to u16 at every dab, so many small dabs do not
  lose to rounding; the shadow goes at the stroke's end. Large brushes run in parallel over pages (the compute pool, not the streamer's).
  Memory: 93 MB for the battle map's LF, 375 MB for the campaign map's (Phase 8: only the pages in use resident, the rest memory-mapped from
  a page file in the cache).
- **Per-page min / max and a pyramid** over them: picking (a ray against the LF walks the pyramid, not samples), the blend boxes' y range
  (today `LfMap::range` walks samples), the overview's shading, and which pages a brush can reach.
- **Undo by page, copy on write** (as `PixelLayer`'s 64 x 64 blocks; `HeightField::begin_edit` copies the whole field, 186 MB for this
  map: not reused). A stroke of radius 256 px keeps about 80 pages (650 KB). One budget for the document's history (default 256 MB; the
  oldest steps go first, the user is told).
- **Cell layers: dense, row-major arrays per layer**: tile set / tile / variation as a u16 palette index (the palette holds every colour
  the PNG has, unknown ones too: a PNG written from an unedited layer is the original bytes, an edited one goes through the kit-exact
  encoder), climate and culture u8, snow one bit a season. About 13 MB for the campaign battle map. Dirty state per 8 x 8 cell chunk
  (the streamer's `CHUNK`), so a dirty chunk is a chunk to rebuild.
- **Placements**: the fill's result as a flat array of 24-byte records (location, variation, origin, size, rotation, flags, climate, LF
  min / max) and a u32 cell -> placement grid (11.7 MB), instead of `tile_map.tiles`' 2 x 20 bytes a cell (117 MB).
- **The `.terry` scene**: components in arrays by type (transforms, rectangles, polylines ...), entities as index ranges into them, a
  chunk grid for picking and drawing; the XML kept as a token stream with spans (`wce_formats::xml_doc`), so unknown attributes, order and
  whitespace come back and an edit rewrites only its attribute.
- **GPU updates in parts**: the LF window becomes a `DEFAULT` texture updated by `UpdateSubresource` over the dirty rectangles (it is
  `IMMUTABLE` and remade when the camera moves today); the LF normal map the same (its unrolled source and the rolled copy); the cell overlay
  texture (one texel a cell, 11.7 MB RGBA); a budget of bytes uploaded a frame (4 MB), the rest the next frames.
- **What an LF edit costs elsewhere**: the neighbours' objects are lifted by the LF (`Frame::place`), so a stroke marks the chunks under it;
  their entities are moved again at the stroke's end (not every dab), on the workers. The blend boxes take a margin during a stroke. The sea
  bed (5 to 8 ms to make) is made again at the stroke's end.

Budgets (to check with a perf gate on `main_attila_map`, RX 7900 XTX and the integrated GPU): 16 ms a frame p95 at the battle camera's view
distance while painting; a dab of radius 256 px under 2 ms CPU; the map open in under 10 s from the cache (Phase 0 measures what it is
now); resident memory besides the tile templates' 1.5 GB budget under 1 GB for the campaign battle map.

## Rendering

- The game's look: the map context's passes, unchanged in what they draw. The tile window is the near ring around the focus; the mid and
  far rings as today; the view distance slider up to the whole map.
- Overlays (editor views, not the game's look, so editor shaders): the cell layers draped on the ground by a screen-space pass in
  `renderer/editor.rs` (the depth to world xz, the cell's texel, alpha); the brush outline on the LF; BCA rectangles, deployment polylines,
  rivers, volumes drawn as lines on the ground; the explicit tiles' footprints. Off in the game view (`view_mode game`).
- The Map panel (2D): the same cell texture with the LF shaded under it, drawn by ImGui; painting there edits the same layers.

## Editing

- **LF**: the tile tool's operations on the paged field, through one `Brush` (aspect ratio and rotation added); the sea LF as a second
  target; the LF bedding values (`lf_bedding`, docs/plans/tile.md item 4) as presets of flatten; import / export of a region as 16-bit PNG
  or TIFF; the custom-mesh LF patches.
- **Cell layers**: paint, fill (flood by colour), rectangle, picker; a layer's palette from the tile database (sets, tiles, variations, climates
  by their colours); the brush in cells. **Explicit tiles**: placed with the gizmo on the cell grid, quarter turns, the fit test shown live
  (green / red), written to `explicit_tiles.txt`.
- **After a tile map edit**: the fill runs again on a worker and the chunks whose placements changed are rebuilt (the streamer's `Set` /
  `Remove`); the others stay. The first version matches BOB (decision 3): BOB's fill draws from one random sequence over the whole map in row order, so one painted cell
  changes the variations and turns of every placement after it.
- **Entities**: the `.terry` scene in the outliner (by type, then the hierarchy), the inspector on the components' fields, the gizmo on
  transforms (snapped to the cell grid for `ECTileAlignment`), creation modes as Terry's; positions lifted to the LF (`ECLandscapeAbove`).
- **Validation** (a panel, also a command for scripts): cells whose colour is in no database, tiles that do not fit, missing tile folders,
  painted sizes against `tile_upgrades.xml` (`tilemap_audit`, docs/plans/tile.md item 3), BCAs whose infield holds restricted tiles.

## Building and checking

- Raw files: written only when edited, byte-identical otherwise; minimal edits (gates as the tile files').
- BOB_Terrain's outputs from our builders: `tile_map.index` / `.tiles`, `terrain.tile_list` (exist), `lf_height_map.dds` / `.data`,
  `lf_sea_height_map.dds`, the normal chunks, `locations.blm` and its masks, the deployment files. BOB on a scratch kit is the oracle
  (`bob_run.py scratch`, never the user's kit; `Tilemap` takes 7 minutes on the campaign map, so its oracle files are kept in the cache).
- Gates (Cargo examples, in `scripts/gates.sh`): `terry_roundtrip_check` (every `.terry` of the kits), `map_files_roundtrip_check` (the maps'
  PNG / TIFF / DDS / TXT / XML), `map_edit_minimal_check`, `lf_build_check` and `locations_build_check` (against BOB's outputs),
  `terrain_perf_check` (a scripted walk and strokes on `main_attila_map`: frame time p95 and memory), and the tile tool's map context
  captures before / after the streamer moves (`scripts/shots.sh`).
- Pack output and Launch in game: the edited map's built files in a pack (or loose), the battle at a BCA the user picks.

## Phase 0 results (2026-11-02)

- **The oracle**: BOB_Terrain runs headless in a scratch kit (`I:\wce_scratch\attila\assembly_kit`; its needs in docs/re/bob.md):
  `main_attila_map` in 14 minutes, a settlement-sized map in under a second.
- **Already exact against fresh BOB output**: the fill's `terrain.tile_list` and the final `tile_map.tiles` / `.index` (the campaign
  battle map byte for byte), `lf_height_map.dds` and `lf_sea_height_map.dds` (conversion and mip rules found), `lf_height_map.data`
  (the CHMF encoder, one rule left: the region tag). To do: `locations.blm`'s rounding, its masks, the deployment files' writer.
- **The fill on the campaign battle map: 74 s** (one thread, with loading and writing 117 MB; BOB: 13 minutes). Phase 4's "fill again
  after an edit" cannot be a full fill per stroke: profile it, and run it on a worker after the stroke with the old placements shown
  until it lands.
- **The map context on the campaign map today** (`western_city_rome/small` at its placement, default rings 8 / 24 / 64, RX 7900 XTX,
  vanilla packs): cold 117 tiles opened in 20.6 s (first ring 7.1 s), 206 MB of cache written; warm 1.2 s to the first ring; 877 MB of tile
  templates, 297 093 draw items; **19.8 ms a frame (p95 21.6), CPU-bound**; peak working set about 3.0 GB. The viewer starts above the
  16 ms budget: Phase 2 starts with the culling by area (docs/plans/map_context_todo.md, "Large map contexts").

## Phases

0. **Survey and measurements** (no tool code). Terry's outputs: run BOB_Terrain's three actions on a scratch kit for a settlement map and
   `main_attila_map`, list what each writes; decode the normal chunks, `locations.blm.mask0` / `.mask1`, the deployment files, the `.terry`
   -> `locations.blm` mapping (docs/re/terry.md, "Open"). Measure the port's fill on the campaign map (BOB: 5 minutes) and opening the map
   context there (time, memory). Thrones' Terry as a reference: screenshots and a few black-box runs (the user sends them when a phase needs them).
   **Done 2026-11-02** (results below and in docs/re/terry.md, "BOB_Terrain's outputs"); left for the phases that need them: the masks'
   bits and the blm rounding (Phase 6), the normal chunks' source (Phase 3), Thrones' references.
1. **Model and files**: `wce_terrain`, `wce_terrain_io::attila`; every map file of the kits and the packs read and written; the round-trip
   gates; the profile's `MapFacts`. Done when every kit map round-trips byte for byte.
   **Done 2026-11-02** (docs/formats/map_files.md): 129 kit maps of Attila and Thrones, 862 files, through the model and back with no
   failure; the built files of Attila's and Thrones' packs and BOB's outputs written back byte for byte. Left: Rome 2's tile list (versions
   8, 9) and tile map (version 6) writers (Phase 9). The built files' readers stay in `wce_tile::battle_map` (the tile tool uses them)
   instead of moving into `wce_terrain`.
2. **Viewer**: the streamer made a public module with the frame as a parameter (the tile tool's captures unchanged); `MapDocument`; the
   Terrain tool opens a map (kit, project, or packs read-only), the whole map in 3D, the Map panel, the overlays, the LF picking pyramid.
   Done when `main_attila_map` walks within the budgets.
  The streamer's frame is the only part of this phase that touches the tile tool.
3. **LF editing**: the paged field, the brushes, the page undo, the GPU updates in parts, the normals, the sea LF, the LF patches, saving
   the TIFFs, `Heights & Normals` against BOB.
4. **Tile map editing**: the cell layers, explicit tiles, the fill on a worker and the partial rebuild, validation; `Tilemap` against BOB.
5. **The `.terry` scene**: the ECS model, outliner, inspector, gizmo, creation modes, overlays; `Terry file` against BOB.
6. **BCA generation**: the spec, the generator, its errors file; Thrones' Terry's output as the oracle.
7. **Export**: pack output, Launch in game at a location, the minimal-edit gates over the whole build.
8. **Campaign maps** (in scope, after the battle maps): `terrain/campaigns/<map>`: props, VFX, environment volumes, season masks, the layout map;
   the 375 MB LF paged from disk.
9. **Other games**: Thrones (same plugin and formats: an oracle that runs), Rome 2 (`tile_map` v6, `blm` v7, maps only in the packs), later
   games when their profiles come (their Terry is another tool: out of this seam's first pass).

## Risks

- The fill's global random sequence: matched for now (decision 3); a stable alternative is a later step, measured against this baseline.
- The normal chunks and deployment files are undecoded; until they are, the export may have to keep the game's copies or run BOB.
- The kit's `main_attila_map/tile_map.tiles` is an old file and terrain.pack's tiles are not BOB's fill of the kit's map (72 % of origins
  agree, docs/re/battle_maps.md): the kit's is the base first, terrain.pack's after (decision 5).
- Moving the streamer out of the tile document touches the tile tool's map context; the capture gate guards it.
- Memory with the campaign map's LF (375 MB) on top of the tile templates.

## Decisions (user, 2026-11-02)

1. **Scope**: battle maps and campaign maps both.
2. **Order**: battle maps first (Phases 1 to 7), then the campaign maps (Phase 8).
3. **The fill**: the first version matches BOB exactly, reshuffle included (a painted cell changes the variations and turns after it).
   Faster or stable ways come later, measured against this baseline.
4. **Building**: our own versions of BOB_Terrain's outputs; BOB is the oracle only.
5. **The campaign battle map's base**: the kit's raw images, filled by the port. terrain.pack's `tile_map.tiles` as a base comes after
   (a later item of Phase 9 or its own step).
6. **Thrones references**: the user sends screenshots and runs when a phase needs them; ask then, with the list of what is needed.
7. **BCA generation**: in the first version (Phase 6 is part of the battle map release, not optional).
