# Units: rendering and animation

Draws units through the game's own weighted shaders, animated. It serves three consumers: the tile editor's scale reference, the
Cinematic tool ([cinematic.md](cinematic.md): its preview, movies and historical battles) and the Unit tool, a VMD editor
([unit_editor.md](unit_editor.md)). This is the renderer and data work they share, not any of those tools. Formats already specified:
`attila-toolkit/skills/attila-file-formats/references/{anim,bone_inv_trans_mats,rigid_model_v2}_spec.md`.

State (2026-11-22): investigated and decided 2026-11-06 (decisions at the end); steps 1 to 7 and 7b built 2026-11-06 .. 11-11, in
master: the Tile tool shows any unit of the DB standing in a tile (a mount under a rider, materials and skin lighting, idles, TDD's
trolls too) and ships afloat with their men, oars, sails and ropes; the Cinematic tool poses units in its slots. Compared with the
game's screenshots by eye 2026-11-07 (below). Left: what each step lists, and step 8. The scratch scripts and disassembly are in
`~/wce_re/scratch/units`, the screenshots in `~/wce_re/scratch/units/ref`.

## What the game does

Skinning, meshes, pixel shaders, animations, assembly, other games and the comparison with the game's screenshots (2026-11-06 / 07):
[docs/re/units.md](../re/units.md). In the editor:

- Units load with `MODEL_SKINNED_SUFFIX` (step 3); without it the model cache still poses an `_anim` model's Weighted blocks once on
  the CPU (`pose_first_frame`) as a static mesh.
- The editor draws `_skin` with stencil `SKIN_STENCIL` and a second sun and point light pass of `skinlm` over them; the fog takes them
  through `ds_geometry`.
- Flicker on helmets, faces, mail and bosses (user's recording and frame logs, 2026-11-07): `prepare_skinned` picked the LODs from `self.camera`, which on the fourth
  frames the environment cube is rebuilt on is still the cube's, above the battle's centre: every soldier took another LOD for that frame. It needs a camera far from
  the centre, and the checks had the soldiers on it. `unit_render_check` now puts the centre 900 units away and wants still frames identical (64,611 channels changed
  before the fix, 0 after). Found by rebuilding the cube every frame and never (no flicker), then a per-frame log of draw counts. A milder shimmer of self-shadow from
  the cascades' depth range is left as the game has it: PLAN.md, after the final baseline.

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
   a few units, not for a crowd), not drawn in the environment cube, Rome 2's baked shadows, or with faded opacity. Not tried with MSAA.
   Picking: the G-buffer's id target gets 0 and the id pass doesn't know them; the tile's references and the Unit tool pick on the CPU
   instead (step 7, unit_editor.md).
4. **Materials.** Done: the four families, dirt, decals (their textures come with the part, from the VMD in step 5), faction colours, tints,
   skin lighting for the sun and point lights. Compared by eye with the game's screenshots 2026-11-07 (docs/re/units.md, "Comparison
   with the game"): they agree; the game looks brighter by a factor not measured, and the dirt map's small black mark is not seen in the editor.
