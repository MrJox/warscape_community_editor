# The Prefab tool

A tool of its own for editing prefabs, as the Weather tool is for weather: a stripped-down Tile tool that opens a prefab instead of a tile. Its
prefabs are the editor's (`<key>.wce` in the projects' `art/prefabs`), and they gain one thing TEd's lack: a **height layer**. Placed in a
tile, a prefab's height layer shows in the reference's own settings (the Inspector), not in the tile's Terrain layers list. The goal is
prefabs that stop needing hand-made meshes that fake the terrain.

State: planned 2026-11-07, decisions taken with the user (the list at the end). Steps 1 to 7 done (2026-11-07/08): the tool works behind the `prefab` feature; step 8 and the follow-ups below are open.

## Why

Today a prefab is edited inside a tile (`wce_tile_tool::prefab_panel`): "Edit prefab" opens one copy, Apply writes the file and replays the
contents into the other copies **of that tile only**, Cancel puts the file's contents back. Problems:

- The edit happens over whatever ground, weather and neighbours the tile has, mixed into its undo history and its scene tree.
- Other open tiles see the change only when they next fill their references in.
- A prefab has no terrain, so prefabs that need a platform, a mound or a dip ship meshes that pretend to be ground. They don't blend with
  the tile's terrain.

## What the code has to build on

- **Prefab files** (`wce_tile::prefabs`): `PrefabLibrary` scans the projects' prefab folder first, then the kits'; a `.wce` takes the place
  of the `.xml` of the same key. `save_edit` writes the `.wce`, `save_for_kit` writes TEd's `.xml` (references baked, props as models).
- **References** (`wce_tile::prefab_refs`, `project_files`): a group with `wce_prefab="<key>"` keeps empty `children` in tile.wce and is
  filled in on open; per-copy edits are `WceOverrides`. Structural edits inside a reference are refused.
- **Terrain layers** (`wce_tile::terrain_layers`): a `Stack` per map type beside each field; the field holds the composite, so the renderer,
  Save for Assembly Kit and the game export see the result without knowing about layers. Height layers add. Layers have an **`owner`**: a
  procedural feature's layers are not listed in the Tile panel, not edited by hand, and apply after the user's.
- **Procedural roads** (`tile_document/roads.rs`) are the model to copy: each `WceRoadSpline` owns a layer per type, owner
  `road:<id>#<signature>`; `refresh_roads` remakes the layers of roads whose signature changed and drops those of roads that went, so undo
  needs nothing of its own and opening a tile doesn't remake them. A road's height layer is made against what lies below it
  (`Stack::below`): a target height turned into an additive layer.
- **New tile** (`wce_tile::new_tile`): writes a flat tile project (entry, tile.wce, the three maps) at a size and density: what a stage
  needs.
- **Tools** (`wce_editor::tool::Tool`, `wce_app::tools`): each tool behind a Cargo feature; `e.with_tool::<T>()` finds a tool by type.

## Design

### A prefab's height layer

- **Heights only.** No texture or ground type layers: those would mean different things in each tile (its texture groups) and give
  unintended visuals.
- Stored **inside the `.wce`**, one file per prefab (easy to share in the prefab repo): a `WceTerrain` element after the group holding the
  patch, deflated and base64'd, with its frame as attributes: origin relative to the prefab group's origin (metres), metres per pixel, size,
  the flags below. TEd never sees it (`project_files::to_ted` drops it, with a report line).
- A patch is heights relative to the stage's plain (0 = untouched) plus a **coverage** per pixel (0..1): how much the prefab's sculpting
  touched it. Coverage comes from the brushes' weights on the stage; it is what "normalize ground" flattens. Cropped to where coverage
  is non-zero on save.
