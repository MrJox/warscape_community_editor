//! Every entry of a kit's tile database: its links taken out and put back (`TileEntry::set_links`) give its bytes back, and each edit
//! reads back as written.
//!
//!   cargo run --release -p wce_formats --example tile_links_roundtrip -- <kit raw_data>/terrain/tiles/battle/_tile_database/TILES

use wce_formats::tile_entry::TileEntry;

fn main() {
    let dir = std::env::args().nth(1).expect("usage: tile_links_roundtrip <TILES folder>");
    let (mut n, mut bad) = (0, 0);
    for f in std::fs::read_dir(&dir).expect("a folder").flatten().map(|f| f.path()).filter(|p| p.extension().is_some_and(|x| x == "xml")) {
        let bytes = std::fs::read(&f).expect("readable");
        let Ok(mut e) = TileEntry::parse(bytes.clone()) else { continue };
        n += 1;
        let (targets, links) = (e.link_targets(), e.links());
        let mut fail = |why: &str| {
            bad += 1;
            println!("{}: {why}", f.display());
        };

        if e.set_links(&targets, &links).unwrap_or(true) || e.save() != bytes {
            fail("unchanged set changed it");
            continue;
        }

        // Without the last target and the last link, then with them again.
        let (t2, l2) = (&targets[..targets.len().saturating_sub(1)], &links[..links.len().saturating_sub(1)]);
        if e.set_links(t2, l2).is_err() || e.link_targets().len() != t2.len() || e.links().len() != l2.len() {
            fail("taking one out");
            continue;
        }

        if e.set_links(&targets, &links).is_err() || e.save() != bytes {
            fail("put back: not the same bytes");
        }
    }

    println!("{n} entries, {bad} failed");
}
