# FASTBIN0 tile files

All little endian. Every file: `"FASTBIN0"` (8 bytes), `u16 version`, payload. `str` = `u16 length` + bytes (no terminator). Verified by `roundtrip` (C++, `src/formats/fastbin_tile.*`) over the whole corpus, 100% byte-identical.

| File | Version | Payload | Corpus |
| --- | --- | --- | --- |
| `*.tree_list.bin` | 3 | `u32 n_types`, then n x { `str name`, `u32 n`, n x { `f32 x, f32 y, f32 z, u16 w` } } | 562 files |
| `default.grass_list.bin` | 1 | `u32 n_groups`, then n x { `str name` (e.g. `grass/0`), `u32 n`, n x 6 bytes } | 81 |
| `civilian_deployment.bin` | 1 | `u32 n`, n x { `u16 flag` (=1), `str kind` (`Agricultural`), `str size` (`small`), `u32 np`, np x { `f32 x, f32 z` } } | 85 |
| `civilian_shelter.bin` | 1 | `u32 n`, n x { `u16 flag` (=1), `f32 x1, z1, x2, z2`, `u32 capacity` } | 85 |

Notes: the 6-byte grass entry looks like `u16 half-float x`, `u16` (high bit set, meaning unknown), `u16 half-float z` (count matches the BOB log "N valid positions": 11050 x 6 + 49 header = 66349 bytes); kept opaque. `w` of a tree is 0x007f in the samples (probably packed scale/rotation). Tree types use backslashes in names.

# BATTLE_MAP_DEFINITION_DATA binary (`*_procedural_bmd_data.bin`, `bmd_data_escalation{1,2,3}.bin`)

Binary twin of the XML next to it (same data, `serialise_version` attributes become `u16` versions). **Implemented in C++ (`src/formats/bmd_bin.*`), 860 of 860 files round-trip byte-identically.** Decoded from the sample pairs `middle_men_city_b/small/bmd_data_escalation1` and `orc_port_a/small/bmd_data_escalation1`:

- Header: `FASTBIN0`, `u16 ver = 6`, then in XML order: `u32 n` PROPS, `u32 n` PARTICLE_EMITTERS, `u32` AI_HINT_POLYLINES, `u32` AI_HINT_POLYLINE_LISTS, `u32` AI_SEPARATORS (all zero in TDD tiles: hints live in `.markers`), then the deployment manager (`u16 ver 1` + `u32`), BATTLEFIELD_BUILDING_LIST (`u16 ver 1`, `u32 n`, n x BUILDING), BUILDING_LIST_FAR, CAPTURE_LOCATION_SET, EF_LINE_LIST, GO_OUTLINES, NON_TERRAIN_OUTLINES, ZONES_TEMPLATE_LIST (`u16 ver 1`), PREFAB_INSTANCE_LIST, BMD_OUTLINE_LIST, TERRAIN_OUTLINES, LITE_BUILDING_OUTLINES, CIVILIAN lists. The empty file is 104 bytes. Exact order and widths of the empty-list tail still to pin down.
- BUILDING (xml `serialise_version=3`): `u16 3`, `str building_key`, `str position_type` (`BBPT_LF_RELATIVE`), `f32 x,y,z` position, `f32 i,j,k,w` orientation, `f32 x,y,z` scale, `f32 starting_damage_unary`, then 8 bytes: on_fire, start_disabled, weak_point, ai_breachable, indestructible, dockable, toggleable, lite.
- PROP (key `str`): marker `u16 ver 2`, 16 x `f32` transform (m00..m33 row-major), `u8 clamp_to_surface`, `u32 type`, `u8 allow_in_outfield`, `f32 parallax, scalex, scaley, scalez`, then 4 bytes: `tiled`, `is_fauna`, **one byte that is not in the XML** (values 0x08, 0x48, 0x88, 0xc8, 0xec vary per instance with the same key; the XML says snow=destruction=true for all), `0`. Record size 142 for a 48-char key.
- PARTICLE_EMITTER (key `str`): marker `u16 ver 4`, `f32 emission_rate`, 16 x `f32` transform, `str instance_name`, `u8 autoplay`, `f32 scale`, 5 x `u8` (use_in_outfield, snow_inside, snow_outside, destruction_inside, destruction_outside) (to confirm field by field).

Consequences: (1) the XML is lossy relative to the bin (the PROP byte), so the bin cannot be regenerated from the XML alone byte for byte; that byte needs to be understood (likely uninitialised struct padding or a field TEd does not export) or masked in the oracle diff. (2) 21 `bmd_data_escalation*.xml` files contain duplicate attributes (invalid XML), so any reader must tolerate them; `XmlDoc` already round-trips them.

## BMD binary, remaining sections (decoded 2026-10-08)

In file order after the header and the building list: BUILDING_LIST_FAR (`u16 ver 1`, `u32 0`; always empty), CAPTURE_LOCATION_SET (`u16 ver 1`, `u32 n_sets`, per set `u32 n`, n x CAPTURE_LOCATION; an empty `<CAPTURE_LOCATION_SET/>` is just `u32 0`), EF_LINE_LIST (`u32`, always 0), GO_OUTLINES and NON_TERRAIN_OUTLINES (each `u32 n`, n x { `u32 np`, np x { `f32 x, f32 y` } }), ZONES / PREFAB_INSTANCE / BMD_OUTLINE lists (`u16 ver 1`, `u32 0`), then four `u32 0` (TERRAIN_OUTLINES, LITE_BUILDING_OUTLINES, CIVILIAN_DEPLOYMENT_LIST, CIVILIAN_SHELTER_LIST). The parser throws on a non-empty always-empty section so nothing is dropped silently.

CAPTURE_LOCATION: `f32 x, f32 y` (location), `f32 radius`, `u32 min_players`, `u32 max_players`, `str capture_point_type`, `u32 np` + np x (`f32,f32`) points, `u32 ni` + ni x `u32` building indices, `str database_key`, `f32,f32` flag_facing.
