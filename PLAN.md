# Warscape Community Editor: plan to merge weather_editor and tile_editor

## Status (updated 2026-10-13)

| Phase | State |
|---|---|
| 0 Freeze and new repo | **Done.** Baseline in `docs/baseline.md` and `docs/baseline/`. |
| 1 foundation, formats libs | **Done** (commit `8dfa720`). |
| 2 VFS / rpfm_server | **Done** (commit `6de91c4`). See `docs/vfs.md`. |
| 3 Render core, RenderScene | **Done** (commit `fad7ab7`). See the Phase 3 deviations below and `docs/re/terrain.md`. |
| 4 Editor core and shell | **Done** (commits `9ab9f55`..`96f6db3`). `docs/editor.md`, Phase 4 deviations below. |
| 5 Weather tool | **Done** (commit `209fbf3`). `tools/weather`, `docs/editor.md` (Weather tool), Phase 5 deviations below. |
| 6 Tile tool | **Done, waiting for your go.** `tools/tile`, `docs/editor.md` (Tile tool), Phase 6 deviations below. |
| Performance pass | **Done, waiting for your go** (between 6 and 7, asked for by you). `docs/performance.md`: Osgiliath at 140 to 270 fps (was 1.7), tile open to first image 0.4 s (0.9 s with an empty cache). |
| 7 Cross-tool composition | **Done, waiting for your go.** `docs/editor.md` (Cross-tool composition), gate `cross_tool_check`, Phase 7 deviations below. |
| 8 Retire the old apps | **Done, waiting for your go.** `legacy/` is gone, the old folders are archived, Phase 8 deviations below. |
| 9 | Not started. |
| Step 0 of the Rust/rpfm plan | **Done, waiting for your go** (2026-10-13). Pack access on RPFM 5.2's WebSocket API, rpfm_cli backend removed: `docs/vfs.md`, `docs/baseline.md` (Step 0). |
| Rust port | **Started** (2026-10-13). Renderer spike passed; crates `wce_d3d11`, `wce_vfs`, `wce_formats` (XmlDoc). See "Rust port" below. |

Phase 8 deviations: the `oracle` (BOB) gate was never built, so it did not run; the four legacy window shots (`shot_weather_{dry,rain,snow}`, `shot_tile`) left `gates.sh` and `shots.sh` with their apps: the same scenes are covered by the `wce.exe` captures that `scripts/shots.sh` compares (`capture_{dry,rain,snow}_1280x720`, `capture_tile_*`), and the window path by `app_check`;
`vfx_check` and `env_vs_dump` moved to `cli/`; `legacy/weather_editor/tools/re` is `scripts/re`, the tile survey scripts are `scripts/proto`, the other Python helpers are in `scripts/`; the old READMEs and `current-state.md` are in `docs/plans/` next to the two plans (`weather.md`, `tile.md`);
`variation_e2e.ps1` was deleted (superseded by `scripts/weather_check.sh`). `..\weather_editor` and `..\tile_editor` hold `ARCHIVED.md` and a pointer at the top of their README, and their files are read-only (`build/` and `scratch/` stay writable; undo with `attrib -R /S`). The attila-toolkit sources named no old path; the memory notes did and were updated.
Open finding: `scripts/shots.sh` reports the two dry captures (1280x720 and 800x600) as differing from `build/gates_base_perf` by up to 2 levels in about 3 % of the pixels (39,661 px in the 1280x720 one); every `shots_perf*` run of the performance pass shows the same difference, so it predates Phases 7 and 8, and rain, snow and both tile captures are identical or within one level. The reference was not re-baselined.

Phase 7 deviations: most of the plan's list existed already from Phases 4 to 6 (one `DocumentManager` with a layer and a modified marker per document, per-document visibility and Save all in the Outliner / File menu); what was missing and is new: the weather of an open `.environment` / `.lighting` can light the scene of any tool
(`Editor::set_weather_preview`, `Document::environment`, `Tool::lights_scene`; the preview is explicit and off by default, so the tile captures did not change), the tile and the weather file share the Outliner with tool tags and a context menu, the sample map is a stand-in scene that a tile hides (`Document::stand_in` / `shows_scene`),
and closing, closing all, quitting and the window's close button ask about unsaved changes (modal; commands `close ... save|discard`, `close_prompt`, `quit_prompt`, `answer`). `quit` and `--shot` runs still leave at once, as scripts expect. The "outliner groups items by document" and "layer visibility per document" items already held and are now covered by commands and the gate.
Not done on purpose: vfx files stay `.xml` documents of the weather tool (the VFX tool is still a placeholder), and the cross-fade partner is still chosen in the weather tool only. Unattended runs (`--script` / `--shot` / `--capture`) no longer take the focus: the window is non-activating and sent behind the others (`--foreground` restores the old behaviour). A pre-existing undefined shift (`1u << 40`) in the `move` command's gesture base was fixed.

