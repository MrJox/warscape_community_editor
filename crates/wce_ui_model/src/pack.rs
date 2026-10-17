//! Writing a layout into a mod pack.

use std::path::Path;

use rpfm_lib::files::pack::Pack;
use rpfm_lib::files::{Container, FileType, RFile};
use rpfm_lib::games::pfh_file_type::PFHFileType;
use rpfm_lib::games::supported_games::SupportedGames;

/// Puts a file into a mod pack, creating the pack if it does not exist.
pub fn write_pack(pack_path: &Path, inside: &str, data: &[u8]) -> Result<(), String> {
        let games = SupportedGames::default();
    let game = games.game("attila").ok_or("rpfm does not know attila")?;
    let mut pack = if pack_path.exists() {
        Pack::read_and_merge(std::slice::from_ref(&pack_path.to_path_buf()), game, false, false, false).map_err(|e| e.to_string())?
    } else {
        let name = pack_path.file_name().and_then(|n| n.to_str()).unwrap_or("mod.pack");
        let mut p = Pack::new_with_name_and_version(name, game.pfh_version_by_file_type(PFHFileType::Mod));
        p.set_pfh_file_type(PFHFileType::Mod);
        p
    };
    let file = RFile::new_from_vec(data, FileType::Unknown, 0, inside);
    pack.insert(file).map_err(|e| e.to_string())?;
    pack.save(Some(pack_path), game, &None).map_err(|e| e.to_string())
}
