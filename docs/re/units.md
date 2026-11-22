# Units: skinning, meshes, shaders, animations and assembly (Attila, Rome 2; 2026-11-06 / 07)

What the game does to draw and animate a unit, from Attila's vanilla packs and the shaders' disassembly unless marked **ASSUMPTION**.
Formats already specified: `attila-toolkit/skills/attila-file-formats/references/{anim,bone_inv_trans_mats,rigid_model_v2}_spec.md`.
The editor's unit rendering: docs/plans/units.md; the Unit tool: docs/plans/unit_editor.md. Ships' sails, ropes and oars:
[cloth.md](cloth.md); slot names the engine looks up: [variant_mesh_slots.md](variant_mesh_slots.md). The scratch scripts and
disassembly are in `~/wce_re/scratch/units`, the screenshots in `~/wce_re/scratch/units/ref`.

## Skinning

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

So thousands of soldiers are one instanced draw per mesh, each with an offset into one texture.

## Meshes

All 3,210 models under `variantmeshes/_variantmodels` decode with rpfm_lib, no failures. Tally of LOD 0 blocks in the unit folders:

| Material | Vertex format | Where | Needed |
|---|---|---|---|
| Weighted, WeightedDirtmap | Weighted (2 bones) | most clothing, armour, helmets, hair, mounts | first |
| WeightedSkin, WeightedSkinDirtmap | Weighted, Cinematic | skin | first |
| Weighted*, WeightedSkin | Cinematic (4 bones) | `man/skin` cinematic heads, cloaks | first: the shader only differs by inputs |
| WeightedDecal, WeightedDecalDirtmap | Weighted | a few trousers, greaves, elephants | second |
| DefaultMaterial, Dirtmap, DecalDirtmap | Static | weapons, crests, bosses, shields | attached rigid parts |
| Cloth, CollisionShape | Static, Collision | 4 + 8 cloak blocks; the rest are ship sails | not for units at first |

Most models have 4 or 5 LODs, with distances in the file (100 / 200 / 400 / 500 on a sample).

## Pixel shaders

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
`skinlm`; `ps30_gbuffer_skin_slm` writes what `slm` reads, and which the engine uses when is not traced.

Not known: the engine's choice of entry. The names give material to family (Weighted, WeightedDirtmap -> `weightedwithdirt`,
WeightedDecal -> `weightedwithdecal`, WeightedDecalDirtmap -> `weightedwithdecaldirt`, the same for WeightedSkin*), `_skin` for the
WeightedSkin types, `_noclip` by the alpha mode (only 1 is alpha-tested). **ASSUMPTION** until screenshots agree.

## Animations

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

## Assembly

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
- Definitions: 8,507 files under `variantmeshes/` (slot names: [variant_mesh_slots.md](variant_mesh_slots.md)), only 4,894 of them under
  `variantmeshdefinitions/`; most are a part's file (a helmet's, a shield's), where references point.
- `.texture_array` is a text list of texture paths (decals).

## The looks tables (DB, 2026-11-14, Attila)

Checked with the `wce_render` example `unit_tables_check`, vanilla and with TDD:

- `variants` has only `variant_name` (the schema's `variant_filename`, `scale`, `mount_scale` columns are other games' versions), so a
  unit's `variant` is its VMD's file name: 1440 of 1445 vanilla rows find their file (the 5 others are transport crews with none), 2087
  of 2112 with TDD (25 of TDD's Arnor and Gondor units name files it doesn't ship).
- `unit_variants_colours`: a unit's own colours by faction and soldier type (soldier, officer, standard bearer, musician), 2 rows in
  vanilla, 57 (28 units) in TDD.
- A `unit_variants` row is told apart by its unit and faction (the schema's key columns), not its `name`, which repeats per faction.

## Other games

Rome 2 has the same picture: 5,925 anims (5,894 v5, 31 v4), `weightedcommon` with the same entries, 145 skeleton files, fragments as text.
Later games differ (`.anim` v6+, binary fragments, `wsmodel`): the seams go behind the game profile, nothing more for now.

## Comparison with the game (2026-11-07)

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
- Shots 14 to 17 (2026-11-07, Rome medium settlement, the Sagittarii and comitatenses up close, graphics maxed) are Mediterranean: `weather/rome_med`, nine files,
  not the seven above. `evening_med_2` fits: pink horizon, violet sky. There, soldiers facing the sun (`WCE_UNIT_TURN`), the tunics' yellow, green and red, the shading
  and the skin agree by eye and the face's blue cast is gone: olive cloth reads yellow in sun and green in shade, so the tint and mask assumptions hold. The game
  looks brighter and more saturated, by a factor not measured: the regions sampled were different soldiers, and a stone city and a flat field meter differently.
  The small black mark on the game's tunics, from their dirt map, hasn't been seen in the editor.
- With TDD enabled every one of these weathers is one file: `tdd_pack1` overrides them all with the same blue sky (sun at 24 degrees). Compare with
  the mods off.
