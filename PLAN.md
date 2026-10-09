# Attila Tile Studio: plan to read, author and write battle and campaign tiles

## Status (updated 2026-10-08)

**Scope change (user, 2026-10-08):** campaign map files (`map.hex`, `map_data.esf`, `pathfinding.ppd`, trade routes, `.ptd/.pbd`) are irrelevant to campaign tiles. **Tilemaps and Terry are a separate, later scope**: `tile_map.png`, `climate_map.png`, `terrain.tile_list`, `tile_map.index/.tiles`, `locations.blm`, `lf_height_map.*`, `.agf` of maps, `.terry`, `explicit_tiles.txt`, deployment XML of maps are out for now. This removes plan items 3 (map pipeline) and the campaign map outputs of Phase 3, and the BLMF/CHMF map-level formats. In scope stays: battle and campaign **tiles** (`terrain/tiles/{battle,campaign}`), the tile database (`_settings.xml`, `TILES/*.xml`, `tile_upgrades.xml`, `tile_database.bin`) and everything TEd/BOB read or write per tile. `climate`/weather selection (Phase 10) now works on tile-level climate only. The corpus manifest marks the out-of-scope files `scope=out`.

**Phase 0 done:**
- BOB skill released (plugin 1.1.0/1.1.1). Tile and Vegetation processors verified headless on a scratch kit; determinism measured (see `docs/re/bob.md`): everything reproducible except 11 header bytes in two RMV2 meshes.
- `tools/corpus_inventory.py` + `manifest/corpus.tsv`: 37,041 files in both kits (no LFS stubs). In scope: ~14.9k battle raw tile files (57 GB), 5.7k campaign raw tile files (16 GB), tile databases, 4.1k working tile files.
- `tile_editor/` skeleton (CMake, `attila_core` compiled from `weather_editor/src` in place, `tile_formats` registry, `roundtrip` gate). **XML gate: 10,046 of 10,061 in-scope XML/.ted files round-trip byte-identically (includes 61 MB `tile.ted`).** The 15 failures are malformed CA files (`groupformations.xml`, 6 `*_tree_parameters.xml` per kit, one UTF-16 vfx with an odd byte count): they need the opaque pass-through fallback.
- **Deviation:** the physical extraction of `attila_core` from `weather_editor` is postponed. `weather_editor` has uncommitted edits from other sessions (xml_doc, app), so nothing is moved; both projects build from the same sources until it is quiet.

**Phase 1 progress:** C++ readers/writers with 100% corpus round trip: tree_list.bin (562), grass_list.bin (81), civilian_deployment.bin (85), civilian_shelter.bin (85); layouts in `docs/formats/fastbin_tile.md`. BMD binary (`*_procedural_bmd_data.bin`, `bmd_data_escalation*.bin`) partly decoded (BUILDING, PROP, PARTICLE_EMITTER records); finding: the PROP record has one byte not present in the XML, so XML cannot regenerate the bin exactly. `ted_p4_history` is not used (user: it is unused). TEd's file list will come from the TEd binary and BOB DLL strings plus the corpus itself.

**Phase 1 progress (cont.):** BMD binary (`*_procedural_bmd_data.bin`, `bmd_data_escalation*.bin`) fully decoded in C++: 860/860 byte-identical. `.markers` framing parser: 449/449 byte-identical with the per-marker payload still opaque (framing = repository header + class tag/name/version/count; trailer of the separator file looks like a layer/group list: name + marker indices). **PNG encoder de-risked:** zlib level 6 + libpng min-sum filter heuristic reproduce the kit's PNGs (see `docs/formats/image_encoders.md`). `ted_p4_history` stays unused.

**Status 2026-10-09 (overnight session):**
- Phase 1/2 formats, all byte-identical over the corpus unless noted: XML + tile.ted (10,046/10,061; 15 malformed CA files), BMD binaries 860/860, tree/grass/civilian bins, markers framing 449/449 (payload opaque), CHMF 171/171, DDS 5,880/5,880 (R32F header = generated template), TGA 1,756/1,756, TIFF 2,247/2,248 (TEd blend map header = generated template), PNG 6,122/6,122 of those without ancillary chunks (zlib 1.3.1 level 6, Z_FILTERED, libpng filter heuristic), opaque/authoring files and RMV2 magic. `tile_database.bin` **builder reproduces real BOB output with 0 differing bytes** (battle, same sources); campaign matches except one stale float. See `docs/formats/`.
- tile.ted scene model (`src/scene`), TEd number formatting reproduced for 3.24 M values, world transforms (euler = Rz(c) Ry(a) Rx(-b)), scene gate.
- Height field editing (brushes, undo, kit-exact PNG save), `bob_run.py build-tile` (real BOB for stage B/C of one tile, ~40 s).
- **Tile Studio v0** (`tile_studio.exe`): terrain, instances, layers, inspector, brushes, grab, undo/redo, save; scripted mode used to verify an edit changes exactly one line of tile.ted and only the brush area of the height map.
- Findings: building_list.xml export from tile.ted = scene position / 2 (horizontal), z flipped within the tile size, orientation via Rz*Ry*Rx(-b) matrix -> quaternion; 1260/1286 orientations and 1281/1286 positions exact in the test tile (rest last-digit rounding); entries follow tile.ted traversal order. tile_database.bin tile order has no recovered rule (explicit order list used). CHMF blocks (`hf_height_map.data`) are 3-byte flat blocks or variable-length coded blocks, not decoded.
- Not done: stage-A exporter (building_list.xml, markers payloads, zones, deployment...), BOB stage B reimplementation (CHMF, mesh decimation, DDS compression), vegetation generation, AI hint tools, snapping, weather/climate integration, LOD, campaign tile pipeline checks.

