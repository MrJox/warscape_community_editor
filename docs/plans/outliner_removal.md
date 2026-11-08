# One document per tool, no Outliner

The editor holds any number of documents of any tool at once (`DocumentManager`: a list, one of them active), and the shared **Outliner**
panel lists them all, each with its tree. This plan makes it **one document per tool**: opening a tile closes the open tile, opening a
weather file closes the open weather file, and so on. Documents of *different* tools still live side by side (a tile and a weather file;
a tile and the Prefab tool's stage). The Outliner goes with it: what it did for documents moves to the File menu and the menu bar, and
its trees are dropped.

State: planned 2026-11-08 with the user, steps 1 to 9 done the same day (how it works now: [editor.md, Cross-tool
composition](../editor.md#cross-tool-composition-phase-7)). It closes the save system plan's open point ("several tiles drawn at once":
not wanted).

## Why

- Several documents of one tool raise questions the tools answer each in their own way: which tile the Tile tool works on
  (`wce_tile_tool::document_id`: the active one if it is a tile, else the first), which UI layout (`wce_ui_tool::active_doc`, the same
  rule), which map the Terrain tool's view follows (the active one, with a camera kept per map), which environment lights the Weather
  tool (`env_doc`: the last active one). Each rule is a source of "why is it editing that one" surprises.
- The Outliner is mostly a document switcher for a situation that no longer exists. Its trees are covered by each tool's own panel
  (the Tile tool's Scene panel, the Weather tool's Properties, the UI tool's UI Layers) or by viewport picking (the Terrain tool's
  `.terry` entities).
- It costs: a cached tree per document (`outline_stamp`, `docs/performance.md`), an `outline` on every document type, a dock slot in
  every layout.

## Decisions

- **One document per tool.** At most one document per tool id that the user opened. A tool's stand-in (the Weather tool's sample map,
  `Document::stand_in`) does not count: it stays as it is. The tools: Weather (one file, whatever its kind: `.environment`, `.lighting`
  or `.xml`), Tile (one tile), Prefab (one stage, as already: `PrefabTool::open`), UI (one layout), Terrain (one map).
- **Different tools keep coexisting.** A tile and a weather file, a tile and a prefab stage, and so on; the cross-tool features stay
  (the weather preview on a tile, the weather tool lighting an open tile, the stand-in hidden while a tile is open).
- **Opening replaces.** Opening a file a tool already has open just switches to it (the `source_key` / `opened_from` checks of
  `open_path_with`, as today). Opening another file closes the tool's document first, through the unsaved-changes prompt when it is
  modified: **Save** (then open), **Discard** (open without saving), **Cancel** (nothing happens). If the save fails nothing changes and
  the prompt says why, as for Close. The old document goes only once the new one is open: if the open fails, the old one stays, with its
  changes if they were not saved.
- **The active document is the active tool's.** `DocumentManager`'s own active id goes: the menus, History, Save, Close and the default
  Inspector target follow the active tool's document. Switching tool switches document. A click in the viewport on another tool's item
  (a tile's building while in the Weather tool) selects it as today (the Inspector shows it, its edits go on the tile's history) but does
  not change the active document.
- **The cross-fade goes**: the Weather tool's "cross-fade with" combo and slider, `blend_doc` / `blend_t`, and the `blend` command. The
  scene's `EnvironmentSlot::blend` and the renderer's blend stay (`render_check` covers them; the game blends environments).
- **The trees go, with no replacement.** The tile's tree (the Scene panel has it, all but the "building piece" rows), the `.terry`
  entities (the viewport picks them; selection, Inspector and gizmo work as before), the weather XML tree (the Properties panel), the
  UI tree (UI Layers), the sample map's. Building pieces are no longer selectable from the UI: their properties are session-only and
  mostly not drawn. The piece code (`piece_item`, `PieceProps`, the piece Inspector) stays, reachable through `tile_inspect <elem>
  [piece]` and `tile_edit ... piece <n>`, for the building instance parameters research to pick up later.
- **Where the document shows**: a `[Tool] title *` label at the right of the menu bar for the active tool's document (amber while it has
  unsaved changes, as the Outliner's title was), and the same in the OS window title (`WCE - [Tile] aa *`).
- **What the Outliner's document entries become**: Save all, Save, Close, Revert to game's version are in the File menu already; Edit in
  its tool / the double-click is the tool switcher; the eye, Show only this document and Show all documents go (visibility is the
  tools' business: the stand-in rule, the Prefab tool hiding the tile and the Tile tool hiding the stage). "(weather preview)" stays in
  the `documents` command's output only.
- `outline_items` (what the viewport outlines for a selected item) has nothing to do with the Outliner and stays.

## Steps

1. **`DocumentManager` keyed by tool.** `of_tool(tool) -> Option<DocumentId>` (the tool's document, stand-ins excluded) replaces
   `active` / `active_id` / `set_active`; `active()` takes the active tool's id (or the editor wraps it: `Editor::active_document`).
   `add` refuses a second non-stand-in document of a tool (an error, not a silent replace: the replace is the editor's, with its
   prompt). `order`, `unsaved`, `save_all`, `close` stay (the list is now at most one per tool plus the stand-ins). Rewrite the
   multi-document part of `wce_editor_core/tests/editor_check.rs`: a second document of a tool is refused, documents of two tools
   coexist, the stand-in rules unchanged.
2. **The editor's open replaces.** `open_path_with`: when the claiming tool has another document, close it through the prompt (a new
   `PromptKind::Replace`, or `Close` with a pending open), then open; the old one closes only after the new one opened. Every other place
   a tool adds a document goes through the same path: the Tile tool's New tile, kit and exported-tile opens (`open_exported`) and the
   tile browser; the UI tool's folder open; the Terrain tool's `adopt` and `open_now` (the map is read on a worker for up to a minute
   and a half: ask before the read starts, replace in `adopt`); the Prefab tool's `open_prefab` (already closes its stage: move it onto
   the shared path). Scripts: `open <path> [save|discard]` (and the tools' own open commands) fail when the tool's document has unsaved
   changes and no answer, as `close` does; `open_prompt <path>` opens the prompt as the menu would, `answer` answers it.
3. **The tools drop their "which one" rules.** Tile: `document_id` becomes the owner tool's document (Tile or Prefab), the `busy` list
   of open project folders (`tool.rs:1290`) and the loops of `prefab_panel.rs` / `prefab_tool.rs` over `docs.order()` work on the one
   tile and the one stage. UI: `active_doc`, `export_pack` and the reload loop (`tool.rs:1469`, `1532`) on the one layout. Terrain: one
   map: `cameras` becomes one saved camera (or goes), the view follows the tool's document, `Job::Built` checks it is still that one.
   Weather: `env_doc` is the tool's document when it is an environment. Remove the `set_active` calls after selections
   (`viewport.rs:952`, the tile tool's `select_elem`, `tile_select_all`, the Scene panel's row click, the Prefab tool).
4. **The cross-fade goes**: the combo and slider (`wce_weather_tool/src/tool.rs` around line 308), `blend_doc` / `blend_t` and their use
   in `update`, the `blend` command (`commands.rs:150`), and its lines in `scripts/weather_smoke.txt` (which opens the rain environment
   after the dry one: the second open now replaces the first, `assert_documents 3` becomes 2: the environment and the sample map).
5. **The Outliner panel goes.** `panels::outliner` and `draw_node`, `Panels::outliner` and its View menu entry, `"outliner"` in the
   `panel` / `show_panel` lists (`commands.rs:70`, `822`, `1114`), the `Outliner` dock entry of the base layout (`tool.rs:57`) and of the
   Tile, Weather, UI and Terrain layouts (the Terrain tool's left slot keeps its Map panel). The `layer_visible` command goes (it was the
   eye); `assert_layer_visible` stays for tests of the tools' visibility rules. The `activate` command goes: `scripts/cross_tool.txt`
   uses `tool weather` / `tool tile` instead. Users' saved layout files may still name an Outliner window: Dear ImGui ignores it, and
   `layout reset` gives the new default.
6. **The menu bar label and the window title** (decision above), with the dirty colour from `theme::modified`.
7. **The trees go from the documents.** `OutlinerNode`, `Document::outline`, `outline_cached`, `invalidate_outline`, `outline_stamp` and
   `DocumentCore::outline`; the implementations in `wce_tile_editor` (`view.rs:2091`, `mod.rs:1055`), `wce_weather`
   (`xml_file_document.rs:330`, `sample_map_document.rs:178`), `wce_ui_tool/src/document.rs:279`, `wce_terrain_editor/src/document.rs:380`;
   `SpliceEffect::Touched::outline` and what feeds it in `tile_document/commands.rs` and `edits.rs` (it only told the outline stamp to
   move). Tests and examples: `wce_tile_editor/tests/tile_document.rs` finds pieces through `piece_item` instead of the outline;
   `doc_dump` and `weather_dump` drop their outline column (the C++ comparison is retired and its code gone, so nothing else changes).
8. **Docs.** `docs/editor.md` (the panels row, the documents paragraph at line 62, the activate / Outliner sentence at 149, the Save
   All, Dirty markers, Close prompts and Outliner rows, the Revert paragraph, the default layout at 202: describe one document per tool,
   the replace prompt, the menu bar label); `docs/ui_editor.md:40`; `docs/rust_port_app_and_gates.md` (the shell's panel list);
   `docs/formats/tile_ted.md:68` (pieces: no longer outliner children, commands only); `docs/plans/tile_ted_parity.md` (the "shared
   Outliner" notes); `docs/plans/terrain_editor.md` (the entities: no tree, viewport picking); `docs/plans/save_system.md` (step 5's
   Outliner context entry, the open point answered). `docs/performance.md` keeps its history.
9. **Scripts and gates.** Go through the `scripts/*.txt` that open more than one file of a tool (found so far: `weather_smoke.txt`;
   `cross_tool.txt` opens a tile and an environment, which is fine; `tile_extracted.txt` reopens the same tile, which switches to it) and
   the ones using `activate`, `layer_visible`, `blend` or counting documents (`assert_documents`). Run the gates and the scripts touched
   (`--filter` sized, per the usual rule) once all steps are in.

## Found on the way

- **The active document falls back to the tool's stand-in.** In the Weather tool with no file open, the sample map is what History,
  undo and the gizmo's edits work on (`app_smoke.txt` edits it): `DocumentManager::active_id` is the tool's document, else its stand-in.
- **The Weather tool lit VFX files with the environment open next to them** (the particle overlay reads the open `vfx/` and
  `weather_definitions.xml` documents). With one file, opening a VFX closes the environment: the last environment goes on lighting the
  scene, as the packs (or the project) have it, while the tool's file is not an environment; closing the file goes back to the default
  light (`WeatherTool::last_env`).
- **The replace prompt runs the open again** after its Save or Discard (`Editor::clear_to_replace` takes the open as a closure). The
  tools whose open is more than `open_path` ask themselves before they start: the Tile tool's `open_tile` (the entry it reads is its
  state during the call only), the Prefab tool's `open_prefab`, the Terrain tool's `start_open` and `open_now` (the map is read on a
  worker: asked before the read; `adopt` asks again if the open map was edited during the read).
- **The Prefab tool's stage is made again in its folder**, so the old stage closes once the prefab is loaded, just before the new stage
  is made, not after it is open.
- `assert_modified` and `assert_history` take an optional document (`cross_tool.txt` checked each document through `activate`).
- `tile_copy_paste.txt` and `tile_roundtrip.txt` open another tile with unsaved changes: they `close tile discard` first.

## Open points

- **What a stand-in is**, for a tool other than the Weather tool. Only the sample map today. If a tool later needs a second document of
  its own (a reference tile next to the edited one, say), it should be a stand-in or a view, never a second editable document.
