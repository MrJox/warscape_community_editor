//! Tests of the tile database entry, climates and index on a fake raw_data folder. `tile_dump` compares them with the C++ code over a kit.

use std::fs;
use std::path::Path;

use wce_formats::c_num::{strtod, strtol};
use wce_formats::tile_climate::{Climate, ClimateList, climate_texture_path};
use wce_formats::tile_entry::{TileEntry, normalize_location};
use wce_formats::tile_index::TileIndex;

const ENTRY: &str = "<TILE serialise_version='3' location='terrain\\tiles\\battle\\set_a\\tile_b\\' tile_set='set_a' width='3' height='2' red='12.5' green='x' blue=' 7'>\r\n\
\t<TEXTURE_GROUP red='rock' green='grass' blue='mud' alpha='forest'/>\r\n\
\t<VARIATIONS>\r\n\t\t<VARIATION serialise_version='2' location='Minor' min_height='-10.5' normal_strength='1'/>\r\n\
\t\t<VARIATION location='major/inner' scale='50' raw_data_tri_density='4294967295' barbarian='true'/>\r\n\t</VARIATIONS>\r\n\
\t<TILE_LINK_TARGETS>\r\n\t\t<TILE_LINK_TARGET tile_set='sea' x='-1' y='2'/>\r\n\t</TILE_LINK_TARGETS>\r\n\
\t<TILE_LINKS>\r\n\t\t<TILE_LINK link_set='roads' x='3' y='1' is_entry='true' test='TLT_EQUALS'>\r\n\t\t\t<blend_quad/>\r\n\t\t</TILE_LINK>\r\n\t</TILE_LINKS>\r\n\
\t<EXTRA odd='1'/>\r\n</TILE>\r\n";

#[test]
fn c_numbers() {
    assert_eq!(strtod(" 12.5abc"), Some(12.5));
    assert_eq!(strtod("-.5"), Some(-0.5));
    assert_eq!(strtod("5."), Some(5.0));
    assert_eq!(strtod("1e5x"), Some(1e5));
    assert_eq!(strtod("1e"), Some(1.0));
    assert_eq!(strtod("+2"), Some(2.0));
    assert_eq!(strtod("."), None);
    assert_eq!(strtod("x1"), None);
    assert!(strtod("-inf").is_some_and(|v| v == f64::NEG_INFINITY));
    assert_eq!(strtol("12.9"), Some(12));
    assert_eq!(strtol(" -7x"), Some(-7));
    assert_eq!(strtol("x"), None);
    assert_eq!(strtol("99999999999999999999"), Some(i64::MAX));
}

#[test]
fn entry_typed_access() {
    let t = TileEntry::parse(ENTRY.as_bytes().to_vec()).unwrap();
    assert_eq!(t.save(), ENTRY.as_bytes());
    assert_eq!((t.kind().as_str(), t.width(), t.height(), t.serialise_version()), ("battle", 3, 2, 3));
    assert_eq!((t.red(), t.green(), t.blue()), (12.5, 0.0, 7.0));
    assert_eq!(t.texture_group().alpha, "forest");

    let vs = t.variations();
    assert_eq!((vs[0].location.as_str(), vs[0].min_height, vs[0].scale), ("Minor", -10.5, 100.0));
    assert_eq!((vs[1].scale, vs[1].raw_data_tri_density, vs[1].barbarian), (50.0, u32::MAX, true));
    assert_eq!(t.find_variation("MINOR/"), Some(0));
    assert_eq!(t.find_variation("nope"), None);
    assert_eq!(t.link_targets()[0].x, -1);
    assert!(t.links()[0].is_entry && t.links()[0].has_blend_quad);
    assert_eq!(t.unknown_names(), ["TILE/EXTRA"]);

    let raw = Path::new("/kit/raw_data");
    assert_eq!(t.variation_folder(raw, 1), raw.join("terrain/tiles/battle/set_a/tile_b/major/inner"));
    assert_eq!(t.ted_path(raw, 0), raw.join("terrain/tiles/battle/set_a/tile_b/Minor/tile.ted"));
    assert_eq!(normalize_location("Terrain\\Tiles\\X//"), "terrain/tiles/x");
    assert!(TileEntry::parse(b"<NOT_A_TILE/>".to_vec()).is_err());
}

#[test]
fn climates() {
    let xml = b"<SETTINGS><CLIMATES><CLIMATE name='ROME_Med' texture_set='med'><TEXTURES>\
        <TEXTURE name='rock' blend_pixel_scale='20' lerp_smoothness2='0.5' mid_distance_strength=''/></TEXTURES></CLIMATE></CLIMATES>\
        <CLIMATE name='not_in_the_list'/></SETTINGS>";
    let list = ClimateList::parse(xml).unwrap();
    assert_eq!(list.climates.len(), 1);
    let climate = list.find("rome_med").unwrap();
    assert_eq!(climate.folder(), "terrain/textures/battle/rome_med");
    let rock = climate.find("rock").unwrap();
    assert_eq!((rock.blend_pixel_scale, rock.mid_distance_strength, rock.lerp_smoothness), (20.0, 1.0, [0.1, 0.1, 0.5, 0.1]));

    let exists = |p: &str| p == "terrain/textures/battle/default/rock/2_su_normal.dds";
    assert_eq!(climate_texture_path(&climate.folder(), "rock", 2, "wi", true, exists).as_deref(), Some("terrain/textures/battle/default/rock/2_su_normal.dds"));
    assert_eq!(climate_texture_path(&climate.folder(), "rock", 1, "wi", true, exists), None);
}

