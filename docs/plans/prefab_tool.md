# The Prefab tool

A tool of its own for editing prefabs, as the Weather tool is for weather: a stripped-down Tile tool that opens a prefab instead of a tile. Its
prefabs are the editor's (`<key>.wce` in the projects' `art/prefabs`), and they gain one thing TEd's lack: a **height layer**. Placed in a
tile, a prefab's height layer shows in the reference's own settings (the Inspector), not in the tile's Terrain layers list. The goal is
prefabs that stop needing hand-made meshes that fake the terrain.

State (2026-11-22): planned 2026-11-07, decisions taken with the user (the list at the end); steps 1 to 8 and the follow-ups built
2026-11-07 / 08, in master; `prefab` is a default feature and in the `release` set. How it works now: docs/editor.md (the Prefab tool).
What is left: "Open" below.

## Why

Before, a prefab was edited inside a tile (Edit prefab / Apply / Cancel): over that tile's ground, weather and neighbours, mixed into its
undo history, and other open tiles saw the change only when they next filled their references in. And a prefab had no terrain, so
prefabs that need a platform, a mound or a dip shipped meshes pretending to be ground, which don't blend with the tile's terrain.

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

- `wce_tile_tool::prefab_tool` (planned as a crate of its own; `tool::with_tile` has to name it), Cargo feature `prefab`. Listed in the
  switcher before the Tile tool.
- **Stage**: a flat tile the tool makes itself (`wce_tile::prefab_stage`, as `new_tile` does), under `<projects>/.prefab_stage`, never
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

## As built (2026-11-07 / 08)

Steps 1 to 8 as designed above: `wce_tile::prefab_terrain` (the patch, `WceTerrain`), `wce_tile::prefab_stage`,
`tile_document/prefab_ground.rs` (`refresh_prefab_ground`), `tile_document/stage.rs`, `wce_tile_tool::prefab_tool`. Where it differs
from the design:
- The footprint is painted with the Ground brush (one ground type) and read back from the stage's ground type map; sculpted samples count
  as covered too. The flag and feather are attributes of the stage's group (`wce_stage_normalize`, `wce_stage_feather`): undoable, saved
  in the prefab's `WceTerrain`.
- The stage's settings (size, ground height, density) are in the Inspector under the root; climate and season in the viewport's ground
  chip, kept from one stage to the next. The prefab's group is right under the stage's root; the working layer on a stage is the group
  (or a layer inside it), never the root. A stage starts without grass (`grass on`).
- The stage is a tile placement (f55cf611): the group's origin on the stage's ground, its landscape parts snapped onto it; a save gives
  the file's height back to what is still where the stage put it (byte-identical).
- Outlines of the ground's cover (orange) and the feather's reach (yellow); Save as / Duplicate / Rename / Delete for the projects'
  prefabs (a rename leaves tiles' references to the old name as they are); nested prefabs' ground on a stage; a reference's ground
  following its opacity and feather while dragged.
- The Prefabs panel lists the library by folder as the Create panel does, as a grid of pictures since 2026-11-14. The in-tile Edit /
  Apply / Cancel is gone. The Prefab menu went in the menus' review (2026-11-14): File's Save, Revert and Close document do it.
- Later: prefab references keep their contents' exact values (landscape heights and drift saved as overrides, 2026-11-15); the
  Assembly Kits' prefabs can be ported to the projects (2026-11-16). Prefab stages are not autosaved (autosave.md).

## Open

- Thumbnails of prefabs with their ground (today they stand on the thumbnails' flat grey ground); smoother outlines (they follow the
  samples, in steps).
- The user's checks: the GUI flow, and in game a normalized platform under a building with units on it.
- A real tile's ground as the stage's base (decision 1: later).

## Decisions (2026-11-07, with the user)

1. Stage ground: a flat stage only for now; a real tile's ground as the base later.
2. Heights only: no texture or ground type layers in prefabs.
3. Height behaviour: the `normalize ground` flag. Off adds; on flattens the covered area to a plain at the tile's ground under the
   prefab's origin, with an edge feather, then applies the patch, so it looks as on the stage. No separate partial "Level" mode.
4. In-tile prefab editing (Edit / Apply / Cancel) is replaced by Open in Prefab tool; overrides stay.
5. No capturing of a tile's heights into a prefab on Save as prefab.
6. Storage: inside the `.wce` (`WceTerrain`), one file per prefab.
7. No sculpting over a prefab's layer in a tile.
