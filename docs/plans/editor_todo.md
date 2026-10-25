# Editor: to do later

Editor-wide ideas, noted for later (user, 2026-10-24). Status (2026-10-26): all three open.

## Minimap with views

Done (2026-10-25): the Tile tool's Minimap panel (docs/editor.md, "Minimap"): the colour, height or no base, with the siege AI, the deployment
zones and the camera as layers. Other views to decide later (ground types and protection are there through the Ground section's view).

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
