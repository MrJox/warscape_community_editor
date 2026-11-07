# Units: rendering and animation

Draws units through the game's own weighted shaders, animated. It serves three consumers: the tile editor's scale reference (and later
historical battle scripting), the Cinematic tool (the most pressing, after the terrain editor) and a possible unit / VMD editor (not
decided). This is the renderer and data work they share, not any of those tools. Formats already specified:
`attila-toolkit/skills/attila-file-formats/references/{anim,bone_inv_trans_mats,rigid_model_v2}_spec.md`.

State: investigated and decided 2026-11-06 (decisions at the end). Steps 1 to 3, 5 and 7's `ref_human` are done, step 4's code, and step 6's idle: the Tile tool shows any unit of the DB
standing in a tile, a mount under a rider, with materials and skin lighting, looping its idle, TDD's trolls too. Step 4's comparison with the game has begun (screenshots in `~/wce_re/scratch/units/ref`).
Waiting for the screenshots' weather to compare the lighting. The scratch scripts and disassembly are in
`~/wce_re/scratch/units`.

## What the game does

Checked on Attila's vanilla packs unless marked **ASSUMPTION**.

### Skinning

`weightedcommon.hlsl` `vs30_main` (2 bones) and `vs30_main_4_bones`, read from the disassembly:

- Bone matrices are **not in a constant buffer**. They are in `t_animation_map` (t0, Texture2D of floats), read with `ld`. A bone takes 3
  consecutive texels, the rows of its 3x4 matrix: `texel = instance offset + 3 * bone`, wrapped at `g_animation_texture_width`
  (`variant_animation_size`, cb2), so `x = texel % width`, `y = texel / width`.
- The instance's offset is the first `TEXCOORD` after its world rows: `v9.x` in the 2-bone shader, `v10.x` in the 4-bone one, because the
  4-bone vertex has one input more (POSITION0..6 against POSITION0..5). The world matrix is the three instance rows before it.
- 2-bone vertex: bone indices in bytes 0 and 1, the first bone's weight in byte 2, the second bone's `1 - w`. rpfm_lib's second weight is
  byte 3, always 0 (sums down to 0.5): `anim_pose::vertex_weights` is the rule, and the raw bytes go to the GPU as stored. A 4-bone vertex
  has four indices and three weights in the shader, the last `1 - sum`; as stored, the four add up to 1.
  Position, normal and both tangent vectors are all skinned with the same blend.
- The palette holds skin matrices (`pose * inverse bind`). The engine's `WEIGHTED_MESH_ANIMATION_TEXTURE_CACHE` confirms it: each
  `TEXTURE_LINE_STACK` pairs a texture line and offset with a `matrix_stack` and an `inv_t_pose`, filled in parallel into a ring of
  textures, with a `dirty` flag.

So thousands of soldiers are one instanced draw per mesh, each with an offset into one texture. The editor copies this.

### Meshes

All 3,210 models under `variantmeshes/_variantmodels` decode with rpfm_lib, no failures. Tally of LOD 0 blocks in the unit folders:

| Material | Vertex format | Where | Needed |
|---|---|---|---|
| Weighted, WeightedDirtmap | Weighted (2 bones) | most clothing, armour, helmets, hair, mounts | first |
| WeightedSkin, WeightedSkinDirtmap | Weighted, Cinematic | skin | first |
| Weighted*, WeightedSkin | Cinematic (4 bones) | `man/skin` cinematic heads, cloaks | first: the shader only differs by inputs |
| WeightedDecal, WeightedDecalDirtmap | Weighted | a few trousers, greaves, elephants | second |
| DefaultMaterial, Dirtmap, DecalDirtmap | Static | weapons, crests, bosses, shields | attached rigid parts |
| Cloth, CollisionShape | Static, Collision | 4 + 8 cloak blocks; the rest are ship sails | not for units at first |

Most models have 4 or 5 LODs, with distances in the file (100 / 200 / 400 / 500 on a sample). Today `model_cache` flattens `Weighted` into
a static mesh posed once on the CPU (`pose_first_frame`) and rejects every other weighted material.

### Pixel shaders

