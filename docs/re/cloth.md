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
- `uk_1` ≈ 0.031 (0.037 on the thick stays): **ASSUMPTION** the tube radius for drawing; the simulation does not read it.
- `uk_2` (0, 3, 8, 16, 32, 64): the node index released when the rope breaks under ship damage (`FUN_11801e60` frees node `uk_2`: weight 1,
  unpinned, and detaches the rope entry with that node). **ASSUMPTION** on the meaning; irrelevant without damage.
- Node `uk_1`: decreasing along the rope through ~0.5 at the middle; copied by the loader (`FUN_117c8f90`) but **not read by the simulation**
  (the renderer may use it; not traced). The loader also makes a per-node arc length 0..1 (mesh +0x584) for the texture.
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

## The wind
Built per ship per frame by `FUN_102ba0f0` (the sail item's update, dispatched by `FUN_1022d9f0`, multi-threaded), then passed as the
context of the manager's update. With R the sail item's 3×4 matrix at item +0x08 (rows; its inverse at +0x48 is used to bring projectile
segments from world to model space, so **ASSUMPTION** R is model → world):
- `(a, b, c) = SHIP_DISPLAY_WIND_SCALE (30) × (R[2][0], R[2][1], R[2][2])`: world +Z expressed in model space, × 30.
- `f = max(SHIP_DISPLAY_MIN_FORWARD_WIND (1.0), c)`, `L = SHIP_DISPLAY_MAX_LATERAL_WIND_FORWARD_SCALE (5) × f`.
- `W = −9.8·(R[1][0], R[1][1], R[1][2]) + (clamp(a, −L, L), clamp(b, −L, L), f)`: gravity (0, −9.8, 0) in model space plus the clamped wind.
- Context: `[W, 0, 0, rope break value, projectile segments, count]`; rope break value = max(`SHIP_DISPLAY_SAIL_ROPES_BREAK_VALUE` (0),
  `..._ROPES_BREAK_DAMAGE_MULTIPLIER` (1) × the ship's damage (+0x688)); the segment list is cleared after the call.

So the sails see a fixed world wind of 30 along world +Z (no battle weather or ship speed enters this path), never less than 1 forward
along the model's +z, sideways at most 5× the forward part: a ship heading +Z gets full wind, one heading −Z gets (0, −9.8, 1) and its sails
sag. **ASSUMPTION**: that nothing upstream rotates R by a weather wind direction (not traced further). The `*_SAIL_WIND_GUST` and
`SHIP_GUST_HEIGHT_OFFSET` tweakers drive sound events (`FUN_104e7340`, `FUN_104341a0`), not the cloth.

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
| `rigid_rope_culling_threshold` | 4000 | read at RVA 0x17f5f79, not traced (**ASSUMPTION** rope draw distance) |
| `rigid_show_weighted_cloth_sim_mesh`, `_unsimulated`, `rigid_show_weighted_mesh` (true), `rigid_show_colliders` | false | debug draw |

Ship side (`EmpireBattle`): `SHIP_DISPLAY_WIND_SCALE` 30, `SHIP_DISPLAY_MIN_FORWARD_WIND` 1, `SHIP_DISPLAY_MAX_LATERAL_WIND_FORWARD_SCALE` 5,
`SHIP_DISPLAY_SAILS_PROJECTILE_EFFECT_MULTIPLIER` 0.1, `SHIP_DISPLAY_SAIL_ROPES_BREAK_VALUE` 0, `..._ROPES_BREAK_DAMAGE_MULTIPLIER` 1,
`ship_display_max_sail_simulations_per_frame` 8, `ship_display_num_sail_simulations_per_thread` 2, `ship_display_multi_thread_sail_simulation`
true, `ship_display_update_all_sail_simulations` false (these four have no code reader besides the registration: their effect, likely a
per-frame budget of sails updated, is not traced), `RIGGING_UPDATE_ALL_SAILS` false, `render_disable_ship_verlet` false (read by `FUN_10473c10`,
not traced).

## Not traced
- Where R (sail item +0x08) is written, so whether a weather wind direction ever rotates it.
- How many sails are simulated per frame (the `ship_display_*` budget) and what dt a skipped sail then gets.
- The rope break threshold (+0x70), the rope node `uk_1`, the rope `uk_1` radius, and the rope particle builder (pinned assumed `w < 0.5` as for
  the cloth).
- The sail furl / half-sail states (`FUN_102bb100` animates a sail state machine; `battle_sail`, `half_sail`).

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
| 0x102589b0 | projectile segments into the sail context |
