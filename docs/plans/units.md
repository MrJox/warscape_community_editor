# Units: rendering and animation

Draws units through the game's own weighted shaders, animated. It serves three consumers: the tile editor's scale reference (and later
historical battle scripting), the Cinematic tool (the most pressing, after the terrain editor) and a possible unit / VMD editor (not
decided). This is the renderer and data work they share, not any of those tools. Formats already specified:
`attila-toolkit/skills/attila-file-formats/references/{anim,bone_inv_trans_mats,rigid_model_v2}_spec.md`.

State: investigated and decided 2026-11-06 (decisions at the end). Step 1's CPU spike is done; the renderer has nothing yet. The scratch scripts and disassembly are in
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
colour mask channel it applies to (VMD `TINT`, `mask0..2`), read by `ps30_gbuffer` only; `v9.y` scales the fourth faction colour; `v10.zw`
blend the colour and gloss towards something (**ASSUMPTION**: dirt and blood levels; 0 leaves them alone); `v11.y` reaches the pixel shader
unused here; the pixel selection id is `t1` at `instance id + cb1[4].w`, as the rigid shaders. Skin puts its mask in the third target, which
is how the light pass can tell it.

Not known: the engine's choice of entry. The names suggest material to family (Weighted, WeightedDirtmap -> `weightedwithdirt`,
WeightedDecal -> `weightedwithdecal`), `_skin` for the WeightedSkin types, `_noclip` by the alpha mode. **ASSUMPTION** until screenshots
agree. Which light entry reads the third target (`skinlm` or the `slm` in use) is step 4's first question.

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
- Clip `.meta` tags: `WEAPON_RHAND`, `WEAPON_LHAND`, `RHAND_POSE`, `LHAND_POSE`, `SPLICE`, `SOUND_TRIGGER`, `IMPACT_POS`, `BLOOD`,
  `WOUNDED_POSE`. Finger poses come from these, which is why clips have no finger tracks. `anim_meta.rs` reads other tags and the campaign
  `hand_poses` folders hold the poses; the battle mapping is not traced.

### Assembly

- `variantmeshdefinition` is XML: `VARIANT_MESH` (`model`, `decal_diffuse`, `decal_normal`, `imposter_model`, `mask0..2`),
  `VARIANT_MESH_REFERENCE` (`definition`), `SLOT` (`name`, `attach_point`), `TINT` (`red`, `green`, `blue`, `random_range`). A slot with
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

## Plan

Each step ends with something visible and a gate (a Cargo example) or a shot.

1. **Spike** (done 2026-11-06). `unit_pose_check` poses a real soldier and a horse with its rider on the CPU, with no GPU, and draws them.
   It found the track fallback and the two-bone weights; it stays as the reference for step 3's gate.
2. **Formats** (`wce_formats`, as rpfm_lib decodes headers only): `.anim` complete (names, skeleton, v4, fallback to a skeleton, track
   lookups kept compact), `.meta` tags, VMD with a lossless model for a later editor, fragments and animation tables. Byte-identical round
   trips where a file will be written (VMD at least).
3. **Skinned draw path.** `model_cache` keeps weighted vertices as stored (strides 28 and 32) in a skinned `GpuModel`; a skinned scene kind
   (`Kind::SkinnedInstance` is declared and unbuilt; it needs a `Payload`); `R32G32B32A32_FLOAT` palette texture, written
   once a frame with lines allocated per instance, identical pose + skeleton shared; G-buffer and shadow passes through `weightedcommon`;
   picking through `t_pixel_selection_object_id_texture`. Gate: one rigged part in the bind pose over a CPU-skinned reference, then a clip.
4. **Materials.** Weighted, Dirtmap, Skin, SkinDirtmap, Decal families, faction colours, tints, masks, skin lighting. Compared with the game.
5. **Assembly.** VMD resolved by a seed into parts: weighted parts on the body's palette, static parts on their bone, height scale. The
   whole model at the right scale in a tile (metres in game, `1 / unit_scale` in the tile scene).
6. **Animator.** A pure function `(unit, clip, time) -> pose`, so a timeline can scrub it: slots from fragments, loops, blending, hand
   poses, weapon bone flags. Time comes from outside; no global clock.
7. **First consumers.** `ref_human` in the Tile tool (decisions below); a small viewer in the Unit tool as the test bench, dev-only
   (`TODO(pre-release)`). Tested with infantry, cavalry and TDD's trolls.
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
  `att_rom_legio_comitatenses`. Nothing may assume a human skeleton: TDD brings its own (`rom_troll_v2`, `humanoid01d`, `gob_cave_troll`'s
  VMD), so the skeleton comes from the model and the animation table, not from a constant.
- **Priority:** units for reference first, close-ups later. No crowd target for now.
- **Oracle:** screenshots of the game, taken by the user; RenderDoc does not work here. The skin lighting is the riskiest part to match
  this way.
- **Faction colours, tints and variation seed** are exposed directly, in `ref_human`'s Inspector too, not only as renderer inputs.
- **Unit tool viewer:** dev-only.
- **Parsers** live in `wce_formats`.
