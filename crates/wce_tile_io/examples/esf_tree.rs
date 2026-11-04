//! An ESF file's record tree (a startpos, a campaign map's `map_data.esf`): each distinct record path once, with how many records have it
//! and the kinds of their values; only the paths containing a text, when given.
//!
//!   cargo run --release -p wce_tile_io --example esf_tree -- <file.esf> [text in the path] [--values N]
//!
//! `--values N`: the values of the first N records of each path shown.

use std::collections::BTreeMap;
use std::io::Cursor;

use rpfm_lib::files::esf::{ESF, NodeType};
use rpfm_lib::files::Decodeable;

#[derive(Default)]
struct Seen {
    count: usize,
    kinds: Vec<String>,
    values: Vec<String>,
}

fn kind(n: &NodeType) -> String {
    format!("{n:?}").split(['(', ' ']).next().unwrap_or("?").to_owned()
}

fn short(n: &NodeType) -> String {
    match n {
        NodeType::Record(r) => format!("<{}>", r.name()),
        other => {
            let s = format!("{other:?}");
            if s.len() > 60 { format!("{}...", &s[..60]) } else { s }
        }
    }
}

fn walk(n: &NodeType, path: &str, out: &mut BTreeMap<String, Seen>, keep: usize) {
    let NodeType::Record(r) = n else { return };
    let here = if path.is_empty() { r.name().to_owned() } else { format!("{path}/{}", r.name()) };
    for group in r.children() {
        let e = out.entry(here.clone()).or_default();
        e.count += 1;
        if e.kinds.is_empty() {
            e.kinds = group.iter().map(kind).collect();
        }

        if e.values.len() < keep {
            e.values.push(group.iter().map(short).collect::<Vec<_>>().join(" "));
        }

        for c in group {
            walk(c, &here, out, keep);
        }
    }
}

fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let bytes = std::fs::read(&a[0]).expect("read");
    let esf = ESF::decode(&mut Cursor::new(&bytes), &None).expect("an ESF");
    let filter = a.iter().skip(1).take_while(|x| !x.starts_with("--")).next().cloned().unwrap_or_default().to_lowercase();
    let keep = a.iter().position(|x| x == "--values").and_then(|i| a.get(i + 1)).and_then(|v| v.parse().ok()).unwrap_or(0);
    let mut out = BTreeMap::new();
    walk(esf.root_node(), "", &mut out, keep);
    for (path, s) in out.iter().filter(|(p, _)| p.to_lowercase().contains(&filter)) {
        println!("{path}  x{}  [{}]", s.count, s.kinds.iter().filter(|k| *k != "Record").cloned().collect::<Vec<_>>().join(" "));
        for v in &s.values {
            println!("    {}", if v.len() > 300 { &v[..300] } else { v });
        }
    }
}
