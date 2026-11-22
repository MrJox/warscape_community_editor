# Warscape Community Editor: roadmap

## Status (2026-11-22)

`wce` is one Rust application (a Cargo workspace, one crate per layer under `crates/`) with one renderer on the game's own shaders, one
shared scene and selectable tools, for Total War: Attila, Rome II and Thrones of Britannia (one game profile each, `wce_foundation::game`).
It replaced the two C++ editors (weather_editor, Tile Studio) and the C++ `wce`: the merge (phases 0 to 8, 2026-10-09 to 10-13) and the
Rust port (2026-10-13 to 10-19, then the C++ tree was removed) are done; their records are [docs/baseline.md](docs/baseline.md) (the gates
and their baselines) and [docs/rust_port_app_and_gates.md](docs/rust_port_app_and_gates.md) (the port's decisions and the per-crate checks
against the C++ code). What each tool does is in [docs/editor.md](docs/editor.md) and [docs/ui_editor.md](docs/ui_editor.md).

Each tool is a Cargo feature of `wce_app` ([docs/editor.md](docs/editor.md), "Tools"): the default features have them all, the `release` feature (CI releases,
nightlies, the Flatpak) only the stable ones.

| Tool | In `release` | State |
|---|---|---|
| Weather / Lighting | yes | built: `.environment`, `.lighting`, weather definitions, the weather picker and variation database; the campaign's environment in the Terrain tool |
| Prefab | yes | built: prefabs edited on a stage of their own, placed as references ([prefab_tool.md](docs/plans/prefab_tool.md)) |
| Tile | yes | built: battle and campaign tiles, TEd's authoring, the game's look, export to the game and to mod packs, Launch in game, procedural walls and roads, rivers, siege AI, radar, variations in one project |
| UI | yes | built: UI layouts (binary <-> XML), canvas, `wce-ui` CLI and MCP server |
| Audio | yes | built (browse and listen): sound banks with names, search, what an event plays, playback |
| Terrain | no | built: battle and campaign maps (LF and tile map painting, the `.terry` scene, BCA generation, the build against BOB, export), the campaign's start and look |
| Unit | no | built: variant mesh definitions worn by a unit, the DB's looks rows |
| VFX | no | built: vfx files edited visually; user review pending |
| Cinematic | no | built: the movie viewer and editor, keyed scenes, movies rendered by the editor, historical battles; in-game checks left |

## The plans

Each open item lives in one plan; the others point to it. Plans hold decisions, a short as-built summary and their open items; findings
live in [docs/re/](docs/re/) and [docs/formats/](docs/formats/).

**Tiles**

| Plan | Covers | State |
|---|---|---|
| [tile_ted_parity.md](docs/plans/tile_ted_parity.md) | the Tile tool against TEd | done (2026-10-31); small renderer leftovers and in-game confirmations |
| [tile_roundtrip.md](docs/plans/tile_roundtrip.md) | the game-neutral tile model, the per-game readers and writers, the decompile and the export, the corpus checks | mostly done; stage A residue, campaign stage B, the move to `wce_tile_model`, the TDD corpus |
| [workflow_review.md](docs/plans/workflow_review.md) | TEd's workflows one by one, what wce does and what it needs | workflows 1 to 3 done (2026-10-26/27); none reviewed since |
| [tile_variations.md](docs/plans/tile_variations.md) | a tile's variations as one project | built 2026-11-15; gate script and adding a variation left |
| [prefab_tool.md](docs/plans/prefab_tool.md) | the Prefab tool | built 2026-11-08; thumbnails with ground, smoother outlines, the user's GUI and in-game checks |
| [launch_in_game.md](docs/plans/launch_in_game.md) | Launch in game: the open tile exported, the game started into a battle on it | built 2026-10-28, extended to 2026-11-15; Map context mode and Rome 2 not seen in game |
| [siege_ai.md](docs/plans/siege_ai.md) | Generate SiegeAI in one click | built 2026-10-24, steep ground and wall zones 2026-11-03; in-game checks and 4 kit-tile issues |
| [radar.md](docs/plans/radar.md) | the battle minimap (radar) generator | built 2026-11-11, exported 2026-11-15; user tuning and the rotation question |
| [procedural_walls.md](docs/plans/procedural_walls.md) | walls generated along a drawn spline, exported in a pack | built 2026-10-26, rounds to 2026-11-16; phase 6 gate, TDD Rohan / cliff, in-game and TEd checks |
| [kit_walls.md](docs/plans/kit_walls.md) | walls of the game's own pieces (Procedural off) | built 2026-11-13; user review and a Save for Assembly Kit run |
| [building_splines.md](docs/plans/building_splines.md) | fences and joiners along a line | built 2026-11-13; force-axis setting and tech overlay / radar |
| [procedural_roads.md](docs/plans/procedural_roads.md) | roads along splines | built 2026-11-08, variety 2026-11-16; in-game checks, Phase 0, pinned heights, bridge bases |
| [rivers.md](docs/plans/rivers.md) | river tools: beds, shores, links | built 2026-11-22 incl. Rome 2; in-game checks and Thrones |
| [auto_detail.md](docs/plans/auto_detail.md) | texture rules, biomes | A and C built 2026-11-20, B shelved; the user's look |
| [auto_detail_2.md](docs/plans/auto_detail_2.md) | six more auto-detail tools | shelved 2026-11-21 (`feature/shelved_*` branches) |
| [ground_coverage.md](docs/plans/ground_coverage.md) | automatic ground coverage | shelved 2026-11-21 (`feature/shelved_ground_coverage`) |
| [vegetation_remaining.md](docs/plans/vegetation_remaining.md) | the vegetation port's leftovers | missing converted models, farm_eastern squares, leaf density |
| [map_context_todo.md](docs/plans/map_context_todo.md) | the neighbour tiles, the seams, far maps, the outfield | seams item 1 done; items 2 and 3, far maps / outfield, resolve layers, far-tile culling |

