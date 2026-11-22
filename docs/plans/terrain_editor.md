# Terrain tool: Terry's replacement (plan, 2026-11-02)

The Terrain tool edits a map: the battle map a game's battles are cut from (the campaign's `main_attila_map`, a mod's `rom_third_age_map`, a
historical battle, a settlement map), its low-frequency (LF) ground, the tiles painted on it and the metadata Terry keeps in the map's
`.terry` project. It replaces the Assembly Kit's Terry: Attila's crashes at launch, Rome 2's kit has none, Thrones' works but slowly (user,
2026-11-02). Attila first; every game fact behind the game profile. What Terry does: docs/re/terry.md. It takes over the old tile plan's map-level
scope ("The map pipeline scope taken over from the tile plan", below) and the map context's open items that are about the map rather than the open tile
(docs/plans/map_context_todo.md, "Map-level terrain", "Large map contexts: performance").

**State (2026-11-22).** Phases 0 to 9 built (2026-11-02 to 2026-11-05) for Attila's and Thrones' kit maps, battle and campaign, and Rome
2's maps in their built form; the `terrain` feature of `wce_app` (in the default features, not in `release`). Since then, from an outside
contributor (Jordanakoos, merged 2026-11-10 to 2026-11-22): checked single-byte BLM names (#49), the campaign height metadata from the
layered tile database (#57), Terry overlays removed in bulk (#58), the native campaign props' model and I/O (#59), reliable Save and Export
(#74, [terrain_save_export.md](terrain_save_export.md)) and the land LF region as 16-bit PNG (#75, [lf_region_png.md](lf_region_png.md)).
The campaign map's rendering against the game is docs/plans/campaign_tilemap.md; its lighting docs/plans/campaign_environment.md; the
startpos's settlements and figures docs/plans/startpos_editing.md. LF means the map's low-frequency height; HF the placed tiles' detail.
`MapKind::Battle` includes a campaign's battle map under `terrain/battles/`; `MapKind::Campaign` is the strategic map under
`terrain/campaigns/`: similar `main_*` names do not make their height and normal workflows interchangeable.

## What Terry does and what the tool does with it

| Terry (docs/re/terry.md) | Terrain tool | Phase |
|---|---|---|
| Open / save a `.terry` project, the map's images next to it | a map document: the map folder (kit layout, projects first: `wce_tile::sources` order), every file read and written losslessly | 1 |
| 3D view of the map (engine scene), the tile window (real tiles only inside it), the water plane, the virtual-texture toggle | the map context's streamer and the game's shaders over the whole map; the near ring is the tile window; the sea as today | 2 |
| 2D tile map view | a Map panel: the cell layers as an image (one texel a cell), pan / zoom, the same tools as the 3D view; the viewport's overview map dropped (2026-11-21: it showed the map twice beside the Map panel; the Tile tool's map context keeps it) | 2 |
| Paint tile sets (layers, fill), place / move / rotate / remove explicit tiles | cell layer painting (tile set, tile / variation colours, climate, culture, snow per season), explicit tiles with the gizmo and the fit test (`test_final_tile_position`) | 4 |
| Paint LF heights and sea LF (falloff / image / disk / square brushes, size, aspect, rotation; one add / subtract operation; undo optional) | the tile tool's brushes and operations (raise, lower, smooth, flatten, level, ramp, noise, image brushes, falloff curves) on a paged LF field, always with undo; aspect ratio and rotation added | 3 |
| Apply (per-tile) LF patches | the custom-mesh and sea tiles' `lf_mask.tif` into the LF (docs/re/terry.md, "Per-tile LF patches") | 3 |
| Entities: battle catchment areas, deployment zones and regions, rivers, prop meshes, VFX, environment volumes, season masks; spot / circle / rectangle / polyline / spline creation; gizmo, clone, multi-selection, scene tree, properties, undo view | the `.terry` scene as a lossless model; the editor's selection, inspector (multi-selection), gizmo and creation modes; overlays draped on the ground | 5 |
| Generate battle catchment areas (`bca_generation_spec.xml`) | the generator ported, Thrones' Terry as the oracle | 6 |
| BOB_Terrain: `Terry file`, `Tilemap`, `Heights & Normals` | reimplemented, BOB as the oracle ("The map pipeline scope taken over from the tile plan"); pack output; Launch in game at a chosen location | 7 |
| Campaign projects (`terrain/campaigns/<map>`: props, VFX, season masks, the layout map) | a second document kind on the same model | 8 |
| Thrones (same plugin), Rome 2 (no Terry) and later games | profiles and per-game readers / writers | 9 |

## Architecture

### Crates and source map

Data flows from the mounted sources through `MapFiles` and a format reader into `MapModel`, then through `MapDocument`'s commands and
history to the view. Save writes the authoring files (the kit layout, or a built map's working files); Export builds the game's files.

| Layer | Where | Does |
|---|---|---|
| Sources, game facts | `wce_tile::sources`, `wce_vfs`, `wce_map_view::maps`, the open inputs in `wce_terrain_tool::tool`; `wce_foundation::game::MapFacts` | projects, ordered kits and packs; the map list; the profile and the tile database |
| Model (no GPU, no UI) | `wce_terrain`: `map` (`MapModel`, `Item<T>`: origin bytes and edit state; carried opaque files), `paged`, `pyramid`, `lf_edit`, `lf_patch`, `lf_region`, `cells`, `cell_edit`, `explicit`, `terry`, `bca`, `deployment`, `locations`, `campaign_props`; `wce_formats` | the game-neutral map, its edits and undo records, the BCA generator, the deployment and blm builders |
| Files and builders | `wce_terrain_io`: `attila` (kit maps; `build`, `heights`, `normals`, and the campaign start's `cities`, `roads`, `start`, `characters`, `borders`, `shroud` ...), `built` (maps in their built form; `built::scene`) | readers and writers per game and form, minimal edits, the built files |
| Document, history | `wce_terrain_editor`: `document` (`MapDocument`, `TerryFrame`), `lf`, `cells`, `terry`, `lf_region`, `save_validation`; `wce_editor_core::{command, document}` | commands, undo / redo, saved state, the LF mirror, the Save destination |
| Tool, UI, jobs | `wce_terrain_tool`: `tool`, `map_panel`, `cell_paint`, `explicit_tiles`, `placements`, `refill`, `terry_view`, `prop_models`, `validate`, `export`, `launch`, `lf_region`, `lighting`, `skybox`, `start_figures` | panels, worker refill and export, picking, overlays, checks |
| View, stream, render | `wce_map_view` (`MapView`, `Rings`, `maps`), `wce_tile_editor::tile_document::context::{Frame, stream, template, cache}`, `wce_render::renderer::terrain` | map coordinates, cached tile templates, chunk streaming, the LF's dirty regions, the game's shaders |

The built files' readers (`tile_map`, `tile_list`, `locations`) stayed in `wce_tile::battle_map` (the tile tool uses them) instead of
moving into `wce_terrain`. The streamer stayed in `wce_tile_editor` (its templates open whole `TileDocument`s); `wce_map_view` drives it
for both tools.

### The frame

`context::Frame` is "the map seen from the open tile": the open tile's frame, the LF resampled into it (`lf_map`), the open tile's median
LF as the zero. The Terrain tool has no open tile: its frame is the map's own (the game's: origin at the map's centre, `tile_list`'s
`origin_offset`; x east, z north; world units as the `.terry` positions), the LF's lattice is the map's pixels (no resampling, no copy),
the sea at 0 (`ContextRequest` with no cells). A campaign map is drawn in the game's hex world, z stretched by 2 / sqrt 3
(docs/plans/campaign_tilemap.md, decision 1).

### Game seam

The profile has `MapFacts` (`wce_foundation::game`): the cell size, the file versions (`tile_map` 9 / 6, `blm` 8 / 7, `tile_list` 11),
whether the kit has Terry projects. The model never names a game; `wce_terrain_io::<game>` maps files to it.

## Data structures, memory and frame time

Sizes to plan for (Attila): the campaign battle map is 2032 x 1437 cells (2.92 M), LF 8128 x 5748 (46.7 M samples), sea LF 4064 x 2874,
268 944 placements, 53 782 `.terry` entities; the campaign map's LF is 16256 x 11528 (187 M samples, 375 MB).

- **LF fields: paged u16.** Pages of 64 x 64 samples (8 KB, L1-sized), stored page after page (a brush's rows stay in one page, a
  dab touches a few pages). The file's 16-bit values are the truth (byte-identical round trip; no float copy of the map). A stroke keeps a
  float shadow of the pages it touches (its start values plus the brush's sum), written back to u16 at every dab, so many small dabs do not
  lose to rounding; the shadow goes at the stroke's end. As built: one copy of the samples (`wce_foundation::samples::SampleGrid`) shared
  by the model (`PagedU16`), the placed map and the scene; a kit map's LF a copy-on-write mapping of its TIFF strip (the samples nobody
  edited stay the file's pages).
- **Per-page min / max and a pyramid** over them (`wce_terrain::pyramid::HeightPyramid`: 8 x 8-cell leaf blocks): picking (a ray against
  the LF walks the pyramid, not samples), the blend boxes' y range, and which pages a brush can reach. The campaign battle map's: made in
  8 ms, 1.6 us a ray (`lf_pyramid_bench`).
- **Undo by page, copy on write** (`LfRecord`: the pages a stroke touched, before and after). A stroke of radius 256 px keeps about 80
  pages (650 KB).
- **Cell layers: dense, row-major arrays per layer**: tile set / tile / variation as a palette index (the palette holds every colour the
  PNG has, unknown ones too: a PNG written from an unedited layer is the original bytes, an edited one goes through the kit-exact
  encoder), climate and culture, snow per season. Dirty state per 8 x 8 cell chunk (the streamer's `CHUNK`), so a dirty chunk is a chunk
  to rebuild.
- **The `.terry` scene**: the XML kept as text with spans (`wce_formats::xml_doc`), so unknown attributes, order and whitespace come back
  and an edit rewrites only its attribute; entities added and removed as text splices with small undo records.
- **GPU updates in parts**: the LF window and the LF normal map are `DEFAULT` textures updated over the edited texels only.
- **What an LF edit costs elsewhere**: the chunks under a stroke are built again once no stroke is painted (`Streamer::rebuild`), so their
  objects stand on the new LF.

Budgets: 16 ms a frame p95 at the battle camera's view distance while painting; the map open in under 10 s from the cache. Measured
2026-11-02 (campaign battle map, default rings, RX 7900 XTX): 10.4 ms at the map's centre, 11.0 ms around Rome (325 508 draw items).
Measured 2026-11-05 (the campaign map, the Alps from 9 000 units up): 68.8 ms became 13.7 ms (the terrains in clusters culled by frustum,
a campaign tile one 256-cell patch, flat patches sharing a vertex buffer). Later stream and render work: the terrain stream fixes of
2026-11-17 (#73) and docs/plans/campaign_tilemap.md.

## Rendering

- The game's look: the map context's passes, unchanged in what they draw. The tile window is the near ring around the focus; the mid and
  far rings as the Tile tool's; the view distance slider up to the whole map.
- Overlays (editor views, not the game's look, so editor shaders, never in the game view): a Map panel layer draped on the ground
  (`RenderScene::set_ground_overlay`, the renderer's `pass_ground_overlay`, `EDITOR_HLSL drape_ps`); the brush ring on the LF; the
  `.terry` entities near the camera as pickable overlays; the explicit tiles' outlines in the Map panel; the campaign layout guide.
- The Map panel (2D): the LF shaded, the placements in their tile set's colour, a kit map's cell layers; painting there edits the same
  layers; the hovered cell named from the tile database.
- DEVIATIONS (kept): a battle map is lit by the first day, dry weather of the climate at its centre (`weather/<climate>/`), not the Tile
  tool's weather choice; the fog is an editor view option, "Fog (editor view)" in the camera chip (`view_fog`; docs/editor.md, view
  modes).

## Editing

- **LF** (`wce_terrain::lf_edit`): `LfBrush` (the Tile tool's `Brush` with Terry's aspect and rotation), raise / lower, smooth, flatten
  (to the height where the stroke starts), noise; dabs every quarter of the radius along the stroke's path; the land or the sea LF; the
  per-tile LF patches (`lf_patch`, Terry's `Apply Per-Tile LF Patches`; no kit map has them applied); a land LF region exported to and
  imported from 16-bit PNG (lf_region_png.md).
- **Cell layers** (`wce_terrain::cell_edit`): round or square brush, rectangle, flood fill, the picker; a layer's colours from the tile
  database (sets, tiles, variations, climates); painted in the Map panel or on the 3D view's ground. **Explicit tiles**: listed, placed
  (tile or variation, a quarter turn, the fit shown: the map's edge and the other explicit tiles; BOB tests none), turned, removed;
  `explicit_tiles.txt` kept ending as the kit's (BOB fails on an empty last line, docs/re/bob.md).
- **After a tile map edit** (`refill`): the fill runs again on a worker (`place_images`), and the chunks whose placements differ (and those
  their tiles reach into) are built again with a frame over the new placements (`Frame::with_map`); the others stay. Decision 3: BOB's fill
  draws from one random sequence over the whole map in row order, so one painted cell changes every placement after it (on the campaign
  battle map all 38 920 chunks are built again). The fill there: 18 s (BOB: 13 minutes), byte for byte BOB's tile map and tile list.
- **Entities**: the `.terry` scene picked in the viewport (no tree since the Outliner went, `outliner_removal.md`), filters by kind, the
  Inspector on one or several entities (the shared attributes, one history step), the gizmo (`ECTileAlignment` on the cell grid, a root
  `ECLandscapeAbove` on the LF), a polyline's points edited (Shift+click adds, Ctrl+click removes); creation of BCAs with their catchment
  region, deployment zones (in pairs, under a BCA), and on campaign maps props, VFX and environment volumes; delete, duplicate.
- **Validation** (`validate`, "Check the map", `terrain_validate`): colours of no database entry, climate colours of no climate, the
  cells no tile fits, explicit tiles unknown, off the map or unreadable, tiles in no kit, settlement and port BCAs on no settlement or port
  tile, zones of no BCA. Not checked: painted settlement sizes against `tile_upgrades.xml` (`tilemap_audit`, map pipeline item 3 below;
  its rule to be settled with the user), BCAs whose infield holds restricted tiles.
- **Built form** (the maps of a game without kit maps, Rome 2's, and the packs' campaign maps of any game, a mod's among them;
  `wce_terrain_io::built`, the model marked `built`; other pack maps open read-only): the LF painted (its DDS
  and CHMF made again), the placements of a `terrain.tile_list` or a version 6 tile map (the Placements section: select, variation, turn,
  remove, move, place a database tile; inside the map, on no other placement), the battle locations and deployments as a `.terry` scene
  (`built::scene`); saved under the projects' `assembly_kit/working_data/terrain/{battles,campaigns}/<map>` (read first next time),
  exported as the files that differ from the packs.

## Saving, building and checking

- Raw files: written only when edited, byte-identical otherwise; a TIFF edit rewrites its pixel data only. Save, Save As and the
  freshness, preservation and recovery rules: [terrain_save_export.md](terrain_save_export.md).
- BOB_Terrain's outputs from our builders, byte for byte against BOB on a scratch kit (`bob_run.py scratch`, never the user's kit):
  `tile_map.index` / `.tiles` and `terrain.tile_list` (the fill), `lf_height_map.dds` / `.data`, `lf_sea_height_map.dds`
  (`attila::heights`), `locations.blm` and its masks (`wce_terrain::locations`), the deployment files (`wce_terrain::deployment`), the
  whole build (`attila::build`, also `tile_map.png`), the campaign build (markers, environment collection, LF, fill with shared geometry,
  campaign blm and masks, snow masks).
- The LF normal chunks after an LF edit (`attila::normals`): the chunks the edit reaches, each moved over the edited samples by the change
  of the LF's normal (Attila's from the kit's `normal.dds`, the built form's from its own shipped chunk, Rome 2 has no `normal.dds`); an
  unedited LF exports none. The large maps' 256-sample grid of 576-pixel chunks and one chunk of a 512-sample LF are handled; the other
  layouts keep the game's. A campaign map's normals have no such rule (open items).
- Export (`export`, "Export built files..." in the Terrain panel, File > Export > Built files..., `terrain_export [--wait]`): on a worker, loose under the
  output folder (`terrain/{battles,campaigns}/<map>/`) and as `terrain_<map>.pack`. Launch in game (`launch`, `terrain_launch [<id>]
  [--dry-run]`): the export in the launch pack, a battle at the selected BCA (else the first), one unit a side, summer, day, dry; a
  campaign map is refused (it starts a battle).
- Gates (Cargo examples, `scripts/gates.sh`): `map_roundtrip_check` and `map_built_roundtrip_check` (every map folder of the kits and
  every built map of the packs, Rome 2's too, through the model and back), and with a BOB scratch kit (`WCE_BOB_KIT`) per map
  `heights_build_check`, `tile_map_check`, `built_check`, `build_check` (battle and campaign); `bca_check` (`WCE_BCA_ORACLE`); Rome 2's
  `built_lf_check`, `built_scene_check`, `built_normals_check`, `tile_map_rebuild_check` (`WCE_ROME2_DB`). Other checks run by hand:
  `cities_check`, `roads_check`, `normals_check`, `campaign_prop_check`. Unit and integration tests of Save, Export and the LF region:
  terrain_save_export.md and lf_region_png.md.

## Phases (as built)

0. **Survey and measurements** (2026-11-02): BOB_Terrain runs headless in a scratch kit (docs/re/bob.md): `main_attila_map` in 14
   minutes, a settlement-sized map in under a second; what each action writes in docs/re/terry.md, "BOB_Terrain's outputs".
1. **Model and files** (2026-11-02, docs/formats/map_files.md): 129 kit maps of Attila and Thrones, 862 files, through the model and back;
   the built files of Attila's and Thrones' packs and BOB's outputs written back byte for byte.
2. **Viewer** (2026-11-02): `wce_map_view`, `MapDocument`, the Terrain panel's map list (the packs' maps and the kits'; where each is
   read from: `[game files]`, `[project]`, `[kit: <game folder>]`), the rings and view distance, the Map panel, the overlays, the LF
   picking pyramid. Scripts: `terrain_open`, `terrain_maps`, `terrain_settle`, `terrain_view`, `terrain_pick`, `terrain_map_layer`.
3. **LF editing** (2026-11-03): the brushes on the paged field, the land and sea LF, undo, the GPU updates in parts, the per-tile LF
   patches, `Heights & Normals` against BOB (`heights_build_check`). Scripts: `terrain_paint`, `terrain_stroke`, `terrain_lf_patches`,
   `terrain_brush`.
4. **Tile map editing** (2026-11-03): the cell layers, explicit tiles, the refill on a worker and the partial rebuild, validation; an
   edited map through BOB's `Tilemap` the same (`tile_map_check`). Scripts: `terrain_cells`, `terrain_explicit`, `terrain_refill_wait`,
   `terrain_validate`.
5. **The `.terry` scene** (2026-11-03, 2026-11-04; docs/re/terry.md, "The scene's structure"): `wce_terrain::terry`, the entity tools
   above; an edited `.terry` through BOB gives the blm and deployment files the model computes. Script: `terrain_terry`.
6. **BCA generation** (2026-11-04; docs/re/terry.md, "The BCA generator"): `wce_terrain::bca`, ported from Thrones' TWeak DLL; on
   Thrones' `vik_attila_map` all 636 BCAs, 780 zones, 1 088 regions and 8 317 points agree with Thrones' Terry; the campaign battle map in
   3.2 s; "Generate battle catchment areas" (`terrain_terry generate`) replaces the procedural BCAs in one history step. DEVIATIONS
   (kept): the generated entities' y is the LF's (no built file reads y); the errors go to the status line, not an errors file.
7. **Export** (2026-11-04; docs/re/terry.md, "locations.blm and its masks"): the builders, the whole build against BOB (`build_check`: 892
   files on `main_attila_map`, 393 on Thrones', `tile_map.tiles` but for BOB's uninitialised floats), the export, Launch in game (dry
   run checked).
8. **Campaign maps** (2026-11-04, 2026-11-05; docs/re/terry.md, "Campaign maps"): the kit's campaign maps open (the campaign tile
   database; a cell the world's width in `map_data.esf` over the map's cells, 0.33391 on Attila's and TDD's maps), the campaign build
   (`build_check campaigns/main_attila_map`: 13 of 13 files), export; props, VFX and environment volumes as entities (ASSUMPTION: a new
   prop's y from the LF by the kit's props' least-squares line); the season filter and seasons in the view (`terrain_season`); the
   season-mask and layout layers, the layout guide (`terrain_layout`; DEVIATION: outlines, not Terry's guide models); the LF paged from
   disk. The campaign's start drawn (`terrain_roads`, `terrain_start`, `terrain_weather`; the campaign from the db's `campaigns` row whose
   startpos names the map): the settlements (`attila::cities`), the roads and farmland (`attila::roads`; ASSUMPTIONS: a tile takes the
   highest road level under it; farmland where the chain level >= the area level), sprawl and walls; the rest of the campaign view is in
   campaign_tilemap.md, campaign_environment.md and startpos_editing.md.
9. **Other games** (2026-11-04, 2026-11-05): Thrones as Attila (`vik_attila_map` opens, validates, exports exact against BOB, launch dry
   run); Rome 2 read-only then in its built form (Editing, "Built form"), its seven `main_*` maps' blm and 8 304 deployment files byte for
   byte through the scene, its campaign maps from the packs (`campaigns/<map> (game files)`; `main_rome_map`, `main_gaul`; the cliffs as
   custom-mesh tiles, props from `prop_marker.markers`). Since 2026-11-16 every game lists its packs' campaign maps (a mod's too: TDD's
   `rom_third_age_map` opens as Rome 2's do).

## Supported workflows (2026-11-22)

| Workflow | Input and authoring | Save and export |
|---|---|---|
| Attila / Thrones kit map, battle or campaign | `assembly_kit/raw_data/terrain/{battles,campaigns}/<map>`: land and sea TIFF, cell PNGs, explicit tiles, `.terry`, XML and opaque files; every edit above | Save to the projects' raw layout and reopen; Export builds loose files and a pack. The campaign height scale and offset come from the first layered tile database settings file that exists (#57), not from the saved map folder |
| Built battle map (Rome 2) | the pack's `terrain/battles/<map>` or the projects' `working_data` copy: LF DDS, CHMF, tile list or v6 tile map, blm and deployments, normals | LF, placement and scene edits within the writers' guards; Save the working files, Export the differences from the packs. Never a made-up raw kit map |
| Built campaign map | `terrain/campaigns/<map>` with the campaign tile database | view, placements and LF in the model; native props read and written losslessly (#59) but not editable in the tool; the save path exists, an edited campaign LF's export is not checked (open items) |

Lookup order: the projects' kit layout, then the Assembly Kits in the settings' order, then the packs; models and textures follow the
mounted game sources. Placement edits are guarded by the writers: a tile list's bounds and overlaps; a v6 tile map made again needs the
right database. Unknown or malformed metadata stays opaque for unchanged output and refuses unsafe edits.

## Open items

- **Campaign normals**: no rule for a campaign map's normal chunks after an LF edit (their scale, registration, slope composition,
  overlapping chunk updates, ten mips and compression are not established); a controlled before / after bake is needed. A built
  campaign map's edited LF export is not checked; Rome 2's campaign maps' editing and export in the built form are not exercised.
- **Native normal baseline across reopen**: the export moves the shipped normals by the LF's change since the original; that original is
  kept only while the document is open (`MapDocument`'s `normal_source_bytes`). Repeated edits with Save between them and a reopen have
  no established continuity. A candidate exists (Contributed work, below), rejected in review.
- **Rome 2 / built form**: `tile_map.png` after an edit (the packs' stays); a launch; Rome 2's campaign maps' sprawl pieces (the db names
  models no pack has), the sky under the season lighting (black), the other campaigns' folders (`main_emperor`, `main_greek`,
  `main_punic`, `main_invasion`, `prologue_map_01`); the map list's selection after `terrain_open` (shows the first entry).
- **Fill**: the remaining 18 s on the campaign battle map; a stable fill (decision 3) later, measured against this baseline. terrain.pack's
  `tile_map.tiles` as a base (decision 5).
- **Checks**: settlement sizes against `tile_upgrades.xml`; BCAs with restricted tiles; the deployment masks unchecked against Terry
  (Thrones' kit has none); Attila's BCA generation has no oracle (97 % of its shipped zones agree).
- **Not built**: the other creation modes (circle, spline, river; rivers are the Tile tool's, docs/plans/rivers.md); the Tile tool's map
  context captures against the commit before the streamer's frame became swappable (not run); a real Launch in game (the user's to try); a
  campaign launch (decision 8: a nice-to-have).
- **Campaign view**: the settlement buildings' turn, the sprawl's runtime levels, the farmland's level rule, walls around zones without a
  building (ASSUMPTIONS), Thrones' campaign look: docs/plans/campaign_tilemap.md.

## Contributed work not in master

From the contributor's handover (2026-11-17; the contributor stopped major development then, small fixes may follow). Open upstream PRs:

- [#50](https://github.com/MrJox/warscape_community_editor/pull/50): Rome 2 v9 tile list trailers (`u32 4`, names, a row count, 32 opaque
  bytes a row) parsed and guarded: placement count changes only with zero rows, same-count edits only with paired rows.
- [#51](https://github.com/MrJox/warscape_community_editor/pull/51): the Tile tool's I/O through the shared publication helpers (the
  helpers themselves came with #74: `wce_foundation::publication`).
- [#52](https://github.com/MrJox/warscape_community_editor/pull/52): bounded campaign height deltas (held; no native strategic height
  export; fork PRs #5 to #7 depend on it).

Reference branches on the contributor's fork (`git remote add jordan-handoff https://github.com/Jordanakoos/warscape_community_editor.git`,
`git fetch jordan-handoff 'refs/heads/handoff/*:refs/remotes/jordan-handoff/handoff/*'`); each is on an older base, so compare it against
its own base before taking anything, and keep master's newer campaign coordinates (`TerryFrame::campaign(..., bounds)`), startpos lookup,
prop grounding and scene APIs:

- `handoff/source-native-battle-baseline` (`d1ec429f`): the original LF, normals and CHMF kept outside the game's folders, so continuous,
  saved-between and reopened editing give the same output. Rejected in the second independent review (2026-11-17) for two defects: a
  redirected authoring subfolder can send copy writes into the source map or its store (preflight must check the real destinations,
  junctions included, before any write); a missing normal reference whose key is not in the original inventory is tolerated (recovery
  must require an exact inventoried key). Its notes: `docs/formats/terrain_authoring.md` and
  `docs/investigations/fb_phase4_r1_corrections.md` on that branch.
- `handoff/source-native-props` (`1ac29cd3`): Inspector, gizmo and history for the native campaign props (horizontal moves only); the
  model and I/O are master's (#59).
- `handoff/source-campaign-validation` (`5f6dca25`): read-only campaign checks; carries #52's height work in its ancestry.
- Fork PR #8 (`8be7b8c7`): incremental Terry prop preview (depends on #58, merged).

A playable new strategic campaign also needs logical geography, navigation, settlements and startpos consistency: the campaign readers
above (roads, farmland, trees, cities) are previews, not authoring.

## Risks

- The fill's global random sequence: matched for now (decision 3); a stable alternative is a later step, measured against this baseline.
- The kit's `main_attila_map/tile_map.tiles` is an old file and terrain.pack's tiles are not BOB's fill of the kit's map (72 % of origins
  agree, docs/re/battle_maps.md): the kit's is the base first, terrain.pack's after (decision 5).
- Memory with the campaign map's LF on top of the tile templates (opening the campaign map: 2.2 GB settled, most of the private memory the
  GPU driver's).

## The map pipeline scope taken over from the tile plan (2026-10-08)

The old Tile Studio plan's map-level scope, deferred by the user on 2026-10-08 and taken over by this plan (its file, docs/plans/tile.md,
was folded in here on 2026-11-22; code comments cite these items as "map pipeline item N"). Scope note (user, 2026-10-08): campaign map
files (`map.hex`, `map_data.esf`, `pathfinding.ppd`, trade routes, `.ptd` / `.pbd`) are out altogether: they are not campaign tiles.

Decisions: BOB's processors are reimplemented, real BOB runs (scratch kit, never the user's kit) are the oracle in the checks, not a
runtime dependency; "byte-identical" means round trip, minimal edit and BOB equivalence; unknown bytes are kept; files BOB itself does
not reproduce (`tile_map.tiles` embeds heap pointers) are compared structurally.

1. **Map pipeline** (BOB_Terrain): `tile_map.png` -> `terrain.tile_list`, `tile_map.index`, `tile_map.tiles`; `climate_map.png` ->
   `locations.blm`; `lf_heights.tif` / `lf_sea_heights.tif` -> `lf_height_map.dds` / `.data` (CHMF), `lf_sea_height_map.dds`; the map's
   `.agf` and `map_info.xml`. Built (Phases 3, 4 and 7).
2. **Campaign map inputs**: the campaign `tile_upgrades.xml`, `masks.xml` and the season masks, `layout_map.png`. The season-mask layers
   are painted in the Map panel, `layout_map.png` is outlined as a guide.
3. **`tilemap_audit`**: painted tile size against `tile_upgrades.xml`, missing catchments, missing tile folders. Open (its rules to be
   settled).
4. **LF height tools**: painting the map's LF heights with the bedding rules of `lf_bedding` (beach 1.88144 m, port 0.141909 m, sea plane
   raw 436.9, flat bedding for swamp and lake); metres = raw x `lf_height` / 65535 - `vertical_offset` (`_settings.xml`: 3000, 20). The
   painting is built (Phase 3); the bedding rules are not applied by the brushes.
5. **`.terry`** read first (ECS entities, catchment areas, deployment zones), editing only if asked. Built (Phase 5).

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
9. **Rome 2's maps** (2026-11-04): view first, then edit the built files.
