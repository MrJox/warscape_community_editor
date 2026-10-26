# Weather and particle engine: what is still approximate

The engine-side leftovers of the weather tool's renderer and VFX runtime (the tool's features: docs/plans/weather.md). Everything else
(battle pipeline, wind, HDR S-curve, lens flare and vignette, depth of field, environment cube, cross-fade, render states, shadow light fit
`renderer/shadow_fit.rs`) is verified against the DLL and three memory dumps (dry, rain, snow; `empire.retail.dll` 1.6.0) and recorded in
docs/re/renderer.md and docs/re/particles.md. Rule as everywhere: what is not measured is marked `ASSUMPTION` in the code.

## Status (2026-10-26)

Open items, each with its place in the code:

1. **CPU particle emission shapes** (`wce_render` `particles.rs` ~1447 "still approximations", ~1531 `use_box`). The game (fn
   0x1895130, fn 0x1183f4a0; docs/re/particles.md, "Spawn code") adds a ring, a box and the fixed `SpawnPosition`, with one lerp(min, max)
   radius; the editor picks the box or an annulus.
2. **Spawn / accumulate alternation** (fn 0x1188e240; partial: `particles.rs` ~262 spreads a frame's particles from `last_spawn`, ~1520
   accumulates every frame). The rain dump's emitter accumulator (9.025 at rate 3000) is still unexplained.
3. **VFX constants** (`wce_assets` `vfx_constants.rs`): the scale variation rows (~191 `ASSUMPTION`: min = scale x (1 -
   ParticleScaleVariation)), the colour lerp shape (~250 `ASSUMPTION`), and `ParticlesLocalSpace` -> 2 (~165, inferred; always false in the
   shipped files).
4. **VFX curve baking** (`wce_assets` `vfx_def.rs` ~46): how the game interpolates the keys when it bakes a curve into 128 entries is not
   decoded.
5. **Schema-aware defaults for new elements**: a structural edit adds an empty child; duplicating an existing element is the workaround
   (e.g. a full `CLOUD_ELEMENT`).
6. **Particle tint** (`particles.rs` ~1847): the middle of the reference's range. The game draws one blend per reference (fn 0x117f1860);
   every shipped weather reference has equal bounds, so it is harmless for weather.

Owned by docs/plans/rendering_remaining.md: the environment cube's cloud brightness (top face ~1.8x the game's), the cloud spawn interval
(needs a dump with `cloud_velocity` > 0), the SSAO jitter content.
