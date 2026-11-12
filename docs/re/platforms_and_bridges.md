# Building platforms and bridges in the game (2026-11-08, Attila's `empire.retail.dll`)

What the game does with a building's platforms (`_tech.cs2.parsed`, per piece and destruct level) and what makes a building a bridge,
traced in Ghidra (project `attila` in `C:\Users\frodo\ghidra_work`, see the memory notes). Addresses are RVAs (image base 0x10000000;
`FUN_1xxxxxxx` = RVA `0xxxxxxx`). Everything here was read in the decompiled code unless it is under **Not traced**; the only things from
testing in game are marked "in game".

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

It is the most the ground's height may differ from the platform's at an edge's two corners for the ground to link to that edge
(`FUN_10354b40`, in "How units get on and off" below). Units are world units, as the building's matrix leaves them.

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

`FUN_103f25e0(region, kind)` only filters by the region's type (+0x18): kind 1 refuses type 1, kind 2 refuses type 0, kind 0
takes any. `FUN_1169dfe0` (point in the region's polygon) was not decompiled.

Other tweaks: `enable_falling_off_platform` (object 0x12058a78, default false, used in `FUN_10375bc0`).

## How units get on and off (the nav graph)

Traced in the nav graph builder `FUN_10337530` (2026-11-08). The pathfinder works on regions (`param_1[0x10]`, count `param_1[0xf]`).
Each region has:

- an edge ring at +0x8 (and +0x28 for regions made from a surface);
- a type at +0x18;
- an owner surface at +0x24, which is 0 for ground regions;
- a flag at +0x5c;
- up to four links at +0x2c..+0x3c, with more in an overflow vector.

Units cross only along links. The builder makes them in five steps.

### 1. The ground triangulation and the buildings' collision outlines

The ground regions come from a constrained triangulation, the world's object at +0x25a8. It is created in `FUN_10136e70` by
`FUN_11663fc0(bounds, 0.1, 1, 16.0)`; `FUN_10337530` reads it back with `FUN_116641a0`. Segments also go into it from:

- the battle's outlines (`FUN_101369c0`);
- the wall lines of buildings with the wall flag (building info +0xe0 / +0xe8 / +0xec, filled by `FUN_10653590`).

`FUN_1016d0a0` collects those segments, and `FUN_10337530` inserts them with `FUN_1166fd00`.

**A building's collision outlines become holes in that triangulation**, one obstacle polygon per outline:

1. `FUN_10655c60` goes through every piece and level of the building.
2. The outline's name decides how it is handled:
   - the plain path takes outlines whose name contains neither "outline" nor "gate";
   - "outline" and "gate" (and "gate_ajar") get their own paths.
3. `FUN_1066f1a0` takes the outline's corners in order, dropping only a last corner that repeats the first. It gives each corner a type
   from the name: "hard" 0, "soft" 1, neither 2; corners of a "secondary" outline go to a second list.
4. `FUN_1065fc10` drops a polygon equal to one already there. It then builds the obstacle, `FUN_10632ef0` → `FUN_10333d50`, with the
   literal type 0 that `FUN_10632ef0` passes.
5. For any type but 2, `FUN_10333d50` inserts the polygon's corners, as they are, into the ground triangulation: `CALL FUN_11665a20`
   at RVA 0x333f9e, with ECX = `[world + 0x25a8]` (RVA 0x333f93).

The corners are not simplified: collinear corners stay as corners of the triangulation. Along a building, then, the ground's edges run
between consecutive corners of its collision outlines, unless another outline or segment crosses there.

`orange_thingies` were not seen read on this path (see "Not traced").

### 2. Platform regions

Next comes one region per walkable surface in the world's list at +0x2fc / +0x300 whose slot 0x30 ("ground") is false: `FUN_1033a4a0`,
called from `FUN_10337530`.

**A `flag_2` platform, or any platform of a `ground_platform` building, gets no region of its own.** Units walk the ground there.

The surface itself (`FUN_1033c580`, the base of the platform object, called from `FUN_1033e000` / `FUN_10635360`) keeps:

- the plane (a, b, c, d) at +0x34..+0x40;
- an edge ring of the platform's corners in file order (`FUN_1033c840` / `FUN_1033c8c0`, a linked list with no size limit).

**Each corner's height is recomputed from the plane**: y = −(c·z + a·x + d) / b. The file's y values only count through the plane,
which goes through vertex 0 with the file's normal.

`FUN_1034d3f0` gives each edge a clearance (+0x60): half the distance to the nearest other edge across the polygon (`DAT_11afbd68`
= 0.5). This is the width units fit through; nothing in it rejects a shape.

### 3. Candidate neighbours

Every region's corners go into a point grid (`FUN_1168c150`). For each region R:

- if R's type is 2, every ground region is a candidate;
- otherwise, the candidates are the regions with a corner inside a ±0.5 box (`DAT_11afbd68`) around one of R's corners
  (`FUN_11699e60`, then sorted by `FUN_1032d3b0`).

### 4. Platform ↔ ground: `FUN_10354b40`

It is called from `FUN_10337530` for a platform region R and each ground candidate C that has C+0x5c == 1. The platform owner's slot
0x10 is 0 (`FUN_100c46b0`) and slot 8 is non-zero (`FUN_1017ad40`), so building platforms take this path.

The tolerance is R's surface slot 0x3c (`FUN_1065ee50`): **1.0 for bridges and walls, 0.1 otherwise**.

For each edge r0→r1 of R, with heights on R's plane, and each edge c0→c1 of C, with heights on the ground at those points
(`FUN_1034b8f0` → `FUN_1034b920`), the edges link when **all four** of these hold:

- |c0 − r1| ≤ 0.01414 in x/z (squared ≤ `DAT_11b2b790` = 2.0e-4);
- |h(c0) − hR(r1)| ≤ tolerance;
- |c1 − r0| ≤ 0.01414 in x/z;
- |h(c1) − hR(r0)| ≤ tolerance.

So the ground edge must be the platform edge itself, run the other way, **corner for corner**. An edge that only overlaps the platform
edge, or that ends at a corner the platform edge doesn't have, does not link.

When the edges match, `FUN_1034f9a0` gates on the two regions' types and +0x84 flags: type 0 never links with type 1. Then
`FUN_10339b80` makes the link both ways. `FUN_10354e20` also runs, setting +0x64 and clearing +0x44 on an edge; which edge it is
was not resolved.

**Then the function returns: one link per (platform, ground region) pair, the first matching edge in the platform's ring order.**

### 5. Platform ↔ platform: `FUN_10353950`

This runs for R and each later candidate, both with a surface. For each edge pair:

- **reversed corners within 0.01414 in x/z**;
- **heights on each one's own plane within 0.1**: `DAT_11afbd60`, fixed, *not* the bridge tolerance.

A match makes a type-3 link both ways. A flag then stops further edge links for the pair, so **one shared edge per pair of platforms**
is linked. Corners that coincide get `FUN_10354aa0`.

For pairs where one region has no surface (the ground), the function's second branch does the following:

- it matches reversed edges with 0.01 per coordinate (`DAT_11b0ef84`);
- for anti-parallel, partly overlapping edges, it compares the clipped overlap with the same 0.01;
- it logs "PATHFINDING: we have 2 lines very close … is it our intention to join them up" for near misses within 0.05
  (`DAT_11b04044`).

### What a link needs, in short

A platform edge is a way on and off the ground only if the ground's triangulation has an edge with exactly its two corners: an edge of
the building's collision outline, between two consecutive outline corners. The ground's height at both corners must also be within
the tolerance (1.0 for a bridge) of the platform's plane there.

The kit's `bridge_wood_2_end` has the following, in local units:

- platform corners (−5, 0, −2), (−5, 1, 0), (5, 1, 0), (5, 0, −2);
- collision outline `piece01_destruct01_hard01` with corners (5, 0, 0), (−5, 0, 0), (−5, 0, −2), (5, 0, −2).

Its foot edge (5, 0, −2)→(−5, 0, −2) is the outline's edge (−5, −2)→(5, −2), and it is at ground height, so the ground links to it.

## What a generated bridge must satisfy (traced rules only)

1. **Category `bridge`** in its `battlefield_buildings` row. Without it the tolerance is 0.1 instead of 1.0 and there is no bridge
   cost preference.
2. **A logic file version above 5**, or `flag_2` / `flag_3` are not read (above 0 for `flag_1`).
3. **The platforms in the level the building starts at**: only the current level's platform objects are active.
4. **Each platform's normal agrees with its corners.** Every corner's height is taken from the plane through vertex 0 with that normal.
5. **Every edge units should step on or off by (the ramp's foot) runs between two consecutive corners of the piece's collision
   outline** (a plain one, its name without "outline" / "gate"). Two conditions:
   - each corner within 0.0141 in x/z of the outline's;
   - the ground's height at both within 1.0 of the platform's plane.
6. **Joints between platforms are edges with the same two corners**, run in opposite directions: within 0.0141 in x/z, and within
   0.1 in height on each one's plane. Only one such edge per pair of platforms is linked, so a joint split into several edges is
   linked along one of them only.
7. `flag_1`, `flag_2` and `flag_3` are **not required**: the kit's pieces have all three false. A `flag_2` platform has no region of its
   own, so units walk the ground there.

## Our generated bridge against these rules (dry-run pack, 2026-11-08)

The pack is `%TEMP%\wce_scratch\lay3\sp\build\launch\dw89pnmx\wce_launch.pack`. It has three pieces:
`wce_assembly_kit_wce_bridge_test_battlefield_r0_00` (deck, from `bridge_wood_2`), and `_r0_01` and `_r0_02` (ends, from
`bridge_wood_2_end`). The logic files were read out of the pack itself.

1. The pack's rows for each key carry the category `bridge`, copied from the base's row (`db::copy_rows`). ✓
2. Version 11, the same as the kit pieces'. ✓
3. One platform in destruct level 0 of each, like the kit's. ✓
4. Normals fit the corners within a millimetre: end `r0_01` has n = (0.004, 0.894, −0.447), and n·v is −0.449 / −0.448 at its corners;
   the deck has n = (0, 1, 0) with every corner at y 0. ✓
5. **Broken.** End piece `r0_01`:
   - platform corners: (−5.358, −1, −1.045), (−5.375, 0, 0.954), (−0.008, 0, 1.000), (5.358, 0, 1.045), (5.375, −1, −0.954),
     (0.008, −1, −1.000);
   - its foot is two edges: (5.375, −0.954)→(0.008, −1.000) and (0.008, −1.000)→(−5.358, −1.045);
   - its collision outline `piece01_destruct01_hard01` has, along that side, the corners (5.375, −0.954), (3.228, −0.973),
     (1.082, −0.991), (−1.065, −1.009), (−3.212, −1.027), (−5.358, −1.045).

   The outline has no corner at the foot's middle corner (0.008, −1.000): the nearest are 1.07 away. Its edges along the foot are
   2.15 long where the platform's are 5.37. **Neither foot edge has a ground edge with its two corners, so the ground cannot link to
   the ramp's foot.** `r0_02` is the same, mirrored.

   What does match the outline is the two side edges, e.g. (−5.358, −1.045)→(−5.375, 0.954) against the outline's
   (−5.375, 0.954)→(−5.358, −1.045). Those can link wherever the ground is within 1.0 of the ramp. That fits units getting onto
   the bridge from its side (2026-11-08, in game). Whether they can is not checked against the ground here.
6. The deck–end joints: corners shared within a centimetre (`bridge_check`), each joint split in two edges at the middle corner, so
   one of the two halves is linked.

`bridge_check` now tests rule 5 (below). It fails the same way on the pieces it makes itself and on those an export lowers, and passes
the kit's `bridge_wood_2_end` (1 of 1 foot edges along its outline). So the generator's warp itself breaks the rule.

## Not traced

- Whether ground regions are single triangles or merged polygons. With step 4's "one link per pair", a merged region beside both a
  ramp's side and its foot would get only the first matching edge in the platform's ring order.
- The ground regions' type and +0x5c flag, and `FUN_1034f9a0`'s type rules beyond "0 never with 1".
- `FUN_1169dfe0` (point in a region), and whether any code assumes a convex platform. Nothing in the paths above limits corner counts
  or rejects collinear corners.
- What `flag_1` allows (slot 0x48), and what sets bit 4 (slot 0x34).
- `flag_3`'s use anywhere (not on the building platform path).
- **`orange_thingies` (`vertex_type`)**: not seen read on the ground-triangulation path. The in-game observation in
  [buildings.md](buildings.md) is that extending one without the other kept units off the difference. Also not seen: what the
  collision outlines' corner types ("hard" 0, "soft" 1, other 2) change, and the "outline" / "gate" / "secondary" paths.
- `collision_3d`'s part in movement.
- `AIH_BRIDGE_CROSSING` (string at file offset 0x1bab290): no code references its address directly.
- Water under bridges.

## The editor's check: *Check bridges* (2026-11-11)

The Tile panel's Problems section has a *Check bridges* button (`check_bridges`; `tile_document/bridges.rs`, its overlay in the tool's
`bridge_check.rs`). It gathers the platforms of the tile's `bridge` buildings and of the roads' generated bridges, and of the buildings and
walls around them, in the world at the states their pieces show, and applies the rules above: ground links along plain collision outline
edges (corner for corner, the ground within the snap tolerance), joints along reversed edges. A bridge is its platforms joined together;
its ways off are its ground links and its joints to other buildings' platforms, grouped by side (closer than a third of its length, at
least 4 m). None: units cannot get onto it; one group: it reaches one bank only.

