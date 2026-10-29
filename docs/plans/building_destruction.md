# Building destruction (plan, 2026-10-29)

A building's damage levels and the collapses between them, generated from its intact model: the artist picks a building on the tile,
opens it in the **Destruction** mode, sets where and how it breaks, previews the levels and the collapses, and confirms; the editor
writes a new building (a new key) that works in the game as it is. What a vanilla building folder holds (pieces, levels, collapses,
`.anim`, `.meta`, the logic file) and how the editor plays collapses: [docs/re/buildings.md](../re/buildings.md). The feasibility
assessment behind this plan was a conversation with the user (2026-10-29); its findings are folded in below.

## Decisions (user, 2026-10-29)

- **Any building**, not only fort walls; a universal solution. **Gates in v1** (the leaves' destruction animations); siege engines
  (skinned bodies, looped wheels) later.
- **Always a new key**; the original is never changed. Files and rows go into the user's mod pack and the TEd polyfill pack (as
  procedural walls do); the selected instance switches to the new key, other instances stay.
- **Physics crate allowed** if it gives a decent result: `rapier3d` (Apache-2.0, pure Rust), with `enhanced-determinism`.
- **The mode**: whatever is most convenient; a dedicated mode in the main viewport is fine if it works and looks right.
- **Damage input**: presets per level (breach, top collapse, corner, full collapse) whose damage volumes are then moved and resized
  with the gizmo; sliders for fragment size and count.
- **Levels**: the artist picks 2 to 4 (intact plus 1 to 3 damage stages), 3 by default, each damage stage with a collapse into it.
- **Regenerate from the intact level**: every level and collapse comes from the intact mesh, so fragments, holes and rubble match
  across levels and animations. CA's damaged meshes are ignored.
- **Interiors**: auto-detect, thick parts (walls, towers) break solid, shells (houses) break with a wall thickness (estimated,
  adjustable) and show an empty inside.
- **Per-material behaviour**: materials are classified from their textures (overridable): stone breaks in chunks, wood in planks and
  splinters, roofs in slabs; the broken faces take the culture's damage texture for the material.
- **Rubble**: per building toggle, default by category: walls become a walkable breach, other buildings a rubble footprint units path
  around.
- **Debris**: physics plus art direction (collapse direction, force, spread, stagger, landing distance).
- **Effects**: dust, debris particles and sounds generated from vanilla templates by material and size, timed to the simulation, and
  editable.

## Ground truth (vanilla, measured 2026-10-29)

From `roman_fort_straight` (`wce_destruction` example `destruction_inspect`):

- **Static levels** (`<key>_pieceNN_destructNN.rigid_model_v2`): 5 LODs (distances 100, 200, 400, 500, 1000), no skeleton. The damaged
  level (643 KB) is four times the intact one (158 KB): the debris is baked in. The destroyed level is a rubble ramp (rubble and broken
  brick materials, little of the wall left).
- **Collapses** (`..._destructNN_anim.rigid_model_v2` + `.anim` + `.meta`): one LOD, skeleton `building`, every block bound to one bone by
  its material's `i_matrix_index`, vertices in the bone's frame (block pivots near zero for fragments). Bone 0 is the part that stays, at the
  identity throughout. Wall: 16 bones (intact to damaged), 53 bones (damaged to destroyed); 20 fps, 10 s (201 frames); the battlements'
  pieces 3 bones, 5 and 9.9 s. All bones are roots; fragments start at their place with the identity rotation (one at 180 degrees).
  Material names `building_pieceNN_destructNN_ani` (truncated), the static levels' `pieceNN_destructNN_lod01`; mesh names empty.
- **Broken faces** take culture materials of their own: `<culture>_damage_bricks_all` (exposed masonry) and `<culture>_wall_rubble`
  (heaps), for roman, greek, eastern, sassanid, persian and egyptian; barbarian buildings `barbarian_fort_wood_damage` and
  `barbarian_forts_splinteredwood`; `nordic_rubble01`, `western_wall_rubble_test`. Diffuse, normal, specular, gloss map and
  `test_mask`, `DefaultMaterial`, `params_i32 [(0, 0)]`.
- **Logic file** per level: intact 5 platforms, damaged 17, destroyed 28 (flags `false, true, false`: the rubble ramp); the damaged hard
  outline is ragged (8 points), the destroyed level has none (the breach is walkable) and attaches the bridging columns. Every level has its
  own `collision_3d` (a triangle mesh with edge adjacency). Ids (`uk_1` of `collision_3d` and of the outlines) are unique per file.