`weighted.hlsl` has `ps30_gbuffer`, `_noclip`, `_no_faction_colour`, `_skin` (plus `_noclip`, `_no_faction_colour`, `_slm`), `_unlit`,
`_imposter*`, `_alpha_blend`, `_alpha_overlay*`, the forward variants, and `weightedwithdirt`, `withdecal`, `withdecaldirt`, `withburn`,
`withblood` families. They write the same five targets as the rigid shaders. Skin reads `s_skin_mask_map`, and the directional and point
lights have `ps30_skinlm_*` entries next to the `slm` one the renderer uses (`DIR_LIGHT_PS`). Faction colours are `rigid_faction_colours`
(three in `faction_colours`) against `s_colour_mask_map`.

Instance inputs, from `ps30_gbuffer` and `ps30_gbuffer_skin` (2-bone numbering): `v12` is the tint, rgb with `.w` 1, 2 or 3 picking the
colour mask channel it applies to (VMD `TINT`, `mask0..2`), read by the non-skin entries only: `.w` 0 leaves the mask to the faction colours,
any other value sets the other channels white; `v9.y` scales the fourth faction colour; `v9.zw` offset the dirt map's uv (0 here; the engine's
value per soldier is not traced); `v10.zw`
blend the colour and gloss towards something (**ASSUMPTION**: dirt and blood levels; 0 leaves them alone); `v11.y` reaches the pixel shader
unused here; the pixel selection id is `t1` at `instance id + cb1[4].w`, as the rigid shaders. Skin puts its mask in the third target, which
is how the light pass can tell it.

Skin lighting (2026-11-07). The engine registers a lighting pass per model with a number: default 0, `slm` 1, `ulm` 2, `skinlm` 3
(fn 0x116e4b60, and 0x116d4720 the same for the sun) and its lights compare the stencil with mask 3 (capture), so skin pixels are stencil 3
(**ASSUMPTION**, read from those; no capture has skin). `ps30_gbuffer_skin` writes the colour unsquared and the skin mask in channel 3, for
`skinlm`; `ps30_gbuffer_skin_slm` writes what `slm` reads, and which the engine uses when is not traced. The editor draws `_skin` with
stencil `SKIN_STENCIL` and a second sun and point light pass of `skinlm` over them; the fog takes them through `ds_geometry`.

Not known: the engine's choice of entry. The names give material to family (Weighted, WeightedDirtmap -> `weightedwithdirt`,
WeightedDecal -> `weightedwithdecal`, WeightedDecalDirtmap -> `weightedwithdecaldirt`, the same for WeightedSkin*), `_skin` for the
WeightedSkin types, `_noclip` by the alpha mode (only 1 is alpha-tested). **ASSUMPTION** until screenshots agree.

### Animations

- 6,225 `.anim` (226 MB): 6,194 version 5, all of which match the layout to the byte. 3,880 use `rome_man_game`.
- 31 are version 4 (siege towers, archers firing from walls): UTF-16 strings and no track tables. After the bones, two bit sets of the
  moving bones, the frame count, then per frame every bone's translation and rotation in f32, so they need no skeleton.
- **Bones without a track keep the skeleton's pose**, not the identity the format spec says. A normal clip has 6 translation tracks
  (`bn_hips`, `bn_weapon_01..05`) and 32 rotation tracks of 50 bones. The other translations, and the fingers, eyelids, eyes and jaw, come
  from the skeleton (`animations/skeletons/<name>.anim`, frame 0): `Animation::on_skeleton`. Checked by `unit_pose_check`: bone lengths stay
  exact with it and change by 0.44 without it.
- Convention checked: `world = parent world * local` over frame 0 of `rome_man_game.anim`, times its `.bone_inv_trans_mats`, gives the
  identity for all 50 bones within 1.4e-4. So `anim_pose::inverse_binds` and the bind pose are right.
- `rome_man` (105 bones, with the face) shares its first 50 bones and their order with `rome_man_game`, so a battle clip drives a cinematic
  head; `animations_cinematic/.../faces` adds the facial clips.
- Clips run at 10, 15 or 20 fps; `end = (frames - 1) / fps`.
- Fragments (`animations/animation_fragments/*.txt`, plain text, 126) map 71 slots (`STAND`, `WALK_1`, `FIRE`...) to clips, with several
  clips per slot, `.meta` and `.snd.meta` paths and flags (`blend_in_time`, `weapon_bone_N = on`).
  `animations/animation_tables/animation_tables.txt` groups fragments by name with their `skeleton_type`.
