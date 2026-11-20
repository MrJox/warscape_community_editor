//! The game's DB tables about sound (Attila; docs/plans/audio.md, step 7): which events projectiles and explosions fire
//! (`audio_projectiles`, `audio_explosions`), which voice-over culture a subculture and religion speak
//! (`subculture_religion_voiceover_junctions`), which voice-over each unit has (`main_units.voiceover`). Read with rpfm's DB decoder
//! and schema, as the terrain crates read theirs. The other sound tables of Attila are key lists or empty.

use std::collections::{BTreeMap, HashMap};
use std::io::Cursor;

use rpfm_lib::files::db::DB;
use rpfm_lib::files::{Decodeable, DecodeableExtraData};
use rpfm_lib::schema::Schema;
use wce_vfs::files::Files;

/// A DB row, by column name.
pub type Row = HashMap<String, String>;

/// A DB table's rows (every file of `db/<table>/`).
pub fn table_rows(schema: &Schema, files: &dyn Files, table: &str) -> Vec<Row> {
    let mut out = vec![];
    for path in files.list(&format!("db/{table}/"), "") {
        let Ok(bytes) = files.read(&path) else { continue };
        let mut extra = DecodeableExtraData::default();
        extra.set_schema(Some(schema));
        extra.set_table_name(Some(table));
        let Ok(db) = DB::decode(&mut Cursor::new(&bytes), &Some(extra)) else { continue };
        let names: Vec<String> = db.definition().fields_processed().iter().map(|f| f.name().to_owned()).collect();
        for row in db.data().iter() {
            out.push(names.iter().cloned().zip(row.iter().map(|c| c.data_to_string().into_owned())).collect());
        }
    }
    out
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
}

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
        for r in table_rows(&schema, files, "main_units_tables") {
            if let (Some(unit), Some(vo)) = (r.get("unit"), r.get("voiceover")) {
                db.unit_voiceovers.entry(vo.clone()).or_default().push(unit.clone());
            }
        }
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
