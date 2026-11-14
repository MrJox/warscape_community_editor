//! The DB rows that decide a unit's looks, as the project changes them: `unit_variants` (a unit's definition per faction, its height),
//! `variants` (the definitions' names) and `unit_variants_colours` (a unit's own colours). The project's rows are table files of their own,
//! `<projects>/loose/db/<table>_tables/!wce_unit_tool`, encoded by rpfm: the loose folder is mounted before the packs, so the editor reads
//! them as the game's, and they go into a mod pack as they are.
//!
//! **ASSUMPTION** (modders' practice, not traced here): the game reads a table's files in the order of their names and keeps a key's
//! first row, so a file whose name starts with `!` takes the place of the game's rows. The editor's catalog reads them that way too.

use std::collections::BTreeMap;
use std::io::Cursor;
use std::path::{Path, PathBuf};

use rpfm_lib::files::db::DB;
use rpfm_lib::files::table::DecodedData;
use rpfm_lib::files::{Decodeable, DecodeableExtraData, Encodeable, EncodeableExtraData};
use rpfm_lib::games::supported_games::SupportedGames;
use rpfm_lib::schema::Schema;
use wce_vfs::Vfs;

/// The name of the project's table files.
pub const TABLE_FILE: &str = "!wce_unit_tool";

/// The tables.
pub const TABLES: [&str; 3] = ["unit_variants", "variants", "unit_variants_colours"];

/// A row, its columns as text.
pub type Row = BTreeMap<String, String>;

fn text_row(db: &DB, row: &[DecodedData]) -> Row {
    db.definition().fields_processed().iter().zip(row).map(|(f, c)| (f.name().to_owned(), c.data_to_string().into_owned())).collect()
}

/// Whether a row has these values (any case).
fn has(row: &Row, values: &[(&str, &str)]) -> bool {
    values.iter().all(|(c, v)| row.get(*c).is_some_and(|x| x.eq_ignore_ascii_case(v)))
}

/// One table: the game's rows (and the mods'), and the project's.
pub struct Table {
    #[allow(dead_code, reason = "for messages")]
    name: String,

    /// The columns the schema marks as the key: what tells two rows apart (`unit_variants`: a unit and a faction, not its `name`).
    keys: Vec<String>,

    /// The project's file, rows and all; the game's table's definition when the project has none yet.
    project: DB,
    game: Vec<Row>,
}

impl Table {
    pub fn project_rows(&self) -> Vec<Row> {
        self.project.data().iter().map(|r| text_row(&self.project, r)).collect()
    }

    fn key_of(&self, row: &Row) -> Vec<String> {
        self.keys.iter().map(|c| row.get(c).cloned().unwrap_or_default().to_lowercase()).collect()
    }

    /// The first row with these values as the editor reads it: the project's, else the game's.
    pub fn row(&self, values: &[(&str, &str)]) -> Option<Row> {
        self.project_rows().into_iter().chain(self.game.iter().cloned()).find(|r| has(r, values))
    }

    pub fn game_rows(&self) -> &[Row] {
        &self.game
    }

    /// Whether the project has a row with these values.
    pub fn changed(&self, values: &[(&str, &str)]) -> bool {
        self.project_rows().iter().any(|r| has(r, values))
    }

    /// Puts a row in the project: `base` (a row of the game's, or the project's own) with `values` changed, in the place of the project's
    /// row of the same key.
    ///
    /// # Errors
    ///
    /// A value doesn't fit its column.
    pub fn set(&mut self, base: &Row, values: &[(&str, &str)]) -> Result<(), String> {
        let mut row = base.clone();
        for (c, v) in values {
            row.insert((*c).to_owned(), (*v).to_owned());
        }

        let fields = self.project.definition().fields_processed().to_vec();
        let mut cells = Vec::with_capacity(fields.len());
        for f in &fields {
            let value = row.get(f.name()).cloned().unwrap_or_default();
            let mut cell = DecodedData::new_from_type_and_value(f.field_type(), &None);
            cell.set_data(&value).map_err(|e| format!("{}.{}: '{value}': {e}", self.name, f.name()))?;
            cells.push(cell);
        }

        let key = self.key_of(&row);
        let at = self.project.data().iter().position(|r| self.key_of(&text_row(&self.project, r)) == key);
        let data = self.project.data_mut();
        match at {
            Some(at) => data[at] = cells,
            None => data.push(cells),
        }

        Ok(())
    }