- Clip `.meta` entries (27 kinds, `anim_meta::Meta`), by count: sounds, `RHAND_POSE` / `LHAND_POSE`, `WEAPON_RHAND` / `LHAND` / `HIP` / `ON`, `SPLICE`
  (another clip over part of this one), `BLOOD`, `PROP`, `ANIMATED_PROP`. Bows are `ANIMATED_PROP`s: the bow's model with its own clip,
  on a bone. Finger poses come from the hand entries, which is why clips have no finger tracks. A pose number's clip is not traced (the
  campaign's `hand_poses` folders hold such poses).

### Assembly

- `variantmeshdefinition` is XML: `VARIANT_MESH` (`model`, `decal_diffuse`, `decal_normal`, `imposter_model`, `mask0..2`),
  `VARIANT_MESH_REFERENCE` (`definition`), `SLOT` (`name`, `attach_point`, `probability`: 0 to 1, how often it is filled, **ASSUMPTION**),
  `TINT` (`red`, `green`, `blue`, `random_range`), `META_DATA` (text: `female`). TDD ships 11 definitions that aren't well-formed XML
  (doubled quotes, stray end tags): the loader skips them with a warning. One `mask3`, ignored. A slot with
  several children picks one (**ASSUMPTION**: uniform random); a variant nests its own slots.
- `attach_point`: `weapon_01..05` (the root bones `bn_weapon_NN`, which clips animate) and `crest_centre` (**ASSUMPTION**: an attachment
  point in the helmet's RMV2, as `material.attachment_points`). Static weapons and shields follow the bone's world matrix: checked, shield on
  the forearm and sword in the hand in the stand and walk clips.
- Mounts: a rider's table names its mount's (`mount_table rome_mount_horse_sword_shield`), as it names `skeleton_type_cinematic rome_man`.
  Rider and horse clips (`RIDER_STAND` with the horse's `STAND`) are in one frame with one origin and the same length: both figures at the same
  world matrix fit, feet by the flanks, hands at the neck, no offset. Checked with `horse_rider_stand` and `horse_stand`.
- DB: `land_units` (`man_animation`, `man_entity`...) -> `battle_animations_table` -> animation table; `unit_variants` (`unit`,
  `faction`, `variant`, `height_scale`, `height_variation`) -> `variants` -> VMD file; `mounts` (`animation`, `entity`, `variant`).
  `.texture_array` is a text list of texture paths (decals).

### Other games

Rome 2 has the same picture: 5,925 anims (5,894 v5, 31 v4), `weightedcommon` with the same entries, 145 skeleton files, fragments as text.
Later games differ (`.anim` v6+, binary fragments, `wsmodel`): the seams go behind the game profile, nothing more for now.

### Comparison with the game (2026-11-07)

The user's screenshots (`~/wce_re/scratch/units/ref`): vanilla, spring, day, dry, Terra Incognita land map, north-west of Scotland. The database
gives 7 files for that (`default_map`, Atlantic or Continental): `day_north_1`, `morn_north_1`, `morn_north_2`, `evening_north_1..3`, `morn_6`, and
each battle picks one at random. By their skies (by eye, not from the game) shots 1 to 5 are `morn_north_2`, shots 6 to 13 `morn_north_1`.

`unit_render_check` in `morn_north_1`, 48 soldiers, the sun behind them (`WCE_UNIT_COUNT`, `WCE_UNIT_CAM`), against shot 11: shields, mail, tunics and
faces agree by eye. Nothing was tuned for it. Sampled, editor against game (regions picked by hand, different soldiers): white shield (109, 118, 130) /
(108, 120, 134), blue (26, 43, 61) / (25, 42, 62), mail (23, 25, 29) / (25, 26, 28); wine (42, 28, 29) / (33, 22, 21) and a face (71, 62, 69) / (76, 63, 57)
are further off, and the face's blue is the one to watch.
- The tone mapper's black point adapts from the frame's darkest luma, by the time between frames, and the first frame snaps to it. A check that
  renders at one `time_s` has dt 0: the point stays as the first frame left it, with nothing loaded yet (0.58 here, 0.004 with the clock running),
  and every dark colour sits on the floor. `frames_running` is for pictures; this cost a long detour through materials and lighting.
- Decal diffuse is sRGB: the game's sampler table says so, and linear overshoots ((48, 63, 77) against the game's (25, 42, 62)). Faction colours
  aren't what makes the wine shields wine: it is their decal.
- With a bright flat test ground the reflections warm a white shield (138, 109, 87); the game's dark grass doesn't. Not a bug of the units.
- With TDD enabled every one of these weathers is one file: `tdd_pack1` overrides them all with the same blue sky (sun at 24 degrees). Compare with
  the mods off.

## Plan

Each step ends with something visible and a gate (a Cargo example) or a shot.

1. **Spike** (done 2026-11-06). `unit_pose_check` poses a real soldier and a horse with its rider on the CPU, with no GPU, and draws them.
   It found the track fallback and the two-bone weights; it stays as the reference for step 3's gate.
2. **Formats** (done 2026-11-06, `wce_formats`): `anim` (versions 4 and 5, whole), `anim_meta::Meta` (every kind, encodes back to the byte),
   `variant_mesh` (a tree over an `XmlDoc`, so an edit touches only its value), `anim_tables`. Gate `unit_formats_check`, over every file of
   Attila, Rome 2 and Attila with TDD: no failures. It does not check meanings: the fields step 6 reads are still named by position.
3. **Skinned draw path** (done 2026-11-06). `Payload::SkinnedInstance` (parts, a palette the entities can share, faction colours; replacing it
   costs no structural change), `MODEL_SKINNED_SUFFIX` in the model cache, `renderer/skinned.rs`: one dynamic palette texture and instance
   buffer written once a frame, one instanced draw per mesh, G-buffer and sun shadows through `weightedcommon`. Gate `unit_render_check`:
   the picture against the same vertices skinned on the CPU, 2 and 4 bones. Left for later: no culling at all (the clipper does it; fine for
   a few units, not for a crowd), not drawn in the environment cube, Rome 2's baked shadows, or with faded opacity. Not tried with MSAA or
   in a tile. Picking: the G-buffer's id target gets 0, and the id pass doesn't know them; `ref_human` needs one of those in step 7.
4. **Materials.** Done but for the comparison with the game's screenshots: the four families, dirt, decals (their textures come with the part,
   from the VMD in step 5), faction colours, tints, skin lighting for the sun and point lights.
