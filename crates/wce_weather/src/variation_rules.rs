//! What the game asks the weather selection database (warscape_asset_variation_db) for, so the editor can tell the values the engine sends from
//! the ones that are data (docs/re/variation_db.md, "What the engine asks for").
//!
//! Every battle's query (fn 0x117d9f10) has one value for each of eight categories. Five come from strings in the game's code; the map is the
//! campaign's, the climate a name of the battle tile database and the climate change index the campaign's current phase
//! (`campaign_climate_change_phases_tables`). The game finds a value by its name in all categories, and a value that is in no list makes the
//! query match nothing (fn 0x11862300), so a missing engine value costs those battles their weather; category names and values the engine
//! never sends change nothing.

use std::io::Cursor;

use rpfm_lib::files::asset_variation_db::AssetVariationDb;
use rpfm_lib::files::db::DB;
use rpfm_lib::files::tile_database::TileDatabase;
use rpfm_lib::files::{Decodeable, DecodeableExtraData};
use rpfm_lib::schema::Schema;
use wce_vfs::Vfs;

/// The battle tile database in the packs: its climates are the names the game sends as `climate`.
pub const BATTLE_TILE_DATABASE: &str = "terrain/tiles/battle/tile_database.bin";

/// The campaigns' climate change phases: their `phase` values are the indices the game sends as `cc<phase>`.
pub const CLIMATE_PHASES_TABLE: &str = "campaign_climate_change_phases_tables";

/// The map the game asks again with when nothing matched.
pub const DEFAULT_MAP: &str = "default_map";

/// The climate the game sends for a battle without one.
pub const DEFAULT_CLIMATE: &str = "default";

/// The climate change index of a battle without a campaign (fn 0x10da9430) or of a campaign without phases.
pub const DEFAULT_CLIMATE_CHANGE: &str = "cc0";

/// Where the values of a category the game asks for come from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Values {
    /// Strings in the game's code.
    Fixed(&'static [&'static str]),

    /// The campaign map's name (data); `default_map` is the game's.
    Maps,

    /// A climate of the battle tile database (data); `default` is the game's.
    Climates,

    /// `cc<phase>` of the campaign's current climate change phase (data); `cc0` is the game's.
    Phases,
}

/// A category of every battle's query.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EngineCategory {
    pub name: &'static str,
    pub values: Values,
}

/// The query of fn 0x117d9f10, in the order of the shipped database. Season "default" (a battle without a campaign season) is in neither shipped
/// database and left out here.
pub const ENGINE_CATEGORIES: [EngineCategory; 8] = [
    EngineCategory { name: "map", values: Values::Maps },
    EngineCategory { name: "season", values: Values::Fixed(&["sp", "su", "au", "wi"]) },
    EngineCategory { name: "precipitation", values: Values::Fixed(&["dry", "rain", "snow", "fog"]) },
    EngineCategory { name: "time_of_day", values: Values::Fixed(&["day", "night"]) },
    EngineCategory { name: "climate_change_index", values: Values::Phases },
    EngineCategory { name: "snow_attrition", values: Values::Fixed(&["snow_attrition_on", "snow_attrition_off"]) },
    EngineCategory { name: "battle_type", values: Values::Fixed(&["land", "sea"]) },
    EngineCategory { name: "climate", values: Values::Climates },
];

/// What a value is to the game.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Role {
    /// The game's code sends it: removing or renaming it costs battles their weather.
    Engine,

    /// A map, climate or climate change phase from the game files: free to add, rename and remove.
    Data,

    /// The game never sends it: it only takes a bit.
    Unused,
}

pub fn engine_category(name: &str) -> Option<&'static EngineCategory> {
    ENGINE_CATEGORIES.iter().find(|c| c.name == name)
}

/// True for the categories whose values are data (map, climate, climate change index): those values are free to edit.
pub fn is_data_category(name: &str) -> bool {
    matches!(engine_category(name).map(|c| c.values), Some(Values::Maps | Values::Climates | Values::Phases))
}

