# Worker prompt: Tile tool TEd parity, start with Phase 0 and Phase 0b

You are continuing work on **Warscape Community Editor** (`wce.exe`, C++20 / D3D11 / Dear ImGui, namespace `wce`) in
`D:\SteamLibrary\steamapps\common\Total War Attila\TDD Claude\warscape_community_editor`. The migration plan
(`PLAN.md`, phases 0 to 8) is finished; you are now implementing a **new plan**: bring the Tile tool (`tools/tile`) to
feature parity with Creative Assembly's TEd (`assembly_kit/binaries/TEd.AssemblyKit.exe`).

**Read first, fully, before doing anything:**
1. `docs/plans/tile_ted_parity.md`: the plan (findings, gap table, phases 0, 0b, 1 to 7, risks, decisions). It is the
   source of truth for scope and order. The user has answered its questions; do not re-open them.
2. `docs/worker_prompt.md`: the working rules of the previous plan. Everything under "How to work" (small commits that
   build, gates after every step, report and stop at the end of a phase), "Hard rules", "Code conventions" and
   "Environment gotchas" still applies, with the changes listed below.
3. `docs/editor.md` (Tile tool, Layouts, Command line, Gates), `docs/formats/tile_ted.md`,
   `docs/formats/tile_database.md`, `docs/re/terrain.md`, `docs/performance.md`.
4. Your memory index (`MEMORY.md`), especially the notes on this project, the weather/tile projects, and the focus
   rule.

## Where things stand (2026-10-09)

- Repo state: clean on `master` (last commit `803d657` Phase 8) except the untracked plan
  `docs/plans/tile_ted_parity.md` and this prompt. Commit both first as "Plan: Tile tool TEd parity".
- Build in a private tree so a running `wce.exe` is never overwritten: `cmake -S . -B build/work3` (or reuse
  `build/work2`) and run gates with `WCE_BIN=build/work2/bin/RelWithDebInfo` (see `docs/performance.md`, Perf pass
  facts). `build/dev` and `build/` belong to the user's open editor.
