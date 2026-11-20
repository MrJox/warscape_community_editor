//! The historical battle's DB rows and loc (docs/plans/cinematic.md, step 8), with rpfm's DB codec and the game's schema: each
//! table starts from the game's own table file (its definition) and, for `battles`, from a vanilla historical battle's row, so the
//! columns this editor does not set keep vanilla's values.

use std::collections::BTreeMap;
use std::io::Cursor;
use std::path::Path;

use rpfm_lib::files::db::DB;
use rpfm_lib::files::loc::Loc;
use rpfm_lib::files::table::DecodedData;
use rpfm_lib::files::{Decodeable, DecodeableExtraData, Encodeable, EncodeableExtraData};
use rpfm_lib::schema::Schema;
use wce_vfs::files::Files;

use crate::battle_export::{BattleScripts, battle_scripts, script_folder, shortform};
use crate::scene::{Project, sanitize_key};

/// The table files' name in the pack (`db/<table>/<this>`).
pub fn table_file(project: &Project) -> String {
    format!("wce_{}", sanitize_key(&project.battle.key))
}

fn template(schema: &Schema, files: &dyn Files, table: &str) -> Result<DB, String> {
    for path in files.list(&format!("db/{table}/"), "") {
        let Ok(bytes) = files.read(&path) else { continue };
        let mut extra = DecodeableExtraData::default();
        extra.set_schema(Some(schema));
        extra.set_table_name(Some(table));
        if let Ok(db) = DB::decode(&mut Cursor::new(&bytes), &Some(extra)) {
            return Ok(db);
        }
    }
    Err(format!("the game has no readable {table}"))
}

fn columns(db: &DB) -> Vec<String> {
    db.definition().fields_processed().iter().map(|f| f.name().to_owned()).collect()
}

fn set(row: &mut [DecodedData], names: &[String], column: &str, value: &str) -> Result<(), String> {
    let i = names.iter().position(|n| n == column).ok_or_else(|| format!("no column {column}"))?;
    row[i].set_data(value).map_err(|e| format!("{column} = {value}: {e}"))
}

fn encode(mut db: DB, rows: Vec<Vec<DecodedData>>) -> Result<Vec<u8>, String> {
    db.set_data(&rows).map_err(|e| e.to_string())?;
    let mut extra = EncodeableExtraData::default();
    extra.set_table_has_guid(!db.guid().is_empty());
    let mut out = vec![];
    db.encode(&mut out, &Some(extra)).map_err(|e| e.to_string())?;
    Ok(out)
}

/// The tables: `battles`, `historical_battles_ui_locations`, `scripted_subtitles` (when there are subtitles), as pack paths and bytes.
///
/// # Errors
///
/// A table the game doesn't have, or a value that doesn't fit its column.
pub fn battle_tables(
    project: &Project,
    scripts: &BattleScripts,
    files: &dyn Files,
    schema: &Schema,
) -> Result<Vec<(String, Vec<u8>)>, String> {
    let key = sanitize_key(&project.battle.key);
    let file = table_file(project);
    let folder = script_folder(project);
    let mut out = vec![];

    // battles: a copy of a vanilla historical battle's row.
    let db = template(schema, files, "battles_tables")?;
    let names = columns(&db);
    let type_col = names.iter().position(|n| n == "type");
    let mut row = db
        .data()
        .iter()
        .find(|r| type_col.and_then(|c| r.get(c)).is_some_and(|c| c.data_to_string() == "napoleon_historic"))
        .cloned()
        .unwrap_or_else(|| db.new_row());
    set(&mut row, &names, "key", &key)?;
    set(&mut row, &names, "type", "napoleon_historic")?;
    set(&mut row, &names, "specification", &format!("{folder}/{}_battle.xml", shortform(project)))?;
    set(&mut row, &names, "screenshot_path", &format!("{folder}/screenshot_small.png"))?;
    set(&mut row, &names, "year", &project.battle.year.to_string())?;
    out.push((format!("db/battles_tables/{file}"), encode(db, vec![row])?));

    // The Historical Battles map's position.
    let db = template(schema, files, "historical_battles_ui_locations_tables")?;
    let names = columns(&db);
    let mut row = db.new_row();
    set(&mut row, &names, "key", &key)?;
    set(&mut row, &names, "x", &project.battle.menu_position[0].to_string())?;
    set(&mut row, &names, "y", &project.battle.menu_position[1].to_string())?;
    out.push((format!("db/historical_battles_ui_locations_tables/{file}"), encode(db, vec![row])?));

    if !scripts.subtitles.is_empty() {
        let db = template(schema, files, "scripted_subtitles_tables")?;
        let names = columns(&db);
        let mut rows = vec![];
        for k in scripts.subtitles.keys() {
            let mut row = db.new_row();
            set(&mut row, &names, "key", k)?;
            set(&mut row, &names, "character_for_vo", "narrator")?;
            rows.push(row);
        }
        out.push((format!("db/scripted_subtitles_tables/{file}"), encode(db, rows)?));
    }
    Ok(out)
}