pub fn value_role(category: &str, value: &str) -> Role {
    match engine_category(category).map(|c| c.values) {
        Some(Values::Fixed(values)) if values.contains(&value) => Role::Engine,
        Some(Values::Fixed(_)) | None => Role::Unused,
        Some(Values::Maps) if value == DEFAULT_MAP => Role::Engine,
        Some(Values::Climates) if value == DEFAULT_CLIMATE => Role::Engine,
        Some(Values::Phases) if value == DEFAULT_CLIMATE_CHANGE => Role::Engine,
        Some(Values::Maps | Values::Climates | Values::Phases) => Role::Data,
    }
}

/// Why an engine value or category must stay, for tooltips and refusals.
pub fn engine_reason(category: &str, value: Option<&str>) -> String {
    match value {
        Some(DEFAULT_MAP) if category == "map" => format!("The game asks again with '{DEFAULT_MAP}' when nothing matched a battle's map."),
        Some(DEFAULT_CLIMATE) if category == "climate" => format!("The game sends '{DEFAULT_CLIMATE}' for a battle without a climate."),
        Some(DEFAULT_CLIMATE_CHANGE) if category == "climate_change_index" => {
            format!("The game sends '{DEFAULT_CLIMATE_CHANGE}' for battles outside a campaign and in campaigns without climate change phases.")
        }
        Some(value) => format!("The game sends '{value}' as the {category} of a battle; without it those battles get no weather from the database."),
        None => format!("The game sends a {category} value in every battle; without its values no battle gets weather from the database."),
    }
}

/// The climate names of a battle tile database (`tile_database.bin`), in their order.
pub fn battle_climates(bytes: &[u8]) -> Result<Vec<String>, String> {
    let db = TileDatabase::decode(&mut Cursor::new(bytes), &None).map_err(|e| e.to_string())?;
    Ok(db.climates().iter().map(|c| c.name().clone()).collect())
}

/// The `phase` values of `campaign_climate_change_phases_tables` files, sorted, each once.
pub fn climate_phases(tables: &[Vec<u8>], schema: &Schema) -> Result<Vec<u32>, String> {
    let mut phases = vec![];
    for bytes in tables {
        let mut extra = DecodeableExtraData::default();
        extra.set_schema(Some(schema));
        extra.set_table_name(Some(CLIMATE_PHASES_TABLE));
        let db = DB::decode(&mut Cursor::new(bytes), &Some(extra)).map_err(|e| e.to_string())?;
        let phase = db.column_position_by_name("phase").ok_or("the table has no 'phase' column")?;
        for row in db.data().iter() {
            let value = row[phase].data_to_string();
            phases.push(value.trim().parse::<u32>().map_err(|_| format!("phase '{value}' is not a number"))?);
        }
    }

    phases.sort_unstable();
    phases.dedup();
    Ok(phases)
}

/// What the game files say about the values the game sends; None where they could not be read.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct GameFacts {
    /// The battle tile database's climates.
    pub climates: Option<Vec<String>>,

    /// The campaigns' climate change phases.
    pub climate_phases: Option<Vec<u32>>,

    /// Why something could not be read.
    pub notes: Vec<String>,
}

