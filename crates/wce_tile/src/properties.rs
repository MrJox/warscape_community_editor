//! The properties TEd's Property Editor shows for each kind of instance of tile.ted, and how a value is read and written.
//!
//! Read from TEd.AssemblyKit.exe (docs/formats/tile_ted.md, "Property Editor"): every `Ted::*_INSTANCE` class has a property list (names, in the
//! order of the grid), a type per property, and a `property_flags` function that hides (8), disables (1 / 2) a property, sometimes by a condition.
//! The types decide the widget: a ranged float is a spin box with a slider (`TRANGED_VALUE<float>`: the float type `[0, 1]` of opacity, dirt_level,
//! burn_amount and glow_amount; others are `[0, 5]`, `[0, 100]`, `[0, 1000]`, an angle in radians shown in degrees ...), a bool a check box, an enum
//! a combo box (its entries are TEd's own tables, [`crate::property_enums`]), a colour a colour button, a string a line edit, `destruction_level` an
//! int slider whose maximum is the building's number of destruction levels - 1. A property whose type has no widget (`behaviour_delegate`, a mesh
//! handle, a command like `recenter_polygon`) is not in the grid.
//!
//! The file stores a property as an attribute of the same name (`name`, `lf_offset` and a few helper attributes are not properties).

use crate::property_enums as en;
use crate::ted_scene::{format_ted_number, parse_vec3};

/// How a property is edited, and what its text looks like.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Kind {
    /// ` OIF_VISIBLE|OIF_ACTIVE|OIF_DRAGGABLE` as three check boxes.
    Flags,

    /// ` SEASON_BF_WINTER|SEASON_BF_SPRING|SEASON_BF_SUMMER|SEASON_BF_AUTUMN` as four check boxes.
    Seasons,

    /// `MSM_LANDSCAPE` / `MSM_FREEFORM`: "Landscape: Above" / "Freeform"; the edit snaps the instance (the placement rules of the tool).
    Placement,

    /// `vector(x,y,z)`: the instance's position, in the scene's units (the tool's own row edits the world position in metres).
    Position,

    /// `euler(a,b,c)` in radians, shown in degrees.
    Orientation,

    Bool,

    /// A float in `[min, max]` with a slider (`TRANGED_VALUE<float>`).
    Slider(f32, f32),

    /// A float without a range.
    Float,

    /// `vector(x,y,z)`, three number boxes.
    Vec3,

    /// `vector(x,y)`.
    Vec2,

    /// A float in radians in `[0, max]`, shown in degrees (a slider).
    Angle(f32),

    /// `argb(r,g,b,a)` (the order is r, g, b, a in spite of the name).
    Colour,

    /// One of TEd's enum entries (file name, display name).
    Enum(&'static [(&'static str, &'static str)]),

    /// A line edit.
    Text,

    /// An int in `[min, max]`.
    Int(i32, i32),

    /// The building's destruction level: an int slider in `[0, levels - 1]`.
    DestructionLevel,

    /// A float the grid shows but does not let anyone change (`area`).
    ReadOnlyFloat,
}

/// When a property is in the grid (TEd's `property_flags`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Rule {
    Normal,

    /// Greyed out, the value is shown.
    ReadOnly,

    /// Never in the grid (`lf_offset`, `behaviour_delegate`, and what only a command changes).
    Hidden,

    /// Only for a building of the category `fort_wall` (`BUILDING_INSTANCE::property_flags` of TEd: `weak_wall`, `ai_breachable`).
    FortWall,

    /// For `fort_wall` and `unbreachable_fort_wall` (`dockable`).
    FortWallOrUnbreachable,

    /// `radius` of a polygon: in the grid for a circle or semicircle, hidden for the other shapes (`+0x13c` is `polygon_shape`).
    CircleShape,
}

#[derive(Clone, Copy, Debug)]
pub struct Prop {
    /// The attribute, and the name TEd shows.
    pub attr: &'static str,
    pub kind: Kind,
    pub rule: Rule,

    /// The attribute `attr` is the same property as (`generated` and `is_generated` in an AI hint polygon: the file keeps both).
    pub twin: Option<&'static str>,
}

const fn p(attr: &'static str, kind: Kind) -> Prop {
    Prop { attr, kind, rule: Rule::Normal, twin: None }
}

const fn ro(attr: &'static str, kind: Kind) -> Prop {
    Prop { attr, kind, rule: Rule::ReadOnly, twin: None }
}

const fn ruled(attr: &'static str, kind: Kind, rule: Rule) -> Prop {
    Prop { attr, kind, rule, twin: None }
}

