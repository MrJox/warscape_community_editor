# Worker prompt: implement the Warscape Community Editor plan

You are implementing the migration plan in
`D:\SteamLibrary\steamapps\common\Total War Attila\TDD Claude\warscape_community_editor\PLAN.md`. The goal is to merge
two working C++20 / D3D11 / Dear ImGui editors for Total War: Attila, the weather/lighting editor and Tile Studio, into
one application, **Warscape Community Editor** (`wce.exe`, namespace `wce`). It has one renderer, one shared
render/editor scene, and selectable tools. Read PLAN.md fully before doing anything; it is the source of truth for
scope, layout, interfaces and phase order. The user already approved every decision in its "Agreed scope" table. Do
not re-open them.

## Where things stand (2026-10-09)

- **Phase 0 is done except `docs/baseline.md`:**
  - The repo is created, with both histories imported under `legacy/weather_editor` and `legacy/tile_editor`.
  - The root `CMakeLists.txt` builds both legacy projects unchanged.
  - `scripts/gates.sh` and `scripts/imgcmp.py` are committed, and `manifest/corpus.tsv` is copied in (git-ignored).
  - The old folders `..\weather_editor` and `..\tile_editor` are frozen (FROZEN.md). Never edit them again.
- **A full baseline run** (`scripts/gates.sh build/gates_base --corpus --render`, log in `build/gates_base.log`) was
  started in the background. Check whether it finished.
  - **If it finished:** write `docs/baseline.md` from it and commit.
  - **If it is still running:** wait. Do not rebuild while gates run, because the exes are in use.
  - **If it is missing or incomplete:** re-run it.
- **Known baseline facts to record in `docs/baseline.md`:**
  - All 10 fast gates pass.
  - `xml_roundtrip`: 621 files, 0 failed.
  - Tile corpus `roundtrip`: the expected known failures are 15 malformed CA XML files and 2,432 PNGs with ancillary
    chunks. Compare with the old `..\tile_editor\scratch\full_gate.txt`.
  - `unit_tests` crashes (pre-existing) on a tile.ted without a SCENE_GRAPH (`custom_monte_rosa/aa`): in
    `TedScene::add_layer(-1)`, `scene_graph()` returns -1. The gate uses `khazad_wall_test` meanwhile. Fix the bug in
    Phase 1 (guard plus a test on monte_rosa).
  - **Screenshots:**
    - The tile shot is bit-identical between runs.
    - The weather dry shot is about 53 dB PSNR, not identical: wind and tone-map adaptation run on wall-clock time.
    - Rain and snow shots vary a lot (particles seeded per run) and are informational only.
    - Phase 3 must add the fixed-step `Clock` **first**. After that, weather shots must be bit-identical between runs.
      Re-baseline them once at that point, explicitly, and say so in the commit.

## How to work

1. **Order:** go through the phases in PLAN.md order (1 VFS-independent libs, 2 VFS/rpfm_server, 3 render core +
   RenderScene + Clock, 4 editor core + shell, 5 weather tool, 6 tile tool, 7 cross-tool, 8 retire legacy, 9 release
   readiness).
2. **Keep it green:**
   - Each phase is a series of small commits. Each commit builds.
   - After each logical step, run `scripts/gates.sh` (add `--corpus` and `--render` at the end of each phase).
   - Diff against the baseline: summary lines must be identical, and screenshots must pass `imgcmp.py` with the rules
     above.
   - A changed gate result is a bug unless you can explain it and the user agrees.
3. **Move, don't copy:** use `git mv` so history follows the files. Remove duplicates instead of keeping two versions:
   - `tile_editor/src/weather/environment_map.*` goes away in favour of `formats/variation_db`. Port `weather_check`'s
     cases onto `variation_db` first, so both gates still pass.
   - Studio's own renderer goes away in Phase 6.
4. **Keep the legacy apps building until parity:** `weather_editor.exe` and `tile_studio.exe` keep building from the
   moved code until `wce.exe` reaches parity (Phase 5/6 checklists). Then retire them in Phase 8.
5. **Report and stop at the end of every phase.** Report:
   - what changed;
   - gate results versus the baseline;
   - deviations from PLAN.md and why;
   - open questions.

   Update the status block at the top of PLAN.md. Then wait for the user's go before the next phase.

## Hard rules

- **Byte-identical:**
  - Round trip, minimal edit (only edited spans change) and BOB equivalence must never regress.
  - Unknown bytes are preserved.
  - No parser may drop data to pass a gate.
- **Pack access goes through the VFS interface only.** rpfm_server (MCP over HTTP, `127.0.0.1:45127/mcp`) is the
  primary backend; the existing rpfm_cli code stays as the automatic fallback. Our own pack priority logic is kept
  (used_mods.txt order, then manifest + DLC packs).
  - **The attila-toolkit MCP bridge used by Claude sessions talks to the same server.** Start Phase 2 with a spike that
    proves the needed ops (list a pack, read raw bytes of one file in one pack, batch extract) and checks for
    shared-state conflicts (selected game, open packs).
  - **Never** run state-changing server calls that could break another session's work (closing packs you did not
    open, changing settings, saving packs) without asking the user.
  - Do not write an own pack reader.
- **Render with the game's own `.fxc` shaders**, pixel-close; RE notes go in `docs/re/`. Never redistribute game
  assets or shader binaries in the repo.
