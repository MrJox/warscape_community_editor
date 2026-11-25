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

## How the AI finds crossings (2026-11-25, partial)

What is known for sure:
- **Bridges**: the pathfinder knows a bridge by its building's `battlefield_buildings` category `bridge` (building flag 0x8 at +0xfc,
  set by `FUN_10632440`): a path step over one of its platforms has its cost capped by `PATH_SEARCH_BRIDGE_MAX_COST_MULTIPLIER` (0.4,
  `FUN_1033bc90` -> `FUN_1032b7d0`), so paths prefer bridges ([platforms_and_bridges.md](platforms_and_bridges.md)). Other reads of the
  flag: the ground snap tolerance (`FUN_1065ee50`), movement checks (`FUN_10402b30`, `FUN_10491370`).
- **Water**: the ground types are `GT_FOREST` 0 .. `GT_DEEP_WATER` 6, `GT_SHALLOW_WATER` 7, `GT_ROAD` 8 ... (0x11d349a8). The tile
  loader (`FUN_1184eeb0`) reads `ground_types.dds`; where the pathfinder turns 6 / 7 into impassable / wadeable was **not found** (the
  wading animations' code `FUN_10407e90` has no ground type test). `GROUP_DROWNING_HEIGHT` (tweak 0x122afaf0, default -1.0) is read
  only by the ship audio (`FUN_104d4440`).

What was looked for and **not found**: any code that creates or tests the crossing tasks (16..21). Searched: compares of a task type
against 16..21, factory switches allocating per case (none in the AI's range 0x10580000..0x10800000), constructors storing a type
constant, tiny type getters. The task names table is read only by the task log dump (`FUN_1060ae60` from `FUN_105d2760`). The calls
`FUN_10594ea0(0x10 / 0x14, ...)` are another enum (a sorted list of up to 51 scored entries), not tasks.
Correction to a first reading: `FUN_10632440` and `FUN_10253f00` switch on the **building category** (`**(row + 0x10)`: 3 bridge,
0x14 fort wall, 0x16 ground platform ...), not on the task type.

So: the AI does not find crossings through hints; whether its crossing tasks run at all in Attila, and if so from what (the
pathfinder's graph over bridges and shallow water is the likely source), is open. Next: an in-game look (a custom *River Crossing
Battle* on a tile with a bridge and a ford, with the debug overlay), or the pathfinder's ground type reads.

## The AI debug overlay cannot be turned on in retail (2026-11-25)

The debug menu items ("Battle AI Debug Display/...", "Views/Battle Debug/Water Heights", ...) are objects of a toggle class (base ctor
`FUN_100def40`, vtable 0x11b03768; the bool toggle's vtable 0x11afb0cc: flip `100c4120`, get `100c07b0`, the bool's address at +0x3c).
After building them, `FUN_105be570` (and `FUN_102cfba0` for the Views ones) hands each to `FUN_100e20f0`, the menu's register call:
**an empty function in retail** (`ret`, 3 bytes). Nothing else sets the flags: the overlay's master flag (`owner + 0x242c`, the owner
being the object at `+0xc4` of the game's root) starts at 0 and has no other writer. No debug menu strings, commands or key actions are
in the dll. Showing the overlay would take writing those bytes in the running process (master +0x242c, *River or Limes* +0x24b4).

## Shallow against deep water (2026-11-25)

- **The ground type map is the only thing that tells them apart.** `GT_DEEP_WATER` 6 and `GT_SHALLOW_WATER` 7 come from the tile's
  `ground_type_map.png` as painted (BOB copies them into `ground_types.dds`; black pixels get the type from the textures). BOB derives
  nothing from the water: the kit's `river/2x2_lb_rb` has deep water painted over its river (5.5 % of the map raw, 11 % of
  `ground_types.dds` as 6), `river_shallow/2x2_lb_rb` shallow (7), `river_ford/2x2_lt_rt_ford` deep with a painted shallow strip (the
  ford, 0.3 % raw); `lakes/10x10` has **no water types at all** (raw all black; compiled grass 1, mud 2, sand 3 under its `water_plane`).
- **What the types do** (databases): `ground_type_to_stat_effects` speed and charge x0.4 in deep water, x0.5 in shallow;
  `unit_ground_type_movement_modifiers` speed 0.4 deep, 0.65 shallow, every category. TDD's `rom_*` tables (in its patch pack) make it
  0.1 deep and 0.4 / 0.45 shallow, and give deep water the `terrain_ocean` cursors. Also the footstep audio materials (`AM_DEEP_WATER`
  / `AM_SHALLOW_WATER`, a separate list) and the ambient sound's water grid (above).
- **Water depth** (water height `FUN_103f2870` minus ground `FUN_103bca80`, the `hf_water_map.data` against the terrain): its users
  read it for the surface height (`max(ground, water)`: floating things, effects, the audio grid, debug drawing). No depth threshold for
  movement was found, and there is no wading or depth tweak (`GROUP_DROWNING_HEIGHT` is ship audio). So, as read: **deep water is a
  slow ground type, not an impassable one**, and water over ground typed grass (a lake's plane) is grass to the units.
  ASSUMPTION to check in game: whether units can walk into a kit deep river at all (nothing found stops them).
- The kit's deep river tile has an empty `terrain_outlines.xml` and an all-white `nogo.png`: nothing else blocks its water either.

## What it means for the editor

- `AIH_BRIDGE_CROSSING` and `AIH_COVER_LINE` change nothing in Attila's battles (as read here). Writing them is harmless and keeps the
  kit's layout, which may matter to Rome 2 or Thrones of Britannia (not checked: their dlls are other builds).
- `AIH_RIVER_EDGE` matters for the sound: a map with rivers but no river edge gets the still-water ambience. The editor's rivers already
  export their edges (docs/plans/rivers.md).

## Not traced

- Whether the crossing tasks run (above), and how a `land_bridge` / `river_crossing_battle` picks its map and deployment.
- How deep / shallow water ground types limit movement.
- What flag 4 against 8 changes in each sound (the four `FUN_104d5800` queries' masks), and the field objects' consumer.
- Whether Rome 2 and Thrones of Britannia read the crossing and cover lists.
