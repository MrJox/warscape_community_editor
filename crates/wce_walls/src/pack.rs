//! The pack the generated buildings go into: a mod pack of the game with their folders and database rows.

use std::path::Path;

use rpfm_lib::files::db::DB;
use rpfm_lib::files::pack::Pack;
use rpfm_lib::files::{Container, EncodeableExtraData, FileType, RFile, RFileDecoded};
use rpfm_lib::games::pfh_file_type::PFHFileType;
use rpfm_lib::games::supported_games::SupportedGames;

/// Writes a mod pack for `game` (rpfm's key: the game profile's) with the files (path, bytes) and the tables (`<name>_tables` -> rows), each table
/// as `db/<name>_tables/<file stem of the pack>`.
///
/// # Errors
///
/// The game is unknown, a file can't be added or the pack can't be written.
pub fn write_pack(path: &Path, game: &str, files: Vec<(String, Vec<u8>)>, tables: Vec<(String, DB)>) -> Result<(), String> {
    let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).ok_or("no pack name")?;
    let stem = name.trim_end_matches(".pack").to_owned();
    write_pack_as(path, game, PFHFileType::Mod, files, tables, &stem)
}

/// [`write_pack`] of a type, each table as `db/<name>_tables/<table_file>`.
///
/// # Errors
///
/// As [`write_pack`].
pub fn write_pack_as(path: &Path, game: &str, kind: PFHFileType, files: Vec<(String, Vec<u8>)>, tables: Vec<(String, DB)>, table_file: &str) -> Result<(), String> {
    let games = SupportedGames::default();
    let info = games.game(game).ok_or_else(|| format!("rpfm knows no {game}"))?;
    let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).ok_or("no pack name")?;

    let mut pack = Pack::new_with_name_and_version(&name, info.pfh_version_by_file_type(kind));
    pack.set_pfh_file_type(kind);
    for (p, bytes) in files {
        // Raw bytes: written as they are, not decoded again.
        pack.insert(RFile::new_from_vec(&bytes, FileType::Unknown, 0, &p)).map_err(|e| format!("{p}: {e}"))?;
    }

    for (table, db) in tables {
        let p = format!("db/{table}/{table_file}");
        pack.insert(RFile::new_from_decoded(&RFileDecoded::DB(db), 0, &p)).map_err(|e| format!("{p}: {e}"))?;
    }

    let mut extra = EncodeableExtraData::default();
    extra.set_nullify_dates(true);
    extra.set_game_info(Some(info));
    pack.save(Some(path), info, &Some(extra)).map_err(|e| format!("{}: {e}", path.display()))
}

/// The files of a pack (its tables too), as they are, sorted by path.
///
/// # Errors
///
/// The game is unknown, or the pack or a file in it doesn't read.
pub fn read_pack_files(path: &Path, game: &str) -> Result<Vec<(String, Vec<u8>)>, String> {
    let games = SupportedGames::default();
    let info = games.game(game).ok_or_else(|| format!("rpfm knows no {game}"))?;
    let mut pack = Pack::read_and_merge(&[path.to_path_buf()], info, true, false, false).map_err(|e| format!("{}: {e}", path.display()))?;
    let mut names = pack.files().keys().cloned().collect::<Vec<_>>();
    names.sort();
    let mut out = vec![];
    for n in names {
        let Some(Ok(Some(bytes))) = pack.files_mut().get_mut(&n).map(|f| f.encode(&None, false, false, true)) else { return Err(format!("{}: {n}: no bytes", path.display())) };
        out.push((n, bytes));
    }

    Ok(out)
}

/// The editor's pack of generated buildings for TEd, in the game's `data` (a movie pack: the game and TEd always load it). Every tile saved
/// for the Assembly Kit with procedural walls has its pieces in it: their folders and its own file of each table (`db/<table>/<prefix>`).
pub const PROC_PACK: &str = "@wce_proc_data.pack";

