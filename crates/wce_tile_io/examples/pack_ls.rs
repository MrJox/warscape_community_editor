//! Files of Attila's packs whose path contains a text: one line per file (pack, path, size), and with `--extract DIR` the files below DIR
//! as `<pack name>/<path>`.
//!
//!   cargo run --release -p wce_tile_io --example pack_ls -- <pack or folder of packs> <text> [--extract DIR]

use std::path::{Path, PathBuf};

use rpfm_lib::files::pack::Pack;
use rpfm_lib::games::supported_games::SupportedGames;

fn main() {
    let args = std::env::args().skip(1).collect::<Vec<_>>();
    let (Some(input), Some(text)) = (args.first(), args.get(1)) else {
        eprintln!("usage: pack_ls <pack or folder of packs> <text> [--extract DIR]");
        std::process::exit(2);
    };

    let extract = args.iter().position(|a| a == "--extract").and_then(|i| args.get(i + 1)).map(PathBuf::from);
    let text = text.to_lowercase();
    let input = Path::new(input);
    let mut packs = if input.is_dir() {
        std::fs::read_dir(input).expect("folder").flatten().map(|e| e.path()).filter(|p| p.extension().is_some_and(|x| x == "pack")).collect::<Vec<_>>()
    } else {
        vec![input.to_path_buf()]
    };

    packs.sort();
    let games = SupportedGames::default();
    let attila = games.game("attila").expect("attila");
    for p in packs {
        let Ok(mut pack) = Pack::read_and_merge(std::slice::from_ref(&p), attila, true, false, false) else { continue };
        let pack_name = p.file_name().unwrap().to_string_lossy().into_owned();
        let names = pack.files().keys().filter(|n| n.to_lowercase().contains(&text)).cloned().collect::<Vec<_>>();
        for n in names {
            let Some(Ok(Some(bytes))) = pack.files_mut().get_mut(&n).map(|f| f.encode(&None, false, false, true)) else { continue };
            println!("{pack_name}\t{n}\t{}", bytes.len());
            if let Some(dir) = &extract {
                let out = dir.join(&pack_name).join(&n);
                std::fs::create_dir_all(out.parent().unwrap()).expect("mkdir");
                std::fs::write(out, &bytes).expect("write");
            }
        }
    }
}
