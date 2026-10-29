# Buildings: pieces, animations, attachments and effects (2026-10-20)

What a placed `BUILDING_INSTANCE` draws besides its standing piece meshes, as found in the packs' files (no engine trace unless said).
Code: `crates/wce_tile_editor/src/tile_document/index.rs` (piece selection), `columns.rs` (attachments and effects), `collapse.rs`
(collapses), `crates/wce_render/src/anim_pose.rs` (posing), `crates/wce_formats/src/anim.rs` (`.anim`), `anim_meta.rs` (`.meta`). Each **ASSUMPTION** below says how to
check it.

## The files of a building folder

`rigidmodels/buildings/<key>/` holds, per piece (`pieceNN`) and destruction state (`destructNN`, 01 intact):

| File | What |
|---|---|
| `<key>_pieceNN_destructNN.rigid_model_v2` | the standing piece at that level |
| `..._destructNN_anim.rigid_model_v2` + `.anim` (+ `.meta`) | the collapse from that level to the next (a burst of fragments, "building" skeleton, one bone per fragment) |
| `..._destructNN_<state>.rigid_model_v2` | a two-state piece: `_gate_closed` / `_gate_open`, ramps, levers |
| `..._destructNN_gate_opening_anim` / `_closing_anim` / `_gate_*_destruct_anim` | the gate swings and their collapses |
| `..._destructNN_left_wheel_looped_anim` / `right_wheel_looped_anim` | siege engines' wheels: the only looped animations in the game (rams, ladders, towers) |
| `..._destructNN_weighted_anim` (+ `.bone_inv_trans_mats`) | a skinned piece ("generic" skeleton): the battering rams' body (`piece01`), which has no plain intact file |
| `..._destructNN_destruction_weighted_anim` | the skinned piece's collapse |
| `<piece file>.meta` | what plays with the piece (effects, sounds), see below |
| `<key>_tech.cs2.parsed` | the logic file (platforms, pipes, collision, attachments), see below |
| `<key>_zone_tech.xml` | zone / go / no-go regions |

## Which files the editor draws

- A piece draws its plain file at its level. A two-state piece draws its closed state. **ASSUMPTION**: a placed gate, ramp or lever
  stands closed. Check: a settlement battle at deployment, gates closed or open.
- A piece with no plain intact file draws its **working animation** (`*_looped_anim`, `*_weighted_anim`), posed in its first frame.
  Its other levels use that animation's file of the level when there is one, else the plain file (the ram's broken body is a plain
  `destruct02`). Never drawn: collapses (`_destructNN_anim`, `_destruction_weighted_anim`) and gate swings.
- **ASSUMPTION**: the first frame is what a standing siege engine shows. For the ram it is exactly the bind pose (its inverse binds
  are the inverses of frame 0). Check: a deployed ram or tower in game, wheels in place.

## `.anim` (version 5)

rpfm_lib reads only the header. Layout, checked to the byte on battering_ram_light's and roman_fort_straight's files:

- u32 version (5), u32 (0), f32 frame rate (10), string skeleton name (`building` / `generic`), f32 end time, u32 bone count;
- per bone: string name, i32 parent (-1 for a root);
- per bone: i32 index of its translation track; per bone: i32 index of its rotation track;
- u32 translation track count, u32 rotation track count, u32 frame count;
- per frame: the translations (3 f32 each), then the rotations (4 i16 each, x y z w over 32767).

`.bone_inv_trans_mats`: u32 (1), u32 bone count, per bone 12 f32 (a 4 x 3 row-vector matrix: three axes, then the translation), the
inverse bind pose.

Posing (CPU, once at load): a rigid mesh by its bone's world matrix (the material's `i_matrix_index`, else bone 0); a weighted mesh
by `sum(weight * bone world * inverse bind)` over its vertex bones. Then the static rigid shaders draw it.
**ASSUMPTION**: the game skins these with its weighted shaders on the GPU; the result for a still pose is the same geometry. Not
checked against a capture.

## Attachments from the logic file (`_tech.cs2.parsed`)

