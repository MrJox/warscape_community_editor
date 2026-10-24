//! TEd's enum tables (file name, display name), read from TEd.AssemblyKit.exe's data (the strings of each enum's two arrays). Generated once by a script; the order is TEd's combo box order.

pub const EXPORT_TYPE: &[(&str, &str)] = &[
    ("LayerExportType_Normal", "Export"),
    ("LayerExportType_Off", "Don't Export"),
    ("LayerExportType_Separate", "Export Separately"),
    ("LayerExportType_Escalation1", "Escalation 1"),
    ("LayerExportType_Escalation2", "Escalation 2"),
    ("LayerExportType_Escalation3", "Escalation 3"),
];

pub const LOW_FREQUENCY_CHANGE: &[(&str, &str)] = &[
    ("GRM_AS_SINGLE", "Children move as group"),
    ("GRM_CHILDREN", "Children move independently"),
];

pub const POLYGON_SHAPE: &[(&str, &str)] = &[
    ("PS_CIRCLE", "Circle"),
    ("PS_SEMICIRCLE", "Semicircle"),
    ("PS_RECTANGLE", "Rectangle"),
    ("PS_POLYGON", "Polygon"),
];

pub const AI_HINT_TYPE: &[(&str, &str)] = &[
    ("AIH_DEFENSIVE_LINE", "[DEPRECATED] defensive_line"),
    ("AIH_DEFENSIVE_HILL", "defensive_hill"),
    ("AIH_DEAD_GROUND", "[DEPRECATED] dead_ground"),
    ("AIH_IMPASSIBLE", "[DEPRECATED] impassible"),
    ("AIH_TOWN_CLUSTER", "[DEPRECATED] town_cluster"),
    ("AIH_CHOKE_POINT", "[DEPRECATED] choke_point"),
    ("AIH_GOOD_CAVALRY_GROUND", "[DEPRECATED] good_cavalry_ground"),
    ("AIH_COVER", "[DEPRECATED] cover"),
    ("AIH_AMBUSH_FOREST", "forest"),
    ("AIH_AMBUSH_BUILDINGS", "[DEPRECATED] ambush_buildings"),
    ("AIH_REDOUBT", "[DEPRECATED] redoubt"),
    ("AIH_SUITABLE_GROUND_BOMBARDMENT", "[DEPRECATED] suitable_ground_bombardment"),
    ("AIH_SUITABLE_DEPLOYABLE_GABIONADE", "[DEPRECATED] suitable_deployable_item_gabionade"),
    ("AIH_SUITABLE_DEPLOYABLE_FOUGASSE", "[DEPRECATED] suitable_deployable_item_fougasse"),
    ("AIH_SUITABLE_DEPLOYABLE_CHEVAUX_DE_FRISE", "[DEPRECATED] suitable_deployable_item_chevaux_de_frise"),
    ("AIH_SUITABLE_DEPLOYABLE_EARTHWORKS", "[DEPRECATED] suitable_deployable_item_earthworks"),
    ("AIH_FORDING_POINT", "[DEPRECATED] fording_point"),
    ("AIH_FORT_APPROACH", "fort_approach"),
    ("AIH_BRIDGE_CROSSING", "river_limes_crossing"),
    ("AIH_BEACH_LANDING", "beach_landing"),
    ("AIH_RIVER_EDGE", "river_limes_edge"),
    ("AIH_SIEGE_AREA_NODE", "siege_area_node"),
    ("AIH_SIEGE_INTERSECTION_NODE", "siege_intersection_node"),
    ("AIH_SIEGE_STREET_EDGE", "siege_street_edge"),
    ("AIH_CAPTURE_POINT_MAJOR", "major_capture_point"),
    ("AIH_CAPTURE_POINT_MINOR", "minor_capture_point"),
    ("AIH_SIEGE_WALL_AREA_NODE", "siege_wall_area_node"),
    ("AIH_SIEGE_AREA_CONNECTION_EDGE", "siege_area_connection_edge"),
    ("AIH_SIEGE_WALL_EDGE", "siege_wall_edge"),
    ("AIH_COVER_LINE", "river_limes_covering_fire_line"),
    ("AIH_SIEGE_ENTRY_NODE", "siege_entry_node"),
    ("AIH_AMBUSH_MELEE_LINE", "ambush_melee_line"),
    ("AIH_AMBUSH_MISSILE_LINE", "ambush_missile_line"),
    ("AIH_AMBUSH_CAVALRY_LINE", "ambush_cavalry_line"),
    ("AIH_AMBUSH_DEFENDER_INITIAL_PATH", "ambush_defender_initial_path"),
    ("AIH_FORT_PERIMETER", "fort_perimeter"),
    ("AIH_BEACH_LANDING_FORT_APPROACH", "beach_landing_fort_approach"),
    ("AIH_AMBUSH_ASSAULT_AREA", "ambush_assault_area"),
    ("AIH_AMBUSH_FIREBALL_LINE", "ambush_fireballs"),
    ("AIH_DEFAULT_DEPLOYMENT_LINE", "default_deployment_line"),
    ("AIH_SIEGE_PALISADE_EDGE", "siege_palisade_edge"),
];

