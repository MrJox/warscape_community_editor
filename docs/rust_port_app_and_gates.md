# Rust port: the app, the tools and the CLI gates (notes, 2026-10-14 to 2026-10-17)

What was ported, how it was checked against the C++ code, the bugs found on the way, and what is still open. Commits `3de5892..cd17a9c`
on `rust-rewrite`, all signed. Section 0 has the per-crate rows (they were in PLAN.md); the rest is the narrative and the "how to reproduce" for later.

## 0. The port: decisions and the per-crate checks (moved from PLAN.md, 2026-10-26)

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
vfs, formats, assets, scene, render, editor, tools, app). The C++ tree was the reference for the differential checks below; with those done
it was removed (2026-10-19), and only `cmake/linux_deps.cmake` is left of the CMake build. rpfm_lib is a path dependency on the `wce_formats` worktree until that branch is pushed.
UI: dear-imgui-rs (Dear ImGui 1.92.9 docking) with dear-imguizmo and the SDL3 platform backend; the D3D11 renderer backend is ours.
ImGuizmo is upstream's (dear-imguizmo-sys); instead of the local `SetAllowedItem` change, the viewport clears its button from Dear ImGui's hovered and active ids while `Manipulate` runs.

Ported so far (each checked against the C++ code on the same inputs):

