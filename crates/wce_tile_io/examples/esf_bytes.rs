//! Writes the byte arrays of an ESF file's records whose path contains a text, one file each (`<out prefix><n>.bin`), with the record's
//! other values on stdout.
//!
//!   cargo run --release -p wce_tile_io --example esf_bytes -- <file.esf> <text in the path, e.g. HEX_MAP_DATA> <out prefix>

use std::io::Cursor;

use rpfm_lib::files::Decodeable;
use rpfm_lib::files::esf::{ESF, NodeType};

fn walk(n: &NodeType, path: &str, filter: &str, out: &str, k: &mut usize) {
    let NodeType::Record(r) = n else { return };
    let here = if path.is_empty() { r.name().to_owned() } else { format!("{path}/{}", r.name()) };
    for group in r.children() {
        if here.contains(filter) {
            for c in group {
                match c {
                    NodeType::U8Array(v) => {
                        let f = format!("{out}{k}.bin");
                        std::fs::write(&f, v).expect("write");
                        println!("{here}: {} bytes -> {f}", v.len());
                        *k += 1;
                    }

                    NodeType::Record(_) => {}
                    other => println!("{here}: {other:?}"),
                }
            }
        }

        for c in group {
            walk(c, &here, filter, out, k);
        }
    }
}

fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let bytes = std::fs::read(&a[0]).expect("read");
    let esf = ESF::decode(&mut Cursor::new(&bytes), &None).expect("an ESF");
    walk(esf.root_node(), "", &a[1], &a[2], &mut 0);
}