pub const DEPLOYMENT_POLYGON_TYPE: &[(&str, &str)] = &[
    ("DZPT_ADDITIVE", "Standard Additive"),
    ("DZPT_SUBSTRACTIVE", "Standard Subtractive"),
    ("DZPT_GUERRILLA_EXCLUSION_ADDITIVE", "Guerrilla Exclusion Additive"),
    ("DZPT_GUERRILLA_EXCLUSION_SUBTRACTIVE", "Guerrilla Exclusion Subtractive"),
];

pub const DEPLOYMENT_CATEGORY: &[(&str, &str)] = &[
    ("DAC_DEFAULT", "Default"),
    ("DAC_AMBUSH", "Ambush"),
    ("DAC_RIVER", "River"),
    ("DAC_COAST", "Coast"),
    ("DAC_ENCAMPMENT", "Encampment"),
];

pub const NEW_POLYGON_TYPE: &[(&str, &str)] = &[
    ("PDM_POLYGON", "Polygon"),
    ("PDM_FREEHAND", "Freehand"),
    ("PDM_RECTANGLE", "Square"),
    ("PDM_CIRCLE", "Circle"),
];

pub const LIGHT_ANIMATION: &[(&str, &str)] = &[
    ("LAT_NONE", "None"),
    ("LAT_RADIUS_SIN", "Radius Sin"),
    ("LAT_RADIUS_SIN_SIN", "Radius Sin Sin"),
];

pub const LIGHT_FALLOFF: &[(&str, &str)] = &[
    ("WPLFT_LINEAR", "Linear"),
    ("WPLFT_SMOOTH", "Smooth"),
];

pub const TECH_LEVEL: &[(&str, &str)] = &[
    ("TL_TECH_LEVEL1", "Tech level 1"),
    ("TL_TECH_LEVEL2", "Tech level 2"),
    ("TL_TECH_LEVEL3", "Tech level 3"),
    ("TL_TECH_LEVEL4", "Tech level 4"),
    ("TL_TECH_LEVEL5", "Tech level 5"),
];

pub const CAPTURE_TYPE: &[(&str, &str)] = &[
    ("CLT_MAJOR", "Major"),
    ("CLT_MINOR", "Minor"),
];

pub const SIEGE_EDGE_TYPE: &[(&str, &str)] = &[
    ("SAET_STREET", "Street Edge"),
    ("SAET_AREA_CONNECTION", "Area Connection Edge"),
    ("SAET_WALL", "Wall Edge"),
    ("SAET_PALISADE", "Palisade Edge"),
];

pub const SIEGE_NODE_TYPE: &[(&str, &str)] = &[
    ("SANT_AREA", "Area Node"),
    ("SANT_INTERSECTION", "Intersection Node"),
    ("SANT_WALL_AREA", "Wall Area Node"),
    ("SANT_ENTRY", "Entry Node"),
];

pub const SEPARATOR_TYPE: &[(&str, &str)] = &[
    ("PALT_NONE", "No Link"),
    ("PALT_GENERIC", "Generic Area Link"),
    ("PALT_FORT", "Fort Area Link"),
];

pub const UNIT_TYPE: &[(&str, &str)] = &[
    ("UT_MAIN", "Land/Naval Unit"),
    ("UT_ASSAULT", "Assault Unit"),
];

pub const MARKER_TYPE: &[(&str, &str)] = &[
    ("BMD_GM_PEASANT_SPAWN", "peasant_spawn"),
    ("BMD_GM_PEASANT_EXIT", "peasant_exit"),
    ("BMD_GM_FRONTEND_CHARACTER", "frontend_character"),
];

