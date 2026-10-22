# TEd's export of the logic (stage A)

What TEd writes into an exported tile, read in Ghidra (`~/wce_re/bob_proj`, program `TEd.AssemblyKit.exe`, addresses of the Attila
assembly kit's TEd). The compiler that follows it: `wce_tile_io::attila::compile`, docs/formats/tile_compile.md.

## The BMD exporter

- `Ted::BMDExporter::export_` (BMDExport.cpp; `FUN_005022e0` for a building): builds an `EMPIREUTILITY::BATTLEFIELD_BUILDING` per
  building instance and pushes it into `BATTLE_MAP_DEFINITION::building_list`; the definition's serializer (`FUN_004cd1d0`) writes the
  lists (`BATTLEFIELD_BUILDING_LIST`, `_FAR`), the marker tables (`PROPS`, `PARTICLE_EMITTERS`, `AI_HINT_POLYLINES`,
  `AI_HINT_POLYLINE_LISTS`, `AI_SEPARATORS`), the deployment areas, capture locations, outlines (`GO_OUTLINES`, `NON_TERRAIN_OUTLINES`,
  `TERRAIN_OUTLINES`, `LITE_BUILDING_OUTLINES`, `BMD_OUTLINE_LIST`), zones, prefabs and civilian lists, each behind a version check.
- The instance's world matrix comes from a virtual of its transform interface (slot 0x34); `FUN_00449610` copies its 3 x 3 out of the
  `MATRIX_44`, whose element (r, c) is at r + 4 c (`UTILITYLIB::MATRIX_44::e`, UtilMatrix.inl).
- **Position** (`Ted::transform_to_game_tile_space`, `FUN_005059d0`): x * s, the extent minus z * s, y unchanged; s from the tile
  database's render parameters (0.5), the extent the entry's height in cells * `m_terrain_tile_size` (128).
- **Quaternion** (`FUN_00474030`): Shoemake's method on the 3 x 3; constants 1, 2, 0.25, 0.5; x87 work at 53-bit precision, stores to
  float where the code has them (the port in `compile.rs`).
- **Starting damage**: the building's levels {level, min 0, max} (max = the most destructs over its pieces - 1). Without a
  `battlefield_buildings` row or without `destruct_thresholds`: level / (max - min) (`FUN_005066c0`, f32); with them: 0 for level 0, 1 for
  level max, else threshold[level - 1] (`FUN_00505c70`, a `VECTOR<float>`).
- **Flags**: `weak_point`, `ai_breachable`, `indestructible`, `dockable`, `toggleable` from the instance (offsets 0x1e8 .. 0x1ec);
  `lite` from `FUN_00505740` (a lite building zone test); `position_type` 2.
- **Props** (into `BATTLE_MAP_DEFINITION::markers`, `FUN_00509a60`): simple meshes `FUN_00502c60` (a `water_plane` model is special
  cased), decals `FUN_00503360` (scales divided by the unit scale `param[99]`), a third kind `FUN_00504710`. Not read in detail yet.

## Marker tables (`EMPIREUTILITY::MARKER_TABLE`, UtilMarker.h)

A marker table keeps its records in insertion order and groups them by key (`MARKER_KEY`: a list of key parts; a prop's key is one part,
the model path). The groups come out in the order of a `CA::HashTableNoRemove<MARKER_KEY, MARKER_IB_NODE *>`'s slots:

- **Hash** (`FUN_0050e100`): over the key's parts, h = combine(h, part hash) from h = 0, where the part hash (`FUN_004d2740`) runs over
  the part's UTF-16 code units from the last to the first: h = 0xf73a51c9; h ^= unit << shift; shift = (shift + 8) % 32; and combine
  (`FUN_0050e160`) is b = ((low byte of p ^ low byte of h) + 13) & 31; h = (h >> b | h << (b ^ 31)) ^ p.
- **Table** (CAHashTable.inl): slot = hash % capacity, linear probing that wraps; when every slot is full the table grows to
  capacity * 2 + 1 and re-inserts in slot order. The initial capacity is 13.
- Keys are inserted in the order their first record is added. This gives the group order of all 554 `prop_marker.markers` of the packs
  (`tile_marker_keys`). The same table (EmpireUtility) orders BOB's BMD props, which the vegetation port does not reproduce yet
  (docs/re/vegetation.md).
