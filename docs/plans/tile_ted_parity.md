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
  and water against; with it, a winter battle on a river map (is the river frozen?) and, if possible, a RenderDoc / PIX capture of a
  coastal battle (the sea's radial mesh and sea bed texture). None has been supplied; the terrain and water shots are compared only run against run. **Deferred** (user,
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
- Done (2026-10-27): 4c, river ends against the link targets (`tile_document/river_links.rs`). Measured on the kit's 187 river ends at
  an edge: all on a `river` target cell, 0.1 .. 12.4 m past the edge (median 5.8), at most 4.8 m off the cell's centre along it.
  `river_link_problems`: a river target without an end at it, an end at an edge cell that is no target, an end more than 8 m off its
  cell's centre (the Tile panel's Problems section, command `river_check`); `snap_river_ends`: the ends at target cells level with the
  centre, 6 m past the edge unless past it already, handles along, one undo entry (the Problems section's button, `river_snap`).
  `scripts/tile_river_links.txt`; four kit river tiles (straight, fork, 4 x 4 fork, crossing) report nothing.
- Done (2026-10-27): 4d, the water depth view and Paint water ground types (user's choice). The game's shallow / deep water is the
  painted ground types (deep water 6, shallow water 7), which the ground types view shows and the ground type brush paints; the view adds
  where the water is and how deep: `wce_tile_io::attila::terrain_mesh::water_fields` (the calls `terrain_files` makes for
  `hf_water_map.data`: the rivers' triangles and the water planes over BOB's final heights) on the tile as it is in memory
  (`wce_tile_tool::water_view`, worked out again once an edit settles), `TerrainView::WaterDepth` / `bake_water_depth` (cyan shallow to
  dark blue 6 m and deeper, the painted deep water's edges white, shallow yellow), `TileDocument::paint_water_ground_types` (shallow from
  0.3 m, deep from 2.5 m, water paint cleared where the water is shallower; one undo entry). The thresholds from the kit
  (`water_depth_check` on six river tiles): painted deep water 4.4 .. 5.7 m deep (quartiles), shallow 0.2 .. 1.25 m, unpainted water at
  the banks -1.25 .. 0.8 m. Ground section: the view, the two depths, the button (each with its tooltip); commands
  `tile_ground_view water`, `paint_water_types`, `assert_water_depth`; `scripts/tile_water_depth.txt`. TEd has no water view. The river
  bed stays as authored (BOB carves it).
- Water rendering (2026-10-27): the river's uv, fade, foam and `g_refraction_depth` settled from the shaders, the water takes shadows
  (`rendering_remaining.md`, docs/re/water.md "Settled"). **Deferred to the in-game checks** (with the reference shots): winter ice (the
  engine's ice textures belong to a terrain node water mode next to the campaign's; does a winter battle freeze its river?), and the
  battle sea's radial mesh and sea bed texture (GPU-only: a RenderDoc / PIX capture of a coastal battle, docs/re/water.md).

### Phase 5a: terrain painting
Done: the height brushes, the blend brush, the ground type brush, the falloff curve editor, the image brush, the stroke along a line,
the Blendmap Filters, Auto-Flange, Rock-painting (`workflow_review.md`, section 3). Left:
- TEd's height tools (2026-10-27, its Height Map module fn 0x00487d50): Add, Subtract, Smooth (a blur), Set (`PM2OpFalloffSet0`) and
  Spline Road; wce has the first four (raise, lower, smooth, flatten / level). Noise, stamp and PNG import are not TEd's: wce additions
  the user asked for (2026-10-27).
- Done (2026-10-27): **Spline Road** (`Ted::SplineBlendPaintOperation`'s height twin: `apply` fn 0x00485160 moves a height inside the
  road's strip towards the road's height there, `h + (road - h) * w`): `TileDocument::level_along`, a flatten stroke along the selected
  line whose target at each dab is the line's height there, as wide as the spline (else the brush's radius), the brush's falloff for
  the banks, full strength; landscape instances follow. The brush Inspector's and the spline section's "Level the ground" buttons
  (tooltips), command `level_along`, `scripts/tile_level_along.txt`.
- Done (2026-10-27): the **noise brush** (wce's): `HeightField::noise` / `fractal_noise` (four octaves of value noise, -1 .. 1, a function
  of the pixel so that dabs deepen the same bumps), `BrushKind::Noise`, the toolbar's mountain button (Noise mode), its feature size in
  the Inspector (tooltip), `paint noise`, `scripts/tile_noise.txt`, unit tests.
- Done (2026-10-27): the **stamp** (wce's): `BrushKind::Stamp` adds `stamp_height_m` at full weight once per click, in the brush's shape
  (the image brush's picture when on, else the falloff); negative for a hollow. The toolbar's picture button (Stamp mode), its height in
  the Inspector (tooltip), commands `stamp_height`, `paint stamp`, `scripts/tile_stamp.txt`.
- Done (2026-10-27): **import a height map picture** (wce's): `Tile::import_heights` / `TileDocument::import_height_map` (the picture
  read as the image brush reads it: grey or colour PNG, 8 or 16 bit, or DDS; stretched over the whole height map, `height_map_0.png`'s
  layout; black and white at the heights given; replace or add; landscape instances follow; one undo entry, `HeightFillCommand`), Tile >
  Import height map... (a dialog with tooltips, black / white starting at the tile's lowest / highest heights), command
  `import_heights`, `scripts/tile_import_heights.txt`.
- Closed (user, 2026-10-27): **nogo map and LF-influence map painting**, not needed. Nothing the game reads depends on them: BOB_Tile's
  processor (fn 0x100095b0) takes `lf_influence_map.png` when present and writes `final_lf_influence.dds` into the raw folder (460 kit
  tiles have one), which nothing downstream reads (not the tile compile, which wce reproduces byte for byte without it; not BOB_Battle;
  not the game, whose DLL names no LF influence); every shipped tile's influence map is 1 everywhere. `nogo.png` is an input BOB lists
  when present; the no-go areas the game gets are `terrain_outlines.xml` (the final heights' slopes) and the NOGO polygons. **No-go
  areas are to be handled as polygons** (user): the NOGO_REGION_POLYGON_INSTANCEs (drawn today; painting them as brushes is Phase 5b).
- Done (2026-10-27): TEd's **Normal (Grouped)** (`PM2OpBlend2Base`, properties `channel_group` and `flange`; `apply` fn 0x00482ad0):
  a group painted at a continuous variation, split between the two around it (`PaintMode::Grouped`, `grouped_pixel`); TEd's gate on the
  weight (a per-pixel function the module hands it: where 0, the weight is its neighbourhood's average) was not traced and is left out.
  And its **blur** (`GPOWeightedAverage` "Smooth" built with radius 2 on the blend map, 3 on the height map; weights `exp(-d^2 / 8)`, fn
  0x004814b0): `PaintMode::Blur`, each channel towards that average as the stroke found it, by the stroke's built-up weight. The paint
  brush's mode choice (tooltips), the Grouped variation slider, `brush_paint ... grouped [v] | blur`,
  `scripts/tile_paint_grouped_blur.txt`, unit tests. **Phase 5a is done.**

### Phase 5b
- Moved to "Ideas for later" (user, 2026-10-27): painting polygons with a brush is not a TEd feature.

### Phase 5d: building catalogue
- Key search UI beyond the typed key: done, the Create panel searches every valid key by words.
- Thumbnails from the models (wce-only, user 2026-10-27): done. The Create panel's tooltip of a building, tree, model or prefab shows its
  models (what the placing preview draws) rendered alone by a renderer of the panel's own, under the editor's weather; made when hovered
  or for the whole filtered list ("Make thumbnails of the list"), kept as PNGs in the cache folder per game folder
  (`wce_tile_tool::thumbnails`; `thumbnail` script command, `scripts/tile_thumbnails.txt`).
- The look properties, traced in TEd and the kit's Warscape.dll (2026-10-27, `docs/re/buildings.md` "Look properties"):
  - `opacity`: done. TEd hands it to the renderer, which packs round(opacity * 1000) into POSITION4.w; the pieces draw with it
    (`Tile::opacity_w`, kept up when the Inspector sets it; `tile_set_prop`, `assert_instance_param`, `scripts/tile_building_look.txt`).
    The alpha-tested G-buffer shaders fade with it. Below 1 TEd files the building for the blended forward shaders (render flag
    0x2000, `rigidstandard ps30_alpha_blend_40`), which the renderer does not have: opaque materials show no fade yet. The G-buffer
    dissolve (`g_dissolve_enable`) was tried and dropped: its test (opacity - dissolve * (1 - opacity) >= 0.5) only fades between 0.75
    and 0.5 and hides the building below that.
  - `burn` / `burn_amount`: done (2026-10-28). The game's burning buildings: render flag 0x200 draws the pieces with their material's
    fire shader, charred by POSITION6.x = the burn amount (`docs/re/buildings.md`). The editor does the same (`MeshInstance::burning`,
    the fire passes of `GBUFFER_PASSES`, `s_fire_map` = `vfx_fire_diffuse`); `scripts/tile_building_look.txt`. Check the look against
    a burning building in game. `burn` is exported (`on_fire`); `burn_amount` is a preview only.
  - `glow_amount`: nothing reads it in TEd's engine or the game (the game's glows are unit and deployable highlights). Hidden from the
    Property Editor (user, 2026-10-28).
  - `dirt_level` never reaches the renderer; `building_tech_level` has no effect in TEd (no case in its property handler, no model
    choice reads it) and is not exported. Both hidden from the Property Editor (user, 2026-10-28); the file keeps them.
  - `destruction_level`: already previewed (the piece files of the level).
- Every Property Editor row of a building checked (user, 2026-10-27; `scripts/tile_building_props.txt` on hadrians/8x1_lower): each lands
  in its attribute, shows where TEd shows it and undoes; a piece's own rows (visible, opacity, position, destruction level) act on that
  piece alone, and a piece starts from the building's look values (`Tile::piece_props_or_building`). Rows that only store data (weak_wall, ai_breachable, dockable, indestructible, toggleable, capture_location; a piece's active, draggable,
  surface_mode, show_collision, seasons) change nothing in the viewport, as in TEd. Script commands `assert_attr`, `assert_piece`,
  `assert_instance_param`. **Phase 5d is done**; the blended forward pass for faded opaque materials is left for the renderer plan.

### Phase 5e
- Point-light radius gizmo: done (2026-10-28). A selected light shows its reach as three circles (the renderer's selection overlay,
  instead of the box) and, in the tile tool's Select mode, a handle on the +x side: dragging it sets `radius` on the light's horizontal
  plane, through the Inspector's property (clamped to `[0, 1000]`), one undo entry per drag (`TileDocument::set_light_radius`;
  `light_radius_drag`, `assert_attr_near`, `scripts/tile_light_radius.txt`). **Phase 5e is done.**

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

## Ideas for later (user, 2026-10-27)
- Polygons painted with a brush instead of drawn point by point (exclusion polygons, and the no-go areas, which are to be handled as
  polygons): a new feature, not TEd's; for a future pass.
- Move the ground views (colour, blend groups, ground types, protection, water depth) into a quick filter in the viewport, next to the
  other viewport chips, instead of the Tile panel's Ground section.

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
