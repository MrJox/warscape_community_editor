# Tile tool: TEd feature parity

**Status (2026-10-26).** Started 2026-10-09 from the user's report on the Tile tool: layers could not be edited, water did not
render, terrain did not look like the game, some buildings stood at the wrong height, and "what else does TEd have". Phases 0, 0b,
1, 2, 3, 4a / 4b, 7 are done, and so are most of 4c, 5 and 6. What is left is the TEd tools below. The editor itself is described in
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
| 5b-5f Tools | vegetation brush and sets, procedural trees / grass, decals, Create panel with preview, prefabs (group / ungroup), props, VFX, point lights, every zone type | `docs/tile_entity_types.md`; walls: `procedural_walls.md`; siege AI generation: `siege_ai.md` |
| 5g Selection | multi-item gizmo, type filters | `docs/editor.md` |
| 6a Links | read-only view (list and cells on the ground) | `docs/re/tile_links.md` |
| 6b Tile module | New tile (Tile > New tile..., a project); height range fitted on save | `workflow_review.md` |
| 7 Export | done (`save_exported`, stages A-C) | `tile_roundtrip.md` |

## Open

### Phase 0 / 3 / 4 gate
- An in-game reference shot of a fixed tile / camera / climate (a settlement tile, a river tile, a sea tile), to compare the terrain
  and water against. None has been supplied; the terrain and water shots are compared only run against run.

### Phase 1
- Gizmo at a constant screen size. Partly done: the handles are 0.16 of the clip height (were 0.1) and do not scale with the
  object; there are no plane handles beyond ImGuizmo's own.

### Phase 2
- Viewport tint for freeform instances. Model entities have no per-entity tint in the renderer. The Inspector shows the mode and the
  Placement section counts the instances.
- The 15 stacked crates of `osgiliath_test` (freeform groups nested under a landscape group, up to 2.70 m off in TEd's export). TEd's
  rule for stacked groups is not plain composition; the tool shows the file's y.

### Phase 3
- 3e season mixing: the season combo picks one season's textures (sections (1, 1, 0)); whether the game mixes seasons or
  climate-change states on a tile (`g_remapped_season`, `g_climate_change_index`) is open (`docs/re/terrain.md`, "Seasons and snow"
  and "Assumptions / open"). Settle it with the in-game shot.

### Phase 4
- 4c: flip a river's direction.
- 4c: snap river end points to the link target cells (`docs/re/tile_links.md`: x along +x, y from the +z edge).
- 4c: check river ends against the entry's targets (osgiliath: both ends lie on the target cells).
- 4d: water map / shallow-deep view (`hf_water_map.data`, `GT_SHALLOW_WATER` / `GT_DEEP_WATER` in `ground_type_map.png`; neither is
  read yet, `docs/re/water.md`). The river bed stays as authored (BOB carves it).
- Water rendering (river uv, shadows on water, foam / ice, `g_refraction_depth`, river ends blending with neighbours, battle sea mesh):
  `rendering_remaining.md`.

### Phase 5a: terrain painting
Today: the height brushes (raise, lower, smooth, flatten, level). Writes need the kit-exact encoders (`blend_map.tif`,
`ground_type_map.png`, `nogo.png`, `lf_influence_map.png`; `docs/formats/image_encoders.md`); one undo entry per stroke.
- Blend layer brush (the four `TEXTURE_GROUP` channels, 16 weights per pixel as read in Phase 3).
- Ground type brush (TEd's `GBP_GROUND_TYPE1..5`).
- Brush falloff editor (TEd's Brush Falloff Editor window) and image brush (`ImageBrush`, Load Brush).
- Spline paint stroke (TEd's Spline Paint Stroke / Spline Segment brush).
- Height brushes: noise, stamp, import PNG.
- Nogo map and LF-influence map painting.

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
- Tile properties editing UI (`min_height`, `scale`, texture groups, colour, flags; the entry panel is read-only).
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