5. **Assembly** (done 2026-11-07; in a tile with step 7). `unit_assembly::assemble`: a definition and a seed give a soldier's parts:
   slots by `probability`, references, tints, mask remaps, decals (`.texture_array`s pick one). Static parts (shields, weapons, crests) are in the
   `SkinnedInstance` too, on a bone (`SkinnedInstance::bones`) or an attachment point of their parent's model, and `renderer/skinned.rs` draws
   them through the rigid shaders with their own decal and colours: `rigidwithdecalmesh`, `rigidwithdecaldirtmesh` are drawn nowhere else.
   The picks use the editor's generator, so a seed is not the game's soldier (since 2026-11-14 a generator per node, for the Unit tool's pins).
   `unit_catalog` reads `unit_variants`, `unit_variants_colours` (a unit's own colours, 2026-11-14), `main_units`, `land_units`,
   `mounts`, `factions` and the animation tables, mods included; a unit's mount is a second figure at the same origin. Left: weighted weapons other
   than bows.
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
   What the flag does in the engine isn't traced. Left: `stretch`, face poses. Splices play in the idles (`STAND` and most `STAND_IDLE_n` have one, 2026-11-07), not looked at closely.
7. **First consumers.** The references (`wce_tile_tool/src/ref_units.rs`; `ref_human` 2026-11-07, as entities with `ref_unit` the same day): tile elements
   `WceRefHuman` and `WceRefUnit`, placed from the Create panel's References, as many as wanted, saved in `tile.wce` and dropped by `to_ted`, so selection, gizmo,
   undo, groups and the Scene panel are the elements' own. `ref_unit`: `main_units.num_men` at the game's `gfx_unit_size`, `land_units.rank_depth`,
   `unit_spacings` by `land_units.spacing`, close formation; 16 soldiers (`UNIT_VARIATIONS`) posed once a frame and shared by the men, 0.5 ms for 160 men.
   **ASSUMPTION**: a quarter of the men per step of `gfx_unit_size`: the game's formula wasn't found, only the debug panel that names it. Attila's `main_units` has
   no `unit_scaling` column: there every unit scales, which artillery and generals may not. Deployment gaps, officers and loose formation aren't modelled.
   Skinned meshes aren't in the id pass: a click finds a soldier by a sphere per man (`Tile::set_ref_spots`), and there is no selection outline on them. They
   follow the ground man by man; freeform keeps them level at the element's height. The Unit tool (2026-11-15, unit_editor.md) is the
   viewer now.
7b. **Naval units** (`ref_ship`, 2026-11-11; decided with the user: a third reference, the men placed by the ship's own logic, afloat on the
   water, all of the ship in scope). Tile element `WceRefShip` (Create panel's References, `tile_ref ship`), its `unit` a naval `main_units` key
   (Attila's default `att_shp_rom_heavy_marines_lib_heavy`). The tables: `main_units.is_naval` -> `land_unit` (the men; their `unit_variants` rows
   are under the land unit's key) and `naval_unit` -> `naval_units.ship` -> `ship_dbs.model` (a `models_naval` key) and `unit_variants_ships`
   (the ship's definition). `models_naval` is the ships' compiled logic, which rpfm doesn't read: `wce_formats::models_naval`
   (docs/formats/models_naval.md, every row of Attila and Rome 2, byte for byte, against the kits' `logic.xml`; gate `models_naval_check`).
   `wce_render::ship_model`: the definition's intact pieces (`_dm1`; `_dm2` are the damaged states), a shield on every point of a slot's list,
   the oars (`models_oars`) on the logic's oar sockets, the weapons (`naval_weapons` -> `models_entity_weapons`) on its engine lines, as
   rigid mesh instances (picked by the picture), and the men on its Marines EF lines (docs/formats/models_naval.md, "Men aboard": the
   lines highest first, 0.7 m slots centred on each, facing along the line; the first N slots: the stern platform first). Afloat: on `Tile::water_level` (rivers, lakes' planes, the sea at 0 m in the sea's sets), the ground where that is
   higher; freeform keeps the element's height. **ASSUMPTION**: a ship's origin is its waterline.
   Sails and ropes (2026-11-11). A sail model holds `display_*` WeightedCloth meshes (Cinematic vertices, four bones: the cloth simulation's,
   rpfm's `cloth_bones`), `sim_*` Cloth meshes (the simulation's own, grey test textures) and their colliders, never drawn, and Rope meshes
   without vertices: each a chain of nodes in its material (rpfm's `Rope`). The sails go through the skinned path at rest (an identity
   palette: the vertices as stored) with `weightedcommon`'s four-bone shaders and `weightedcloth.hlsl` ps30_gbuffer (`MeshKind::WeightedCloth`),
   their masks and the faction's colours (or the reference's own); they cast shadows. The ropes: `<model>#ropes` (MODEL_ROPES_SUFFIX,
   `MeshKind::Rope`), two vertices per node, through `rope.hlsl`'s rigid technique (vs30_main_rigid, ps30_main_gbuffer_rigid: a strip turned to
   the camera in view space, shaded as a cylinder, uv x across). That technique reads the instance's POSITION4.w as a burn (0 to 1), not the
   rigid materials' opacity: a ropes instance has it 0 and is never filed as faded. **ASSUMPTION**s: the material's `uk_1` (about 0.03) is
   the radius; the texture repeats once per diameter along the rope; the ropes come out nearly black (the shader writes specular (1, 1, 1)),
   not compared with the game yet. Ropes cast no shadow (`rope.hlsl`'s depth techniques aren't wired) and aren't in the id pass.
   Motion (2026-11-11). On the battle sea (the tile's sea on, `tile_sea`, with a wind's waves: `Renderer::sea_wave_height`, the sea and
   swell surfaces as the ocean's chunks are displaced), `ShipModel::afloat` samples the waves under the logic's buoyancy spheres and fits a
   plane through them, each by its mass: heave, pitch and roll, men and parts with it. **ASSUMPTION**: the game floats ships with a physics
   simulation over those spheres (their `connections`); the editor shows where the ship would rest on the waves of the moment, without
   inertia (a storm: about 1 degree of pitch and roll, decimetres of heave, smooth). Rivers and lakes stay flat. `rowing` ("1", the
   Inspector's Rowing, `tile_ref ... rowing`): the oars run their `models_oars.left_row` / `right_row` clips; without it they hold the clip at
   their own offset (up to `MAX_OAR_ANIM_OFFSET` 0.25 s: a ship that never moved; docs/re/cloth.md, "Oars"), blades in the water, as the
   game's idle ships. These moves go through
   `RenderScene::update_world_quiet`: the draw lists follow them, without a new scene revision every frame. Level 1 is taken as the lowest oar
   row and the primary weapon as the `Engine_placement` line's (**ASSUMPTION**s).
   Sails' cloth (2026-11-11): the game's verlet simulation, traced in docs/re/cloth.md and run on the CPU by `wce_render::cloth_sim`
   (`ClothRig` from a sail model, `ClothSim` per reference): the `sim_*` meshes' particles, links and triangle fans, the ropes tied to them
   and the free ones, the masts' spheres; 20 Hz fixed steps, ten relaxations, the wind of the ship's heading (`ship_wind`: 30 along world +Z,
   gravity, clamped) with its travelling side perturbation. Each frame the sails' palette (`X_k (v - p0_k) + x_k` per bone) goes to their
   skinned entity (`update_pose`), and the model's ropes to a `Payload::Ropes` entity drawn by `rope.hlsl`'s world technique
   (`renderer/ropes.rs`; the static `#ropes` strips are for models without a cloth). A model's sails share one palette, each mesh from its
   `GpuMesh::palette_base`. The wind is the ship's world matrix's (traced: nothing rotates it, no weather or battle wind reaches the sails;
   every ship feels 30 along world +Z). Attila's sails have no states: always set (Empire's furling code is there, its data isn't).
   The ropes are built as the game builds them (docs/re/cloth.md, "Rope drawing": half width the material's `uk_1`, v the nodes' `uk_1`).
   A particle's pin is its sim vertex's colour alpha: rpfm keeps those bytes raw (`uk_1`) for the Static layout; read from `color` (zero),
   every particle was pinned and nothing moved but the ropes until 2026-11-11 (`cloth_rig_dump` lists what a sail model builds).
   **ASSUMPTION**s (docs/re/cloth.md): the clock of the perturbation; the wind's per-sail phase uses the editor's
   generator, not the game's. Not done: projectile gusts and rope breaks (no battle in the editor). The moving ropes cast shadows (`rope.hlsl` vs30_depth / ps30_depth into each
   cascade, after the units; thinner than a shadow texel, they come out dotted); a model's static `#ropes` strips don't.
8. **Later, for cinematics.** Close-ups once units render for reference: facial clips on `rome_man`, `cinematics_only` models, baked cloth
   clips (`cinematic_dynamiccape_roman`, `army_banner_cine_sim`). Rider and mount in sync in motion, layering and matched combat as the
   tool needs them.

The first goal was steps 1 to 7 with units standing or idling for reference (reached 2026-11-07); blending and the rest of the slots came
with step 6's schedules, the moving slots with the Cinematic tool.

Mounts move earlier than the first draft had them: comparing cavalry with infantry needs a horse and its rider together in step 5, standing
at least. Done in step 5: the clips already fit (docs/re/units.md, "Assembly"), and `mounts`' `variant` gives the horse's VMD.

Out of scope: battle simulation, formations, pathing, sounds and blood, dismemberment, imposters, real cloth simulation (the ships'
sails got the game's own verlet simulation in 7b; `ref_unit` draws a close formation).

## Decisions (user, 2026-11-06)

- **`ref_human`** is the scale reference, named as in Terry; **`ref_unit`** the same with a whole unit. 2026-11-07: entities the user places, not one per tile. No prop of that name exists in Attila's or Rome 2's packs, TDD's or the
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
- **Unit tool viewer:** dev-only. Replaced 2026-11-14: the Unit tool is a tool of its own behind the `unit` feature
  ([unit_editor.md](unit_editor.md)); kept out of the `release` set (2026-11-21, not ready for production).
- **Parsers** live in `wce_formats`.