- 336 collapses (`_destruct01_anim.anim`) over 786 logic files in the game's model packs: most buildings change level without one.

## Design

Crate **`wce_destruction`** (game-neutral geometry and the files; the Attila specifics in one table), used by the Tile tool.

- **Source** (`source`): the building's folder through the VFS (`wce_walls::building::BuildingFiles`), its pieces' intact models (every
  LOD), the logic file, the zone file, its rows.
- **Damage** (`params`, `damage`): per level a preset and its volumes (oriented boxes or ellipsoids in the building's frame); damage is
  cumulative (a level removes what the previous removed and more).
- **Cells** (`voronoi`): seeds scattered in the building's box, denser in the damage volumes; convex Voronoi cells (planes, and their face
  polygons) clipped to the box. A cell whose seed is in a level's volume (with noise on the boundary, for a ragged outline) is removed at
  that level. One cell set per building, so every LOD and every level cuts the same way.
- **Solid** (`solid`): what is inside the building, for the broken faces: the generalised winding number over the LOD's triangles (robust
  on the game's open meshes), and the distance to the surface (a uniform grid). Solid parts: inside; shells: inside and within the wall
  thickness of the surface. Thick or shell is decided by the building's category and key (fort, wall, tower, gate, bastion: solid), and can
  be overridden.
- **Fracture** (`fracture`): each LOD's triangles clipped against the removed cells (Sutherland-Hodgman per triangle, every vertex
  attribute interpolated, so UVs, normals and tangents are kept exactly); what is inside a cell goes to that cell's fragment, the rest
  stays. **Caps** (`caps`): on each face of a removed cell, the solid's section is sampled on a grid and contoured (marching squares; a face
  inside the solid throughout is one polygon), triangulated, given box-projected UVs at the damage texture's scale, normals, tangents, and
  the culture's damage material for the material class of the triangles nearby. Wood fragments are split again along the grain into planks.
- **Materials** (`materials`): classes (stone, wood, roof, plaster, other) from the texture names; the culture from the key and the
  texture names; the damage material (textures that exist in the packs) per culture and class.
