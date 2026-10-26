//! A tile database's settings written as the Attila kit's `_tile_database/_settings.xml`: what a workspace of Rome 2's tiles holds next
//! to the decompiled entries, for the editor's readers (the climates, the tile sets) and for the compile (BOB's inputs, the merge of an
//! entry into the database).

use rpfm_lib::files::tile_database::TileDatabase;
use wce_formats::c_num::printf_f_msvc;

fn f(v: f32) -> String {
    printf_f_msvc(f64::from(v), 6)
}

/// An attribute value, `"` quoted.
fn esc(v: &str) -> String {
    v.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

/// The settings of `db` as the kit writes them (render and conversion parameters, climates and their textures, tile sets).
pub fn kit_settings_xml(db: &TileDatabase) -> String {
    let r = db.render_params();
    let mut s = String::from("<TILE_DATABASE serialise_version=\"1\">\r\n");
    s += &format!(
        "    <RENDER_PARAMS serialise_version=\"6\" lf_height=\"{}\" hf_height=\"{}\" blend_pixel_scale=\"{}\" unit_scale=\"{}\" mid_distance_detail_scale=\"{}\" \
         mid_distance_detail_strength=\"{}\" mid_distance_normal_strength=\"{}\" mid_distance_detail_near=\"{}\" mid_distance_detail_far=\"{}\" \
         mid_distance_detail_slope_low=\"{}\" mid_distance_detail_slope_high=\"{}\" vertical_offset=\"{}\" normal_lf_scale=\"{}\" normal_tile_scale=\"{}\" \
         normal_terrain_scale=\"{}\" blend_contrast=\"{}\" layer_exempt_0=\"{}\" layer_exempt_1=\"{}\" layer_exempt_2=\"{}\" layer_exempt_3=\"{}\" \
         campaign_sea_transparency_scale=\"{}\" campaign_sea_uv_scale=\"{}\" campaign_lake_transparency_scale=\"{}\" near_distance_detail_distance=\"{}\" \
         near_distance_detail_scale=\"{}\" near_distance_detail_strength=\"{}\" mid_distance_detail_near_lq=\"{}\"/>\r\n",
        f(*r.lf_height()), f(*r.hf_height()), f(*r.blend_pixel_scale()), f(*r.unit_scale()), f(*r.mid_distance_detail_scale()),
        f(*r.mid_distance_detail_strength()), f(*r.mid_distance_normal_strength()), f(*r.mid_distance_detail_near()), f(*r.mid_distance_detail_far()),
        f(*r.mid_distance_detail_slope_low()), f(*r.mid_distance_detail_slope_high()), f(*r.vertical_offset()), f(*r.normal_lf_scale()),
        f(*r.normal_tile_scale()), f(*r.normal_terrain_scale()), f(*r.blend_contrast()), esc(r.layer_exempt_0()), esc(r.layer_exempt_1()),
        esc(r.layer_exempt_2()), esc(r.layer_exempt_3()), f(*r.campaign_sea_transparency_scale()), f(*r.campaign_sea_uv_scale()),
        f(*r.campaign_lake_transparency_scale()), f(*r.near_distance_detail_distance()), f(*r.near_distance_detail_scale()),
        f(*r.near_distance_detail_strength()), f(*r.mid_distance_detail_near_lq())
    );

    let c = db.conversion_params();
    s += &format!("    <CONVERSION_PARAMS serialise_version=\"1\" triangle_density=\"{}\" max_lf_heights_per_pixel=\"{}\"", c.triangle_density(), c.max_lf_heights_per_pixel());
    let size = [
        c.triangle_decimation_size_factors0(), c.triangle_decimation_size_factors1(), c.triangle_decimation_size_factors2(),
        c.triangle_decimation_size_factors3(), c.triangle_decimation_size_factors4(), c.triangle_decimation_size_factors5(),
    ];
    let angle = [
        c.triangle_decimation_angle_factors0(), c.triangle_decimation_angle_factors1(), c.triangle_decimation_angle_factors2(),
        c.triangle_decimation_angle_factors3(), c.triangle_decimation_angle_factors4(), c.triangle_decimation_angle_factors5(),
    ];
    for (k, v) in size.iter().enumerate() {
        s += &format!(" triangle_decimation_size_factors{k}=\"{}\"", f(**v));
    }

    for (k, v) in angle.iter().enumerate() {
        s += &format!(" triangle_decimation_angle_factors{k}=\"{}\"", f(**v));
    }

    s += "/>\r\n    <CLIMATES>\r\n";
    for cl in db.climates() {
        s += &format!(
            "        <CLIMATE serialise_version=\"3\" name=\"{}\" texture_set=\"{}\" red=\"{}\" green=\"{}\" blue=\"{}\" grass_alpha_mul=\"{}\" grass_alpha_add=\"{}\" destruction_climate=\"{}\">\r\n            <TEXTURES>\r\n",
            esc(cl.name()), esc(cl.texture_set()), f(*cl.red()), f(*cl.green()), f(*cl.blue()), f(*cl.grass_alpha_mul()), f(*cl.grass_alpha_add()), esc(cl.destruction_climate())
        );
        for t in cl.textures() {
            s += &format!(
                "                <TEXTURE serialise_version=\"4\" name=\"{}\" mid_distance_strength=\"{}\" lerp_reflectivity0=\"{}\" lerp_smoothness0=\"{}\" lerp_reflectivity1=\"{}\" \
                 lerp_smoothness1=\"{}\" lerp_reflectivity2=\"{}\" lerp_smoothness2=\"{}\" lerp_reflectivity3=\"{}\" lerp_smoothness3=\"{}\" blend_pixel_scale=\"{}\" outfield_blend_pixel_scale=\"{}\"/>\r\n",
                esc(t.name()), f(*t.mid_distance_strength()), f(*t.lerp_reflectivity0()), f(*t.lerp_smoothness0()), f(*t.lerp_reflectivity1()),
                f(*t.lerp_smoothness1()), f(*t.lerp_reflectivity2()), f(*t.lerp_smoothness2()), f(*t.lerp_reflectivity3()), f(*t.lerp_smoothness3()),
                f(*t.blend_pixel_scale()), f(*t.outfield_blend_pixel_scale())
            );
        }

        s += "            </TEXTURES>\r\n        </CLIMATE>\r\n";
    }

    s += "    </CLIMATES>\r\n    <TILE_SETS>\r\n";
    for t in db.tile_sets() {
        s += &format!(
            "        <TILE_SET serialise_version=\"1\" name=\"{}\" linking_tile=\"{}\" shared_geometry=\"{}\" also_place_tile_set=\"{}\" link_as_set=\"{}\" red=\"{}\" green=\"{}\" blue=\"{}\"/>\r\n",
            esc(t.name()), esc(t.linking_tile()), esc(t.shared_geometry()), esc(t.also_place_tile_set()), esc(t.link_as_set()), f(*t.red()), f(*t.green()), f(*t.blue())
        );
    }

    s + "    </TILE_SETS>\r\n</TILE_DATABASE>\r\n"
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_kit_reads_the_settings_back() {
        let mut db = TileDatabase::default();
        let mut set = rpfm_lib::files::tile_database::tile_set::TileSet::default();
        set.set_serialise_version(1);
        set.set_name("river".to_owned());
        set.set_linking_tile("terrain\\tiles\\battle\\river\\1x1_blend\\aa".to_owned());
        db.tile_sets_mut().push(set);
        let mut climate = rpfm_lib::files::tile_database::climate::Climate::default();
        climate.set_serialise_version(3);
        climate.set_name("ROME_Med".to_owned());
        db.climates_mut().push(climate);
        let read = TileDatabase::from_assembly_kit(&kit_settings_xml(&db), &[], None, false).unwrap();
        assert_eq!(read.tile_sets(), db.tile_sets());
        assert_eq!(read.climates()[0].name(), "ROME_Med");
    }
}
