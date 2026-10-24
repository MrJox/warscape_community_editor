//! The pack the generated buildings go into: a mod pack of the game with their folders and database rows.

use std::path::Path;

use rpfm_lib::files::db::DB;
use rpfm_lib::files::pack::Pack;
use rpfm_lib::files::{Container, EncodeableExtraData, FileType, RFile, RFileDecoded};
use rpfm_lib::games::pfh_file_type::PFHFileType;
use rpfm_lib::games::supported_games::SupportedGames;

/// Writes a mod pack for `game` (rpfm's key, `attila`) with the files (path, bytes) and the tables (`<name>_tables` -> rows), each table
/// as `db/<name>_tables/<file stem of the pack>`.
///
/// # Errors
///
/// The game is unknown, a file can't be added or the pack can't be written.
pub fn write_pack(path: &Path, game: &str, files: Vec<(String, Vec<u8>)>, tables: Vec<(String, DB)>) -> Result<(), String> {
    let games = SupportedGames::default();
    let info = games.game(game).ok_or_else(|| format!("rpfm knows no {game}"))?;
    let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).ok_or("no pack name")?;
    let stem = name.trim_end_matches(".pack").to_owned();

    let mut pack = Pack::new_with_name_and_version(&name, info.pfh_version_by_file_type(PFHFileType::Mod));
    pack.set_pfh_file_type(PFHFileType::Mod);
    for (p, bytes) in files {
        // Raw bytes: written as they are, not decoded again.
        pack.insert(RFile::new_from_vec(&bytes, FileType::Unknown, 0, &p)).map_err(|e| format!("{p}: {e}"))?;
    }

    for (table, db) in tables {
        let p = format!("db/{table}/{stem}");
        pack.insert(RFile::new_from_decoded(&RFileDecoded::DB(db), 0, &p)).map_err(|e| format!("{p}: {e}"))?;
    }

    let mut extra = EncodeableExtraData::default();
    extra.set_nullify_dates(true);
    extra.set_game_info(Some(info));
    pack.save(Some(path), info, &Some(extra)).map_err(|e| format!("{}: {e}", path.display()))
}
