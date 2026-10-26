//! What the game asks the weather selection database (warscape_asset_variation_db) for, so the editor can tell the values the engine sends from
//! the ones that are data (docs/re/variation_db.md, "What the engine asks for").
//!
//! Every battle's query (fn 0x117d9f10) has one value for each of eight categories. Six come from strings in the game's code; the map is the
//! campaign's and the climate is a name of the battle tile database. The game finds a value by its name in all categories, and a value that is
//! in no list makes the query match nothing (fn 0x11862300), so a missing engine value costs those battles their weather; category names and
//! values the engine never sends change nothing.

use std::io::Cursor;

use rpfm_lib::files::Decodeable;
use rpfm_lib::files::asset_variation_db::AssetVariationDb;
use rpfm_lib::files::tile_database::TileDatabase;

/// The battle tile database in the packs: its climates are the names the game sends as `climate`.
pub const BATTLE_TILE_DATABASE: &str = "terrain/tiles/battle/tile_database.bin";

/// The map the game asks again with when nothing matched.
pub const DEFAULT_MAP: &str = "default_map";

/// The climate the game sends for a battle without one.
pub const DEFAULT_CLIMATE: &str = "default";

/// Where the values of a category the game asks for come from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Values {
    /// Strings in the game's code.
    Fixed(&'static [&'static str]),

    /// The campaign map's name (data); `default_map` is the game's.
    Maps,

    /// A climate of the battle tile database (data); `default` is the game's.
    Climates,
}

/// A category of every battle's query.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EngineCategory {
    pub name: &'static str,
    pub values: Values,
}

/// The query of fn 0x117d9f10, in the order of the shipped database. Season "default" (a battle without a campaign season) is in neither shipped
/// database and left out here; the climate change range (`cc%d` of a battle field) is taken from the shipped lists, not traced.
pub const ENGINE_CATEGORIES: [EngineCategory; 8] = [
    EngineCategory { name: "map", values: Values::Maps },
    EngineCategory { name: "season", values: Values::Fixed(&["sp", "su", "au", "wi"]) },
    EngineCategory { name: "precipitation", values: Values::Fixed(&["dry", "rain", "snow", "fog"]) },
    EngineCategory { name: "time_of_day", values: Values::Fixed(&["day", "night"]) },
    EngineCategory { name: "climate_change_index", values: Values::Fixed(&["cc0", "cc1", "cc2", "cc3", "cc4"]) },
    EngineCategory { name: "snow_attrition", values: Values::Fixed(&["snow_attrition_on", "snow_attrition_off"]) },
    EngineCategory { name: "battle_type", values: Values::Fixed(&["land", "sea"]) },
    EngineCategory { name: "climate", values: Values::Climates },
];

/// What a value is to the game.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Role {
    /// The game's code sends it: removing or renaming it costs battles their weather.
    Engine,

    /// A map or climate name: free to add, rename and remove.
    Data,

    /// The game never sends it: it only takes a bit.
    Unused,
}

pub fn engine_category(name: &str) -> Option<&'static EngineCategory> {
    ENGINE_CATEGORIES.iter().find(|c| c.name == name)
}

/// True for the categories whose values are data (map, climate): those values are free to edit.
pub fn is_data_category(name: &str) -> bool {
    matches!(engine_category(name).map(|c| c.values), Some(Values::Maps | Values::Climates))
}

pub fn value_role(category: &str, value: &str) -> Role {
    match engine_category(category).map(|c| c.values) {
        Some(Values::Fixed(values)) if values.contains(&value) => Role::Engine,
        Some(Values::Fixed(_)) | None => Role::Unused,
        Some(Values::Maps) if value == DEFAULT_MAP => Role::Engine,
        Some(Values::Climates) if value == DEFAULT_CLIMATE => Role::Engine,
        Some(Values::Maps | Values::Climates) => Role::Data,
    }
}