Decoded with rpfm_lib's `Cs2Parsed` (fields private; read through its serde form). Per piece and destruction state, `file_refs` with key
`<state>_file:<building>` attach another building with a transform in the piece's frame (row-vector 4 x 4, translation in m30..m32).
Seen: the walls' bridging columns (`roman_fort_straight`'s `piece01_destruct03` attaches `roman_fort_bridgingcolumn_right` at x = -10 and
`_left` at x = +10) and the walls' `torch_sconce` torches on their intact states.

A bridging column is a building with a standing (`destruct01`) and a broken (`destruct02`) state. The two columns of a joint are
halves of one: `barbarian_fort_bridgingcolumn_left`'s broken state is only a trench of disturbed soil (a terrain-blend mesh), the
`_right` one's holds the splintered posts and the rubble.

The editor's rule (from the user's description of the game): a damaged wall shows its columns; where two walls' columns meet (within
0.5 m) and both walls are destroyed, both columns are drawn broken.
**ASSUMPTIONS**: the engine's neighbour pairing was not traced (here it is positional, 0.5 m); a column at a joint with only one wall
destroyed stands; an attached building takes the attaching piece's transform and no destruction of its own otherwise. Check: in game,
destroy one wall of a straight run, then its neighbour, and compare the joint with the editor's (`tile_edit <elem> destruction_level`).

## Effects from `.meta`

A piece's `<piece file>.meta` (not the `_anim.meta` of the collapses, which hold the collapse's dust and sounds). All 1495 standing
pieces' files of the game and the TDD packs parse to the byte with this layout:

- u32 version (2), u32 entry count; per entry: string type, u32 entry version, f32 start, f32 end, and from entry version 2 a string tag;
- `EFFECT` (versions 1, 2, 3, 5): string effect (`vfx/<name>.xml`), string flag (`"true"` / `"false"`), position (3 f32) and rotation
  (quaternion x y z w) in the piece's model space (the space the mesh's pivot is in: the torch's flame at (0.72, 2.09, 0) is the top of
  the torch head), i32 bone, and for version 3 and from 5 an f32 scale (fn 0x116a8680);
- `VOLUMETRIC_EFFECT` (2): string effect, then 20 f32 (fn 0x116a9c00), read as a position (3) and rotation (4) (zero, and the identity or all zero, in
  every file seen), the box's centre (3) and size (3) in the piece's model space, the box's rotation (4, the identity in every file seen),
  3 more (zero in every file seen). **ASSUMPTION** (centre and size): the walls' boxes span their pieces (`roman_fort_straight` piece01:
  20 x 18 x 12 m around (0, 6.5, 4.3)), the houses' theirs (`barbarian_ahouse_1`: 10.2 x 7 x 12.8 m around (0, 2.9, 0.2));
- `SOUND_BUILDING` (2): 20 bytes: u32 the sound (a hash), its position (3 f32, the piece's model space: the collapse's opening sounds
  at the damage, its impacts about 1 m above the ground where fragments land) and u32 0 (one file has 9.0 there). Its start and end
  are seconds into the collapse (impacts from 0.8 to 6.5 s in a 10 s collapse). Read from the game's collapses (2026-10-29,
  `wce_destruction` example `destruction_calibrate`); the engine was not traced.

Every `.meta` and `.anim` under `rigidmodels/buildings/` of the game's and TDD's packs (2267 and 1032 files) writes back to the byte
through wce_formats' lossless `MetaFile` and `AnimFile` (example `anim_meta_corpus`). The largest collapse has 798 bones.

Tags seen on standing pieces (counts over the game and TDD): `ip_fire` (fire and smoke loops, ~1400), `capture` (capture smoke, 507),
`fire_transition` (108), `fire` (timed fire and smoke, 74), and untagged (the torch fires, the Carthage lighthouse's `enviro_vfx_fire_loop`,
and `destruction_vfx_*` bursts on the damaged states).

