//! Writing files into a mod pack of a game.

use std::path::Path;

use rpfm_lib::files::pack::Pack;
use rpfm_lib::files::{Container, FileType, RFile};
use rpfm_lib::games::pfh_file_type::PFHFileType;
use rpfm_lib::games::supported_games::SupportedGames;

use crate::GameProfile;

/// Puts a file into a mod pack of `game`, creating the pack if it does not exist.
///
/// # Errors
///
/// rpfm doesn't know the game, or the pack can't be read or written.
pub fn write_file(game: &GameProfile, pack_path: &Path, inside: &str, data: &[u8]) -> Result<(), String> {
    let games = SupportedGames::default();
    let info = games.game(game.key).ok_or_else(|| format!("rpfm does not know {}", game.key))?;
    let mut pack = if pack_path.exists() {
        Pack::read_and_merge(std::slice::from_ref(&pack_path.to_path_buf()), info, false, false, false).map_err(|e| e.to_string())?
    } else {
        let name = pack_path.file_name().and_then(|n| n.to_str()).unwrap_or("mod.pack");
        let mut p = Pack::new_with_name_and_version(name, info.pfh_version_by_file_type(PFHFileType::Mod));
        p.set_pfh_file_type(PFHFileType::Mod);
        p
    };

    pack.insert(RFile::new_from_vec(data, FileType::Unknown, 0, inside)).map_err(|e| e.to_string())?;
    pack.save(Some(pack_path), info, &None).map_err(|e| e.to_string())
}

/// Writes a new mod pack of `game` (an existing file at `pack_path` is replaced) holding `files`: (path inside the pack, bytes).
///
/// # Errors
///
/// rpfm doesn't know the game, or the pack can't be written.
pub fn write_new(game: &GameProfile, pack_path: &Path, files: &[(String, Vec<u8>)]) -> Result<(), String> {
    let games = SupportedGames::default();
    let info = games.game(game.key).ok_or_else(|| format!("rpfm does not know {}", game.key))?;
    let name = pack_path.file_name().and_then(|n| n.to_str()).unwrap_or("mod.pack");
    let mut pack = Pack::new_with_name_and_version(name, info.pfh_version_by_file_type(PFHFileType::Mod));
    pack.set_pfh_file_type(PFHFileType::Mod);
    for (inside, data) in files {
        pack.insert(RFile::new_from_vec(data, FileType::Unknown, 0, inside)).map_err(|e| e.to_string())?;
    }

    if let Some(parent) = pack_path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }

    pack.save(Some(pack_path), info, &None).map_err(|e| e.to_string())
}
