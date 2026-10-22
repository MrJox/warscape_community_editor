//! How the building lists' ESF values are encoded: for each value type, the `optimized` flag against the value (to find the writer's rule).
//!
//!   cargo run --release -p wce_tile_io --example tile_esf_flags -- <folder>

use std::collections::BTreeMap;
use std::io::Cursor;
use std::path::{Path, PathBuf};

use rpfm_lib::files::Decodeable;
use rpfm_lib::files::esf::{ESF, NodeType};

fn files(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    for e in entries.flatten() {
        let p = e.path();
        if p.is_dir() {
            files(&p, out);
        } else if p.file_name().is_some_and(|n| n.to_string_lossy().contains("building_list") || n.to_string_lossy().starts_with("bmd.")) && p.extension().is_none() {
            out.push(p);
        }
    }
}

fn walk(n: &NodeType, counts: &mut BTreeMap<String, usize>) {
    let mut add = |k: String| *counts.entry(k).or_default() += 1;
    match n {
        NodeType::Record(r) => {
            add(format!("record flags {:?}", r.record_flags()));
            for b in r.children() {
                for c in b {
                    walk(c, counts);
                }
            }
        }

        NodeType::F32(v) => add(format!("f32 zero={} optimized={}", *v.value() == 0.0, v.optimized())),
        NodeType::Bool(v) => add(format!("bool optimized={}", v.optimized())),
        NodeType::U32(v) => add(format!("u32 {} optimized={}", if *v.value() < 256 { "small" } else { "big" }, v.optimized())),
        NodeType::I32(v) => add(format!("i32 optimized={}", v.optimized())),
        other => add(format!("{other:?}").split('(').next().unwrap_or_default().to_owned()),
    }
}

fn main() {
    let root = std::env::args().nth(1).expect("usage: tile_esf_flags <folder>");
    let mut all = vec![];
    files(Path::new(&root), &mut all);
    let mut counts = BTreeMap::new();
    for p in &all {
        let esf = ESF::decode(&mut Cursor::new(std::fs::read(p).expect("read")), &None).expect("ESF");
        walk(esf.root_node(), &mut counts);
    }

    for (k, n) in counts {
        println!("{n:>10} {k}");
    }
}
