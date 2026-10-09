# Attila Weather Editor — Plan

Standalone C++20 / D3D11 / Dear ImGui editor for Total War: Attila weather and lighting.
Reads game data through RPFM, renders with the game's own compiled shaders, writes loose XML byte-identically.

## Agreed scope (2026-10-06)

| Topic | Decision |
| --- | --- |
| Stack | C++20, MSVC 2022, CMake, D3D11, Dear ImGui. No Assembly Kit dependency. |
| Data access | `rpfm_cli` extracts from packs (vanilla + `used_mods.txt` load order) into a cache. No own pack reader/writer. |
| Output | Loose files only, byte-identical round trip (attribute order, quoting, whitespace, line endings, encoding, untouched number text). |
| Fidelity | Pixel-close. The game's compiled shaders are loaded and run directly. |
| Preview scene | Test map with randomly picked buildings. Real tiles / TEd replacement is a later phase. |
| Materials | Only the most common one first: `RS_STANDARD_V5` (68). Others come in later phases. |
| Modes | Battle and campaign. |
| In scope | Particles, VFX, precipitation. |
| Later | DB tables (read first, then write). |

## Research findings

**Weather and lighting files are text XML (ASCII, CRLF)**
- **Battle:** `weather/**/*.environment`. That's 488 files in `data.pack`, plus TDD's in `tdd_pack6_weather_1.0.0.pack`. Elements:
  - `LIGHTING`, `POST_PROCESSING`, `FOG`, `DEPTH_OF_FIELD`, `HDR`, `WEATHER`
  - `SCENE_NODE_LIST > WEATHER_SCENE_NODE > SKY_DOME > SKYGEN`
  - `CLOUD_PLANE_MESH > CLOUD > CLOUD_ELEMENT` (near and far planes, about 60 cloud elements per file)
  - `SUN_MESH`
- **Value syntax:** plain floats/bools, `rgb(r,g,b)`, `argb(a,r,g,b)`, `variance(a,b)`.
- **`weather/weather_definitions.xml`:** `WEATHER_DEFINITION` (wind gusts, vegetation wind, `vfx1` precipitation), `WEATHER_PARTICLE_DEFINITION`, `WEATHER_DEFINITION_REF` / `_GROUP`. The `WEATHER@definition_name` attribute in each `.environment` points to an entry here.
- **Campaign:** `SCENE_ENVIRONMENT` `.lighting` files, one per season:
  - `terrain/campaigns/<map>/environment/{spring,summer,autumn,winter}.lighting` (`terrain.pack`)
  - `rigidmodels/skybox/default_campaign_*.lighting` (`models.pack`)
  - campaign skybox: `campaign_maps/<map>/display/skybox/` (`campaign_skybox.rigid_model_v2`, `cubemap.dds`)
- **Porthole/UI lighting** (`porthole/**.lighting`) uses the same format and comes almost for free.

**No keyframes in the weather files (your question 7).** Every `.environment` and `.lighting` is a static snapshot. Change over time comes from:
- engine cross-fades between environments (the DLL string `start_lighting_environment_cross_fade`), e.g. season to season on the campaign map;
- wind gust curves in `weather_definitions.xml`;
- per-particle curves in VFX (`ranged_property`).

The editor will offer a cross-fade preview slider between two files. That matches what the game does, and nothing in the files can be keyframed.

**VFX:** `vfx/*.xml`, **UTF-16 LE with BOM**, CRLF. They contain:
- `shader_library` (`fx/particle_vfx_library.hlsl`)
- `emitters`, `behaviour_chains`, `effect_render_stacks`
- `property`, `ranged_property` (curves), `modifier`, `texture_property`
- `vfx_references` to library VFX (e.g. `library_vfx_weather_snow`)

Weather VFX are `att_bel_weather_*`, `weather_vfx_*`, `campaign_clouds_*` and `campaign_mountain_cloud`.

**Shaders:** `data.pack/fxc/*.fxc` (5443 files, DX11 + GLSL).
- Each file is a CA header followed by a **standard DXBC blob with an RDEF chunk**. The CA header has magic `0x075BCD15`, the define list (e.g. `ENABLE_SHADOWS`, `SM_5_0`) and the include list with hashes.
- RDEF means constant buffer and resource names, offsets and types come for free through `D3DReflect`. No guessing of register layouts.
- File names encode `defines_source.hlsl_stage_entrypoint`, e.g. `dx11_sm_5_0_rigidstandard.hlsl_ps_ps30_gbuffer`. The include list shows the pipeline modules: `gBuffer`, `lighting`, `sky`, `fog`, `weather`, `wind`, `parallel_split_shadowing`, `sm5shadows`, `hdr_tone_mapper_shared`, `colorimetry`.
- Pipeline seen from the shader set: deferred G-buffer → directional light + point lights → environment specular → sky/clouds → fog → HDR tone mapper → `hdr_to_screen` (DOF and distortion variants). There are also forward HDR/LDR variants for alpha.

