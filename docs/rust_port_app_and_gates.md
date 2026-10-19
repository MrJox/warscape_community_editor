# Rust port: the app, the tools and the CLI gates (notes, 2026-10-14 to 2026-10-17)

What was ported, how it was checked against the C++ code, the bugs found on the way, and what is still open. Commits `3de5892..cd17a9c`
on `rust-rewrite`, all signed. PLAN.md carries the per-crate rows; this file is the narrative and the "how to reproduce" for later.

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
- Use `WCE_SETTINGS_DIR=~/.cache/wce_cpp_settings` (a wce.ini with `game_dir` and `rpfm_dir=~/Proyectos/rpfm/target/release`) so the user's
  own wce.ini stays untouched; `DXVK_WSI_DRIVER=SDL3`.
- Clean-rebuild the C++ tree before trusting a C++ failure. The server exits by itself after the session.

## 6. What the C++ tree is still needed for

- On Linux, `cmake/linux_deps.cmake` builds DXVK-native and vkd3d-utils into `build/_deps/prefix`; `crates/wce_d3d11/build.rs` links them and
  the binary loads them at run time. A fresh Linux checkout runs the CMake configure step once before `cargo build`. Windows needs nothing.
- Everything else (`libs/`, `tools/`, `app/`, `cli/`, the vendored Dear ImGui, ImGuizmo and zlib) was the reference for the differential
  checks and was removed on 2026-10-19. The root `CMakeLists.txt` now only runs `cmake/linux_deps.cmake`.

## 7. Still open

- The real tile corpus check (PLAN.md "Before the port is closed"): `scene_check` and the C++ / Rust tile dumps on kit tiles, and the
  `scripts/tile_*.txt` gates on them. Everything tile-related above ran on synthetic fixtures (`~/.cache/wce_tile/fx2`).
- `env_vs_dump` and `vfx_check` have no game memory dump on this machine (self-checked only).
- The Rust app has not been built or run on Windows.
- Interactive checks by hand on a scaled display: clicks in the 3D view and gizmo drags after the HiDPI change.
- Decisions pending: whether command-line actions other than `--script` / `--shot` / `--capture` should keep the window interactive;
  whether DXVK should default to `DXVK_LOG_LEVEL=warn`.
- Using the synthetic kit in the Tile tool needs `tile_kits=~/.cache/wce_tile/fx2/kit/raw_data` in wce.ini (it replaces the kit search).
