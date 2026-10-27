# Warscape Community Editor: roadmap

## Status (2026-10-26)

`wce` is one Rust application (a Cargo workspace, one crate per layer under `crates/`) with one renderer on the game's own shaders, one
shared scene and selectable tools. It replaced the two C++ editors (weather_editor, Tile Studio) and the C++ `wce`: the merge (phases 0 to
8, 2026-10-09 to 10-13) and the Rust port (2026-10-13 to 10-19, then the C++ tree was removed) are done; their records are
[docs/baseline.md](docs/baseline.md) (the gates and their baselines) and [docs/rust_port_app_and_gates.md](docs/rust_port_app_and_gates.md)
(the port's decisions and the per-crate checks against the C++ code). What each tool does is in [docs/editor.md](docs/editor.md) and
[docs/ui_editor.md](docs/ui_editor.md).

| Tool | State |
|---|---|
| Weather / Lighting | built: `.environment`, `.lighting`, weather definitions and vfx files, the weather picker and variation database |
| Tile | built: battle tiles (campaign tiles open and export with limits), TEd's authoring, the game's look, export to the game, procedural walls, siege AI generation |
| UI | built: UI layouts (binary <-> XML), canvas, `wce-ui` CLI and MCP server |
| VFX, Unit, Terrain (battle map / Terry), Cinematic | placeholders (the `placeholders` feature of `wce_app`) |

## The plans

Each open item lives in one plan; the others point to it.

| Plan | Covers | State |
|---|---|---|
| [workflow_review.md](docs/plans/workflow_review.md) | TEd's workflows one by one, what wce does and what it needs (a conversation with the user) | in progress |
| [tile_ted_parity.md](docs/plans/tile_ted_parity.md) | the Tile tool against TEd: what is still missing (terrain painting, selection tools, links editors, the tile module) | live |
| [tile_roundtrip.md](docs/plans/tile_roundtrip.md) | the game-neutral tile model, the per-game readers and writers, the decompile and the export (stages A to C), the corpus checks | mostly done |
| [procedural_walls.md](docs/plans/procedural_walls.md) | walls generated along a drawn spline, exported in a pack | mostly done |
| [siege_ai.md](docs/plans/siege_ai.md) | Generate SiegeAI: a settlement's siege AI layout in one click | done but in-game tests |
| [launch_in_game.md](docs/plans/launch_in_game.md) | Launch in game: the open tile exported and the game started straight into a battle on it | design; command line gate passed (loose XML, mods as arguments) |
| [map_context_todo.md](docs/plans/map_context_todo.md) | map-level terrain: the neighbour tiles, the seams, far maps, the outfield, large-context performance | open |
| [rendering_remaining.md](docs/plans/rendering_remaining.md) | the rendering gaps of the tile view (sea, water, shadows, engine details) | open |
| [vegetation_remaining.md](docs/plans/vegetation_remaining.md) | the vegetation port's leftovers | small |
| [weather.md](docs/plans/weather.md) | the Weather tool's leftovers: a VFX editor, campaign mode | open |
| [weather_current_state.md](docs/plans/weather_current_state.md) | the particle and VFX engine items still approximated | open |
| [editor_todo.md](docs/plans/editor_todo.md) | editor-wide ideas: a minimap with views, siege AI problems on save, autosave | open |
| [tile.md](docs/plans/tile.md) | tilemaps and the map pipeline (a later scope) | not started |

## Project-level open items

- **Release readiness:** choose the licence (the user's call; no `LICENSE` yet); a GitHub Actions build with the non-corpus tests; a
  release zip and a first-run README (game, RPFM schema); a content audit (no game assets or shader binaries in the repo, no machine
  paths). `THIRD_PARTY.md` is up to date.
- **The BOB oracle gate:** `scripts/gates.sh` has no `oracle` label; BOB comparisons run through `bob_tile_check` and the RE scripts.
- **`scripts/shots.sh`:** the two dry captures differ from their baseline by up to 2 levels in about 3 % of the pixels since before the
  merge's phase 7; re-baseline them or find the cause.
- **Future tools** (seams only): Unit editor (skinned instances, bone palettes, anim / skeleton / variantmeshdefinition formats),
  VFX editor (see weather.md), battle map / Terry (map-scale terrain, tile_map and lf_height formats; see tile.md), Cinematic editor
  (keyframed scenes, recording).

## Decisions that still hold

- **Games:** Attila first; every game-specific fact behind one game profile, so that the other Warscape games (Rome 2 to Pharaoh
  Dynasties) can follow.
- **Data:** game and mod files through rpfm_lib (in-process); the Assembly Kit's raw files on disk; the editor's projects in the
  projects folder (`<projects>/assembly_kit`), never in the kit unless saved for it.
- **Fidelity:** byte-identical round trips and minimal edits (only the edited spans change); the game's own shaders for everything that
  shows the game's look (no look-alike HLSL); BOB's outputs reimplemented with real BOB as the oracle.
- **Documents:** each tool keeps its own lossless model and publishes into one shared scene; many documents open at once.
- **UI:** Dear ImGui docking, a default layout per tool, layouts saved per tool.
- **Audience:** a public community release later: no machine paths in code, a first-run setup, never ship game assets, permissive
  third-party licences only.