**Models:** the 2279 vanilla building `.rigid_model_v2` files in `models2.pack` (TDD: 3300 in `tdd_pack5`) contain 21,619 meshes. Breakdown by type:

| Type | Meshes | Share |
| --- | --- | --- |
| `RS_STANDARD_V5` (68) | 17,348 | 80% |
| `RS_STANDARD_TILED_DIRTMAP_V5` (63) | 2,842 | 13% |
| `RS_TERRAIN_BLEND_V5` (86) | 699 | |
| `RS_STANDARD_AMBIENT_V5` (64) | 530 | |

All are vertex format 0 with lighting constants `default_dry`.

**DB tables** for later:
- battle: `battle_weather_types`, `battle_weather_effects`, `battle_sky_types`, `battle_climate_weather_descriptions`, `battles_to_battle_sky_types_junctions`
- campaign: `seasons`, `campaign_vfx_descriptions`, `campaign_vfx_lookups`

## Architecture

```
weather_editor/
  src/core/      vfs (rpfm_cli cache + load order), lossless XML (UTF-8/UTF-16), logging
  src/formats/   environment/lighting model, weather_definitions, vfx, rmv2, dds, fxc container
  src/render/    D3D11 device, game-shader registry (fxc + reflection), CB binder, passes, test scene
  src/editor/    ImGui shell, property inspector, file browser, cross-fade, camera
  tools/         round-trip tester, shader dumper, RE notes/scripts
  docs/re/       reverse-engineering notes (addresses, CB mappings, pass order)
```

- **Lossless XML.** A small tokenizer keeps every byte span: prolog, comments, whitespace, quote char, attribute order and the original value text. An edited attribute only rewrites its value span, formatted the way the game writes it (`rgb(1.0,0.788235,0.615686)`, 6-decimal floats, `'` quotes). Unedited files save byte for byte.
- **Parameter schema.** One table per element type: attribute name → type (float/bool/rgb/argb/variance/enum/string), range and the shader constant it drives. The inspector is generated from this table, so every parameter in the files is exposed, including unknown ones (shown raw).
- **Game shaders.** Load the `.fxc` → strip the CA header → `CreateVertexShader`/`CreatePixelShader` from the DXBC → `D3DReflect` for the CB layout. CB variables are filled by name from the parameter model through mappings that I reverse-engineer from `empire.retail.dll` and record in `docs/re/`.
- **Render loop.** A parameter edit marks the affected CBs dirty and re-uploads them in the next frame, so changes show immediately.

## Phases

### Phase 0: Foundations and reverse-engineering spikes
1. CMake skeleton, git repo, D3D11 + ImGui window, free camera.
2. VFS:
   - resolve the pack order (vanilla `manifest.txt` + `used_mods.txt`);
   - list files with `rpfm_cli pack list`;
   - extract on demand into a short cache path (MAX_PATH 260!) keyed by the pack's size and mtime;
   - apply the newest-pack-wins override.
3. Lossless XML reader/writer + round-trip test over **every** `.environment`, `.lighting`, `weather_definitions.xml` and `vfx/*.xml` in vanilla and TDD. **Gate:** 100% byte-identical.
4. FXC loader: parse the CA header, create every DX11 SM5 shader, dump the RDEF of each to `docs/re/shaders/`. **Gate:** all SM5 `.fxc` files create without errors.
5. RE spike:
   - from the DLL, map `.environment` attributes to CB variables (string xrefs on attribute names → setters → CB writes);
   - find the render pass order, render-target formats, the G-buffer layout and how shadow cascades are split.
6. Ground truth: one full memory dump of the game in a battle → `dump_cbs.py` (see Verification).

### Phase 1: Battle weather MVP (first usable release)
1. RMV2 v6 reader for `RS_STANDARD_V5` (LOD0, vertex format 0), DDS loader (BCn upload natively, no conversion).
2. Test map: a flat plane plus N random buildings from `rigidmodels/buildings` (vanilla or TDD, seeded and re-rollable).
3. Passes with game shaders:
   - G-buffer
   - directional sun light with cascaded shadows
   - ambient cube
   - environment specular
   - SKYGEN sky dome, sun mesh, near/far cloud planes
   - distance, height and volume fog
   - HDR auto-exposure + tone mapper
   - POST_PROCESSING grading
   - DOF