**Maps and campaigns**

| Plan | Covers | State |
|---|---|---|
| [terrain_editor.md](docs/plans/terrain_editor.md) | the Terrain tool, Terry's replacement; the old tile plan's map pipeline scope | Phases 0 to 9 built 2026-11-02 to 05; campaign normals, Rome 2 built-form leftovers, settlement-size and BCA checks, launch checks |
| [terrain_save_export.md](docs/plans/terrain_save_export.md) | reliable map Save / Export (contributed, #74) | merged 2026-11-22; normal baseline across reopen |
| [lf_region_png.md](docs/plans/lf_region_png.md) | land LF region PNG interchange (contributed, #75) | merged 2026-11-21 |
| [campaign_tilemap.md](docs/plans/campaign_tilemap.md) | the campaign map drawn as the game draws it | built 2026-11-19 to 21 for Attila, TDD and Thrones; paused: walls / flags, army colours, rain constants, forest cost |
| [campaign_environment.md](docs/plans/campaign_environment.md) | the campaign map's lighting, sky, fog and weather | Phases 1 to 5 built 2026-11-16 to 18; sphere overlays, live relighting, editing (Phase 6), Rome 2 `light_grid.xml` |
| [startpos_editing.md](docs/plans/startpos_editing.md) | the campaign's start (characters, forces) | read and drawn for Attila, TDD and Thrones (2026-11-21); editing (Phases 3 to 6) deferred |
| [rome2.md](docs/plans/rome2.md) | Rome 2 support (packs only, no TEd / BOB tiles) | Phases 0 and 1 done, renderer and editor partial; map context sea holes, launch at a placement, in-game checks |

**Rendering and weather**

| Plan | Covers | State |
|---|---|---|
| [rendering_remaining.md](docs/plans/rendering_remaining.md) | the rendering gaps | battle sea stand-ins, in-game graphics checks, shadow stability, smaller items |
| [weather_current_state.md](docs/plans/weather_current_state.md) | the particle and VFX engine items still approximated | 6 approximations open |
| [msaa.md](docs/plans/msaa.md) | MSAA as the game does it | built 2026-11-01; OIT with MSAA, editor overlays on MS targets, 4x / 8x captures |
| [weather.md](docs/plans/weather.md) | the Weather tool's leftovers | built; campaign mode moved to campaign_environment.md; "one panel" open |
| [vfx_tool.md](docs/plans/vfx_tool.md) | the VFX tool | built 2026-11-18; user review pending |

**Units, audio, cinematics**

| Plan | Covers | State |
|---|---|---|
| [units.md](docs/plans/units.md) | units in the renderer: skinning, variant meshes, animation | steps 1 to 7b built 2026-11-06 to 11; close-ups (step 8) |
| [unit_editor.md](docs/plans/unit_editor.md) | the Unit tool | built 2026-11-15; in-game DB check, new faction rows, DB undo |
| [audio.md](docs/plans/audio.md) | the Audio tool and the shared audio layer | built 2026-11-19 to 20; UI button anomaly, unit-lines pack 2, by-ear checks |
| [cinematic.md](docs/plans/cinematic.md) | the Cinematic tool: movies and historical battles | built 2026-11-20 to 21; every in-game check and the Tile tool extras (zones, reference image...) |

**Editor-wide**

| Plan | Covers | State |
|---|---|---|
| [editor_todo.md](docs/plans/editor_todo.md) | editor-wide ideas | siege AI problems on save, Rome 2 ship checks, piers, the shared scene (to discuss) |
| [ux_todo.md](docs/plans/ux_todo.md) | small UX items | where the display toggles go, weather chips, map Save as, draggable, tooltips |
| [autosave.md](docs/plans/autosave.md) | autosave and recovery | built 2026-11-14; prefab stages and an interactive try |
| [save_system.md](docs/plans/save_system.md) | where each tool saves | done 2026-11-08 |
| [outliner_removal.md](docs/plans/outliner_removal.md) | one document per tool, no Outliner | done 2026-11-08; stand-ins for other tools |

## Project-level open items

- **Release:** CI builds and tests (`.github/workflows/test.yml`), tagged releases and nightlies (`release.yml`, `nightly.yml`: the
  Windows zip and the Linux Flatpak) and the Update Manager exist. Left: a `LICENSE` file (the workspace's `Cargo.toml` says MIT since
  2026-10-19; the user's call); a content audit (no game assets or shader binaries in the repo, no machine paths: `scripts/gates.sh` and
  the scripts still default `WCE_GPU` and `WCE_GAME` to one machine's values, overridable). `THIRD_PARTY.md` is up to date but for the
  credit of TheTollski's `TWA_Names.txt` (`third_party/soundbank_editor`, docs/plans/audio.md).
- **The BOB oracle gate:** `scripts/gates.sh` has no `oracle` label; BOB comparisons run through `bob_tile_check` and the RE scripts.
- **`scripts/shots.sh`:** the two dry captures differ from their baseline by up to 2 levels in about 3 % of the pixels since before the
  merge's phase 7; re-baseline them or find the cause.
- **Map context: lakes at altitude** (2026-10-31, TDD's `rom_third_age_map`, the Long Lake around Esgaroth, 626 m): fixed in the editor
  (its `sea` tiles stay on the land's LF when it is above the sea, `Frame::on_sea_lf`); the lake's water there comes from its tiles
  (which ones not traced). Not checked in game: whether the game draws water on such a lake.
- **Fixes to port to rpfm_lib** (a pass later, user 2026-11-06; until then the editor works around them):
  - `rigidmodel`: a `Weighted` vertex's second weight. rpfm reads byte 3, unused and always 0; the shaders use `1 - w` of byte 2. Reads only:
    the bytes round-trip. Worked around by `wce_render::anim_pose::vertex_weights`.
  - `anim`: only the header is decoded; `wce_formats::anim` reads versions 4 and 5 whole, and the bones without a track take the skeleton's
    pose. Also `anim_meta`, the variant mesh parser and the fragments, if they should live there.
- **After the final baseline, not before** (user 2026-11-07: accuracy to the game first, improvements on top of it once everything important renders properly):
  - Steady shadow cascades. The cascades are sized by this frame's depth range, as the game does; whatever moves at its near end (grass, soldiers) changes it by
    centimetres every frame, each new size refits the light boxes, and a soldier's shadow on himself shimmers mildly (a still scene with that jitter added: up to 52
    levels, on the soldiers only, nothing with shadows off). Tried and taken out: hold the range while the depth seen stays inside it, with room left to each side
    when it moves (near: 0.5 + 10 % of the near depth, in game units; far: 5 %), in `shadow_fit`, used by `pass_shadows`.
- **Variant mesh error detection tool** (user 2026-11-07): a check that finds what is wrong in a pack's `.variantmeshdefinition` files, for modders: files that aren't XML
  (`variant_mesh::repair` reads them, and the Unit tool marks a repaired file, but nothing says what it changed: TDD has 11 of 8,507), references to definitions
  that don't exist (TDD's `rough_helm2_blond`), models and textures that don't exist. The editor only drops these without a word.

## Decisions that still hold

- **Games:** Attila first; every game-specific fact behind one game profile, so that the other Warscape games (Rome 2 to Pharaoh
  Dynasties) can follow. Rome 2 and Thrones of Britannia are supported; Rome 2 from its packs only (no TEd / BOB tiles).
- **Data:** game and mod files through rpfm_lib (in-process); the Assembly Kit's raw files on disk; the editor's projects in the
  projects folder (`<projects>/assembly_kit`), never in the kit unless saved for it.
- **Fidelity:** byte-identical round trips and minimal edits (only the edited spans change); the game's own shaders for everything that
  shows the game's look (no look-alike HLSL); BOB's outputs reimplemented with real BOB as the oracle.
- **Documents:** each tool keeps its own lossless model and publishes into one shared scene; one document per tool (no Outliner).
- **UI:** Dear ImGui docking, a default layout per tool, layouts saved per tool.
- **Audience:** a public community release: no machine paths in code, a first-run setup, never ship game assets, permissive third-party
  licences only.