The editor plays: untagged effects except `destruction_vfx_*`; `ip_fire` effects while the building burns (its `burn` property, or a
piece's own); nothing else. Placed at `piece world * translation(position) * rotation`, scale `scale / unit` like a placed VFXInstance.
**ASSUMPTIONS**, to check in game:
- untagged effects play all the time (the torches burn at deployment, by day too);
- `destruction_vfx_*` bursts play once when a piece reaches the level, not on a battle that starts with the building already damaged;
- `ip_fire` means "while the building is on fire", and a tile's `burn = true` building starts burning;
- `capture` plays on a captured capture point, `fire_transition` / `fire` during the burning sequence;
- the effect's `scale` multiplies like a VFXInstance's scale;
- the string flag and the bone (-1 on almost all; 0 on 21 entries) are not used: what they do is unknown (the flag may be "attached to
  a bone" or "looping").

## Collapses between levels

When a piece's level goes up (an edit, an undo or a redo), the editor plays its collapse instead of switching meshes; going down is
instant. From level L to L + 1 the collapse is `..._destruct{L+1}_anim` (`.rigid_model_v2` + `.anim` + `.meta`):

- The mesh ("building" skeleton) has one bone per fragment; each block follows the bone of its material's `i_matrix_index`, with its
  vertices in that bone's frame (`roman_fort_straight_piece01_destruct01_anim`: 36 blocks, 16 bones, all roots; bone 0 holds the base
  that stays, at the identity throughout). **ASSUMPTION**: its first frame is the standing piece and its last the next level's rubble (on that
  wall the end meets the rubble by eye; the start was not compared closely).
- The animation runs at its frame rate to its end time (that wall: 20 fps, 10 s; its battlements' pieces 9.9 and 5 s). The editor
  interpolates between frames (translations linearly, rotations nlerp) and draws each bone's blocks as an entity (the model cache's
  `<file>#bone=K`). A step starts once its fragments' models are loaded (the tool queues them), and the standing piece hides the frame
  after the fragments first draw, so there is no frame without either.
- Skipped levels chain (0 -> 2: `destruct01_anim`, then `destruct02_anim`). A level without a collapse (most houses; the siege engines'
  weighted collapses, not drawn) changes at once, with its bursts.
- The collapse's `.meta`: a `VOLUMETRIC_EFFECT` of dust (`destruction_vfx_area_dust_light` from intact, `_heavy` from damaged) at about
  0 s, and the `SOUND_BUILDING` entries (timed impacts; not played: the editor has no audio).
- The standing `.meta` of the new level holds bursts: `VOLUMETRIC_EFFECT`s (`destruction_vfx_generic_rigid_stone` / `_wood`,
  `_area_dust_*`, 399 in all) and some untagged `EFFECT` explosions (`destruction_vfx_medium_stone_explosion`, 31), plus
  `fire_transition` ones (108).

The editor plays, per step: the collapse's effects at their times, the new level's bursts at the step's start, and the `fire_transition`
effects of both when the building burns. Each effect entity is removed 15 s after it started (its particles live up to 8 s after its
short emitters).

### What the engine was found to do (empire.retail.dll)

- `.meta` entry readers: the classes and their descriptions are a table (fn 0x116a3280); `VOLUMETRIC_EFFECT` is "Trigger a volumetric
  particle effect group". Its reader (fn 0x116a9c00) reads the common header, the effect name, then 20 dwords; `EFFECT`'s (fn 0x116a8680)
  reads name, a flag string compared with "true", 8 dwords (position, quaternion, bone) and the scale for versions 3 and from 5 (the
  editor read it from 3 on; fixed). Buildings copy the volumetric entries into their own array (0x84 bytes each, fn 0x10d511f0).