- What the user reported on the Tile tool (all confirmed or made plausible by the plan's "Live check"): layers cannot be
  hidden / locked / reordered / found; gizmo drags orbit the camera instead; water and rivers do not render; the terrain
  is the interim single-texture pass (no climate, no layer blending, no tile-edge blending); some buildings sit at the
  wrong height because `surface_mode` (`MSM_LANDSCAPE` / `MSM_FREEFORM`) is never read.
- Decisions (user): the order is yours; Phase 5 is every TEd tool; **tile links are important** (Phase 6a and the
  read-only view in Phase 0b); **a tile's root is its database entry** `_tile_database/TILES/*.xml`
  (`TILE@location` + `VARIATION@location` -> folder with `tile.ted`; colour, links, ground groups, height and normal
  parameters), not the folder of `tile.ted` (plan section 0).

## What to do now: Phase 0, then Phase 0b, then stop and report

### Phase 0: reproduce and measure (read-only on product code except new CLI gates)
1. **Scripted repros** for each complaint, as `.txt` scripts under `scripts/` run by `wce.exe --script` (see
   `docs/editor.md`, Command line): layer actions through the Layers panel with the scripted `mouse` (coordinates are
   pixels of the viewport image; negative values reach the panels on its left; `mouse wheel -N` scrolls the panel under
   the pointer); gizmo drag on each axis of a selected building (the arrow was tiny; aim from `viewport().project` of
   the axis tip, do not guess pixels); a screenshot of the default Tile layout. The scratchpad scripts `s3` to `s8` of
   the planning session show the working patterns (kit tile `assembly_kit_battle_tilemap/raw_data/terrain/tiles/battle/assembly_kit/osgiliath_test/battlefield`;
   quote paths with spaces in the script file). Record what each run shows in the plan (adjust phases if a cause turns
   out different).
2. **`cli/placement_check`**: for every kit tile that has a TEd export `building_list.xml` (in `assembly_kit/working_data`
   and the kits' working data), compare the world position / orientation of every building from
   `TileDocument::world_matrix` with TEd's own values; print the error by `surface_mode`, depth in groups and type, the
   mean / median / max, and the worst 20. This is the oracle for Phase 2. Also compute heights from the tile entry
   (`min_height`, `scale`) and from `_tile_database/_settings.xml` (`lf_height`, `vertical_offset`) and say which formula
   fits; the planning session saw a constant 17.19 m offset on `osgiliath_test` landscape buildings with the formula
   `min_height + scale * v / 65535`, which may be wrong. Do not fix anything in Phase 0; report.
3. **TEd black-box probes** (needs the user's TEd; see "Using TEd" below): on a scratch copy of a kit tile, toggle a
   layer's eye and selectable (cursor arrow) in TEd's Property Editor > Scene tab, save, and diff the saved `tile.ted`
   against the original with `scripts/span_diff.py`: which attribute and flag change (`OIF_VISIBLE`, `OIF_DRAGGABLE`,
   other). Do the same for changing a building's placement mode and for moving a landscape building after a terrain
   edit, to learn the re-snap rule. Write the findings into `docs/formats/tile_ted.md`.
4. Optional time-boxed spikes (about one day each, only if the black-box result above does not settle the question; see
   the plan's RE section): TEd's snapping code for landscape / freeform in groups; the tile link rule semantics
   (`TLT_NOT_EQUALS`, `base_x/base_y`, `is_entry`, `blend_size`) from the consumers' strings and code. Facts go into
   `docs/re/` (formats and behaviour, no copied code, no game assets). The terrain draw-order spike belongs to Phase 3,
   not now.

### Phase 0b: the tile entry as the root of a tile
Implement exactly the plan's Phase 0b: `TileEntry` (lossless `XmlDoc`, typed access to every attribute and child, variations,
links, targets), an index of the kit's `TILES/` cached by size and mtime (no scan per open), open by entry / folder /
`tile.ted` (the last two resolve their entry from the path and warn when there is none), a tile browser panel and variation
chooser, the entry shown read-only in the Tile panel (colour, size, ground groups, per-variation height / normal /
overlap values, links and targets), height and normal parameters read from the entry. Keep `find_tile_info` only as the
path-to-entry resolver (indexed). Add the gates the plan lists: entry round trip over the 699 files (byte-identical),
`tdb_build` still 0 bytes differ, open-by-entry gives the same document as open-by-folder for every kit tile with an
entry, and a `tile.ted` outside the kit opens with a warning. New commands for scripts (`tile_entry`, `assert_entry`, ...)
go through the `CommandRegistry` like the existing ones.

Then **stop**: report what changed, the gate results against the previous baseline (`docs/baseline.md`, all existing
gates must still match), the Phase 0 findings, deviations from the plan and why, and open questions; update the status
block at the top of `docs/plans/tile_ted_parity.md`; wait for the user's go. Do not start Phase 1.

## Rules that differ from `docs/worker_prompt.md` or are easy to miss

- **Byte-identical stays absolute:** round trip, minimal edit (only the edited spans of `tile.ted`, the entry and the
  PNG change), BOB equivalence. Unknown attributes and bytes are preserved. A parser must never drop data to pass a gate.
- **Focus (important):** the user plays games on this machine. Unattended runs (`--script`, `--shot`, `--capture`) are
  non-activating by default: keep it that way, never pass `--foreground` unless the user asks in that session, and end
  every scripted run with `quit`. Never bring TEd or any window in front of a running game on your own; ask. Do not take
  full-screen captures; capture only your own window.
- **Using TEd:** `assembly_kit/binaries/TEd.AssemblyKit.exe`, working directory `assembly_kit/binaries`. It starts with a
  new 8x8 tile and hidden main window (`Ted BUILD 12791(Slavs)...`); load a tile through its File menu (the tile entries
  are listed from the kit). Work on **scratch copies** of kit tiles (copy the tile folder and, if TEd needs it, its
  `TILES/*.xml` entry into a scratch kit), never on the user's real files. Ask the user to run TEd interactively when
  you need pixels or clicks you cannot make safely; they have already sent screenshots. TEd uses about 800 MB: close
  it when done (it is yours if you started it; do not kill a TEd the user opened).
- **Do not kill a running `wce.exe` of the user** (LNK1168 on link means one is running): build in the private tree.
- **Commits:** on `master` of this repo only, no push, small commits that build, never skip hooks; end the message with
  the attribution line the harness gives you (the old prompt's `Opus 5.5` line is out of date).
- **Rendering uses the game's own shaders (mandatory, user rule of 2026-10-10):** whatever the editor draws to show the
  game's look (terrain, water, sky, effects) goes through the compiled shaders of the packs' `fxc/` folder
  (`ShaderLibrary` / `ConstantBuffers`, `libs/render/water_pass.cpp` is the pattern), fed the inputs the engine feeds
  them, with every unverified input marked ASSUMPTION in `docs/re/`. No look-alike HLSL, no CPU stand-ins for what the
  game computes in a shader. Read the **DXBC disassembly** of the `dx11_*.fxc` (`cli/fxc_dump <dir> <out> <asm dir>`)
  for orientation and constants: the packs' `glsl_sm_*` listings are the GL path and differ (e.g. the y flip of the
  screen-to-NDC step). Editor overlays (markers, gizmos, link cells, outlines) are not game renders.
- **Third-party code:** nothing new without asking (ImGui, ImGuizmo, zlib are approved).
- **No machine paths in code**, tool paths from `%LOCALAPPDATA%\awe\wce.ini` / environment variables; scripts may
  default to this machine but accept overrides.
- **Shell:** Bash heredocs mangle backslash escapes, and PowerShell state does not persist between calls: write C++
  and scripts with the Write / Edit tools; keep helper scripts in the scratchpad.
- **Reverse engineering** is targeted and time-boxed (see the plan). Record behaviour, formats and constants, not code,
  in `docs/re/` and `docs/formats/`; never ship game assets or shader binaries.
- After Phase 0b, **do not go on by yourself**: the user wants a report after every phase and their "go". The order
  after that (decided in the plan) is 1 (layers, gizmo, numeric entry), 2 (placement modes), then the terrain and
  water spikes alongside the read-only links view, 3, 4, 6a, 5, 6b, 7.