- **`normalize ground`** (a flag of the layer, saved with it):
  - **Off**: the patch adds to whatever ground is under the prefab (a mound, a ditch).
  - **On**: the prefab was made on plain ground, so where it covers, the tile's ground is taken as plain first and the patch applies over
    that plain: the result looks exactly as on the stage. The plain is at the **tile's ground height under the prefab's origin**
    (sampled again when it moves). The area is the **covered pixels**, blended into the tile's ground by an adjustable **edge feather**
    (metres, saved with the layer; the reference can override it).
  - In layer terms, on: `target = plain + patch`, `applied = lerp(0, target - below, coverage x feather)`, made against what lies below as
    roads do; off: `applied = patch`. Either way it is an ordinary additive layer of the tile's stack, so nothing downstream changes.
- A prefab without a `WceTerrain` is exactly today's prefab. A kit `.xml` prefab never has terrain.

### A prefab placed in a tile

- A reference whose prefab (or a prefab nested in it) has a height layer owns one height layer, owner `prefab:<n>#<signature>` (n: its
  place among them). No id on the reference: the layers are derived data, known again by their signatures, which hash the prefab's layer,
  the reference's position and yaw, its settings and, normalized, the ground below its layer over what it reaches. `refresh_prefab_ground`
  (`tile_document/prefab_ground.rs`) runs before `refresh_roads`, looks again only when the history, the structure, an item or the height
  map changed, and remakes them all when any differs: nested ones first (an element ends after what is in it), then the scene's order, all
  over the user's layers and under the roads' (a road is graded over a prefab's platform; the roads are marked stale).
- The patch is rotated by the group's yaw (pitch and roll ignored for terrain) and resampled bilinearly to the tile's height grid. Nested
  references compose into the outer reference's layer, each with its own flag.
- Items inside the prefab that stand on the terrain (Landscape: Above) follow the new ground, as they already follow the tile's.
- **Inspector, Prefab section** of a selected reference: its height layer (on/off, opacity, feather override, normalize shown read-only
  from the prefab), "Open in Prefab tool", and the existing overrides. Never listed in the Tile panel's Terrain layers (already true for
  owned layers).
- **No sculpting over a prefab's layer in a tile**: tile brushes edit the user's layers or the base, which still add on top. To change a
  prefab's ground: the Prefab tool, turning its layer off on the reference, or unpacking.
- **Unpack prefab** turns the reference's owned layer into a user layer named after the prefab (the ground stays as it was).
- **Save for Assembly Kit** of a tile: nothing new, the composite height map carries it. **Save prefab for Assembly Kit**: TEd's prefabs
  can't hold terrain; the report says it was dropped.

### The tool

- **Crate** `wce_prefab_tool`, Cargo feature `prefab` (off by default until it works, then in the release set with weather, tile, ui).
  Opens `.wce` / `.xml` prefabs; listed in the switcher after Tile.
- **Stage**: a flat tile the tool makes itself (`new_tile`), outside the user's projects (`<projects>/.wce/prefab_stage/<settings>`), never
  saved as a tile. Settings: size (default 2 x 2 cells, 256 m), density (128: 1 m a pixel), base height, climate and tile set (textures,
  weather chips), a grid and an origin marker. A real tile's ground as the base is for later.
- **The document** is the stage's `TileDocument` with one group at the origin, the prefab's group; the scene tree is rooted at it. The
  height brushes write into one layer that is the prefab's (and record coverage); the stage's base is locked; texture and ground type
  brushes are not offered. Undo is the tile document's. The "normalize ground" flag and the feather are on the Prefab properties page, and
  the stage can preview the feather's edge.
- **Reused from the Tile tool**: viewport, gizmo and snapping, Scene tree, Inspector, Create panel and the quick-add popup (Space), the height brushes, the
  weather chips, camera modes, unit references for scale. **Left out**: paint and ground type brushes, tile properties, minimap, battle map
  context, deployment, siege AI, roads, rivers, launch in game, tile database, the Tiles browser.
