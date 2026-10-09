# The editor shell (`libs/editor`, `app/`)

`wce.exe` is one window with one renderer and one shared scene; tools are modes of it. Phase 4 built the shell, Phase 5 moved the Weather tool onto it
(`tools/weather`, see below), Phase 6 the Tile tool, Phase 7 lets the tools share documents (see Cross-tool composition).

## Pieces

| Piece | File | What it does |
|---|---|---|
| `Command`, `CommandStack` | `editor/command.h` | undo/redo per document; a drag gesture pushes many commands with one merge key and ends as one entry (`break_merge` when the gesture ends); `modified()` follows the saved position |
| `Document` | `editor/document.h` | one thing a tool edits, with its lossless model inside: `publish` into its scene layer, `outline`, `inspect`, `item_transform` / `transform_items` for the gizmo, `save` |
| `DocumentManager` | `editor/document_manager.h` | owns the open documents, one `RenderScene` layer each, re-publishes a document when its stack moved; `unsaved`, `save_one`, `save_all`, layer visibility, the stand-in scene rule |
| `Selection` | `editor/selection.h` | a set of `SourceRef` (document, item); the scene entities that show them come from `RenderScene::entities_of` |
| `Inspector` | `editor/inspector.h` | the documents describe an item through this interface; every property has a `PropertyPath` (document, item, field), the seam the Cinematic editor animates through; `panels.cpp` has the ImGui implementation |
| `CommandRegistry`, `ScriptRunner` | `editor/command_registry.h` | every action is a named command; menus, `--cmd`, `--script` and tests all run the same entries |
| `OrbitCamera`, `FlyCamera` | `editor/camera_controller.h` | viewport cameras on the game's battle lens (FOV 65 degrees, near 1, far 13000) |
| `Viewport` | `editor/editor.h`, `viewport.cpp` | renders the scene into a `RenderTarget`, camera input, click to select (through `Renderer::pick`), the ImGuizmo gizmo (translate/rotate/scale, snap), offscreen `capture` |
| panels | `editor/panels.cpp` | Outliner (per document: tool tag, eye, `*`, context menu, Save all), Inspector, Assets (the VFS tree, filter, only the tool's file types, double-click opens), Log, History (click to jump), Commands (console) |
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

| Old weather_editor feature (`docs/plans/weather_current_state.md`) | Where it is now |
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

Differences: the legacy app held one active file; here every open file is a document, and the one that lights the scene is the last active `.environment` / `.lighting`. `weather_editor.exe` was removed in Phase 8; the render regression is the `wce.exe` captures (`scripts/shots.sh`). `scripts/weather_check.sh` supersedes `variation_e2e.ps1`.

## Tile tool (`tools/tile`)

A tile's root is its tile database entry (`_tile_database/TILES/*.xml`, [formats/tile_database.md](formats/tile_database.md)): it names the folder of `tile.ted` (location + variation) and holds the size, colour, ground groups, height / normal parameters and links. Open one with the **Tiles** panel (the kits' entries from an index, no scan; word filter; double-click opens the first variation, a button per variation), Tile > Open tile... (an entry `.xml`, a `tile.ted`), `--tile <target>` or the `tile <target> [variation]` command, where the target is an entry file, a name (`assembly_kit/osgiliath_test`, or the entry's file name), a variation folder, a tile folder (opens its first variation) or a `tile.ted`. A folder or `tile.ted` resolves its entry from the path; outside a kit there is none and the tile opens with a warning and no height scale. The **Tile** panel shows the entry read-only (colour, size, flags, ground groups, a variation chooser that opens the other variation as its own document, the variations' height / normal / overlap values, links and targets). A `TileDocument` wraps a `TileProject` (the lossless `TedScene` and the `HeightField`), and everything it does is a `Command` on the document's stack:
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
| weather picker (which environment the game picks for the map) | **Weather / climate** section of the Tile panel; a double-click on a candidate opens that file and previews it on the tile (Cross-tool composition) |
| inspector: position, euler, Drop to terrain, attributes | shared Inspector (position in scene units, euler in degrees, attributes read-only), Drop to terrain in the panel |
| wireframe terrain | not ported (the renderer has no wireframe mode) |
| script flags `--tile --paint --select --move --duplicate --delete --rotate --add --add-polygon --polygon-point --new-layer --move-to-layer --save-to --undo --redo --cam --shot --frames` | the same flags of `wce.exe` (a flag is the command of that name: `tile`, `paint`, `tile_select`, `tile_move`, `duplicate`, `delete_item`, `rotate`, `add_building`, `add_polygon`, `polygon_point`, `new_layer`, `move_to_layer`, `save_to`, `undo [n]`, `redo [n]`); `--cam yaw pitch dist` uses the shared orbit camera's convention (target = tile centre) |