pub const SEASONS: &[(&str, &str)] = &[
    ("SEASON_BF_WINTER", "Winter"),
    ("SEASON_BF_SPRING", "Spring"),
    ("SEASON_BF_SUMMER", "Summer"),
    ("SEASON_BF_AUTUMN", "Autumn"),
];

pub const ZONE_PURPOSE: &[(&str, &str)] = &[
    ("EFP_LOW_WALL", "Low wall"),
    ("EFP_HIGH_WALL", "High wall"),
    ("EFP_WINDOW", "Window"),
    ("EFP_OVER_FLOW", "Overflow"),
    ("EFP_MARINES", "Marines"),
    ("EFP_SEAMEN", "Seamen"),
    ("EFP_GUNNERS_OVERFLOW", "Gunners_overflow"),
    ("EFP_CAPTAIN", "Captain"),
    ("EFP_OFFICER1", "Officer1"),
    ("EFP_BOARDING", "Boarding"),
    ("EFP_FIRING_STAND", "Naval_firing_position_stand"),
    ("EFP_FIRING_CROUCH", "Naval_firing_position_crouch"),
    ("EFP_FIRING_STAND_360", "Naval_firing_position_stand_360"),
    ("EFP_PERIMETER", "Naval_perimeter_position"),
    ("EFP_TREE", "Tree"),
    ("EFP_ENTRANCE_DEFENCE", "Entrance Defence"),
    ("EFP_OFFICER2", "Officer2"),
    ("EFP_OFFICER3", "Officer3"),
    ("EFP_CRENEL_LEFT_OUTER", "Crenel left outer"),
    ("EFP_CRENEL_LEFT_INNER", "Crenel left inner"),
    ("EFP_CRENEL_RIGHT_INNER", "Crenel right inner"),
    ("EFP_CRENEL_RIGHT_OUTER", "Crenel right outer"),
    ("EFP_ENGINE_PLACEMENT", "Engine_placement"),
    ("EFP_SECONDARY_ENGINE_PLACEMENT", "Secondary_engine_placement"),
    ("EFP_DISEMBARK_LEFT", "Disembark_left"),
    ("EFP_DISEMBARK_RIGHT", "Disembark_right"),
];

pub const DEPLOYMENT_TYPE: &[(&str, &str)] = &[
    ("EDZT_1V1", "1v1"),
    ("EDZT_1V2", "1v2"),
    ("EDZT_1V3", "1v3"),
    ("EDZT_1V4", "1v4"),
    ("EDZT_2V1", "2v1"),
    ("EDZT_2V2", "2v2"),
    ("EDZT_2V3", "2v3"),
    ("EDZT_2V4", "2v4"),
    ("EDZT_3V1", "3v1"),
    ("EDZT_3V2", "3v2"),
    ("EDZT_3V3", "3v3"),
    ("EDZT_3V4", "3v4"),
    ("EDZT_4V1", "4v1"),
    ("EDZT_4V2", "4v2"),
    ("EDZT_4V3", "4v3"),
    ("EDZT_4V4", "4v4"),
];

/// Not TEd's: the base walls a `WALL_SPLINE` (a procedural wall, docs/plans/procedural_walls.md) is generated from; the keys of
/// `wce_walls::profile::BASES`, in its order.
pub const WALL_BASE: &[(&str, &str)] = &[
    ("barbarian_fort_straight", "8 m: barbarian"),
    ("eastern_fort_small_wall_straight", "8 m: eastern small wall"),
    ("sassanid_fort_small_wall_straight", "8 m: sassanid small wall"),
    ("western_fort_small_wall_straight", "8 m: western small wall"),
    ("khazad_fort_wall_small_straight", "8 m: khazad (TDD)"),
    ("mordor_fort_small_wall_straight", "8 m: mordor (TDD)"),
    ("roman_fort_straight", "15 m: roman"),
    ("eastern_fort_straight", "15 m: eastern"),
    ("sassanid_fort_straight", "15 m: sassanid"),
    ("greek_fort_straight", "15 m: greek"),
    ("egyptian_fort_straight", "15 m: egyptian"),
    ("persian_fort_straight", "15 m: persian"),
    ("easterling_fort_straight", "15 m: easterling (TDD)"),
    ("gondorean_fort_straight", "15 m: gondorean (TDD)"),
    ("gondor_fort_straight", "15 m: gondor (TDD)"),
];
