//! The kinds of the attributes of Attila's TEd files, from a survey of every tile.ted, river spline and tile database entry of the
//! Assembly Kit (docs/formats/tile_ted.md, "Attribute kinds"). An attribute's kind follows its name in every class, except `position`
//! (a `vector(...)` on instances, `x,y,z` on polygon points) and `scale` (a vector on decals and simple meshes, a number on trees and VFX).
//! Integers that are counts, ids, seeds and command placeholders are `Int`; every other number is a float, written TEd's way, except
//! `lf_offset`, the spline control points and the point positions, which TEd writes with `%g`. Also the editor's own `WALL_SPLINE`
//! (`wall_base`, `wall_tolerance`, `wall_flip`, its points' `sharp`; docs/plans/procedural_walls.md), which no kit file has.

use wce_tile_model::Sym;

use crate::xml::values::{Kind, Num, Schema};

const VECTOR: Kind = Kind::Vec3(Num::Ted, "vector");
const FLOAT: Kind = Kind::Float(Num::Ted);
const FLAGS: Kind = Kind::Flags { leading_space: true };

/// tile.ted.
pub struct TedSchema;

impl Schema for TedSchema {
    fn kind(&self, class: Sym, attr: Sym) -> Option<Kind> {
        let class = class.as_str();
        Some(match attr.as_str() {
            "position" if class == "point" => Kind::Vec3(Num::G, ""),
            "scale" if matches!(class, "VEGETATION_INSTANCE" | "VFXInstance") => FLOAT,
            "position" | "scale" | "target" | "flag_position" => VECTOR,
            "orientation" => Kind::Euler(Num::Ted),
            "animation_speed_scale" => Kind::Vec2(Num::Ted, "vector"),
            "controlIn" | "controlOut" => Kind::Vec3(Num::G, ""),
            "bounds_colour" | "colour" | "colour_1" | "colour_2" | "line_colour" | "selected_line_colour" => Kind::Colour,
            "flags" | "seasons" => FLAGS,
            "lf_offset" => Kind::Float(Num::G),

            "ai_breachable" | "autoplay" | "burn" | "dockable" | "exportable" | "generated" | "head_bob" | "indestructible" | "is_generated"
            | "locked" | "orthographic" | "show_collision" | "smoothed" | "terrain_collision" | "tiled" | "toggleable" | "use_in_outfield"
            | "visible_inside_destruction_zone" | "visible_inside_snow_zone" | "visible_outside_destruction_zone" | "visible_outside_snow_zone"
            | "weak_wall" | "wall_flip" | "sharp" => Kind::Bool,

            "add_boundary" | "add_polygon" | "alliance_id" | "closed" | "decal_generation_seed" | "deployment_zone_id" | "destruction_level"
            | "flatten" | "generation_seed" | "max_num_players" | "max_subdivisions" | "min_num_players" | "polygon_closed" | "rank_count"
            | "recenter_polygon" | "select_all_points" | "version" | "vfx_generation_seed" | "max_shelter_capacity" => Kind::Int,

            "alpha_fade" | "area" | "aspect_ratio" | "burn_amount" | "colour_min" | "colour_scale" | "dirt_level" | "DOF_blur_size"
            | "facing_direction" | "far_distance" | "flag_facing" | "flow_speed" | "foam_amount" | "focal_length" | "glow_amount"
            | "ground_height" | "near_distance" | "opacity" | "orthographic_height" | "parallax_scale" | "radius" | "random_offset"
            | "rank_distance" | "roll" | "step_size" | "subdivision_length" | "terrain_offset" | "vertical_fov" | "width"
            | "zone_skirt_distance" | "wall_tolerance" => FLOAT,

            "animation_type" | "building_tech_level" | "capture_location" | "civilian_amount" | "civilian_type" | "database_type"
            | "deployment_category" | "deployment_polygon_type" | "deployment_type" | "DOF_focus" | "export_type" | "falloff_type"
            | "hint_type" | "key" | "low_frequency_change_effect" | "movement_mode" | "name" | "new_polygon_type" | "polygon_shape"
            | "purpose" | "season_mask" | "surface_mode" | "type" | "vfx" | "wall_base" => Kind::Str,
            _ => return None,
        })
    }
}

/// river_spline<n>.xml: `SEGMENTED_SPLINE` > `SPLINE` > `POINT0..3`, every number `%g`.
pub struct RiverSchema;

impl Schema for RiverSchema {
    fn kind(&self, _: Sym, attr: Sym) -> Option<Kind> {
        match attr.as_str() {
            "x" | "y" | "z" | "width" | "foam_amount" | "alpha_fade" | "flow_speed" | "terrain_offset" => Some(Kind::Float(Num::G)),
            _ => None,
        }
    }
}

/// _tile_database/TILES/*.xml (written by BOB / the kit's tools: `%.6f` numbers, `true` / `false`).
pub struct EntrySchema;

impl Schema for EntrySchema {
    fn kind(&self, class: Sym, attr: Sym) -> Option<Kind> {
        Some(match attr.as_str() {
            // TEXTURE_GROUP names the blend layers; TILE and VARIATION have a colour.
            "red" | "green" | "blue" | "alpha" if class == "TEXTURE_GROUP" => Kind::Str,
            "red" | "green" | "blue" => Kind::Float(Num::Fixed6),
            "min_height" | "scale" | "normal_strength" | "overlap_border_size" => Kind::Float(Num::Fixed6),
            "serialise_version" | "width" | "height" | "raw_data_tri_density" | "x" | "y" | "base_x" | "base_y" | "blend_size" => Kind::Int,
            "requires_infield_lodding" | "random_rotatable" | "scalable" | "encampable" | "barbarian" | "is_entry" | "no_offline_blend" => {
                Kind::Bool
            }

            "location" | "tile_set" | "mask" | "custom_alpha_blend_texture" | "custom_blend_tile" | "blend_common" | "index_common"
            | "normal_common" | "link_set" | "test" => Kind::Str,
            _ => return None,
        })
    }
}
