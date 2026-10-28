# Rome 2's effects (VFX)

Rome 2 simulates its particles on the CPU and draws them with a vertex and pixel shader the game generated per effect file
(`dx11_sm_3_0_vfx_<effect>_generated.hlsl`). Attila's GPU particle system (`particle_vfx_*.hlsl`, docs/re/particles.md) does not exist
in it. This is what `crates/wce_render/src/vfx_rome2` ports. Everything below is from `empire.retail.dll` (Win32, Ghidra; function
addresses `FUN_...`, fields as offsets) and the shaders' disassembly. The scratch notes, with more detail, are in
`~/wce_re/scratch/rome2/vfx_re_notes.md`.

## Files

- `vfx/<effect>.xml`: UTF-16, the same structure as Attila's (`emitters`, `behaviour_chains`, `effect_render_stacks`, `vfx_references`),
  an older property set. Emitter classes: `continuous_mesh` and `burst_mesh` (billboards), `continuous_ribbon`, `burst_rigid` and
  `continuous_rigid` (models: lights, debris, decals).
- The shaders: per effect `vs30_main`, and the pixel shaders `ps30_main`, `ps30_main_high_quality`, `ps30_distortion`, `ps30_overlay`,
  `ps30_shadowmap` (also under the `enable_shadows_*` prefixes). Library effects referenced by others have their own.
- Textures: the chains' `ps_texture` `diffuse` / `normal` are names under `vfx/textures/`.

## Descriptors

Each emitter class has a descriptor (base fn 0x114b2b80, continuous 0x114b1aa0, burst 0x114b14d0) holding its XML properties with the
constructor's defaults. The ones that differ from 0: `Enable`, `Visible`, `EventEnable`, `SpreadMethod` true; `Density` 40; `SpawnSpread`
90; scales 1; `ParticleScaleMidPointTime` 0.5; `SpawnRadius.w` 360. Continuous: `LifeTime` 50, `ParticlesPerSecond` 10,
`SpawnSpacingMaxSpeed` 80, `ParticleSpeedMax` 1000. Burst: `Bursts` 10, `BurstInterval` 2, `ParticlesPerBurst` 10, `ParticleSpeedMax`
1000. The burst class names its "infinite" flag `InfiniteBursts`; it is the base's `InfiniteLife` field.

## The generator

One global LCG (DAT_11a46f50): `x = x * 0x343fd + 0x269ec3`, a draw is `(x >> 16) / 65535`. Every emitter draws from it, so the order of
the draws is part of the behaviour. Ranged properties are `draw * (max - min) + min`. The update's square roots use the bit trick
`as_float(((as_int(x) - 0x3f800000) >> 1) + 0x3f800000)` (an arithmetic shift).

## Tweaks

Registered values (object + 0x48): `VFX_ALPHA_CUTOFF` 0.01, `VFX_PRE_INITIALIZE_TIME_STEP` 0.2, `VFX_SORT_PARTICLES` true (not in the
sea overlay or shadow views), `VFX_WIND_SCALE_FACTOR` 20, `VFX_WIND_ANGLE_DELTA` 5, `VFX_DISTANCE_CULLING_SCALE_CAPPED` true,
`VFX_FAR_DISTANCE_CULLING_BIAS` 0, `VFX_DEFAULT_DISTANCE_FROM_CAMERA` 0.01.

## Effect instances

An effect instance (fn 0x115639a0, every frame): the emitters go to the instance's position (plus a reference's offset turned by its
parent and times the scale) and orientation; one draw makes the wind's turn of the frame, `(2 draw - 1) * dt * 5`; the camera distance
is `fastsqrt(|camera - position|^2)` (0.01 when camera-following). Then every emitter runs its state machine with (time, active, wind,
distance, camera, previous camera, turn). The battle's wind is (environment wind x * 20, 0, 0) (fn 0x115e8c20).

