//! Writing a layout into a mod pack.

use std::path::Path;

use wce_vfs::GameProfile;

/// Puts a file into a mod pack of `game`, creating the pack if it does not exist.
pub fn write_pack(game: &GameProfile, pack_path: &Path, inside: &str, data: &[u8]) -> Result<(), String> {
    wce_vfs::mod_pack::write_file(game, pack_path, inside, data)
}

/// Writes a new mod pack of `game` (an existing file at `pack_path` is replaced) holding `files`: (path inside the pack, bytes).
pub fn write_pack_many(game: &GameProfile, pack_path: &Path, files: &[(String, Vec<u8>)]) -> Result<(), String> {
    wce_vfs::mod_pack::write_new(game, pack_path, files)
}