    /// Takes the project's rows with these values away: the game's show again.
    pub fn remove(&mut self, values: &[(&str, &str)]) {
        let keep: Vec<bool> = self.project.data().iter().map(|r| !has(&text_row(&self.project, r), values)).collect();
        let mut k = 0;
        self.project.data_mut().retain(|_| {
            k += 1;
            keep[k - 1]
        });
    }
}

/// The three tables.
pub struct LooksTables {
    pub tables: BTreeMap<String, Table>,
}

/// Where the project's file of a table is, in the packs' paths.
pub fn project_path(table: &str) -> String {
    format!("db/{table}_tables/{TABLE_FILE}")
}

fn decode(bytes: Vec<u8>, schema: &Schema, table: &str) -> Result<DB, String> {
    let name = format!("{table}_tables");
    let mut extra = DecodeableExtraData::default();
    extra.set_schema(Some(schema));
    extra.set_table_name(Some(&name));
    DB::decode(&mut Cursor::new(bytes), &Some(extra)).map_err(|e| e.to_string())
}

impl LooksTables {
    /// The game's rows (the mods' too, the project's file aside) and the project's, from `loose` (the project's loose folder).
    ///
    /// # Errors
    ///
    /// A table doesn't decode, or the game has none to take the definition from.
    pub fn load(vfs: &Vfs, schema: &Schema, loose: &Path) -> Result<Self, String> {
        let mut tables = BTreeMap::new();
        for table in TABLES {
            let mut game = vec![];
            let mut template = None;
            for path in vfs.list(&format!("db/{table}_tables/"), "") {
                if path.ends_with(TABLE_FILE) {
                    continue;
                }

                let db = decode(vfs.read(path).map_err(|e| e.to_string())?, schema, table).map_err(|e| format!("{path}: {e}"))?;
                game.extend(db.data().iter().map(|r| text_row(&db, r)));
                template.get_or_insert(db);
            }

            let file = loose.join(project_path(table));
            let project = if file.is_file() {
                let bytes = std::fs::read(&file).map_err(|e| format!("{}: {e}", file.display()))?;
                decode(bytes, schema, table).map_err(|e| format!("{}: {e}", file.display()))?
            } else {
                let mut empty = template.ok_or_else(|| format!("the game has no {table} table"))?;
                empty.data_mut().clear();
                empty
            };

            let definition = project.definition();
            let keys = definition.key_column_positions().iter().map(|&k| definition.fields_processed()[k].name().to_owned()).collect();
            tables.insert(table.to_owned(), Table { name: table.to_owned(), keys, project, game });
        }

        Ok(Self { tables })
    }

    pub fn table(&self, name: &str) -> &Table {
        &self.tables[name]
    }

    pub fn table_mut(&mut self, name: &str) -> &mut Table {
        self.tables.get_mut(name).expect("one of TABLES")
    }

    /// Writes the project's files (a table without rows: its file goes). Returns the paths written or removed, in the packs' terms.
    ///
    /// # Errors
    ///
    /// rpfm doesn't know the game, a table doesn't encode, or a file can't be written.
    pub fn save(&mut self, game: &str, loose: &Path) -> Result<Vec<String>, String> {
        let games = SupportedGames::default();
        let info = games.game(game).ok_or_else(|| format!("rpfm does not know {game}"))?;
        let extra = Some(EncodeableExtraData::new_from_game_info(info));
        let mut changed = vec![];
        for (name, table) in &mut self.tables {
            let path = project_path(name);
            let file: PathBuf = loose.join(&path);
            if table.project.data().is_empty() {
                if file.is_file() {
                    std::fs::remove_file(&file).map_err(|e| format!("{}: {e}", file.display()))?;
                    changed.push(path);
                }

                continue;
            }

            let mut bytes = vec![];
            table.project.encode(&mut bytes, &extra).map_err(|e| format!("{name}: {e}"))?;
            if std::fs::read(&file).is_ok_and(|old| old == bytes) {
                continue;
            }

            if let Some(dir) = file.parent() {
                std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
            }

            wce_foundation::files::write_replacing(&file, bytes).map_err(|e| format!("{}: {e}", file.display()))?;
            changed.push(path);
        }

        Ok(changed)
    }
}
