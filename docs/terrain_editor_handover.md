# Terrain Editor developer handover

Maintained entry point for Attila and Rome II, following the [Terrain roadmap](plans/terrain_editor.md). Last verified: **15 November 2026**, upstream base `master` at `45fa5fe8f334aa72451aa756671f73174d5e0ad9`, with the locally prepared self-contained Reliable Terrain Save/Export feature including its required publication modules/tests directly. It has no dependency on unmerged #51 and leaves upstream Tile I/O unchanged. This document describes source and accepted local behavior through D3E; it does not certify every profile or game-runtime output. Update the revision, availability and focused verification when a feature lands.

## Start here

Read this handover, then the relevant part of the roadmap. Reuse the existing [map formats](formats/map_files.md), [terrain notes](re/terrain.md), [battle-map notes](re/battle_maps.md), [Terry/BOB notes](re/terry.md), [Save plan](plans/save_system.md), [editor guide](editor.md) and [Rome II plan](plans/rome2.md) for detail. Their dated progress entries are historical; use source and the revision table below for current availability.

`MapKind::Battle` includes a campaign's **battle** map under `terrain/battles/`; `MapKind::Campaign` means strategic terrain under `terrain/campaigns/`. Similar `main_*` names do not make their height/normal workflows interchangeable. LF means low-frequency map height; HF is the placed tiles' detail.

## Availability and accepted revisions

Acceptance, publication and upstream merge are separate states. This feature branch starts directly from verified upstream master and includes its publication infrastructure. Upstream autosave/recovery is retained; restored documents stay unsaved after Save As or failure until successful owned Save. Paths marked **local** below refer to the named accepted revision and may not exist in this checkout; inspect them with `git show <revision>:<repository-path>`, obtaining missing Git objects through the contribution owner. No private report or game-resource archive is required.

