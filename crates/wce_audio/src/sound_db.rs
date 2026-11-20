//! The game's DB tables about sound (Attila; docs/plans/audio.md, step 7): which events projectiles and explosions fire
//! (`audio_projectiles`, `audio_explosions`), which voice-over culture a subculture and religion speak
//! (`subculture_religion_voiceover_junctions`), which voice-over each unit has (`main_units.voiceover`). Read with rpfm's DB decoder
//! and schema, as the terrain crates read theirs. The other sound tables of Attila are key lists or empty.

use std::collections::{BTreeMap, HashMap};
use std::io::Cursor;

use rpfm_lib::files::db::DB;
use rpfm_lib::files::{Decodeable, DecodeableExtraData, Encodeable, EncodeableExtraData};
use rpfm_lib::schema::Schema;
use wce_vfs::files::Files;

/// A DB row, by column name.
pub type Row = HashMap<String, String>;

/// A DB table's rows (every file of `db/<table>/`).
pub fn table_rows(schema: &Schema, files: &dyn Files, table: &str) -> Vec<Row> {
    let mut out = vec![];
    for path in files.list(&format!("db/{table}/"), "") {
        let Ok(bytes) = files.read(&path) else { continue };
        out.extend(rows_of(schema, table, &bytes));
    }
    out
}

/// The rows of one table file.
pub fn rows_of(schema: &Schema, table: &str, bytes: &[u8]) -> Vec<Row> {
    let Some(db) = decode_table(schema, table, bytes) else { return vec![] };
    let names: Vec<String> = db.definition().fields_processed().iter().map(|f| f.name().to_owned()).collect();
    db.data().iter().map(|row| names.iter().cloned().zip(row.iter().map(|c| c.data_to_string().into_owned())).collect()).collect()
}

/// Where the edit session writes its DB rows: `db/<table>/wce_audio`, one table file per table, loose in the project.
pub fn edit_table_path(table: &str) -> String {
    format!("db/{table}/wce_audio")
}

fn decode_table(schema: &Schema, table: &str, bytes: &[u8]) -> Option<DB> {
    let mut extra = DecodeableExtraData::default();
    extra.set_schema(Some(schema));
    extra.set_table_name(Some(table));
    DB::decode(&mut Cursor::new(bytes), &Some(extra)).ok()
}

/// A table file holding the row whose `key_column` is `key` with `column` set to `value`: the row copied from the game's
/// tables, into `current` (the session's own table file, when it has one) or a new file. Mod packs' rows override the game's
/// with the same key.
///
/// # Errors
///
/// When the table, the row or the column is not found, or the value does not fit the column's type.
#[allow(clippy::too_many_arguments)]
pub fn set_cell(
    schema: &Schema,
    files: &dyn Files,
    current: Option<&[u8]>,
    table: &str,
    key_column: &str,
    key: &str,
    column: &str,
    value: &str,
) -> Result<Vec<u8>, String> {
    let mut source = None;
    for path in files.list(&format!("db/{table}/"), "") {
        if path == edit_table_path(table) {
            continue;
        }
        let Some(db) = files.read(&path).ok().and_then(|b| decode_table(schema, table, &b)) else { continue };
        let names: Vec<String> = db.definition().fields_processed().iter().map(|f| f.name().to_owned()).collect();
        let k = names.iter().position(|n| n == key_column).ok_or_else(|| format!("{table} has no column {key_column}"))?;
        if let Some(row) = db.data().iter().find(|r| r.get(k).is_some_and(|c| c.data_to_string() == key)).cloned() {
            source = Some((db, names, k, row));
            break;
        }
    }
    let (template, names, k, mut row) = source.ok_or_else(|| format!("no row {key} in {table}"))?;
    let c = names.iter().position(|n| n == column).ok_or_else(|| format!("{table} has no column {column}"))?;
    row[c].set_data(value).map_err(|e| format!("{column} = {value}: {e}"))?;

    let mut db = match current.and_then(|b| decode_table(schema, table, b)) {
        Some(db) => db,
        None => {
            let mut db = template;
            db.set_data(&[]).map_err(|e| e.to_string())?;
            db
        }
    };
    let mut rows = db.data().into_owned();
    match rows.iter().position(|r| r.get(k).is_some_and(|c| c.data_to_string() == key)) {
        Some(i) => rows[i] = row,
        None => rows.push(row),
    }
    db.set_data(&rows).map_err(|e| e.to_string())?;
    let mut extra = EncodeableExtraData::default();
    extra.set_table_has_guid(!db.guid().is_empty());
    let mut out = vec![];
    db.encode(&mut out, &Some(extra)).map_err(|e| e.to_string())?;
    Ok(out)
}

