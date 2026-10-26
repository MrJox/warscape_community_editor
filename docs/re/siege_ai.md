# Siege AI graph (ATTILA)

What the game does with a tile's siege AI graph (`ai_hint_polyline_list_marker.markers`, TEd's `SiegeAINodeInstance` /
`SiegeAIEdgeInstance`), from empire.retail.dll and TEd.AssemblyKit.exe (Ghidra), and measured over the 73 vanilla settlement and
encampment `tile.ted` files of the kit (2026-10-24). Lengths in TEd scene units (su); the markers hold su / 2.

## The records

| TEd type | Hint | Polylines | Graph |
| --- | --- | --- | --- |
| `SANT_AREA` | 21 | Bounding Box (closed, 4 points), Boundary (closed) | street |
| `SANT_INTERSECTION` | 22 | same | street |
| `SANT_ENTRY` | 30 | same | street |
| `SANT_WALL_AREA` | 26 | same | wall |
| `SAET_STREET` | 23 | Street (open), Street boundary x 2 (open) | street |
| `SAET_AREA_CONNECTION` | 27 | Street (2 points) | street |
| `SAET_WALL` | 28 | Street | wall |
| `SAET_PALISADE` | 40 | Street | wall (matched to nothing) |

No ids: an edge's nodes come from geometry. Every vanilla instance has `add_boundary="0"`, orientation 0.

## The engine (confirmed)

- **Loader** `FUN_10287560` (RVA 0x287560): records sorted per hint type into the battle's hint store; `AIH_FORT_APPROACH` and
  `AIH_DEFAULT_DEPLOYMENT_LINE` share a list, `AIH_FORT_PERIMETER` its own.
- **Ingest** (`FUN_1024e3f0` nodes, `FUN_1024e5a0` edges): polyline 0 needs 2+ points and a size >= 0.01, and passes a per-segment
  spatial test (probably within the battlefield), else the record is **dropped silently**. An edge with 3+ polylines takes 1 and 2 as its
  boundaries.
- **Two graphs** (`FUN_10266550`): the street graph (area, intersection, entry; street, area connection) and the wall graph (wall area;
  wall, palisade). They never connect.
- **Node rectangle** (`FUN_102262a0`): the Bounding Box's first 4 points: centre their mean, axis p3 -> p0, width |p1 - p0|, height
  |p2 - p1|. So it must be a rectangle with its points in order; rotated is fine.
- **The connection rule** (`FUN_1027b320` -> `FUN_10250230`): for each street / area connection edge and each node, the node whose
  **rectangle** holds the Street's first point is its start, the one holding its last point its end; the last match wins. Wall edges are
  matched against wall area nodes only. Each street boundary is reversed if needed so its ends lie in the nodes' **Boundary** polygons.
- **Capacity** (`FUN_10537330`, tweak `settlement_ai_area_overoccupation_penalty`): a node's rectangle width x height / (units + 1). The
  Bounding Box is the occupiable area, the Boundary the walkable outline.
