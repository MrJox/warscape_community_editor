//! A binary `building_list` read as `attila::building_list`: each building's key, position and scale, one a line (a test of what an export
//! wrote: the kit walls' pieces, docs/plans/kit_walls.md phase 5).
//!
//!   cargo run --release -p wce_tile_io --example building_list_dump -- <building_list> [key filter]

use std::io::Cursor;

use rpfm_lib::files::Decodeable;
use rpfm_lib::files::esf::ESF;
use wce_tile_io::attila::building_list::read;

fn main() {
    let args = std::env::args().skip(1).collect::<Vec<_>>();
    let Some(list) = args.first() else {
        eprintln!("usage: building_list_dump <building_list> [key filter]");
        std::process::exit(2);
    };

    let esf = ESF::decode(&mut Cursor::new(std::fs::read(list).expect("read")), &None).expect("ESF");
    let filter = args.get(1).cloned().unwrap_or_default();
    for b in read(&esf).expect("building list").iter().filter(|b| b.key.contains(&filter)) {
        println!("{} at ({:.2}, {:.2}, {:.2}) scale {:?}", b.key, b.position[0], b.position[1], b.position[2], b.scale);
    }
}
