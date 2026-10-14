//! weather_check <environment_map.xml>: parses the assembly kit's source form into a variation database and runs a few queries. Port of
//! `cli/weather_check.cpp`.

use std::collections::BTreeMap;

use rpfm_lib::files::asset_variation_db::AssetVariationDb;

fn main() {
    let Some(path) = std::env::args().nth(1) else { std::process::exit(2) };
    let db = match std::fs::read(&path).map_err(|e| e.to_string()).and_then(|b| AssetVariationDb::from_environment_map(&String::from_utf8_lossy(&b)).map_err(|e| e.to_string())) {
        Ok(db) => db,
        Err(e) => {
            eprintln!("error: {e}");
            std::process::exit(1);
        }
    };

    println!("{} environment files", db.entries()[0].variations().len());
    for m in db.categories() {
        let mut line = format!("{:<22} {} values:", m.name(), m.values().len());
        for (k, x) in m.values().iter().enumerate() {
            if k >= 12 {
                line += " ...";
                break;
            }

            line += &format!(" {x}");
        }

        println!("{line}");
    }

    let q = BTreeMap::from(
        [
            ("map", "main_attila_map"), ("climate", "ROME_Desert"), ("season", "su"), ("precipitation", "dry"), ("time_of_day", "day"), ("battle_types", "land"),
            ("snow_attrition", "snow_attrition_off"),
        ]
        .map(|(k, v)| (k.to_owned(), v.to_owned())),
    );
    let r = db.matches_listed_map("environment", &q);
    println!("query desert/summer/dry/day: {} files{}", r.variations.len(), if r.used_default_map { " (default_map retry)" } else { "" });
    for c in &r.variations {
        println!("  {}", c.filename());
    }

    let default = BTreeMap::from([("map".to_owned(), "default_map".to_owned())]);
    println!("unconstrained query on default_map: {} files", db.matches_listed_map("environment", &default).variations.len());
}
