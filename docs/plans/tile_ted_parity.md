# Tile tool: TEd feature parity

**Status (2026-10-26).** Started 2026-10-09 from the user's report on the Tile tool: layers could not be edited, water did not
render, terrain did not look like the game, some buildings stood at the wrong height, and "what else does TEd have". Phases 0, 0b,
1, 2, 3, 4a / 4b, 7 are done, and so are most of 4c, 5 and 6; most of 5a (terrain painting) and part of 6b were done by the workflow
review (`workflow_review.md`, branch `workflow-review`). What is left is the TEd tools below. **This plan is to be finished on the
`workflow-review` branch before that branch is merged** (user, 2026-10-26); the items are worked through one at a time, in this
file's order. The editor itself is described in
`docs/editor.md` ("Tile tool"); what each `tile.ted` element type can do (create / edit / draw / export) is in
`docs/tile_entity_types.md`. Findings are in `docs/formats/tile_ted.md` (placement, layer flags, TEd's modules, Property Editor),
`docs/formats/tile_database.md` (the entry), `docs/re/terrain.md`, `docs/re/water.md` and `docs/re/tile_links.md`.

| Area | State | Where |
|---|---|---|
| 0 Repros, oracle, TEd probes | done: repro scripts, `placement_check` (`crates/wce_tile_editor/examples/placement_check.rs`, TEd's `building_list.xml` as the oracle), the flag / placement probes in TEd, the link-semantics spike | `docs/formats/tile_ted.md` |
| 0b Tile entry as the root | done: `TileEntry`, `TileIndex`, Tiles panel, open by entry / folder / `tile.ted` | `docs/formats/tile_database.md`, `docs/editor.md` |
| 1 Layers and scene graph | done: Scene panel (eye = `OIF_VISIBLE`, cursor arrow = `OIF_ACTIVE`), structure commands, export type, gizmo fix, W / E / R, snap toggle. The shared Outliner shows the tile tree read-only (accepted) | `docs/editor.md` ("Scene panel") |
| 2 Placement and heights | done: landscape / freeform rules, strokes carry landscape instances, oracle exact; stale entries fixed by `TileProject::calibrate_from_final_heights` | `docs/formats/tile_ted.md`, `docs/editor.md` ("Placement") |
| 3 Terrain | done with the game's shaders (3a, 3b, 3d; 3c largely); 3e partial: the season combo picks the textures | `docs/re/terrain.md`; map-level terrain: `map_context_todo.md` |
| 4 Water | 4a, 4b done; 4c mostly (points, handles, insert / prepend, per-point values, `river_spline*.xml` written on save) | `docs/re/water.md`; rendering leftovers: `rendering_remaining.md` |
| 5a Terrain painting | height brushes; blend brush with a live preview, texture groups, ground type brush, falloff curve editor, image brush, stroke along a line (at a spline's width), TEd's Blendmap Filters, Auto-Flange and Rock-painting | `workflow_review.md` (section 3), `docs/re/terrain.md` |
| 5b-5f Tools | vegetation brush and sets, procedural trees / grass, decals, Create panel with preview, prefabs (group / ungroup), props, VFX, point lights, every zone type | `docs/tile_entity_types.md`; walls: `procedural_walls.md`; siege AI generation: `siege_ai.md` |
| 5g Selection | multi-item gizmo, type filters | `docs/editor.md` |
| 6a Links | read-only view (list and cells on the ground) | `docs/re/tile_links.md` |
| 6b Tile module | New tile (Tile > New tile..., a project); height range fitted on save; texture groups edited in the Tile panel | `workflow_review.md` |
| 7 Export | done (`save_exported`, stages A-C) | `tile_roundtrip.md` |

## Open

### Phase 0 / 3 / 4 gate
- An in-game reference shot of a fixed tile / camera / climate (a settlement tile, a river tile, a sea tile), to compare the terrain
  and water against. None has been supplied; the terrain and water shots are compared only run against run. **Deferred** (user,
  2026-10-26: the user takes the shots later).

### Phase 1
- Done (2026-10-26): the gizmo at a constant screen size. Its handles are a length in points (settings `appearance.gizmo_size`,
  default 60, times the display's scale; View > Gizmo size, command `gizmo_size`), turned into ImGuizmo's clip size from the
  viewport's height every frame, so they keep their size whatever the viewport, the camera's distance or the object; ImGuizmo's plane
  handles scale with them. `scripts/tile_gizmo_size.txt`.

### Phase 2
- Done (2026-10-26, a wce view filter, not TEd's: TEd has no tint, checked in its strings): **Tint freeform instances**. The Placement
  section's "tint freeform" box, Tile menu > Tint freeform instances, command `tint_freeform on|off`. The renderer's `pass_tint` (an
  editor pass like the selection outline: the tinted meshes drawn again over the finished image, against the scene's depth, in a
  translucent colour; the game's shaders are untouched); `Document::tint_key` / `tint`, `Editor::tint_entities` (cached like the
  selection's entities); the tile gives its freeform instances (`Placement::Freeform`, in effect through groups) in a translucent
  blue (`FREEFORM_TINT`).
- The 15 stacked crates (`docs/baseline/placement_check.txt`: osgiliath_test and TDD's wildmen_city_a medium / small): TEd's export puts
  them **exactly 2.701 m** (5.402 scene units) above the tool's world y, on every tile and at any ground height (2 m and 47.7 m), so the
  difference is one node's height, not the terrain. Found 2026-10-26 (static RE of `Ted::BMDExporter::export_`, `BMDExport.cpp`): the
  exported y is the node's own world position (the scene object's virtual +0x14), passed through `Ted::transform_to_game_tile_space`
  unchanged (it only scales x / z and mirrors z). The tool draws them at ground level while TEd lists them 2.7 m up, i.e. on the crates
  under them, so the tool is probably the one wrong. Not the nesting itself: freeform buildings inside landscape groups occur 256,480
  times in the vanilla kit and Phase 0 found only these 15 off. **Deferred: waiting for the files** (only on the Windows side): osgiliath_test's
  `tile.ted` and its `working_data` `building_list.xml`, or TDD's wildmen_city_a raw tile and export (the user, with the reference
  shots).

### Phase 3
- 3e season mixing: the season combo picks one season's textures (sections (1, 1, 0)); whether the game mixes seasons or
  climate-change states on a tile (`g_remapped_season`, `g_climate_change_index`) is open (`docs/re/terrain.md`, "Seasons and snow"
  and "Assumptions / open"). Settle it with the in-game shot.

### Phase 4
- Done (2026-10-27): 4c, flip a river's (or road's) direction: `TileDocument::flip_spline` (the points' attribute texts permuted, the
  handles swapped; nothing recomputed, so the spline does not move and two flips give the bytes back; the first point is no longer at the
  instance's origin, TEd's habit only), the spline section's "Flip direction" button, command `spline_flip`, `scripts/tile_spline_flip.txt`.
  TEd has no such command.
- 4c: snap river end points to the link target cells (`docs/re/tile_links.md`: x along +x, y from the +z edge).
- 4c: check river ends against the entry's targets (osgiliath: both ends lie on the target cells).
- 4d: water map / shallow-deep view (`hf_water_map.data`, `GT_SHALLOW_WATER` / `GT_DEEP_WATER` in `ground_type_map.png`; neither is
  read yet, `docs/re/water.md`). The river bed stays as authored (BOB carves it).
- Water rendering (river uv, shadows on water, foam / ice, `g_refraction_depth`, river ends blending with neighbours, battle sea mesh):
  `rendering_remaining.md`.

### Phase 5a: terrain painting
Done: the height brushes, the blend brush, the ground type brush, the falloff curve editor, the image brush, the stroke along a line,
the Blendmap Filters, Auto-Flange, Rock-painting (`workflow_review.md`, section 3). Left:
- Height brushes: noise, stamp, import PNG.
- Nogo map and LF-influence map painting (`nogo.png`, `lf_influence_map.png`, the kit-exact encoders; TEd's Influence Map module has
  Subtract among its operations, `docs/re/terrain.md`).
- TEd's Normal (Grouped) blend operation (not traced) and its blur (`GPOWeightedAverage`: each channel towards its neighbourhood's
  average by the stroke's weight).

### Phase 5b
- Exclusion polygons as brushes (paint the area instead of drawing the polygon).

### Phase 5d: building catalogue
- Key search UI beyond the typed key (search `battlefield_buildings`).
- Thumbnails from the models.
- Tech level, destruction and burn preview.

### Phase 5e
- Point-light radius gizmo (the radius is an Inspector value only).

### Phase 5f
- Draw a civilian shelter's target in the viewport.
- Draw a siege AI edge's nodes (pick the two nodes in the viewport).
- Road splat painting: needs RE first (what TEd writes and what reads it).

### Phase 5g: selection and transform
- Box select (TEd's selection shape).
- Select by type / key in the UI (`tile_select_all` exists only as a command).
- Invert selection; saved selections.
- Copy / paste.
- Align / distribute.
- Camera bookmarks.
- Snap tolerance; move along the normal.
- Piece-to-piece snapping from the buildings' `_tech.cs2.parsed` (collision, platforms, attachments; `docs/re/buildings.md`).

### Phase 6a: tile links
- Editors for `TILE_LINK` / `TILE_LINK_TARGET` as edits of the entry: text splice on `TILES/<tile>.xml`, rebuild of
  `tile_database.bin` with the byte-exact builder, one undo entry per edit.
- Validator: road / coast end points on the target cells, border heights equal to the neighbours' for every neighbour the rules
  would allow. Its results should match TEd's integrity errors where TEd reports any.
- Link overview: which tiles can follow this one on each edge.
- Neighbour candidates preview: load a tile that satisfies a link and show it next to this one (the map context of the Tile panel
  shows neighbours from a map; this is from the links).
- Open semantics: `base_x / base_y` (osgiliath's rules have base (1, 0)), `is_entry`.

### Phase 6b: tile module
- Save As as TEd's *Save tile as* (new name, variation, entry). The generic File > Save as... only writes the tile to another
  folder.
- Resize tile.
- Tile properties editing UI (`min_height`, `scale`, colour, flags; the texture groups are edited already).
- Tile database management (TEd's Database management dialog; new tile set).
- Integrity errors view.
- Stats view (counts, memory budget).

## Decisions
- The tile's root is its database entry, not the folder of `tile.ted` (2026-10-09).
- Everything rendered uses the game's shaders; the CPU bake only draws TEd's debug views (2026-10-10).
- The climate is a tool setting: the tile does not store it.
- Phase 5 is every TEd tool; tile links are in scope, including the tile-edge overlap.
- Not carried over: campaign map files, tile_map / Terry, units, ships and projectile testing.
- Edits are text splices on the lossless XML; undo restores the bytes.
- Reverse engineering: black box first (TEd / BOB on scratch copies, diffs, `building_list.xml` and BOB outputs as oracles), then
  short targeted spikes only when that does not settle a question. No full decompile; notes record formats and behaviour, never code.
- Layer editing lives in the Scene panel; the shared Outliner keeps the tile tree read-only.