- **Simulation** (`sim`): rapier3d, gravity -9.81, the ground a half-space at y = 0 (the building's frame), the standing part and the
  earlier levels' debris as fixed triangle meshes, each fragment a dynamic convex hull with its material's density, friction and
  bounce. Release times from the distance to the damage's centre (stagger), an initial velocity along the level's direction (force,
  spread), a spin. Sampled at 20 fps until everything sleeps (at most the duration cap).
- **Rubble** (`rubble`): a height field around the landed debris (each fragment's volume splatted where it lies), relaxed to the angle of
  repose: a heap mesh in the culture's rubble material, and, when the rubble is walkable, the platforms of the level.
- **Levels** (`levels`): per piece and level, every LOD: the standing part (fracture) plus every earlier collapse's fragments at their
  final place plus the heap, merged per material (16-bit indices: split above 65535 vertices). Small fragments are left out of the far
  LODs.
- **Collapses** (`collapse`): per damage level, the `_anim` model (bone 0 the standing part with the earlier debris, one bone per fragment),
  the `.anim` (wce_formats::anim writer) and the `.meta` (wce_formats::anim_meta writer: dust over the volume at the start; the new
  level's bursts by material).
- **Logic** (`tech`): per level from the intact level's: platforms cut by the removed cells (pieces outside kept), firing lines, pipes,
  docking lines, emitters, attachments and soft collisions in removed cells left out, the firing lines' platforms re-indexed; the hard
  outline and footprint from a raster of the standing part (and the heap when it blocks); the rubble's platforms when walkable;
  `collision_3d` from the intact one clipped and capped like the meshes, its adjacency rebuilt; boxes recomputed; fresh ids.
- **Rows** (`rows`): the original's rows copied under the new key (`wce_walls::db::copy_rows`), `models_building` `EFLines` rebuilt from
  the new logic file, `battlefield_buildings.destruct_thresholds` for the level count.
- **Output** (`generate`): `BuildingFiles` under the new key plus the rows and notes; deterministic for a seed.

**Editor** (`wce_tile_editor` `tile_document/destruction.rs`, `wce_tile_tool`): *Destruction...* on a selected building (Inspector and
right-click menu) opens the mode: the tile hidden but for the building, the camera framed on it, a panel with the levels (preset,
volumes, fragment size and count, direction, force, spread, stagger), the materials (class and damage texture per material), the interior,
the rubble toggle, the effects list, the seed; the volumes are gizmo-editable; *Preview level N* shows a level, *Play collapse* plays it
(the files are generated into the editor's generated files, `wce_vfs::generated`, and drawn by the tile's own collapse player).
*Confirm* writes the new building into the mod pack and the polyfill pack and switches the instance's key; *Cancel* leaves.

## Open (engine questions, to check in game)

- The collapse's last frame must meet the next level's mesh (the editor assumes it; a generated building makes it exact).
- A limit on bones per collapse (vanilla's most: 53 seen); the generator caps at 64.
- `SOUND_BUILDING`'s 20 bytes are not decoded: copied from a vanilla entry of the same material.
- How the pathfinder takes a generated breach (platforms from the rubble, no hard outline): a siege battle on a generated wall.
- `destruct_thresholds` for a building that had fewer levels.

## Status (2026-10-29)

Built (branch `feature/building_destruction`, from master):

- **Formats:** `.anim` and `.meta` written back losslessly (`wce_formats::anim::AnimFile`, `anim_meta::MetaFile`), every file of the
  game's and TDD's packs to the byte (`anim_meta_corpus`); `SOUND_BUILDING` decoded (docs/re/buildings.md).
- **Generator** (`wce_destruction`): cells (a cell that is mostly wood cut in planks along its grain, `Cells::split_planks`), solid,
  fracture, caps (contour rings with holes), materials (stone, wood, roof, plaster, earth, other; a texture name that tells nothing is
  the culture's material: timber for barbarian and nordic buildings) and cultures, rapier3d collapses,
  rubble heaps, levels with every LOD (the same cells cut each LOD), collapse models and animations, effects (dust, bursts, the game's
  stone or wood sounds at the simulated impacts), the logic file (platforms cut, firing lines re-parented, hard outline and footprint
  from a raster, collision meshes cut and capped with their adjacency, walkable rubble platforms), the rows, the pack. Deterministic.
- **Editor:** the Destruction mode (docs/editor.md, building destruction row): window, live preview in the viewport (levels, collapse
  playback with effects, volumes), the selected volume moved, turned and sized with the viewport's gizmo, Confirm into the mod pack and `@wce_proc_data.pack` with the instance switched (one undo entry).
  Generated files are listed by the editor's file sources too (`wce_vfs::generated::list`), so a confirmed key draws at once.

Checks:

- `cargo test -p wce_destruction` (cells, solid, fracture, caps, rubble, sim, materials, tech, assembly, pack) and the format tests.
- `destruction_check <game> <schema> roman_fort_straight barbarian_fort_straight roman_fort_tower_b_corner_with_extension
  nordic_longhouse_1 sassanid_townhouse_3`: every file reads back, the logic file writes back through its serde form, each collapse's
  blocks are on its bones, the levels keep each piece's LOD count, two runs give the same bytes; 1 to 4 s a building.
- A scripted run of the editor (the Destruction mode on a vanilla wall in a new tile: preview, levels, both collapses played) with
  captures; Confirm was not run unattended (it writes into the game's `data`).

Measured on `roman_fort_straight` (defaults: 2 damage levels, breach): 38 and 34 fragments, collapses of 7-10 s at 20 fps; LOD 0 of the
damaged level 9.1k vertices (the game's 6.9k), destroyed 12.3k (5.1k); the logic file's damaged level 62 platforms (the game's 17).

## Open

- [ ] **In game (user):** a generated wall and a generated house in a battle: the levels swap, the collapses play (their last frame
  meets the next level), the dust and sounds play, units cross a walkable breach and path around blocking rubble, projectiles hit the
  new collision meshes, the collision mesh's open edges (`across_face_index` -1, never seen in the game's files) are accepted.
- [ ] **In TEd (user):** the building from `@wce_proc_data.pack` lists, draws and exports.
- [ ] **Gates:** the gate leaves (`_gate_closed`, `_gate_open`) are copied to every level unbroken; their `gate_*_destruct_anim` are
  not made yet.
- [ ] **Weight:** houses (hollow shells) come out at about four times their intact LOD 0 (the cut triangles, the inner sides near the
  damage); a mesh simplifier for the debris, or fewer cuts, would bring it down. Platforms on rubble are many small quads.
- [ ] **Engine questions** above (`destruct_thresholds` for 2 and 4 levels, the collapse's last frame).