/// Whether a file of [`PROC_PACK`] is a tile's (its walls' keys are `<prefix>_<wall>_<piece>`): its table files, its pieces' folders.
pub fn is_tiles_file(path: &str, prefix: &str) -> bool {
    let p = path.to_lowercase();
    let prefix = prefix.to_lowercase();
    if let Some(rest) = p.strip_prefix("db/") {
        return rest.split_once('/').is_some_and(|(_, file)| file == prefix);
    }

    let Some(key) = p.strip_prefix("rigidmodels/buildings/").and_then(|r| r.split('/').next()) else { return false };
    let Some(numbers) = key.strip_prefix(&prefix).and_then(|r| r.strip_prefix('_')) else { return false };
    numbers.split_once('_').is_some_and(|(w, i)| !w.is_empty() && !i.is_empty() && w.bytes().chain(i.bytes()).all(|b| b.is_ascii_digit()))
}

/// Puts a tile's pieces (`files`, `tables`) into [`PROC_PACK`] at `path`, in place of what it had of the tile's (`prefix`); the other
/// tiles' stay. A tile without walls takes its pieces out. Returns how many files of the other tiles were kept.
///
/// # Errors
///
/// The pack exists and doesn't read, or it can't be written.
pub fn replace_in_proc_pack(path: &Path, game: &str, prefix: &str, files: Vec<(String, Vec<u8>)>, tables: Vec<(String, DB)>) -> Result<usize, String> {
    let existing = if path.is_file() { read_pack_files(path, game)? } else { vec![] };
    let mut kept = existing.into_iter().filter(|(p, _)| !is_tiles_file(p, prefix)).collect::<Vec<_>>();
    let others = kept.len();
    if kept.is_empty() && files.is_empty() && tables.is_empty() {
        if path.is_file() {
            std::fs::remove_file(path).map_err(|e| format!("{}: {e}", path.display()))?;
        }

        return Ok(0);
    }

    kept.extend(files);
    write_pack_as(path, game, PFHFileType::Movie, kept, tables, &prefix.to_lowercase())?;
    Ok(others)
}

/// Puts a tile into a mod pack at `path` (created if missing): its files (`folder`'s, the tile database's, its walls' pieces) and its
/// walls' rows (`db/<table>/<prefix>`), in place of what the pack had of the tile (`folder`'s files, [`is_tiles_file`] of `prefix`, the
/// files written again); the rest stays. Returns how many files were kept.
///
/// # Errors
///
/// The pack exists and doesn't read, or it can't be written.
pub fn replace_in_pack(path: &Path, game: &str, prefix: &str, folder: &str, files: Vec<(String, Vec<u8>)>, tables: Vec<(String, DB)>) -> Result<usize, String> {
    let existing = if path.is_file() { read_pack_files(path, game)? } else { vec![] };
    let below = format!("{}/", folder.trim_end_matches('/').to_lowercase());
    let written = files.iter().map(|(p, _)| p.to_lowercase()).collect::<std::collections::HashSet<_>>();
    let mut kept = existing
        .into_iter()
        .filter(|(p, _)| {
            let l = p.replace('\\', "/").to_lowercase();
            !is_tiles_file(&l, prefix) && !l.starts_with(&below) && !written.contains(&l)
        })
        .collect::<Vec<_>>();
    let others = kept.len();
    kept.extend(files);
    write_pack_as(path, game, PFHFileType::Mod, kept, tables, &prefix.to_lowercase())?;
    Ok(others)
}

#[cfg(test)]
mod tests {
    use super::is_tiles_file;

    #[test]
    fn a_tiles_files() {
        let p = "wce_farm_western_1x1_aa";
        assert!(is_tiles_file("db/battlefield_buildings_tables/wce_farm_western_1x1_aa", p));
        assert!(is_tiles_file("rigidmodels/buildings/wce_farm_western_1x1_aa_0_03/x_tech.cs2.parsed", p));
        assert!(!is_tiles_file("db/battlefield_buildings_tables/wce_farm_western_1x1_ab", p));
        assert!(!is_tiles_file("rigidmodels/buildings/wce_farm_western_1x1_aa_b_0_03/x.rigid_model_v2", p));
        assert!(!is_tiles_file("rigidmodels/buildings/wce_farm_western_1x1_aa_0/x.rigid_model_v2", p));
    }
}
