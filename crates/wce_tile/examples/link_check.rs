//! The tile links' validator (`wce_tile::link_checks`) over every entry of a kit: the problems it reports, counted by kind, to see how
//! often CA's own entries depart from what it checks.
//!
//!   cargo run --release -p wce_tile --example link_check -- <kit raw_data>

use std::collections::BTreeMap;
use std::path::Path;

use wce_formats::tile_entry::TileEntry;
use wce_formats::xml_doc::XmlDoc;

fn main() {
    let raw = std::env::args().nth(1).expect("usage: link_check <kit raw_data>");
    let raw = Path::new(&raw);
    let settings = std::fs::read(raw.join("terrain/tiles/battle/_tile_database/_settings.xml")).expect("_settings.xml");
    let doc = XmlDoc::parse(settings).expect("xml");
    let sets = doc.elements().iter().enumerate().filter(|(_, e)| e.name == "TILE_SET").filter_map(|(i, _)| doc.get(i, "name")).collect::<Vec<_>>();
    let mut kinds: BTreeMap<String, usize> = BTreeMap::new();
    let (mut n, mut with) = (0, 0);
    for f in std::fs::read_dir(raw.join("terrain/tiles/battle/_tile_database/TILES")).expect("TILES").flatten() {
        let Ok(e) = TileEntry::load(&f.path()) else { continue };
        n += 1;
        let p = wce_tile::link_checks::link_problems(&e, &sets);
        with += usize::from(!p.is_empty());
        for x in p {
            // The kind: the message without the names and numbers.
            let kind = x.message.split(": ").last().unwrap_or_default().to_owned();
            let kind = if kind.contains("no tile set") { "no tile set".to_owned() } else { kind };
            *kinds.entry(format!("{} {kind}", if x.target { "target" } else { "rule" })).or_default() += 1;
            if std::env::var("WCE_VERBOSE").is_ok() {
                println!("{}: {}", f.path().file_name().unwrap_or_default().to_string_lossy(), x.message);
            }
        }
    }

    println!("{n} entries, {with} with problems");
    for (k, v) in kinds {
        println!("{v:5}  {k}");
    }
}