- Mesh emission (fn 0x1183fa30, a particle's birth): when the effect's owner has a mesh (owner +0x94 / +0x98) and the emitter
  AllowEmitFromMesh, the point is on a triangle of that mesh, the triangle drawn uniformly by index from the instance's generator and
  masked by a per-triangle bit set (owner +0xa8), the point from two draws of the shared generator (folded back when they sum past 1),
  scaled by the effect's scale and turned by the instance's rotation; UseSurfaceNormal takes the triangle's normal as the direction. The
  editor does exactly this (particles.rs `mesh_point`).
- The destruction effects' emitters have no spawn area at all and short lives: the dust plumes live 0.07 - 0.1 s at 40 - 100 particles
  a second, size 25, rising 5 m/s along the normal; `burst_rigid` emitters throw 1 - 2 pebbles (`vfx/pebbles/pebble_01`) or wooden
  shards each, at 4 - 15 m/s, falling (GravityAmount 60 - 80), spinning (Spin 2 - 4), living 2 - 3 s. One instance alone is a few puffs
  and a pebble: the "group" must be many instances.

The editor now: builds the effect's mesh from the triangles of the piece's standing mesh at the level the collapse starts from whose
centre lies inside the volume's box (in the volume's frame), spawns the volumetric effect as a group of instances on it, and draws the
`burst_rigid` debris (non-light rigid models) as ballistic models (particles.rs `effect_rigids`, drawn through the scene's draw set).

**ASSUMPTIONS**, to check in game:
- the mesh a `VOLUMETRIC_EFFECT` emits from is the piece's own standing mesh inside the box (the engine's owner mesh and its triangle mask
  were found, not what fills them); the box is the volume's centre and size floats (7 - 12, rotation 13 - 16);
- the group is one instance per 64 m^2 of the box's surface, 1 to 32 (the wall: 25 instances);
- debris rigids fly ballistically (gravity 0.1962 x GravityAmount, no drag, no collisions: the destruction debris has EnableCollisions
  false), spin about ParticleSpinAxis from a random orientation, scale lerp(ParticleScaleStart.x, .y); the engine's rigid particle
  update was not traced;
- the new level's bursts play when the collapse starts (not when it ends);
- an effect's bone is not followed (a few `.meta` effects name bone 0);
- the editor's playback matches the game's speed (the `.anim`'s rate and end time, real time).

Check: in a siege battle, watch a wall section collapse from intact to destroyed, and compare with the editor's
(`tile_edit <elem> destruction_level 2` on an intact wall).

## Look properties (2026-10-27)

TEd (`~/wce_re/ted_proj`) and the kit's Warscape.AssemblyKit.dll (`~/wce_re/bob_proj`):

- `Ted::BUILDING_INSTANCE::set_property` (0x41e1a0) stores opacity, dirt_level, bounds_colour, burn, burn_amount, glow_amount,
  destruction_level, building_tech_level; its property-changed handler (0x41eee0, vtable 0x6a1694 slot 0x2c) passes 6..11 on to each
  piece, a `Ted::MESH_INSTANCE` (`set_property` 0x420ff0: 7 opacity, 8 burn, 9 burn_amount, 10 glow_amount, 11 dirt_level), and 12
  (destruction_level) swaps the piece files. 13 (building_tech_level) has no case.
- A piece's render parameters (0x448010) are the engine's instance parameter block: [0] = opacity times a fade, [2] = burn_amount,
  [3] = glow_amount, [0x1f] = 0x2000 when the opacity is below 1 (another draw list: the blended forward shaders). `burn` ORs a render
  flag (0x4480f0). dirt_level is never read for drawing.
- Warscape packs POSITION4 as (xyz, round(clamp(opacity, 0, 1) * 1000) + a fraction) (0x1025e700 / 0x10262e20); POSITION7 is the
  block's faction colour ([0x31..0x34]). [2] goes to the instance record next to two other parameters ([0x2b], [0x2a]); the record's
  path to POSITION5/6 was not followed, and nothing was found reading [3].
- rigidstandard (`rigid_config_PS`: g_unlit, g_dissolve_enable, g_untextured): `ps30_gbuffer_alpha_test` and `ps30_alpha_blend_40`
  multiply the diffuse alpha by floor(P4.w) * 0.001, or with g_dissolve_enable keep opacity - dissolve * (1 - opacity) (the
  screen-tiled `s_dissolve_map`) against the 0.5 test; P6.y brightens, P6.z desaturates.
- The game export (`BATTLEFIELD_BUILDING`) carries burn (`on_fire`) and the destruction level (`starting_damage`), none of the rest.
- Burning, in the game (empire.retail.dll, 2026-10-28): a burning building's render parameter [2] (the one TEd's `burn_amount` sets) is
  ramped by the time since the fire started (0x10438340, 0x10525250; the tweakers `BURN_SHADER_AMOUNT` 0.3,
  `BUILDING_LITE_BURN_SHADER_AMOUNT` 0.3, `BUILDING_LITE_MAX_BURN_SHADER_AMOUNT` 1.0) and render flag 0x200 is set. The flag draws the
  pieces with their material's fire variant (`rigidstandardfire`, `rigidtileddirtmapfiremesh`, `rigidambientmapfiremesh`,
  `rigidwithdirtburnmesh`; G-buffer and alpha-test shaders), which char the colour by POSITION6.x with embers from `s_fire_map`
  (`commontextures/vfx_fire_diffuse`, bound with the blood map by 0x117c3830). So [2] is POSITION6.x. A tile's `burn` starts the battle
  with the building on fire (`on_fire`); its `burn_amount` is TEd's preview only (the game ramps its own). Attila's building models have
  no burning meshes (material 56: none in the 5639 building models).
