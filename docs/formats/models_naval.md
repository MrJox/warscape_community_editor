# `models_naval` (ship logic table)

`db/models_naval_tables/models_naval`: one row per ship model, its plain columns and its compiled ship logic. rpfm's schema doesn't
cover it; the reader is `wce_formats::models_naval` (decode and a byte-identical encode), the gate `models_naval_check` (2026-11-11).

| Game | Size | Version | Rows |
|---|---|---|---|
| Attila | 7,930,625 | 11 | 35 |
| Rome II | 5,601,038 | 11 | 50 |

Both decode to the last byte with the same layout and encode back identically. The oracles are the kits' only ship logic sources,
Attila's `assembly_kit/raw_data/art/battle/naval/models/ships/attila_dromon_large/logic.xml` (rows `att_shp_dromon_large` and
`attila_dromon_large`) and Rome II's `.../ships/roman_three/logic.xml` (row `roman_three`, the only one with ropes): every bobby, deck
corner, rope point, damage panel vertex, collision vertex, EF line and custom position matches within 0.002 (the XML's 3 decimals).

## Axes

Every position is in the ship model's space, the space its `variantmeshes/_variantmodels/ships/<ship>/*.rigid_model_v2` parts are
authored in: **x right (starboard), y up, z forward (bow)**. logic.xml writes z up and y forward; the table stores the XML's
`(x, z, y)`, no sign flips. Checked against `attila_dromon_large`: the XML part `bow_02` spans x 0..4.75, y 12.94..27.98, z -2.85..4.44;
the model `bow_02_dm1` x 0..4.96, y -3.23..5.08, z 12.94..28.06. `starb_*` parts are at x > 0, `port_*` at x < 0.

## Layout

Little endian. `string` is a u16 byte length and UTF-8 (ASCII in practice), `wstring` a u16 length in characters and UTF-16 LE, `vec3`
three f32, `list<T>` a u32 count then the items.

Header (as every DB table of these games): `FD FE FC FF` and a wstring GUID; `FC FD FE FF` and u32 version (11); u8 1; u32 row count.

Row:

| Field | Type | Notes |
|---|---|---|
| key | string | `att_shp_dromon_large` |
| rigging_file | string | `VariantMeshes\_VariantModels\ships\<ship>\logic.xml.rigging` |
| destruction_file | u8 flag + string | Attila: flag 1, empty. Rome II: flag 0, or a `RigidModels\Naval\Destruction\*.rigid_model_v2` |
| display_folder | string | the folder of the ship's rigid models |
| oar_upper, oar_middle, oar_lower | 3 string | `models_oars` keys, in this order (`four_l3`, `four_l2`, `four_l1`) |
| selection_indicator_shape | string | `capsule` / `ellipse` |
| cameras | list<camera> | |
| bobbies | list<bobby> | `<buoyancy>` |
| decks | list<deck> | `<decks>` |
| pipes | list<pipe> | `<pipes>`; Attila: none |
| cannons | u32 | always 0 (`<cannons>` is empty in both kits; the reader fails on anything else) |
| damage parts | list<part> | `<damage_model>` |
| collision | list<face> | `<collision>` |
| ef lines | list<ef_line> | `<ef_lines>` |
| custom positions | list<custom_position> | `<custom_positions>` |
| trailing | u32 | always 0; **ASSUMPTION** the `<ef_artillery_info>` list (empty in both kits), written last |

The kit's `logic_folder` column (`ships\attila_dromon\`) has no field: the logic sections are what it compiled to.

- camera: wstring name (`navalcam_01`, `navalcam_02`; from the kit's `ship_cameras.cs2`), 16 f32. Rows 0-2 are an orthonormal basis
  with w 0, row 3 a position and, in w, 0.817 in every row: **ASSUMPTION** the vertical field of view in radians (46.8 degrees).
- bobby: u32 id, f32 mass, f32 radius, vec3 position, f32 breach radius, vec3 breach offset (both 0 when the XML has no `<breach>`),
  list<u32> connections (bobby ids). **ASSUMPTION** the breach is where water comes in once holed, its offset relative to the bobby.
- deck: wstring id (`deck01`; Rome II's raiders `deck_01_main`, `deck_02`, `deck_03`), u32 index (1, 2, 3 in order), list<piece>.
  - piece: u32 id (from 1), list<corner>, f32[4] plane: unit normal and d with `n . p + d = 0` for its corners (normal up).
  - corner: vec3, u8 type: 0 `clear`, 1 `hard`.
- pipe (a rope): u32 point count n (2 or 4), u32 id, u32 unknown, i32 first point's deck piece, n-1 vec3 (the first n-1 points), i32
  last point's deck piece, vec3 last point. The XML's `<segment deck_piece="..">` carries the attribute only on the first and last
  segment; -1 is none. The id is the XML id's number less 100 (`mpipe_rope_131` is 31) in the one oracle: **ASSUMPTION** in general.
  The unknown is 2 for four-point ropes, 1 for two-point ones (whose ends are tied to deck pieces 11..60).
- part: wstring name (`bow_01`, `port_02`, `starb_01`, `stern_02`, `misc_12`; a ship's `<name>_dm1` / `_dm2` models), list<wstring> sail states (`stationary`, `battle_sail`, `half_sail`, read by `FUN_10fe9620` as 1, 2, 3; empty in every row: Attila's and Rome 2's sails have no states, docs/re/cloth.md), list<panel>; panel: list<vec3> (3 or 4 vertices).
- face: wstring name (`logic_collision3d`, some ships `logic_collision3D`), u32 index (from 1), 3 vec3 (no count).
- ef_line: wstring name (`EFLine001`), u32 purpose, vec3 end, vec3 end, vec3 direction. The XML's direction (length about 0.38) is
  stored normalised. The purpose is the game's entity formation purpose enum, named as the XML's `action` (the names are in
  `empire.retail.dll` in this order, and the oracles confirm 4, 9, 10, 13, 22, 23, 24, 25):
  0 Low wall, 1 High wall, 2 Window, 3 Overflow, 4 Marines, 5 Seamen, 6 Gunners_overflow, 7 Captain, 8 Officer1, 9 Boarding,
  10 Naval_firing_position_stand, 11 Naval_firing_position_crouch, 12 Naval_firing_position_stand_360, 13 Naval_perimeter_position,
  14 Tree, 15 Entrance Defence, 16 Officer2, 17 Officer3, 18-21 Crenel left outer / left inner / right inner / right outer,
  22 Engine_placement, 23 Secondary_engine_placement, 24 Disembark_left, 25 Disembark_right.
  Used: Attila 4, 9, 10, 22, 23, 24, 25; Rome II also 7, 8, 13, 16, 17.
- custom_position: wstring name, u32 kind, u32 unknown (1 in every row), vec3 position, vec3 (0, 1, 0) in every row
  (**ASSUMPTION** an up axis). The kind follows the name: `flag` 0, `oar_left_l1`..`l3` 6..8, `oar_right_l1`..`l3` 9..11 (all seen).
  The DLL lists `wheelsplash`, `steamfunnel`, `wheel`, `paddle`, `propellor` before the oar names: **ASSUMPTION** kinds 1..5, Empire's
  steamships.

## What the sections are for

- **Decks** are the walkable navigation mesh of the ship: convex polygons (triangles, a few quads) with a plane each. A corner's type
  is the type of the edge from it to the next corner: in both oracles every `hard` edge belongs to one piece only (the deck's border,
  rails and hull) and every `clear` edge is shared with another piece (100%, 112 + 272 edges on `attila_dromon_large`). Men walk across
  clear edges and stop at hard ones. Deck heights are inside the hull: `attila_dromon_large` decks are at y 1.838..5.158, the hull model
  y -3.61..6.06.
- **EF lines** are where the men of the ship's unit stand. **ASSUMPTION** (from the data, not the code): each line is a rank, men spread
  along it from one end to the other, facing `direction`; the purpose says who (Marines, the firing positions for missile men, Boarding
  lines along the gunwales facing outwards, Disembark lines). On `attila_dromon_large` the 47 Marines lines run port to starboard at
  y 1.854, 1.6 cm above the main deck (1.838), facing the bow; so placing a unit's men on a ship is filling its Marines lines (or the
  firing lines) in order, not sampling the deck polygons. The deck mesh is where they walk once they leave a line (boarding, fighting).
- **Engine_placement / Secondary_engine_placement** lines place the primary and secondary naval weapon: on `attila_dromon_large` one
  each, 3.63 m wide across the ship at the bow (z 20.26, y 3.215) and the stern (z -22.44, y 4.484), facing the bow. **ASSUMPTION** the
  weapon model goes at the line's midpoint, facing `direction`.
- **Custom positions** are the oar sockets (one oar model per socket, from the row's oar columns; **ASSUMPTION** `_l1`.. `_l3` pick
  lower / middle / upper) and the flag.
- **Damage parts** are the hit panels of the ship's damage models (`<part>_dm1`, `_dm2`); **collision** the triangles ships and
  projectiles hit; **bobbies** the buoyancy simulation's spheres.

## Which weapon model goes on the EF lines

The table names no weapon model. The chain (Attila kit DB): `naval_units.ship` -> `ship_dbs.key`, whose `model` is the `models_naval`
key (`att_shp_dromon_siphon` -> `att_shp_dromon_large`); `naval_units.primary_naval_weapon` / `secondary_naval_weapon` ->
`naval_weapons.key`, which has a `type` (`naval_weapons_enums`: autonomous_engine, fighting_towers, fire_pots, grappling_engine) and
either `models_entity_weaponry` -> `models_entity_weapons` (`model_file`, e.g. `fire_siphon_dromon`, and `logic_file`
`RigidModels/naval/naval_weaponry/fire_siphon_dromon/fire_siphon_dromon.meta_data`) or `autonomous_engine` ->
`battlefield_engines_autonomous` (`att_shp_scorpio`; its model through `battlefield_engines.model`). **ASSUMPTION** the primary weapon
goes on the `Engine_placement` line and the secondary on `Secondary_engine_placement`.

## Unknown

The pipe's second u32, the custom position's u32 (1) and vector (0, 1, 0), the camera's last float, the
trailing count's meaning. The reader keeps them raw and fails on a non-zero cannon or trailing count, whose layouts no row shows.
