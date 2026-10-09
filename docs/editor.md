# The editor shell (`libs/editor`, `app/`)

`wce.exe` is one window with one renderer and one shared scene; tools are modes of it. Phase 4 built the shell; the Weather and Tile tools move
onto it in Phases 5 and 6. Until then `tools/weather` hosts a sandbox document (the sample map) that exercises every part of the shell.

## Pieces

| Piece | File | What it does |
|---|---|---|
| `Command`, `CommandStack` | `editor/command.h` | undo/redo per document; a drag gesture pushes many commands with one merge key and ends as one entry (`break_merge` when the gesture ends); `modified()` follows the saved position |
| `Document` | `editor/document.h` | one thing a tool edits, with its lossless model inside: `publish` into its scene layer, `outline`, `inspect`, `item_transform` / `transform_items` for the gizmo, `save` |
| `DocumentManager` | `editor/document_manager.h` | owns the open documents, one `RenderScene` layer each, re-publishes a document when its stack moved |
| `Selection` | `editor/selection.h` | a set of `SourceRef` (document, item); the scene entities that show them come from `RenderScene::entities_of` |
| `Inspector` | `editor/inspector.h` | the documents describe an item through this interface; every property has a `PropertyPath` (document, item, field), the seam the Cinematic editor animates through; `panels.cpp` has the ImGui implementation |
| `CommandRegistry`, `ScriptRunner` | `editor/command_registry.h` | every action is a named command; menus, `--cmd`, `--script` and tests all run the same entries |
| `OrbitCamera`, `FlyCamera` | `editor/camera_controller.h` | viewport cameras on the game's battle lens (FOV 65 degrees, near 1, far 13000) |
| `Viewport` | `editor/editor.h`, `viewport.cpp` | renders the scene into a `RenderTarget`, camera input, click to select (through `Renderer::pick`), the ImGuizmo gizmo (translate/rotate/scale, snap), offscreen `capture` |
| panels | `editor/panels.cpp` | Outliner (per document, visibility), Inspector, Assets (the VFS tree, filter, only the tool's file types, double-click opens), Log, History (click to jump), Commands (console) |
| `Editor` | `editor/editor.h` | the host window with menu bar, tool switcher and dockspace; owns VFS, renderer, scene, documents, selection, clock; layouts; first run |
| `Tool` | `editor/tool.h` | a mode: opens, panels, menus, viewport input, default layout, commands; `PlaceholderTool` for the future ones |

`libs/editor` is two targets: `wce_editor_core` has no ImGui or GPU code and is what `cli/editor_check` tests; `wce_editor` is the UI.

## Tools

Registered by `app/main.cpp` through `tools/tool_factories.h`: Weather (sample map sandbox), Tile (placeholder), Unit, VFX, Terrain, Cinematic (placeholders with a
panel that says what is planned). **Ctrl+1..9** switch tools.

To add a tool: derive from `Tool`, give it an id and title, return the file extensions it opens, implement `open` (return a `Document`), add panels in
`draw_panels`, commands in `register_commands`; add a factory to `tool_factories.h` and register it in `main.cpp`.

## Layouts

ImGui runs with docking and no ini file. Each tool has its own layout file `<settings folder>/layouts/<tool id>.ini` (the settings folder is
`%LOCALAPPDATA%\awe` or `$WCE_SETTINGS_DIR`). Switching tools saves the leaving tool's layout and loads the entering one's; without a file the tool's `default_layout`
(DockBuilder calls; the base class builds the standard one: outliner left, inspector right, assets/log/history below, viewport in the centre) is built. `layout save`,
`layout reset`, File menu entries and saving on exit (not in batch runs) manage the files.

## Settings and first run

`wce.ini` (see `docs/vfs.md`). When the game folder is not set or does not exist the editor starts without the game and shows the first-run dialog: it fills in the
Steam library's Attila folder (registry `SteamPath` + `libraryfolders.vdf`) and a nearby RPFM folder, lets you browse, and **Save and continue** (command `setup`) writes `wce.ini`,
mounts and starts the active tool.

## Command line

```
wce.exe [--tool <id>] [--open <path> | --open-disk <file>] [--cam yaw pitch dist [target_y]] [--cmd "<command>"]... [--script <file>]
        [--frames N] [--shot <file.png>] [--capture WxH <file.png>] [--fixed-step <seconds>]
```

Everything becomes a script: `--tool` and `--open` first, then `--cmd` / `--script` in order, then `frames N` + `shot` (the whole window after N rendered frames) or
`capture` (the scene offscreen, no window involved), then `quit`. Shots, captures and scripts run on the fixed-step clock (1/60 s per rendered frame), so the same
script gives the same pixels. Exit code 1 when a script line fails.

Script format: one command per line, `#` comments, double quotes group, a leading `-` lets a line fail, `frames N` waits N rendered frames. `help` in the Commands panel (or as a command)
lists them all. Test commands (`assert_*`) fail the script when they do not hold; `key Ctrl+2`, `click_item 13`, `move 3 5 0 2` drive the same code paths as the keyboard,
mouse and gizmo.

## Gizmo and ImGuizmo

ImGuizmo wants row-vector matrices (v * M, row-major); the engine's are column-vector (M * v), so the viewport passes transposes (`to_gizmo` / `from_gizmo` in `viewport.cpp`). The gizmo moves the
first selected item's pivot (`Document::item_transform`); the change since the last frame, `new * inverse(old)`, goes to `Document::transform_items` for every selected item with a gesture id,
so a whole drag is one undo entry. Scale is accepted by the gizmo but a document may ignore it (the sample map does).

## Gates

- `editor_check` (fast): command stack, documents and layers, selection, registry and script runner, cameras, inspector gestures, property paths. No GPU.
- `scripts/app_check.sh` (render): `wce.exe` runs `scripts/app_smoke.txt` twice in throw-away settings folders (tools, documents, selection by click, gizmo edit and undo, shortcuts, layouts, two
  documents, the asset browser, a whole-window shot and an offscreen capture; the shot and the capture must be identical between the runs) and `scripts/app_firstrun.txt` once without any settings.
- `scripts/shots.sh`: the weather shots (legacy app) and the `wce.exe --capture` references, byte for byte.