4. Editor:
   - file browser over `weather/**`
   - inspector for every element and attribute
   - colour pickers with HDR scale
   - undo/redo
   - Save / Save As (loose XML)
   - "modified" diff view against the original
5. Cross-fade slider between two environments.
6. **Gate:** CB values match the memory dump for the same `.environment`; round trip still byte-identical after edit + revert.

### Phase 2: Wind, precipitation and particles
1. `weather_definitions.xml` editing: wind gust simulation, wind on clouds (`cloud_velocity`), precipitation links.
2. VFX runtime:
   - parse emitters, behaviour chains and render stacks;
   - evaluate `ranged_property` curves and modifiers;
   - resolve library `vfx_references`;
   - run the game's `particle_vfx_main` / `particle_vfx_spawn` shaders.
   - RE needed: how the CPU-side simulation splits work with the GPU.
3. Precipitation: rain, snow, ash, sand storms; `snowing_intensity`, `fog_precipitation_distance_modifier`, wet/snow material response.
4. VFX editor: tree view of emitters and chains, curve editor for ranged properties, byte-identical UTF-16 save.

### Phase 3: Campaign
1. Seasonal `.lighting` files, campaign skybox model + cubemap, campaign camera rig and scale.
2. Campaign test map: a flat plane plus random `rigidmodels/campaign` models (only those using the Phase 1 material).
3. Campaign shaders (shroud and campaign fog specifics), campaign cloud/mountain VFX.
4. Season cross-fade preview (spring → summer → autumn → winter), as the game does it.
5. Porthole/UI `.lighting` (cheap add-on).

### Phase 4: DB tables
1. Read weather, sky, season and VFX tables through `rpfm_cli` TSV extraction with the RPFM schema.
2. "What would the game pick" view: climate + weather type + time → environment file(s). **Done** (Selection > Weather picker, `docs/re/variation_db.md`). Scope decision 2026-10-08: no general DB table editor; what is needed is editing `warscape_asset_variation_db` and converting it between XML and FASTBIN, which is done (items 1 and 3 of this phase are dropped).
3. Edit and write TSVs for RPFM import (still no own pack writing).

### Phase 5: More materials
By coverage: tiled dirtmap (63) → terrain blend (86) → ambient (64) → decals and projected decals → vegetation (trees, leaves, grass from `tdd_pack6`) → water. Vegetation and water matter most for judging wind and weather.

### Phase 6: Real battle tiles / TEd replacement
Read, edit and save actual tiles. To be scoped separately.

## Verification approach
- **Formats:** the automated round-trip test over all files in every phase.
- **Shaders:** every SM5 `.fxc` must create and reflect.
- **Rendering (decided 2026-10-06):** no whole-frame image comparison against the game. A real battle frame has too much other content (units, terrain, other lights) for a diff to mean anything, and battles can't be started from the CLI. Because the editor runs the game's own shaders, the pixel math is identical by construction. What can differ is the **inputs**, so ground truth is checked per input:
  1. **Constant buffer values:** RenderDoc crashes the game mid-capture (2026-10-06), so the values come from a **full memory dump** instead (Task Manager > Attila.exe > Create dump file, during a battle). `scripts/re/dump_cbs.py` follows the engine's shader manager and prints every named cbuffer variable's live value. Compare them with what the editor computes from the same `.environment`. Values only, not pixels, so scene content does not matter.
  2. **Pipeline state:** render target formats, pass order and sampler states from the same capture.
  3. **Isolated pass checks:** feed a pass recorded in the capture with the captured inputs and compare that pass's output only.
  - Start simple: the first checks use the sky dome and tone mapper, which depend only on the environment file and the camera.
- **RE findings:** recorded with DLL addresses in `docs/re/`, like `references/engine-notes.md`.

## Risks
- **Pixel-close needs ground truth.** Without a frame capture of the real game I can only match the final image, not the intermediate passes.
- **Engine-computed parameters.** Some values derived on the engine side (auto-exposure history, sun direction conversion, cloud animation time base) must be matched exactly. That's DLL reverse-engineering work and may take several iterations.
- **GPU particle path.** The particle split between CPU and GPU is unknown until Phase 2 RE.
- **TDD packs change during the day.** The VFS cache is keyed by pack mtime, so it stays current.
