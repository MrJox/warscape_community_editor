# The editor shell (`libs/editor`, `app/`)

`wce.exe` is one window with one renderer and one shared scene; tools are modes of it. Phase 4 built the shell, Phase 5 moved the Weather tool onto it
(`tools/weather`, see below); the Tile tool follows in Phase 6.

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

Registered by `app/main.cpp` through `tools/tool_factories.h`: Weather (Phase 5), Tile, Unit, VFX, Terrain, Cinematic (placeholders with a
panel that says what is planned). **Ctrl+1..9** switch tools.

To add a tool: derive from `Tool`, give it an id and title, return the file extensions it opens, implement `open` (return a `Document`), add panels in
`draw_panels`, commands in `register_commands`; add a factory to `tool_factories.h` and register it in `main.cpp`.

## Weather tool (`tools/weather`)

Opens `.environment`, `.lighting` and `.xml` (weather definitions, vfx) from the packs or from disk (File > Open file, `--open`, `--open-disk`, double-click in Files). Each file is an
`XmlFileDocument` around the lossless `XmlDoc`: attribute edits are `XmlAttrCommand`s (merge per drag gesture), structural edits (duplicate, delete, move, add child or attribute,
remove attribute) are snapshot commands. Save writes a loose file under the output folder (File > Set output folder, `output_dir`; the game's `data` folder is never touched), re-reads it as
the new baseline and clears the history, so the saved bytes differ from the pack file in the edited spans only.

| Legacy feature (`legacy/weather_editor/current-state.md`) | Where it is now |
|---|---|
| Files browser with filter and categories | **Files** panel (and the shared Assets panel, filtered to the tool's types) |
| Properties tree with typed editors, search, per-attribute revert, add / remove | **Properties** panel (tree) and the shared **Inspector** (typed editors for bool, int, float, colour, variance, euler, string) |
| Structural edits with snapshot undo | context menu in Properties; `xml_op`; Ctrl+Z / Ctrl+Y, History panel |
| Changes tab | **Changes** tab of Properties (every modified attribute of the file, revert) |
| Live preview on the sample map, orbit camera, battle lens | shared viewport; the active environment document lights `RenderScene::environment` every frame (a default dry environment when none is open) |
| Cross-fade between two environments | **Weather scene** bar and `blend <t>` |
| View modes (HDR, albedo, normal, depth, ...) | **Weather scene** bar and `view <n>` |
| Sample map seed and count, re-roll | **Weather scene** bar, `sample`, `reroll` |
| Graphics options (gamma, brightness, SSAO, DOF, shadows, vignette, flare) | **Settings** menu (read from `preferences.script.txt`) |
| Weather selection database: Pick and Edit tabs, `.xml` and `.bin` | **Selection > Weather picker** (`WeatherPicker`, `VariationEditor`), `picker`, `pick k=v`, `vdb-script` |
| CLI `--open`, `--open-disk`, `--cam`, `--frames`, `--shot`, `--capture`, `--view`, `--blend`, `--pick`, `--picker`, `--vdb-script`, `--no-flare` | same flags of `wce.exe`; the weather ones are commands of the tool (`--name args` runs the command `name`) |

Differences: the legacy app held one active file; here every open file is a document, and the one that lights the scene is the last active `.environment` / `.lighting`. `weather_editor.exe` is still built from `legacy/weather_editor`
(window shots stay its render regression) until Phase 8; it shares the sample map, XML and picker code with the tool. `scripts/weather_check.sh` supersedes `variation_e2e.ps1` (which only knew the legacy build folder).

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
- `scripts/weather_check.sh` (render, `weather_tool_check` in `gates.sh`): `scripts/weather_smoke.txt` twice in throw-away folders: open, attribute edit with undo and redo, structural edits, save (exactly one line differs
  from the pack file, both runs save the same bytes), a second environment and cross-fade, view modes, the weather picker, a database edit saved as XML and FASTBIN (they convert into each other byte for byte), an offscreen capture.
- `scripts/shots.sh`: the weather shots (legacy app) and the `wce.exe --capture` references, byte for byte.

Scripted runs (`--fixed-step` / `--shot` / `--capture` / `--script`) ignore the real mouse and keys never auto-repeat, and an offscreen capture restarts the renderer's temporal state
(auto exposure, frame counter), so a capture is a function of the scene only.

## GPU

`libs/render/gpu_adapter` chooses the D3D11 adapter: `WCE_GPU=<part of the adapter name>`, otherwise the high-performance GPU, and the renderer logs `GPU: <name>` when it starts. Two GPUs do not round float math the same way,
so reference pixels only compare on one adapter (the gate scripts default `WCE_GPU` to `AMD Radeon`, see `docs/baseline.md`).