- **Third-party code:** approved are Dear ImGui (switch to the `1.91.9-docking` branch in Phase 4), zlib 1.3.1
  (vendored) and ImGuizmo (MIT). Ask before adding anything else. Keep `THIRD_PARTY.md` current.
- **No machine paths in code** (public release later): defaults come from settings and the first-run dialog. Test
  scripts may default to this machine but must accept environment-variable overrides like `gates.sh` does.
- **The licence is decided in Phase 9.** Do not add one before.
- **Phase 8 needs the user's explicit confirmation**: it deletes the legacy targets and archives the old folders.
- **Do not kill a running `weather_editor.exe` / `tile_studio.exe` / `wce.exe`**: the user may have it open. Build
  test configurations with `--config RelWithDebInfo`.
- **Git:**
  - Commit on `master` in `warscape_community_editor/` only; there is no remote, so never push.
  - End every commit message with `Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>`.
  - Don't skip hooks.

## Code conventions

- Match the surrounding code: terse comments that state facts and sources (dump, DLL function RVA), the same naming,
  `std::` containers, exceptions for parse errors, `/W4` clean for new code.
- **Dependency direction is strict:** `foundation ← game ← vfs ← formats ← assets ← render ← scene ← editor ← tools/* ←
  app`. `formats` and everything below it never include D3D or ImGui, so CLI gates and headless jobs stay
  window-free.
- **Include roots:** `libs/` and `tools/`. Use `"foundation/…"`, `"vfs/…"`, `"formats/…"`, `"render/…"`, etc.
  `core/*` becomes `foundation/*`, except `vfs` → `vfs/`, and `render/math` moves into foundation.
- **Namespaces:** `we` and `te` both become `wce`. This was checked: all .cpp helper types (`Reader`, `In`) sit in
  anonymous namespaces, so there is no ODR clash. Rename the tile scene's `format_number(double)` to
  `format_ted_number` to avoid overload confusion with `env_value`'s. `ts` (Studio) stays until Phase 6.
- **Compile options** (`/W4 /permissive- /utf-8 /MP /wd4100`, `NOMINMAX WIN32_LEAN_AND_MEAN _CRT_SECURE_NO_WARNINGS
  UNICODE _UNICODE`) move to the root CMakeLists. Register gates with CTest using labels `fast`, `corpus`, `oracle`,
  `render`.

## Environment gotchas

- Windows. The shells are Git Bash and PowerShell 5.1. The Bash heredoc mangles backslash escapes, so write any C++ or
  Python containing `\n` or `\\` with the Write/Edit tools, never via `cat <<EOF`.
- Tools and paths:
  - Game: `D:\SteamLibrary\steamapps\common\Total War Attila`.
  - Tilemap assembly kits: `assembly_kit_battle_tilemap` and `assembly_kit_campaign_tilemap` in the game folder.
  - VFS cache: `%LOCALAPPDATA%\awe` (keep it short: rpfm_cli silently writes nothing past MAX_PATH 260).
  - `rpfm_cli`: `C:\Users\Khaiali\Downloads\rpfm-v4.3.14-x86_64-pc-windows-msvc\rpfm_cli.exe` (reports version 4.5.100).
  - Build generator: Visual Studio 17 2022, x64.
- Attila RE: code section file offset = RVA - 0xc00, data = RVA - 0x1000. Many engine functions are vtable-only (find
  them via the object's vtable in a dump; DLL base 0x78040000 in the three dumps). RenderDoc crashes Attila, so use
  Task Manager full dumps + `scripts/re/dump_cbs.py`. Ghidra project and scripts are in
  `TDD Claude\tools\`.
- The attila-toolkit skills (`attila-file-formats`, `attila-battle-terrain`, `attila-bob-cli`, `attila-rpfm`) hold
  format and terrain facts. Load them when a phase touches those areas (RMV2/anim/skeleton for the Unit-editor seams,
  terrain for Phase 3's terrain spike).

## Phase 1 starting checklist (concrete)

1. Write and commit `docs/baseline.md` (see above).
2. Move `third_party/imgui` (from legacy weather) and `third_party/zlib` (from legacy tile) to the root `third_party/`,
   and fix both legacy CMakeLists.
3. Create `libs/foundation` (text_encoding, process, math), `libs/vfs` (vfs as is; the interface comes in Phase 2) and
   `libs/formats`:
   - from weather: xml_doc, env_value, fxc, rmv2, variation_db, variation_io;
   - from tile: registry, xml_format, fastbin_tile, bmd_bin, markers, image_containers, chmf, png_file, tile_database,
     opaque_formats.

   Also create `tools/tile/` with ted_scene, world_transform, height_field and tile_project. Each is a CMake target;
   the legacy projects link these targets instead of compiling sources (drop `we_core`, `attila_core` and the in-place
   `${WE}` paths).
4. Rename namespaces and fix includes; fix the `add_layer(-1)` crash; dedupe `environment_map`. Decide `image_io`
   versus `png_file` for screenshots as PLAN.md says, and keep screenshot output identical or re-baseline explicitly.
5. Add `libs/game` (GameProfile: extra_vanilla packs, shader names, engine constants) now or in Phase 2 with the VFS,
   and note which.
6. Gates: everything identical to the baseline. Report, update PLAN.md status, and stop for the user's go.
