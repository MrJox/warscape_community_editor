# River AI hints in the game (2026-11-25, Attila's `empire.retail.dll`)

What Attila does with the three hints the kit's river tiles carry: `AIH_BRIDGE_CROSSING` (TEd: *river_limes_crossing*),
`AIH_RIVER_EDGE` (*river_limes_edge*) and `AIH_COVER_LINE` (*river_limes_covering_fire_line*). Static reading only (Ghidra project
`~/wce_re/game_proj`, whole-dll decompile in `~/wce_re/scratch/river_hints/full`, scripts in `~/wce_re/scripts`, incl. the new
`DecompileRange.java`). Addresses are VAs (image base 0x10000000). Nothing here is checked in game.

The kit's layout of these hints (per crossing tile: 1 crossing, 2 river edges, 4 cover lines) is in
[docs/plans/rivers.md](../plans/rivers.md), "AI hints at rivers".

## Numbers

The game's hint names table (0x11d32790, 43 entries) is TEd's order, so the compiled `ai_hint_polyline_marker.markers` numbers are the
game's: `AIH_BRIDGE_CROSSING` 18, `AIH_RIVER_EDGE` 20, `AIH_COVER_LINE` 29. A second table (0x11d326e0) holds TEd's display names.

## Loading: the battle's hint store

At battle start (`FUN_10137ef0` area) the game builds a hint store (0x11c bytes, ctor `FUN_1022f770`, vtable 0x11b19944) and keeps it at
`world+0xc4 -> +0x2530`. `FUN_10287560` fills it: every hint polyline of the battlefield goes through these filters, then into one list
(a 12-byte vector: capacity, count, data) per kind, by a switch on the type (jump table at 0x102878a8 / 0x102878e8):

| Type | List | Type | List |
|---|---|---|---|
| 1 DEFENSIVE_HILL (> 2 points) | +0x14 | 31 AMBUSH_MELEE_LINE | +0x74 |
| 8 AMBUSH_FOREST (> 2 points) | +0xa4 | 32 AMBUSH_MISSILE_LINE | +0x68 |
| 17 FORT_APPROACH, 39 DEFAULT_DEPLOYMENT_LINE | +0x08 | 33 AMBUSH_CAVALRY_LINE | +0x80 |
| **18 BRIDGE_CROSSING** | **+0x44** | 34 AMBUSH_DEFENDER_INITIAL_PATH | +0x8c |
| 19 BEACH_LANDING | +0x2c | 35 FORT_PERIMETER | +0x20 |
| **20 RIVER_EDGE** | **+0x50** | 36 BEACH_LANDING_FORT_APPROACH | +0x38 |
| **29 COVER_LINE** | **+0x5c** | 37 AMBUSH_ASSAULT_AREA / 38 AMBUSH_FIREBALL_LINE | +0x98 / +0xb0 |

Every other type (the deprecated ones, the siege graph's) is dropped here. The filters:
- at least 2 points, and a length of at least 0.01 (`FUN_10550bb0` against 0x11b0ef84);
- the playable area (the world's rectangle at +0x84 .. +0x90): types 1, 18, 29, 31, 32, 33, 34, 37 and 38 (`FUN_101e1e80`) are kept only
  if **every** segment is inside it; the others (20 river edge among them) if **any** segment is (`FUN_101dfa30` with the tests
  `FUN_101c1c90` / `FUN_101c1ae0`).

So a crossing or cover line that pokes out of the playable area is thrown away; a river edge only needs one segment inside.

## Who reads the river lists

Searched: every function reading the store through `+0x2530` (direct, through a variable, and the lists passed by address), the store's
own virtual methods, functions near its constructor and its add method (`FUN_102a2c00`), getters returning the store. Readers found:

- **`AIH_BRIDGE_CROSSING` (+0x44) and `AIH_COVER_LINE` (+0x5c): only the AI debug overlay.** `FUN_105efdf0` draws the store's lists, one
  toggle per group of the debug menu built by `FUN_105be570` ("Battle AI Debug Display/Display Hint Lines/..."); *River or Limes
  Battle* (debug +0x88) draws +0x50, +0x44 and +0x5c. Nothing else in the dll reads either list. In Attila they look like leftovers:
  the AI does not use them.
- **`AIH_RIVER_EDGE` (+0x50): two readers, both ambience, not AI.**
  - The battle audio's water grid (built by `FUN_10260ce0`, called from `FUN_10241140`): the job `FUN_1024f710` sets a flag on every
    grid cell whose water is above the ground (`FUN_103e0b90`): **8 if the battlefield has at least one river edge, 4 if it has none**.
    The audio listener analysis `FUN_104d32d0` samples that grid around the camera (`FUN_104d5800`, by flag mask) for the Wwise
    parameter `amount_water_under_listener` (0xa1e0c065), and `FUN_104e5f20` picks the water wildlife sounds (`battle_water_wildlife`,
    `battle_seagull`, `battle_crow`...) from the results. Names: FNV-1 hashes matched against the strings of the dll and `sound.pack`.
    So a river edge anywhere tells the ambience that the map's water is a river.
  - `FUN_102ee710` (run once by `FUN_10320470`, which then updates every 20 ticks): one field object per river edge (and per fort
    perimeter, and per item of `world+0x2418` with its +0x3d flag), vtable 0x11b222c4: the nearest point on the line within a radius
    200, a force falling off as 1 - (d / 200)^3, strength 3, and random points near the line. It sits with the ambient fauna code
    (`battle_flock_startled` 0x10314650, `battle_whale_spout` 0x10320560): ASSUMPTION (not traced to the caller) that these are the
    ambient wildlife's places and steering fields.

## Battle types

- Custom battle types (`FUN_113049d0` / `FUN_1158f750`, the `battle_types` table): 5 `river_crossing_battle` (*River Crossing Battle*,
  shown in custom battle) and 6 `limes` (*Limes*); campaign battle types (0x11d31f20): `land_bridge` (*Bridge Battle*).
- The AI's task types (0x11d28e60) have six crossing tasks: 16 `defend_crossing`, 17 `block_crossing`, 18 `assault_crossing`, 19
  `outflank_crossing`, 20 `seize_crossing`, 21 `dtt_move_between_crossings`. Since no AI code reads the crossing or cover hints, these
  tasks must find their crossings another way (the pathfinder's bridges and shallow water?): **not traced**.
- `battle_bridge_subculture_jcts` (subculture -> `simple_bridge` / `refined_bridge` building keys) is empty in vanilla Attila and in TDD.

## What it means for the editor

- `AIH_BRIDGE_CROSSING` and `AIH_COVER_LINE` change nothing in Attila's battles (as read here). Writing them is harmless and keeps the
  kit's layout, which may matter to Rome 2 or Thrones of Britannia (not checked: their dlls are other builds).
- `AIH_RIVER_EDGE` matters for the sound: a map with rivers but no river edge gets the still-water ambience. The editor's rivers already
  export their edges (docs/plans/rivers.md).

## Not traced

- How the crossing tasks find their crossings, and how a `land_bridge` / `river_crossing_battle` picks its map and deployment.
- What flag 4 against 8 changes in each sound (the four `FUN_104d5800` queries' masks), and the field objects' consumer.
- Whether Rome 2 and Thrones of Britannia read the crossing and cover lists.
