# Generate SiegeAI (one click)

A button that writes a settlement's whole siege AI layout: the street graph, the wall edges, the fort perimeter, the approach lines and
the deployment zones. What the game needs and how CA lays it out: docs/re/siege_ai.md. Scene units (su) throughout.

## Decisions (user, 2026-10-24)

- **Streets from the free space's skeleton.** Free space: inside the perimeter, minus the no-go regions, the buildings' footprints and the
  walls. Its medial axis gives the streets; junctions become intersection nodes, plazas (wide spots), capture points and gates area nodes.
- **Obstacles:** no-go regions and every building's footprint (its logic file's collision outlines), so a tile without no-go over its
  blocks works too.
- **Long streets** (over about 300 su, a guide: the AI takes long to walk them) get area nodes along them.
- **Everything in one click:** street graph, wall edges, fort perimeter, fort approach lines, deployment zones.
- **Deployment zones** as CA's: defender = the perimeter about 7 su in, its guerrilla-exclusion polygon the perimeter about 150 su out;
  attacker = a rectangle over the whole map minus the perimeter about 190 su out, its guerrilla-exclusion the same rectangle minus the
  perimeter about 70 su out. The approach lines (8, tangential) on the attacker zone's inner edge, inside it. **Ports now:** the sea
  (terrain under 0 m joined to the map's edge) is cut from the attacker zone and the defender zone takes in the water next to the town.
- **Perimeter found automatically:** walls and no-go regions (and buildings) as a barrier, the outside flood-filled from the map's edge;
  what is not reached is the settlement, its outline about 12 su out is the perimeter. Inner walls (citadels) stay inside. A town without a
  closed ring gets the barrier closed (morphological closing, growing radius) until it is closed.
- **Entry nodes** where a street crosses the perimeter on a stretch with no wall (unwalled towns, harbours).
- **Output in a new layer `(AUTOGEN) SiegeAI Layout`** (Normal export). Generating again replaces that layer. The old layout goes out of
  the export: a layer holding only siege AI nodes / edges, fort perimeter / approach / deployment line hints or deployment zones gets
  `LayerExportType_Off` (TEd's "Don't Export"); such instances in a layer with other content are moved into a `(OLD) SiegeAI Layout`
  layer set Off. One undo entry. The Scene panel's Export type menu gets Off and Separate.
- **UI:** a "Generate SiegeAI" button in the Polygons / AI hints panel; the summary and warnings in the status and the log.
- **Testing:** in the editor (the user). A check example compares the generator with CA's layouts on the kit's tiles.

## Design

- **`wce_siege`** (new crate, pure geometry, no document): a raster in su (2 su cells) for the free space, distance transform, flood
  fill, closing, thinning (medial axis), contour tracing; the generator from an input (the map's bounds, no-go polygons, building
  footprints, wall pieces with their walkway lines and gate flags, captures, a sea mask) to a layout (nodes, edges, hints, deployment
  zones, warnings); a checker (each street / connection end in exactly one node box, boxes don't overlap, the street graph connected,
  every capture in an area, the walls inside the defender zone, the approach lines inside the attacker zone).
- **Walls:** a fort piece is a building whose logic file has docking lines or gates; its walkway line runs along its frame's x at the
  middle of its top platforms. Pieces chain end to end (across towers) into runs; a run's line is cut where it enters no-go. `WALL_SPLINE`s
  give their layout's walkway line.
- **Nodes:** box = a rectangle along the street's direction that fits the free space (intersections ~22 su, areas up to ~80 su), boxes
  never overlap; Boundary = the free space nearest the node (a geodesic split between nodes) within a reach, grown a few su into the
  obstacles (CA draws it over the building faces).
- **Edges:** street = the skeleton path between two nodes, its ends ~10 su inside the boxes, its sides the corridor's two faces with a
  4 su tick into each node; area connection (2 points) between nodes whose Boundaries touch; wall edges on the walkway lines.
- **`wce_tile_editor` `tile_document/siege.rs`:** the input from the document (no-go, buildings and their logic files, walls, captures,
  the heights), the layout written into the layer (nodes and deployment groups are instances with polygon children).

## Status

- [ ] `wce_siege`: raster tools, generator, checker.
- [ ] Document side: input, output, the old layout out of export, undo.
- [ ] UI: button, Export type menu.
- [ ] Check example on the kit's vanilla settlements.