| Crate | What | Check |
|---|---|---|
| `wce_d3d11` | D3D11, DXGI and D3D compiler bindings | the renderer spike above |
| `wce_vfs` | the packs in the editor's priority order, plus overlay folders | same packs, files and winners as the C++ VFS (158,769 files); 422 cached files byte-identical |
| `wce_formats` | text encodings and the lossless `XmlDoc` (attribute edits, functional and in-place structural edits, splice journal, parallel read) | `xml_roundtrip` and `xml_splice_check` ported as examples: the round-trip output is byte-identical to the C++ gate over 3,197 files (the Attila kit and the cache; the same 7 malformed CA files fail); the splice gate passes on 8.6 and 9.7 MB kit tables, and the 383 MB `translated_texts.xml` reads to the same table in pieces as in one go |
| `wce_formats` | `env_value`: typed attribute values that keep their number style (`%f` stripped, shortest, fixed) | `env_value_dump` against the C++ code: identical over 2,025,816 floats in all 13 styles and 525,089 value texts (every attribute value of the kit and the cache, plus fuzzed ones); `format_check` output identical to the C++ gate |
| `wce_formats` | `tile_entry`, `tile_climate`, `tile_index` (with its row cache), and `c_num` (C's `strtod` / `strtol` prefix reading, which the typed views rely on) | `tile_entry_dump` against the C++ code over the Attila kit: identical for all 1,107 entries (every one saves byte for byte, no unknown names), the climates and 1,992 texture path lookups, the index built without cache, cold and warm, and 9,072 resolve / find lookups |
| `wce_formats` | `png_file` (the kit's PNG encoder, on classic zlib from libz-sys) and `image_containers` (DDS, TGA, TIFF with the generated TEd headers) | `image_dump` against the C++ registry's round trips over 16,234 images of the kit and local caches: identical, including 11,037 PNGs re-encoded to the same bytes (libz-sys' zlib 1.3.2 against the vendored 1.3.1); golden hashes of the C++ encoder in the tests |
| `wce_formats` | `dds_decode`: one mip of a DXT1 / DXT3 / DXT5 or 32-bit RGB DDS to RGBA8 on the CPU (the terrain colour bake reads its textures with it) | `dds_decode_dump` against the C++ code: 4,888 game textures at mips 0, 1, 3 and past the last: identical |
| `wce_foundation` | math (same operation order as the C++ code), the simulation clock, the log, machine settings (wce.ini) and the scope profiler. Not ported: the JSON and process helpers (they served the rpfm_server client, replaced by rpfm_lib in-process); `parallel_for` is rayon; the priority job pool (`jobs`, for background loading) is ported with a test of its ordering, cancelling and re-ranking; `std_random` is `std::mt19937` with libstdc++'s `shuffle` and `uniform_int_distribution`, draw for draw (what the C++ code picked from a seed, the port picks too) | `math_dump` against the C++ functions: bit-identical over 20,000 random cases |
| `wce_assets` | vfx definitions, particle emitter constants and the weather effects of a definition | `assets_dump` against the C++ code over the game's 528 vfx files and 51 weather definitions (extracted with `vfs_probe --extract`): identical, including the constants of every emitter with every chain of its file |
| `wce_scene` | the render scene (entities, layers, revisions, moved-entity log), the camera, environment parameters with the colour matrix, tone curve and cross-fade, the cloud planes, and an exact f32 -> f16 conversion | `scene_dump` against the C++ code over the game's 489 .environment / .lighting files (each read and cross-faded with the previous one at t = 0.3 and 0.7; 2,920 cloud planes generated and simulated for 300 steps): identical; `half_sweep`: the half conversion equals the C++ `_Float16` cast on all 2^32 floats |
| `wce_render` (CPU side) | the wind simulation, the constant buffers (declare / set / upload) and the environment -> cbuffer mapping | `render_cpu_dump` against the C++ code: the wind of all 51 weather definitions and of none over 3,000 frames, and the buffer bytes after each of the 489 environments: identical |
| `wce_render` (D3D11 basics) | DDS textures (whole, mip-streamed, arrays with neutral slices), RGBA textures with mip chains, render targets, PNG capture, adapter choice, GPU timer, the shader library (game fxc -> D3D11 shaders, reflection, input layouts, binding by name) | `gpu_dump` against the C++ code on the same DXVK device: 4,398 game textures loaded UNORM and sRGB, streamed and read back from the GPU (every mip and slice hashed), 271 texture arrays, and the reflection of all 5,056 game shaders with the 103 cbuffer layouts they declare: identical |
| `wce_render` (model cache) | textures by path (with mip streaming swapped in later), rigid models decoded by rpfm_lib (vertex buffers re-encoded with rpfm's own writer), background loading by priority on the job pool, terrain patches with LODs, skirts and skins, rebuilt per changed patch | `models_dump` against the C++ code on the same DXVK device: 3,569 building and campaign models (every vertex and index buffer read back from the GPU, LODs, bounds, texture choices, load errors) and four terrain builds (plain, LODs, skinned, edited): identical |
| `wce_render` (draw set) | culling items by frustum, screen size and LOD, grouping instances by batch, gathering instance records | `draw_set_dump` against the C++ code on 30 synthetic models, 20,000 items and five views (perspective, narrow, shadow, coarser LODs, small-item skip), with moved items and the id-pass filter: identical |
| `wce_render` (particles) | the GPU particle system with the game's particle_vfx shaders (simulation, sort keys, bitonic sort, spawn, draw, distortion buffer) and the CPU spawner | `spawn_dump` against the C++ code: the effects of the 20 weather definitions with emitters over 300 frames each (every spawned vertex, the random state, alive estimates, texture slices): identical. The C++ spawner drew some random numbers in unsequenced sub-expressions; it now sequences them explicitly in the order GCC used, and the Rust port follows that order. The GPU passes are checked with the renderer's image comparison |
| `wce_render` (renderer) | the frame: sky cube, G-buffer, terrain (interim and the game's material passes), shadows, SSAO, lighting, water, fog, clouds, particles, tone mapping, lens flare, the debug views, editor overlays and picking, reading the game's files from the packs or a folder | `frame_dump` against the C++ code on the same DXVK device, from the same extracted pack files: six scenes (sky only; terrain, buildings, overlays, grid and selection; night fog; sea and river; rain over 30 frames; the game's terrain material) and the ten debug views, as fixed-step frames: every pixel, the frame stats, exposure, ambient and a 13 x 9 grid of picks identical |
| `wce_editor_core` | the editor shell without UI or GPU: undo stacks with merged gestures and saved-state tracking, documents and the document manager (scene layers, re-publishing, save, stand-in scenes), the selection, the inspector interface and property paths, named commands with the script runner, the orbit and fly cameras. A command stack owns the model its commands edit (Rust has no back references from a command to its document); commands run on the host that owns the registry and say through their result when a script should wait or repeat | `editor_check` ported as a test (every check of the C++ gate passes), and `editor_dump` against the C++ code: both cameras over 200 runs of 300 random inputs with view switches and focusing (18,000 states, bit for bit) and the tokenizer on 20,000 fuzzed lines: identical |
| `wce_weather` | the sample map (seeded building picks on flat ground) and its document (placements with undo, gizmo moves), and the weather files as documents (attribute edits in the original number style, structural edits as snapshots, undo, revert, save as minimal edits) | `weather_dump` against the C++ code: `std::shuffle` for 1,200 sizes and seeds and 100,000 elements, the sample map for 25 seeds at 8 and 30 buildings over the game's 999 building folders (every pick, placement and error), 400 random placement edits with undo and redo, and random edit sessions on 123 weather files (1,513 structural edits, saved bytes, history names, outlines): identical. The sample map now refuses models of other versions than 6 with the model cache's message, as the C++ code does |
| `wce_tile` | the tile.ted scene graph on the lossless XmlDoc (TEd's number formatting, attribute and flag edits, in-place structural edits with splice journals, layers, moves, polygons), world transforms, the height field (brushes, ramps, undo), and tiles opened through the kit's tile database | `tile_dump` against the C++ code: 300,000 numbers formatted and 100,000 vector texts parsed, random edit sessions of 150 operations on 40 tile.ted files (every saved byte, world transforms, journals, snapshots, refused edits), 80 brush operations on each of 6 height maps, and tiles opened from variation folders, tile folders, entry files, a folder that is not a variation and a loose tile (placement of the height map, saves and backups): identical. `unit_tests` and `height_check` are tests, `scene_check` an example. **The checks ran on synthetic fixtures** (generated from docs/formats/tile_ted.md into a small kit with a 16-bit height map): the kit tiles and the 1,583-file corpus are not on this machine, so `scene_check` and the dump still have to run on them |
| `wce_tile_editor` | the tile as a document: tile.ted and the height map on the undo stack (moves, turns, structure, layers, flags, placements, polygons, brush strokes that carry landscape instances, the gizmo), the element index, building models, markers, polygon outlines, rivers and the sea plane, tile links, the highlight, picking, the ground for the game's terrain shaders and the debug views of the tile's maps (`terrain_material`, `terrain_bake`), save | `doc_dump` against the C++ code: three tiles of a generated kit (real building keys, rivers, a sea coast with links, 16-channel blend maps, the kit's `_settings.xml`) through the document manager, 160 random edits each with undo and redo, every scene entity, the saved text, history and outline hashed after each step, 300 picks, five ground looks, display changes, a save and undo / redo of everything: identical. The comparison found four bugs in the C++ code, fixed in both: undo re-formatted numbers instead of giving the file's text back (a polygon's area could change, and a later undo of a structural edit then corrupted the document), flags were added to elements without them outside the history, a stroke that went on after another edit shared its height record with the old stroke, and the delete of one item took its name from the element that moved into its place. **Synthetic fixtures only**: see the corpus check below |
| `wce_imgui` | Dear ImGui on the editor's window and device: an SDL3 platform backend (a port of `imgui_impl_sdl3.cpp`, on the system SDL3 that DXVK-native presents through) and a D3D11 renderer backend (a port of `imgui_impl_dx11.cpp`, with Dear ImGui 1.92's managed textures and user textures for the viewport image) for dear-imgui-rs | the `demo` example renders Dear ImGui's demo window with docking on DXVK and saves the back buffer. The UI runs Dear ImGui 1.92.9b where the C++ app has 1.91.9, so UI shots are compared by layout and behaviour, not pixel for pixel |
| `wce_editor` | the editor shell: dockspace, menus, toolbar, tool switching with a layout file per tool, the viewport (rendering, picking, rays, captures, focus, the gizmo, the active tool's mouse events), the shared panels (outliner, inspector, assets, log, history, commands), the built-in commands, the unsaved-changes prompt and the first-run dialog. Tools are owned by the editor and lent out while it calls them; a tool opening its own files passes itself (`open_path_from`). Upstream ImGuizmo has no `SetAllowedItem`: the viewport clears its button from Dear ImGui's hovered and active ids around `Manipulate`. The first run asks for the game folder only | scripted runs of the shell commands (tool switching by key and command, prompts, panels, the scripted mouse orbiting the camera, captures, perf, wait_loaded); the default layouts match the C++ app's shots |
| `wce_app` | the `wce` binary: an SDL3 window on both platforms, the D3D11 device on DXVK-native or Windows, the command line as a script (the C++ app's options, and any other `--name` runs the command `name`), the frame loop with shots and captures | the shots above. The C++ crash handler (minidump) is not ported |
| `wce_weather_tool` | the Weather / Lighting tool: Files, Properties (the attribute tree with typed widgets, structural edits, changes) and Weather scene panels, the Settings, Selection and Weather menus, the weather picker and the variation database editor (on rpfm_lib's asset variation db), lighting the scene with cross-fades, live precipitation from open vfx / weather definition documents, and its commands | `scripts/weather_smoke.txt` passes under the Rust binary (this machine's packs give 9 candidates for the picker's conditions, as the C++ `VariationDb` does on the same database); the saved environment differs from the pack file in exactly the edited line, and the saved database XML and FASTBIN are byte-identical to the C++ code's for the same edits and convert into each other with the C++ `variation_convert` |
| `wce_tile_tool` | the Tile tool: Tile (brushes, instances, the entry, placement, ground, links, polygons, the weather picker of the tile's map), Scene (TEd's layer tree with eyes, cursor arrows, rename, drag and drop) and Tiles (the kit's tile database) panels, the Tile menu, the viewport's brush / grab / polygon gestures and keys, and its commands | on the synthetic kit only (see below): a 120-command script over a tile logs the same 180 lines under the C++ and the Rust binary, with byte-identical placement dumps, diagnostics and saved files (everything undone saves the original bytes); the mouse gestures and the Scene panel's clicks and drags pass as scripts under the Rust binary. The gate scripts (`scripts/tile_*.txt`) name kit tiles and still have to run on them |

Text is kept as bytes in Rust (`Vec<u8>`): 8-bit game files are not always UTF-8, and UTF-16 files decode to WTF-8.

**Before the port is closed (decided 2026-10-14): run the tile checks on the real corpus** (still owed: the TDD corpus, now tracked in docs/plans/tile_roundtrip.md; the C++ / Rust differential dumps can no longer run, the C++ tree is gone). The Linux machine has no kit tiles (its Assembly Kit holds
only the tile database; the 1,583-file corpus is on the Windows side), so `wce_tile` and everything built on it were checked on synthetic fixtures only.
With the corpus available: `cargo run --release -p wce_tile --example scene_check -- <kit tiles folder>` must give the baseline of `docs/baseline.md`
(1,583 `tile.ted`, 0 parse failures, 0 parse/save differences, every number reproduced), and the C++ / Rust differential dumps (`tile_dump` and the
tile editor's) must be re-run on kit tiles (khazad_wall_test, custom_monte_rosa/aa, plains/3x1/3x1_aa and a settlement tile) and give identical output.
**Status (2026-10-22, branch `tile-roundtrip`):** the vanilla Attila Assembly Kit is now on this machine (Steam, one `assembly_kit` holding battle
and campaign tiles: 1,380 `tile.ted`, no TDD tiles). On it `scene_check` gives 1,380 files, 0 parse failures, 0 parse/save differences, 17,770,581 of
17,770,581 numbers reproduced (`docs/baseline/scene_check_attila_kit.txt`), and `tile_entry_check --all` passes (1,107 entries, 1,317 tiles open the same
three ways). Still owed: the 1,583-file corpus with the TDD tiles (v17) on the Windows side, and the differential dumps on kit tiles.

## 1. The app

| Crate | What | Commits |
|---|---|---|
| `wce_editor_core` | script runner hook: a command can include script lines (`script` command) while the runner is lent out | `3de5892` |
| `wce_editor` | the shell: dockspace, menus, toolbar, tool switching with a layout file per tool, viewport (render, pick, rays, capture, focus, gizmo, tool mouse events), outliner / inspector / assets / log / history / commands panels, ~60 built-in commands, unsaved-changes prompt, first run (game folder only, RPFM detection dropped). `gui.rs` is a thin wrapper over Dear ImGui's C API | `089f4e0`, `2e7cc42`, `5b8ab54` |
| `wce_app` | the `wce` binary: SDL3 window on both platforms (Windows: the HWND from the window properties), D3D11 on DXVK-native / Windows, the C++ command line (`--tool --open --open-disk --tile --cam --cmd --script --frames --shot --capture --fixed-step --live --foreground --background`, any other `--name` runs the command `name`), frame loop with shots and captures | `bd3f087` |
| `wce_weather_tool` | Weather / Lighting tool: Files, Properties (typed attribute widgets, structural edits, changes), Weather scene; Settings / Selection / Weather menus; weather picker and variation database editor on rpfm_lib's asset variation db; lighting with cross-fades; live precipitation from open vfx / weather definition documents (an overlay map instead of the C++ pointer to the editor) | `b74e295` |
| `wce_tile_tool` | Tile tool: Tile panel (brushes, entry, placement, ground, links, polygons, weather picker of the tile's map), Scene panel (TEd's tree, eyes, cursor arrows, rename, drag and drop), Tiles browser, Tile menu, brush / G-grab / polygon gestures and keys, ~60 commands | `1e88b97`, `29ff1d6` |

Design points worth remembering:

- Tools are owned by the editor and lent out while it calls them (`Editor::with_tool_at` / `with_tool`). A tool that opens one of its own files
  from its panel must call `Editor::open_path_from(self, ...)` (the plain `open_path` finds the tool missing and fails with "X is busy").
- Upstream ImGuizmo has no `SetAllowedItem`: the viewport clears its button from Dear ImGui's hovered / active ids around `Manipulate`.
- Commands that drive the scripted mouse must return the `mouse` command's frame wait (`CommandResult::wait`): the C++ registry kept the
  nested command's wait globally, the Rust one carries it in the result.
- HiDPI: interactive runs create the window with `SDL_WINDOW_HIGH_PIXEL_DENSITY`; Dear ImGui works in logical units with the framebuffer
  scale, the viewport renders its image at the window's density and hands tools / picking positions in image pixels. Scripted runs
  (`--script`, `--shot`, `--capture`, without `--live`) keep one pixel per unit so their mouse coordinates and shots are machine-independent
  (`6acfa25`).
- Any command-line action makes the run "scripted": non-focusable window, fixed-step clock, real mouse ignored (same as C++). For
  interactive use start plainly, or add `--live --foreground`. Changing this split (only `--script` / `--shot` / `--capture` unattended)
  was proposed and not decided.

## 2. The CLI gates

Every C++ gate under `cli/` now has a Rust counterpart (examples; `order_test` and `png_debug` are experiments and were not ported):

| Gate | Crate (example) | Check against C++ |
|---|---|---|
| `tile_entry_check` | `wce_tile` | identical on the synthetic kit |
| `variation_check`, `variation_convert`, `weather_check` | `wce_weather` | identical (variation_check over the packs; convert in both directions and on a database with problems; weather_check on the kit's environment_map.xml) |
| `terrain_check` | `wce_tile_editor` | identical |
| `placement_check` | `wce_tile_editor` | identical report and CSV on synthetic `building_list.xml` exports |
| `perf_check` | `wce_tile_tool` | identical on the fixture tiles (the parallel loops are rayon in Rust, the job queue `JobPool`) |
| `tdb_build`, `rmv2_check`, `roundtrip` | `wce_formats` (rpfm_lib as dev-dependency) | tdb_build: same bytes / same error; rmv2_check: identical over the game's 3,788 rigid models; roundtrip: identical on the local kit (3,208 files) and on one pack region (858 files), except that rpfm_lib round-trips the packs' version 1 `.tree_list.bin` the C++ reader refuses |
| `fxc_dump` | `wce_render` | identical index, cbuffers and all 5,056 disassemblies |
| `render_check` | `wce_render` | identical |
| `env_vs_dump` | `wce_render` | identical; `--all` orders equal scores differently (C++ `std::sort` is unstable). Extra `--emit` writes the editor's own values as a dump (self-check without a game dump) |
| `vfx_check` | `wce_assets` | identical to the C++ output and to `docs/baseline/vfx_check_rain.txt` |

Small additions made for them: `printf_g_prec` (`%.Ng`, checked against glibc on 200,000 values), `D3DDisassemble` binding,
`ConstantBuffers::buffers()`, `draw_set::View::default()` with the C++ `lod_scale` of 1. `fxc_dump` releases its device before exiting
(exiting under DXVK's background compiles crashed the driver).

## 3. Other end-to-end checks against the C++ app

- Weather smoke script (`scripts/weather_smoke.txt`, run by hand on Linux): passes under both binaries; saved environment and database
  XML / FASTBIN byte-identical. On this machine the picker finds 9 candidates (the script's default 2 is the Windows packs' number).
- Weather preview capture (800 x 450, after `wait_loaded`): pixel-identical between the two apps.
- A 120-command tile script over the fixture tile `plains_town`, unmounted and mounted: identical log, placement dumps, diagnostics and
  saved files (everything undone saves the original bytes).
- Tile mouse script (click select, G grab with confirm / Esc, brush drag, polygon drawing, handle drag) and Scene panel script (arrow, eye,
  cursor, drag before / into a layer, Delete): pass under both binaries with the same coordinates.

The scripts and harnesses live outside the repo: `~/.cache/wce_tile_tool/` (diff.txt, run.sh, run_mounted.sh, mouse.txt, scene_ui.txt,
run_ui.sh), `~/.cache/wce_cli/` (corpus manifests, C++ harnesses), `/tmp/wce_rs/wcheck.sh` (weather smoke; /tmp is not kept).

## 4. Bugs found and fixed

| Where | Bug | Fix |
|---|---|---|
| Rust `wce_d3d11` / renderer | `DXGI_ADAPTER_DESC1` has 16-bit WCHARs in the bindings, DXVK-native writes 32-bit `wchar_t`: `GetDesc1` wrote 256 bytes past a stack struct (and the log said "GPU: N") | `wce_d3d11::adapter_info` with the platform's layout (`9749a8e`) |
| Rust `wce_editor` | Dear ImGui 1.92 renumbered the popup flags: right-click menus asserted | `2e7cc42` |
| Rust `wce_editor` | a tool opening its own file from its panel failed ("Weather is busy") | `open_path_from` (`5b8ab54`) |
| Rust Scene panel | rows 23 px instead of 19 (the Dummy replacing the C++ cursor move added the item spacing) | `29ff1d6` |
| Rust app | blurry on scaled HiDPI displays | high pixel density window (`6acfa25`) |
| C++ `libs/vfs/rpfm_client.cpp` | use-after-free: the server version was read through a pointer into a destroyed temporary `Json` (only with a server that answers `/version`, i.e. rpfm_server 5.2) | `5fffec9` |
| DXVK 3.1.1 | `ResizeBuffers` gave the presenter the old size; on Wayland a maximized window kept its first swapchain (blurry) | one-line backport of the upstream master fix in `cmake/linux_deps.cmake`, noted in THIRD_PARTY.md (`cd17a9c`) |
| C++ build tree | stale objects ninja did not rebuild after header changes ("double free", `std::bad_alloc`) | `cmake --build build --clean-first`; not a source change |

## 5. Running the C++ app for comparisons on Linux

- rpfm_server 5.2 is needed (`/usr/bin/rpfm_server` is 5.1 and refused): `cargo build --release -p rpfm_server` in `~/Proyectos/rpfm`.
- Use `WCE_SETTINGS_DIR=~/.cache/wce_cpp_settings` (a settings.toml with `[games.attila] dir = ...`) so the user's
  own settings stay untouched; `DXVK_WSI_DRIVER=SDL3`.
- Clean-rebuild the C++ tree before trusting a C++ failure. The server exits by itself after the session.

## 6. What the C++ tree is still needed for

- On Linux, `cmake/linux_deps.cmake` builds DXVK-native and vkd3d-utils into `build/_deps/prefix`; `crates/wce_d3d11/build.rs` links them and
  the binary loads them at run time. A fresh Linux checkout runs the CMake configure step once before `cargo build`. Windows needs nothing.
- Everything else (`libs/`, `tools/`, `app/`, `cli/`, the vendored Dear ImGui, ImGuizmo and zlib) was the reference for the differential
  checks and was removed on 2026-10-19. The root `CMakeLists.txt` now only runs `cmake/linux_deps.cmake`.

## 7. Still open

- The real tile corpus check (section 0, "Before the port is closed"): `scene_check` and the C++ / Rust tile dumps on kit tiles, and the
  `scripts/tile_*.txt` gates on them. Everything tile-related above ran on synthetic fixtures (`~/.cache/wce_tile/fx2`).
- `env_vs_dump` and `vfx_check` have no game memory dump on this machine (self-checked only).
- The Rust app has not been built or run on Windows.
- Interactive checks by hand on a scaled display: clicks in the 3D view and gizmo drags after the HiDPI change.
- Decisions pending: whether command-line actions other than `--script` / `--shot` / `--capture` should keep the window interactive;
  whether DXVK should default to `DXVK_LOG_LEVEL=warn`.
- Using the synthetic kit in the Tile tool needs `WCE_TILE_KITS=~/.cache/wce_tile/fx2/kit/raw_data` (or the game's `assembly_kit` set to `~/.cache/wce_tile/fx2/kit`) (it replaces the kit search).