/// The loc entries: the menu's name and description and every subtitle, in `language` (English where it has no text).
pub fn battle_loc_entries(project: &Project, scripts: &BattleScripts, language: &str) -> BTreeMap<String, String> {
    let key = sanitize_key(&project.battle.key);
    let mut out = BTreeMap::new();
    let text = |id: &str| project.text(id, language).unwrap_or("").to_owned();
    out.insert(format!("battles_localised_name_{key}"), text(&project.battle.title_text));
    out.insert(format!("battles_description_{key}"), text(&project.battle.description_text));
    for (k, id) in &scripts.subtitles {
        out.insert(format!("scripted_subtitles_localised_text_{k}"), text(id));
    }
    out
}

/// A `.loc` file of entries.
///
/// # Errors
///
/// rpfm can't encode it.
pub fn loc_file(entries: &BTreeMap<String, String>) -> Result<Vec<u8>, String> {
    let mut loc = Loc::new();
    let mut rows = vec![];
    for (k, v) in entries {
        let mut row = loc.new_row();
        row[0].set_data(k).map_err(|e| e.to_string())?;
        row[1].set_data(v).map_err(|e| e.to_string())?;
        row[2].set_data("true").map_err(|e| e.to_string())?;
        rows.push(row);
    }
    loc.set_data(&rows).map_err(|e| e.to_string())?;
    let mut out = vec![];
    loc.encode(&mut out, &None).map_err(|e| e.to_string())?;
    Ok(out)
}

/// Everything a historical battle needs, as pack paths and bytes, and the warnings (what was left out or changed).
///
/// # Errors
///
/// As [`battle_tables`] and [`loc_file`].
pub fn battle_files(
    project: &Project,
    files: &dyn Files,
    schema: &Schema,
    language: &str,
) -> Result<(Vec<(String, Vec<u8>)>, Vec<String>), String> {
    let scripts = battle_scripts(project);
    let mut out: Vec<(String, Vec<u8>)> = scripts.files.iter().map(|(p, t)| (p.clone(), t.as_bytes().to_vec())).collect();
    out.extend(battle_tables(project, &scripts, files, schema)?);
    let loc = loc_file(&battle_loc_entries(project, &scripts, language))?;
    out.push((format!("text/db/{}.loc", table_file(project)), loc));
    Ok((out, scripts.warnings))
}

/// Writes the battle as a mod pack.
///
/// # Errors
///
/// As [`battle_files`], or the pack can't be written.
pub fn write_battle_pack(
    path: &Path,
    project: &Project,
    files: &dyn Files,
    schema: &Schema,
    language: &str,
) -> Result<Vec<String>, String> {
    let (out, warnings) = battle_files(project, files, schema, language)?;
    wce_walls::pack::write_pack(path, &project.game, out, vec![])?;
    Ok(warnings)
}
