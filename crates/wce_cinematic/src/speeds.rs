//! Every unit's speeds (docs/plans/cinematic.md, "Units"): `main_units.land_unit` -> `land_units.man_entity` ->
//! `battle_entities.walk_speed / run_speed / charge_speed` (metres per second). The evaluation picks a moving unit's gait by them.

use std::collections::HashMap;

use rpfm_lib::schema::Schema;
use wce_audio::sound_db::table_rows;
use wce_vfs::files::Files;

use crate::eval::{SpeedTable, Speeds};

/// The speeds of every `main_units` key whose man entity is known (with the game's schema; empty without one).
pub fn speed_table(files: &dyn Files) -> SpeedTable {
    let Some(schema) = files.game().and_then(wce_foundation::settings::rpfm_schema_path).and_then(|p| Schema::load(&p, None).ok()) else {
        return SpeedTable::new();
    };
    let num = |r: &HashMap<String, String>, c: &str| r.get(c).and_then(|v| v.trim().parse::<f32>().ok());
    let entities: HashMap<String, Speeds> = table_rows(&schema, files, "battle_entities_tables")
        .into_iter()
        .filter_map(|r| {
            let s = Speeds { walk: num(&r, "walk_speed")?, run: num(&r, "run_speed")?, charge: num(&r, "charge_speed")? };
            Some((r.get("key")?.clone(), s))
        })
        .collect();
    let man_of: HashMap<String, String> = table_rows(&schema, files, "land_units_tables")
        .into_iter()
        .filter_map(|r| Some((r.get("key")?.clone(), r.get("man_entity")?.clone())))
        .collect();
    table_rows(&schema, files, "main_units_tables")
        .into_iter()
        .filter_map(|r| {
            let man = man_of.get(r.get("land_unit")?)?;
            Some((r.get("unit")?.clone(), *entities.get(man)?))
        })
        .collect()
}