/// Why an engine value or category must stay, for tooltips and refusals.
pub fn engine_reason(category: &str, value: Option<&str>) -> String {
    match value {
        Some(DEFAULT_MAP) if category == "map" => format!("The game asks again with '{DEFAULT_MAP}' when nothing matched a battle's map."),
        Some(DEFAULT_CLIMATE) if category == "climate" => format!("The game sends '{DEFAULT_CLIMATE}' for a battle without a climate."),
        Some(value) => format!("The game sends '{value}' as the {category} of a battle; without it those battles get no weather from the database."),
        None => format!("The game sends a {category} value in every battle; without its values no battle gets weather from the database."),
    }
}

/// The climate names of a battle tile database (`tile_database.bin`), in their order.
pub fn battle_climates(bytes: &[u8]) -> Result<Vec<String>, String> {
    let db = TileDatabase::decode(&mut Cursor::new(bytes), &None).map_err(|e| e.to_string())?;
    Ok(db.climates().iter().map(|c| c.name().clone()).collect())
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Severity {
    /// Battles lose their weather.
    Error,

    /// Something the game never uses, or a likely mistake.
    Warning,

    Info,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Finding {
    pub severity: Severity,
    pub text: String,
}

fn finding(severity: Severity, text: String) -> Finding {
    Finding { severity, text }
}

/// What the game will make of the database: engine values it is missing, values and categories the game never asks for, and (with the battle
/// tile database's climates) climates on either side that the other lacks. `AssetVariationDb::validate` covers the file's own consistency.
pub fn check(db: &AssetVariationDb, tile_climates: Option<&[String]>) -> Vec<Finding> {
    let mut out = vec![];
    for engine in &ENGINE_CATEGORIES {
        let listed = db.category(engine.name);
        match engine.values {
            Values::Fixed(values) => {
                for value in values {
                    match db.category_of(value) {
                        None => out.push(finding(Severity::Error, format!("'{value}' is in no list: battles with {} '{value}' get no weather from the database.", engine.name))),
                        Some(other) if other != engine.name => out.push(finding(
                            Severity::Info, format!("'{value}' is under '{other}', not '{}': the game finds it anyway (it looks values up by name).", engine.name),
                        )),
                        Some(_) => {}
                    }
                }
            }

            Values::Maps if listed.is_none() && db.category_of(DEFAULT_MAP).is_none() => out.push(finding(
                Severity::Error, format!("No 'map' category and no '{DEFAULT_MAP}': the game sends a map in every battle, so no battle gets weather from the database."),
            )),
            Values::Maps => {}
            Values::Climates => {
                if db.category_of(DEFAULT_CLIMATE).is_none() {
                    out.push(finding(Severity::Warning, format!("'{DEFAULT_CLIMATE}' is in no list: battles without a climate get no weather from the database.")));
                }

                if let Some(climates) = tile_climates {
                    for climate in climates.iter().filter(|c| db.category_of(c).is_none()) {
                        out.push(finding(Severity::Error, format!("Climate '{climate}' of the battle tile database is in no list: its battles get no weather from the database.")));
                    }

                    for value in listed.map(|c| c.values().as_slice()).unwrap_or_default() {
                        if value != DEFAULT_CLIMATE && !climates.contains(value) {
                            out.push(finding(
                                Severity::Warning,
                                format!("The battle tile database has no climate '{value}': the game never asks for it until a climate of that name is added there."),
                            ));
                        }
                    }
                }
            }
        }

        if let (Values::Fixed(values), Some(category)) = (engine.values, listed) {
            for value in category.values().iter().filter(|v| !values.contains(&v.as_str())) {
                out.push(finding(Severity::Info, format!("The game never sends '{value}' ({}): removing it frees a bit.", engine.name)));
            }
        }
    }

    for category in db.categories().iter().filter(|c| engine_category(c.name()).is_none()) {
        out.push(finding(Severity::Info, format!("The game never asks for '{}': its {} value(s) only take bits.", category.name(), category.values().len())));
    }

    if tile_climates.is_none() {
        out.push(finding(Severity::Info, format!("{BATTLE_TILE_DATABASE} was not found: climates are not checked against it.")));
    }

    out.sort_by_key(|f| f.severity);
    out
}
