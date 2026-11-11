# Ship sails and ropes: cloth and rope simulation (spike, 2026-11-11)

Attila's sails are a verlet particle cloth (a `Cloth` sim mesh) that skins a `WeightedCloth` display mesh, with verlet `Rope`s hanging from
the cloth or from the model and `CollisionShape` spheres keeping the cloth off the masts. Method: the sail models' numbers first
(`rmv2_mats` and a scratch v6 dump of one sail model per ship class, 11 models, 2026-11-11), then Attila's `empire.retail.dll` in Ghidra
(project `attila`). Addresses are RVAs (image base 0x10000000; `FUN_1xxxxxxx` = RVA `0xxxxxxx`). Everything here was read in the decompiled
code or checked on the data unless marked **ASSUMPTION**. Source files named by the strings: `Warscape\Source\systems\rigid\RigidMesh_v2.cpp`
(the tweakers), `EmpireBattle\Source\view\Naval\Rigging\*.cpp`.

Units: model units are metres, time in seconds. All the simulation runs in the **sail model's own space** (the ship's model space): pinned
particles stay where the file puts them and the ship's motion reaches the cloth only through the wind vector, which is given in model space.

## The data

### A sail model's blocks (LOD 0)
- `sim_<x>_LOD1`: `Cloth` (material 60), `Static` vertices, grey test textures, never drawn. The simulation mesh.
- `display_<x>`: `WeightedCloth` (58), `Cinematic` vertices (4 bone indices and weights). The drawn mesh; the factory (`FUN_1185b0c0`, case 0x3a)
  pairs it with the `Cloth` block named `sim_` + the display name without `display_` + `_LOD1`.
- `rope_mesh_NN_LOD1`: `Rope` (93), no vertices.
- `sphere_collider_NN_LOD1`: `CollisionShape` (62).
- Two kinds of cloth: sails (`sim_dm1_misc_*`, tens to 170 particles) and **rigid blocks** (`sim_dm1_rigid_*`, one triangle, 3 particles, one
  of them pinned: pulley blocks and the like hanging on ropes).

### Cloth material (rpfm `uk_7`, `uk_8`, `uk_9`)
The sim mesh is two-sided: vertices `0..n-1` are the particles, `n..2n-1` repeat them for the back faces (checked: identical positions in every
model). `n` = the length of `uk_8`. The index buffer's first half uses only the particles.
- **Particle state** comes from the vertex: position = the `Static` position's xyz (w is 1.0 in every sim vertex), weight `w` = colour alpha / 255;
  **pinned when `w < 0.5`** (`FUN_1184df00`). Data: alpha is 0 (pinned, the yard and mast corners) or 255.
- `uk_7` = **distance constraints** `(a, b, rest length)` over particle indices. Rest length = |p_a − p_b| at rest (max error 0.016 over all
  models). They are every triangle edge plus one **bend link** per pair of adjacent triangles, between the two opposite vertices (checked: all of
  them, exactly; e.g. 170 particles, 288 triangles: 457 edges + 407 bend links = 864). Solved in file order.