**Ground platforms as the ground** (inferred, user 2026-11-11, not traced): the kit's working river bridges fail the height rule against
the terrain. `river_crossing/2x2_riv_lb_br_road_bl_tr/aa`'s `bridge_wood_complete` has its ramps' feet ~2.3 game units over the terrain
(the editing heights and `final_heights.dds` agree there), but each foot lies on the edge of one of the building's two `flag_2` (green)
platforms, at the foot's height. Since a `flag_2` platform has no region of its own and "units walk the ground there", the check takes the
ground's height from a ground platform where there is one (on it, or on its edge within 0.0141), else from the terrain. With that, both
kit river crossings pass. `farm_eastern/4x4/ab`'s four lone `bridge_wood_1_end` pieces (no deck, no ground platform, 0.6 to 2.6 m over the
terrain) are flagged, and a road's generated bridge passes (its feet run along its outline edges since 88348e13).

## Checker: `bridge_check` (`crates/wce_walls/examples/bridge_check.rs`)

`cargo run --release -p wce_walls --example bridge_check -- <game folder> [radius] [rise] [rpfm schema .ron] [exported .pack]`

Implemented, from the traced rules (2026-11-08):

- **Rule 5, the ramps' feet.** For every logic file with a platform sloped over 15 degrees, every edge with both corners at that
  platform's lowest height must have its two corners at two consecutive corners of a collision outline (x/z, squared distance
  ≤ 2.0e-4).

  It runs on:
  - the kit's end piece, as a reference;
  - the end pieces it generates;
  - the pieces `export::lower_walls` makes, when a schema is given;
  - every logic file of a pack, when one is given (a tile's launch or export pack).

  It prints "n of m foot edge(s) along its collision outline" and fails on any shortfall.

Still to add (traced, not yet implemented):

- **E1** A bridge piece whose `battlefield_buildings` category is not `bridge`.
- **E2** A logic file version ≤ 5 with any `flag_2` / `flag_3` set.
- **E3** A platform whose normal is off its corners: max |n·(vᵢ − v₀)| / |n| above 0.01.
- **E4** No platform in destruct level 0 of a bridge piece.
- **E6** For a tile: each foot edge's ground height (`hf_height_map`) at both corners against the platform's plane, over 1.0 for a
  bridge (0.1 otherwise).
- **E7** Joints between platforms of different pieces: in the world, the same two corners within 0.0141 and heights within 0.1.

Warnings (deviations from the game's pieces, no rule traced): more than 6 corners or not convex (TDD's 2530 fort platforms: none);
outline and `orange_thingies` with different corners.
