//! A generated building put into a pack: its folder (`rigidmodels/buildings/<key>/`) and its rows (`db/<table>/<key>`), in place of what
//! the pack had of that key; the pack's other files stay (wce_walls::pack writes it).

use std::path::Path;

use rpfm_lib::files::db::DB;
use rpfm_lib::games::pfh_file_type::PFHFileType;
use wce_walls::pack::{read_pack_files, write_pack_as};

/// Whether a pack's file is the building's (its folder, or its table files).
pub fn is_buildings_file(path: &str, key: &str) -> bool {
    let p = path.replace('\\', "/").to_lowercase();
    let key = key.to_lowercase();
    if let Some(rest) = p.strip_prefix("db/") {
        return rest.split_once('/').is_some_and(|(_, file)| file == key);
    }

    p.starts_with(&format!("rigidmodels/buildings/{key}/"))
}

/// Puts the building into the pack at `path` (made if missing) of type `kind` for `game` (rpfm's key). Returns how many of the pack's
/// files were kept.
///
/// # Errors
///
/// The pack exists and doesn't read, or it can't be written.
pub fn put_building(path: &Path, game: &str, kind: PFHFileType, key: &str, files: Vec<(String, Vec<u8>)>, tables: Vec<(String, DB)>) -> Result<usize, String> {
    let existing = if path.is_file() { read_pack_files(path, game)? } else { vec![] };
    let mut kept = existing.into_iter().filter(|(p, _)| !is_buildings_file(p, key)).collect::<Vec<_>>();
    let others = kept.len();
    kept.extend(files);
    write_pack_as(path, game, kind, kept, tables, &key.to_lowercase())?;
    Ok(others)
}

#[cfg(test)]
mod tests {
    use super::is_buildings_file;

    #[test]
    fn a_buildings_files() {
        assert!(is_buildings_file("db/battlefield_buildings_tables/wce_x", "wce_x"));
        assert!(is_buildings_file("rigidmodels/buildings/WCE_X/wce_x_tech.cs2.parsed", "wce_x"));
        assert!(!is_buildings_file("db/battlefield_buildings_tables/wce_xy", "wce_x"));
        assert!(!is_buildings_file("rigidmodels/buildings/wce_xy/a.rigid_model_v2", "wce_x"));
    }
}