impl GameFacts {
    /// Reads the battle tile database and the climate change phase tables the packs give the game; the tables need rpfm's schema of the
    /// game (`wce_foundation::settings::rpfm_schema_path`).
    pub fn read(vfs: &Vfs) -> Self {
        let mut facts = Self::default();
        match vfs.read(BATTLE_TILE_DATABASE).map_err(|e| e.to_string()).and_then(|bytes| battle_climates(&bytes)) {
            Ok(climates) => facts.climates = Some(climates),
            Err(e) => facts.notes.push(format!("{BATTLE_TILE_DATABASE} was not read ({e}): climates are not checked against it.")),
        }

        let schema = wce_foundation::settings::rpfm_schema_path(vfs.profile());
        let phases = schema.ok_or_else(|| format!("no rpfm schema ({} in the DB schemas folder or rpfm's)", vfs.profile().rpfm_schema)).and_then(|path| {
            let schema = Schema::load(&path, None).map_err(|e| format!("{}: {e}", path.display()))?;
            let tables = vfs.list(&format!("db/{CLIMATE_PHASES_TABLE}/"), "").into_iter().filter_map(|p| vfs.read(p).ok()).collect::<Vec<_>>();
            climate_phases(&tables, &schema)
        });
        match phases {
            Ok(phases) => facts.climate_phases = Some(phases),
            Err(e) => facts.notes.push(format!("The climate change phases were not read ({e}): climate_change_index is not checked.")),
        }

        facts
    }
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

/// Data values the game files define for a category, against the database's list: the missing ones are errors, the listed ones the game files
/// lack are never asked for.
fn check_data(out: &mut Vec<Finding>, db: &AssetVariationDb, category: &str, default: &str, known: &[String], what: &str, source: &str) {
    for value in known.iter().filter(|v| db.category_of(v).is_none()) {
        out.push(finding(Severity::Error, format!("{what} '{value}' of {source} is in no list: its battles get no weather from the database.")));
    }

    for value in db.category(category).map(|c| c.values().as_slice()).unwrap_or_default() {
        if value != default && !known.contains(value) {
            out.push(finding(Severity::Warning, format!("{source} has no {what} '{value}': the game never asks for it until one is added there.")));
        }
    }
}

/// What the game will make of the database: engine values it is missing, values and categories the game never asks for, and (with the game
/// files' climates and climate change phases) data values on either side that the other lacks. `AssetVariationDb::validate` covers the file's
/// own consistency.
pub fn check(db: &AssetVariationDb, facts: &GameFacts) -> Vec<Finding> {
    let mut out = vec![];
    for engine in &ENGINE_CATEGORIES {
        let default = match engine.values {
            Values::Maps => DEFAULT_MAP,
            Values::Climates => DEFAULT_CLIMATE,
            Values::Phases => DEFAULT_CLIMATE_CHANGE,
            Values::Fixed(_) => "",
        };

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

                if let Some(category) = db.category(engine.name) {
                    for value in category.values().iter().filter(|v| !values.contains(&v.as_str())) {
                        out.push(finding(Severity::Info, format!("The game never sends '{value}' ({}): removing it frees a bit.", engine.name)));
                    }
                }
            }

            Values::Maps => {
                if db.category(engine.name).is_none() && db.category_of(DEFAULT_MAP).is_none() {
                    out.push(finding(Severity::Error, format!("No 'map' category and no '{DEFAULT_MAP}': the game sends a map in every battle, so no battle gets weather from the database.")));
                }
            }

            Values::Climates | Values::Phases => {
                if db.category_of(default).is_none() {
                    out.push(finding(Severity::Error, format!("'{default}' is in no list: {}", engine_reason(engine.name, Some(default)))));
                }

                if engine.values == Values::Climates && let Some(climates) = &facts.climates {
                    check_data(&mut out, db, engine.name, default, climates, "climate", "the battle tile database");
                }

                if engine.values == Values::Phases && let Some(phases) = &facts.climate_phases {
                    let known = phases.iter().map(|p| format!("cc{p}")).collect::<Vec<_>>();
                    check_data(&mut out, db, engine.name, default, &known, "climate change phase", CLIMATE_PHASES_TABLE);
                }
            }
        }
    }

    for category in db.categories().iter().filter(|c| engine_category(c.name()).is_none()) {
        out.push(finding(Severity::Info, format!("The game never asks for '{}': its {} value(s) only take bits.", category.name(), category.values().len())));
    }

    out.extend(facts.notes.iter().map(|n| finding(Severity::Info, n.clone())));
    out.sort_by_key(|f| f.severity);
    out
}
