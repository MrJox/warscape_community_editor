//! The node tree of a CAAB ESF file of a tile (`building_list`, `far_building_list`, `bmd.grass_list` ...): records with their version and
//! children, values with their type.
//!
//!   cargo run --release -p wce_tile_io --example tile_esf_dump -- <file> [max blocks per record]

use std::io::Cursor;

use rpfm_lib::files::Decodeable;
use rpfm_lib::files::esf::{ESF, NodeType};

fn show(node: &NodeType, depth: usize, max: usize) {
    let pad = "  ".repeat(depth);
    match node {
        NodeType::Record(r) => {
            println!("{pad}{} (v{}, {} block(s))", r.name(), r.version(), r.children().len());
            for (i, block) in r.children().iter().enumerate().take(max) {
                if r.children().len() > 1 {
                    println!("{pad}  [{i}]");
                }

                for c in block {
                    show(c, depth + 2, max);
                }
            }

            if r.children().len() > max {
                println!("{pad}  ... {} more", r.children().len() - max);
            }
        }

        other => {
            let text = format!("{other:?}");
            println!("{pad}{}", if text.len() > 160 { format!("{}...", &text[..160]) } else { text });
        }
    }
}

fn main() {
    let args = std::env::args().skip(1).collect::<Vec<_>>();
    let Some(path) = args.first() else {
        eprintln!("usage: tile_esf_dump <file> [max blocks]");
        std::process::exit(2);
    };

    let max = args.get(1).and_then(|m| m.parse().ok()).unwrap_or(3);
    let esf = ESF::decode(&mut Cursor::new(std::fs::read(path).expect("read")), &None).expect("ESF");
    println!("{:?}, unknown_1 {}, creation_date {}", esf.signature(), esf.unknown_1(), esf.creation_date());
    show(esf.root_node(), 0, max);
}
