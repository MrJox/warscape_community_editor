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

## Tile tool (`tools/tile`)

Opens a tile folder (the one with `tile.ted`): File > Open file / Tile > Open tile... on a `tile.ted`, `--tile <folder>`, the `tile <folder>` command. A `TileDocument` wraps a `TileProject` (the lossless `TedScene` and the `HeightField`), and everything it does is a `Command` on the document's stack:
attribute edits on the scene (moves, rotations, polygon points) merge per gesture, structural edits (duplicate, delete, add building, polygons, layers) keep whole-document snapshots, and a brush stroke is one entry however many steps it took. Items are element indices + 1 (item 0 is the terrain), so the shared
outliner, selection, inspector, gizmo and history work on them. Save writes `tile.ted` and `height_map_0.png` in place (a one-time `.bak` of each), only what changed: the edited spans of `tile.ted`, the kit-exact PNG encoder for the height map; `save_to <folder>` writes both elsewhere.

What it shows: the terrain from the height map (patches of the shared terrain pass), BUILDING_INSTANCEs as their real models (`rigidmodels/buildings/<key>/` pieces; a key without a drawable model is a wire box and is listed in the log), every other instance as a small marker batched per type
with distance culling (vegetation 450 m, decals 300 m, others 3000 m, adjustable in the Tile menu), polygon outlines, the tile's rectangle, and highlights for the selection (box, axes, polygon point handles) and the brush ring. Where the height map sits is in `docs/formats/tile_ted.md`.

| Tile Studio (legacy) | Tile tool |
|---|---|
| RMB orbit, MMB pan, wheel zoom | the shared orbit / fly camera |
| LMB select; Raise / Lower / Smooth / Flatten / Level brushes | **Tile** panel: mode, radius, strength, falloff, flatten height; in a brush mode the left button paints (and does not orbit); Esc returns to Select |
| `G` grab on the ground, click confirms, Esc / right click cancels | same (`G`; grid snap in the panel); the gizmo steps aside while a grab or brush owns the button |
| Ctrl+Z / Ctrl+Y, Ctrl+S | shared (terrain strokes and instance edits share the document's one history) |
| `Ctrl+D` duplicate, `Del` delete | same; `R` rotate is now **Ctrl+R** (Ctrl+Shift+R reverses) because `R` is the shared scale-gizmo key; step in the panel |
| layers: right-click a layer (new layer, move selected here, rename, delete) | **Layers** panel (same context menus; per-type visibility checkboxes) next to the shared Outliner |
| building add by key | **Tile** panel: key + Add, `add_building <key> [x z]` |
| polygons / AI hints: draw, point handles, shift+click inserts, `Del` removes, hint combo, `area` recomputed | same (panel, mouse, Inspector's hint combo) |
| weather picker (which environment the game picks for the map) | **Weather / climate** section of the Tile panel (data only; Phase 7 lights the tile with the result) |
| inspector: position, euler, Drop to terrain, attributes | shared Inspector (position in scene units, euler in degrees, attributes read-only), Drop to terrain in the panel |
| wireframe terrain | not ported (the renderer has no wireframe mode) |
| script flags `--tile --paint --select --move --duplicate --delete --rotate --add --add-polygon --polygon-point --new-layer --move-to-layer --save-to --undo --redo --cam --shot --frames` | the same flags of `wce.exe` (a flag is the command of that name: `tile`, `paint`, `tile_select`, `tile_move`, `duplicate`, `delete_item`, `rotate`, `add_building`, `add_polygon`, `polygon_point`, `new_layer`, `move_to_layer`, `save_to`, `undo [n]`, `redo [n]`); `--cam yaw pitch dist` uses the shared orbit camera's convention (target = tile centre) |

Extra commands: `tile_mode`, `brush`, `grab_to x z`, `tile_click`, `tile_click_ground`, `tile_drag_point`, `draw_polygon`, `tile_diagnostics` and the tests `assert_instances`, `assert_type_count`, `assert_world`, `assert_world_xz`, `assert_height`, `mark_height` / `assert_height_delta`, `assert_selected_type`,
`assert_at_cursor`, `assert_mode`, `assert_grabbing`, `assert_file_exists`. `mouse move|click|drag|wheel|off x y` (pixels or `50%` of the viewport image) drives the same ImGui events as the real mouse; in fixed-step runs the real mouse is ignored.

`tile_studio.exe` stays built from `legacy/tile_editor` (own renderer, its shot is still a gate) until Phase 8.

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
- `scripts/tile_check.sh` (render, `tile_tool_check` in `gates.sh`): `scripts/tile_smoke.txt` twice (select / move / rotate / duplicate / delete / polygons / layers / brush, each with undo and redo, then a save; the saved files and the capture must be identical between the runs),
  `tile_mouse.txt` (click select, G grab, brush drag, polygon drawing with Enter, point handle drag, through the scripted mouse), `tile_roundtrip.txt` (every kind of edit, undo all, save: the files equal the originals byte for byte) and `tile_save.txt` (save in place keeps a `.bak`; undo and save
  again restores the bytes); and the saved tile.ted differs from the original in exactly the edited line, the saved height map only inside the brush.
- `perf_check` (fast, no window): thread pool, frustum / LOD / size culling of the draw set, the DDS mip stream plan, the tile document's element index against the plain transform walk on three tiles (77 633 instances on the biggest), the pick grid against a brute-force ray test. `xml_splice_check` (fast): the in-place structural edits of `XmlDoc` against the functional ones, undo and redo of their records exact.
- `scripts/shots.sh`: the weather shots (legacy app) and the `wce.exe --capture` references (identical, or within one level in under 0.05 % of the pixels; `build/gates_base_perf`).

Scripted runs (`--fixed-step` / `--shot` / `--capture` / `--script`) ignore the real mouse and keys never auto-repeat, and an offscreen capture restarts the renderer's temporal state
(auto exposure, frame counter), so a capture is a function of the scene only.

## Performance

How the editor stays fast on a 100 000 instance tile (instanced draws and culling, LODs, terrain patches, background loading, mip streaming, in-place structural edits), the measurements and the tools
(`perf`, `profile`, `gfx`, `wce.exe --live --script`, `scripts/perf/run.sh`) are in [performance.md](performance.md).

## GPU

`libs/render/gpu_adapter` chooses the D3D11 adapter: `WCE_GPU=<part of the adapter name>`, otherwise the high-performance GPU, and the renderer logs `GPU: <name>` when it starts. Two GPUs do not round float math the same way,
so reference pixels only compare on one adapter (the gate scripts default `WCE_GPU` to `AMD Radeon`, see `docs/baseline.md`).