- Glow: TEd's `glow_amount` goes to parameter [3], which no rigid instance path of the kit's engine reads, and the export does not carry
  it. The game's glows are its unit and deployable highlights (`UNIT_GLOW_INTENSITY_SELECTION`, `..._MOUSEOVER`,
  `DEPLOYABLE_GLOW_INTENSITY_...`), its own parameters (POSITION6.y brightens). `glow_amount` does nothing in TEd or the game.

## Wall pieces (2026-10-24)

From `models2.pack` and the logic files (`Cs2Parsed` version 11) of every straight in the game's and TDD's packs; used by the procedural
walls (`crates/wce_walls`, docs/plans/procedural_walls.md).

- **Files and rows:** as above, plus `battlefield_buildings` (category `fort_wall`, hit points, `destruct_thresholds`, `model`) and
  `models_building` (below). rpfm_lib writes `Cs2Parsed`, `RigidModel` and the DB tables; both families' base files write back byte for
  byte.
- **Frame** (`barbarian_fort_straight`): x 0..20 along the wall, outer side -z (palisade, hard collision and outer strip at z -1.8..-2.8),
  ground y = 0, mesh buried to y = -2. A mesh's vertices are stored around its material's pivot (`v_pivot`, its box's centre, e.g.
  (10.0, 3.6, 4.2)): the logic file's frame is vertex + pivot, and `min_bb` / `max_bb` are in it. The walls are long triangles (20 m
  along x; 24 m in the 15 m rubble).
- **Walkway = platforms** (flags `false, true, false`). Barbarian straight: flat at y = 8, z -2.4..2.3; the inner earth bank in four
  sloped strips down to y = 4 at z = 11.5 (units walk onto it from the ground, in game). `_step_up_a`: flat 0..2 at 8 m, a ramp 2..5 to
  10.5 m, flat 5..15, a ramp 15..18 to 13 m, flat 18..20 (about 40 degrees); **the stairs are only visual**, the pathfinding surface is
  a slope.
- **Joining:** no pipes, docking lines or docking points in the logic files of the 8 m pieces; pieces join because their platforms meet
  at the ends (x = 0 and 20). In game (2026-10-24) units walk from a bent piece onto vanilla neighbours.
- **Firing lines** (`ef_lines`): 8 m, per metre a `HighWall` line on the walkway (0.5 m, facing -z) and two `Overflow` (60 intact, 58
  damaged, 0 destroyed). 15 m: `LowWall`, `Crenel{Left,Right}{Inner,Outer}`, `Overflow`.