| Capability | Upstream at the revision above | Accepted local boundary / delivery status |
|---|---|---|
| Map model/view, raw LF/cell/Terry edits, native battle LF/placements/scene, builders | Present, with profile limits below | Preservation and Save improvements are separate unmerged work |
| Checked single-byte BLM names and writer errors | Merged [#49](https://github.com/MrJox/warscape_community_editor/pull/49) | Retain error propagation in later callers |
| Native v9 placement integrity | Corrected contribution not merged | [#50](https://github.com/MrJox/warscape_community_editor/pull/50), history-preserving head `c91f91005fabc84a8c6338fe1eb80340c8abc58f` |
| Shared publication helpers | #51 not merged; required staging included directly on this branch | [#51](https://github.com/MrJox/warscape_community_editor/pull/51) remains at `8a2b5e9d313d3c579ac54c074b1a39c878028032`; included modules sync staged data before rename and preserve retention/permissions; Tile-only changes are excluded and old #51 is pending explicit supersession disposition |
| Attila height-metadata reopen correction (F1) | Merged [#57](https://github.com/MrJox/warscape_community_editor/pull/57) at `33892a81` | [#57](https://github.com/MrJox/warscape_community_editor/pull/57), `8ab7c89faffa231ee5710874c1fd1ffdc7cb5534`; also the prerequisite in accepted D3E |
| Terry overlay bulk removal (F2) | Merged [#58](https://github.com/MrJox/warscape_community_editor/pull/58) at `03f14742` | [#58](https://github.com/MrJox/warscape_community_editor/pull/58), `1261fb908c471b02866dae0116732c485aa70aa9` |
| Native campaign prop model/I/O | Merged [#59](https://github.com/MrJox/warscape_community_editor/pull/59) at `fe596a24` | [#59](https://github.com/MrJox/warscape_community_editor/pull/59), `b5af4e73e2c75d2298004841173d8cd3c5c8a10a`; model/I/O only, not editor controls or Save |
| Reliable Terrain Save/Export, freshness and native scene preservation | Present on this local feature branch; not upstream-merged | [Usage, supported profiles, refusals, recovery and focused tests](plans/terrain_save_export.md) |
| Native prop controls/history, bounded campaign authoring, shared validation | Not merged; not included in this feature | Accepted integrated D3C `5f6dca2595dda2c75946852e228e541e4f84d4e2`; packaging prerequisites remain unresolved |
| Incremental prop/overlay refresh (D3B-P) | Not merged | Accepted `a0ae3fc69b2c534d0b9e69d4cae9c1883f622f0c`; prepared performance contribution `8be7b8c7aad1d89567235b00b16051001834f0f5` depends on F2 |
| Exact land LF region PNG (D3E) | Absent | Owner accepted **`43109bf082ea3455d096ace02527b501bcbebd23`**, unchanged and unpublished; complete feature delivery awaits queue/documentation coordination |

The earlier native-prop milestone is `1ac29cd3a678cdd6054d974552732e1658b42909`; accepted Attila F1/F2 integration is `35c4f358754f6841dd7ad00f4b8b13ad732669e8`. These are provenance landmarks, not recommended upstream bases. D3E is independently based on F1 and does **not** contain all D3C work. Its D3C compatibility composition is a test witness, not a delivery prerequisite.

Height-only preparation [#52](https://github.com/MrJox/warscape_community_editor/pull/52) remains **held**, although GitHub lists it open. D3C contains that work transitively through campaign authoring. Do not import the whole integration to get validation or Save: check module/type/build dependencies as well as runtime calls. Work 3 owns feature packaging and coordination with Frodo/Victimized. New submissions are paused pending review; this handover authorizes no publication or history restructuring.

## Architecture and source map

Data flows from mounted sources through `MapFiles` and a format reader into `MapModel`, then through `MapDocument` commands/history to the view. Save prepares authoring resources; Export builds native resources. Keep these boundaries explicit.

| Layer | Main source references | Responsibility |
|---|---|---|
| Sources/game facts | [`wce_tile/src/sources.rs`](../crates/wce_tile/src/sources.rs), `wce_vfs`, `wce_map_view::maps`, Terrain `tool.rs` open inputs | Projects/ordered kits/packs, map listing, profile and database selection |
| Lossless data/model | [`wce_terrain/src/map.rs`](../crates/wce_terrain/src/map.rs), `paged.rs`, `lf_edit.rs`, `cells.rs`, `terry.rs`; `wce_formats` | `Item<T>` keeps origin bytes and edit state; `carried` retains opaque resources; 64 x 64 u16 pages and bounded undo records |
| File adapters/builders | [`wce_terrain_io/src/lib.rs`](../crates/wce_terrain_io/src/lib.rs), [`attila.rs`](../crates/wce_terrain_io/src/attila.rs), [`built.rs`](../crates/wce_terrain_io/src/built.rs), `attila/{build,heights,normals}.rs`, `built/scene.rs` | Named blobs, format conversion, minimal edits and native generation; publication is a separate concern |
| Document/history | [`wce_terrain_editor/src/document.rs`](../crates/wce_terrain_editor/src/document.rs), `lf.rs`, `cells.rs`, `terry.rs`; `wce_editor_core::{command,document}` | Commands, Undo/Redo, saved state, LF mirror, scene references, Save destination |
| Tool/UI/jobs | [`wce_terrain_tool/src/tool.rs`](../crates/wce_terrain_tool/src/tool.rs), `refill.rs`, `placements.rs`, `terry_view.rs`, `prop_models.rs`, `validate.rs`, `export.rs` | Editing controls, worker refill/export, picking/selection, overlays and diagnostics |
| View/stream/render | `wce_tile_editor::tile_document::context::{Frame,stream,template,cache}`, [`wce_render/src/renderer/terrain.rs`](../crates/wce_render/src/renderer/terrain.rs) | Map coordinates, cached tile templates, chunk streaming, LF dirty regions and game shaders |

Local additions at D3C: `wce_terrain/src/campaign_props.rs`, `wce_terrain_io/src/built/{campaign,campaign_authority,campaign_heights}.rs`, `wce_terrain_editor/src/{native_props,campaign_lf,save_validation}.rs`, and foundation `publication`, `disk_changes`, `save_path`, `operation`. Native marker translation stays associated with immutable source records; campaign authoring metadata is not native output authority.

Local D3E sources: `wce_formats/src/lf_region_png.rs` (bounded codec), `wce_terrain/src/lf_region.rs` and `lf_edit.rs` (checked rectangle/page replacement), `wce_terrain_editor/src/lf_region.rs` (document/history), and `wce_terrain_tool/src/lf_region.rs` (panel/commands). Its `docs/plans/lf_region_png.md` is the detailed feature contract; retain and link it when integrating D3E rather than duplicating it here.

## Workflows and supported operations

| Workflow | Input and authoring | Persistence/export boundary |
|---|---|---|
| Attila raw/kit battle or campaign | `assembly_kit/raw_data/terrain/{battles,campaigns}/<map>`: land/sea TIFF, cell PNGs, explicit tiles, `.terry`, XML and opaque files. LF raise/lower/smooth/flatten/noise, cell painting/fill, explicit placements, Terry entities/history | Save to project raw layout and reopen; native builder emits loose resources and a pack. TIFF sample edits preserve headers/tails. F1 resolves scale/offset from layered database settings independently of the saved map folder |
| Rome II built/native battle | Pack `terrain/battles/<map>` or project `assembly_kit/working_data/terrain/battles/<map>`: full LF DDS, CHMF, tile list or v6 tile map, BLM/deployments and normals | Bounded native LF, placement and scene edits; Save project working files, reopen project first, Export differences from pack inputs. Never manufacture a raw-kit round trip |
| Rome II native campaign | `terrain/campaigns/<map>` with campaign database and native resources; preview plus accepted local existing-prop X/Z editing | Local prop/history/Save/reopen works on supported marker profiles. Bounded raise/lower authoring on supported campaign profiles may persist WCE state/baselines; **effective edited-height native Export is refused** in the accepted local stack |

Kit-layout resolution is project first, then Assembly Kits in settings order, then packs where supported; model/texture lookup follows mounted game source order. F1 uses the first existing layered database settings file, even if it lacks render values. Do not substitute hardcoded campaign height constants. A saved map folder need not contain its database settings.

Placement support is profile-dependent: native tile-list variation/turn/move/place/remove must satisfy bounds/overlap and writer guards; v6 reconstruction needs the correct database. Corrected v9 handling permits supported count changes with zero trailer rows, but paired nonzero rows allow only supported same-count edits. Unknown/malformed metadata remains opaque for unchanged output and refuses unsafe edits. Native `tile_map.png` regeneration and general shared-geometry authoring are incomplete.

Attila Terry supports existing scene/prop/VFX/volume editing and creation paths; native battle scenes map supported BLM/deployment structures into the entity tools. Local native campaign props translate existing uniquely associated records only: retain Y, rotation, scale, record identity/order and other bytes. Creation/deletion, model replacement, rotation/scale and native campaign VFX/volume authoring are unsupported. Attila landscape-bound movement currently recomputes Y; whether that matches intended native semantics remains unresolved.

D3C extends **Validation â†’ Check the map** / `terrain_validate` with read-only resource/reference/capability findings, distinguishing invalid data, unsupported operations and informational limits. It reuses writer guards and reports known campaign palette/fill limitations. A clean result is not native export or in-game certification. Startpos settlements, roads, farmland, sprawl and forests include preview/read paths; their display does not imply corresponding strategic editing support.

D3E exports a land LF rectangle from any decoded raw/native document and imports only into raw/kit documents. Coordinates are file samples, top-left origin, x right/y down. Import requires same-size non-interlaced **16-bit grayscale** PNG without alpha/transparency; no gamma conversion, resampling or normalization. Limits: 1,048,576 samples and 16 MiB input. Changed import creates one existing page-based Undo step; no-op/error/cancel retains history/redo. Export creates a new file and refuses an existing destination. Native import refuses before file access or mutation; sea LF, map resizing and TIFF region interchange are outside D3E.

```text
terrain_lf_region_export <x> <y> <width> <height> <new.png>
terrain_lf_region_import <x> <y> <width> <height> <source.png>
```

Owner acceptance on 11 November 2026 confirmed a visible but **subtle** terrain response in a real Attila campaign preview. Exact 16-bit/outside-region preservation, Undo/Redo, raw Save/reopen and native import refusal remain established. Material/rendering limitations and game-runtime verification remain open; no further D3E rendering investigation is requested.

## Save, reopen and preservation rules

The pinned upstream base writes resources through temporary-file renames and may retain file-backed outputs by size/mtime. This feature branch instead prepares the complete output set before publication, retains exact content, checks source/destination freshness and preserves current Save As copy semantics. See the [Terrain Save/Export guide](plans/terrain_save_export.md); this branch's implementation is not yet merged upstream.

The Terrain Save feature preflights representability/source associations, stages outputs, checks destinations/content/freshness and shares a Save/Export operation lease. Failures preserve unsaved state and report partial publication; publication is not a whole-directory transaction. Save/retry, independent copy and external-change handling have focused tests. A live `normal_source_bytes` cache protects certain normal inputs during partial publication; it is not evidence of durable continuity after close/reopen. Close/reopen and multiple-edit native normal-baseline continuity remain a separate limitation; no persisted baseline format or continuity implementation is included.

Always retain opaque resources, headers, trailers, deployment extras and original source associations. No-op/reverted output should reuse original bytes; an edit must change only supported spans/resources. Refusal must not mutate model/history/selection or discard originals. Test both memory-backed pack inputs and file-backed reopened projects. Keep PNG extraction, raw project Save, WCE campaign-authoring Save and **Export built files** distinct. Existing native campaign-height guards must survive integration; upstream's generic built writer is not proof that campaign reconstruction is safe.

## Build, tests and reproduction

Use the [README build instructions](../README.md#build), lockfile and existing caches. Windows uses SDL3's CMake build and Cargo 1.91+ for configured intermediate paths; Linux needs the documented D3D11 translation dependencies. Rendering/database checks require legally installed game resources, suitable kits and RPFM schemas selected through settings. Synthetic codec/model/document tests need no game assets. The README's opening RPFM path-dependency sentence is stale at this revision: `Cargo.toml` selects the Git `develop` dependency; follow the checked-out manifest/lockfile and confirm any local override.

Run from the repository root; `--offline` assumes the existing cache is populated. These are reproducible commands, **not a claim that this documentation branch was built/tested**:

```text
cargo build --release --locked --offline -p wce_app
cargo test --release --locked --offline -p wce_terrain_io --lib --test blm_codec
cargo test --release --locked --offline -p wce_terrain --lib lf_edit
cargo check --release --locked --offline -p wce_app -p wce_terrain_io -p wce_terrain_tool --all-targets
```

On a revision containing the respective **local** feature, select its focused controls:

| Feature | Commands / fixtures |
|---|---|
| D3E | `cargo test --release --locked --offline -p wce_formats --test lf_region_png`; `cargo test --release --locked --offline -p wce_terrain_editor --test lf_region`; `cargo test --release --locked --offline -p wce_terrain_tool --lib lf_region`; `cargo test --release --locked --offline -p wce_terrain_tool --lib campaign_metadata_tests` |
| Save/native preservation | `cargo test --release --locked --offline -p wce_terrain_editor --lib publication_tests`; `cargo test --release --locked --offline -p wce_terrain_editor --test native_preservation`; fixtures in `src/publication_tests.rs`, `src/freshness_tests.rs`, `tests/common/map_fixture.rs` |
| Props/validation | `cargo test --release --locked --offline -p wce_terrain_io --test native_props`; `cargo test --release --locked --offline -p wce_terrain_editor --test campaign_props`; `cargo test --release --locked --offline -p wce_terrain_tool --lib validation_tests` |

D3E's accepted focused set recorded 18 passes, including one explicitly enabled read-only real-region test. The feature tests cover all 65,536 sample values, byte/row order, bounds/CRC/type refusal, page edges, mirrors/history, production-loader metadata and raw Save/reopen. Its synthetic document fixture is 130 x 67 samples; region `(62,62,68,5)` crosses page boundaries and the partial bottom page. Existing raw I/O tests cover TIFF minimal edits. Do not treat historical passing evidence as validation of a newly composed feature head.

Opt-in tests are ignored by default. D3E's `real_witnesses_export_small_regions_without_mutation` uses `D3E_ATTILA_MAP` and `D3E_ROME2_MAP` read-only map folders. F1's `corrected_attila_real_save_reopen` uses `WCE_DIAG_RAW` (pristine kit raw root) and **fresh scratch** `WCE_DIAG_ROOT`; it writes scratch outputs. D3C's `validation_real_campaign_witnesses` takes `WCE_D3C_RAW`, `WCE_D3C_PROJECT_RAW`, `WCE_D3C_NATIVE`. Read each test's setup before adding `-- --ignored`; never run all ignored tests against personal projects.

For a portable D3E application check: use the tiny synthetic fixture recipe in `wce_terrain_tool/src/lf_region_tests.rs` with an isolated settings/project root; open it in Terrain, export that rectangle, edit one u16 value in an external tool that preserves the PNG type, import, Undo/Redo, Save, close/reopen and export to a different filename. Compare decoded samples and raw TIFF header/tail/outside-region bytes; an identical import must leave the redo tail. Open a supported native document and confirm export is read-only and import refuses. A tile-free fixture proves persistence, not rendering fidelity.

For native/raw format checks, use existing `wce_terrain_io/examples/{map_roundtrip_check,map_built_roundtrip_check,built_lf_check,built_scene_check,built_normals_check}.rs`; usage is in each file. Example battle-normal control in PowerShell:

```powershell
$env:WCE_GAME_KEY = 'rome_2'
cargo run --release --locked --offline -p wce_terrain_io --example built_normals_check -- '<game-dir>' terrain/battles/main_gaul
```

It checks a single edit's normals in memory, not F-B or campaign normals. The broader [gates/baseline workflow](../README.md#gates) needs a locally inventoried corpus and render resources; do not recreate archives to run a narrow check. Automated editor scripts must stay non-activating and end with `quit`; capture only the editor, preserve a user-opened instance. Run `cargo fmt --all` before Rust commits, then `cargo fmt --all --check` and `git diff --check` using the current upstream configuration. On Windows, error 206 requires equivalent per-package formatting/checks for every workspace member; report the actual fallback instead of claiming the one-shot command passed.

## Limits, priorities and resuming work

Native battle normals use a detail-preserving slope-change approximation and known shipped-chunk layouts; unsupported layouts retain originals. CHMF generation retains shipped tags/layout assumptions. Neither supplies a general native campaign normal reconstruction rule. **P0-02C remains blocked** pending a defensible campaign-normal relationship; F-B is the separate battle continuity question. Campaign rendering/material/sky/sprawl and asset coverage remain incomplete; accepted prop responsiveness in one scene is not an all-map FPS guarantee. Large global refill, LF memory and broader brush/UI parity remain roadmap work.

Current packaging priority is the complete reliable Terrain Save/Export feature. Complete D3E and incremental Terry preview remain independent later features. F-B persistence and held campaign-height preparation are outside this package. Resume as follows:

1. Verify upstream SHA, open PR heads and the feature owner's current packaging decision; update this snapshot. Use a new local branch, leaving accepted and published refs intact.
2. Pick one game/map kind, source profile and operation. Trace loader â†’ command/history â†’ writer â†’ publisher â†’ reopen; identify every real prerequisite before composing it. Do not silently absorb held #52.
3. Reuse the smallest synthetic fixture and relevant existing tests, then one read-only real witness only if needed. Record exact head, commands, changed resources and byte comparisons; keep game assets out of Git.
4. Preserve newer upstream scene/picking/invalidation APIs when integrating older work. Current-master naval/cloth/scene changes were not covered by old Terrain performance evidence; port bounded changes, not whole historical files.
5. Include feature docs, supported profiles, refusal behavior and focused verification with the proposed package. Work 3 coordinates publication with Frodo/Victimized; owner review precedes any new submission.

Publication coordination still needed: owner review of this complete feature and explicit disposition of historical #51 before replacement publication; #50 is updated and remains separate. Maintainer input is still needed for the separate native normal-continuity contract; supported native profiles and native-Y semantics; a portable real-fixture/oracle policy; and reconciliation of stale roadmap/top-level status and RPFM setup wording. None is permission to relax preservation guards or start campaign-normal reconstruction.