/// Flags, surface_mode, position and orientation of a `BASE_3D_INSTANCE`.
macro_rules! base3d {
    ($($rest:expr),* $(,)?) => {
        &[
            p("flags", Kind::Flags), p("surface_mode", Kind::Placement), p("position", Kind::Position), p("orientation", Kind::Orientation),
            $($rest),*
        ]
    };
}

/// The polygon family (`POLYGON_LIST_INSTANCE_BASE`): `polygon_shape`, `area` and `orientation` are read-only, `radius` is for circles only.
macro_rules! polygon {
    ($($rest:expr),* $(,)?) => {
        &[
            p("flags", Kind::Flags), p("surface_mode", Kind::Placement), p("position", Kind::Position), ro("orientation", Kind::Orientation),
            ro("polygon_shape", Kind::Enum(en::POLYGON_SHAPE)), p("selected_line_colour", Kind::Colour), p("line_colour", Kind::Colour),
            ruled("radius", Kind::Float, Rule::CircleShape), ro("area", Kind::ReadOnlyFloat),
            $($rest),*
        ]
    };
}

const BUILDING: &[Prop] = base3d![
    p("key", Kind::Text),
    p("opacity", Kind::Slider(0.0, 1.0)),
    p("dirt_level", Kind::Slider(0.0, 1.0)),
    p("bounds_colour", Kind::Colour),
    p("burn", Kind::Bool),
    p("burn_amount", Kind::Slider(0.0, 1.0)),
    p("glow_amount", Kind::Slider(0.0, 1.0)),
    p("destruction_level", Kind::DestructionLevel),
    p("building_tech_level", Kind::Enum(en::TECH_LEVEL)),
    ruled("weak_wall", Kind::Bool, Rule::FortWall),
    ruled("ai_breachable", Kind::Bool, Rule::FortWall),
    p("indestructible", Kind::Bool),
    ruled("dockable", Kind::Bool, Rule::FortWallOrUnbreachable),
    p("toggleable", Kind::Bool),
    p("capture_location", Kind::Text),
];

const SIMPLE_MESH: &[Prop] = base3d![
    p("scale", Kind::Vec3),
    p("key", Kind::Text),
    p("opacity", Kind::Slider(0.0, 1.0)),
    p("burn", Kind::Bool),
    p("burn_amount", Kind::Slider(0.0, 1.0)),
    p("glow_amount", Kind::Slider(0.0, 1.0)),
    p("dirt_level", Kind::Slider(0.0, 1.0)),
    p("bounds_colour", Kind::Colour),
    p("show_collision", Kind::Bool),
    p("seasons", Kind::Seasons),
    p("visible_inside_snow_zone", Kind::Bool),
    p("visible_outside_snow_zone", Kind::Bool),
    p("visible_inside_destruction_zone", Kind::Bool),
    p("visible_outside_destruction_zone", Kind::Bool),
];

const DECAL: &[Prop] = base3d![p("scale", Kind::Vec3), p("parallax_scale", Kind::Float), p("tiled", Kind::Bool)];

const VEGETATION: &[Prop] = base3d![p("key", Kind::Text)];

const VFX: &[Prop] = base3d![
    p("vfx", Kind::Text),
    p("name", Kind::Text),
    p("autoplay", Kind::Bool),
    p("use_in_outfield", Kind::Bool),
    p("scale", Kind::Float),
    p("seasons", Kind::Seasons),
];

const POINT_LIGHT: &[Prop] = base3d![
    p("colour", Kind::Colour),
    p("colour_scale", Kind::Slider(0.0, 5.0)),
    p("radius", Kind::Slider(0.0, 1000.0)),
    p("animation_type", Kind::Enum(en::LIGHT_ANIMATION)),
    p("animation_speed_scale", Kind::Vec2),
    p("colour_min", Kind::Slider(0.0, 100.0)),
    p("random_offset", Kind::Slider(0.0, 100.0)),
    p("falloff_type", Kind::Enum(en::LIGHT_FALLOFF)),
];

const GROUP: &[Prop] = base3d![p("low_frequency_change_effect", Kind::Enum(en::LOW_FREQUENCY_CHANGE))];

/// A layer: only the flags, the position and the export type are in the grid (`LayerInstance::property_flags`).
const LAYER: &[Prop] = &[
    p("flags", Kind::Flags),
    p("position", Kind::Position),
    p("export_type", Kind::Enum(en::EXPORT_TYPE)),
];