Entry commands: `tiles [words]` (the browser's rows), `tile_entry` (log the entry), `assert_entry <field> <value>`, `assert_tiles <n> [words]`; test helpers `gizmo_drag <x|y|z> <pixels>` and `cam_info`. Extra commands: `tile_mode`, `brush`, `grab_to x z`, `tile_click`, `tile_click_ground`, `tile_drag_point`, `draw_polygon`, `tile_diagnostics` and the tests `assert_instances`, `assert_type_count`, `assert_world`, `assert_world_xz`, `assert_height`, `mark_height` / `assert_height_delta`, `assert_selected_type`,
`assert_at_cursor`, `assert_mode`, `assert_grabbing`, `assert_file_exists`. `mouse move|click|drag|wheel|off x y` (pixels or `50%` of the viewport image) drives the same ImGui events as the real mouse; in fixed-step runs the real mouse is ignored.

`tile_studio.exe` (and its own renderer) was removed in Phase 8; the tile captures of `scripts/shots.sh` are the render regression.

## Cross-tool composition (Phase 7)

Every open file is a `Document` in the one `DocumentManager`, whichever tool opened it: a tile and an `.environment` (and vfx files) are open at once, each with its own history, modified marker and layer. Opening a file of another tool
switches to that tool (`open_path(..., switch_tool = false)` does not); `activate <id|kind|title>` and a click on a document in the Outliner choose which one the menus, History and `save` act on; a double-click there (or *Edit in its tool*) switches to its tool.

| What | How |
|---|---|
| Preview this weather on this tile | An `.environment` / `.lighting` document says what it would light the scene with (`Document::environment`). `Editor::set_weather_preview(id)` (command `preview_weather <doc\|none>`, Outliner context menu, the combo in the Tile panel's Weather section, a double-click on a candidate of the tile's weather picker) lights the scene of every tool but the weather tool with it, live while the file is edited (`Editor::update_lighting`, after the tool's `update`). Without a preview the scene has the default light as before. The weather tool keeps lighting its own scene with the last active weather file (`Tool::lights_scene`). The preview ends when the document closes. |
| Stand-in scene | The weather tool's sample map is `stand_in()`; a tile is `shows_scene()`. The document manager hides the stand-in when the first tile opens and shows it again when the last one closes (your own eye toggles in between stay). The weather tool does not move the camera or make a sample map while a tile is open. |
| Save All | File > Save all (Ctrl+Shift+S; the count of unsaved documents is in the label), the Outliner button, `save_all`: every modified document is saved in place by its own `save` (the weather file as a loose file under the output folder, the tile as `tile.ted` / `height_map_0.png` with a one-time `.bak`), each only in the edited spans. A failing document is reported and the others are still saved. |
| Dirty markers | `*` after the title and the amber colour in the Outliner, in the File menu's Save all count and in the unsaved-changes prompt. |
| Close prompts | File > Close document / Close all documents, the Outliner's *Close*, File > Quit and the window's close button ask when something is unsaved: **Save** (and close), **Discard** (close without saving), **Cancel**. If a save fails nothing closes and the prompt says why. Scripts: `close [doc] save\|discard`, `close_all save\|discard` (they fail with unsaved changes and no answer), `close_prompt`, `quit_prompt` open the prompt like the menu and `answer save\|discard\|cancel` answers it; `quit` leaves at once, as scripts expect. |
| Outliner | The documents of all tools are listed together: `[Tool] title *`, the checkbox is the document's layer visibility (`layer_visible <doc> 0\|1`), the context menu has Save, Close, Edit in its tool, Preview / Stop previewing this weather, Show only this document, Show all documents. |
| Test commands | `expect_fail <command...>`, `documents`, `activate`, `assert_preview`, `assert_prompt none\|close\|quit [N]`, `assert_layer_visible`. |

Gate: `scripts/cross_tool_check.sh` (`cross_tool_check`): a copy of a kit tile and `med_sp_land_day_dry.environment` open at once, the weather previewed on the tile (the capture differs, follows an edit, equals the default light again with `preview_weather none` and after the document closed), one edit in each, the prompts
(close and quit, cancel, discard), Save All, and then `scripts/span_diff.py` shows that the saved `.environment` differs from the pack file in exactly the one value, `tile.ted` in exactly one span inside one tag, every other file of the tile is untouched and the `.bak` is the original; two runs write the same bytes and captures.

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
        [--frames N] [--shot <file.png>] [--capture WxH <file.png>] [--fixed-step <seconds>] [--live] [--background | --foreground]
```

`--shot`, `--capture` and `--script` runs are unattended: their window is created non-activating, kept out of the taskbar and Alt-Tab and put behind the other windows, so a test run never takes the focus from a game or an editor in use. `--foreground` (or `WCE_FOREGROUND=1`) brings back
the normal window, `--background` (`WCE_FOREGROUND=0`) makes an interactive run quiet too.

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
- `tile_entry_check` (fast; `--all` in the corpus run, `scripts/gates.sh --corpus`): every `TILES/*.xml` of both kits round-trips through `TileEntry` byte for byte with no unknown name, the `TileIndex` (plain, cold cache, warm cache) agrees with itself and resolves every variation folder, a few kit tiles (all of them with `--all`) open identically by folder, by `tile.ted` and by entry, a `tile.ted` outside a kit opens with a warning and no entry. `scripts/tile_entry_run.sh` (render, `tile_entry_script`): `scripts/tile_entry.txt` opens tiles by name, entry file, folder and `tile.ted` and asserts the entry's fields. `placement_check` (corpus): the tool's building positions and rotations against TEd's `building_list.xml` over the kit (Phase 0 of `docs/plans/tile_ted_parity.md`); `scripts/tile_repro.sh` runs the Phase 0 repro scripts.
- `scripts/cross_tool_check.sh` (render, `cross_tool_check` in `gates.sh`): see Cross-tool composition. `editor_check` also covers the document manager's unsaved list, `save_one` and the stand-in scene rule.
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
