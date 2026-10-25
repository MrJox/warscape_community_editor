//! How the Tile tool shows the element types of tile.ted (docs/tile_entity_types.md): an icon per type, and the export a layer marks its
//! contents for (`export_type`, TEd's "Export" property of a layer) as an icon and TEd's label.

use wce_editor::icons;
use wce_tile::ted_scene::TedScene;

/// The icon of an element type; a type not listed gets a question mark.
pub fn type_icon(ty: &str) -> &'static str {
    match ty {
        "BUILDING_INSTANCE" => icons::BUILDING_2,
        "VEGETATION_INSTANCE" => icons::TREE_PINE,
        "ProjectedDecalInstance" => icons::STICKER,
        "POINT_LIGHT_INSTANCE" => icons::LIGHTBULB,
        "VFXInstance" => icons::SPARKLES,
        "SIMPLE_MESH_INSTANCE" => icons::BOX,
        "AI_HINT_POLYGON_INSTANCE" => icons::BRAIN,
        "NOGO_REGION_POLYGON_INSTANCE" => icons::BAN,
        "GO_REGION_POLYGON_INSTANCE" => icons::CIRCLE_CHECK,
        "SIEGE_AI_POLYGON_INSTANCE" => icons::CASTLE,
        "VegetationExclusionPolygonInstance" => icons::SHRUB,
        "ZONE_POLYGON_INSTANCE" => icons::LAND_PLOT,
        "DeploymentZonePolygonInstanceGroup" | "DEPLOYMENT_ZONE_POLYGON_INSTANCE" => icons::SWORDS,
        "CaptureLocationInstance" => icons::FLAG,
        "CaptureLocationPolygonInstance" => icons::CIRCLE_DOT_DASHED,
        "CivilianDeploymentZone" => icons::USERS,
        "CivilianShelter" => icons::HOUSE,
        "LiteBuildingZone" => icons::SQUARE_DASHED,
        "SiegeAINodeInstance" => icons::GIT_COMMIT_HORIZONTAL,
        "SiegeAIEdgeInstance" => icons::ROUTE,
        "AISeparatorInstance" => icons::SPLIT,
        "SplineRiverInstance" => icons::WAVES,
        "SEGMENTED_SPLINE_INSTANCE" => icons::SPLINE,
        "WceWallSpline" => icons::BRICK_WALL,
        "LayerInstance" | "SCENE_GRAPH" => icons::LAYERS,
        "UserInstanceGroup" | "INSTANCE_GROUP" | "LAYER_INSTANCE_GROUP" => icons::BOXES,
        _ => icons::CIRCLE_HELP,
    }
}

/// A type's name for people (the Create panel).
pub fn type_name(ty: &str) -> &'static str {
    match ty {
        "BUILDING_INSTANCE" => "Building",
        "VEGETATION_INSTANCE" => "Tree",
        "ProjectedDecalInstance" => "Decal",
        "POINT_LIGHT_INSTANCE" => "Point light",
        "VFXInstance" => "Effect",
        "SIMPLE_MESH_INSTANCE" => "Model",
        "AI_HINT_POLYGON_INSTANCE" => "AI hint",
        "NOGO_REGION_POLYGON_INSTANCE" => "No-go region",
        "GO_REGION_POLYGON_INSTANCE" => "Go region",
        "SIEGE_AI_POLYGON_INSTANCE" => "Siege AI polygon",
        "VegetationExclusionPolygonInstance" => "Vegetation exclusion",
        "ZONE_POLYGON_INSTANCE" => "Zone",
        "DeploymentZonePolygonInstanceGroup" => "Deployment zone (64 m square)",
        "DEPLOYMENT_ZONE_POLYGON_INSTANCE" => "Deployment zone polygon (drawn)",
        "CaptureLocationInstance" => "Capture location",
        "CivilianDeploymentZone" => "Civilian deployment zone",
        "CivilianShelter" => "Civilian shelter",
        "LiteBuildingZone" => "Lite building zone",
        "SiegeAINodeInstance" => "Siege AI node",
        "SiegeAIEdgeInstance" => "Siege AI edge",
        "AISeparatorInstance" => "AI separator",
        "SplineRiverInstance" => "River",
        "SEGMENTED_SPLINE_INSTANCE" => "Road",
        "WceWallSpline" => "Wall (procedural)",
        _ => "Other",
    }
}

/// A layer's export types (without the `LayerExportType_` prefix), as TEd's layer properties name them.
pub const EXPORT_TYPES: [(&str, &str); 6] = [
    ("Normal", "Export"), ("Off", "Don't Export"), ("Separate", "Export Separately"), ("Escalation1", "Escalation 1"), ("Escalation2", "Escalation 2"),
    ("Escalation3", "Escalation 3"),
];

/// The icon of an export type.
pub fn export_icon(kind: &str) -> &'static str {
    match kind {
        "Off" => icons::PACKAGE_X,
        "Separate" => icons::PACKAGE_PLUS,
        "Escalation1" => icons::SIGNAL_LOW,
        "Escalation2" => icons::SIGNAL_MEDIUM,
        "Escalation3" => icons::SIGNAL_HIGH,
        _ => icons::PACKAGE_CHECK,
    }
}

/// TEd's label of an export type.
pub fn export_label(kind: &str) -> &'static str {
    EXPORT_TYPES.iter().find(|(k, _)| *k == kind).map_or("Export", |(_, l)| l)
}

/// A layer's own export type (`Normal` when it has none).
pub fn own_export(sc: &TedScene, layer: usize) -> String {
    let t = sc.doc().get(layer, "export_type").unwrap_or_default();
    t.strip_prefix("LayerExportType_").map_or_else(|| "Normal".to_owned(), str::to_owned)
}

/// Where an element is exported, as the export decides it (`wce_tile_io::attila::compile`): the nearest layer at or above it whose export type
/// is not Normal (a normal layer exports with the layer around it), else Normal; and that layer.
pub fn effective_export(sc: &TedScene, elem: usize) -> (String, Option<usize>) {
    let mut e = Some(elem);
    while let Some(x) = e {
        if sc.type_of(x) == "LayerInstance" {
            let own = own_export(sc, x);
            if own != "Normal" {
                return (own, Some(x));
            }
        }

        e = sc.doc().elements()[x].parent;
    }

    ("Normal".to_owned(), None)
}
