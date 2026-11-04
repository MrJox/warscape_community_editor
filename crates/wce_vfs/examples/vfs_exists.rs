//! Which of a list of paths the packs have: reads paths (one a line) from a file, prints the missing ones and a count.
//!
//!   WCE_GAME=<game folder> [WCE_GAME_KEY=<key>] cargo run --release -p wce_vfs --example vfs_exists -- <list file>

use wce_vfs::{Config, GameProfile, Vfs};

fn main() {
    let list = std::fs::read_to_string(std::env::args().nth(1).expect("list file")).expect("read");
    let mut cfg = Config::new(&std::env::var("WCE_GAME").expect("WCE_GAME"), GameProfile::from_env().clone());
    cfg.mod_packs = Some(vec![]);
    let vfs = Vfs::mount(&cfg).expect("vfs");
    let (mut have, mut missing) = (0, 0);
    for p in list.lines().map(str::trim).filter(|l| !l.is_empty()) {
        if vfs.read(p).is_ok() {
            have += 1;
        } else {
            missing += 1;
            println!("missing {p}");
        }
    }

    println!("{have} found, {missing} missing");
}