Performance pass deviations: `RenderScene` gained `structure_revision` / `terrain_revision` / `moved_since` and `reserve`; `Document` gained `outline_cached` / `outline_stamp`; `ModelCache` / `TextureCache` / `Vfs::extract` are thread safe; `XmlDoc` gained in-place structural edits (`splice`, `bake_pending`, `remove_element_inplace`, `duplicate_element_inplace`) next to the functional ones; `read_file` no longer streams bytes through an iterator;
terrain meshes are chunked with LODs and skirts and the tile tool shows the height map at full resolution; the two tile captures were re-baselined and the legacy window shots now use the capture tolerance (`docs/baseline.md`). New gates `perf_check`, `xml_splice_check`; `--live` for scripts.

Phase 6 deviations: `tile_studio.exe` is still built from `legacy/tile_editor/src/app` (its own renderer, its shot is still a gate) until Phase 8, the new tool has no renderer of its own; the height map placement was wrong in the Studio (it stretched the map over the tile): the map has a one-cell border,
measured and documented in `docs/formats/tile_ted.md`; building models resolve in `tools/tile` (key -> `rigidmodels/buildings/<key>/` pieces, boxes with a log line for keys without a drawable model) because `libs/assets` has no model resolution yet; every other instance type is a marker (cross or box), not a model; rotate is `Ctrl+R` (`R` is
the shared scale-gizmo key); the wireframe terrain option is not ported; the Studio flags that clash with generic commands got new command names (`--select` = `tile_select`, `--move` = `tile_move`, `--delete` = `delete_item`, `--add` = `add_building`); the Layers panel stays next to the shared Outliner because the Outliner has no context menus yet;
the weather picker panel is data only until Phase 7. Shell changes: tools get Press / Drag / Release / Hover mouse events and can own the left button, the viewport has ray and project helpers, fixed-step runs take a scripted mouse (`mouse` command) instead of ignoring the real one, `CommandStack::take_dirty` accumulates, `undo [n]` / `redo [n]`.

Phase 5 deviations: `weather_editor.exe` is still built from `legacy/weather_editor` (main.cpp and app.cpp only; the sample map, XML documents and the picker are shared with the tool) because its window shots are the render regression until Phase 8;
`variation_e2e.ps1` is replaced by `scripts/weather_check.sh` (gate `weather_tool_check`); the XML documents live in `wce_weather` (links `wce_editor_core`, no ImGui) and the widgets in `wce_weather_ui` / `wce_tools`; scripted runs ignore the real mouse;
captures restart the renderer's temporal state and the `wce` capture references were re-baselined (`docs/baseline.md`); the GPU is chosen explicitly (`WCE_GPU`) because the reference pixels come from one of the machine's two GPUs.

Phase 4 deviations: the Weather tool is a host for the sample-map sandbox document until Phase 5 (the old `weather_editor` stays the real weather tool, and its window shots stay the render regression); Tile, Unit, VFX,
Terrain and Cinematic are `PlaceholderTool`s; the editor library is split in `wce_editor_core` (no ImGui or GPU, tested by `editor_check`) and `wce_editor`; layouts are ImGui ini files per tool, swapped on tool switch, instead of one
shared ini; ImGuizmo's scale mode exists but the sample map document ignores scale. `Vfs` now makes its paths absolute (rpfm runs in another working directory: a relative cache folder extracted into the server's directory). The
Phase 3 offscreen capture references came from `weather_editor --capture`, which rendered with the default environment (the legacy app only picks the active environment inside its UI frame); they are replaced by `wce.exe --capture` references
(explicit re-baseline, `build/gates_base_p3/capture_dry_*.png`). ImGui is the 1.91.9 docking tag and ImGuizmo master of 2026-08-08 (see `THIRD_PARTY.md`).