- **Collision:** `hard01` (the palisade's footprint, ragged when damaged), `ground_ad` (a line at z = -2.5). **`orange_thingies`**: per
  level a 2D x/z outline (vertex type 2), the footprint a building stands on (8 m: the palisade's; 15 m: z -2..10.5).
- **Levels:** intact (5 platforms), damaged (17), destroyed (28: the breach's rubble ramp; no collision, no firing lines). The destroyed
  level attaches the bridging columns (`_right` at x = 0, `_left` at x = 20); the intact one a `torch_ground_post`. Step pieces
  (`barbarian_fort_step_up_a` / `_down_a`) have only intact and damaged levels, no rubble.
- **Zones** (`zone_tech.xml`): one region, the footprint rectangle x -0.2..20.2, z -2..2.5.

### The `models_building` row

Columns `display_path`, `key`, `logic_file`, `unknown`, `EFLines`, `DockingLines` (the kit's table has others).

- `logic_file`: `buildings\<key>\<key>_tech.cs2.parsed`, the file the game reads (a copy must point at its own).
- `EFLines` (bytes): u32 count; per line a name (u16 length, UTF-16), u32 action, start, end, direction (3 f32 each, the direction a
  unit vector). Barbarian straight: 118 (60 intact, 58 damaged).
- `DockingLines` (bytes), **where ladders and towers dock**: u32 count; per line a name, start, end, direction in x / z (2 f32 each,
  facing out). A straight has one along its outer face (barbarian (19.93, -2.5) to (0.07, -2.5); 15 m (10, 0) to (-10, 0)); CA's curves
  a polyline of short ones (`barbarian_fort_curve_20m_radius` 6 of ~5 m, `_60m_radius` 3 of ~10 m, the kinks 4); **step pieces none**;
  gateways two of 0.26 m.

The rows of `barbarian_fort_straight` and `roman_fort_straight_with_doors_a` are the same in `data.pack` and TDD's main pack; TDD's
`@tdd_dev_buildings` overrides `barbarian_fort_straight`'s files (one more platform when damaged) but not its row.

**Docking is only valid on aligned pieces** (in game, 2026-10-24): ladders and towers docked onto generated walls whose joints were off
(a piece leaving a gate at 18.8 degrees; bastions 0.23-0.36 units and 0.8-1.2 degrees off) but units could not cross onto them. The game's
walls never turn at a joint, only inside a piece (kinks, curves). The exact rule is not known; exact joints fix it.

### The families

| Family | Bases | Frame | Walkway | Way up | Height changes | Levels |
|---|---|---|---|---|---|---|
| 8 m (earth bank + palisade) | `barbarian_fort_straight`, `{eastern,sassanid,western}_fort_small_wall_straight`, TDD `khazad_fort_wall_small_straight`, `mordor_fort_small_wall_straight` | x 0..20, one piece | y 8, z -2.4..2.3 | the bank's sloped platforms behind | `_step_up_a` / `_step_down_a` (5 m, two 2.5 m ramps) | 3 |
| 15 m (stone) | `{roman,eastern,sassanid,greek,egyptian,persian}_fort_straight`, TDD `{easterling,gondorean,gondor}_fort_straight` | x -10..10; piece01 the wall, piece02 / 03 the walkway's end strips (x -10..-8, 8..10, z 0.5..3, 2 levels) | y 15, z 0.5..9 | none in the straight: `_straight_with_doors_a` has 6 `WallDoorTeleport` pipes (inner door at the ground, z 13, to the walkway, z 6..8), the towers 6 more | only `_straight_stepped_up/down_10m_square_bastion_a` (a bastion with a 10 m ramp, 15 to 25 m, over ~12 m, inner side) | 3 (end strips 2) |
| TDD Rohan (timber) | `rohan_wall_straight` (+ `_ladders`, `_ladders_attacker`, `_ladders_definitive`, `_bastion`, `_step_up`) | x -10..10, 4 m thick | y 10, z -2..2, 16 platforms of 5 x 1 m | `_ladders*`: 4-5 `SiegeLadder2` pipes | `_step_up` (10 to 15 m) | 1 |
| TDD cliff | `persian_cliff_fort_straight` | x -10..10 | y 15, 2 platforms | ? | ? | 1, no firing lines |

`gondor_fort_straight`'s frame is x -10.5..10.8 (pieces overlap their neighbours).
