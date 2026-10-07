# VFX particles (precipitation and effects)

Status: reverse-engineering in progress (2026-10-07). Sources: `particle_vfx_*.hlsl` disassembly (sm_4_0),
the rain and snow battle dumps (`Attila (2).DMP`, `Attila (3).DMP`), `tools/re/dump_cbs.py --array=512`.

## From weather file to emitters

- `.environment` `WEATHER definition_name` -> `weather/weather_definitions.xml` `WEATHER_DEFINITION vfx1`
  (e.g. `rain heavy_daytime` -> `weather_vfx_rain_heavy_daytime`, `snow stormy` -> `weather_vfx_snow_storm`).
- `vfx/<vfx1>.xml` (UTF-16) is usually empty itself and holds `vfx_references`: each `<vfx inst_name vfx_ref=library ...>`
  instantiates a library effect and overrides per emitter `enable`, tint min/max, ... The same library can be
  referenced twice (rain_heavy_daytime: `vfxref_153` and `more_rain`, both enabling `rain_heavy_BTL_daytime_only`):
  the dump has two identical emitter-constant rows (54, 55) for it.
- Library file: `<emitters>` (emitter type + ~60 `property`/`ranged_property`, curves as
  `name,colour;t,v;t,v|name2,...`), `<behaviour_chains>` (shader fragment modifiers: `vs_translation_velocity_v2`,
  `vs_colour`, `vs_cell_texture`, `vs_vertex_alpha`, `ps_texture`, `ps_lighting`, ...), `<effect_render_stacks>`
  pairing emitters with behaviour chains.

## GPU pipeline (game shaders)

Particle state lives in textures (float4 2D arrays, width `g_particle_instance_tex_width` = 1024):

| Layer | Contents |
| --- | --- |
| t1[0] | x emitter-instance id (-1 = dead), y random seed (0..1), z sub-step count, w 0 |
| t1[1] | position xyz, w spawn time (emitter clock) |
| t1[2] | velocity xyz, w lifetime |

Passes:
1. **spawn** (`particle_vfx_spawn` vs40_main + ps_main): one point per new particle (CPU vertex: COLOR0 = emitter
   instance id, seed, sub-steps; COLOR1 = position, spawn time; COLOR2 = velocity, lifetime). The VS takes a free slot
   from the end of the sorted-data texture (`g_sorted_data_tex_width^2 - 1 - vertex_id`, .y = slot) and writes the
   point there; the PS integrates the sub-steps (0.2 scaled) and writes the 3 targets.
2. **simulation** (`particle_vfx_simulation` ps_main_simulation): per-texel update.
3. **count / sort** (`particle_vfx_count`, `prepare_sortdata_cs`, `gpu_sort(_cs)`): alive particles sorted, written
   to `g_tex_sorted_data` (.y = particle slot); `g_num_alive_particles` (dump: rain 8426, snow 43816).
4. **draw** (`particle_vfx_main` vs40_main / gs_main / ps_main): VS by SV_VertexID over the sorted list
   (index `num_alive - vertex_id`), GS expands quads.

Emitter instance texture `g_emitter_instances` (width 256): layer 0 .w = emitter clock (particle age = clock - spawn
time); layer 2 = tint rgb + intensity (w > 0.01 required); layer 3 .x = emitter-constants index, .y = scale;
layer 4 = rotation quaternion (follow modes).

Emitter constants texture `g_tex_emitter_constants` (2D array, layer = emitter-constants index, 8 rows):
u = age / lifetime; v = 0.0625 + 0.125 * (row + random): the min/max curve pair is interpolated by the particle's
random seed with bilinear filtering. Rows 0-1 scale (xy), 2-3 colour, 4-5 ?, 6-7 alpha.

## Emitter constants (cbuffers, [512] per emitter-constants index)

Verified on `rain_heavy_BTL_daytime_only` (rows 54, 55 in the rain dump):

| Constant | Source |
| --- | --- |
| ps_sim_gravity_params | GravityAmount min, max; EffectedByWindFactor; WindVarianceFactor |
| ps_sim_following_emitter_params | x mode (3 = RecycleClipped: wrap in box around the emitter), yzw RecycleClippedBox |
| ps_sim_turbulence_params | TurbulenceStart, TurbulenceEnd, VectorFieldScale, ? |
| ps_sim_deceleration_params | DecelerationStartTime?, DecelerationAmount? (zero when Deceleration=false) |
| ps_sim_game_logic_params | x != 0: collide with terrain / sea surface |
| vs_culling_distances | ParticleCullDistances x,y,z,w: fade in between x..y, fade out z..w (camera distance, scaled) |
| vs_translation_params (gs) | vs_translation_velocity_v2: pivot_x, pivot_y, orientation_mode, velocity_stretch_mul |
| vs_atlas_texture_params | 1/atlas_cells_x, 1/atlas_cells_y, u, v of atlas_cell |
| vs_cell_texture_params (uint) | number_of_cells_x, number_of_cells_y, cell, randomize_cell |
| ps_lighting_legacy_params | x legacy_lighting?, z = emissive, w = directionality |
| ps_lighting_params | average_size, additive_blend_mul, small_particles, large_particles (the PS divides by small + large: all-zero gives NaN = black particles) |
| vs_rotation_params | vs_rotation initial_rotation min/max, rotation_over_life min/max |
| behaviour_mask (uint) | x = fragment bits; y = diffuse slice in the texture array; z = normal slice; w = view id (30, = cb_vfx.g_view_id) |

