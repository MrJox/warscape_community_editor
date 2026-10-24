# Editor: to do later

Ideas noted for later (user, 2026-10-24).

## Minimap with views

A minimap of the open tile, with views to switch between. One of them is the siege AI view drawn by `wce_siege::svg` (the
`siege_check` example's pictures):
- the no-go regions, buildings' hard outlines and fort pieces;
- the nodes' rectangles and Boundaries;
- the streets with their sides, the area connections, the wall and palisade edges;
- the perimeter, separators, approach lines, the port lines and beaches;
- both deployment zones.

Other views to decide (plain top-down, height, ground types, ...).

## Save: log the siege AI's problems

When a tile is saved or exported, write a short error to the log for each broken part of its siege AI. The checks already exist in
`wce_siege::check` (`check`, `check_captures`) and are run on `Tile::current_siege_layout()`:
- no fort perimeter;
- an edge's end in no node's rectangle;
- a node without a rectangle or Boundary;
- the street graph in parts;
- the main capture point in no area;
- walls, separators and no-go regions not closing the ring.

These are only log lines: the save still goes ahead.
