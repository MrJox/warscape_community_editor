# Generate SiegeAI (one click)

The Tile menu's **Generate SiegeAI** (after Import from Assembly Kit; `crates/wce_tile_tool/src/tool.rs`, command
`tile_generate_siege_ai`) writes a settlement's whole siege AI layout: the street graph, the wall edges, the fort perimeter, separators,
the approach lines and the deployment zones. What the game needs and CA's layouts (incl. encampments, ports, separators):
[docs/re/siege_ai.md](../re/siege_ai.md). Scene units (su) throughout. The minimap's siege AI view and logging siege problems on save are
in [editor_todo.md](editor_todo.md).

## Decisions (user, 2026-10-24)

- **Streets from the free space's skeleton.** Free space: inside the perimeter, minus no-go regions, buildings' hard collision outlines
  (not `orange_thingies`; siege barricades left out) and walls. Its medial axis gives the streets; junctions become intersections, plazas,
  captures and gates areas. Streets over about 300 su get areas along them.
- **Everything in one click**, one undo entry. Output in a new layer `(AUTOGEN) SiegeAI Layout` (Normal export), replaced when generated
  again; the old layout goes out of the export (a layer holding only siege AI content set Don't Export, mixed content moved into an
  `(OLD) SiegeAI Layout` layer set Off). Summary and warnings in the status and the log.
- **Perimeter found automatically:** walls, no-go and buildings as a barrier, the outside flood-filled from the map's edge; the settlement
  holds the main capture and every capture within 1000 units; outline about 12 su out. Inner walls stay inside; an open ring is closed
  (morphological closing) only to tell what is outside.
- **Deployment zones** as CA's (defender ~7 su in, attacker ~190 su out, guerrilla exclusions 150 / 70), 8 tangential approach lines on
  the attacker zone's inner edge. **Ports** (perimeter within 40 su of sea: ground under -0.5 m joined to the map's edge): the defender
  takes the sea within 400 su, the attacker land and sea polygons, deployment lines, beach landings, quay entries.
- **Encampments:** palisade runs of `siege_barricade_wall_1_encampment`, a `SAET_PALISADE` per run, an entry outside each opening, the
  inside as a normal street graph (the user's choice; 3 of CA's 4 camps have one big area).
- **Separators** (`PALT_FORT`) along the perimeter where no wall runs, in towns whose walls cover 30 % of it or more; a check warns when
  the ring stays open. Wall edges run 8 su past each gate.
- **Entry nodes** where a street crosses the perimeter with no wall.

## Design

- **`wce_siege`** (pure geometry): a 2 su raster (distance transform, flood fill, closing, thinning, contours); the generator from an input
  (bounds, no-go, footprints, wall pieces with walkways and gate flags, captures, a sea mask) to a layout; a checker (`check`,
  `check_captures`); SVG output.
- **Fort pieces:** buildings with wall firing lines (`Crenel*`, `HighWall`, `LowWall`) or gates, plus raised buildings touching them; wall
  edges along the middle of their platforms. Procedural walls give their layout's walkway (`wce_walls::scene::walls`).
- **Nodes:** boxes along the street that fit the free space and never overlap; Boundary a geodesic split of the free space, grown a few su
  into the obstacles. **Edges:** streets along the skeleton, ends ~10 su inside the boxes, sides on the corridor's faces with 4 su ticks;
  area connections between touching Boundaries.
- **Document side:** `wce_tile_editor` `tile_document/siege.rs`.

## Status (2026-10-26)

Done: `wce_siege`, the document side, the menu item and command, the Export type menu (Separate, Don't Export), encampments, ports,
separators. On the kit's 73 settlements and camps (`cargo run --release -p wce_tile_editor --example siege_check -- <game> <out> <kit
tiles>/settlement_* <...>/encampments`, SVGs of ours and CA's, about 3 s a tile, 2026-10-24) every tile gets a layout that exports and
reads back; counts of entries, palisades, deployment lines, beaches and separators mostly match CA's.

## Open

- [ ] In game (user): Edoras, a port, an encampment.
- [ ] Rivers below 0 m joined to the map's edge are taken for sea on 6 tiles (among them Ctesiphon, Constantinople, Ravenna small,
  western_city_a minor, western_port_a medium): ports zones and lines where CA has none.
- [ ] The ring stays open on nordic_city_c medium (separators).
- [ ] 9 tiles get the street graph in parts (pockets with no gate found between them).
- Wall area nodes are not generated: their purpose is an RE question (docs/re/siege_ai.md, Open).