**Next (Phase 1):** decompile TEd loaders and read `ted_p4_history` to list every file TEd reads; readers for markers, tile.ted semantics, tile database; formats still unclaimed by the gate: `.markers` 449, `.bin` 1677 (FASTBIN0, tree lists), `.data` 171 (CHMF), `.dds` 5880, `.png` 8554, `.tif` 2248, `.tga` 1756, `.rigid_model_v2` 198, `.tmd` 498 (World Machine, probably pass-through), `.bob` 26, `.agf` 23.

## Context

TDD needs custom battle tiles (settlements, terrain, sieges) and campaign tiles. Today that means CA's closed-source GUI tools: **TEd** (tile editor: buildings, AI hints, markers, vegetation, zones) and **BOB** (batch processor that turns raw data into the working data the game packs). Both write only part of the output, both are slow (BOB costs about 20 s per run), and neither can be scripted or version-controlled well.

Goal: a standalone tool that reads everything TEd reads, writes everything TEd and BOB write, byte-identically, and offers a modern editor on top. Decisions already taken with the user in this session:

| Topic | Decision |
| --- | --- |
| BOB | **Reimplement BOB's processors**; real BOB CLI runs are the ground-truth oracle in test gates, not a runtime dependency. |
| Home | **New sibling project `tile_editor/`** next to `weather_editor/`, with a **shared core library** extracted from weather_editor. |
| Corpus | Round-trip gates run on **everything in both kits** (battle + campaign, raw + working, plus TDD packs via RPFM). |
| Stack | C++20 / MSVC 2022 / CMake / D3D11 / Dear ImGui, same as weather_editor (decided 2026-10-06). Packs only through `rpfm_cli`. Python allowed for RE and diff tooling. |

"Byte-identical" has three meanings, each with its own gate:
1. **Round trip:** parse then write of an unmodified file equals the input.
2. **Minimal edit:** an edit rewrites only the bytes it must (lossless XML spans, patched binary fields).
3. **BOB equivalence:** for the same raw inputs, our output equals what real BOB produces.

## Step 0: ship the BOB CLI skill now (requested mid-session; first action after plan approval)

