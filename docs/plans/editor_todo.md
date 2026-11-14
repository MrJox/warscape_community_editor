# Editor: to do later

Editor-wide ideas, noted for later (user, 2026-10-24). Status (2026-10-26): all three open.

## Minimap with views

Done (2026-10-25): the Tile tool's Minimap panel (docs/editor.md, "Minimap"): the colour, height or no base, with the siege AI, the deployment
zones and the camera as layers. Done (2026-10-28): the ground views unified (a chip on the viewport and one on the minimap, docs/editor.md,
"Ground"), every view on the minimap as its own base, the tile's cells and the automatic no-go areas as layers.

## Save: log the siege AI's problems

When a tile is saved or exported, write a short error to the log for each broken part of its siege AI. The checks exist in
`wce_siege::check` (`check`, `check_captures`), but today they run only inside the siege AI generation
(`wce_tile_editor` `tile_document/siege.rs`), not on save. They would run on `Tile::current_siege_layout()`:
- no fort perimeter;
- an edge's end in no node's rectangle;
- a node without a rectangle or Boundary;
- the street graph in parts;
- the main capture point in no area;
- walls, separators and no-go regions not closing the ring.

These are only log lines: the save still goes ahead. The natural place is next to the vegetation problems, which a save already puts at
the front of its message (docs/editor.md, "Problems").

## Autosave

Autosave of modified documents, from the old tile plan's cross-cutting items; nothing exists yet. To decide: where the copies go (never
over the user's file) and how they are offered back after a crash.

## Share / Import shared project: visible errors

User, 2026-11-12: share and import work, but when one fails it is hard to see why. Today a failure is only the status line's text
(`share failed: ...`, `import failed: ...`, `share_prompt.rs`), easy to miss and gone at the next message, and the reasons a share can
refuse (an unsaved tile, no database entry, the tile not in a kit's layout, a file that does not read) or an import can (not a shared
zip, another game's, a path outside the projects folder, the shared tile open, a file that does not write) never reach the log. To do:
show a failure in a dialog (or a lasting notice) with the reason and what to do about it, write it to the log, and list there the
prefabs a share left out and an import could not find.

Done (2026-11-12, `share_prompt.rs`): a failure (menu, prompt or console) opens a dialog with the reason (red) and what to do about it, and
is a red line in the log; a share that leaves out prefabs found nowhere, and an import that lacks prefabs, open it too with the list (a
warning in the log).

## The Inspector comes to the foreground when it changes

User, 2026-11-12: when something changes what the Inspector shows (a selection, Flip mode's panel, a brush's settings, a placing's
Inspector), and the Inspector is a tab that is not the one shown in its dock (behind Weather or Tile, say), it is brought to the
foreground, so that what changed is seen. Only on a change of what it shows, not on every frame: a tab the user picked must stay picked
while they work in it.

Done (2026-11-12): docs/editor.md, "Layouts".

## Rome 2 references: to check, and ships on the sea

User, 2026-11-12, after the Rome 2 reference fixes (feature/rome2_refs):
- Check a Rome 2 ship's sails up close: their cloth is skinned with the per-palette lines of `renderer/skinned.rs` (a sail's bones on a line
  of their own), not seen yet.
- Check a Rome 2 ship's crew up close (the shot so far was too far to tell).
- A ship reference reported no water under it on roman_minor_port_b/minor_b, even where the sea should be: check whether the water lookup
  (`Tile::water_level`) sees Rome 2's sea at all.
- Ships placed over the sea should float on the sea's surface by default, not sit freeform or on the ground at the bottom of the sea: a
  way to make a ship reference take the sea water level (0 m, or the tile's sea) when it is over the sea.

## Units stuck on middle_men_city_b/medium's walls (paused)

User, 2026-11-12: a video report of units getting stuck on the barbarian_fort palisade of TDD's middle_men_city_b/medium (the wall around
(490, 270)), waiting for more information (where exactly they stop: the walk, the inner ramp, a gate, the wall's foot; attackers or
defenders; which escalation). Checked with the editor (`wall_pairs`, `on_walks`, `siege_check`, docs/editor.md "Check platforms"): every
pair of its pieces joins (5 edges each), the terrain over the ramps is as on the vanilla tiles, the siege AI graph has no problems. The one
lead: four `vik_small_rocks_group_06` of category generic (vanilla's rocks there are incidentals) on rampart and gateway walks, at (379,
485), (368, 482), (667, 307) and (575, 280). Not for this branch.

## Check platforms: piers

User, 2026-11-12: on port tiles Check platforms lists many `bridge_wood_1` pieces (piers: buildings of the bridge category that reach the
ground on one side by design) as bridges units cannot cross, and they flood the list. Leave out (or list apart) the bridges that are piers.
Not for now.

## Tools sharing one scene: what each tool shows (to discuss)

Noted 2026-11-13 (user: undecided, needs discussion). The Tile, Prefab, Terrain and Weather tools share one renderer and one scene. That is
right most of the time: the Weather tool edits the weather of the open tile, and later a tile of the tilemap could be opened in the Tile
tool from the map. But a tile open in the Tile tool is still drawn when the Terrain tool shows a tilemap: both at once, the tile at its own
origin with no relation to the map. What is shared today:
- **Scene layers**: every open document has a layer (`DocumentManager`), all drawn; only a stand-in (the Weather tool's sample map) hides
  while a document with its own ground (`Document::shows_scene`: a tile, a map) is open. A Prefab tool stage is a tile document too.
- **Weather light**: one editor-wide preview (`Editor::set_weather_preview`), set by both the Tile and the Terrain tool; the last one wins.
- **Camera**: one, shared by every tool.
- Also a global: the scene's ground overlay (`RenderScene::set_ground_overlay`, the Terrain tool's map panel).

Options raised, none chosen:
- Each scene tool shows only its own document (switching tools swaps them); the Weather tool lights the last scene tool's scene.
- The Terrain tool draws an open tile at its placements on the map, in place of the map's copy (towards "click a tile, edit it in the
  Tile tool"); the Tile tool's Map context already does the reverse.
- A camera per tool or the shared one; a weather per scene tool or the shared one.
