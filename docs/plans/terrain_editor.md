# Terrain tool: Terry's replacement (plan, 2026-11-02)

The Terrain tool edits a map: the battle map a game's battles are cut from (the campaign's `main_attila_map`, a mod's `rom_third_age_map`, a
historical battle, a settlement map), its low-frequency (LF) ground, the tiles painted on it and the metadata Terry keeps in the map's
`.terry` project. It replaces the Assembly Kit's Terry: Attila's crashes at launch, Rome 2's kit has none, Thrones' works but slowly (user,
2026-11-02). Attila first; every game fact behind the game profile. What Terry does: docs/re/terry.md. It takes over the map-level scope of
docs/plans/tile.md (items 1, 3, 4, 5) and the map context's open items that are about the map rather than the open tile
(docs/plans/map_context_todo.md, "Map-level terrain", "Large map contexts: performance").

Status: **Phases 0, 1 and 2 done (2026-11-02); the user's decisions are in "Decisions" at the end.** Next: Phase 3 (LF editing).

## What Terry does and what the tool does with it

| Terry (docs/re/terry.md) | Terrain tool | Phase |
|---|---|---|
| Open / save a `.terry` project, the map's images next to it | a map document: the map folder (kit layout, projects first: `wce_tile::sources` order), every file read and written losslessly | 1 |
| 3D view of the map (engine scene), the tile window (real tiles only inside it), the water plane, the virtual-texture toggle | the map context's streamer and the game's shaders over the whole map; the near ring is the tile window; the sea as today | 2 |
| 2D tile map view | a Map panel: the cell layers as an image (one texel a cell), pan / zoom, the same tools as the 3D view; the overview map stays | 2 |
| Paint tile sets (layers, fill), place / move / rotate / remove explicit tiles | cell layer painting (tile set, tile / variation colours, climate, culture, snow per season), explicit tiles with the gizmo and the fit test (`test_final_tile_position`) | 4 |
| Paint LF heights and sea LF (falloff / image / disk / square brushes, size, aspect, rotation; one add / subtract operation; undo optional) | the tile tool's brushes and operations (raise, lower, smooth, flatten, level, ramp, noise, image brushes, falloff curves) on a paged LF field, always with undo; aspect ratio and rotation added | 3 |
| Apply (per-tile) LF patches | the custom-mesh and sea tiles' `lf_mask.tif` into the LF (docs/re/terry.md, "Per-tile LF patches") | 3 |
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
   **Progress 2026-11-02.** Done: `wce_map_view` (the map context's scene driver and map loading moved out of the Tile panel: `MapView`,
   `Rings`, `maps`; the Tile tool's map context captures unchanged, 92 to 107 dB against the baseline as two baseline runs are);
   `ContextRequest` with no cells is the map's own frame (origin at the map's centre, heights from the sea); `wce_terrain_editor::
   MapDocument`; `wce_terrain_tool` (the `terrain` feature of `wce_app`, replacing the placeholder): a Terrain panel with the map list
   (the packs' maps, the kits'), Open (read on a worker; a kit map also into the model), the rings, the view distance, the overview map;
   the view follows the active map document (each map keeps its camera); scripts `terrain_open`, `terrain_settle`, `terrain_view`. The
   campaign battle map from the packs at its centre: 15.2 ms a frame (p95 17.4), CPU-bound, 100 251 draw items (RX 7900 XTX).
   DEVIATION (for now): the map is lit by the first day, dry weather of the climate at its centre (`weather/<climate>/`), not the Tile
   tool's weather choice. **Phase 2 done (2026-11-02).** The fog: an editor view option, "Fog (editor view)" in the camera chip
   (`view_fog`), the user's permission for the deviation (docs/editor.md, view modes).
   **Overlays (2026-11-02).** The Map panel's layer "on the ground" (with an opacity): `RenderScene::set_ground_overlay` (a picture, a
   frame -> texel map, an id, the map view's layer: drawn while that layer is visible; not a structural change) and the renderer's
   `pass_ground_overlay`, an editor pass (`EDITOR_HLSL drape_ps`, never in the game view): each pixel's world point from the scene's
   depth (clip z / w) and the inverse view-projection, its texel blended over. The campaign map's placements line up with its coasts and
   rivers. The Tile tool's captures are unchanged (one capture run in four showed scattered single-pixel speckle up to 38 levels on all
   its shots, the next run did not: run-to-run noise to keep in mind when comparing).
   **Map panel (2026-11-02).** The active map from above, a texel a cell: the LF (shaded), the placements (each in its tile set's colour;
   what a map of the packs has instead of a tile map image) and a kit map's cell layers from the model; wheel zoom, middle-button pan, a
   click moves the camera there; the camera and its view direction; the hovered cell's colour named from the map's tile database (tile
   set, tile, variation, climate) and the placement over it. The Terrain tool's own default layout (Map and Outliner left, Terrain and
   Inspector right); `terrain_map_layer` for scripts. `MapDocument` keeps the tile database it was filled with.
   **LF picking (2026-11-02).** `wce_terrain::pyramid::HeightPyramid`: min / max over 8 x 8-cell leaf blocks, merged 2 x 2 to one root;
   a ray descends nearest block first and meets each cell's bilinear surface exactly (a quadratic along the ray; the field is solid below
   its surface, so a ray coming in through a side meets it there); `update` measures again only the blocks an edit touched. Tests: 240
   rays in every direction against a fine march. The campaign battle map's LF (8 128 x 5 748, `lf_pyramid_bench`): made in 8 ms (about
   4 MB), 1.6 us a ray, a 256 x 256 edit's update 84 us. The Terrain tool keeps one for the shown map (from its view's LF) and shows the
   LF under the cursor (cell, metres, raw sample); `terrain_pick <x> <y>` for scripts. At Rome: cell (619.24, 676.25), 59.88 m, the
   median the map context reported under its tile.
   **Frame time (2026-11-02).** CPU-bound (half the render scale changed nothing). Where it went and what was done, the Terrain tool on
   the campaign battle map, default rings, game view distance, RX 7900 XTX:
   - the terrain's ~5 500 entities (mostly far LF ground) made ready per view (5 times a frame, 0.46 ms each) and every patch tested in
     every view (49 600 tests): now once a frame, each entity's box before its patches (5 400 tests), no map of the entities built per
     draw, the placements in a hash map;
   - the sea bed's update went over every scene entity each frame (325 k around Rome): skipped while the scene's revision and the sea's
     origin stand;
   - the selection outline went over every entity each frame looking for overlays: a list of the overlays per overlay revision;
   - the environment cube looked for the ground under the battle's centre over every entity (3 ms every 4th frame, the p95): cached by
     the scene's revision.
   Map centre: 14.6 -> 10.4 ms (p95 15.5 -> 11.8). Around Rome (325 508 draw items): 15.5 -> 11.0 ms (p95 18.1 -> 11.9); the Tile tool's
   map context there was 19.8 ms in Phase 0. The Tile tool's map context captures are unchanged. Left on the CPU around Rome: shadows
   2.7 ms, terrain 2.0, the sea 1.5 (its chunks' waves 0.8), the G-buffer 1.2, view culling 1.8 (10 views).
3. **LF editing**: the paged field, the brushes, the page undo, the GPU updates in parts, the normals, the sea LF, the LF patches, saving
   the TIFFs, `Heights & Normals` against BOB.
   **Progress 2026-11-02.** Done:
   - `LfMap`'s samples shared and edited in place (`LfSamples`: a revision, the samples edited since the renderer looked); the renderer
     updates only the edited texels of the LF window and normal map (`DEFAULT` textures, `LfMap::normal_texel` shared with the context's
     normal map); the Tile tool's map context captures unchanged.
   - `wce_terrain::lf_edit`: `LfBrush` (the Tile tool's `Brush` with Terry's aspect and rotation), `LfStroke` (a float copy of the pages
     it touches, so small dabs add up; the pages before), `LfRecord` (the undo record of those pages: a 1500 m brush's stroke 592 KB);
     raise / lower, smooth, flatten (to the height where the stroke starts), noise.
   - `wce_terrain_editor::lf`: `LfMirror` (the model's field into the view's LF, rows the other way up; what it touched), the stroke
     command (undo / redo on the model, mirrored); `MapDocument::lf_field_mut` / `push_stroke` / `save` (into the projects' kit layout,
     `<projects>/assembly_kit/raw_data/terrain/battles/<map>`: the edited files made again, the others copied once).
   - The Terrain tool's "LF brush" (paint with the left button, the settings, the ring on the ground); the picking pyramid follows every
     edit; the chunks under an edit are built again once no stroke is painted (`Streamer::rebuild`: their objects stand on the new LF);
     `terrain_paint` for scripts.
   - Checked on `settlement_western_city_rome_medium`: a hill raised and smoothed, undone and redone in the view; saved, the kit's
     `lf_heights.tif` and the saved one differ only in the pixel data (the header and size kept), the other files byte for byte; the map
     reopens from the projects folder with the hill.
   - `wce_terrain_io::attila::heights`: BOB's `Heights & Normals` outputs from the model's LF fields, `lf_height_map.dds`,
     `lf_sea_height_map.dds` (its mips) and `lf_height_map.data` (CHMF), byte for byte against BOB on `wce_relief` and `main_attila_map`
     (`heights_build_check`; `gates.sh` runs it over a BOB scratch kit's outputs when `WCE_BOB_KIT` names one).
   **2026-11-03.** Opening: the kits' tile databases read as one (`TileDatabase::load_layered`: the projects' kit layout's few tiles over
   the Assembly Kit's; the project's alone filled the campaign map with nothing); the view starts over the land nearest the map's centre.
   The sea LF: the Terrain tool's sea tiles stand on it (`PlacedMap::sea_lf`, the frame's second `LfMap`: `LF * 400 - 20` m, Rome 2's
   sea tiles' mapping taken as Attila's; without one, the Tile tool's map context, the flat -20 m as before); the LF brush paints either
   field, with undo, redo and save (`lf_sea_heights.tif` rewritten in its pixels only). Dabs every quarter of the radius along the
   stroke's path (`spacing`): the same stroke however fast the mouse goes (`terrain_stroke`: 2, 7 and 200 moves, the same field).
   Note for scripts: a `shot` is taken a frame or more later, so the command after it may already show (`frames 5` after each).
   The per-tile LF patches (Terry's `Apply Per-Tile LF Patches`, decompiled: docs/re/terry.md): `wce_terrain::lf_patch`, a button and
   `terrain_lf_patches`, one undo step; no kit map has them applied (BOB never calls its copy).
   Left: writing the built files on save / export (Phase 7), the edits of a map with a different view lattice (only the map's own frame
   mirrors). **Phase 3 done (2026-11-03)** but for those.
4. **Tile map editing**: the cell layers, explicit tiles, the fill on a worker and the partial rebuild, validation; `Tilemap` against BOB.
   **Progress 2026-11-03.** Done:
   - Cell layers (`wce_terrain::cell_edit`): a stroke (round or square brush, rectangle, flood fill; the picker) keeps the 8 x 8-cell
     chunks it touches, its record undoes them; the document's `CellStrokeCommand` and `CellWatch` (what changed, undo and redo too). The
     Terrain panel's Cell layers section (the colour from the tile database's sets, tiles, variations, climates, or the layer's own),
     painted in the Map panel or on the 3D view's ground; the Map panel's picture written where cells change (`ImageTextures` patchable
     pictures). `terrain_cells`.
   - The fill again after an edit of the tile map, the climates or the explicit tiles (`refill`): the model's layers taken on the UI
     thread, filled on a worker (`place_images`, the LF as 16-bit samples), the chunks whose placements differ (and those their tiles
     reach into) built again with a frame over the new placements (`Frame::with_map`; the streamer's frame swappable, results tagged with
     its generation); the others stay. A kit map is filled from its model when opened too. `terrain_refill_wait`.
   - The fill's speed: a tile is rejected at a cell when each rotation's first checked cell already fails, with the random draws BOB makes
     all the same: the campaign battle map 72 s -> 18 s (byte for byte BOB's tile map and tile list still). One painted cell changes
     every placement after it (decision 3): on the campaign map all 38 920 chunks are built again.
   - Explicit tiles: listed, placed (tile or variation, a quarter turn, the fit shown: the map's edge and the other explicit tiles; BOB
     tests none), turned, removed, outlined in the Map panel; `ExplicitCommand`; the file kept ending as the kit's (BOB fails on an empty
     last line). `terrain_explicit`.
   - Validation (`terrain_validate`): colours of no database entry, climate colours of no climate, the cells no tile fits (the fill's
     messages), explicit tiles unknown, off the map or unreadable, tiles in no kit.
   - Against BOB: an edited Rome map run through BOB's Terrain processor in the scratch kit (`wce_edit`): the battle field exact, the tile
     map equal but for BOB's left-over memory in covered cells' LF min / max; `tile_map_check` fails on anything else, `gates.sh` runs it
     over `WCE_BOB_KIT`'s maps.
   Left: painted settlement sizes against `tile_upgrades.xml` (its rule, docs/plans/tile.md item 3, to be settled with the user); BCAs
   with restricted tiles (Phase 6); the Tile tool's map context captures against the commit before the streamer's frame became swappable
   (not run: C: too full for a second build); the fill's remaining 18 s on the campaign map.
5. **The `.terry` scene**: the ECS model, outliner, inspector, gizmo, creation modes, overlays; `Terry file` against BOB.
   **Progress 2026-11-03.** Done (battle maps; docs/re/terry.md, "The scene's structure"):
   - `wce_terrain::terry`: positions through the transform tree, rectangles, polylines, kinds; attribute edits; entities added
     (`insert_entities`, the kit's layout and line endings, an old header brought to 3 / 10), removed with their descendants and the
     references to them (`remove_entities`), listed under a parent (`add_child`): text splices, exact and small undo records.
   - The map document: `TerryAttrCommand` (merged per gesture), `TerrySpliceCommand`; the Outliner (by kind, then the entity tree), the
     Inspector (every component's attributes; the enumerated ones as choices), the gizmo (`ECTileAlignment` on the cell grid, a root
     `ECLandscapeAbove` on the LF).
   - The Terrain tool: the entities within a radius of the camera as pickable overlays on the LF (editor view; the campaign map's 1 091
     near its centre, no cost in frame time), filters by kind; creation of BCAs (with their `Catchment Region`) and deployment zones
     (clicks; in pairs, under a BCA), delete, duplicate; `terrain_terry`. Validation: settlement and port BCAs on no settlement or port
     tile, zones of no BCA.
   - Against BOB: an edited `.terry` (two BCAs, a zone pair) through BOB's Terrain processor: `locations.blm` lists the BCAs at the cells
     the model puts them, `deployment_0000.xml` has the zones' boundaries as the model computes them.
   **2026-11-04**: a polyline's points edited (the left-button mode "edit the selected polyline's points": pick and move, Shift+click
   adds, Ctrl+click removes; `TerryProject::set_polyline`, `terrain_terry points`), the Inspector on several entities (`inspect_many`:
   the shared attributes, one history step for all); `locations.blm`'s rounding found (Phase 7). Left: the other creation modes
   (circle, spline, river, prop, VFX: campaign maps, Phase 8).
6. **BCA generation**: the spec, the generator, its errors file; Thrones' Terry's output as the oracle.
   **Progress 2026-11-04.** Done (docs/re/terry.md, "The BCA generator"):
   - `wce_terrain::bca`: Terry's generator ported from Thrones' TWeak DLL: the spec, the BCAs (infield, restrictions, border, names),
     the deployment zones (`bca::zones`: the 8 x 8 cells' regions with the deployment masks, the Zonifier, Douglas-Peucker, the coast
     split, the dual configurations), the errors and `delete_invalid`; the `.terry` entities as Terry writes them (`terry_xml`).
   - `bca_check` against Thrones' Terry on `vik_attila_map`: all 636 BCAs (name, type, cell), 780 zones, 1 088 regions, 8 317 points (to
     5 cm) agree, given the tile map Terry reads (the game packs': Terry builds its battle field through the game's file system); with
     BOB's battle field the same BCAs, 514 by name. Our written project reads back the same; its text is Terry's but for ids, y and the
     last digit of about 290 of 11 000 points. Gate `bca_<map>` (`WCE_BCA_ORACLE`). The campaign battle map in 3.2 s.
   - The Terrain tool: "Generate battle catchment areas" (`terrain_terry generate`): the procedural BCAs replaced, one step on the
     history; on `main_attila_map` 24 895 BCAs with 1 778 zones in place of 24 882.
   DEVIATIONS (kept): the y of the generated entities is the LF's (Terry's is the terrain's, tiles included: the editor has the tiles'
   heights only near the camera; no built file reads y: the blm, deployment files and masks use x and z); the errors go to the status line,
   not an errors file (where Terry writes it is not known; it wrote none for the oracle). Left: Attila has no oracle (its shipped
   project was made from other inputs: 97 % of its zones agree), the deployment masks are unchecked against Terry (Thrones' kit has
   none).
7. **Export**: pack output, Launch in game at a location, the minimal-edit gates over the whole build.
   **Progress 2026-11-04.** Done:
   - The builders BOB's `Tilemap` metadata needed, decompiled (BOB_Terrain, ToolDataBuilderDll, the engine; docs/re/terry.md,
     "locations.blm and its masks"): `wce_terrain::deployment` (the deployment files) and `wce_terrain::locations` (`locations.blm`, the
     engine's hash map order, the climate table from `environment_map.xml`, `mask0` / `mask1`). `built_check`: byte for byte on Attila's
     `main_attila_map`, `wce_edit`, `wce_relief` and Thrones' `vik_attila_map`.
   - The whole build from the model (`wce_terrain_io::attila::build`: tile map, battle field, LF heights, blm and masks, deployment
     files, `tile_map.png`). `build_check` (gate `build_<map>`): every file of BOB's runs the same (892 on `main_attila_map`, 21 s; 393 on
     Thrones'), `tile_map.tiles` but for BOB's uninitialised floats.
   - The Terrain tool's Export section: "Export built files" (`terrain_export [--wait]`, on a worker): loose under the output folder
     (`terrain/battles/<map>/`) and as `terrain_<map>.pack`. "Launch in game" (`terrain_launch [<id>] [--dry-run]`): the export (made
     again when the map changed) in the launch pack, a battle at the selected BCA (else the first) through the Tile tool's launch.
     Dry run checked on `main_attila_map` (315 MB pack, Thermopylae at 0.438976, 0.536534).
   The LF normal chunks (`wce_terrain_io::attila::normals`, 2026-11-04): the game's are DXT5 windows of the kit's `normal.dds`, which is
   no function of the LF alone (correlation 0.2 to 0.3: the tiles' relief, most likely); an edited LF gets the chunks its edit reaches,
   `normal.dds` where the LF is unchanged and the LF's own normals over the edited samples (ASSUMPTION: their scale, the editor's);
   `normals_check`. An unedited LF: none (the game's stay).
   DEVIATIONS: a map the game lacks gets no chunks unless its LF is edited; the launch's battle is one unit a side, summer, day, dry.
   Left: a real launch (not run here: it takes the screen; the user's to try).
8. **Campaign maps** (in scope, after the battle maps): `terrain/campaigns/<map>`: props, VFX, environment volumes, season masks, the layout map;
   the 375 MB LF paged from disk.
   **Progress 2026-11-04.** Done:
   - The Terrain tool opens the kit's campaign maps (`campaigns/<map>`): the campaign tile database, the LF at its scale, the `.terry`'s
     frame (0.32 a cell from the bottom-left corner, z north), the props, VFX and environment volumes as overlays.
   - The whole campaign build (docs/re/terry.md, "Campaign maps"): the markers with the kit's transform, the environment collection, the
     LF DDSs and CHMF, the fill with shared geometry and `also_place_tile_set`, the campaign blm and masks. `build_check
     campaigns/main_attila_map` (gate `build_campaign_<map>`): 13 of 13 of BOB's files byte for byte, 21 s.
   - Export of a campaign map: under `terrain/campaigns/<map>/` and as `terrain_<map>.pack` (run in the app: 15 files, 287 MB, 31 s);
     Launch in game refuses it (it starts a battle).
   - Campaign entities: new prop (model, scale), VFX (effect) and environment volume (lighting, radii) at a click, with the kit's
     components (`terrain_terry prop | vfx | ev`); editing through the Inspector and the gizmo as before. ASSUMPTION: a new prop's y from
     the LF by the kit's props' least-squares line (theirs follow the tiles' geometry).
   - Seasons: Terry's season filter (entities whose `ECSeasonMask` leaves the season out hidden). The season-mask and layout layers are
     painted in the Map panel as the other cell layers; the game ships neither (`masks.xml` and the `season_mask_*.png` are the kit's
     own). The snow masks: the map folder's `snow_mask/*.tga` built into the game's `snow_mask_*.dds` (L8).
   - The layout guide: `layout_map.png`'s legend colours (`layout_map_legend.xml`) outlined on the ground near the camera
     (`terrain_layout`). DEVIATION: outlines, not Terry's guide models.
   - Render modes: the geometry-only placements (0x100) are left out of the view; the textured tiles over them draw with their own mesh
     (ASSUMPTION: the bits' meaning, from how BOB sets them).
   - Memory: the LF shared between the placed map and the fill (`LfField` over an `Arc`): opening the campaign map 4.25 -> 3.34 GB
     private.
   Left: the LF paged from disk (still three copies of the campaign's 375 MB: the model's pages, the placed map's rows, the scene's
   samples), the game's own use of the render modes (shared meshes under textured tiles), a campaign launch.
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
8. **After Phase 8** (2026-11-04): a campaign launch stays a nice-to-have (not now); the LF paged from disk comes last, once the rest of
   the plan is done.
