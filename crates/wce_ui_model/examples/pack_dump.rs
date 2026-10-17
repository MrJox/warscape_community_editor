//! Reads a mod pack back, for the scripted checks.
//!
//!   pack_dump list <pack>                 one line per file: path, a tab, its size
//!   pack_dump get <pack> <inside> <out>   writes one file of the pack

use rpfm_lib::files::pack::Pack;
use rpfm_lib::games::supported_games::SupportedGames;

fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let (Some(cmd), Some(pack_path)) = (a.first(), a.get(1)) else {
        eprintln!("pack_dump list <pack> | get <pack> <inside> <out>");
        std::process::exit(2);
    };

    let games = SupportedGames::default();
    let game = games.game("attila").expect("attila");
    let mut pack = Pack::read_and_merge(&[pack_path.into()], game, false, false, false).unwrap_or_else(|e| {
        eprintln!("cannot read {pack_path}: {e}");
        std::process::exit(1)
    });

    let mut names: Vec<String> = pack.files().keys().cloned().collect();
    names.sort();
    let mut read = |name: &str| -> Vec<u8> {
        let rf = pack.files_mut().get_mut(name).unwrap_or_else(|| {
            eprintln!("{name} is not in the pack");
            std::process::exit(1)
        });
        rf.encode(&None, false, false, true).ok().flatten().unwrap_or_default()
    };

    match cmd.as_str() {
        "list" => {
            for n in &names {
                println!("{n}\t{}", read(n).len());
            }
        }
        "get" => {
            let (Some(inside), Some(out)) = (a.get(2), a.get(3)) else {
                eprintln!("pack_dump get <pack> <inside> <out>");
                std::process::exit(2);
            };

            std::fs::write(out, read(&inside.to_lowercase())).expect("write");
        }
        _ => std::process::exit(2),
    }
}