Phase 3 deviations: the dependency order is `... formats <- assets <- scene <- render ...` (scene below render): `Renderer::render` takes a `RenderScene`, which holds
paths and plain data, never GPU objects; `libs/assets` only has the weather vfx definitions for now (models and materials stay in `render/ModelCache` until the
Unit editor needs them on the CPU). Picking uses an id pass of its own (the game's G-buffer id target is fed by an engine-private lookup table), so the G-buffer
passes are unchanged. The terrain RE spike found that the game ships readable GLSL translations of every shader: `docs/re/terrain.md` has the pass structure; the pixel-close
terrain pass is not built, the interim pass draws `Terrain` entities in patches. The weather screenshots were re-baselined once, at the fixed-step clock (see `docs/baseline.md`).
The old docs moved to `docs/re`, `docs/formats` and `docs/plans`.

Phase 2 deviations: the spike showed that rpfm_server state is per MCP session, so the editor shares the running server with the attila-toolkit bridge (no second instance; the port cannot be changed anyway); the pack backends sit below `Vfs` (which keeps pack priority and both caches) instead of being implementations of `IVfs`; `libs/foundation` gained `json` and `settings` (no JSON library was approved, so the reader is our own); loose overlay folders are part of `Vfs`, not a third backend.

Phase 1 deviations from this plan: gates moved to `cli/` already (only `vfx_check` and `env_vs_dump` stay under `legacy/weather_editor/tools/`, they need the vfx and render code of Phase 3); `libs/game` (GameProfile) is built in Phase 2 together with the VFS because `extra_vanilla` is part of the VFS config; vfx stays in `legacy/weather_editor/src/vfx` until Phase 3; `image_io` (the D3D readback) moved to `libs/render` and writes through `png_file`, so screenshot file bytes changed (pixels checked); the assembly kit's `environment_map.xml` is a different XML from the variation db's, so `VariationDb::parse_environment_map` was added instead of reusing `parse_xml`.

## Rust port (started 2026-10-13)

Decided with you on 2026-10-12/13: the editor moves to Rust; parsing moves into rpfm_lib wherever the format belongs to the game's files
(rpfm branch `wce_formats`: Attila's fxc, rigid models v1-v6, BMD v4-v6, tile lists, asset variation db, tile database build, DDS mipmaps),
while the Assembly Kit image writers (byte-exact PNG with classic zlib, TIFF/TGA/DDS header templates) stay in the editor.
Unknown files and fields (`.markers` classes, CHMF blocks, the `Uk*` material types) get a later pass based on decompiling the kit's tools.

Renderer spike (the go/no-go of the port), passed: `cargo run -p wce_d3d11 --example game_shader` reads `skydome.hlsl_ps_ps30_skygen` from
Attila's packs with rpfm_lib (in-process, 0.3 to 0.6 s for all vanilla packs), reflects it with `D3DReflect` (the same cbuffers, offsets and
slots as `fxc_dump`), compiles a helper vertex shader with `D3DCompile` and draws the game's pixel shader into a render target on D3D11,
reading the pixels back. On Linux this runs on DXVK-native and vkd3d-utils (the same builds as the C++ app).

How: the `windows` crate does not build for Linux (its mandatory `windows-future` dependency is Windows-only), but `windows-core` does.
`crates/wce_d3d11` holds bindings generated with windows-bindgen for exactly the API the editor uses (`examples/generate_bindings.rs`
lists it); on Linux they link DXVK-native and vkd3d-utils from `build/_deps/prefix` (`WCE_DEPS_PREFIX`), with an RPATH so DXVK's two
libraries find each other. COM methods are `extern "system"`, the native ABI on Linux, which DXVK-native and the patched vkd3d use.

Layout: a Cargo workspace at the repo root, one crate per layer under `crates/` (same dependency direction as the C++ libs: d3d11/foundation,
vfs, formats, assets, scene, render, editor, tools, app). The C++ tree keeps building and stays the reference for the gates until the Rust
app reaches parity, as in the C++ merge. rpfm_lib is a path dependency on the `wce_formats` worktree until that branch is pushed.
UI: dear-imgui-rs (Dear ImGui 1.92.9 docking) with dear-imguizmo and the SDL3 platform backend; the D3D11 renderer backend is ours.
ImGuizmo's local `SetAllowedItem` change needs a patched copy of its C source.

Ported so far (each checked against the C++ code on the same inputs):

| Crate | What | Check |
|---|---|---|
| `wce_d3d11` | D3D11, DXGI and D3D compiler bindings | the renderer spike above |
| `wce_vfs` | the packs in the editor's priority order, plus overlay folders | same packs, files and winners as the C++ VFS (158,769 files); 422 cached files byte-identical |
| `wce_formats` | text encodings and the lossless `XmlDoc` (attribute edits, functional and in-place structural edits, splice journal, parallel read) | `xml_roundtrip` and `xml_splice_check` ported as examples: the round-trip output is byte-identical to the C++ gate over 3,197 files (the Attila kit and the cache; the same 7 malformed CA files fail); the splice gate passes on 8.6 and 9.7 MB kit tables, and the 383 MB `translated_texts.xml` reads to the same table in pieces as in one go |
| `wce_formats` | `env_value`: typed attribute values that keep their number style (`%f` stripped, shortest, fixed) | `env_value_dump` against the C++ code: identical over 2,025,816 floats in all 13 styles and 525,089 value texts (every attribute value of the kit and the cache, plus fuzzed ones); `format_check` output identical to the C++ gate |
| `wce_formats` | `tile_entry`, `tile_climate`, `tile_index` (with its row cache), and `c_num` (C's `strtod` / `strtol` prefix reading, which the typed views rely on) | `tile_dump` against the C++ code over the Attila kit: identical for all 1,107 entries (every one saves byte for byte, no unknown names), the climates and 1,992 texture path lookups, the index built without cache, cold and warm, and 9,072 resolve / find lookups |
| `wce_formats` | `png_file` (the kit's PNG encoder, on classic zlib from libz-sys) and `image_containers` (DDS, TGA, TIFF with the generated TEd headers) | `image_dump` against the C++ registry's round trips over 16,234 images of the kit and local caches: identical, including 11,037 PNGs re-encoded to the same bytes (libz-sys' zlib 1.3.2 against the vendored 1.3.1); golden hashes of the C++ encoder in the tests |

Text is kept as bytes in Rust (`Vec<u8>`): 8-bit game files are not always UTF-8, and UTF-16 files decode to WTF-8.

## Agreed scope (2026-10-09, 10 questions + follow-ups)

| Topic | Decision |
|---|---|
| Name | **Warscape Community Editor**. Repo folder `warscape_community_editor/`, exe `wce.exe`, C++ namespace `wce`. |
| Games | Attila only, but every game-specific fact sits behind one `GameProfile` so Rome 2 / ToB could be added later. |
| Repo | New repo. Both existing histories are imported (subtree merge), then files move to the new layout as ordinary commits. Old folders are archived once the unified app is at parity. |
| Migration | Freeze feature work. Commit what is in flight. Migrate in steps (libs → renderer → shell → tools). `weather_editor.exe` and `tile_studio.exe` keep building until `wce.exe` reaches parity. Every existing gate must pass after every step. |
| Pack access | **rpfm_server** (persistent process), behind a VFS interface. Since 2026-10-13: RPFM 5.2+, JSON-RPC over WebSocket (`127.0.0.1:45127/ws`), a session of our own; no rpfm_cli fallback (RPFM dropped it). Before: MCP over HTTP at `127.0.0.1:45127/mcp`, with the rpfm_cli code as the fallback backend. This replaces the old "rpfm_cli only" decision. |
| Scene | **Own data model per tool, shared render/editor scene.** Each tool keeps its own lossless document model (TedScene, XmlDoc environment, VfxDef, …) and publishes its objects into one shared scene. That scene provides the viewport, picking, selection, gizmos and undo plumbing. |
| Documents | One composed scene, many open documents. Tools are modes: a tool changes the panels, the viewport tools and which document is being edited. Example: edit a `.environment` while looking at a real tile. |
| UI | ImGui **docking** branch. Each tool ships a default layout; users can rearrange panels; layouts are saved per tool. |
| Tools | Now: **Weather/Lighting**, **Tile**. Future (seams only, nothing built): **Unit editor** (animations, VMD assembly), **VFX/particle editor**, **Battle map / Terry terrain**, **Cinematic editor** (keyframed scene composition, recording movies). |
| Audience | Public community release later. No hard-coded machine paths, first-run setup, never ship game assets, permissive third-party licences only. |

Unchanged from the two projects: C++20 / D3D11 / Dear ImGui; byte-identical round trip, minimal edit and BOB equivalence; the game's own `.fxc` shaders for pixel-close rendering; BOB reimplemented with real BOB as the test oracle.

## What exists today (surveyed 2026-10-09)

- `weather_editor/` (git, 120 files, **7 files with uncommitted changes** around xml_doc structural edits): `src/core` (vfs over rpfm_cli, process, text_encoding), `src/formats` (xml_doc, env_value, fxc, rmv2, variation_db + io), `src/render` (game-shader Renderer, environment, clouds, wind, shader_lib, texture, TestScene, math, image_io), `src/vfx` (particles, weather_vfx, vfx_def), `src/editor` (App, variation_editor). Namespace `we`. 11 tools/gates, `docs/re/*`.
- `tile_editor/` (git, 96 files): `src/formats` (registry, fastbin_tile, bmd_bin, chmf, markers, image_containers, png_file, tile_database, opaque_formats, xml_format), `src/scene` (TedScene, world_transform), `src/terrain` (height_field), `src/project` (TileProject), `src/weather` (environment_map), `src/app` (Studio). Namespaces `te` and `ts`. 10 tools/gates, `docs/formats/*`, `docs/re/bob.md`, `tools/corpus_inventory.py`.
- Code that is already shared or duplicated:
  - tile_editor's CMake compiles `weather_editor/src/{core/text_encoding, formats/xml_doc, formats/variation_db, formats/variation_io, render/image_io}` and weather's vendored ImGui **in place** (`${WE}`).
  - **Duplicate:** `tile_editor/src/weather/environment_map.*` (an XML-only weather matcher) repeats what `weather_editor/src/formats/variation_db.*` already does, and the weather version also covers FASTBIN, byte-identically.
  - **Second renderer:** `Studio` has its own small D3D11 path (height-field mesh, instanced boxes, line batches). The game-shader `Renderer` exists only in weather_editor.
  - Both apps have their own undo models (XmlDoc attribute/snapshot changes; TedScene undo), their own file browser/inspector, and their own command-line scripting (`--open/--shot/--pick/--vdb-script…` vs `script_paint/select/move…`).

## Target layout

```
warscape_community_editor/
  CMakeLists.txt  PLAN.md  README.md  THIRD_PARTY.md  (LICENSE: chosen before release)
  third_party/    imgui (1.91.9-docking), zlib 1.3.1, [ImGuizmo: needs your OK]
  libs/
    foundation/   text_encoding, process, logging, settings + paths, math (from render/math), jobs, Clock (fixed-step capable)
    game/         GameProfile: Attila pack list (extra_vanilla), shader names, format versions, engine constants from RE
    vfs/          IVfs + backends: RpfmServer (primary), RpfmCli (current code, fallback), LooseFolder (raw_data kits, working dirs)
    formats/      every parser/writer, pure CPU, no D3D: xml_doc, env_value, fxc, rmv2, variation_db(+io), fastbin_tile,
                  bmd_bin, chmf, markers, image_containers, png_file, tile_database, opaque_formats, registry (round-trip)
                  future: anim, skeleton, variantmeshdefinition, cs2_parsed, terry, cinematic (own format)
    assets/       CPU asset cache: model (rmv2 → mesh + material refs), DDS decode, material resolution, building-key → model
    render/       device, ShaderLibrary (game fxc), ConstantBuffers, TextureCache, GpuModel cache, Renderer + passes,
                  environment, clouds, wind, particles/vfx, terrain pass, overlay pass, picking, offscreen capture
    scene/        RenderScene: shared render/editor scene (see below)
    editor/       shared scene editor: Document/Tool interfaces, DocumentManager, CommandStack, Selection, gizmos,
                  camera controllers, Viewport, AssetBrowser, Outliner, Inspector (schema-driven), Log, Settings/first run,
                  layout presets, CommandRegistry (menus + CLI automation)
  tools/          one library per editor tool (implements Tool + its Document types)
    weather/      .environment/.lighting/weather_definitions/vfx XML documents, parameter schema, cross-fade, picker,
                  variation db editor, sample map generator (today's TestScene)
    tile/         TileProject, TedScene, world_transform, height_field, brushes, polygon/AI-hint tools, layers,
                  tile database build, later the BOB processors
    unit/  vfx/  terrain/  cinematic/      stubs: a registered Tool with a placeholder panel, no features
  app/            wce.exe: window, device, ImGui docking, tool registry, main loop, command line
  cli/            all current gates and utilities as separate exes (see Verification); registered with CTest
  scripts/        corpus_inventory.py, ted_files.py, re/*.py, variation_e2e.ps1, edit/apply_edits.py
  docs/           formats/ (tile's + new), re/ (weather's + tile's bob.md), plans/weather.md, plans/tile.md (the old PLANs)
```

Dependency direction: `foundation ← game ← vfs ← formats ← assets ← render ← scene ← editor ← tools/* ← app`. `formats` never includes D3D or ImGui, so CLI gates and future headless batch jobs (BOB processors, movie render farm) link without a window.

## Shared vs separate

| Shared (one copy, in `libs/`) | Per tool (in `tools/<tool>/`) |
|---|---|
| The whole renderer: G-buffer, lighting, shadows, SSAO, sky, sun, clouds, fog, weather particles, distortion, HDR/S-curve, DOF, lens flare, env cube, tone map | Which documents the tool opens and how they map into the scene |
| Terrain rendering, 3D assets (rigid now, skinned later), textures, materials | Export/processing: loose XML (weather), tile.ted + height PNG + tile_database.bin + BOB stages (tile), VMD/anim (unit), movie frames (cinematic) |
| VFS, pack priority, cache, rpfm_server/cli backends | Tool-specific panels (variation picker, brushes, layer tree, timeline, …) |
| All format parsers/writers and the round-trip registry | Tool-specific viewport tools (height brushes, polygon draw, keyframe handles) |
| RenderScene, picking, selection, gizmos, camera, overlays | Validation rules specific to a file type |
| Command stack/undo framework, document manager, dirty/save flow | Default dock layout |
| Asset browser, outliner, schema-driven inspector, log, settings, first run | |
| CommandRegistry for automation and test scripting | |
| Clock (live and fixed-step), offscreen capture, screenshots | |

## Core design

### RenderScene (the shared scene)
A flat set of entities with stable handles, rebuilt incrementally. It is not serialized: documents own the truth.

```cpp
struct SourceRef { DocumentId doc; uint64_t item; };   // back-reference for picking → document element
enum class Kind { MeshInstance, SkinnedInstance /*future*/, Terrain, VfxEmitter, Overlay /*lines, polygons, markers, boxes*/, CameraRig /*cinematic*/ };
class RenderScene {
    LayerId add_layer(DocumentId);               // one layer per open document: visibility, lock, order
    EntityId add(LayerId, Kind, const Transform&, Payload); void update(EntityId, …); void remove(EntityId);
    EnvironmentSlot environment;                 // active + optional blend environment (EnvironmentParams), owner doc
    uint64_t revision;                           // renderer and overlays rebuild draw lists only when it changes
};
```

- **Publishing:** `Document::publish(RenderScene&, LayerId)` runs on open, and after each command the document re-publishes only the items it touched (dirty set).
- **Weather documents** publish the `EnvironmentSlot` plus VFX emitters.
- **Tile documents** publish terrain, building instances (real models through `assets/`, a box only when the key does not resolve), and polygons, AI hints and markers as overlays.
- **The sample map** (today's TestScene) becomes a document that publishes random buildings plus ground. That keeps the weather tool's current default view and the regression baselines.
- **Environment precedence:** the active weather document's environment wins. Otherwise the tile tool's weather-picker result (from `variation_db`) applies. Otherwise the GameProfile default.

### Renderer changes
- `Renderer::render(const TestScene*, …)` becomes `render(const RenderScene&, const ViewParams&, RenderTarget&)`. Draw lists are built per revision and grouped by model/material, and the pass order is unchanged. This is the riskiest refactor, so the render regression gate runs before and after (see Verification).
- **Picking:** the G-buffer already writes a per-instance id target (`gb_[4]`). Map ids to `EntityId → SourceRef` and read back one pixel per click. Overlays pick in screen space.
- **Overlay pass:** after `hdr_to_screen`, depth-tested against scene depth. It handles lines, polygons, markers, boxes, gizmos, grid and selection outline. It replaces Studio's line batches.
- **Terrain pass:** the tile height field goes into the G-buffer. Step 1 uses an interim material so the tile tool keeps working. Step 2 is an RE spike on the game's battle-terrain shaders and constants (same method as `docs/re/renderer.md`), followed by a pixel-close terrain pass. The Terry tool later reuses the same pass at map scale.
- **Offscreen capture:** render to an arbitrary-size target with the window detached. It powers `--shot`, the regression gate and Cinematic recording.
- **Deterministic time:** the renderer reads time from a `Clock` (live or fixed-step). Wind, clouds and particles step with the clock's dt rather than wall time (the `last_time_ / frame_dt_` logic moves into the Clock), seeds are explicit, and a recorded frame N is reproducible.
- **Future seams:** declared, not built.
  - **SkinnedInstance:** bone palette constant buffer plus weighted RMV2 vertex formats, for the Unit editor.
  - **Multiple viewports:** a Renderer instance per viewport sharing ShaderLibrary/TextureCache, for a cinematic camera preview next to the free camera.

### Editor core
```cpp
class Document {            // owned by a tool, lossless model inside
  virtual std::string title() const; virtual bool modified() const;
  virtual bool save(const SaveTarget&, std::string* msg);   // writes minimal edits of the source bytes
  virtual void publish(RenderScene&, LayerId);
  virtual void build_outliner(OutlinerBuilder&); virtual void inspect(SourceRef, Inspector&);
  CommandStack& history();
};
class Tool {                // registered in app/, one per mode
  virtual const char* id() const; virtual std::span<const char*> opens() const;     // file patterns / VFS prefixes
  virtual void default_layout(ImGuiID dockspace); virtual void draw_panels(EditorContext&);
  virtual bool viewport_input(EditorContext&, const ViewportEvent&);                 // brushes, polygon draw, …
  virtual void register_commands(CommandRegistry&);
};
```

- **CommandStack:** one per document. Each command has `apply`/`revert`, a merge key (for drag gestures) and the dirty items for re-publishing. It wraps both existing undo schemes: XmlDoc attribute edits and snapshots, and TedScene edits. A global history panel shows the active document's stack.
- **Selection and gizmos:** selection is a set of `SourceRef`. The gizmo edits the entity transform and commits a command to the owning document; the document decides how that maps to its format (TEd euler order, %.6f formatting, …).
- **Inspector:** schema-driven. Weather's parameter schema table and the TedScene attributes both feed it, and unknown attributes show raw. Every inspectable property has a stable **PropertyPath** `{doc, item, field}`. The Cinematic editor animates through these paths, so this is the seam it needs.
- **CommandRegistry:** every menu action is a named command with arguments. The CLI (`--cmd name args`, `--script file`) and test scripts call the same entries. Today's `--open --shot --frames --view --pick --picker --blend --cam --vdb-script` and Studio's `script_*` become registry commands, and the gates keep their behaviour.
- **Settings/first run:** game folder, RPFM folder, cache folder (short path; MAX_PATH caveat), rpfm_server port and autostart, and graphics options from `preferences.script.txt`. No machine defaults in code; the dev machine values live in a local settings file.

### VFS with rpfm_server
- `IVfs` keeps today's `Vfs` API: `mount`, `packs`, `list`, `owner(s)`, `fetch`, `fetch_from`, `prefetch`, `read`, plus a `LooseFolder` overlay for raw_data kits.
- **Pack priority stays ours** (used_mods.txt order, then manifest + DLC packs, from GameProfile). The server is only used to list and read individual packs, not for its own dependency resolution, so behaviour does not change.
- **Spike first:**
  - Confirm the server ops for listing one pack's files, reading raw bytes of one file in one pack, and batch extraction (candidates: `open_packfiles`, `get_packed_files_info`, `get_packed_file_raw_data`, `extract_packed_files`).
  - Check whether the server's global state (selected game, open packs) can be shared with the attila-toolkit MCP bridge that Claude sessions use on the same port.
  - **If the shared state conflicts**, the editor starts its own rpfm_server instance if the port can be changed, or falls back to rpfm_cli automatically.
- Keep the on-disk extract cache and pack index cache (keyed by size + mtime) in both backends.

## Phases

### Phase 0: Freeze and new repo (~0.5 week)
1. Check in-flight work. `weather_editor` has 7 modified files (xml_doc structural edits, README, current-state). I'll ask you to commit them from their owning session, or to let me commit them. Then announce the freeze.
2. Create `warscape_community_editor/` with git init. Import both repos with history: fetch each, `read-tree --prefix=legacy/<name>/`, merge with `--allow-unrelated-histories`. Untracked dirs (`build/`, `scratch/` 883 MB, `manifest/`) are not imported, and `.gitignore` covers them.
3. Write a root `CMakeLists.txt` that `add_subdirectory`s both legacy trees unchanged. The `${WE}` path is fixed to `legacy/weather_editor`. Both old exes and all gates build from the new repo.
4. **Baseline:** run every gate and record its output in `docs/baseline.md`. Render baseline PNGs with `weather_editor --shot` at fixed cameras for the 3 dump environments (dry/rain/snow) plus 2 tile screenshots.

### Phase 1: foundation, game, formats libs (~1–1.5 weeks)
- **Move code:** `git mv` core and formats from both trees into `libs/foundation`, `libs/formats`, and render/math into foundation. Namespaces `we/te/ts → wce`. Remove the in-place `${WE}` compilation.
- **Remove the duplicate:** delete `environment_map`. The tile picker uses `variation_db`'s matcher, and `weather_check`'s cases are ported onto it so both gates must still pass.
- **PNG code:** `png_file` (byte-exact game PNGs) stays the only PNG writer. Screenshots use it too, so WIC `image_io` is dropped for writing if its output is equivalent; reading stays where needed.
- **GameProfile:** pull out `extra_vanilla` packs, shader names and engine constants that are currently scattered.
- **Gate:** every Phase 0 baseline is identical.

### Phase 2: VFS (~1 week)
- Spike on rpfm_server (above) → `IVfs`, then the `RpfmServer`, `RpfmCli` and `LooseFolder` backends.
- **Gate:** `vfs_probe --compare` lists and reads a fixed sample (shaders, rmv2, DDS, XML from vanilla, DLC and a mod pack) through both backends: same winners, same bytes.
- **Timing:** record cold and warm open times for the weather tool's startup set.

### Phase 3: Render core and RenderScene (~2 weeks)
- **Move code:** render and vfx → `libs/render`, plus `libs/scene`. TestScene becomes the SampleMap publisher. Introduce `ViewParams`, `RenderTarget`, the Clock and offscreen capture.
- **Add:** picking readback, the overlay pass, and the interim terrain pass.
- **Spike:** RE the battle-terrain shaders; write `docs/re/terrain.md`.
- **Gates:** `env_vs_dump`, `vfx_check`, `rmv2_check` unchanged. **Render regression:** the Phase 0 PNGs re-rendered through the new path at a fixed clock are bit-identical on this machine (any diff must be explained and re-baselined explicitly).

### Phase 4: Shared editor core and app shell (~2 weeks)
- **Shell:** upgrade ImGui to `1.91.9-docking` (same version, docking branch). Build `wce.exe` with a dockspace, tool switcher (toolbar + Ctrl+1..9) and per-tool layout files.
- **Editor core:** Document/Tool/DocumentManager, CommandStack, Selection, camera controllers (orbit as today, plus fly and the game's battle camera FOV/near/far), Viewport panel, AssetBrowser (from weather's browser, VFS-wide, filtered per tool), Outliner, Inspector framework, Log, history panel, CommandRegistry with CLI, settings and the first-run dialog.
- **Gizmo:** ImGuizmo (MIT) **if you approve the dependency**, otherwise our own translate/rotate gizmo in the overlay pass.

### Phase 5: Weather tool (~1.5 weeks)
- **Port:** App → `tools/weather` on the shared core: documents, schema inspector, structural edits, cross-fade, weather picker + variation editor, graphics options, sample map.
- **Parity checklist:** everything in `current-state.md` "Completed", plus the CLI flags.
- **Gates:** `xml_roundtrip`, `format_check`, `variation_check`, `variation_convert`, `variation_e2e.ps1`, render regression.

### Phase 6: Tile tool (~2 weeks)
- **Port:** Studio → `tools/tile`. Height brushes, select/move/rotate/duplicate/delete/add, G-grab on ground, layers, polygons and AI hints all go through the shared selection, gizmo, CommandStack and overlay pass.
- **Real models:** instances render real models via `assets/` (building key → model resolution; box fallback reported in the log).
- **Parity checklist:** the Studio README feature list plus `script_*`.
- **Gates:** `roundtrip`, `scene_check`, `unit_tests`, `height_check`, `tdb_build`, `order_test`, `png_debug`.

### Phase 7: Cross-tool composition (~1 week)
- Open a tile and an environment (and vfx) at once; "Preview this weather on this tile"; Save All; per-document dirty markers and close prompts; the outliner groups items by document; layer visibility per document.
- **Gate:** scripted end-to-end. Open a tile and a `.environment`, edit one attribute in each, save, and diff: only the two edited spans changed.

### Phase 8: Retire the old apps (~0.5 week)
- Remove `legacy/*` app targets, then the remaining legacy folders.
- Archive the original `weather_editor/` and `tile_editor/` folders read-only, with a README pointer.
- Move both old PLANs into `docs/plans/` and continue their remaining phases there: weather Phases 3, 5 and 6 (campaign, more materials, real tiles, the last now served by the tile tool), and tile Phases 3–11.
- Update attila-toolkit skills and memory that name the old paths.

### Phase 9: Release readiness (rolling, before the first public build)
- **Licence:** choose it (your call).
- **THIRD_PARTY.md:** list ImGui, zlib and any gizmo library.
- **Content audit:** check that the repo has no game assets or shader binaries (`docs/re/shaders` is index/cbuffer notes only, which looks fine, but verify), and no machine paths.
- **Build and release:** GitHub Actions build plus the non-corpus tests; release zip; README for first run (game + RPFM required).

## Future tools: seams the core must provide (not built now)

| Tool | Needs from shared core | Tool-specific later |
|---|---|---|
| **Unit editor** (animations, VMD assembly) | SkinnedInstance render kind + bone palette CB + weighted RMV2 vertex formats; `formats/anim`, skeleton (`.bone_inv_trans_mats`), variantmeshdefinition parser/writer; Clock for playback; asset browser filtered to variantmeshes/animations | Slot/variant assembly UI, attachment points, animation timeline, VMD export |
| **VFX / particle editor** | Existing particles + vfx_def in render; emitter entities in RenderScene; Clock with restart/scrub | Emitter graph/property UI, standalone vfx preview scene, vfx XML export |
| **Battle map / Terry** | Terrain pass at map scale (LOD/streaming), LooseFolder VFS for kits, tile_map/lf_height formats, overlay pass | Tilemap painting, .terry/tile_list/blm export, BOB Terrain stages |
| **Cinematic editor** (keyframed scenes, movie recording) | PropertyPath on every inspectable property; CameraRig entity; fixed-step Clock with fully seeded wind, clouds and particles; offscreen capture at any resolution; second viewport (camera preview) | Timeline/keyframe tracks over PropertyPaths (transforms, camera, environment values, cross-fade weight, visibility), its own scene file (references other documents rather than copying them), PNG sequence export through `png_file`, optional ffmpeg pipe if the user has ffmpeg installed. Game cinematic formats are out of scope unless you add them. |

Each future tool gets a stub `Tool` registration in Phase 4, so the switcher, layout save and document routing are exercised from day one.

## Verification
- **CTest labels:** `fast` (unit_tests, scene_check, order_test, format_check samples), `corpus` (xml_roundtrip 621 files, roundtrip 10,061 tile files, variation_check), `oracle` (BOB runs, ~40 s/tile), `render` (env_vs_dump against dumps, render regression PNGs).
- **Per phase:** all `fast` + `corpus` + `render` gates pass and match the Phase 0 baseline. `oracle` runs at Phase 6 and Phase 8.
- **Parity checklists** for each retired app, ticked by a scripted run through the CommandRegistry plus a short manual session in `wce.exe`.
- **Build tests:** use `--config RelWithDebInfo` so a running editor isn't killed (existing gotcha).

## Risks
- **Renderer refactor breaks pixel match:** bit-exact regression PNGs from Phase 0, gated in every phase.
- **rpfm_server:** its global state is shared with Claude's MCP bridge, its MCP tool names can change between RPFM versions, and the port is hard-coded. Mitigations: version check at mount, automatic rpfm_cli fallback, spike before committing.
- **Terrain shaders not reverse-engineered yet:** the interim terrain pass keeps the tile tool usable; pixel-close terrain is its own milestone.
- **Docking-branch upgrade:** same version number, so the API delta is small; ImGui ini files move to per-tool layouts.
- **History import:** done in a scratch clone first; the original repos stay untouched until Phase 8.
- **Other sessions:** the freeze must actually hold. Any change landing in the old repos after Phase 0 is replayed by hand, and the old folders get a FROZEN note.

## Open items to confirm
1. Commit the 7 uncommitted weather_editor files yourself, or may I commit them as they are?
2. ImGuizmo (MIT) as a new third-party dependency, or our own gizmo?
3. Licence for the public release (can wait until Phase 9).