Plan mode blocks edits, so this runs immediately after approval, independent of the tile editor work:
1. Create `attila-toolkit-marketplace/plugins/attila-toolkit/skills/attila-bob-cli/` with `SKILL.md` (frontmatter `name`/`description` like `attila-rpfm`; sections: paths and the two kits, command line, configuration XML and logical paths, processors table, rules.bob, batching and timing, success/failure detection, gotchas, what is still unknown), `references/config_templates.md` (Building, Pack, Cs2, Texture templates, CA's `custom_tile_processing_configuration.xml`), and `scripts/bob_run.py` (stdlib-only port of `blender_buildings_plugin/.../bob/cli.py`: resolve kit via `toolkit_config.py`, write config, run with cwd `<AK>\binaries`, `CREATE_NO_WINDOW`, timeout, refuse if BOB already running, read `bob_error.log`/`bob.log`, exit-code mapping, `--scratch-kit` option for oracle runs). Sources to port from: `cli.py` lines 17-140 (templates), 213-297 (running and errors), `rules.py` (rules.bob writer), `PLAN_buildings.md` 562-624 (verified behaviour).
2. Register it: mention in `attila-modding/SKILL.md` index and plugin `README.md`; add "BOB" to the plugin/marketplace descriptions and keywords.
3. Bump `plugin.json` `version` 1.0.1 -> **1.1.0** (new skill = minor), then `claude plugin validate attila-toolkit`.
4. Deploy: commit in `attila-toolkit-marketplace` (remote `origin` = `git@github.com:MrJox/attila-modding-marketplace.git`) with message "attila-toolkit 1.1.0: attila-bob-cli skill" and push to the default branch; then refresh the local plugin cache (`claude plugin update attila-toolkit`) and confirm the skill appears. The skill states honestly which parts are verified (Building/Pack/Cs2/Texture from the Blender add-on) and which are not (Tile/Terrain/Vegetation processors; Phase 0 spike fills those in and the skill gets a 1.1.x update).

## What exists (verified in this session)

**Kits:** `assembly_kit_battle_tilemap` (raw 58 GB, working 2.3 GB) and `assembly_kit_campaign_tilemap` (raw 21 GB, working 0.9 GB), both git+LFS repos. BOB and TEd binaries are in `binaries/`. TEd startup script `binaries/scripts/public_ted.script` is UTF-16 and lists TEd's modules.

**TEd modules** (these define the "full TEd feature support" matrix): `ai_hint_view`, `building_view`, `campaign_tile_view`, `capture_point_view`, `castle_view`, `deployment_zone_view`, `effects_view`, `efline_editor_view`, `hardoutline_view`, `marker_view`, `point_light_view`, `polygon_editor_view`, `projectile_firer`, `prop_view`, `ship_view`, `sky_view`, `spline_editor_view`, `stats_view`, `terrain_decal_view`, `vegetation_painting_view`, `zone_editor_view`. Grid spacing 6.0, grid snap tolerance 6.0, angle snap 90 deg.

**Files per battle tile** (`raw_data/terrain/tiles/battle/<set>/<size>/<variation>/`, e.g. `plains/4x4/4x4_aa`):
- Text/XML: `tile.ted` (XML `<project version="15">`: Vegetation/Grass seeds, SCENE_GRAPH, camera), `definition.xml` (BATTLE_MAP_DEFINITION), `building_list.xml`, `far_building_list.xml`, `capture_location_list.xml`, `deployment_areas.xml`, `ef_lines.xml`, `zones.xml`, `go_outlines.xml`, `non_terrain_outlines.xml`, `sound_emitter_list.xml`, `tree_random_seed.xml`. All CRLF, mostly single-quote attributes.
- Binary: `*.markers` (ai_hint_directed_point, ai_hint_polyline, building, decal, particle_emitter, prop, tree; UTF-16 names plus records), `bmd.tree_list`.
- Images: `height_map_0.png`, `blend_map.tif` (+ `.dds`, `.jpg`), `lf_influence_map.png`, `nogo.png`, `custom_normal.DDS`, `tile_normal.tga`, `final_*.dds`, `raw/*.png` (sculpt intermediates).
- Tile database: `_tile_database/_settings.xml` (RENDER_PARAMS, CONVERSION_PARAMS, TILE_SETS; `lf_height=3000`, `vertical_offset=20`) and `_tile_database/TILES/*.xml` (699 files; TILE, TEXTURE_GROUP, VARIATIONS, TILE_LINK_TARGETS, TILE_LINKS); `terrain/tiles/{battle,campaign}/tile_upgrades.xml`.
- Per map (`raw_data/terrain/battles/<map>/`, `.../campaigns/<map>/`): `tile_map.png`, `climate_map.png`, `lf_heights.tif`, `lf_sea_heights.tif`, `normal.dds`, `explicit_tiles.txt`; campaign adds `layout_map.png`, `lf_sea_colour.tif`, `masks.xml`, season masks, snow mask, `*.terry`, `lighting/`.

**BOB outputs** (`working_data/...`): `tile_database.bin` (**FASTBIN0**, same container family as `warscape_asset_variation_db.bin`, already converted both ways in weather_editor); per map `terrain.tile_list`, `tile_map.index`, `tile_map.tiles`, `lf_height_map.dds`/`.data` (magic `CHMF`), `lf_sea_height_map.dds`, `locations.blm` (`BLMF`, holds `ROME_*` climate names), `<map>.agf`, `<map>.xml` (battle script), `map_info.xml`, `rules.bob`; per tile `battlefield/`: `*.tree_list.bin`, `default.grass_list.bin`, `*_procedural_bmd_data.bin/.xml`, `building_list` (binary), `civilian_*.bin`, `blend.dds`, markers. Campaign: `map_data.esf`, `pathfinding.ppd`, `trade_routes.ptd`, lookup `.tga`, `dynamic_resources.esf`.

**Strings in the tools** tell us who touches what (extracted from the DLLs): BOB_Tile reads `tile.ted`, `height_map_0.png`, `blend_map.tif`, `ground_type_map.png`, `lf_influence_map.png`, `nogo.png`, `building_list.xml`, `far_building_list.xml`, `terrain_outlines.xml`, writes `bmd_data.bin/.xml`, `final_*.dds`, `index.dds`, `ground_types.dds`, `protection_map.dds`, `tile_database.bin`. BOB_Terrain handles `tile_map.png`, `climate_map.png`, `lf_*.tif`, `masks.xml`, season masks, `tree_database.xml`, `environment_collection.xml`. BOB_Vegetation produces `default.grass_list.bin`, `_procedural_bmd_data.bin`, uses `per_climate_tree_conversions.xml`. BOB_Battle handles `groupformations.bin`, `LandBattle.xml`, `tree_climate_conversions.xml`. TEd itself loads/saves `tile.ted`, `map_info.xml`, `config.xml`, deployment polygon XML, `explicit_tiles.txt`, `protection_map.dds`, and launches BOB with a generated `PackFile = <retail>/data/%1.pack` rules file.

**BOB CLI** (from `blender_buildings_plugin/total_war_cs2_addon/bob/cli.py`, `rules.py`, `PLAN_buildings.md`): `BOB.AssemblyKit.exe /dont_stop_on_error /configuration:<name> /offline`, cwd `<AK>\binaries`, config at `binaries\BOB\<name>_configuration.xml`, entries are `<raw>/`, `<working>/`, `<retail>/` logical paths, files to build go in `selected_consumers` (not providers), exit code 0/1, reasons in `bob_error.log`, never run two at once, any bare argument pops a modal and hangs. CA's tile example: `binaries/BOB/custom_tile_processing_configuration.xml` (Pack processor). **Unknown:** processor names for Tile, Terrain, Vegetation, Battle, Campaign (spike in Phase 0).

**Reusable code**
- `weather_editor/src/formats/xml_doc.*` lossless XML (UTF-8/UTF-16, byte spans): used for all XML here.
- `weather_editor/src/formats/variation_db.*`, `variation_io.cpp`: FASTBIN0 parse/convert, byte-identical to game files. Base for `tile_database.bin`.
- `weather_editor/src/formats/rmv2.*`, `render/{shader_lib,texture,image_io,math,scene,renderer}.*`, `core/{vfs,process,text_encoding}.*`, `editor/app.*`: building preview, D3D11 and ImGui shell, rpfm_cli VFS.
- `blender_buildings_plugin/total_war_cs2_addon/binary/{cs2_parsed_reader,cs2_reader,cs2_writer,rigid_model_v2_reader}.py` and the specs in `file_format_specs/` (cs2.parsed v11/v13 holds collision, lines, no-go and docking data: needed for snapping).
- `lf_bedding/scripts/build.py`, `verify.py` and memory notes `attila-lf-heightmap-facts`, `tdd-tilemap-painted-size`: LF height units and bedding rules for height painting.
- `weather_editor` environment/lighting/variation-db code for the weather and climate phase.

## Addendum: survey findings that change the plan (data agents, after approval)

Step 0 is done (attila-toolkit 1.1.0 pushed to `MrJox/attila-modding-marketplace`, local install updated; skill `attila-bob-cli`, `bob_run.py` tested with stub executables only, never against the real kit).

1. **Source of truth for TDD tiles is `tile.ted`** (XML, v16/v17, up to 61 MB, 6.3 GB in total): `LayerInstance`s (Logic, Go Zones, Nogo Zones, Escalation 1-3) holding `BUILDING_INSTANCE` (key, position, orientation, scale), `POINT_LIGHT`, `VFXInstance`, `ProjectedDecalInstance`, `UserInstanceGroup`, `SIEGE_AI_POLYGON`, `SiegeAINode/Edge`, `AI_HINT_POLYGON`, `AISeparator`, `CaptureLocation`, `ZONE_POLYGON`, `DEPLOYMENT_ZONE_POLYGON`, `NOGO_REGION`, `SplineRiverInstance`, `VegetationExclusionPolygon`, camera. The loose lists (`building_list.xml`, `*.markers`, `zones.xml`, ...) are **TEd's logic export ("stage A") into `working_data`**, not sources. Legacy vanilla tiles (v15 `tile.ted`, empty scene) keep the lists in `raw_data`. So Phase 6 writes `tile.ted` scene graph first, then the stage-A exporter (TEd's job) produces the lists, `bmd_data_escalation{1,2,3}`, `civilian_*.bin`, deployment XML. Both are needed byte-identical.
2. **Stage split per tile** (hornburg timestamps): A = TEd export (22 logic files at once); B = BOB Tile (blend/index/normal/ground_types.dds, `hf_height_map.data`, `hf_water_map.data`, `mesh.rigid_model_v2`, `outfield_mesh`, `terrain_outlines.xml`); C = BOB Vegetation (`*_procedural_bmd_data.*`, `*.tree_list.bin`, `default.grass_list.bin`). Phase 4 = stage B, Phase 9 = stage C, Phase 6/8 = stage A.
3. **Not byte-reproducible by BOB itself:** `tile_map.tiles` embeds heap pointers; treat it as pass-through / structural comparison only. Run BOB twice in Phase 0 to find any other such file.
4. **Image encoders (concrete evidence):** PNG: libpng-style, zlib level 6, 8192-byte IDAT chunks (confirmed on `ground_type_map.png`, `nogo.png`; `height_map_0.png` untested). TIFF: little-endian, header offset 8, 15 IFD entries, uncompressed, one strip at offset 242, no extra tags (TEd/BOB-written `blend_map.tif`, campaign `lf_heights.tif`); Photoshop-saved TIFFs keep their own layout and are pass-through. DDS: TEd's R32F/L8/L16 files have `dwFlags=0`, no caps, so a generic DDS writer will not match; `blend/index/normal.dds` are DXT5 with 11 mips (flags 0xA1007).
5. **New formats found:** `.terry` (XML, ECS entities, 25 MB for the TDD battle map; ECBattleCatchmentArea, deployment zones; written by Terry/TWeak, which also runs BOB Terrain actions), `CHMF` v5 block tables, `BLMF` v8 (`locations.blm`+`.mask0/.mask1`), 0xABCA streams (`building_list` binary, tree lists), `.markers` with class tags, `river_splineN.xml`, `casloadersettings` (boost archive), `.tmd` (World Machine), `explicit_tiles.txt` (`x,y,tile_path,rotation`), `config.xml`, `tile_map.index` (u32 9, w, h). `terrain.tile_list` starts with u32 11 then a u16-prefixed path table. Campaign `map.hex` (written by the external CAIME tool; `map_data_config.xml` gives slots/ports) feeds BOB Campaign (`map_data.esf`, `pathfinding.ppd`, `trade_routes.ptd`, `borders.pbd`, lookup `.tga`).
6. **Evidence sources for Phase 1 RE:** `binaries\ted_p4_history` (13,655 lines listing every file TEd touched), `binaries_terry\bob.log` (real Terry run: Terrain actions `Terry file`, `Tilemap` 7 min, `Heights & Normals`), DLL strings above. Also `tile_upgrades.xml` is **not in the kits** (TDD ships it in its pack) and `.gitattributes` makes `*.tif/*.terry/*.tiles/*.exe/*.dll/*.psd` LFS files, so check for LFS pointer stubs in the manifest.
7. **Campaign scope refined:** campaign tiles are 1x1/2x2/4x4/8x8 ground sets plus lakes, mountains, cliffs (6,231 files, 16 GB) with `mask` footprints; `tile.ted` v1-17 mixed; BOB Tile outputs `hf_height_map.data`, `mesh.rigid_model_v2`, `blend/index/normal.dds`. The campaign `terrain.tile_list` (13 MB, 41 sets, 371 variations) and `lf_height_map.*` come from BOB Terrain. Still assumed out of scope unless confirmed: `map.hex`/CAIME, ESF/PPD/PTD outputs, `.terry` editing.