- **Debug draw** (`FUN_10226910`): the settlement's ways in are labelled "Entry Node", "Wall", "Gate".
- No graph-level check, assert or log anywhere: a broken graph fails silently. TEd checks only each instance ("Siege AI Node should have
  a main box shape", "Siege AI Node has no boundary", "Siege AI Edge should have a main street shape", "Street Edge needs to have two
  boundary splines"); it has no generator. The community (tw-modding wiki, WH2 / 3K) reports a hard crash without `AIH_FORT_PERIMETER`.

## CA's layouts (measured)

- **Counts:** area 1197, intersection 220, entry 213, wall area 2 (one tile); street 1701, area connection 522, wall 172, palisade 24
  (encampments only). 5-63 nodes per tile (median 21), node centres ~75 su apart.
- **Area:** plazas, capture yards, the major victory point's square (inside an area in 89 of 99), the yard behind each gate, stretches
  of street. Box median 59 x 52 su; Boundary drawn out over the building faces (90% of its points in no-go), area ~4500 su^2.
- **Intersection:** street junctions (degree 3 in 89%), box ~22 x 22 su, joined almost only by streets, never a capture.
- **Entry:** where streets leave an unwalled settlement (minor towns ~6 each, harbours, palisade gaps), about on the fort perimeter;
  leaves (degree 1). **Never at gates:** a gate's way in is an area on the city side whose Boundary stops on the wall edge's line.
- **Street:** centre line 2-11 points, 8-470 su (median 76; 43 over 300, so no length limit); its ends ~12 su inside the nodes' boxes,
  not at their centres; never through no-go. Boundaries: the corridor's two sides along the building / no-go faces (width median 32 su),
  each ending in a ~4 su tick turned into the node, ends ~2 su inside its Boundary.
- **Area connection:** 2 points, ~19 su, across the shared border of two nodes whose Boundaries touch or overlap.
- **Wall:** one open polyline per continuous wall run, endcap to endcap or into a no-go region, through gates and towers, on the walkway
  centre (8 m walls on the piece axis, 15 m walls 5 su to the city side), ~10 su samples on curves. None over walls inside no-go. Its ends
  touch no node; the nodes along it abut it (0-5 su).
- **Obstacles:** CA's areas lie over buildings' `orange_thingies` (plazas, temple floors, roads stand on them); only hard collision
  outlines bound the graph, and it runs over the siege barricades across the streets. Buildings with no hard outline (garden walls,
  decorations) block nothing.
- **Topology:** one connected street graph (72 of 73) with cycles: a ring road inside the walls, the major capture ~4 hops from each gate.
- **Fort perimeter:** exactly one closed `AIH_FORT_PERIMETER`, ~10-14 su outside the walls, along the no-go edges where there is no wall;
  holds every node but some entries, every capture and the walls. A `PALT_FORT` separator often runs along it. 3-8 two-point
  `AIH_FORT_APPROACH` lines ~200 su outside it; ports add `AIH_DEFAULT_DEPLOYMENT_LINE` and the beach hints.

## Encampments, ports, separators (measured)

- **Encampments** (4): the palisade is `siege_barricade_wall_1_encampment` pieces, origins on the line 10 apart along their z; runs of
  them with ~40-unit steps between (openings ~30 wide; 8, 4, 6, 6). One `SAET_PALISADE` per run through the origins (within 1 unit),
  ending on the opening's node boundary. One entry per opening, outside the palisade (box 24-32 across, 14-31 deep, inner edge on the
  line). Inside: one big area (3 camps) or a ring of areas (barbarian). Perimeter 2-13 out of the palisade; no separators, no-go or
  zones; 4 approach lines 375-540 out.
- **Ports** (29): 4 `AIH_DEFAULT_DEPLOYMENT_LINE` (2 points, ~250 long) in deep water inside the defender's sea lobe (~280 offshore),
  across the direction to the town, cross(end - start, town - start) < 0 in (x, z) (the approach lines the other
  way). 3-5 `AIH_BEACH_LANDING` along the 0 m line either side of the town; beside each an `AIH_BEACH_LANDING_FORT_APPROACH` (2 points,
  ~100 long, ~40 inland). The defender's zone takes a sea lobe off the town (200-500 out) and the harbour; the attacker gets a sea
  polygon (~150 offshore, ~200 from the lobe) and land polygons (~20 short of the coast, ~200 from the perimeter), no subtractive one.
  Entries on the quays (~16 from the water) in walled ports.
- **Separators** (`AISeparatorInstance`, `type` `PALT_FORT`, open polylines; 26 of 28 walled cities, 20 of 21 walled ports, no minor
  town or camp): they run where the ring has no wall (25 % of the perimeter's length; walls 55 %, no-go only 13 %), from a wall run's
  end (on its line within ~3 units) along the perimeter, through the no-go just outside the streets, over the harbour water in ports.
  Walls + separators + no-go regions close the ring in 47 of 48 walled tiles (not Olbia). Their direction is mixed.

## Open

- The ingest's spatial test (`FUN_116a06d0`); what an edge missing a node does at runtime; how the attack and defence tactics use the
  graphs; what wall area nodes and palisade edges are for (the community could not make them work).
