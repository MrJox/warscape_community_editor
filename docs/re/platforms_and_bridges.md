# Building platforms and bridges in the game (2026-11-08, Attila's `empire.retail.dll`)

What the game does with a building's platforms (`_tech.cs2.parsed`, per piece and destruct level) and what makes a building a bridge,
traced in Ghidra (project `attila` in `C:\Users\frodo\ghidra_work`, see the memory notes). Addresses are RVAs (image base 0x10000000;
`FUN_1xxxxxxx` = RVA `0xxxxxxx`). Everything here was read in the decompiled code unless it is under **Not traced**; nothing in this
file comes from testing in game.

Why: a generated road bridge (`wce_walls::bridge`, pieces bent from `bridge_wood_2` / `bridge_wood_2_end`) is not crossable in game,
the same kit pieces placed by hand are, and hours went into changes tested blind. This file is what is known for sure, and what a
checker can test without launching the game.

## Reading the logic file

The parser is `FUN_110f61c0` (called from `FUN_110e62b0`, which builds the `rigidmodels\...` path; the `rigidmodels\` string's user is
at 0x10e62eb).

- The first u32 is the file's version (`local_524`, read by `FUN_10630a00(ptr, 4)`).
- Per destruct level, platforms are a count and then that many 0x1c-byte records, into the level's vector at +0x84 (capacity), +0x88
  (count) and +0x8c (data).
- A record holds:
  - the normal (vec3, read by `FUN_1168a490`) at 0x0–0x8;
  - a vertex count and the vertices (vec3 each), as a vector at 0xc (capacity) / 0x10 (count) / 0x14 (data);
  - three bytes, read only if the version allows them:
    - 0x18 = `flag_1`, read if version ≠ 0;
    - 0x19 = `flag_2`, read if version > 4;
    - 0x1a = `flag_3`, read if version > 5.

  This is rpfm's order (`cs2_parsed/versions/v13.rs`: normal, vertices, `flag_1`, `flag_2`, `flag_3`).
- After the platforms, the parser reads +0x90 (u32) and +0x94. Another vector of 0x38-byte elements sits at +0x10c / +0x110 / +0x114.
  Neither was identified.

## The building's flags (building info +0xfc)

`FUN_10632440` builds a placed building's info object; its `param_1[0x3f]` is the dword at +0xfc. It takes:

- `param_3`: the `building_list` record. +0xc and +0x14 are x and z, +0x20..+0x2c the quaternion (the yaw is taken from it by `atan2`),
  +0x7c is kept.
- `param_4`: the building's `battlefield_buildings` row. Its category is `**(param_4 + 0x10)`, numbered by `FUN_110e0b80`:
  - armoury 0, barracks 1, boulder 2, **bridge 3**, church 4, command_HQ 5, farmhouse 6;
  - generic / house 9, incidental 0xb, ruins 0xc, rural 0xd, townhall 0xe, warehouse 0xf, windmill 0x10, beacon 0x12;
  - fort_tower 0x13, fort_wall 0x14, vaultable 0x15, **ground_platform 0x16**, key_building_* 0x17–0x1b, coastal_battery 0x1c,
    unbreachable_fort_wall 0x1d, anything else 0x1e.
  - 7 and 8 are two categories whose strings were not recorded.

The bits that matter for movement are all set from the category:

| Bit | Set when (`FUN_10632440`) | Read by |
|---|---|---|
| 0x2 | fort_wall, 8, unbreachable_fort_wall, fort_tower; or any piece level whose entry +0xb0 ≠ 0 | not traced |
| 0x4 | fort_wall, 8, unbreachable_fort_wall; fort_tower only if a piece level's entry +0x88 ≠ 0 | ground snap tolerance (below) |
| **0x8** | **category == bridge (3)** | ground snap tolerance; the pathfinder's bridge cost (below) |
| 0x10 | incidental or boulder | not traced |
| **0x20** | **category == ground_platform (0x16)** | the platform constructor: every platform of the building is "ground" |
| 0x200 | none of 0x8 / 0x20 / 0x80, not incidental, row +0x4d == 0, and (no 0x4, or 0x40, or fort_tower) | not traced; seems to be the building as an obstacle |

`FUN_10632440` also reads other bits from the `building_list` record (bytes +0x50, +0x53, +0x54, +0x78 → bits 18, 20, 21, 22) and the
row (+0x3c bit 0 → 0x100). Those are not about movement.

**So a building is a bridge only through its `battlefield_buildings` category.** Nothing in the logic file and nothing in
`building_list` makes it one.

## Platform objects

`FUN_1068cf20`, called from the building setup `FUN_10634940`, walks every piece (`building + 0x3c` → count +0x64, array +0x68) and
every platform record of each level. For each one it:

- allocates a 0x84-byte object;
- moves the vertices and the normal to the world by the instance's matrix (`*(building + 0x38)` +0x74..+0xa0);
- stores the plane, with d = −(vertex 0 · n) / |n|, so the plane goes through the first vertex;
- calls `FUN_10635360(this, building, &plane, &worldVerts, levelActive, levelData, record[0x18], record[0x19], 0)`.

`levelActive` is true when the level's index equals the building's current level (byte +0x159) and a virtual check passes.

The constructor `FUN_10635360` (vtables 0x1b5271c, 0x1b52768, 0x1b19760) keeps `this + 0x78` = building and `this + 0x7c` = level. It
packs these flags into byte +0x82:

- **bit 2 = `flag_1`**;
- **bit 3 = `flag_2` OR the building's 0x20 bit (ground_platform category)**;
- bit 5 = its 9th argument, always 0 from `FUN_1068cf20`. **`flag_3` (record byte 0x1a) is not passed in, so building platforms do not
  use it** on this path. No other reader of 0x1a was found (not searched exhaustively).

It also sets `this[0x18]` = 8 for windmill (0x10) and key_building 0x17, and 9 for 0x1b.

### What the platform answers (vtable 0x1b5271c)

| Slot | Function | Returns |
|---|---|---|
| 0x30 | `FUN_1067b620` | bit 3: "ground" (`flag_2`, or a ground_platform building) |
| 0x34 | `FUN_10697ad0` | sets bit 4 (0x10); its caller was not traced |
| 0x38 | `FUN_1065f100` | bit 3 or bit 4 |
| 0x3c | `FUN_1065ee50` | the ground snap tolerance (below) |
| 0x40 | `FUN_10676910` | the short at +0x80 |
| 0x48 | `FUN_106882e0` | bit 2: `flag_1` |

Other walkable surfaces answer the same slots with constants (vtables 0x1b1a094, 0x1b213f4, 0x1b2a420, 0x1b28ce4, 0x1b2a470,
0x1b2a4c0):

- slot 0x30 is 0 (`FUN_100c07a0`), or 1 for class 0x1b213f4 (`FUN_100c0850`);
- slot 0x48 is always 1;
- slot 0x3c is `FUN_10265530`, which returns 0.1.

So `flag_1` is a property every non-building surface has and that building platforms have only when the flag is set. What it allows
was not traced.

### Ground snap tolerance (slot 0x3c, `FUN_1065ee50`)

The building's flags are `*(*(this + 0x78) + 0x38) + 0xfc` (`FUN_10652490`). The tolerance is:

- 1.0 if bit 0x4 is set (walls);
- otherwise, if bit 0x8 is set (bridges), the tweak `BATTLE_BRIDGE_GROUND_SNAP_TOLERANCE`. Its object is 0x1230a060, registered at
  RVA 0x56350 in `BuildingPlatform.cpp`, default **1.0**;
- otherwise **0.1** (`DAT_11afbd60`).

The units are presumably metres. The tolerance compares something to the ground, but what it compares and where it is used were not
traced (see below).

## The pathfinder and bridges

`FUN_1033bc90` (path search) finds the nav region at a position (`FUN_1038c700` → `FUN_103f3aa0`). If the region's owner (+0x24, slot
0x20) is a platform of a building with bit 0x8, the step's cost multiplier becomes `min(cost, PATH_SEARCH_BRIDGE_MAX_COST_MULTIPLIER)`
(`FUN_1032b7d0`; tweak object 0x12059718, default **0.4**). The bridge category makes paths prefer the bridge. It does not make the
bridge reachable.

`FUN_103f3aa0`, the position → region lookup:

1. tests the position against the bounds at `*(*(p + 0x38) + 0x38)` +0x74..+0x80;
2. finds a grid cell (scale +0x24, offset +0x10, max +0x14);
3. takes the cell's candidate region ids (< 0xfffe, `FUN_103b64d0`), mapped through `*(p + 0x3c) + 0x40`;
4. keeps the first candidate that passes `FUN_103f25e0(region, …)` and `FUN_1169dfe0(pos, 0)`;
5. otherwise falls back to a loop over `*(p + 0x3c) + 0x3c`.

Neither test function was decompiled.

Other tweaks: `enable_falling_off_platform` (object 0x12058a78, default false, used in `FUN_10375bc0`).

## What a generated bridge must satisfy (traced rules only)

1. **Category `bridge`** in its `battlefield_buildings` row. Without it there is no 1.0 snap tolerance (0.1 instead) and no
   bridge cost preference.
2. **A logic file version above 5**, or `flag_2` / `flag_3` are not read (above 0 for `flag_1`).
3. **The platforms in the level the building starts at**: only the current level's platform objects are active.
4. **Each platform's normal agrees with its corners.** The engine keeps a plane through vertex 0 with the stored normal, so a normal
   that does not fit the other corners describes a different surface from the outline.
5. `flag_1`, `flag_2` and `flag_3` are **not required**: the kit's `bridge_wood_2` and `bridge_wood_2_end` have all three false, and a
   bridge's platforms are not "ground" unless `flag_2` is set.

## Our generated bridge against these rules (dry-run pack `%TEMP%\wce_scratch\lay3\sp`, 2026-11-08)

Pieces `wce_assembly_kit_wce_bridge_test_battlefield_r0_00` (deck, from `bridge_wood_2`), `_r0_01` and `_r0_02` (ends, from
`bridge_wood_2_end`):

1. The pack's rows for each key carry the category `bridge`, copied from the base's row (`db::copy_rows`). ✓
2. Version 11, the same as the kit pieces'. ✓
3. One platform in destruct level 0 of each, like the kit's. ✓
4. Normals fit the corners within a millimetre: end `r0_01` has n = (0.004, 0.894, −0.447), and n·v is −0.449 / −0.448 at its corners;
   the deck has n = (0, 1, 0) with every corner at y 0. ✓
5. All three flags false, as the kit's. ✓

**It breaks no traced rule.** What still sets it apart from the hand-placed kit bridge (facts, none of them traced to a rule):

- the deck's platform has 24 corners (the kit's 4) and its outline / footprint ring 30 (the kit's 4); the ends have 6 and 12 (the
  kit's 4 and 4);
- the deck hull has 280 triangles (the kit's 12);
- the pieces' origin is at the deck (platform local y 0, outlines −1), where the kit's is on the ground (platform local y +1);
- the bending is baked into the meshes, with identity rotations.

The cause is in what is not traced yet: how platforms join the terrain and each other.

## Not traced

- **How platform edges join the terrain and each other in the nav graph**: which edges become exits, how heights are compared,
  whether the slot 0x3c tolerance is that comparison, and any limit on corners, convexity or collinear corners. This is the piece
  that decides "can units get on and off". To find next: who reads the 0x84-byte platform objects' vertex vector and calls slot
  0x3c on them. The vertex vector is in the level entries at `building + 0x154`, 0x9c bytes each, with the platform vectors at
  +0x38 / +0x3c / +0x40. Also the region builder behind `*(p + 0x3c) + 0x3c/+0x40` in `FUN_103f3aa0`.
- What `flag_1` allows (slot 0x48), and what sets bit 4 (slot 0x34).
- `flag_3`'s use anywhere (not on the building platform path).
- How collision outlines and `orange_thingies` (`vertex_type`) take part in movement. The in-game observation in
  [buildings.md](buildings.md) is that extending one without the other kept units off the difference.
- `AIH_BRIDGE_CROSSING` (string at file offset 0x1bab290): its users were not followed.
- Water under bridges.

## Spec: a bridge / platform checker

A report, per building of a tile or pack, that tests only traced rules and lists the deviations from the game's own pieces separately,
labelled as untraced. It could extend `crates/wce_walls/examples/bridge_check.rs`, which already decodes the logic files.

Errors (traced):

- **E1** A building named as a bridge (made by `wce_walls::bridge`, or with "bridge" in its base key) whose `battlefield_buildings`
  category is not `bridge`.
- **E2** A logic file version ≤ 5 with any `flag_2` / `flag_3` set (they would not be read).
- **E3** A platform whose normal is off its corners: max |n·(vᵢ − v₀)| / |n| above 0.01.
- **E4** No platform in destruct level 0 of a bridge piece.
- **E5** A degenerate platform: fewer than 3 corners, or a zero normal.

Warnings (deviations from the game's pieces, no rule traced):

- **W1** A platform with more than 6 corners, or not convex. In TDD's 2530 fort platforms none had more than 6 corners and all were
  convex.
- **W2** Collinear corners on a platform.
- **W3** A platform not wound clockwise in x/z.
- **W4** Neighbouring platforms that share an edge only partly (no shared corners).
- **W5** A collision outline and `orange_thingies` with different corners.
- **W6** For each platform edge that is not shared with another platform (an exit), the height gap to the compiled ground (`hf_height_map`)
  along the edge. Show it against 0.1 and against 1.0 (the snap tolerances), without a verdict until the snap comparison is traced.

Output: one line per finding, with building key, piece, level, platform index and the numbers. Example:
`wce_..._r0_00 piece0 level0 platform0: W1 24 corners`.