- `uk_8` = per particle, the **number of triangles touching it**; `uk_9` = those triangles `(i0, i1, i2)`, listed particle after particle
  (`uk_9[start_k .. start_k + uk_8[k]]`, `start` = prefix sum; each triangle appears three times, once per corner; the set equals the index
  buffer's front triangles). Used for the particle normals and frames (below), not for the dynamics.
- Material params: f32 2 = **mass multiplier** (mesh +0x110; 1.0 on every sail); f32 0 and 1 (4.0, 4.0) and i32 0 (−1) are not read by the
  simulation (`FUN_1186c880` stores f32 param k at mesh +0x108 + 4k).
- The cloth mesh object (`FUN_117cab20`): +0x56c vertex count, +0x584 the raw 32-byte vertices, +0x570 particle count, +0x588/+0x58c the
  `uk_7` links, +0x59c `uk_8`, +0x5a8 its prefix sums, +0x5b4 `uk_9`, +0x5c0 per-particle inverse rest frames, +0x5c4 per-particle normals.

### WeightedCloth material (`ClothBone`)
One bone per particle (bone count = particle count; the `vertex_index`es are a permutation of `0..n-1`). The 124-byte bone is copied whole
(`FUN_117d2d00`); the runtime reads only:
- `vertex_index` (+0x78): the sim particle the bone follows.
- `matrix` (+0x44): the bone's **inverse bind** (model → bone): rotation R and translation −R·p0, with p0 the particle's rest position
  (R·p0 + t = 0 within 0.01 for every bone of every model). Its inverse is kept at mesh +0x55c, the matrix itself at +0x578. R is the
  exporter's own frame and **cancels out** of the skinning (below).
- `parent_index` (+0x74): a skeleton bone the pinned particle follows (`FUN_11896c60`); −1 on every ship bone, so nothing is driven.

Not read by the simulation: `name` ("b"), `uk_1` (100, 200, 400), `uk_2` 12, `uk_3` 64, `uk_4` (the same 32-bit value for all bones of a mesh,
an exporter pointer), `uk_5`/`uk_6` (24 for sails, 25 for rigid blocks), `uk_7` (1.0 free / 0.0 pinned: a copy of the sim vertex's pin state,
checked against the alpha on every bone). The material's u32 after the bone count is 0 everywhere. Display f32 params 0/1 are 16.0.

### Rope material
- Nodes `(position, weight, uk_1)`, links `(i, i+1, length)` in order, length = |node_i − node_i+1| exactly.
- `weight`: 0 = pinned in model space, 1 = free. Interior nodes are always 1.
- `attachments = [v0, b0, v1, b1]`: node 0 is tied to particle `v0` of mesh block `b0` (the block's index in the LOD), the last node to
  particle `v1` of block `b1`; −1 = not attached. An attached end has weight 1 and sits on that particle (within 0.008). In the data one end at
  most is attached, the other is pinned; 31 of the 341 ropes have both ends pinned. Attached ends sit on rigid blocks or on sail particles
  (sail corners and edges).
- `uk_1` ≈ 0.031 (0.037 on the thick stays): the drawn ribbon's **half width** in model units (see "Rope drawing"); the simulation does not
  read it.
- `uk_2` (0, 3, 8, 16, 32, 64): the node index released when the rope breaks under ship damage (`FUN_11801e60` frees node `uk_2`: weight 1,
  unpinned, and detaches the rope entry with that node). **ASSUMPTION** on the meaning; irrelevant without damage.
- Node `uk_1`: decreasing along the rope through ~0.5 at the middle; **the texture's v** (see "Rope drawing"), not read by the simulation.
  The loader (`FUN_117c8f90`) also makes a per-node arc length 0..1 (mesh +0x584) that the vertex builder is passed but does not read.
- Material params: f32 2 = mass multiplier (1.0 or 1.7), f32 3 = **rest-length multiplier** (mesh +0x114: 0.8 on the long sagging stays, 1.0 on
  the others; scales every link's rest length, so 0.8 pulls the stays taut).

### CollisionShape
Sphere centre and radius are explicit in the material (`uk_1` 0 everywhere; radius 1.106 on the liburnian's mast spheres). For ships they are
static in model space (`FUN_11894710` copies the centre; the bone-driven path `FUN_11894740`, which adds `rigid_collision_z_offset` to the
centre's z, needs a skeleton). They sit along the masts, so the sails cannot fold through them.

## The solver

### Particle (0x48 bytes)
`+0x00` pinned flag, `+0x04` weight w, `+0x0c` position x, `+0x18` previous position, `+0x24` acceleration a (set once per frame),
`+0x30` rest position r, `+0x3c` one-shot displacement d (projectile hits; zero otherwise). Initial state (`FUN_1184df00`): x = previous = r =
the file position, a = 0, d = 0: **at rest, zero velocity**.

### Per frame (`FUN_118996e0`, the rigid-sim manager's update; runs only while `rigid_simulation_auto_run`)
1. Colliders take their positions (`FUN_118994e0`).
2. Each cloth (`FUN_11899620`): pinned particles follow their parent bones (none on ships); forces (`FUN_117fef10`) with W = ctx wind ×
   `rigid_simulation_force_scale`; stepping (`FUN_1188d310`); bone palette (`FUN_11890e10`).
3. Each rope not attached to a cloth (`FUN_11899590`): rope forces with the rope wind (below), stepping with the rope stiffness.
4. Each rope: break test (`FUN_11801e40`: breaks when ctx +0x14 > the rope's threshold at +0x70; source of the threshold not traced).

### Stepping (`FUN_1188d310`)
Fixed step **h = `rigid_simulation_interval` = 0.05 s** (20 Hz) with an accumulator: `dt = min(frame dt, 0.1)`, `acc += dt`, then
`while acc >= h { step; acc -= h }`. No interpolation between steps (the bones show the last step). One step:
1. **Integrate** every free particle (`FUN_1188d5a0`): `x' = x + (x − x_prev)·D + a·h² + d`, `x_prev = x`, `d = 0`, with
   **D = `rigid_simulation_damping` = 0.999** per step. Pinned particles never move.
2. Integrate every rope attached to this cloth the same way (`FUN_1188da60`), skipping the attached node, then copy the cloth particle (all
   0x48 bytes) into that node.
3. **N = `rigid_num_simulation_steps` = 10 iterations**, i = 0..9, with `s_i = (1 − i/N)²`:
   1. If `rigid_collision_test` (true) and `i % rigid_collision_test_interval == 0` (3: i = 0, 3, 6, 9): sphere collisions (`FUN_1186c1f0`).
   2. Relax the `uk_7` links in file order (`FUN_117ff750` → `FUN_118157f0`), rest = length × 1.0.
   3. Rigid blocks only (handle flag, below), while `s_i > 0.5` (i = 0, 1, 2): handle tension (`FUN_118159d0`).
   4. Each attached rope: relax its links in order with the cloth particle standing in for the attached node (`FUN_1188d8e0`/`FUN_1188da00`),
      rest = length × rope stiffness; then copy the moved cloth particle back into the node. On rigid blocks the cloth particle's weight is
      `s_i` during this (the rope's pull fades over the iterations); on sails it keeps its weight 1.

**Distance constraint** (`FUN_118157f0`), particles 1 and 2, `δ = x1 − x2`, `L = |δ|`, `k = (L − rest) / max(L, 1e-4)`: both free:
`x1 −= w1·0.5·k·δ`, `x2 += w2·0.5·k·δ`; particle 2 pinned: `x1 −= w1·k·δ`; particle 1 pinned: `x2 += w2·k·δ`. Pinned particles have w = 0.

**Sphere collisions** (`FUN_1186c1f0`), radius `ρ = r × rigid_collision_sphere_scale` (1.0), against every sphere:
- each free particle with `|x − c| < ρ`: `x_prev = x`, then `x += (ρ − |x − c|)·(x − c)/|x − c| × rigid_collision_scale` (1.1). Setting `x_prev`
  first turns the push into the next step's velocity.
- each link with both ends free: the same test on the link's midpoint, and the same push (from the midpoint) added to both ends, both
  `x_prev` reset first.
- `|x − c|` here is the bit-trick square root (below), not the exact one.

**Handle tension** (rigid blocks; `FUN_1184e060` sets it up when a rope is attached to a 3-particle cloth): the handle link is the cloth link
between the rope's particle and a pinned particle; the handle vertex is the third particle, with its rest position. Each time: the link's free
end is put at exactly its rest length from the other end along their current direction; then, if `rigid_simulation_handle_tension` (0.95) ≠ 0,
a distance constraint between the handle vertex and a pinned copy of it at its rest position with rest = 0.95 × the current distance, i.e. the
handle vertex moves 5 % of the way back to rest. Rigid blocks also skip the side-force perturbation.

### Forces (`FUN_117fef10`, once per frame, before stepping)
W = the frame's wind (model space, includes gravity; below) × force_scale. Let `n̂ = (W.x, W.z)/|(W.x, W.z)|` (zero if W has no horizontal
part), side `S = (n̂.z, 0, −n̂.x)`, along `A = (n̂.x, 0, n̂.z)`.
- **Side-force perturbation** (sails): `k = 2π / max(spacing, 0.01)` with spacing = `rigid_simulation_side_force_perturbation_spacing` (3.5 m),
  `τ = speed × t` with speed = `..._perturbation_speed` (1.0) and t the clock (the sim's time source, vtable +0x104; **ASSUMPTION** the battle
  time in seconds), `q = x + φ` with φ a per-sail random offset (three LCG values in 0..1000, `FUN_117f94c0`):
  `m = 1 + scale·(sin(k·(S·q + τ)) + sin(k·(q.y + τ)) + sin(k·(A·q + τ)))/3` with scale = `..._perturbation_scale` (1.0).
  The particle's wind is `Wp = (W.x·m, 0, W.z·m)`. Rigid blocks: `Wp = (W.x, 0, W.z)`.
- **Pressure**: with N the particle's normal (+0x5c4, from the last skinning pass; the rest normals before the first):
  `F = N·(Wp·N) + (0, W.y, 0) + restore·|r − x_prev|²·(r − x_prev)/max(|r − x_prev|, 1e-6)`; restore = `rigid_simulation_restore` (0: off).
  Only the normal component of the horizontal wind pushes; W.y (gravity) is added whole.
- `a = F / (rigid_cloth_particle_mass (1.0) × mesh mass multiplier)`.
- Ropes attached to this cloth (`FUN_1188d890` → `FUN_117fedb0`): their attached node takes the cloth particle, every other free node
  `a = (W + restore term) / (rigid_rope_particle_mass (5.0) × the rope's mass multiplier)` with this W (not scaled by the rope wind scalar).
- **Free ropes** (`FUN_117fed50`): the same with `W_rope = (W.x·0.01, W.y, W.z·0.01)`, 0.01 = `rigid_rope_wind_pressure_scalar`.
- **Projectile gusts**: the frame context carries model-space segments `(p0, p1, strength)` of projectiles that crossed the ship
  (`FUN_102589b0`; strength = `SHIP_DISPLAY_SAILS_PROJECTILE_EFFECT_MULTIPLIER` 0.1 × a projectile value). For each, `G = (p1 − p0)/|p1 − p0| × strength`,
  and for every `uk_7` link `(a, b, len)` and each free end within `0.55·len` of the segment's line: `d = G` (a one-step displacement). None
  in the editor.

Rope stiffness (`FUN_1188d530`/`FUN_1188d9b0`): `rigid_rope_flex` (1.0) × the rope's f32 param 3, or 1.0 once the rope is broken. It multiplies
the rest lengths. Free ropes step exactly like a cloth (same h, D, N, sphere collisions every 3rd iteration) with no attached ropes.

### Skinning: bones from particles
Per particle k (`FUN_11814990` at runtime, `FUN_11814c00` for the rest pose from the raw vertex positions, `FUN_118141a0` inverts it):
- For each triangle `(i0, i1, i2)` in k's `uk_9` fan, in the stored vertex order (not rotated to k): `t = norm(x1 − x0)`, `e = norm(x2 − x0)`,
  `n = norm(t × e)`, `b = norm(n × t)`. The particle normal `N_k` = norm(Σ n) is stored (+0x5c4).
- Frame `F_k` (3×3, columns): one triangle: `(t, b, n)` of it; more: from `N_k` alone (`FUN_1181ce30`, exact square roots):
  reference `r = (0, 1, 0)`, or `r = (0.0995037, 0.9950373, 0)` when `|N_k.y| > 0.95`; `u = norm(N_k × r)`, `v = norm(N_k × u)`, columns `(u, v, N_k)`.
- Rest: `F0_k` from the file positions, inverted once (3×3 adjugate / determinant; kept as is when the determinant is tiny).
- Each frame `X_k = F_k · F0_k⁻¹` (`FUN_11890e10`), and the bone's palette is `[X_k · R_iᵀ | x_k]` (R_i the bone's file rotation), applied
  after the bone's inverse bind `[R_i | −R_i p0]`. Effective transform of a display vertex bound to bone i (particle k):
  **`v' = X_k (v − p0_k) + x_k`**, blended over the 4 weights. At rest it is the identity. **ASSUMPTION**: where the inverse bind is multiplied in
  (the 4-bone shader `weightedcommon.hlsl` `vs30_main_4_bones` fetches one 3×4 matrix per bone, so the CPU does it); the result is the only
  one that leaves the rest pose undeformed.
- Rigid blocks have one triangle, so all three bones get the same rotation: the block moves rigidly.
- Normalisation in the triangle frames, the normal sum and the collision distance uses the **bit-trick square root**
  `sqrt(x) ≈ f32::from_bits(((x.to_bits() as i32 − 0x3F80_0000) >> 1) + 0x3F80_0000)` (then `1/that`); vectors come out slightly off unit length.
  The constraint lengths, wind normalisation and the normal-based frame use the exact square root.

## The wind (confirmed 2026-11-11)
Built per ship per frame by `FUN_102ba0f0` (the sail item's update, dispatched by `FUN_1022d9f0`, multi-threaded), then passed as the
context of the manager's update.

**The sail item** (0xa0 bytes, `FUN_1023e160`, made by `FUN_102570e0`): one per ship part whose model has a simulation (`FUN_1023dec0`
keeps it at part +0x30). `+0x00` the part model's rigid-sim manager, `+0x04` the ship display, `+0x08` a 4×4 matrix M (row-major, rows of
4, translation in column 3: `p' = M·(p, 1)`), `+0x48` its inverse, `+0x90` flag, `+0x94/+0x98/+0x9c` the projectile segments (capacity,
count, data). Both matrices start as the identity.

**What M is**: every frame the ship display update (`FUN_10273420`) builds the ship's display transform from its position and orientation
quaternion (the two latest simulation states, or the interpolated one from `FUN_1025b730`), and the part update (`FUN_102aa7e0`, through
`FUN_102aa700`/`FUN_102aa730`) uses that same matrix to place every part of the ship (its world bounds for culling), copies it whole into
the sail item's +0x08 and writes its full 4×4 inverse (cofactors / determinant) into +0x48. So **M is the ship's model → world matrix**,
the one its hull is drawn with; nothing else (weather, battle setup, ship speed, sail order) touches it. Row i of its 3×3 part is world
axis i in model coordinates.

With R = M's 3×3 rows:
- `(a, b, c) = SHIP_DISPLAY_WIND_SCALE (30) × (R[2][0], R[2][1], R[2][2])`: world +Z expressed in model space, × 30.
- `f = max(SHIP_DISPLAY_MIN_FORWARD_WIND (1.0), c)`, `L = SHIP_DISPLAY_MAX_LATERAL_WIND_FORWARD_SCALE (5) × f`.
- `W = −9.8·(R[1][0], R[1][1], R[1][2]) + (clamp(a, −L, L), clamp(b, −L, L), f)`: gravity (0, −9.8, 0) (a constant vector at
  RVA 0x1d24368) in model space plus the clamped wind.
- Context: `[W, 0, 0, rope break value, projectile segments, count]`; rope break value = max(`SHIP_DISPLAY_SAIL_ROPES_BREAK_VALUE` (0),
  `..._ROPES_BREAK_DAMAGE_MULTIPLIER` (1) × the ship display's sail damage d (+0x688)); the segment list is cleared after the call.
- Sail damage d (`FUN_10273420`): each frame `d = min(d + SHIP_DISPLAY_SAILS_DAMAGE_SPEED (1.0 /s) × dt, target)`, target (+0x68c) =
  `1 − min(1, ship[+0x710] / ship[+0x70c])` (set in `FUN_102904e0`; **ASSUMPTION** hit points left over the maximum). The burn value
  (+0x690) follows its target (+0x694, a 0..1 burn fraction that only grows) the same way at `SHIP_DISPLAY_SAILS_BURN_SPEED` (0.03 /s). A
  ship at full health: d = 0, no rope breaks.

**Conclusion**: in a battle the sails' wind always blows toward **world +Z** (from −Z), strength 30, for every ship and every battle; it
never varies. Ships differ only by heading (and pitch/roll, through M): heading +Z gets the full 30 forward, heading −Z gets (0, −9.8, 1)
and the sails hang, broadside gets the lateral part clamped to 5 (with f = 1). f is clamped from below at 1, so a ship always feels at
least 1 forward and its sails never blow backwards.

There is no battle wind direction on this path. What exists elsewhere, none of it reaching the sails:
- `EmpireBattle\Source\model\wind\Wind.cpp` tweakers `wind_maximum` (200), `wp_damp` (0.1), `wp_gain` (0.1): no code reads their values
  (byte scan of the DLL for their value addresses, 2026-11-11). Empire's wind model, dead in Attila.
- The battle setup field names `prevailing_wind`, `wind_strength` (a name table at RVA 0x1d229c4, next to `attacker_direction`,
  `battle_size`): not traced; not on the sail path.
- `BattleShipSailingIntoWind` / `BATTLE_SHIP_SAILING_INTO_WIND` (script event, registered at RVA 0x41f40), `g_wind_dir_offset`
  (`WindGaugeDisplay.cpp`, 0), the weather's `wind_dir_degrees` / `g_wind_direction` (shaders, vegetation): not traced; not on the sail path.
- The `*_SAIL_WIND_GUST` and `SHIP_GUST_HEIGHT_OFFSET` tweakers drive sound events (`FUN_104e7340`, `FUN_104341a0`), not the cloth.

**For the editor**: the game's battle world +Z, i.e. W from the ship's model → world rotation exactly as above; no wind setting.

## Sail states (furling)
**Attila's sails have no states: they are always fully set.** The ship's speed, its sail order, anchoring and deployment do not change the
sail model, its simulation or its wind. A stationary ship shows the same set sails as a moving one, billowing by its heading alone (above).

What was checked (2026-11-11):
- **No furled data.** One sail model per variant (`variantmeshes/_variantmodels/ships/<ship>/sails_*.rigid_model_v2`), no `*_furled*`
  model anywhere in the packs, no `.anim` for sails (the only ship animations are `animations/rome2/ships/campaign_trireme_*` for the
  campaign map and two rope destruction anims). The `Cloth` / `WeightedCloth` materials and the solver have no furl input: the pinned
  particles never move, rest lengths are only scaled by the rope flex, and the wind has no speed or order term.
- **Empire's rigging system, unused.** `logic.xml.rigging` (the `models_naval` row's rigging file) is parsed by `FUN_105c98f0`, whose
  keywords include `sail` blocks with `yard`, `surface_area`, `drag_coefficient`, `gravity_coefficient`, and a `size` of `battle_sail` or
  `half_sail` (the strings at RVA 0x1b49ffc..0x1b4a034); `FUN_10412bd0` loads such sails' `RigidModels/naval/Variant/<name>.rigid_model_v2`
  and `<name>_furled.rigid_model_v2`. None of Attila's 24 rigging files has a `sail`: they hold `lanterns` (empty, or 4 `lantern x y z`
  on the heavy liburnians), an empty `sub_systems` and at most `version 1.0`, `rope_width 0.05`; and `RigidModels/naval/Variant/` holds only oars: the path never runs.
- **`models_naval` part sail states, empty.** The damage part's u32 after its name (docs/formats/models_naval.md, "u32 unknown (0)") is
  the count of a list of wstrings read by `FUN_10fe9620` and mapped `stationary` → 1, `battle_sail` → 2, `half_sail` → 3 (other names
  dropped): **ASSUMPTION** the sail states in which that part is shown (Empire's furled / half / battle sail meshes). 0 in every Attila
  and Rome II row.
- **The sail order is movement only.** `BCQ_SHIP_ORDER_SET_SAIL` (a battle command, handler `FUN_10452230`) sets the ship's speed
  setting (`FUN_1037fbf0`, ship +0x1078: 1, 2, 4, 8, 16) and its speed fraction (`FUN_1037c940`: 0, 0.25, 0.5, 1.0, 0.5). No display code
  reads +0x1078 (every reader is in battle logic).
- **`FUN_102bb100` is not a sail state machine.** It times one animation (slot 6 of the ship model; length from `FUN_102abaf0(6)`)
  played forward (states 3 → 6) when a ship flag (+0xb15, set once by `FUN_106027a0` when the ship reaches a target position, cleared by
  the state built in `FUN_10238900`) is set, and backwards (7 → 8 → 2) when it clears; while it plays, the ship's animated parts are posed
  at that time (`FUN_10273120` → `FUN_102562a0`). **ASSUMPTION** a beaching / disembark animation; it never touches the sails.

So the editor's always-set sails are what the game shows. Nothing to add to `cloth_sim` for states.

## Rope drawing (2026-11-11)
A simulated rope (the `Rope` mesh object of a sail model, vtable RVA 0x1cbe330, loader `FUN_117c8f90`) is drawn as a **camera-facing
ribbon**: two vertices per node, rebuilt from the rope's particles every frame, offset sideways in the vertex shader. Not a tube.

**Rope mesh fields** (loader): +0x574 node count, +0x578 the nodes (20 bytes: position, weight, `uk_1`), +0x57c/+0x580 the links, +0x584 the
per-node arc length 0..1 (unused by the drawing), +0x588 the vertices (2 × nodes, 0x30 bytes each), +0x58c..+0x598 its fallback textures
(below), +0x59c.. the material header as stored (+0x5b0 = the material's `uk_1`, +0x5b4 = `uk_2`).

**Vertices** (`FUN_1189a3e0`, called each frame by vtable slot 25 `FUN_1188a180` with the rope's sim object; the sim particle positions are
in the sail model's space). Node i (of n) gives vertex 2i (A) then 2i+1 (B), 12 floats each:

| attribute | A (2i) | B (2i+1) |
|---|---|---|
| POSITION0 (xyz) | particle i's position | the same |
| NORMAL0 (xyz) | tangent t_i | the same |
| TEXCOORD0 (u, v) | (0, node i's `uk_1`) | (1, node i's `uk_1`) |
| TEXCOORD1 (x, y) | (+r, 1.0) | (−r, 1.0) |
| TEXCOORD2 (x, y) | (1, 1) for even i, (1, 0) for odd i | (0, 1) for even i, (0, 0) for odd i |

- **r = the material's `uk_1`** (mesh +0x5b0) exactly, B its sign-flipped copy (xor 0x80000000). No constant, no tweaker, no taper: the width
  is the same along the whole rope, and node `uk_1` never scales it.
- **v = node `uk_1` as stored**, not computed. It is the exporter's texture coordinate along the rope: it runs from about 0.5 + L·k/2 down to
  0.5 − L·k/2 over the rope's length L, k = 3.1..5.0 texture repeats per metre (the 44 ropes of the spike's dumped sail model; e.g. 59.7 →
  −58.8 over 23.6 m). The 0..1 arc length at +0x584 is passed to the builder but not read.
- Tangent: `d_i = normalize(p_i+1 − p_i)`, or (1, 0, 0) when |p_i+1 − p_i|² ≤ 0.001. Node 0 takes d_0, the last node d_n−2, every other node
  `(d_i−1 + d_i) × 0.5` (not renormalised; the shaders normalise it).
- TEXCOORD1.y and TEXCOORD2 are not read by the rigid techniques (below). The (0, 0) for odd B comes from a zero-filled static (RVA
  0x2a62fe4; **ASSUMPTION** never written).
- Draw: vertex count = 2n (vtable +0x44 `FUN_11863bd0`); the index-count slots (+0x3c, +0x40 → +0x48) return the same 2n, one part (+0x4c
  returns 1), and the frame update uploads vertices only. **ASSUMPTION** (topology call not traced): a **triangle strip** A0 B0 A1 B1 …,
  2n − 2 triangles; with A on the +side every triangle has the same screen winding. Cull state not traced.

**Technique: rigid.** Vtable +0x58 (`FUN_11890500`) returns 0x3b (`ROPE_SHADER` "gbuffer_rigid": `vs30_main_rigid` /
`ps30_main_gbuffer_rigid`) or, for the shadow pass, 1 ("shadowmap_rigid": `vs30_depth_rigid` / `ps30_depth`). The world techniques
("gbuffer" 0x3a, "shadowmap" 0; `ROPE_SHADER` built by `FUN_116edf60`) belong to another class, `DYNAMIC_ROPE_MESH` (vtable 0x1ccebd0, ctor
`FUN_118c34b0`, made by the "Dynamic Node" `FUN_118c3090`): 0x30-byte vertices drawn as quads (index count = vertices / 4 × 6), not the
ship ropes. The draw call (vtable slot 67 `FUN_117f5f70`) copies the instance's 3×4 model → world matrix into the render item (the rigid
technique's POSITION1..3 rows) and queues it only while the render context's +0xb8 (also its depth sort key, rounded and clamped to 0xffff;
**ASSUMPTION** the camera distance) is below `rigid_rope_culling_threshold` (4000). **ASSUMPTION**: POSITION4.w is the ship's burn value
(the item's other words were not mapped).

What `vs30_main_rigid` does with them: side = the view-space tangent's perpendicular `normalize(t_view.y, −t_view.x, 0)`, and the view-space
position moves by **side × TEXCOORD1.x × |(row0.x, row1.x, row2.x)|** (the instance matrix's x-axis length). So the ribbon is 2r wide in
model units, in the screen plane. The world technique's offset, `TEXCOORD1.x × normalize(cross(camera − P, t))`, is the same direction.
The pixel shader's profile `s = (u − 0.5) × 2` gives the normal `sqrt(1 − s²) · o1 + s · side`: u = 0 (the +r vertex) gets −side, so the
game's cylinder shading is mirrored left to right against the geometry.

**Textures** (vtable slot 26 `FUN_118870c0`, bound as `t_diffuse_map`, `t_normal_map`, `t_dissolve_map`, `t_fire_map`): the mesh's own texture
slots +0x160, +0x164, +0x184, +0x1d4 when set, else `RigidModels/naval/Textures/rope_unwrapped_diffuse.dds`, `..._normal.dds`,
`CommonTextures/dissolve.dds`, `CommonTextures/vfx_fire_diffuse`. **ASSUMPTION** +0x160/+0x164 are the material's diffuse and normal: the
ship rope materials list `attila_repeating_rope_diffuse.dds` / `_normal.dds` (128×256, DXT1 and DXT5), so those are drawn. The gbuffer pixel
shader samples only the diffuse (alpha test at 84/255), the normal (x = R, y = A, z = B) and `s_fire_map`; its only constant buffer is
`camera_VS_PS` (`time_in_sec` scrolls the fire). No rope tint or colour constant. It writes albedo `sqrt(diffuse)` (fire blended in by
POSITION4.w), RT2 = (1, 1, 1, 0).

**For the editor** (its two vertices per node with u 0 at −r, v = distance / (2r)): put **+r on u = 0 and −r on u = 1**, take **v from each
node's `uk_1`** instead of the distance, and use the averaged tangent above. With v = distance / 2r the texture repeats every 2r ≈ 0.06 m
instead of every 1/k ≈ 0.2..0.3 m: 3..5× too many twists, so the strands look 3× finer (a thinner-looking rope), the twist drops into the
smaller mips and, the DXT1 diffuse's cut-out texels being black, the albedo darkens. **ASSUMPTION** that this, not the width, is the
"3× thinner, darker" difference: the game's width is the same ±r.

## Tweakers (defaults)
`RigidMesh_v2.cpp` (registrations at RVA 0xb6910..0xb7210; float value at object +0x48):

| Tweaker | Default | Use |
|---|---|---|
| `rigid_simulation_interval` | 0.05 | fixed step h (s) |
| `rigid_num_simulation_steps` | 10 | relaxation iterations per step |
| `rigid_simulation_damping` | 0.999 | verlet velocity factor per step (cloth and ropes) |
| `rigid_simulation_force_scale` | 1.0 | × the context wind |
| `rigid_simulation_auto_run` | true | gates the manager update |
| `rigid_simulation_handle_tension` | 0.95 | rigid-block handle pull (5 % per application) |
| `rigid_simulation_restore` | 0.0 | quadratic spring to the rest position (off) |
| `rigid_simulation_side_force_perturbation_scale` | 1.0 | amplitude of the wind modulation |
| `rigid_simulation_side_force_perturbation_spacing` | 3.5 | wavelength (m) |
| `rigid_simulation_side_force_perturbation_speed` | 1.0 | phase speed (m/s) |
| `rigid_cloth_particle_mass` | 1.0 | cloth mass |
| `rigid_rope_particle_mass` | 5.0 | rope mass |
| `rigid_rope_wind_pressure_scalar` | 0.01 | free ropes' horizontal wind |
| `rigid_rope_flex` | 1.0 | × rope rest lengths |
| `rigid_collision_test` | true | sphere collisions on |
| `rigid_collision_test_interval` | 3 | every 3rd iteration |
| `rigid_collision_scale` | 1.1 | push-out factor |
| `rigid_collision_sphere_scale` | 1.0 | × sphere radius |
| `rigid_collision_z_offset` | 0.0 | bone-driven colliders only |
| `rigid_cloth_y_axis_rotation_constraint` | 360 | **unused** (no reader besides its registration) |
| `rigid_rope_culling_threshold` | 4000 | rope draw distance ("Rope drawing") |
| `rigid_show_weighted_cloth_sim_mesh`, `_unsimulated`, `rigid_show_weighted_mesh` (true), `rigid_show_colliders` | false | debug draw |

Ship side (`EmpireBattle`): `SHIP_DISPLAY_WIND_SCALE` 30, `SHIP_DISPLAY_MIN_FORWARD_WIND` 1, `SHIP_DISPLAY_MAX_LATERAL_WIND_FORWARD_SCALE` 5,
`SHIP_DISPLAY_SAILS_PROJECTILE_EFFECT_MULTIPLIER` 0.1, `SHIP_DISPLAY_SAIL_ROPES_BREAK_VALUE` 0, `..._ROPES_BREAK_DAMAGE_MULTIPLIER` 1,
`SHIP_DISPLAY_SAILS_DAMAGE_SPEED` 1.0 /s and `SHIP_DISPLAY_SAILS_BURN_SPEED` 0.03 /s (the sail damage and burn values' rates, under "The wind"),
`ship_display_max_sail_simulations_per_frame` 8, `ship_display_num_sail_simulations_per_thread` 2, `ship_display_multi_thread_sail_simulation`
true, `ship_display_update_all_sail_simulations` false (these four have no code reader besides the registration: their effect, likely a
per-frame budget of sails updated, is not traced), `RIGGING_UPDATE_ALL_SAILS` false, `render_disable_ship_verlet` false (read by `FUN_10473c10`,
not traced).

## Not traced
- How many sails are simulated per frame (the `ship_display_*` budget) and what dt a skipped sail then gets.
- The rope break threshold (+0x70) and the rope particle builder (pinned assumed `w < 0.5` as for the cloth).
- Where the sail burn value (+0x690) and the `SHIP_DISPLAY_SAILS_DAMAGE_VALUE` / `_BURN_VALUE` / `_MULTIPLIER` tweakers go (**ASSUMPTION**
  the sail material's damage / burn look, not the simulation).
- What ship +0x70c / +0x710 are (the sail damage target), and which animation slot 6 is (`FUN_102bb100`).

## Key RVAs
| RVA | What |
|---|---|
| 0x18996e0 | rigid-sim manager update (vtable 0x1cbc89c slot 1; ctor 0x17cb680) |
| 0x1899620 / 0x1899590 | per-cloth / per-free-rope frame update |
| 0x17fef10 | cloth forces (wind, perturbation, pressure, ropes, projectile gusts) |
| 0x17fedb0, 0x17fed50 | rope forces; free-rope wind scaling |
| 0x188d310 | fixed-step loop and iteration order |
| 0x188d5a0, 0x188da60 | verlet integration (cloth; rope with its attached node) |
| 0x17ff750, 0x18157b0, 0x18157f0 | link relaxation, distance constraint |
| 0x188d8e0, 0x188da00, 0x188d9b0, 0x188d530 | attached-rope relaxation, rope stiffness |
| 0x186c1f0 | sphere collisions |
| 0x18159d0, 0x184e060 | handle tension, its setup on rigid blocks |
| 0x1815340 | attaching a rope to a cloth |
| 0x184df00, 0x184cf40, 0x17f94c0 | particles from vertices, solver setup, sim object (random phase) |
| 0x17cab20, 0x17d2d00, 0x17c8f90 | Cloth / WeightedCloth / Rope mesh loaders |
| 0x1186c880 | material params → mesh (+0x108 + 4k) |
| 0x1814990, 0x1814c00, 0x18141a0, 0x1813ee0, 0x181ce30 | particle frames, rest frames and their inverses |
| 0x1890e10, 0x1896c60 | bone palette from particles; pinned particles following bones |
| 0x1801e40, 0x1801e60 | rope break |
| 0x102ba0f0 | ship sail wind context |
| 0x23e160, 0x2570e0, 0x23dec0 | sail item constructor (identity matrices), its creation, the ship part keeping it at +0x30 |
| 0x273420, 0x2aa7e0 | ship display update (ship transform, sail damage / burn rates); part update writing the sail item's matrix and inverse |
| 0x5c98f0, 0x412bd0 | Empire's `.rigging` parser (`battle_sail`, `half_sail`), its `_furled` sail loader: unused in Attila |
| 0xfe9620 | `models_naval` row reader (damage part sail states `stationary` / `battle_sail` / `half_sail`) |
| 0x452230, 0x37fbf0, 0x37c940 | `BCQ_SHIP_ORDER_SET_SAIL` handler, ship speed setting (+0x1078), its speed fraction |
| 0x2bb100, 0x290450 | ship model animation slot 6 timer (not sails) and its trigger |
| 0x102589b0 | projectile segments into the sail context |
| 0x189a3e0, 0x188a180 | rope vertices from the particles; its per-frame caller (rope vtable 0x1cbe330 slot 25) |
| 0x17f5f70, 0x1890500, 0x18870c0 | rope draw and culling, technique (gbuffer_rigid / shadowmap_rigid), texture binding |
| 0x16edf60 | `ROPE_SHADER` techniques and samplers |
| 0x18c34b0, 0x18c3090 | `DYNAMIC_ROPE_MESH` (world technique, not ships) and its dynamic node |