#[test]
fn climates_of_a_mod_over_the_kit() {
    let kit = ClimateList::parse(
        b"<SETTINGS><RENDER_PARAMS blend_pixel_scale='9'/><CLIMATES>\
        <CLIMATE name='default' texture_set='grass'><TEXTURES><TEXTURE name='rock' blend_pixel_scale='40'/></TEXTURES></CLIMATE>\
        <CLIMATE name='ROME_Med' texture_set='med'><TEXTURES><TEXTURE name='rock' blend_pixel_scale='20'/><TEXTURE name='mud' blend_pixel_scale='9'/></TEXTURES></CLIMATE>\
        </CLIMATES></SETTINGS>",
    ).unwrap();
    let modded = ClimateList::parse(
        b"<SETTINGS><CLIMATES><CLIMATE name='ROME_Med'><TEXTURES>\
        <TEXTURE name='mordor_gravel' blend_pixel_scale='12'/><TEXTURE name='rock' blend_pixel_scale='25'/></TEXTURES></CLIMATE>\
        <CLIMATE name='ROME_New'/></CLIMATES></SETTINGS>",
    ).unwrap();

    let list = modded.over(kit);
    let names = list.climates.iter().map(|c| c.name.as_str()).collect::<Vec<_>>();
    assert_eq!(names, ["ROME_Med", "ROME_New", "default"]);
    let med = list.find("rome_med").unwrap();
    assert_eq!(med.texture_set, "med");
    let scale = |g: &str| med.find(g).map(|t| t.blend_pixel_scale);
    assert_eq!((scale("mordor_gravel"), scale("rock"), scale("mud")), (Some(12.0), Some(25.0), Some(9.0)));
    assert_eq!(list.blend_pixel_scale, Some(9.0));
}

#[test]
fn campaign_climates_use_the_campaign_textures() {
    let list = ClimateList { kind: "campaign".to_owned(), climates: vec![Climate { name: "Europe".to_owned(), ..Default::default() }], ..Default::default() };
    assert_eq!(list.default_folder(), "terrain/textures/campaign/default");
    assert_eq!(list.folder_named("europe"), "terrain/textures/campaign/europe");
    assert_eq!(list.folder_named("nowhere"), "terrain/textures/campaign/default");

    let exists = |p: &str| p == "terrain/textures/campaign/default/mordor_a/1_su_diffuse.dds";
    assert_eq!(list.texture_path(&list.folder_named("europe"), "mordor_a", 1, "au", false, exists).as_deref(), Some("terrain/textures/campaign/default/mordor_a/1_su_diffuse.dds"));
    assert_eq!(ClimateList::default().default_folder(), "terrain/textures/battle/default");
}

#[test]
fn index_with_cache() {
    let raw = tempfile::tempdir().unwrap();
    let tiles = raw.path().join("terrain/tiles/battle/_tile_database/TILES");
    fs::create_dir_all(&tiles).unwrap();
    fs::write(tiles.join("set_a_tile_b.xml"), ENTRY).unwrap();
    fs::write(tiles.join("broken.xml"), "<TILE").unwrap();
    let cache = raw.path().join("cache/index.tsv");

    let cold = TileIndex::build(raw.path(), Some(&cache));
    assert_eq!((cold.parsed, cold.from_cache, cold.problems.len()), (1, 0, 1));
    let warm = TileIndex::build(raw.path(), Some(&cache));
    assert_eq!((warm.parsed, warm.from_cache, warm.removed), (0, 1, 0));
    assert_eq!(cold.entries(), warm.entries());

    let entry = &warm.entries()[0];
    assert_eq!((entry.key.as_str(), entry.variations.as_slice()), ("terrain/tiles/battle/set_a/tile_b", &["Minor".to_owned(), "major/inner".to_owned()][..]));
    let folder = raw.path().join("terrain/tiles/battle/set_a/tile_b");
    let resolved = warm.resolve(&folder.join("major/inner/tile.ted"));
    assert_eq!((resolved.entry.map(|e| e.name.as_str()), resolved.variation), (Some("set_a_tile_b"), Some(1)));
    assert_eq!(warm.resolve(&folder).variation, None);
    assert_eq!(warm.find_name("SET_A/TILE_B").map(|e| e.name.as_str()), Some("set_a_tile_b"));

    fs::remove_file(tiles.join("set_a_tile_b.xml")).unwrap();
    let after = TileIndex::build(raw.path(), Some(&cache));
    assert_eq!((after.entries().len(), after.removed), (0, 1));
}
