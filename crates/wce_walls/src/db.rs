//! A base building's database rows copied under a generated key: every row of the game's tables (and the enabled mods') that names the
//! base, with the base's key replaced, also inside paths to its folder (`models_building.logic_file`,
//! `buildings\<key>\<key>_tech.cs2.parsed`). In the game: `battlefield_buildings` and `models_building`; the kit's `TWaD_*` tables
//! are not in the packs. `models_building` also holds the firing and docking lines ([`crate::lines`]), which a warped piece must change
//! ([`cell_mut`]).

use std::collections::{BTreeMap, BTreeSet};
use std::io::Cursor;

use rpfm_lib::files::db::DB;
use rpfm_lib::files::table::DecodedData;
use rpfm_lib::files::{Decodeable, DecodeableExtraData};
use rpfm_lib::schema::Schema;
use wce_vfs::Vfs;

/// The rows copied, per table (`<name>_tables`): a table with the source's definition and only the new rows.
pub struct CopiedRows {
    pub tables: BTreeMap<String, DB>,

    /// Per table, the files the rows came from.
    pub sources: BTreeMap<String, BTreeSet<String>>,
}

/// Copies, for each `(base, new)` pair, the rows that name `base`. A row is taken from the table file of the highest priority pack only
/// (rows are told apart by their `key` column, else their first).
///
/// # Errors
///
/// A table that names a base doesn't decode.
pub fn copy_rows(vfs: &Vfs, schema: &Schema, renames: &[(String, String)]) -> Result<CopiedRows, String> {
    let mut paths = vfs.list("db/", "").into_iter().map(str::to_owned).collect::<Vec<_>>();
    paths.sort_by_key(|p| vfs.owner(p));

    let needles = renames.iter().flat_map(|(base, _)| [base.as_bytes().to_vec(), utf16(base)]).collect::<Vec<_>>();
    let mut out = CopiedRows { tables: BTreeMap::new(), sources: BTreeMap::new() };
    let mut seen = BTreeSet::new();
    for path in paths {
        let Some(table) = path.strip_prefix("db/").and_then(|p| p.split('/').next()).map(str::to_owned) else { continue };
        let bytes = vfs.read(&path).map_err(|e| e.to_string())?;
        if !needles.iter().any(|n| contains(&bytes, n)) {
            continue;
        }

        let mut extra = DecodeableExtraData::default();
        extra.set_schema(Some(schema));
        extra.set_table_name(Some(&table));
        let db = DB::decode(&mut Cursor::new(&bytes), &Some(extra)).map_err(|e| format!("{path}: {e}"))?;
        let id = db.column_position_by_name("key").unwrap_or(0);
        for row in db.data().iter() {
            for (base, new) in renames {
                if !row.iter().any(|c| is_text(c, base)) {
                    continue;
                }

                let copy = row.iter().map(|c| if is_text(c, base) { renamed(c, new) } else { in_paths(c, base, new) }).collect::<Vec<_>>();
                if !seen.insert((table.clone(), copy[id].data_to_string().into_owned())) {
                    continue;
                }

                let target = out.tables.entry(table.clone()).or_insert_with(|| {
                    let mut empty = db.clone();
                    empty.data_mut().clear();
                    empty
                });
                target.data_mut().push(copy);
                out.sources.entry(table.clone()).or_default().insert(path.clone());
            }
        }
    }

    Ok(out)
}

/// The cell of a copied row (by its key) in a column.
pub fn cell_mut<'a>(db: &'a mut DB, key: &str, column: &str) -> Option<&'a mut DecodedData> {
    let (id, col) = (db.column_position_by_name("key").unwrap_or(0), db.column_position_by_name(column)?);
    db.data_mut().iter_mut().find(|r| is_text(&r[id], key)).map(|r| &mut r[col])
}

/// The bytes of a byte sequence cell.
pub fn bytes_mut(c: &mut DecodedData) -> Option<&mut Vec<u8>> {
    match c {
        DecodedData::SequenceU16(b) | DecodedData::SequenceU32(b) => Some(b),
        _ => None,
    }
}

fn is_text(c: &DecodedData, text: &str) -> bool {
    match c {
        DecodedData::StringU8(s) | DecodedData::StringU16(s) | DecodedData::OptionalStringU8(s) | DecodedData::OptionalStringU16(s) => s.eq_ignore_ascii_case(text),
        _ => false,
    }
}

fn renamed(c: &DecodedData, text: &str) -> DecodedData {
    match c {
        DecodedData::StringU8(_) => DecodedData::StringU8(text.to_owned()),
        DecodedData::StringU16(_) => DecodedData::StringU16(text.to_owned()),
        DecodedData::OptionalStringU8(_) => DecodedData::OptionalStringU8(text.to_owned()),
        DecodedData::OptionalStringU16(_) => DecodedData::OptionalStringU16(text.to_owned()),
        other => other.clone(),
    }
}

/// A text cell with paths into the base's folder pointed at the new one.
fn in_paths(c: &DecodedData, base: &str, new: &str) -> DecodedData {
    let fix = |s: &String| {
        let lower = s.to_lowercase();
        let mut out = s.clone();
        for sep in ['\\', '/'] {
            if lower.contains(&format!("{sep}{base}{sep}")) {
                out = out.replace(&format!("{sep}{base}{sep}{base}_"), &format!("{sep}{new}{sep}{new}_")).replace(&format!("{sep}{base}{sep}"), &format!("{sep}{new}{sep}"));
            }
        }
        out
    };

    match c {
        DecodedData::StringU8(s) => DecodedData::StringU8(fix(s)),
        DecodedData::StringU16(s) => DecodedData::StringU16(fix(s)),
        DecodedData::OptionalStringU8(s) => DecodedData::OptionalStringU8(fix(s)),
        DecodedData::OptionalStringU16(s) => DecodedData::OptionalStringU16(fix(s)),
        other => other.clone(),
    }
}

fn utf16(s: &str) -> Vec<u8> {
    s.encode_utf16().flat_map(u16::to_le_bytes).collect()
}

fn contains(hay: &[u8], needle: &[u8]) -> bool {
    !needle.is_empty() && hay.windows(needle.len()).any(|w| w.eq_ignore_ascii_case(needle))
}