- **Panels**: Prefabs (the library: folders, search, thumbnails, kit/project badge; New, Duplicate, Rename, Delete for project prefabs),
  Scene, Create, Inspector (with a Prefab properties page: key, folder, the height layer's flag, feather, size, "Crop"), Stage.
- **Files**: Save writes the `.wce` (the first save of a kit prefab makes the project copy, as `save_edit` does), Save as (new key/folder),
  Save for Assembly Kit (with its report), Revert. Unsaved work is reported to the editor as tiles' is.
- **Reuse mechanics**: the Tile tool's panels are `impl TileTool`, and its commands find it with `with_tool::<TileTool>`. The Prefab tool
  holds its own `TileTool` built with a **prefab profile** (a flag the panels and menus check to hide what the list above leaves out), and
  `with_tile` resolves to the active tool's `TileTool`. Cheaper and less risky than splitting the Tile tool into components; the profile
  checks are the cost.

### The Tile tool afterwards

- "Edit prefab" / Apply / Cancel go away; a reference's context menu and Inspector get **"Open in Prefab tool"** (switches tool, opens the
  prefab). Overrides stay as they are.
- A save in the Prefab tool reaches every open tile: the library and the contents cache (`PrefabBase`) are refreshed, references filled in
  again with their overrides, owned layers remade (their signatures change by themselves). Closed tiles pick it up on open, as now.

## Steps

Each a commit or a few, on `feature/prefab_tool`.

1. **Height patch format** (`wce_tile::prefab_terrain`): heights + coverage, frame, flag, feather, crop; the `WceTerrain` element read and
   written in the `.wce`; `to_ted` drops it with a report line. Unit tests: round trip, a prefab without terrain byte-identical.
2. **Applying a patch** (`wce_tile`): rotate and resample into a height stack, normalize off and on (plain at the origin, coverage x
   feather), nested composition. Unit tests on small synthetic fields (a 90 degree yaw lands exactly; a normalized platform on a slope
   comes out as on the stage; off adds).
3. **References own layers** (`wce_tile_editor`): `refresh_prefab_ground` before the roads, remade on move/rotate, on ground changes
   under a normalized one and on library change, undo through signatures; per-reference settings as `wce_terrain*` attributes in
   tile.wce. Tests in `tests/tile_document.rs`.
4. **Inspector Prefab section and Unpack** in the Tile tool; Save prefab for Assembly Kit report.
5. **Tool shell**: crate, feature, registration, layout, the stage tile, the Prefabs panel, open/new/save/save as/revert, unsaved work.
6. **Prefab profile** of the Tile tool: scene tree rooted at the prefab, height brushes into the prefab's layer with coverage, base locked,
   what is left out hidden, `with_tile` routing, menus, the flag and feather on the properties page.
7. **Tile tool integration**: Open in Prefab tool, Edit/Apply/Cancel removed, live refresh of open tiles after a prefab save.
8. **Stage settings and polish**: climate/tile set, weather chips, size, grid, feather preview, thumbnails of the prefab with its terrain.

Checks: unit tests per step; a `--script` run (open a prefab, sculpt a platform, normalize on, save, place it in a scratch copy of a tile on
a slope, move and rotate it, Save for Assembly Kit) with a viewport capture, non-activating, ending with `quit`. The user checks in the
GUI and in game (a prefab with a normalized platform under a building, units walking on it).

## Decisions (2026-11-07, with the user)

1. Stage ground: a flat stage only for now; a real tile's ground as the base later.
2. Heights only: no texture or ground type layers in prefabs.
3. Height behaviour: the `normalize ground` flag. Off adds; on flattens the covered area to a plain at the tile's ground under the
   prefab's origin, with an edge feather, then applies the patch, so it looks as on the stage. No separate partial "Level" mode.
4. In-tile prefab editing (Edit / Apply / Cancel) is replaced by Open in Prefab tool; overrides stay.
5. No capturing of a tile's heights into a prefab on Save as prefab.
6. Storage: inside the `.wce` (`WceTerrain`), one file per prefab.
7. No sculpting over a prefab's layer in a tile.