5. **Assembly** (done 2026-11-07 but a tile). `unit_assembly::assemble`: a definition and a seed give a soldier's parts:
   slots by `probability`, references, tints, mask remaps, decals (`.texture_array`s pick one). Static parts (shields, weapons, crests) are in the
   `SkinnedInstance` too, on a bone (`SkinnedInstance::bones`) or an attachment point of their parent's model, and `renderer/skinned.rs` draws
   them through the rigid shaders with their own decal and colours: `rigidwithdecalmesh`, `rigidwithdecaldirtmesh` are drawn nowhere else.
   The picks use the editor's generator, so a seed is not the game's soldier. `unit_catalog` reads `unit_variants`, `main_units`, `land_units`,
   `mounts`, `factions` and the animation tables, mods included; a unit's mount is a second figure at the same origin. Left: the whole model in
   a tile (metres in game, `1 / unit_scale` in the tile scene), weighted weapons other than bows.
6. **Animator.** `unit_animation::UnitModel`: `instances(t)` is a pure function of the time, so a timeline can scrub it. Done: slots from
   the table's fragments, loops, hand poses (a clip's `RHAND_POSE` / `LHAND_POSE` numbers are the `HAND_POSE_*` slots in the engine's order,
   `BATTLE_ENTITY_ANIMATION_TYPE`), the rig of the clip's own skeleton (TDD's trolls: the table says `rome_man_game`, the clip `humanoid04f_s068`).
   Bows (done 2026-11-07): at rest the definition's rigid bow on `bn_weapon_04`. A clip's `PROP` / `ANIMATED_PROP` entries hold another model from their
   start to the clip's end; a bow's has a skeleton (`shortbow_bendy`) and a clip of its own, in the figure's space, so it is placed at the figure's origin
   and its bone only says which static part it replaces (**ASSUMPTION**). `UnitModel::play` plays a slot; `UnitModel::instances` gives the props as
   instances of their own.
   Schedules (done 2026-11-07): a looping list of clips built from the seed, a slot's lines by `selection_weight` and its `<slot>_IDLE_n` mixed in, each
   blending in from the one before over its `blend_in_time`; a mount follows its rider's schedule; a soldier starts at a phase of his own. `SPLICE`s
   (`META_DATA_SPLICE`) over the body regions, as poses or additive. `WEAPON_ON` shows a bone's parts from its start. `unit_render_check` steps a
   schedule at 30 fps and fails on a bone moving over 0.3 m in a frame, or two poses for one time.
   Crossbows, slings and pikes look right, standing and firing. `weapon_bone_N` flags and `WEAPON_RHAND`, `_LHAND`, `_HIP` are left unused on measurement: the
   clips' tracks carry the weapons (2500 clips; see `unit_animation.rs`). `bn_weapon_05` is the ammunition's: its parts show only under a `WEAPON_ON`.
   A clip's travel on the ground (`bn_hips`, first to last frame) is taken out of its root bones, so a walk loops on the spot; `Clip::travel` is there for a
   timeline that moves a unit.
   `single_frame_variant` is on plain looping clips too (a horse's `STAND` is nothing else): such lines play only in a slot without other lines, the first.
   What the flag does in the engine isn't traced. Left: `stretch`, face poses. Splices are unseen: no standing clip has one.
7. **First consumers.** `ref_human` (done 2026-11-07, `wce_tile_tool/src/ref_human.rs`): the Tile panel's Reference unit section, or the command
   `tile_ref_human`. A scene layer of its own and the settings (which unit, per tile where it stands), never the tile's files. It can't be
   picked: Place moves it. It stands on the ground as it is now, through terrain edits. No viewer in the Unit tool yet: `ref_human` and `unit_render_check` are the test bench so far.
8. **Later, for cinematics.** Close-ups once units render for reference: facial clips on `rome_man`, `cinematics_only` models, baked cloth
   clips (`cinematic_dynamiccape_roman`, `army_banner_cine_sim`). Rider and mount in sync in motion, layering and matched combat as the
   tool needs them.

The first goal is steps 1 to 7 with units standing or idling for reference. Steps 1 to 6 stay what they are, but in step 6 only what an
idle needs comes first (one slot, loops, hand poses); blending and the rest of the slots can wait for the Cinematic tool.

Mounts move earlier than the first draft had them: comparing cavalry with infantry needs a horse and its rider together in step 5, standing
at least. It costs little: the clips already fit (see Assembly); what is left is `mounts`' `variant` to the horse's VMD.

Out of scope: battle simulation, formations, pathing, sounds and blood, dismemberment, imposters, real cloth simulation.

## Decisions (user, 2026-11-06)

- **`ref_human`** is the scale reference, named as in Terry. No prop of that name exists in Attila's or Rome 2's packs, TDD's or the
  kit's raw files, so it is a real unit, not a prop. Editor only: never in TEd's files or an export. Better storage waits for historical
  battles.
- **Unit:** a default per game, which the user can switch to any other, to test differently sized units. Attila's default:
  `att_rom_legio_comitatenses_west`, what `unit_variants` gives the Western Romans' `att_rom_comitatenses`-type units; the bare
  `att_rom_legio_comitatenses` has one body and no hair, and no unit uses it. A unit's definition comes from `unit_variants` (`unit`, `faction`),
  not from its name. Nothing may assume a human skeleton: TDD brings its own (`rom_troll_v2`, `humanoid01d`, `gob_cave_troll`'s
  VMD), so the skeleton comes from the model and the animation table, not from a constant.
- **Priority:** units for reference first, close-ups later. No crowd target for now.
- **Oracle:** screenshots of the game, taken by the user; RenderDoc does not work here. The skin lighting is the riskiest part to match
  this way.
- **Faction colours, tints and variation seed** are exposed directly, in `ref_human`'s Inspector too, not only as renderer inputs.
- **Unit tool viewer:** dev-only.
- **Parsers** live in `wce_formats`.
