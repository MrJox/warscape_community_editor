//! The climates a tile is drawn with, from the tile database the game reads. A mod (TDD) ships its own `terrain/tiles/<kind>/tile_database.bin`,
//! battle and campaign, which adds texture groups to the climates (`mordor_gravel` in every battle climate, `mordor_a` in the campaign's
//! `default`) with their scales and light values. The kit's `_settings.xml` has none of them, so the climates are the packs' database over the
//! kit's settings (docs/re/terrain.md, "Mod tile databases").

use std::io::Cursor;

use rpfm_lib::files::Decodeable;
use rpfm_lib::files::tile_database::TileDatabase;
use wce_formats::tile_climate::{Climate, ClimateList, ClimateTexture};
use wce_foundation::log;
use wce_vfs::files::Files;

/// The climates of a tile database, texture groups with their scales and light values.
pub fn climates_of_database(db: &TileDatabase, kind: &str) -> ClimateList {
    let climates = db.climates().iter().map(|c| Climate {
        name: c.name().clone(),
        texture_set: c.texture_set().clone(),
        destruction_climate: c.destruction_climate().clone(),
        textures: c.textures().iter().map(|t| ClimateTexture {
            name: t.name().clone(),
            blend_pixel_scale: f64::from(*t.blend_pixel_scale()),
            outfield_blend_pixel_scale: f64::from(*t.outfield_blend_pixel_scale()),
            mid_distance_strength: f64::from(*t.mid_distance_strength()),
            lerp_smoothness: [t.lerp_smoothness0(), t.lerp_smoothness1(), t.lerp_smoothness2(), t.lerp_smoothness3()].map(|v| f64::from(*v)),
            lerp_reflectivity: [t.lerp_reflectivity0(), t.lerp_reflectivity1(), t.lerp_reflectivity2(), t.lerp_reflectivity3()].map(|v| f64::from(*v)),
        }).collect(),
    }).collect();

    ClimateList { climates, blend_pixel_scale: Some(f64::from(*db.render_params().blend_pixel_scale())), kind: kind.to_owned() }
}

/// The climates for a kind of tile (`battle`, `campaign`): the database in the packs (the winning pack's, a mod's before the game's) over
/// the kit's settings `kit`. Without a database in the packs (a game whose databases are XML), `kit` alone, as the kind's.
pub fn effective_climates(kit: ClimateList, kind: &str, files: Option<&dyn Files>) -> ClimateList {
    let kit = ClimateList { kind: kind.to_owned(), ..kit };
    let Some(files) = files else { return kit };
    let path = format!("terrain/tiles/{kind}/tile_database.bin");
    if !files.exists(&path) {
        return kit;
    }

    let decoded = files.read(&path).and_then(|bytes| TileDatabase::decode(&mut Cursor::new(bytes), &None).map_err(|e| e.to_string()));
    match decoded {
        Ok(db) => climates_of_database(&db, kind).over(kit),
        Err(e) => {
            log::warn(format!("climates: {path}: {e}"));
            kit
        }
    }
}