behaviour_mask.x bits: 0x1 vs_rotation, 0x2 vs_colour, 0x4 vs_cell_texture, 0x8 animated texture, 0x10 vs_vertex_alpha,
0x100 vs_translation (GS billboard, z = restriction), 0x200 vs_translation_velocity_v2 (GS, z = orientation_mode 0..4),
0x10000 ps_colour, 0x20000 ps_texture diffuse, 0x40000 normal map, 0x200000 ps_lighting, 0x800000 soft edge, 0x1000000 ps_fog.

`tools/vfx_check "<definition>" rows.txt` builds the emitters of a weather definition and compares every row with the
dump (`dump_cbs.py <dmp> cb_vfx_emitter_constants --array=512`). Result (2026-10-07): `rain heavy_daytime` -> row 55,
`rain heavy` -> row 54, `snow stormy` -> row 45 (= 265): every component matches except behaviour_mask.y, the runtime
texture-array slot. Note: dump arrays must be read row by row (`Dump.read` fails across memory-region boundaries;
ps_lighting_params straddles one).

The emitter property schema with defaults (fn 0x17ee510, "Emitter Properties") is printed by `tools/re/schema.py 0x17ee510`;
fn 0x17c1c20 copies the named properties into the emitter descriptor.

Spawn PS (per sub-step of 0.2 * dt): velocity += gravity (lerp min..max by seed, * -0.01962), wind
(wind_velocity + wind_variance_dir * WindVarianceFactor) * EffectedByWindFactor drag, central/rotational force,
vector-field turbulence; mode 3 wraps the position into the box around the emitter.

## Pixel shader output and sorting

- The main PS outputs premultiplied colour (its fog lerps towards fog colour * alpha): blend ONE / INV_SRC_ALPHA.
- `particle_vfx_simulation` ps_main writes the sort keys (distance to camera, linear slot; 1e6 for dead slots).
- `particle_vfx_gpu_sort` ps_main is one bitonic step on an RG32F texture: g_compop_direction_swap = k/2,
  g_compop_dist = j, g_compop_dist_coord2 = (j mod W, j / W), g_ascending_order = 1. Fullscreen VS = one triangle (Draw 3).
- Free slots for spawning are the tail of the sorted list (dead keys sort last), so the order per frame is
  simulate -> keys -> sort -> spawn -> draw (new particles are drawn from the next frame).
- `g_tex_vector_field` (3D) is `vfx/textures/turbulence.dds` (string next to the variable name in the DLL).
- Dump: cb_vfx_shared has instances 256/512/1024 and 256/1024/1024 (emitter / particle / sorted widths).

## Editor implementation (src/vfx)

`vfx_def` (XML), `vfx_constants` (rows above + curve texture), `weather_vfx` (definition -> active emitters),
`particles` (GPU passes with the game shaders, 512^2 slots). Verified: every emitter constant. Assumptions (not yet
traced in the DLL, marked in the code):
- CPU emission: rate from EmissionRateCurve (min/max lerp), SpawnAreaBox = centred box size, SpawnSpread = elevation,
  SpawnArc = azimuth, StartTime < 0 pre-warms with the spawn shader's 0.2 s sub-steps. Alive counts agree with the
  dumps (rain 3000/s * 1.4 s * 2 references = 8400 vs 8426 alive).
- Curve texture: scale min row = scale * (1 - ParticleScaleVariation); colour start -> mid at lerp_time -> end; width 64.
- Emitter instance tint = middle of the reference's tint range; camera-following emitters sit at the camera.
- Wind: |wind_velocity| = 22.5 * mean(wind_size_min, wind_size_max) (dumps: 5.62 for 0.2..0.3, 7.39 for 0.25..0.4),
  direction from the UI (the game randomises it per battle), no gust variance.
- Not supported yet: library_vfx_camera_effects (lens drops, screen space), non-continuous_mesh emitters (lightning),
  scene_view_type 1 (distortion pass, behaviour_mask.w = 12: heat haze), normal maps, soft-edge property name,
  shadow-receiving PS variants.

Properties repeat per graphics quality (`quality="0"` base, `"1"`/`"2"` lower-quality overrides such as
`Enable=false` at quality 2 or fewer particles at quality 1); the editor uses the base level. Verified extra rows:
deceleration ranges are copied even with Deceleration=false; vs_animated_texture = bit 0x8, atlas/cell rows as
vs_cell_texture (no_cells_xy), params (cell_blend, loop, row_restricted, randomize_row), params1 (no_loops, row_restriction).
`vfx_check` result: rain heavy_daytime, rain heavy, rain stormy, snow stormy, snow gusty, dry desert storm all match.
Particle diffuse maps are 1024^2 DXT5 with 11 mips (glow_cells_a, flame_cells_v1), so the texture array is a plain copy.
