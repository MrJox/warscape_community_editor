# Tile tool: TEd feature parity

**Status (2026-11-22).** Started 2026-10-09 from the user's report on the Tile tool: layers could not be edited, water did not
render, terrain did not look like the game, some buildings stood at the wrong height, and "what else does TEd have". Every phase is
done (2026-10-31), and so is the list of leftovers worked through on the branch `ingame-checks` (2026-10-29 .. 31, "In-game checks"
below). What remains are small renderer details and things to confirm in game, listed there. The editor itself is described in
`docs/editor.md` ("Tile tool"); what each `tile.ted` element type can do (create / edit / draw / export) is in
`docs/tile_entity_types.md`. Findings are in `docs/formats/tile_ted.md` (placement, layer flags, TEd's modules, Property Editor),
`docs/formats/tile_database.md` (the entry), `docs/re/terrain.md`, `docs/re/water.md`, `docs/re/buildings.md` ("Look properties") and
`docs/re/tile_links.md`; the TEd findings that are only here are under "Findings kept here".

| Area | State | Where |
|---|---|---|
| 0 Repros, oracle, TEd probes | done: repro scripts, `placement_check` (`crates/wce_tile_editor/examples/placement_check.rs`, TEd's `building_list.xml` as the oracle), the flag / placement probes in TEd, the link-semantics spike | `docs/formats/tile_ted.md` |
| 0b Tile entry as the root | done: `TileEntry`, `TileIndex`, Tiles panel, open by entry / folder / `tile.ted` | `docs/formats/tile_database.md`, `docs/editor.md` |
| 1 Layers and scene graph | done: Scene panel (eye = `OIF_VISIBLE`, cursor arrow = `OIF_ACTIVE`), structure commands, export type, gizmo fix and constant screen size, W / E / R, snap toggle. The Scene panel is the only tile tree since the Outliner went (`outliner_removal.md`) | `docs/editor.md` ("Scene panel") |
| 2 Placement and heights | done: landscape / freeform rules, strokes carry landscape instances, oracle exact; stale entries fixed by `TileProject::calibrate_from_final_heights`; Tint freeform instances (wce's view filter) | `docs/formats/tile_ted.md`, `docs/editor.md` ("Placement") |
| 3 Terrain | done with the game's shaders (3a-3e; 3e settled in game: one season's ground, the battle map's cell climate) | `docs/re/terrain.md`; map-level terrain: `map_context_todo.md` |
| 4 Water | done: 4a-4d (4c river editing: flip a spline, river ends against the link targets; 4d the water depth view and Paint water ground types); the battle sea from a RenderDoc capture | `docs/re/water.md`, `rivers.md`; rendering leftovers: `rendering_remaining.md` |
| 5a Terrain painting | done: height brushes (with TEd's Spline Road, and noise, stamp, height map import), blend brush, ground type brush, falloff curve editor, image brush, stroke along a line, Normal (Grouped) and blur, TEd's Blendmap Filters, Auto-Flange and Rock-painting; Paint the whole tile (2026-11-11) | `workflow_review.md` (section 3), `docs/re/terrain.md` |
| 5b-5f Tools | done: vegetation brush and sets, procedural trees / grass, decals, Create panel with preview and thumbnails, prefabs, props, VFX, point lights (radius gizmo), every zone type, shelter targets, siege AI edges' nodes, the buildings' look properties (opacity, burn, the fire preview) | `docs/tile_entity_types.md`; walls: `procedural_walls.md`; siege AI generation: `siege_ai.md` |
| 5g Selection | done: multi-item gizmo, type filters, box select and selections, copy / paste, align / distribute, snapping (grid tolerance, points, edges, heights, angle, pieces), camera bookmarks | `docs/editor.md` |
| 6a Links | done: editors for links and targets, the validator, the tiles that can follow | `docs/re/tile_links.md` |
| 6b Tile module | done: New tile, Save tile as, Resize tile, Tile properties, Tile database management, Integrity errors, Stats | `workflow_review.md`, `docs/editor.md` |
| 7 Export | done (`save_exported`, stages A-C; To mod pack) | `tile_roundtrip.md` |

## As built, per phase

Each item has a script under `scripts/` (named in its first line after the phase) and its tooltips; the detail is in the git history
and docs/editor.md.

### Phase 0 / 3 / 4 gate
- The in-game references: a winter battle on a river map (the river is not frozen, docs/re/water.md), a burning building (Phase 5d),
  settlement, river and coast captures with the cameras from memory dumps, and RenderDoc captures of a coastal battle (2026-10-29 .. 31,
  "In-game checks" below).

### Phase 1
- The gizmo at a constant screen size (2026-10-26): settings `appearance.gizmo_size` (points, default 60), View > Gizmo size, command
  `gizmo_size`.

### Phase 2
- **Tint freeform instances** (2026-10-26, wce's; TEd has no tint): the renderer's `pass_tint` draws the tile's freeform instances again
  in a translucent blue (`FREEFORM_TINT`); Tile menu, `tint_freeform on|off`.
- The 15 stacked crates of `docs/baseline/placement_check.txt` (osgiliath_test, TDD's wildmen_city_a medium / small): **closed, not
  fixed** (user, 2026-10-30): the tool loses the top layer's own 2.701 m; fixing it needs the raw tiles, which are not available
  (finding under "Findings kept here").

### Phase 3
- 3e season mixing: settled in game (2026-10-30): the game shows one season's ground, as the season combo does
  (`docs/re/terrain.md`, "Seasons and snow").

### Phase 4
- 4c (2026-10-27): `TileDocument::flip_spline` (command `spline_flip`; TEd has no such command); river ends against the link targets
  (`tile_document/river_links.rs`: `river_link_problems`, `snap_river_ends`; commands `river_check`, `river_snap`). Measured on the kit's
  187 river ends at an edge: all on a `river` target cell, 0.1 .. 12.4 m past the edge (median 5.8), at most 4.8 m off the cell's centre
  along it. The rivers' later work: `rivers.md`.
- 4d (2026-10-27): the water depth view (`wce_tile_tool::water_view`, `terrain_mesh::water_fields`, `TerrainView::WaterDepth`) and
  `TileDocument::paint_water_ground_types` (shallow from 0.3 m, deep from 2.5 m; the kit's painted deep water is 4.4 .. 5.7 m deep,
  shallow 0.2 .. 1.25 m, by `water_depth_check`). Commands `tile_ground_view water`, `paint_water_types`, `assert_water_depth`.
- Water rendering: the river's uv, fade, foam and `g_refraction_depth` settled from the shaders; the battle sea's radial mesh and sea bed
  texture from RenderDoc (2026-10-30; docs/re/water.md).

### Phase 5a: terrain painting
- TEd's height tools (its Height Map module, fn 0x00487d50) are Add, Subtract, Smooth (a blur), Set (`PM2OpFalloffSet0`) and Spline Road;
  wce has them all: raise, lower, smooth, flatten / level, and **Spline Road** as `TileDocument::level_along` (`level_along`). Noise
  (`BrushKind::Noise`), stamp (`BrushKind::Stamp`) and Tile > Import height map... (`TileDocument::import_height_map`, `import_heights`)
  are wce's, asked for by the user (2026-10-27).
- TEd's Normal (Grouped) (`PaintMode::Grouped`, `grouped_pixel`, `grouped_gate`) and blur (`PaintMode::Blur`), 2026-10-27 / 30
  (docs/re/terrain.md). Auto-Flange, Rock-painting and the Blendmap Filters: `workflow_review.md`, section 3.
- Paint the whole tile (2026-11-11, wce's): `TileDocument::paint_whole_tile`, `paint_whole_tile [g v]`. Flip the whole tile: docs/editor.md.
- Closed (user, 2026-10-27): **nogo map and LF-influence map painting**, not needed: nothing the game reads depends on them
  (docs/re/terrain.md, "`lf_influence_map.png` and `nogo.png`"). **No-go areas are handled as polygons** (user): the
  NOGO_REGION_POLYGON_INSTANCEs.

### Phase 5b
- Painting polygons with a brush (not a TEd feature): done 2026-11-21 as the area brush (`wce_tile_tool::area_brush`, docs/editor.md).

### Phase 5d: building catalogue
- The Create panel searches every valid key by words; thumbnails from the models (wce's): `wce_editor::thumbnails`, made on hover or for
  the whole list, cached as PNGs per game folder; a grid view (`create_view list|grid`); lit from behind the camera (`SUN_TURN`) on a
  grey ground (`add_ground`), effects included (2026-11-07). Decals have none: only the material terrain and TerrainBlend meshes take them.
- The look properties (docs/re/buildings.md, "Look properties"): `opacity` (POSITION4.w; faded instances drawn by `pass_faded` with each
  material's `ps30_alpha_blend_40`, 2026-10-30), `burn` / `burn_amount` (render flag 0x200, the fire shaders, the engine's
  `burning_entity` flames; `burn` is exported as `on_fire`, `burn_amount` is a preview only), the fire preview (2026-10-31,
  `tile_document/fire.rs`, `tile_fire play|stop|state`; ASSUMPTION: the second stage follows at once). `glow_amount`, `dirt_level` and
  `building_tech_level` are hidden from the Property Editor (user, 2026-10-28; nothing in TEd or the game reads them); the file keeps them.
- Every Property Editor row of a building checked (2026-10-27, `scripts/tile_building_props.txt`): each lands in its attribute and undoes;
  a piece's rows act on that piece alone (`Tile::piece_props_or_building`); the rows that only store data change nothing in the viewport,
  as in TEd.

### Phase 5e
- Point-light radius gizmo (2026-10-28): three circles and a +x handle, `TileDocument::set_light_radius`, `light_radius_drag`.

### Phase 5f
- A civilian shelter's target drawn and dragged over the ground (2026-10-28; `TileDocument::set_shelter_target`,
  `shelter_target_drag`).
- A siege AI edge's nodes (2026-10-28): joined as the game does (`docs/re/siege_ai.md`, "The connection rule") and drawn for the selected
  edge or node (`Tile::siege_edge_ends`, `siege_ends`).
- Road splat painting: TEd's Spline splat, done as the stroke along a spline at its width (Phase 5a).

### Phase 5g: selection and transform
- Selection tools (2026-10-28, `selection_tools.rs`): box select (TEd's Frustum selection), select by type / key / all / invert, named
  selections for the session.
- Copy / paste (2026-10-28): Ctrl+C / X / V (at the mouse) / Ctrl+Shift+V (in place), across the session's tiles
  (`TileDocument::copy_items`, `paste`).
- Align / distribute (2026-10-28, wce's): `TileDocument::align`, `distribute`.
- Snapping (2026-10-28 .. 31): the grid's tolerance (a magnet), snap to points, to edges (footprints included, `Tile::snap_outlines`),
  heights too, an angle tolerance (`Tool::snap_gizmo_rotate`); PageUp / PageDown move a freeform item along the ground's normal;
  `snap ... edges|heights on|off angle <deg>`. wce's own, not TEd's, which is buggy there (user, 2026-10-31).
- Camera bookmarks (2026-10-28, wce's): `[tools.tile] camera_views`, `view_save`, `view_go`.
- Piece-to-piece snapping (2026-10-28, reviewed 2026-10-31 against the Roman fort pieces' logic files): the ends are the walk caps
  (`wce_walls::logic::ends`), the x extent only for pieces without caps (`Tile::snap_piece`, `tile_piece_ends`). The procedural walls'
  attachments (`wce_walls::scene::anchors`) still use the x extent.

### Phase 6a: tile links
- Editors for `TILE_LINK` / `TILE_LINK_TARGET` (2026-10-28, `links_editor.rs`, `TileEntry::set_links`; all 626 kit entries round-trip:
  `wce_formats --example tile_links_roundtrip`); `tile_database.bin` needs no rebuild (compiled from the entries at export).
- The validator (`wce_tile::link_checks`, 28 of 626 kit entries flagged; `--example link_check`). Not checked, on purpose: road ends and
  border heights. TEd has no link checks.
- Tiles that can follow (`wce_tile::battle_map::link_candidates`): all 1,624 neighbours BOB placed on the kit's 68 settlement maps are
  among the candidates (`--example link_candidates_check`); shown through the map context (`ContextUi::show_map`).
- Semantics (`docs/re/tile_links.md`): `is_entry` is the river's flow; `base_x / base_y` only the export's edge blend reads. Not checked
  in game; the tooltips say so.

### Phase 6b: tile module
- Save tile as (File menu; `TileDocument::save_tile_as`), Resize tile (`wce_tile::resize`, `TileDocument::resize_tile`; not undoable,
  `.resize.bak` beside each map), Tile properties (the Tile panel's entry section; `min_height` / `scale` stay read-only, every save fits
  them), Tile database management (`wce_tile::tile_sets`, `tile_database.rs`), Integrity errors (`Tile::integrity_errors`, listed in
  Problems), Stats (`tile_stats.rs`: numbers, no limits; the game's per-tile budgets are not known). All 2026-10-28.

### In-game checks (2026-10-29 .. 31, branch `ingame-checks`)
Captures with the cameras read from memory dumps (the game's camera in scene units: x / 2 + half, y / 2 + offset, z / 2 + half; check
a dump's `camera_near_far`, 1 / 13000: a dump can hold another pass's camera), the editor's matching renders, RenderDoc captures of a
coastal battle at low settings (RenderDoc runs out of the 32-bit game's memory at high settings).
- Settled: the ground (docs/re/terrain.md, docs/re/game_command_line.md), the battle sea (docs/re/water.md), the renderer's roofs,
  environment specular, rivers' order, exposure, trees, faded buildings, escalation layers, the fog at the horizon, projected decals and
  the shadow cascades (docs/re/renderer.md). The image gates' references re-recorded (2026-10-31).
- Left (small): the far sea fogs fully where nothing lies under it (beyond the tile's ground or past the map's edge) while the game
  has its 16 km LF under its sea: a sea bed at -20 m there would close it; the game's waterline steps from its 8-bit, 2 m sea bed
  texture; decals skip the legacy terrain patches (a scene whose ground has no material). MSAA: built 2026-11-01 (`msaa.md`).
- To confirm in game: which decal technique the engine picks per decal (the editor draws all with the `_bugged` one, exact for the
  files' boxes); which shadow pull-back a battle uses (one coast battle at max settings used 150, not 5000); the dun roads, fence
  shadows and the coast's beach after these fixes. The dark draped rocks of gen_dun_medium_final_2 are TDD's `rocklarge_*` textures
  (a different layout than the vanilla rock models' uv), in game as well.

## Findings kept here (TEd static RE; to move into docs/formats/tile_ted.md)

- **The stacked crates** (2026-10-26, `Ted::BMDExporter::export_`, `BMDExport.cpp`): TEd's export puts 15 stacked crates exactly 2.701 m
  (5.402 scene units) above the tool's world y, on every tile and at any ground height. The exported y is the node's own world position
  (the scene object's virtual +0x14), passed through `Ted::transform_to_game_tile_space` unchanged (it only scales x / z and mirrors z).
  Confirmed from the game's copy (2026-10-29): TDD's `gen_dun_medium_final.pack` has wildmen_city_a/medium's 24 top crates at y 50.47 ..
  50.50 on columns at 47.79, 48.69, 49.59 (0.9 m a crate); the tool draws the top crate at the bottom one's height. Not the nesting
  itself: freeform buildings inside landscape groups occur 256,480 times in the vanilla kit and only these are off.
- **TEd's snapping** (2026-10-30, `TEd.AssemblyKit.exe`): `Ted::GRID_SETTINGS` ("Grid and Snapping Settings", constructor
  `FUN_005516d0`, `get_property` `FUN_00551a70`) holds grid_cell_count 100, grid_major_line_frequency 5, grid_spacing 10, the two
  colours, show_grid on, `horizontal_snap` / `vertical_snap` off (bytes +0x45 / +0x46), `grid_snap_tolerance` 10, `angle_snap_tolerance`
  90 (degrees) and `snap_mode` 0, a bit set: grid 1, angle 2, edges 4, vertices 8. The toolbar's two snapping buttons
  (`TedAppProxy::on_grid_snapping_activated` / `on_angle_snapping_activated`, `FUN_004f62a0` / `FUN_004f6310`) flip bits 1 and 2 through
  `set_property(10)`; edges and vertices are only settable in the settings dialog. No code reads `horizontal_snap` or `vertical_snap` at
  their offsets; the gizmo's slots (`SelectModeProxy::on_manipulator_moved` / `_rotated`, `FUN_0048e370` / `FUN_0048e4d0`) take the
  position and angle back through virtuals, and the code that applies the snap mode was not found statically.
- **A shelter's handles**: TERRAIN_LINE_INSTANCE has two viewport handles (its mouse handler at 0x46c910: 1 moves the position, 2 the
  target, over the ground).
- **Integrity errors**: TEd's dialog is `FUN_004bb1e0` (error, type, name); a polygon's own check is `FUN_00452180` (clipped to the
  tile, at least one part inside, two distinct points for a line, three for a closed one). "Building overlaps with another" has a message
  in TEd but nothing calls it.
- **Stats**: TEd's `stats_view` is only a module name in its scripts; its executable has no stats view.

## Decisions
- The tile's root is its database entry, not the folder of `tile.ted` (2026-10-09).
- Everything rendered uses the game's shaders; the CPU bake only draws TEd's debug views (2026-10-10).
- The climate is a tool setting: the tile does not store it.
- Phase 5 is every TEd tool; tile links are in scope, including the tile-edge overlap.
- Not carried over: campaign map files, tile_map / Terry, units, ships and projectile testing.
- Edits are text splices on the lossless XML; undo restores the bytes.
- Reverse engineering: black box first (TEd / BOB on scratch copies, diffs, `building_list.xml` and BOB outputs as oracles), then
  short targeted spikes only when that does not settle a question. No full decompile; notes record formats and behaviour, never code.
- Layer editing lives in the Scene panel (the only tile tree since the Outliner went).
- The ground views are a chip on the viewport (the Ground chip), with the Tile panel's Ground section moved in.