const CAPTURE_LOCATION: &[Prop] = base3d![
    p("type", Kind::Enum(en::CAPTURE_TYPE)),
    p("database_type", Kind::Text),
    p("flag_position", Kind::Vec3),
    p("min_num_players", Kind::Int(0, 100)),
    p("max_num_players", Kind::Int(0, 100)),
    p("flag_facing", Kind::Angle(std::f32::consts::TAU)),
];

const SIEGE_EDGE: &[Prop] = base3d![p("type", Kind::Enum(en::SIEGE_EDGE_TYPE))];
const SIEGE_NODE: &[Prop] = base3d![p("type", Kind::Enum(en::SIEGE_NODE_TYPE))];

const DEPLOYMENT_GROUP: &[Prop] = base3d![
    p("facing_direction", Kind::Angle(std::f32::consts::TAU)),
    p("deployment_category", Kind::Enum(en::DEPLOYMENT_CATEGORY)),
    p("deployment_type", Kind::Enum(en::DEPLOYMENT_TYPE)),
    p("deployment_zone_id", Kind::Int(0, 100)),
    p("alliance_id", Kind::Int(0, 1)),
    p("new_polygon_type", Kind::Enum(en::NEW_POLYGON_TYPE)),
];

const TERRAIN_LINE: &[Prop] = base3d![p("colour_1", Kind::Colour), p("colour_2", Kind::Colour), p("target", Kind::Vec3)];

const SPLINE: &[Prop] = base3d![p("step_size", Kind::Slider(1.0, 100.0)), ro("select_all_points", Kind::ReadOnlyFloat)];

const POLYGON: &[Prop] = polygon![p("subdivision_length", Kind::Slider(0.0, 100.0)), p("max_subdivisions", Kind::Int(0, 100))];

const AI_HINT: &[Prop] = polygon![
    p("subdivision_length", Kind::Slider(0.0, 100.0)),
    p("max_subdivisions", Kind::Int(0, 100)),
    Prop { attr: "generated", kind: Kind::Bool, rule: Rule::Normal, twin: Some("is_generated") },
    p("hint_type", Kind::Enum(en::AI_HINT_TYPE)),
];

const DEPLOYMENT_POLYGON: &[Prop] = polygon![
    p("subdivision_length", Kind::Slider(0.0, 100.0)),
    p("max_subdivisions", Kind::Int(0, 100)),
    p("deployment_polygon_type", Kind::Enum(en::DEPLOYMENT_POLYGON_TYPE)),
];

const ZONE_POLYGON: &[Prop] = polygon![
    p("subdivision_length", Kind::Slider(0.0, 100.0)),
    p("max_subdivisions", Kind::Int(0, 100)),
    p("locked", Kind::Bool),
    p("rank_count", Kind::Int(0, 100)),
    p("rank_distance", Kind::Slider(0.0, 100.0)),
    p("zone_skirt_distance", Kind::Slider(0.0, 100.0)),
    p("purpose", Kind::Enum(en::ZONE_PURPOSE)),
];

const SEPARATOR: &[Prop] = polygon![
    p("subdivision_length", Kind::Slider(0.0, 100.0)),
    p("max_subdivisions", Kind::Int(0, 100)),
    p("type", Kind::Enum(en::SEPARATOR_TYPE)),
];

const CIVILIAN_ZONE: &[Prop] = polygon![p("civilian_type", Kind::Text), p("civilian_amount", Kind::Text)];

/// The properties of an element of `tag`, in the order of TEd's grid; `None` for elements the grid does not edit.
pub fn schema(tag: &str) -> Option<&'static [Prop]> {
    Some(match tag {
        "BUILDING_INSTANCE" => BUILDING,
        "SIMPLE_MESH_INSTANCE" => SIMPLE_MESH,
        "ProjectedDecalInstance" => DECAL,
        "VEGETATION_INSTANCE" => VEGETATION,
        "VFXInstance" => VFX,
        "POINT_LIGHT_INSTANCE" => POINT_LIGHT,
        "UserInstanceGroup" | "INSTANCE_GROUP" => GROUP,
        "LayerInstance" => LAYER,
        "CaptureLocationInstance" => CAPTURE_LOCATION,
        "SiegeAIEdgeInstance" => SIEGE_EDGE,
        "SiegeAINodeInstance" => SIEGE_NODE,
        "DeploymentZonePolygonInstanceGroup" => DEPLOYMENT_GROUP,
        "CivilianShelter" => TERRAIN_LINE,
        "SEGMENTED_SPLINE_INSTANCE" | "SplineRiverInstance" => SPLINE,
        "AI_HINT_POLYGON_INSTANCE" => AI_HINT,
        "DEPLOYMENT_ZONE_POLYGON_INSTANCE" => DEPLOYMENT_POLYGON,
        "ZONE_POLYGON_INSTANCE" => ZONE_POLYGON,
        "AISeparatorInstance" => SEPARATOR,
        "CivilianDeploymentZone" => CIVILIAN_ZONE,

        // The polygon family that has nothing of its own in TEd (no-go, go region, siege AI, vegetation exclusion, lite building zone, capture
        // location boundaries): `POLYGON_LIST_INSTANCE_SUBDIVIDED_PROJECTED`.
        "NOGO_REGION_POLYGON_INSTANCE" | "GO_REGION_POLYGON_INSTANCE" | "SIEGE_AI_POLYGON_INSTANCE" | "VegetationExclusionPolygonInstance"
        | "LiteBuildingZone" | "CaptureLocationPolygonInstance" | "EF_POLYGON_INSTANCE" => POLYGON,
        _ => return None,
    })
}