## Architecture

```
TDD Claude/
  attila_core/        (extracted from weather_editor; both apps link it)
    xml/ binfmt/ (reader+writer primitives, FASTBIN) dds/ png/ tiff/ rmv2/ cs2/
    vfs/ (rpfm_cli) render/ (D3D11 device, shader lib, textures) ui/ (ImGui shell)
  tile_editor/
    src/formats/   one module per file type: parse -> model -> write, plus round-trip registration
    src/project/   TileProject, MapProject, TileDatabase (loads a whole kit or a mod folder)
    src/bob/       reimplemented BOB processors, one per output file type
    src/edit/      commands (undo/redo), brushes, scene graph, layers, gizmos, snapping, AI-hint tools
    src/render/    terrain, building, marker renderers, LOD
    src/app/       ImGui editor
    tools/         corpus_inventory, roundtrip, bob_oracle (runs real BOB and diffs), bin_probe, ghidra scripts
    docs/formats/  one spec per format (same style as attila-file-formats skill)
    docs/re/       notes from decompiling TEd/BOB DLLs (code offset = RVA - 0xc00 style gotchas)
```

Principles:
- **Format first.** Every file type gets a spec in `docs/formats`, a parser + writer, a registration in the round-trip harness, and only then semantic editing on top.
- **Unknown bytes are preserved.** Binary models keep an opaque tail or unknown-field list; the writer emits it. No parser may drop data to pass a gate.
- **Corpus manifest.** `tools/corpus_inventory` hashes every file of both kits once (SHA-256 + size + mtime), classifies it (source of truth, derived, scratch, LFS pointer, unknown) and stores a manifest. All gates read the manifest, so a 80 GB corpus is hashed once and re-checks are fast.
- **Oracle harness.** `tools/bob_oracle` copies a small input set into a scratch kit copy (never the user's kit), runs real BOB through the new skill's wrapper, and diffs every produced file against our output.
- **Image encoders.** To be byte-identical for edited PNG/TIFF/DDS we must reproduce the encoder that TEd/BOB used (kit ships `FreeImage.dll`, `nvtt.dll`, `cudart32_30_14.dll`). Tactic: first fingerprint existing files (zlib level/strategy/filter, TIFF compression and strip layout, DDS compressor settings); unmodified images are passed through untouched; for edited ones call the kit's own FreeImage/nvtt DLLs when installed (CPU path), and replace with own encoders only if fingerprints match. This is the highest-risk item.
- **Pack output** stays with `rpfm_cli` (weather_editor decision); BOB's Pack processor is the oracle for pack contents, not reimplemented.

## Phases (most important first)

### Phase 0: Foundations, BOB skill, oracle (about 1 week)
1. **BOB CLI skill:** done in Step 0 above; this phase only extends it with the tile/terrain/vegetation processors found by the spike.
2. **Spike: tile/terrain/vegetation processor names and invocation.** Grep processor names in `BOB_*.dll`; capture the command line and configuration TEd/Terry/TWeak generate on "Build" (Process Monitor or a wrapper exe); try a tiny tile (`osgiliath_test`, `generic_northmen_unwalled`) through BOB in a scratch kit; record in the skill and `docs/re/bob.md`.
3. Extract `attila_core` from weather_editor (no behaviour change; weather_editor gates must still pass).
4. `tile_editor` skeleton, `corpus_inventory`, manifest, harness that runs every registered format's round trip.
5. **Gate:** manifest covers 100% of both kits; every file is classified; BOB runs headless on a scratch copy and a second run reproduces the same output hashes (determinism check: timestamps, GUIDs, random seeds).

### Phase 1: Read everything TEd reads (battle + campaign) (about 2 weeks)
1. Decompile `TEd.AssemblyKit.exe` loaders (existing Ghidra setup in `tools/`) to enumerate exactly which files and fields TEd reads per module; write `docs/re/ted_inputs.md`. (`ted_p4_history` is an old Perforce log, not evidence of what TEd reads: ignored, per the user.) Cross-check against `bin_probe` over the corpus, so no file type stays "unknown".
2. Readers (model only, no write yet): all text XML above, `tile.ted`, `*.markers`, `*.tree_list`, PNG/TIFF/DDS/TGA (16-bit PNG and 16-bit TIFF height data), `_settings.xml`, `TILES/*.xml`, `tile_upgrades.xml`, `tile_map.png` + `climate_map.png` + `explicit_tiles.txt`, campaign `masks.xml`, season masks, `layout_map.png`, `*.terry` (read-only first).
3. `TileProject` / `MapProject` loaders that open a tile folder or a whole map and resolve cross references (tile -> tile set -> tile_map colour -> catchment).
4. CLI `tile_dump` prints a project; errors on any unknown field.
5. **Gate:** every file in the corpus loads with zero unknown fields; report of unread bytes is empty.

### Phase 2: Write everything TEd writes, byte-identical (about 2 weeks)
1. Writers for all Phase 1 formats; lossless XML for text files; patched writers for binary ones; markers and `tree_list` writers with preserved unknowns.
2. Image encoder fingerprinting spike (see Principles); decide per type: pass-through, kit DLL, or own encoder.
3. **Gate:** parse + write == input for 100% of the corpus for each type, and the minimal-edit test (change one value, revert, bytes equal) passes on a sample of 1000 files per type.

### Phase 3: Tile database and map-level pipeline, battle and campaign (about 3 weeks)
1. Editing model for `_settings.xml`, `TILES/*.xml`, `tile_upgrades.xml` (create tile from template, set tile set colours, variations, links and link targets, size classes, catchments).
2. **Reimplement the tile database build:** XML -> `tile_database.bin` (FASTBIN0) for `terrain/tiles/battle` and `.../campaign`; reuse `variation_db` FASTBIN code; decode the record layout, string tables and hashes.
3. Map pipeline: `tile_map.png` -> `terrain.tile_list`, `tile_map.index`, `tile_map.tiles`; `climate_map.png` -> `locations.blm`; `lf_heights.tif`/`lf_sea_heights.tif` -> `lf_height_map.dds`/`.data` (CHMF), `lf_sea_height_map.dds`; `<map>.agf`, `map_info.xml` generation.
4. Campaign: campaign tile DB, `tile_upgrades.xml` (campaign), season masks and `masks.xml` consumption; campaign `.esf`/`.ppd`/`.ptd` outputs are only decoded if confirmed in scope (see open questions).
5. Validation tool `tilemap_audit` (painted size vs `tile_upgrades.xml`, missing catchments, missing tile folders), reusing the findings in memory `tdd-tilemap-painted-size`.
6. **Gate:** for the vanilla maps, TDD map and the CA test tiles, our `tile_database.bin` and map-level outputs equal real BOB output byte for byte (oracle harness).

### Phase 4: Tile-level BOB outputs for terrain: heights, blend, normals (about 3 weeks)
Reimplement what BOB_Tile does from `height_map_0.png`, `blend_map.tif`, `lf_influence_map.png`, `nogo.png`, `custom_normal`: `final_heights.dds`, `final_alpha.dds`, `final_lf_influence.dds`, `final_normal_map.dds`, `blend.dds`, `index.dds`, `ground_types.dds`, `protection_map.dds`, `bmd_data.bin/.xml`, terrain outlines. Uses `CONVERSION_PARAMS` (triangle decimation factors, `max_lf_heights_per_pixel`) from `_settings.xml`.
**Gate:** oracle diff per tile on a sample of every size class (1x1 ... 8x8, settlement, river, coast).

### Phase 5: Terrain viewer and height/blend painting (about 3 weeks)
1. D3D11 terrain renderer from `height_map_0.png` + LF (chunked quadtree, normals from the real pipeline, blend map texture groups).
2. Height painting: raise/lower/smooth/flatten/level-to-value/noise/ramp/stamp/import PNG; units and clamps exactly as the pipeline (metres = raw * lf_height / 65535 - vertical_offset); LF tools with the bedding rules from `lf_bedding` (beach 1.88144 m, port 0.141909 m, sea plane raw 436.9, flat bedding for swamp/lake); tile-border continuity against neighbours via TILE_LINKS.
3. Blend map painting (4 texture-group channels), nogo and LF influence painting, slope/height auto-layers.
4. Undo/redo by tile-region snapshots; save writes the raw files (Phase 2 writers) and regenerates derived outputs (Phase 4).
5. **Gate:** paint, save, run real BOB on the saved raw tile, outputs equal our derived outputs.

### Phase 6: Scene editor with layers; buildings and VFX; translate/rotate (about 4 weeks)
1. Scene graph and **layers** (model of `tile.ted` SCENE_GRAPH `lf_offset`, `exportable`, names; layer visibility/lock/colour; layer per kind: buildings, props, decals, VFX, markers, AI hints).
2. Selection, picking, gizmos: translate (axis/plane/terrain-follow), rotate (free, 90 deg, angle snap 90 deg tolerance), scale where the format has it, copy/paste/duplicate, multi-select, undo/redo with command merge.
3. Asset browser over packs (RPFM) for buildings, props, VFX (`vfx/*.xml` from weather_editor), decals, sound emitters.
4. Building preview via `rmv2` (RS_STANDARD_V5 first, other materials as weather_editor Phase 5 adds them), and `cs2.parsed` for footprint/collision.
5. Writers: `building_list.xml`, `far_building_list.xml`, `building_marker.markers`, `prop_marker.markers`, `decal_marker.markers`, `particle_emitter_marker.markers`, `sound_emitter_list.xml`, `tile.ted` (scene graph + camera). Settlement tile helpers: wall pieces by size and level, per `tile_upgrades.xml` rules.
6. **Gate:** open every shipped settlement tile, move one building, move it back, save: bytes equal; build the working `building_list` binary (Phase 8 shares this) and diff against BOB.

### Phase 7: Building snapping (about 1 week)
Grid snap (spacing 6.0 default, configurable), angle snap, terrain-height snap, piece-to-piece snap using `cs2.parsed` data (wall ends, docking points, footprint lines, no-go hulls) and a bounding-box fallback; snap preview and per-object toggles. Pure editor feature: no format risk.

### Phase 8: AI hints, deployment, capture points and the remaining TEd modules (about 4 weeks)
Parity matrix of TEd modules, in order of gameplay impact: `ai_hint_view` (directed points, polylines, polyline lists in `ai_hint_*_marker.markers`), `deployment_zone_view`, `capture_point_view`, `castle_view`, `zone_editor_view`, `polygon_editor_view`, `efline_editor_view`, `hardoutline_view`, `go_outlines`/`non_terrain_outlines`, `spline_editor_view` (rivers/roads), `marker_view`, `terrain_decal_view`, `point_light_view`, `effects_view`, `sky_view`, `ship_view`, `stats_view`, `projectile_firer`. Each module: model, tools, writer, and (where BOB consumes it) the BOB output (`civilian_deployment.bin`, `civilian_shelter.bin`, deployment XML, `groupformations.bin`). Hint semantics come from RE of TEd and from the AI-siege findings in `attila-campaign-ai`.
**Gate:** per module, load every shipped tile that uses it, no-op save equals input; BOB oracle for derived files.

### Phase 9: Vegetation (BOB_Vegetation) (about 3 weeks)
Tree/grass painting (`vegetation_painting_view`, `Vegetation generation_seed`, `Grass generation_seed`), `tree_random_seed.xml`; reimplement generation of `*.tree_list.bin`, `default.grass_list.bin`, `*_procedural_bmd_data.bin/.xml` with `per_climate_tree_conversions.xml` and `tree_climate_conversions.xml` per climate. Seeded generation must replicate BOB's RNG and ordering exactly.
**Gate:** same seeds + same painting => identical bins vs real BOB for 50 tiles per climate group.

### Phase 10: Weather and climate selection (about 2 weeks)
Integrate weather_editor modules: climate picker per tile/map (`climate_map.png` colours, `locations.blm` climate names, `climates_tables`, `warscape_asset_variation_db` bits, see memory `attila-battle-climates`), season/time-of-day/precipitation selection through the existing "what would the game pick" logic, live preview of the chosen `.environment` on the tile via the shared renderer. No new formats.

### Phase 11: LOD optimization for rendering (about 2 weeks)
Camera-based LOD in the editor viewport (not in shipped data, so no byte constraints): buildings use RMV2 LOD groups with the game's distance thresholds, instancing for repeated meshes, frustum and distance culling, impostor/billboard fallback for far buildings (`far_building_list`); terrain: quadtree chunks with geomorphing, LF-only far field, tile streaming for large maps; GPU budget HUD. Note the distinction: BOB's triangle decimation (CONVERSION_PARAMS) belongs to Phase 4 and must stay exact.

## Cross-cutting
- **Undo/redo and autosave** from Phase 5 on; every command is serialisable so scripts can replay edits.
- **Safety:** never write into the kits or game folders without an explicit target; default work folder is a mod project folder; originals read-only. Every writer refuses to overwrite a file whose hash differs from the manifest unless told to.
- **Skills and docs:** keep `attila-file-formats` (new references per format), `attila-battle-terrain` and the new `attila-bob-cli` updated as formats are decoded.
- **Memory notes** per phase (formats decoded, RE addresses), like the weather editor's `docs/re/`.

## Critical files to create or touch
- `attila-toolkit-marketplace/plugins/attila-toolkit/skills/attila-bob-cli/{SKILL.md,scripts/bob_run.py,references/*}`; `plugin.json` version; README.
- `attila_core/` (moved from `weather_editor/src/{formats,render,core}`), `weather_editor/CMakeLists.txt` (link core).
- `tile_editor/CMakeLists.txt`, `tile_editor/src/**`, `tile_editor/tools/{corpus_inventory,roundtrip,bob_oracle}`, `tile_editor/docs/{formats,re}/`.

## Verification
- Phase 0: `corpus_inventory` manifest; `bob_run.py` on a scratch kit with a CA test tile; two runs, equal hashes.
- Every phase: `roundtrip` over the manifest per format; `bob_oracle` diff for derived files; weather_editor's existing gates (`xml_roundtrip`, `variation_check`) must keep passing after the core extraction.
- Editor phases: scripted edit scenarios (paint, move, rotate, snap) then save, run real BOB, compare; manual in-game check of one TDD tile packed through `rpfm_cli`, loaded in a custom battle.

## Risks
1. **Image encoders (PNG/TIFF/DDS) not reproducible** (nvtt CUDA path may be non-deterministic): mitigated by pass-through plus kit DLLs; may force "bit-identical pixels, not bytes" for edited images only.
2. **BOB non-determinism** (timestamps, GUIDs, thread ordering): found in Phase 0 determinism check; normalise or document.
3. **Binary formats with hashes/checksums** (FASTBIN, markers GUIDs): RE effort in Ghidra (setup exists).
4. **Scale:** 80 GB corpus; manifest + sampling for slow gates, full runs nightly.
5. **BOB crashes** on missing collision meshes and some rule keys; oracle harness must classify crashes as "no oracle", not failures.
6. **Scope growth:** TEd has 21 modules; Phase 8 is ordered by impact so it can be cut.

## Open questions (assumptions made, shout if wrong)
- **Campaign scope:** assumed = campaign tiles (`terrain/tiles/campaign`, tile DB, `tile_upgrades.xml`) plus reading the campaign map raw inputs; campaign map outputs (`map_data.esf`, `pathfinding.ppd`, trade routes, `.terry` editing) are out unless you want them (they are Terry/TWeak territory).
- Packs are produced with `rpfm_cli`, not by reimplementing BOB's Pack processor.
