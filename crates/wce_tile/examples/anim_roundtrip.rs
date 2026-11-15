//! Every `.anim` of the game's packs (and the enabled mods') read as `wce_formats::anim::RawAnim` and written back: version 5 files must
//! come back byte for byte (version 4 ones are not read).
//!
//!   cargo run --release -p wce_tile --example anim_roundtrip -- <game dir> [prefix]

use std::path::PathBuf;

use wce_formats::anim::RawAnim;
use wce_vfs::{Config, GameProfile, Vfs};

fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let config = Config::new(PathBuf::from(&a[0]), GameProfile::from_env().clone());
    let vfs = Vfs::mount(&config).expect("game files");
    let prefix = a.get(1).map_or("", String::as_str);
    let (mut same, mut other, mut v4, mut bad) = (0, 0, 0, vec![]);
    for path in vfs.list(prefix, ".anim") {
        let Ok(bytes) = vfs.read(path) else { continue };
        match RawAnim::decode(&bytes) {
            Some(raw) if raw.encode() == bytes => same += 1,
            Some(_) => {
                other += 1;
                if bad.len() < 10 {
                    bad.push(path.to_owned());
                }
            }
            None if bytes.get(..4) == Some(&4u32.to_le_bytes()[..]) => v4 += 1,
            None => {
                if bad.len() < 10 {
                    bad.push(format!("{path} (unread)"));
                }
            }
        }
    }

    println!("{same} byte for byte, {other} different, {v4} version 4 (not read)");
    for b in bad {
        println!("  {b}");
    }
}
