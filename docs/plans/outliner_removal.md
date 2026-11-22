# One document per tool, no Outliner

The editor held any number of documents of any tool at once (`DocumentManager`: a list, one of them active), and the shared **Outliner**
panel listed them all, each with its tree. This plan makes it **one document per tool**: opening a tile closes the open tile, opening a
weather file closes the open weather file, and so on. Documents of *different* tools still live side by side (a tile and a weather file;
a tile and the Prefab tool's stage). The Outliner goes with it: what it did for documents moves to the File menu and the menu bar, and
its trees are dropped.

State (2026-11-22): planned 2026-11-08 with the user, steps 1 to 9 done the same day, in master (how it works now: [editor.md,
Cross-tool composition](../editor.md#cross-tool-composition-phase-7)). It closes the save system plan's open point ("several tiles drawn
at once": not wanted). Left: the open point at the end.

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

## As built (2026-11-08)

- `DocumentManager::of_tool` replaced the manager's own active document; `add` refuses a second non-stand-in document of a tool. The
  active document is the active tool's, else its stand-in.
- Every open goes through the editor's replace (`Editor::clear_to_replace`, the Save / Discard / Cancel prompt); scripts: `open <path>
  [save|discard]`, `open_prompt`, `answer`. The tools' own opens ask before they start (the Tile tool's `open_tile`, the Prefab tool's
  `open_prefab`, the Terrain tool's `start_open`, `open_now` and `adopt`).
- The tools' "which one" rules, the `set_active` calls, the cross-fade (`blend_doc` / `blend_t`, the `blend` command), the Outliner panel,
  the `layer_visible` and `activate` commands, and the trees (`OutlinerNode`, `Document::outline` and its cache) are gone. The building
  pieces' code stays, reached through `tile_inspect <elem> [piece]` and `tile_edit ... piece <n>`. `assert_layer_visible`, and
  `assert_modified` / `assert_history` with an optional document, are the scripts' checks.
- The menu bar's `[Tool] title *` label and the OS window title (`WCE - [Tile] aa *`), amber while unsaved (`theme::modified`).
- Docs and scripts updated (`weather_smoke.txt`, `cross_tool.txt`, `tile_copy_paste.txt`, `tile_roundtrip.txt`).

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
