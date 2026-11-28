//! Writes a new pack of the game (`$WCE_GAME_KEY`, Attila by default) from files on disk: a mod pack, or with `--movie` a movie pack
//! (the only kind the game reads shaders and effects from, docs/re/game_command_line.md; `scripts/river_shader.py` builds one).
//!   cargo run --release -p wce_vfs --example pack_write -- [--movie] <out.pack> <path inside>=<file> ...
use std::path::Path;

use rpfm_lib::games::pfh_file_type::PFHFileType;
use wce_vfs::GameProfile;

fn main() {
    let mut a = std::env::args().skip(1).collect::<Vec<_>>();
    let movie = a.first().is_some_and(|s| s == "--movie");
    if movie {
        a.remove(0);
    }

    let Some((pack, rest)) = a.split_first() else {
        eprintln!("pack_write [--movie] <out.pack> <path inside>=<file> ...");
        std::process::exit(2);
    };

    let files = rest
        .iter()
        .map(|s| {
            let (inside, file) = s.split_once('=').expect("<path inside>=<file>");
            (inside.to_owned(), std::fs::read(file).expect("read"))
        })
        .collect::<Vec<_>>();
    let kind = if movie { PFHFileType::Movie } else { PFHFileType::Mod };
    wce_vfs::mod_pack::write_new_of_type(GameProfile::from_env(), Path::new(pack), &files, kind).expect("pack");
    println!("{} files -> {pack} ({})", files.len(), if movie { "movie" } else { "mod" });
}
