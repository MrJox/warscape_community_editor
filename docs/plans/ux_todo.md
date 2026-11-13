# UX pass: to do later

Left from the UX pass of 2026-11-13 (branch `feature/ux_part2`); the weather tool's ideas are in [weather.md](weather.md), the tools sharing
one scene in [editor_todo.md](editor_todo.md).

## Top menus

To review (user: later). Found so far: the Tile menu mixes file actions (New tile, Open tile, Open exported tile, Save tile as, Import
from Assembly Kit), tools (Resize, Import height map, Generate SiegeAI, Launch in game) and display toggles (marker culling, tile border,
freeform tint, VFX boxes, draw distances, Tile browser); File holds Save layout / Reset layout; "Migrate old extracted projects" is a
one-off in the Tile menu.

## Save as

- Campaign maps: File > Save as is on (a copy elsewhere now leaves the map unsaved); a map is a projects-folder layout too, so it may
  want Save as off as tiles have it.

## Draggable flag

- The gizmo and the grab leave a non-draggable item (`Tile::draggable`); Align, Snap and scripted moves still move it. To decide whether
  they should too.

## Tooltips

- The tile Inspector's generic attribute rows (`edit_value`, `edit_string(prop.attr, ...)` in `tile_document/edits.rs`, `draft.rs`) have
  none: `wce_tile::properties::Prop` has no description; needs a table of attribute descriptions.
- Not written, unclear what they do: a building piece's `burn`, `burn_amount`, `bounds_colour`; the scene panel's Export Separately.
- The shared viewport chips (camera, snap; `wce_editor` `chips.rs`) were not part of the tile editor pass.
