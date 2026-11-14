# UX pass: to do later

Left from the UX pass of 2026-11-13 (branch `feature/ux_part2`); the weather tool's ideas are in [weather.md](weather.md), the tools sharing
one scene in [editor_todo.md](editor_todo.md).

## Top menus

Done 2026-11-14 (docs/editor.md, "Menus"). Left:
- **Display toggles** (user: where they go is to decide; listed here): in the Tile menu, *Distance culling of markers*, *Tile border*,
  *Tint freeform instances*, *VFX boxes* and the *Draw distances* (vegetation, decals, others); in the ground chip (top right of the
  viewport), *Rivers*, *Sea at 0 m* and its sea state, *Trees*, *Grass*, the automatic no-go areas; in the areas chip, *Show polygons*,
  *Over buildings* and the outline / fill opacities; in the building technology chip, what of the buildings' logic is drawn; in the Scene panel, each type's visibility (VFX among them: the same as *VFX boxes*, which also keeps the choice for the
  next tiles); View's Grid, Compass, Coordinates and Fog.
- **The Weather effects section by chips** (user: see whether the chips can replace it fully): precipitation on/off and Restart, the
  battle's season (the tile's season chip sets it while a tile is open) and snow on the map, and New battle direction for a random wind;
  the rest of the section is a read-out (definition, effects, wind numbers, particles).

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
