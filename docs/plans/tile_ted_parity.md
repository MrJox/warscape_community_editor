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
  and water against; and, if possible, a RenderDoc / PIX capture of a coastal battle (the sea's radial mesh and sea bed texture). Not
  supplied yet; the terrain and water shots are compared only run against run. **Deferred** (user, 2026-10-26: the user takes the shots
  later). Done (2026-10-29): the winter battle on a river map, a screenshot from the user: the river is not frozen (docs/re/water.md).
  Done (2026-10-29): a burning building in game, three screenshots from the user (Phase 5d below).

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
  under them, so the tool is probably the one wrong. **Confirmed from the game's copy (2026-10-29)**: TDD's
  `gen_dun_medium_final.pack` holds wildmen_city_a/medium's compiled files, and its `building_list.xml` (what the game reads) has the 24
  crates at y 50.47 .. 50.50 each on top of a column of crates at the same x / z: 47.79, 48.69, 49.59, 50.49 (stacks four high, 0.9 m a
  crate; two columns of three start at 48.66). The tool draws the top crate at the bottom one's height (47.79), inside it; the
  middle layers it gets right. So the tool loses the top layer's own 2.7 m. Not the nesting itself: freeform buildings inside landscape groups occur 256,480
  times in the vanilla kit and Phase 0 found only these 15 off. Fixing it still needs the raw tile (the pack has only the compiled files), to see the node
  chain the tool walks wrong. **Closed, not fixed** (user, 2026-10-30): the raw tiles are not available (osgiliath_test's `tile.ted`
  and its `working_data` `building_list.xml`, or TDD's wildmen_city_a raw tile and export).

### Phase 3
- 3e season mixing: settled in game (2026-10-30): the game shows one season's ground, as the season combo does (summer green and
  winter tan at Rome's campaign placement, each matching the editor); no mixing seen (`docs/re/terrain.md`, "Seasons and snow").

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
  (`rendering_remaining.md`, docs/re/water.md "Settled"). Winter ice: settled in game (2026-10-29), a winter battle's
  river is open water, as the editor draws it. **Deferred to the in-game checks** (with the reference shots): the battle sea's radial mesh and sea bed texture (GPU-only: a RenderDoc / PIX capture of a coastal battle, docs/re/water.md).

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
    the fire passes of `GBUFFER_PASSES`, `s_fire_map` = `vfx_fire_diffuse`); `scripts/tile_building_look.txt`. The game's look is
    noted from screenshots (2026-10-29, `docs/re/buildings.md`); compared (2026-10-29): the char now matches at
    burn_amount 0.3 (the fire map was read as sRGB; fixed), the game's likely amount; the flames now cover the
    building as the game's do (the engine's `burning_entity` over each burning piece's mesh; `docs/re/buildings.md`, "The flames"). `burn` is exported (`on_fire`); `burn_amount` is a preview only.
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
- Draw a civilian shelter's target: done (2026-10-28). TEd's TERRAIN_LINE_INSTANCE has two viewport handles (its mouse handler at
  0x46c910: 1 moves the position, 2 the target, over the ground). A selected shelter shows a line over the ground from colour_1 at it to
  colour_2 at the target, and the target's handle drags it over the ground, one undo entry (`Tile::shelter_target`,
  `TileDocument::set_shelter_target`; `shelter_target_drag`, `assert_shelter_target`, `scripts/tile_shelter_target.txt`). Fixed on the
  way: a placed shelter's target stayed near the origin (the draft is built there); it now moves with the placement.
- A siege AI edge's nodes: done (2026-10-28). The file has no ids (an edge's nodes come from geometry), so there is nothing to pick:
  the editor joins them as the game does (`docs/re/siege_ai.md`, "The connection rule") and shows them. A selected edge draws its start
  (green) and end (blue) nodes' rectangles, joined to the Street's ends, and a red cross at an end in no node (not for wall and palisade
  edges, matched to wall area nodes only); a selected node draws the Streets of the edges joining it (`Tile::siege_edge_ends`; cached per
  selection and change). On CA's encampment every area connection joins two nodes. `siege_ends` (`all` logs every edge and counts the
  open ones), `scripts/tile_siege_ends.txt`.
- Road splat painting: done. It is TEd's Spline splat (`docs/re/terrain.md`): a blend channel laid along a spline at its width, which
  the stroke along a spline does with the spline's width (user, 2026-10-26; Phase 5a). **Phase 5f is done.**