A file with `vfx_references` is made of child instances, one per reference (fn 0x11507640): `scale` (at least 0.01), `pos`,
`start_time` (added to the emitters' StartTime), `infinite_life` (an emitter is infinite when its reference is, and its own flag), and per
emitter `enable` and `tint_r..a` (kept as bytes).

## Emitters

State machine (billboards: fn 0x11562cf0 continuous, 0x11562870 burst):

- Every frame the distance culling (fn 0x11506600) from `CullDistances` (x, y, z, w), with the scale `s` = instance scale * emitter scale,
  `near = min(s, 1)`, `far = s <= 1 ? 1 : s`: stopped beyond `w * far`; hidden under `x * near` or beyond; a fade in from x to y and out
  from z to w.
- State 0 waits for StartTime; a negative StartTime runs the steps from `now + start` to now 0.2 s apart (the effect starts as if it
  had been running). State 1 steps. State 2 (stopped or inactive): an infinite emitter drops its particles and, unless stopped, starts
  again (its init); else it is done.

Init (the classes' vtable +4; both start with fn 0x11529480: StartTime drawn, EmitterRotation as a quaternion):

- Continuous (fn 0x11528bb0): the accumulator starts at 1 (the first step spawns at once); LifeTime and ParticlesPerSecond drawn. The pool:
  `round(ParticleLifeTime.max * rate)` (not infinite: at most `trunc(life * rate)`), plus one, at most ParticlesMax when set.
- Burst (fn 0x115288a0): Bursts drawn (1 when infinite); the next burst time `-ParticleLifeTime.max` (negative: the first burst is due).
  The pool: `round(ParticlesPerBurst.max * k)`, k the bursts alive at once `(life + interval + length) / (interval + length)` (not
  infinite: at most Bursts).

The pool (fn 0x114f1dc0): particles in slots, live ones from `first` on; a new one takes `first - 1`, a dead one swaps with the one at
`first`, which moves up. Records stay in slot order: that is the draw order before the sort.

Steps (fn 0x11566350 continuous, 0x11564ec0 burst): the emitter moves by its velocity; dead particles leave (`spawn + life < now`); the
particles update; the spawning; the draw runs (live, alpha over the cutoff).

- Continuous: `accum += min(dt, 0.1) * rate`; `floor(accum)` particles (at most the free slots), spawned at `now + i / rate` with a drawn
  lifetime. (With SpawnSpacing: along the emitter's path instead, fn 0x1155f730, not ported.)
- Burst: the bursts due (fn 0x1150d710: from the next time, each burst draws its length and interval); per burst a drawn count
  (`ParticlesPerBurst`), spawn times spread over the burst's length, lifetimes drawn; the next time is the last burst's time + length +
  interval.

Spawn (fn 0x11519380): the position from the SpawnRadius ring (angle draw, radius draw) and the SpawnArea box (three draws), times the
scale, turned by EmitterRotation and its spin, plus SpawnPosition, turned by the emitter's orientation; the direction a cone of
SpawnSpread (elevation and azimuth draws) or away from SpawnPosition, turned the same way. Then speed, DecelerationStartTime,
DecelerationAmount, GravityAmount, three draws for the record's random vector, RotationalForce and CentralForce; the sizes get
ParticleScaleVariation from the random vector (fn 0x114ed360). The continuous class takes the slot before these draws, the burst after
the motion draws.

Update (fn 0x11568640): wind (EffectedByWind: the wind plus WindVarianceFactor * |wind| along the emitter's turning phase); the size
start -> mid -> end over the life; the velocity (fn 0x115691f0: rotational and central forces about the emitter's axis, turbulence
(six draws per particle), deceleration towards the wind, gravity `-0.0981 * GravityAmount`, the speed cap); the position; follow /
local space; the colour (instance colour * tint, alpha * the emitter's fade); the fade of ParticleCullDistances by camera distance.

## The record (vertex) and the shaders

One record per particle, 31 floats, the instance data of a four-corner quad (TEXCOORD0 the corner, 0..1):

| Input | Floats | Content |
|---|---|---|
| POSITION0 | 0..3 | spawn time, lifetime, emitter index, texture slots (diffuse + 10 * normal) |
| TEXCOORD1 | 4..7 | random vector, scale |
| POSITION1 | 8..11 | previous position, 0 |
| POSITION2 | 12..15 | position, now |
| POSITION3 | 16..19 | spawn position, 0 |
| POSITION4 | 20..22 | velocity |
| POSITION5 | 23..26 | 1, 1, size x, size y |
| POSITION6 | 27..30 | colour |

The emitter index (fn 0x1155fbd0) is the emitter's position in the file (the emitter, chain and render stack orders agree in every
shipped file); the vertex shader branches on it for the behaviour chains' constants. The texture slots (fn 0x11550a80) index the file's
unique diffuse and normal textures, bound as `s_diffuse1..` in the same order. States (VFX_SHADER_BASE, fn 0x114bc6f0): premultiplied
blending (one, inverse source alpha), depth tested, not written; samplers linear wrap; the depth (`gbuffer_channel_4_sampler`) for soft
particles. `cb_vfx`: the camera's axes, `g_viewport_offset_scale` (0, 0, 1, 1).

## Views and sorting

`scene_view_type` 0 DeferredAlpha (main / main_high_quality), 1 Distortion, 2 SeaColourOverlay; `cast_shadows` adds the shadow map view
(fn 0x114fabd0). Per view (fn 0x115e3d10) the visible particles of every effect are gathered, sorted by camera distance squared, far
first, with the game's `std::sort` (fn 0x11578dd0, MSVC 2012's introsort; equal keys are common and keep its order), copied into one
buffer and drawn in runs of one effect.

## Not ported yet

Spawn spacing, SpawnByMesh, ribbons, particle events (`property_event` OnParticleSpawn child effects), the rigid classes (the effects'
point lights, debris and decals), the Distortion, SeaColourOverlay and shadow views, and Rome 2's weather particles.
