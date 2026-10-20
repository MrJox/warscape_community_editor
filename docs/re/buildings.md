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
  the torch head), i32 bone, and from version 3 an f32 scale;
- `VOLUMETRIC_EFFECT` (2): string effect, then 20 f32, read as a position (3) and rotation (4) (zero, and the identity or all zero, in
  every file seen), the box's centre (3) and size (3) in the piece's model space, the box's rotation (4, the identity in every file seen),
  3 more (zero in every file seen). **ASSUMPTION** (centre and size): the walls' boxes span their pieces (`roman_fort_straight` piece01:
  20 x 18 x 12 m around (0, 6.5, 4.3)), the houses' theirs (`barbarian_ahouse_1`: 10.2 x 7 x 12.8 m around (0, 2.9, 0.2));
- `SOUND_BUILDING` (2): 20 bytes (not decoded).

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
  `<file>#bone=K`), the standing piece hidden meanwhile.
- Skipped levels chain (0 -> 2: `destruct01_anim`, then `destruct02_anim`). A level without a collapse (most houses; the siege engines'
  weighted collapses, not drawn) changes at once, with its bursts.
- The collapse's `.meta`: a `VOLUMETRIC_EFFECT` of dust (`destruction_vfx_area_dust_light` from intact, `_heavy` from damaged) at about
  0 s, and the `SOUND_BUILDING` entries (timed impacts; not played).
- The standing `.meta` of the new level holds bursts: `VOLUMETRIC_EFFECT`s (`destruction_vfx_generic_rigid_stone` / `_wood`,
  `_area_dust_*`, 399 in all) and some untagged `EFFECT` explosions (`destruction_vfx_medium_stone_explosion`, 31), plus
  `fire_transition` ones (108).

The editor plays, per step: the collapse's effects at their times, the new level's bursts at the step's start, and the `fire_transition`
effects of both when the building burns. Each effect entity is removed 15 s after it started (its particles live up to 8 s after its
short emitters).

The destruction effects' emitters spawn on a mesh (`AllowEmitFromMesh`, `UseSurfaceNormal`, in
`library_vfx_destruction_spawnmesh_heavy`) and have no spawn area of their own. The editor spawns them over the volume's box (its top and
four sides, chosen by area), moving out along the face's normal.

**ASSUMPTIONS**, to check in game:
- the new level's bursts play when the collapse starts (not when it ends);
- the dust's mesh is the volume's box (it may be the collapsing fragments' own meshes); its emission rate is not scaled by the mesh's size;
- an effect's bone is not followed (a few `.meta` effects name bone 0);
- the editor's playback matches the game's speed (the `.anim`'s rate and end time, real time);
- `burst_rigid` emitters (the pebbles and wooden shards flying out) are not drawn: the particle system draws only `continuous_mesh` ones
  (it logs that once per effect).

Check: in a siege battle, watch a wall section collapse from intact to destroyed, and compare with the editor's
(`tile_edit <elem> destruction_level 2` on an intact wall).