### Phase 5g: selection and transform
- Selection tools: done (2026-10-28). B, then a drag: box select (TEd's Frustum selection; the shown, selectable instances whose position
  is inside; Shift adds, Ctrl takes away). The selection menu's Select: box select, all of the primary's type, all with its key,
  everything, invert, and selections saved by name for the session (out of date once the tile's structure changes)
  (`selection_tools.rs`; `box_select`, `select_by`, `selection_save`, `selection_restore`, `scripts/tile_selection_tools.txt`).
  **Phase 5g is done.**
- Copy / paste: done (2026-10-28). Ctrl+C / Ctrl+X / Ctrl+V (at the mouse: the first item there, the rest around it as they were) /
  Ctrl+Shift+V (in place), and in the selection menu: into the working layer, one undo entry, the pasted items selected; across the
  session's tiles (the tool keeps the clipboard). Each item keeps its world rotation; a landscape one stands on the terrain where it
  lands; a civilian shelter's target goes along (`TileDocument::copy_items`, `paste`, `carry_points`; `copy`, `paste`,
  `scripts/tile_copy_paste.txt`).
- Align / distribute: done (2026-10-28; wce's, TEd has none). The selection menu's "Align / distribute": x, z or height (freeform
  only) to the first selected, the lowest, the middle or the highest; even gaps along x or z between the two furthest apart, or on the
  line from the first selected to the last; one undo entry each (`TileDocument::align`, `distribute`, `move_instances`; `align`,
  `distribute`, `scripts/tile_align.txt`).
- Snapping: done (2026-10-28), after TEd's names (`Ted::GRID_SETTINGS`: `grid_snap_tolerance`, `angle_snap_tolerance`,
  `horizontal_snap`, `vertical_snap`, `snap_mode` = `SF_SNAP_TO_GRID / ANGLE / EDGES / VERTICES`; their code was not traced). The grid
  popup has a tolerance (0: the grid always snaps; above, an axis snaps only that close to a grid line, a magnet) and "snap to points"
  (onto the nearest instance or polygon point within the tolerance, 1.5 m at 0), for the grab and now the placing too (it did not
  snap before). While grabbing, PageUp / PageDown move a freeform item along the ground's normal (0.25 m; Shift: 1 m). `snap`,
  `scripts/tile_snap.txt`. Not done: snap to edges, the angle tolerance, horizontal / vertical snap.
- Camera bookmarks: done (2026-10-28; wce's). The toolbar's camera button: the tile's saved views (a click goes there, x forgets one),
  the current view saved by a name; kept in the settings by the tile's folder (`[tools.tile] camera_views`), not in the tile
  (`view_save`, `view_go`, `scripts/tile_camera_views.txt`).
- Piece-to-piece snapping: done (2026-10-28; wce's). With "snap to points" on, a fort piece grabbed near another's end (within the
  tolerance, 4 m at 0) joins it: it takes the other's rotation (the outer sides agree) and its low end goes onto the other's high end,
  or its high end onto the other's low end. The ends are the walls' anchors (the logic file's frame, else the model's x extent;
  `Tile::snap_piece`, `TileDocument::set_world_transform`; `scripts/tile_piece_snap.txt`). Not done: the logic files' platforms and
  collision (nothing needed them).

### Phase 6a: tile links
- Editors for `TILE_LINK` / `TILE_LINK_TARGET`: done (2026-10-28). The Tile panel's Tile links: the targets and rules listed (a click
  picks one and looks at its cell), the picked one's fields edited (the link set from the kit's `_settings.xml` tile sets, the cell, must
  / must not, is_entry, blend size, no_offline_blend, base), adding and removing, each field's meaning in its tooltip; one undo entry each
  (a typed number is one). `TileEntry::set_links` writes the blocks as the kit writes them and splices them only when they change (all
  626 kit entries round-trip: `wce_formats --example tile_links_roundtrip`); saving merges them into the entry file next to the tile.
  `tile_database.bin` needs no rebuild: it is compiled from the entries at export. `link_edit`, `scripts/tile_links_edit.txt`.
- Validator: done (2026-10-28). The Problems section lists the links as the game would not use them or as the kit never makes them
  (`wce_tile::link_checks`): a target outside the tile or off its edge, a target without a "must be" rule of its set on the cell just
  outside it, a rule on the tile's own cell or not just past an edge, a set that is no tile set of the kit (it never matches), twice the
  same, a cell that must be and must not be the same set; a click picks the link in Tile links. On the kit, 28 of 626 entries have one
  (`--example link_check`). The rivers' ends were checked already (Phase 4c). Not checked, on purpose: road ends (the kit's road tiles
  have their roads painted, 1 of 10 has a road spline) and border heights (the export blends every edge to a profile of the links, so
  neighbours agree: `docs/re/battle_maps.md`, "Tile seams"). TEd has no link checks (its integrity errors are about scene instances).
  `link_problems`.
