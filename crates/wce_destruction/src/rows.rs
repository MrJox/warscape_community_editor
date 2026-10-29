//! The new building's rows: the original's copied under the new key (wce_walls::db), with `battlefield_buildings.destruct_thresholds`
//! for its level count (the game's buildings have `0.8` with three levels, else none) and `models_building.EFLines` made again from the
//! new logic file (the firing lines of every level; their action codes read off the original's row).

use std::collections::HashMap;

use rpfm_lib::files::db::DB;
use rpfm_lib::files::table::DecodedData;
use rpfm_lib::schema::Schema;
use wce_vfs::files::Files;
use wce_walls::db::{bytes_mut, cell_mut, copy_rows};
use wce_walls::lines::{action_codes, ef_lines_of, read_ef_lines, write_ef_lines};

/// The rows of the new building (`<table>_tables` -> rows), and notes.
///
/// # Errors
///
/// A table doesn't decode, or the original's lines don't read.
pub fn rows(files: &dyn Files, schema: &Schema, source_key: &str, new_key: &str, source_logic: Option<&[u8]>, new_logic: Option<&[u8]>, levels: usize) -> Result<(Vec<(String, DB)>, Vec<String>), String> {
    let mut copied = copy_rows(files, schema, &[(source_key.to_owned(), new_key.to_owned())])?;
    let mut notes = vec![];
    if !copied.tables.contains_key("battlefield_buildings_tables") {
        notes.push(format!("{source_key} has no battlefield_buildings row: the new building can't be placed until it has one"));
    }

    if let Some(db) = copied.tables.get_mut("battlefield_buildings_tables") {
        if let Some(c) = cell_mut(db, new_key, "destruct_thresholds") {
            let v = if levels == 3 { "0.8" } else { "" };
            set_text(c, v);
        }
    }

    if let (Some(db), Some(old), Some(new)) = (copied.tables.get_mut("models_building_tables"), source_logic, new_logic) {
        if let Some(cell) = cell_mut(db, new_key, "EFLines") {
            if let Some(bytes) = bytes_mut(cell) {
                let mut codes = HashMap::new();
                action_codes(bytes, old, &mut codes)?;
                let before = read_ef_lines(bytes)?.len();
                let (lines, unknown) = ef_lines_of(new, &codes)?;
                if unknown > 0 {
                    notes.push(format!("{unknown} firing lines of an action the original's row doesn't name were left out of EFLines"));
                }
                notes.push(format!("EFLines: {before} -> {}", lines.len()));
                *bytes = write_ef_lines(&lines);
            }
        }
    }

    Ok((copied.tables.into_iter().collect(), notes))
}

/// A building's `battlefield_buildings` category (`fort_wall`, ...), empty when it has no row.
pub fn category_of(files: &dyn Files, schema: &Schema, key: &str) -> String {
    let Ok(copied) = copy_rows(files, schema, &[(key.to_owned(), key.to_owned())]) else { return String::new() };
    copied.tables.get("battlefield_buildings_tables").and_then(|db| {
        let (k, c) = (db.column_position_by_name("key")?, db.column_position_by_name("category")?);
        db.data().iter().find(|r| r[k].data_to_string().eq_ignore_ascii_case(key)).map(|r| r[c].data_to_string().into_owned())
    }).unwrap_or_default()
}

fn set_text(c: &mut DecodedData, v: &str) {
    match c {
        DecodedData::StringU8(s) | DecodedData::StringU16(s) | DecodedData::OptionalStringU8(s) | DecodedData::OptionalStringU16(s) => *s = v.to_owned(),
        _ => {}
    }
}
