# Tilemaps and the map pipeline: later scope

What is left of the old Attila Tile Studio plan (the C++ plan of 2026-10-06 to 10-09). Its tile work is done in Rust; what remains is the
map-level scope the user deferred on 2026-10-08.

## Status (2026-10-26)

Not started. Scope note (user, 2026-10-08): tilemaps and Terry are a separate, later scope: `tile_map.png`, `climate_map.png`,
`terrain.tile_list`, `tile_map.index` / `.tiles`, `locations.blm`, `lf_height_map.*`, `lf_heights.tif`, `lf_sea_heights.tif`, the maps'
`.agf`, `.terry`, `explicit_tiles.txt`, the maps' deployment XML. Campaign map files (`map.hex`, `map_data.esf`, `pathfinding.ppd`, trade
routes, `.ptd` / `.pbd`) are out altogether: they are not campaign tiles. What is known of these files: docs/formats/tile_binary.md,
"Map-level files"; who reads and writes them: docs/re/bob.md.

## Decisions (still valid)

- BOB's processors are reimplemented; real BOB runs (scratch kit, never the user's kit) are the oracle in the checks, not a runtime
  dependency.
- "Byte-identical" means three gates: round trip (parse then write equals the input), minimal edit (an edit rewrites only the bytes it
  must), BOB equivalence (our output from the same raw inputs equals BOB's).
- Unknown bytes are kept; no parser drops data to pass a gate.
- Files BOB itself does not reproduce (`tile_map.tiles` embeds heap pointers) are compared structurally.

## Open

1. **Map pipeline** (BOB_Terrain): `tile_map.png` -> `terrain.tile_list`, `tile_map.index`, `tile_map.tiles`; `climate_map.png` ->
   `locations.blm`; `lf_heights.tif` / `lf_sea_heights.tif` -> `lf_height_map.dds` / `.data` (CHMF), `lf_sea_height_map.dds`; the map's
   `.agf` and `map_info.xml`. Oracle: BOB_Terrain's actions as Terry runs them (`Tilemap`, `Heights & Normals`).
2. **Campaign map inputs**: the campaign `tile_upgrades.xml`, `masks.xml` and the season masks, `layout_map.png`.
3. **`tilemap_audit`**: painted tile size against `tile_upgrades.xml`, missing catchments, missing tile folders (memory note
   `tdd-tilemap-painted-size`).
4. **LF height tools**: painting the map's LF heights with the bedding rules of `lf_bedding` (beach 1.88144 m, port 0.141909 m, sea plane
   raw 436.9, flat bedding for swamp and lake; memory note `attila-lf-heightmap-facts`); metres = raw x `lf_height` / 65535 -
   `vertical_offset` (`_settings.xml`: 3000, 20).
5. **`.terry`** read first (ECS entities, catchment areas, deployment zones), editing only if asked.

## Where the rest of the old plan went

- Done: the BOB spike and determinism (docs/re/bob.md); readers and writers of every tile file (docs/plans/tile_roundtrip.md, Phase 1);
  the tile database build (`tdb_build`, docs/formats/tile_database.md); stage A `attila::compile`, stage B `attila::stage_b`, stage C
  `attila::stage_c` (docs/formats/tile_compile.md); new tile from a template (`new_tile.rs`); the Tile tool with layers, gizmo, asset
  browser and the weather preview (docs/editor.md).
- Height, blend, nogo and LF influence painting (noise, stamp, import PNG brushes), copy / paste, multi-select, piece-to-piece snapping,
  TEd module parity: docs/plans/tile_ted_parity.md.
- Rendering LOD (terrain geomorphing, impostors for far buildings): docs/plans/rendering_remaining.md.
- Vegetation leftovers: docs/plans/vegetation_remaining.md.
- Autosave: docs/plans/editor_todo.md.
- Campaign stage B (densities that do not divide 128, projects without a blend map), pack output: docs/plans/tile_roundtrip.md.
- Reference moved to docs: the BOB command line, the DLL strings, the files of a raw tile (docs/re/bob.md); TEd's modules
  (docs/re/ted_export.md); image encoder fingerprints (docs/formats/image_encoders.md); map-level formats
  (docs/formats/tile_binary.md).