- Link overview and neighbour candidates preview: done (2026-10-28). Tile links > Tiles that can follow: for each join (link target) of
  the tile, every tile of the kit's database at every turn it may take and every place along the tile's edges whose links and the
  tile's agree as BOB's fill tests them, and that carries the join on (`wce_tile::battle_map::link_candidates`). Checked on the kit's
  68 settlement maps: all 1,624 neighbours BOB placed against a join of an unturned tile are among its candidates
  (`--example link_candidates_check`). A click shows one next to the tile through the map context (user: the neighbours were wired there
  already): a map of the two, on a flat LF 100 m up (`ContextUi::show_map`). The map context shows what one battle map placed around
  the tile; this shows what the links allow, map or not. `link_candidates`, `scripts/tile_link_candidates.txt`.
- Open semantics: done as far as the code goes (2026-10-28, `docs/re/tile_links.md`). `is_entry` is the river's flow (the fill's
  `calculate_flow`); `base_x / base_y` only the export's edge blend reads: an offset, in the link set's (1 x 1) linking tile turned as it
  may be, from the link's cell to the cell of the tile where that linking tile's terrain is blended in. Not checked in game; the tooltips
  say so. **Phase 6a is done.**

### Phase 6b: tile module
- Save tile as: done (2026-10-28). Tile > Save tile as... (TEd's Save Tile As): the tile as a battle tile of a tile set, name and
  variation in the projects folder, with its entry `TILES/<set>_<name>.xml` (the open tile's, the new location and tile set, a colour of
  its own by TEd's rule, this variation alone renamed); a tile of that name and size takes it as one more variation (its entry kept
  otherwise), one of another size is refused, as TEd does; the document goes on as the new tile (`TileDocument::save_tile_as`,
  `TileEntry::set_variations`, `set_tile_attr`; `save_tile_as`, `scripts/tile_save_as.txt`). A set that is not in `_settings.xml` yet is
  the database management's business (below).
- Resize tile: done (2026-10-28). Tile > Resize tile... (TEd's Resize Tile, `TileResizingEditor`: the size, the old tile centred along x
  and / or z or at a cell, "data loss" when shrinking): the scene moved with the old tile, the entry's size, and every map of the tile's
  size resized on disk (heights, blend map, ground types, other images of that size; `.resize.bak` beside each), then the tile opened
  again; not undoable. The new area as TEd leaves it (a new terrain: base height, first texture group, no ground type) or, the default,
  carried out from the old edges (`wce_tile::resize`, `TileDocument::resize_tile`; `resize_tile`, `scripts/tile_resize.txt`). Fixed on the
  way: a save after an edit made only of attribute changes inside a splice (moving the scene, snapping river ends) did not write the scene
  (the splice bakes the edits without marking the scene modified); the save now asks the history too.
- Tile properties: done (2026-10-28). TEd's Tile Properties is the colour, the normal strength, the texture groups and a subtile grid
  (valid, has link, link target). The Tile panel's Tile database entry section edits the colour, the variation's normal strength
  (TEd's 0.01 .. 50; it only feeds the exported normal map), the flags (random_rotatable, encampable, scalable,
  requires_infield_lodding, each explained with the kit's counts) and the mask (a grid of the valid cells), one undo entry each, saved
  into the entry (`TileDocument::set_entry_attr`, `TileEntry::set_variation_attr`; `tile_prop`, `scripts/tile_properties.txt`). The
  links are Tile links' (6a); `min_height` / `scale` stay read-only: every save fits them to the heights, as TEd's.
- Tile database management: done (2026-10-28). Tile > Tile database... (TEd's Tile Database Management: a database, its sets and tiles,
  New Tile Set with a name and a colour, New Tile): the kit's and the projects' battle tile sets with their colours and tiles (filtered;
  a click opens a tile), New tile set into the projects' `_settings.xml` (a colour no other set has; a map picture tells sets apart by
  it), and Save for Assembly Kit carries a set the tile uses into the kit's `_settings.xml` (one line, a `.bak`). New tiles go into a
  set with Save tile as (`wce_tile::tile_sets`, `tile_database.rs`; `tile_set_new`, `tile_database`, `scripts/tile_database.txt`).
- Integrity errors view: done (2026-10-28). TEd's Integrity errors dialog (error, type, name; `FUN_004bb1e0`): the hand-placed trees
  outside every exclusion zone (the Problems section had it) and each instance's own check: the polygons (`FUN_00452180`: clipped to the
  tile, at least one part inside, two distinct points for a line, three for a closed one), the siege AI nodes' box and boundary, the
  edges' street and a street edge's two boundaries, a deployment zone's polygons. Listed in the Problems section with the instance's type
  and name; a click selects and frames it (`Tile::integrity_errors`; `integrity_errors`, `scripts/tile_integrity.txt`). "Building overlaps
  with another" has a message in TEd but nothing calls it. Five kit tiles come out clean.
- Stats view: done (2026-10-28). TEd's `stats_view` is only a module name in its scripts (no stats view in its executable) and the
  game's per-tile budgets are not known, so the Tile panel's Stats shows numbers, no limits: instances by type, building keys and those
  without a model, the files on disk, what the last frame drew (`tile_stats.rs`). **Phase 6b is done.**

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
