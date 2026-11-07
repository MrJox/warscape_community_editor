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
- Never drawn either: **numbered copies** (`..._destruct01_0` beside `..._destruct01`, `..._destruct01_anim_0` beside
  `..._destruct01_anim`, `<key>_tech_0.cs2.parsed` beside `<key>_tech.cs2.parsed`). The Dawnless Days mod packs carry one beside every
  piece file of its Gondor, Moria and Khazad buildings and some cliffs and rocks (258 models in 103 folders, every one with its original;
  in `gondor_fort_straight` every copy has its original's size, and in the pair compared only a few header bytes differ). The logic file
  names only the originals. A file whose name less a trailing `_<digits>`
  after `destructNN` is in its folder is skipped (`index::copied_piece`). The procedural walls drop them from their bases too
  (`wce_walls::building::numbered_copy_of`): warped and shipped beside their originals, the game drew every Gondor wall piece twice.
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
by `sum(weight * bone world * inverse bind)` over its vertex bones (two-bone weights: `anim_pose::vertex_weights`). Then the static rigid shaders draw it.
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

## Logic file fields to decode properly in rpfm (2026-11-05)

Found while generating wall pieces (`wce_walls`); rpfm_lib (`cs2_parsed`, `cs2_collision`) reads and writes them, but under placeholder
names or with wrong descriptions. To rename and document in rpfm (`~/Proyectos/rpfm`, develop) when it is next worked on.

- **`bin_data`** (per destruction state, `Vec<Vec<i16>>`, rpfm: "unknown binary data, correlates with VFX count"): lists of indices
  into the state's `collision_3d.triangles`. Together they cover every hull triangle exactly once, and there are always as many lists
  as `action_vfx` entries (the 15 m straight wall: 7 and 7 over its 104 triangles; the stepped bastion: 3 and 3 over 293; each end
  strip: 1 and 1, all its triangles). Read: list N is the hull triangles of `action_vfx` N (which effect a hit on that part of the
  hull plays). ASSUMPTION: the pairing by index was not traced in the exe. Name to give: something like `action_vfx_triangles`. A
  rewritten hull must keep it in step (each new triangle in its source triangle's lists): with stale lists the cursor and selection
  over the wall misbehaved. Indices are i16: a hull of more than 32 767 triangles cannot be described.
- **`bin_data_2`** (`Vec<Vec<i16>>`): empty in every wall piece seen (rpfm: "present in some Three Kingdoms gates"); probably the same
  for another list of effects.
- **`collision_3d` triangles** (`CollisionTriangle`): `zero_1`, `zero_2`, `zero_3` are not zero: they are the edge's number in the
  triangle (0, 1, 2) in every file seen; `face_index_1..3` repeat `face_index`; `across_face_index_N` is the triangle across edge N
  (-1 on an open edge; the walls' hulls are closed, none). `zero_4` is 0.
- **Hull winding and closure**: hull triangles face out ((v2 - v1) x (v3 - v1), a positive volume). Most hulls are closed, not all: the
  door section's intact state has 6 open edges, the stepped bastion's 3 and 17.
- **`collision_3d`** (`Collision3d`): `uk_1` 1 and `uk_2` 0 in the walls; its `bounding_box` is all zeros in the files seen (the
  state's own `bounding_box` is the hull's extent).
- **The state's `uk_2`** -1, **`uk_7`** 0, **`f_1`** 0.0 in every wall piece seen.
- **Platforms**: `flag_1..3` all false on walls (the editor draws flag_2 as ground, flag_3 as a siege ramp: unconfirmed). The game's
  pieces join platforms edge to edge (the stepped bastion's stair shares its top edge's two corners with the landing); whether it
  joins edges that only overlap was not found, so generated pieces weld theirs.
- **Platform winding**: every platform of the game's pieces (the walls', the door section's, the stepped bastion's, its sloped stair
  too) lists its corners clockwise seen from above: a negative area over x and z (x right, z up). Two neighbours then run their shared
  edge in opposite directions, as half-edges. Generated platforms are turned to it (`wce_walls::logic::wind_as_the_game`); the stairs,
  made the other way round, were the only platforms that did not follow it. Worth a doc comment on rpfm's `Platform`.
- **Platform shapes**: in the fort pieces (TDD's pack, 2530 platforms) every platform is convex, has at most 6 corners (3 or 4 nearly
  all), and only 11 are under 0.2 across (median area 8.6). Whether the game needs that was not traced.
- **Pipes** name no platforms: a polyline (`line.outline`) and its `line_type` (`WallDoorTeleport`, ...).

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
- `SOUND_BUILDING` (2): 20 bytes (not decoded).

Tags seen on standing pieces (counts over the game and TDD): `ip_fire` (fire and smoke loops, ~1400), `capture` (capture smoke, 507),
`fire_transition` (108), `fire` (timed fire and smoke, 74), and untagged (the torch fires, the Carthage lighthouse's `enviro_vfx_fire_loop`,
and `destruction_vfx_*` bursts on the damaged states).

The editor plays, for a game that reads them (not Attila: "What gates a piece's effects"): untagged effects except `destruction_vfx_*`; `ip_fire` effects while the building burns (its `burn` property, or a
piece's own). In every game: a burning piece's `burning_entity` over its mesh (below, "The flames"), and the entries of its destruct's `vfx_emitters` in the logic file. Placed at `piece world * translation(position) * rotation`, scale `scale / unit` like a placed VFXInstance.
**ASSUMPTIONS**, to check in game:
- ~~untagged effects play~~ **wrong for the torches, seen in game** ("What gates a piece's effects"); the other untagged ones are not checked;
- `destruction_vfx_*` bursts play once when a piece reaches the level, not on a battle that starts with the building already damaged;
- `ip_fire` means "while the building is on fire", and a tile's `burn = true` building starts burning;
- `capture` plays on a captured capture point, `fire_transition` / `fire` during the burning sequence;
- the effect's `scale` multiplies like a VFXInstance's scale;
- the string flag and the bone (-1 on almost all; 0 on 21 entries) are not used: what they do is unknown (the flag may be "attached to
  a bone" or "looping").

### What gates a piece's effects (2026-11-07, Attila's `empire.retail.dll`)

`FUN_104cc000` sorts a `.meta` entry by its tag: `ip_fire`, `ip_smoke`, `ip_smoke_post`, `ip_fire_collapsed`, `ip_collapsed` (bit 1), `fire` (bit 2), `fire_transition` (bit 4). `FUN_104b2a80`
walks the entries each time a piece is serviced: bit 1 is skipped there (`FUN_10d52250` plays those), bit 2 needs its `param_5`, bit 4 `param_6` as well; **an untagged entry passes**, and starts once
its start time has passed (0 and 0: at once). The torches' entries are untagged and their string flag is `false`, so nothing in the `.meta` makes a torch conditional. `FUN_104b2a80` runs from the
draw of a piece or standalone entity (`FUN_10464ce0`, `FUN_105340c0`): after a view test (vtable +0x84), for a piece that is shown (flag 1 of +0x7a; +0xdc for the entity) in mode 1 or 3 (2 is the
collapse). No night, weather, season or owner test was found on this path. Not traced: the code that makes the attached buildings of `file_refs`, or which object the entity list of `FUN_10534060` is.

The string flag is read by `FUN_116a8680` into the entry's +0x30 (`"true"`). In `FUN_104b2a80` it keeps a handle to the running effect and moves it every time (`false`: started once, by the `started` table). **Tested in game 2026-11-07**: the three torch `.meta` with `true`, in a mod before TDD, on `gen_dun_minor_final_2`: the torches still did not burn. So the flag is not why. The user sees them never burn in game; the editor draws them (an **ASSUMPTION** of `columns.rs`: that the game plays what the `.meta` says on attached buildings). `_tech.cs2.parsed` has one user in the code, `FUN_10dc5c10`, and that is a prop list (`Filename`, `CollX`, `Burnable`), not the path of the walls' `file_refs`.

**Seen in game 2026-11-07** (`plains/8x8/8x8_aa`, no mods, close up): `action_vfx_torch_fire_small` as a `VFXInstance` burns; `torch_ground_post` and `torch_sconce` as buildings of their own, and the post attached to `barbarian_fort_straight`, show their models and no effect. So the attachment works, the effect works, and **the `.meta` effect of these models does not play**, though the same effect ran in that map (so it was loaded). The editor draws these flames, which the game does not.

**Seen in game 2026-11-07, second test** (the same tile, vanilla): `carthage_lighthouse` has no fire on top either, so no untagged `.meta` effect is known to play in Attila. Only 4 pieces of the game have one (three torches and the lighthouse). The user remembers the fire in Rome 2: not checked. `greek_torch_sconce` (its model is 294 bytes) stopped the game with a runtime error on load when placed as a building, by elimination: the two maps with it failed, the one with the lighthouse and the same patches loaded.

**The cause (2026-11-07, read in the disassembly): Attila drops the `EFFECT` entries of a plain piece `.meta` when it loads it.** It has two `.meta` readers: `FUN_10d4e050` (through `FUN_10d4c7b0`, with the effects' manager) reads an entry (d4e247), looks its effect up (vtable +0x5c, d4e25c), stores it at +0x58 and appends it; `FUN_10d4e3c0` (through `FUN_10d4c910`, no manager) reads the entry (d4e5b7), frees it and jumps on (d4e5c5). `VOLUMETRIC_EFFECT` and `SOUND_TRIGGER` go the same way there; only `SOUND_BUILDING` is kept (d4e616). `FUN_10d52740`, which standing pieces load through (`FUN_104bb1e0` at 4bb4d6, `FUN_104bcba0` for gates), uses the second. Rome 2 has one reader, with the manager (`FUN_10c8c070`), and its entries are 0x58 bytes with the template at +0x54. So tag, version, string flag and effect cannot matter, and the `ip_fire`, `fire`, `fire_transition`, `capture` and volumetric entries of a plain `.meta` do not play in Attila either, which the editor's playback and the notes above assumed. Attila's burning is `burning_entity*`, through `FUN_10d52250`. That it is deliberate is an inference. Other routes, none tested: `FUN_104bcba0` loads `<file>.meta` with the manager when a gate lacks its `_gate_open` mesh; a logic file's Destruct +0xe0 list, which does play (below). The logic file's fire list (+0x70) plays only with the burn flag. A binary patch would call `FUN_10d524d0` with the manager at 4bb4d6. Effects swapped in the `.meta` (the post playing `enviro_vfx_fire_loop`, the sconce `action_vfx_fire_smoke_small_loop`) show nothing either, so it does not depend on the effect. Rome 2's `carthage_lighthouse`, torch and sconce `.meta` have the same entries as Attila's (its `enviro_vfx_fire_loop.xml` is another, 2 388 bytes against 4 428); whether Rome 2 plays them is not checked.

**A route that plays (seen in game 2026-11-07): the logic file's effect list.** On `plains/8x8/8x8_aa`, with one mod holding a patched `torch_ground_post_tech.cs2.parsed`, the two posts placed as buildings burned, flame on top; the two unpatched `torch_sconce` beside them did not. No shipped logic file uses this list (0 of 786 have a key of this form), so a mod is the only known user. The attached post of a `barbarian_fort_straight` beside them burned too, with no change to the fort's own logic file: a building attached through `file_refs` gets the list of its own logic file. But its flame stayed when the wall was destroyed (below), so the entries belong in the wall's logic file.

The list is Destruct +0xdc (capacity), +0xe0 (count), +0xe4 (entries of 0x50 bytes). `FUN_110f61c0` reads it after the soft collisions and before `file_refs`, a list count that is 0 in every shipped file. Each entry on disk (`FUN_10ff45b0`) is a u16 length, that many UTF-16 characters, and 16 floats, row *i* on disk going to column *i* in memory (`FUN_1168a380`): slots 12 to 14 are the position in the piece, 4 to 6 its up. `FUN_10433780` (called at 4bf638) makes an entry of `E+0x18c` for each, walked by `FUN_104da1d0`: the effect's name is the key without its first three `_` tokens and its last, so `piece01_destruct01_emitter_action_vfx_torch_fire_small_01` plays `action_vfx_torch_fire_small` (the `emitter` token is free). It is looked up by name through the manager (vtable +0x5c), which lists the `*.xml` of the effects' folder (`FUN_1192e6a0`), no table involved.

For `torch_ground_post` (v13, 12 608 bytes) that was the u32 at 0x2e18 from 0 to 1 and the 180 bytes of the entry after it, position (0, 2.37034, 0) as in its `.meta`. Gates on the way: the tweak `building_destruction_emitters` (`FUN_100c0130`, default true), `VFX_ENABLE`, and the piece's level (+0x158) not above the destruct's +0xc byte. An entry whose key has `building_destruction_smoke` also sets the entry's +0x4c, not followed.

What was seen and read after that, 2026-11-07, with onagers on a `barbarian_fort_straight` and a `barbarian_fort_curved_bastion`:

- **When it stops.** The list is per state: `FUN_10464ce0` walks the one of the state in the instance's +0x78 (records of 0x1ac bytes), with no other test of it in the walker. `FUN_10497db0` stops a state's list (`FUN_1044a6f0`, at 498018) when the piece's state differs from the one it is going to and the instance is in mode 2. An attached torch has one state of its own and never gets there: its flame stayed after its wall was gone. In the wall's own list the flame goes the moment the wall starts to break. A piece that changes state without mode 2 was not tried.
- **One effect, one flame.** `FUN_10433f30` groups a list's entries by effect, and `FUN_104da1d0` has one running effect per group and moves it through the group's places, so it shows at one of them: of six torches of a state playing `action_vfx_torch_fire_small`, one burned. Each entry needs an effect of its own.
- **`vfx/` in packs.** An effect under a new name in a mod-type pack did not play. Effects under shipped names in a mod-type pack did not replace the shipped ones either (the torches showed the shipped `cindy_*` fires). The same files in a **movie-type** pack replaced them: six torches, one look. New names in a movie-type pack play as well.

`crates/wce_tile_editor/examples/effects_mod.rs` builds this from the game's packs: 68 logic files, a mod-type pack; the wall-mounted effects in the destruct that attaches the model, each after the first of a destruct playing a copy of the effect (at most six on a destruct); the five copies, `wce_action_vfx_torch_fire_small_02` to `_06`, a movie-type pack. A model something attaches keeps no list of its own, so a torch placed alone stays dark: its logic file cannot tell the two apart. `carthage_lighthouse` keeps its own entry, without the `.meta`'s scale 2 (the list has no scale). `greek_torch_sconce` is left out. rpfm calls the list `vfx_emitters` (v8 to v13; v18 and later keep `uk_7`, not checked), and the editor plays it.

`torch_sconce` and `torch_ground_post` are placed per fort piece by its logic file, per destruction state: 67 of the 191 fort pieces of the kit's tables have any (298 `file_refs`, all pieces of the
vanilla packs), 202 on state 0 and 96 on state 1, none on later ones. Bridging columns, end caps and most joiners have none, and a damaged piece usually keeps fewer: walls are not lit evenly by data.

`library_vfx_fire_torch` has no time of day or weather property either, `EventEnable` false and infinite life. Its limits, camera distance in metres by `gfx_effects_quality` (3, 2, lower; CullDistances
`w` stops the emitter, ParticleCullDistances `z` to `w` fades its particles): Fire 110 / 80 / 50 (fade 40-90 / 30-60 / 20-40), Smoke_heavy 150 / 120 / 90, Embers 50 / 22 / off, Distortion 32 / 32 / off,
the light 250. The scale of `FUN_11506600`'s Rome 2 rule (`docs/re/rome2_vfx.md`) was not checked for Attila. So the flames are gone well before the wall is: beyond 90 m at the highest setting.

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

## A building's scale (2026-11-06, in game)

The building list's record has a `scale` (a vec3), and TEd has no such property and writes 1. **The game ignores it**: on `plains/8x8/8x8_aa`, vanilla,
`christian_church_1` at scales 1, 2, 0.5 and (1, 2.5, 1) in `building_list` (the floats checked in the exported file) all stood at the same size in a
battle. The same four as prop markers (`wce_tile::export_as_prop`) were scaled, the non-uniform one too, behaved as props (no collision, no damage), and
at scale 1 matched the building in size and height. So a building can be scaled only as a prop, and not by TEd's restriction: the game's.

Static reading of Attila's `empire.retail.dll` (same day; RVAs): the field is **stored but unused**. The map's building record (0x88 bytes, the same for `building_list` and the
BMDs) keeps the scale at +0x40, and only its readers and writers touch it (ESF about 0xde12c5, fastbin 0xd89150 / 0xd89c70, XML 0xdd34f0 / 0xde7210, vector copies). What makes the
buildings does not: the loop at 0x16bec0, the bounds at 0x170b20 / 0x159a30 (the type's model bounds, heading, position), the full building 0x632440 with its world matrix 0x698340
(rotation and translation), the lite building 0x50bfe0, and the far buildings (`register_far_buildings` 0x4bb5e0: 0x20-byte records without a scale). A BMD's `Building` (fastbin v3
has no `transform`: that is PROP's, 0xd898d0, which is why props scale) lands in the same records (0xdc72e0, 0xdcd910), so the escalation layers should behave alike; not tried in game.
No tweak gates it: `building_scale` is the campaign's (`CampaignBuildingDisplay.cpp`), `battle_building_enable_lod_size_scale` is about LODs. `set_scale` hits in the dll were not
followed, one record site (0x1212b0) was not read closely, and neither was the runtime choice of `building_list_<layer>`.

Not a real scale, and unverified: nothing on those paths normalises the quaternion, so a non-unit one should stretch the drawn model across its axis (yaw: wider, never taller), while
the bounds and far buildings use the heading; and every piece has a matrix of its own in the draw, which may be the `.cs2` node `docs/re/renderer.md` found identity everywhere, so it
would scale a type, not an instance. Rome 2 and Thrones not checked.

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
- Burning, the sequence (2026-10-30, empire.retail.dll, static):
  - The tweakers' defaults (their registrations at 0x10047e00.., 0x10052d30..): `BUILDING_FIRE_STAGE_ONE_DURATION` 18 s,
    `BUILDING_FIRE_STAGE_TWO_DURATION` 10 s, `BURN_SHADER_AMOUNT` 0.3; the "lite" buildings' `BUILDING_LITE_FIRE_STAGE_ONE_DURATION`
    30 s, `_TWO_` 15 s, `BUILDING_LITE_BURN_SHADER_AMOUNT` 0.3, `BUILDING_LITE_MAX_BURN_SHADER_AMOUNT` 1.0.
  - The char (fn 0x10438340, while the entity's flag 0x8 at +0x7a is set; it sets render flag 0x200): with t the time since the fire
    started (+0x50), the burn amount is 0.3 * t / 18 during stage one (0 to 0.3); after it, 0.3 while the second timer (+0x58) is 0,
    then 0.3 + 0.7 * t2 / 10 over stage two, and 1 from there. Which event starts +0x58 was not traced; the screenshots (below) fit
    "the fire went out": the char is strongest once the flames are gone.
  - The effects (fn 0x104bb1e0): `burning_entity`, then `burning_entity_collapse` / `entity_collapse` / `burning_entity_aftermath`;
    the entity type 0xb (the small ones) takes the `_small` set (`burning_entity_small`, `burning_entity_small_collapse`,
    `entity_small_collapse`, no aftermath); `burning_entity_pre_smoke` comes first when a virtual of the entity (vtable +0x20) says so.
    The per-triangle mask of the mesh emission (fn 0x1183fa30) was not followed.
- Burning, as seen in game (2026-10-29, three screenshots from the user of a barbarian longhouse shot by onagers; the last two are 6 s
  apart): (1) flames (the building's fire effects) all over the outline and black smoke, the roof already caved in (the next
  destruction level), the walls still their own grey wood, only the fallen roof inside black; (2) fewer flames, the walls darker;
  (3) the flames out, smoke only, and the whole building, walls and roof, covered in large blotches of rusty orange and black: the
  fire shader's char and embers at the full amount (`BUILDING_LITE_MAX_BURN_SHADER_AMOUNT` 1.0). So the char builds up while the
  building burns and is strongest once the fire is out; the flames do not last as long as the char.
  The editor against it (2026-10-29; barbarian_longhouse_1, _2, ahouse_1, _2 and townhouse_2 with `burn`, at burn_amount 0.3, 0.6 and
  1): the char goes from the building's colour towards an even dark blue-grey, near black at 1, with **no rusty-orange ember
  blotches** at any amount; and the fire effects are a few small flames and smoke columns on the roof, where the game wraps the whole
  building in flames. Both are open: the shader's ember term (`s_fire_map`, its channel or uv scale) and which effect the game plays
  over the whole building. The game's building is in the south-east of TDD's Edoras (middle_men_city_edoras/medium), whose houses there
  are the nordic set (townhouse_1..3, city_building_1..3, longhouse_1, grain_store, ahouse, roundhouse; none scaled); those six
  burnt in the editor look the same: blue-black, with at most a few faint brown patches on a roof.
- **The char, settled (2026-10-29)**: the editor sampled `vfx_fire_diffuse` as sRGB; the game's `s_fire_map` samplers are linear
  (`samplers.tsv`). The texture's channels are masks, not a colour: R flickers the embers (scrolled 0.05 / s along v), B is the char
  grey (B / 4) and the ember mask, A how soon a texel chars (`(1 - A) ^ amount * amount`); uv is the mesh's uv0 times (1.2, 1.5). The
  ember colour is `2 * (0.2 + sat(5 R) * (0.53, 0.07, -0.12))`, then everything goes towards the grey by `amount - 0.025`. The G-buffer
  and forward fire shaders do the same and have no glow. Read through sRGB, every mask was darker and the embers all but vanished. Fixed
  (`renderer/passes.rs`): with linear masks, amount 0.3 gives orange streaks and patches over darkened wood, the game's look in the
  third screenshot, and amount 1 black char. So the game's char most likely settles at `BURN_SHADER_AMOUNT` 0.3 for an ordinary
  building (1.0 is the lite buildings' maximum); at 1 the shader itself leaves no orange. 
- **The flames, settled (2026-10-29)**: the game's flames over the whole building are not the pieces' `.meta` effects but the
  engine's own `burning_entity`: fn 0x104bb1e0 attaches it to the burning entity, with `burning_entity_collapse`, `entity_collapse`,
  `burning_entity_aftermath` for the later stages and `burning_entity_pre_smoke` before (the `_small` variants for an entity of type
  0xb, not identified). Its emitters (`library_burning_entity`: fire, burst_flames, embers, smoke, distortion, and a light) all have
  AllowEmitFromMesh, so they spawn on the entity's own triangles (the particle birth, fn 0x1183fa30: a triangle picked uniformly, not by
  area, hence the flames gather where the mesh is detailed: wall feet, corners, eaves). The fire emitter ramps from 10 to 100 particles
  a second over its first 13-15 s, burst_flames peaks at 60-80 a second. The editor now plays `burning_entity` on every burning piece,
  from its standing mesh (first LOD, decals left out), next to its `ip_fire` effects (`tile_document/columns.rs`). Not done: the stage
  sequence and its timing (`BUILDING_FIRE_STAGE_ONE_DURATION`, `_TWO_`), the `_small` choice, and the engine's per-triangle mask.
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

`gondor_fort_straight`'s logic box is x -10.53..10.76, but its walkways, footprint and meshes end at -10.5 and 10.5 (its frame: 21 long;
the tower and the other gondor pieces the same).