/// What decides a rule: the building's category (`battlefield_buildings` of the kit's database) and the polygon's shape.
#[derive(Clone, Debug, Default)]
pub struct RuleContext<'a> {
    pub category: Option<&'a str>,
    pub shape: Option<&'a str>,

    /// The element has this attribute already (a file written by TEd has `weak_wall` for the buildings that TEd shows it for, and no other).
    pub present: bool,
}

/// Whether the property is in the grid and whether it can be edited.
pub fn access(prop: &Prop, ctx: &RuleContext) -> Access {
    match prop.rule {
        Rule::Normal => Access::Edit,
        Rule::ReadOnly => Access::ReadOnly,
        Rule::Hidden => Access::Hidden,
        Rule::FortWall => match ctx.category {
            Some(c) => if c == "fort_wall" { Access::Edit } else { Access::Hidden },
            None => if ctx.present { Access::Edit } else { Access::Hidden },
        },

        Rule::FortWallOrUnbreachable => match ctx.category {
            Some(c) => if c == "fort_wall" || c == "unbreachable_fort_wall" { Access::Edit } else { Access::Hidden },
            None => if ctx.present { Access::Edit } else { Access::Hidden },
        },

        Rule::CircleShape => match ctx.shape {
            Some("PS_CIRCLE" | "PS_SEMICIRCLE") => Access::ReadOnly,
            _ => Access::Hidden,
        },
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Access {
    Edit,
    ReadOnly,
    Hidden,
}

// ---- values

/// `true` / `false`; anything else reads as false.
pub fn parse_bool(text: &str) -> bool {
    matches!(text.trim(), "true" | "1")
}

pub fn format_bool(v: bool) -> &'static str {
    if v { "true" } else { "false" }
}

/// A number the way the grid shows it; `None` for text that is not one.
pub fn parse_float(text: &str) -> Option<f32> {
    text.trim().parse::<f32>().ok()
}

pub fn parse_int(text: &str) -> Option<i64> {
    let t = text.trim();
    t.parse::<i64>().ok().or_else(|| t.parse::<f64>().ok().map(|v| v.round() as i64))
}

/// `vector(x,y)`.
pub fn parse_vec2(text: &str) -> Option<[f32; 2]> {
    let inner = text.trim().strip_prefix("vector(")?.strip_suffix(')')?;
    let (a, b) = inner.split_once(',')?;
    Some([a.trim().parse().ok()?, b.trim().parse().ok()?])
}

pub fn format_vec2(v: [f32; 2]) -> String {
    format!("vector({},{})", format_ted_number(f64::from(v[0])), format_ted_number(f64::from(v[1])))
}

pub fn parse_vec3_text(text: &str) -> Option<[f32; 3]> {
    parse_vec3(text, "vector").map(|v| [v.x as f32, v.y as f32, v.z as f32])
}

pub fn format_vec3(v: [f32; 3]) -> String {
    format!("vector({},{},{})", format_ted_number(f64::from(v[0])), format_ted_number(f64::from(v[1])), format_ted_number(f64::from(v[2])))
}

/// `argb(r,g,b,a)`.
pub fn parse_colour(text: &str) -> Option<[f32; 4]> {
    let inner = text.trim().strip_prefix("argb(")?.strip_suffix(')')?;
    let mut it = inner.split(',').map(|x| x.trim().parse::<f32>());
    let c = [it.next()?.ok()?, it.next()?.ok()?, it.next()?.ok()?, it.next()?.ok()?];
    it.next().is_none().then_some(c)
}

/// A colour component: the shortest text of the float32 with at least one decimal (`1.0`, `0.74902`).
fn colour_number(v: f32) -> String {
    let s = format_ted_number(f64::from(v));
    if s.contains('.') || s.contains('e') || s.contains("inf") || s.contains("nan") { s } else { format!("{s}.0") }
}

pub fn format_colour(c: [f32; 4]) -> String {
    format!("argb({},{},{},{})", colour_number(c[0]), colour_number(c[1]), colour_number(c[2]), colour_number(c[3]))
}

pub fn format_float(v: f32) -> String {
    format_ted_number(f64::from(v))
}

/// The seasons that are set; every season when the attribute is missing.
pub fn parse_seasons(text: &str) -> [bool; 4] {
    let mut on = [false; 4];
    for t in text.split('|').map(str::trim) {
        if let Some(i) = en::SEASONS.iter().position(|(f, _)| *f == t) {
            on[i] = true;
        }
    }

    on
}

/// TEd's text: a leading space and the seasons in the order winter, spring, summer, autumn.
pub fn format_seasons(on: [bool; 4]) -> String {
    let names = en::SEASONS.iter().zip(on).filter(|(_, o)| *o).map(|((f, _), _)| *f).collect::<Vec<_>>();
    format!(" {}", names.join("|"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn colours_keep_one_decimal_like_ted() {
        assert_eq!(format_colour([1.0, 0.0, 0.0, 1.0]), "argb(1.0,0.0,0.0,1.0)");
        assert_eq!(format_colour([1.0, 0.74902, 0.545098, 1.0]), "argb(1.0,0.74902,0.545098,1.0)");
        assert_eq!(format_colour([0.65, 0.0, 0.0, 1.0]), "argb(0.65,0.0,0.0,1.0)");
        assert_eq!(parse_colour("argb(1.0,0.74902,0.545098,1.0)"), Some([1.0, 0.74902, 0.545098, 1.0]));
        assert_eq!(parse_colour("argb(1.0,0.0,0.0)"), None);
    }

    #[test]
    fn seasons_round_trip() {
        let all = " SEASON_BF_WINTER|SEASON_BF_SPRING|SEASON_BF_SUMMER|SEASON_BF_AUTUMN";
        assert_eq!(parse_seasons(all), [true; 4]);
        assert_eq!(format_seasons([true; 4]), all);
        assert_eq!(format_seasons([false, true, true, true]), " SEASON_BF_SPRING|SEASON_BF_SUMMER|SEASON_BF_AUTUMN");
        assert_eq!(format_seasons([false; 4]), " ");
    }

    #[test]
    fn vectors_round_trip() {
        assert_eq!(format_vec2([1.0, 2.5]), "vector(1,2.5)");
        assert_eq!(parse_vec2("vector(1,2.5)"), Some([1.0, 2.5]));
        assert_eq!(format_vec3([0.0, 17.0, -3.25]), "vector(0,17,-3.25)");
        assert_eq!(parse_vec3_text("vector(0,17,-3.25)"), Some([0.0, 17.0, -3.25]));
    }

    #[test]
    fn building_rules_follow_the_category() {
        let b = schema("BUILDING_INSTANCE").unwrap();
        let weak = b.iter().find(|x| x.attr == "weak_wall").unwrap();
        let dock = b.iter().find(|x| x.attr == "dockable").unwrap();
        let wall = RuleContext { category: Some("fort_wall"), ..Default::default() };
        let unb = RuleContext { category: Some("unbreachable_fort_wall"), ..Default::default() };
        let generic = RuleContext { category: Some("generic"), ..Default::default() };
        assert_eq!((access(weak, &wall), access(dock, &wall)), (Access::Edit, Access::Edit));
        assert_eq!((access(weak, &unb), access(dock, &unb)), (Access::Hidden, Access::Edit));
        assert_eq!((access(weak, &generic), access(dock, &generic)), (Access::Hidden, Access::Hidden));
    }

    #[test]
    fn every_schema_starts_like_teds_grid() {
        for tag in ["BUILDING_INSTANCE", "VEGETATION_INSTANCE", "POINT_LIGHT_INSTANCE", "AI_HINT_POLYGON_INSTANCE", "LayerInstance"] {
            let s = schema(tag).unwrap();
            assert_eq!(s[0].attr, "flags", "{tag}");
        }

        assert!(schema("NoSuchInstance").is_none());
    }
}
