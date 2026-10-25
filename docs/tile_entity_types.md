# What a tile holds, and what the Tile editor does with it

The element types of `tile.ted` (counted over the 1380 `tile.ted` of the Attila assembly kit, 2026-10-25), and for each one whether the
Tile editor can create it, edit it, and draw it, and whether the export (`wce_tile_io::attila::compile`, stages A to C) writes it.

Every type below can be moved and rotated with the gizmo and the Inspector's position, duplicated, deleted, renamed, hidden and locked
(`OIF_VISIBLE`, `OIF_ACTIVE` = selectable), and moved between layers. "Edit" below means more than that: its own properties in the Inspector
(TEd's Property Editor lists, `wce_tile::properties`) or its points. What is exported is decided by the layers: an element goes where the
nearest layer above it with an export type says (`LayerExportType_Normal`: the tile's lists; `_Separate`: a building list of its own;
`_Escalation1..3`: that escalation's lists; `_Off`: nowhere; a normal layer inside another layer exports with it).

| Type | In the kit | Create | Edit | Drawn | Exported |
|---|---:|---|---|---|---|
| `BUILDING_INSTANCE` | 1,896,156 | yes (Placement: a building key) | properties | the building's model pieces (a wire box when the key has no model) | building lists, escalation BMDs, outlines |
| `VEGETATION_INSTANCE` | 1,404,435 | yes (Create: click; a tree key) | properties | the tree model, as a list tree of the shown climate | through the tree lists (vegetation generator) |
| `ProjectedDecalInstance` | 1,921,915 | yes (Create: click; a decal key) | properties | the decal | `prop_marker.markers` |
| `POINT_LIGHT_INSTANCE` | 166,054 | yes (Create: click) | properties | the light (and a cross marker) | `point_light_marker.markers` |
| `VFXInstance` | 33,956 | yes (Create: click; an effect) | properties | the effect when it plays (autoplay, season) and a box marker | emitter markers |
| `SIMPLE_MESH_INSTANCE` | 1,233 | yes (Create: click; a model) | properties | the model (lake water planes among them) | `prop_marker.markers`, the water map |
| `AI_HINT_POLYGON_INSTANCE` | 6,166 | yes (Draw polygon, a hint type) | properties, points | outline | AI hints |
| `NOGO_REGION_POLYGON_INSTANCE` | 3,981 | yes (Draw polygon) | properties, points | outline | no-go regions |
| `GO_REGION_POLYGON_INSTANCE` | 222 | yes (Draw polygon) | properties, points | outline | go regions |
| `SIEGE_AI_POLYGON_INSTANCE` | 9,085 | yes (Draw polygon) | properties, points | outline | siege AI |
| `VegetationExclusionPolygonInstance` | 12,653 | yes (Draw polygon) | properties, points | outline | through the vegetation generator |
| `ZONE_POLYGON_INSTANCE` | 575 | yes (Create: draw) | properties, points | outline | zones |
| `DeploymentZonePolygonInstanceGroup` | 403 | yes (Create: click; with a 64 m square polygon) | properties | box marker | deployment zones |
| `DEPLOYMENT_ZONE_POLYGON_INSTANCE` | 800 | yes (Create: draw; into the selected group, else a new one) | properties, points | outline | deployment zones (in their group) |
| `CaptureLocationInstance` | 1,006 | yes (Create: click; with a 20 m boundary circle) | properties | box marker | capture locations |
| `CaptureLocationPolygonInstance` | 1,006 | with its capture location | points (a circle's are read-only) | outline | the capture location's boundary |
| `CivilianDeploymentZone` | 941 | yes (Create: draw) | properties, points | outline | civilian deployment zones |
| `CivilianShelter` | 9,095 | yes (Create: click; its target 10 m ahead) | properties (its `target` only as numbers) | box marker (no target line) | `civilian_shelter.bin` |
| `LiteBuildingZone` | 140 | yes (Create: draw) | points | outline | lite building zones |
| `SiegeAINodeInstance` | 1,632 | yes (Create: click; with a 20 m square area) | properties | box marker | siege AI nodes |
| `SiegeAIEdgeInstance` | 2,419 | yes (Create: draw one boundary; the second is 10 m to its right) | properties | box marker (no line between its nodes) | siege AI edges |
| `AISeparatorInstance` | 108 | yes (Create: draw) | properties, points | outline | AI separators |
| `SplineRiverInstance` | 254 | yes (Create: draw) | properties, points, handles, point values | the river (game shaders) and its curve | through `river_spline<n>.xml`, written on save for each river that changed |
| `SEGMENTED_SPLINE_INSTANCE` (roads) | 4 | yes (Create: draw) | properties, points, handles, point values | its curve | no (a TEd painting guide) |
| `LayerInstance` | 2,841 | yes (New layer, Ctrl+G) | name, export type | (a layer) | decides the export, see above |
| `UserInstanceGroup` | 2,444,458 | no | properties | (a group) | its contents |
| `INSTANCE_GROUP` | 575 | no | properties | (a group) | its contents |
| `LAYER_INSTANCE_GROUP` | 1 | no | none | (a group) | its contents |

Not instances (no position of their own): `SCENE_GRAPH` (the root layer; `exportable` on 430 of them), `point` / `points` (a polygon's or a
spline's points), `children`, `camera`, and the modules (`Vegetation`, `Grass`, `Particle_Effects_VFX`).

## What is missing

- Every type is made by the Tile panel's Create section (`TileDocument::create`) with the kit's defaults: the most common value of each
  attribute over the kit's tiles (`scripts/re/instance_defaults.py` -> `instance_templates.py` -> `wce_tile::instance_templates`), keys typed
  as text.
- A siege AI edge does not refer to nodes: edges and nodes are exported on their own, each with its boundary polylines.
- Painting along a road (TEd's Spline Road splat) needs reverse engineering first.
- Drawing a civilian shelter's target and a siege AI edge's two nodes.
