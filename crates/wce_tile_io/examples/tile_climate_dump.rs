//! The climates of the tile databases in effect (battle and campaign), with each climate's texture groups, for comparing a mod's
//! database with the kit's `_settings.xml`.
//!
//!   cargo run --release -p wce_tile_io --example tile_climate_dump -- <game folder> [--group TEXT]
//!
//! `--group` keeps the texture groups whose name contains the text.

use std::path::Path;

use wce_foundation::game::GameProfile;
use wce_tile_io::attila::game_data::GameData;

fn main() {
    let args = std::env::args().skip(1).collect::<Vec<_>>();
    let Some(dir) = args.first() else {
        eprintln!("usage: tile_climate_dump <game folder> [--group TEXT]");
        std::process::exit(2);
    };

    let group = args.iter().position(|a| a == "--group").and_then(|i| args.get(i + 1)).map(|g| g.to_lowercase());
    let data = GameData::load(&GameProfile::ATTILA, Path::new(dir), None).expect("game data");
    for (kind, db) in data.kinds.iter().zip(&data.databases) {
        println!("== {kind}: render blend_pixel_scale {}", db.render_params().blend_pixel_scale());
        for c in db.climates() {
            println!("climate {} texture_set {} destruction {} ({} textures)", c.name(), c.texture_set(), c.destruction_climate(), c.textures().len());
            for t in c.textures().iter().filter(|t| group.as_ref().is_none_or(|g| t.name().to_lowercase().contains(g))) {
                println!(
                    "    {:<20} scale {:>8.2} outfield {:>8.2} mid {:.2} smooth {:?} refl {:?}", t.name(), t.blend_pixel_scale(), t.outfield_blend_pixel_scale(), t.mid_distance_strength(),
                    [t.lerp_smoothness0(), t.lerp_smoothness1(), t.lerp_smoothness2(), t.lerp_smoothness3()],
                    [t.lerp_reflectivity0(), t.lerp_reflectivity1(), t.lerp_reflectivity2(), t.lerp_reflectivity3()],
                );
            }
        }
    }
}