/// The tables naming events, and how their rows are shown.
pub const EVENT_TABLES: [&str; 2] = ["audio_projectiles_tables", "audio_explosions_tables"];

/// The game's sound tables, read.
#[derive(Default)]
pub struct SoundDb {
    /// Table -> rows, for [`EVENT_TABLES`].
    pub event_tables: BTreeMap<String, Vec<Row>>,
    /// (subculture, religion, voice-over culture).
    pub voiceovers: Vec<(String, String, String)>,
    /// Voice-over -> the units speaking it.
    pub unit_voiceovers: BTreeMap<String, Vec<String>>,
    /// Every land unit with its voice: what picks its order lines (crate::unit_voices).
    pub units: Vec<UnitVoice>,
    /// The schema the tables were read with (to write rows).
    pub schema: Option<std::sync::Arc<Schema>>,
}

/// A land unit's voice settings.
#[derive(Clone, Debug, Default)]
pub struct UnitVoice {
    /// The `land_units` key.
    pub land_unit: String,
    /// `main_units.voiceover` (of the first main unit using it).
    pub voiceover: String,
    /// `land_units.category` (cavalry, inf_melee...).
    pub category: String,
    /// `selection_vo`, `selected_vo_secondary`, `selected_vo_tertiary`.
    pub selection: [String; 3],
}

/// The `land_units` columns naming a unit's selection lines.
pub const SELECTION_COLUMNS: [&str; 3] = ["selection_vo", "selected_vo_secondary", "selected_vo_tertiary"];

impl SoundDb {
    /// Reads the tables with the game's schema (`rpfm_schema_path`); empty if there is none.
    pub fn read(files: &dyn Files) -> Self {
        let Some(schema) = files.game().and_then(wce_foundation::settings::rpfm_schema_path).and_then(|p| Schema::load(&p, None).ok())
        else {
            return Self::default();
        };
        let mut db = Self::default();
        for t in EVENT_TABLES {
            db.event_tables.insert(t.to_owned(), table_rows(&schema, files, t));
        }
        for r in table_rows(&schema, files, "subculture_religion_voiceover_junctions_tables") {
            let get = |k: &str| r.get(k).cloned().unwrap_or_default();
            db.voiceovers.push((get("subculture"), get("religion"), get("voiceover_culture")));
        }
        let mut voice_of_land_unit: HashMap<String, String> = HashMap::new();
        for r in table_rows(&schema, files, "main_units_tables") {
            if let (Some(unit), Some(vo)) = (r.get("unit"), r.get("voiceover")) {
                db.unit_voiceovers.entry(vo.clone()).or_default().push(unit.clone());
                if let Some(land) = r.get("land_unit").filter(|l| !l.is_empty()) {
                    voice_of_land_unit.entry(land.clone()).or_insert_with(|| vo.clone());
                }
            }
        }
        for r in table_rows(&schema, files, "land_units_tables") {
            let get = |k: &str| r.get(k).cloned().unwrap_or_default();
            let key = get("key");
            let Some(voiceover) = voice_of_land_unit.get(&key).cloned() else { continue };
            db.units.push(UnitVoice { voiceover, category: get("category"), selection: SELECTION_COLUMNS.map(get), land_unit: key });
        }
        db.units.sort_by(|a, b| a.land_unit.cmp(&b.land_unit));
        db.units.dedup_by(|a, b| a.land_unit == b.land_unit);
        db.schema = Some(std::sync::Arc::new(schema));
        db
    }

    /// Every event name the tables hold: (table, row key, column, event).
    pub fn events(&self) -> Vec<(String, String, String, String)> {
        let mut out = vec![];
        for (table, rows) in &self.event_tables {
            for r in rows {
                let key = r.get("key").cloned().unwrap_or_default();
                for (column, value) in r {
                    if column.contains("event") && !value.is_empty() {
                        out.push((table.clone(), key.clone(), column.clone(), value.clone()));
                    }
                }
            }
        }
        out.sort();
        out
    }
}
