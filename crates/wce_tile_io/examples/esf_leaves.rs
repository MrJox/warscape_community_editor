//! Every record of a name in an ESF file, its leaves in full (numbers, bools, texts; records as `<NAME>`), and per leaf position the
//! values it takes and how often.
//!
//!   cargo run --release -p wce_tile_io --example esf_leaves -- <file.esf> <record name> [rows to print]

use std::collections::BTreeMap;
use std::io::Cursor;

use rpfm_lib::files::Decodeable;
use rpfm_lib::files::esf::{ESF, NodeType, RecordNode};

fn text(n: &NodeType) -> String {
    match n {
        NodeType::Record(r) => format!("<{}>", r.name()),
        NodeType::Bool(v) => v.value().to_string(),
        NodeType::U8(v) => v.to_string(),
        NodeType::U16(v) => v.to_string(),
        NodeType::I16(v) => v.to_string(),
        NodeType::U32(v) => v.value().to_string(),
        NodeType::I32(v) => v.value().to_string(),
        NodeType::F32(v) => v.value().to_string(),
        NodeType::Coord2d(c) => format!("({}, {})", c.x(), c.y()),
        NodeType::Ascii(s) | NodeType::Utf16(s) => format!("{s:?}"),
        other => {
            let s = format!("{other:?}");
            if s.len() > 40 { format!("{}...", &s[..40]) } else { s }
        }
    }
}

fn walk(r: &RecordNode, name: &str, out: &mut Vec<Vec<String>>) {
    for g in r.children() {
        if r.name() == name {
            out.push(g.iter().map(text).collect());
        }

        for c in g {
            if let NodeType::Record(child) = c {
                walk(child, name, out);
            }
        }
    }
}

fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let bytes = std::fs::read(&a[0]).expect("read");
    let esf = ESF::decode(&mut Cursor::new(&bytes), &None).expect("an ESF");
    let rows_shown: usize = a.get(2).and_then(|v| v.parse().ok()).unwrap_or(5);
    let NodeType::Record(root) = esf.root_node() else { return };
    let mut rows = vec![];
    walk(root, &a[1], &mut rows);
    println!("{} records", rows.len());
    for r in rows.iter().take(rows_shown) {
        println!("  {}", r.join(" | "));
    }

    let width = rows.iter().map(Vec::len).max().unwrap_or(0);
    for k in 0..width {
        let mut h: BTreeMap<&str, usize> = BTreeMap::new();
        for r in &rows {
            if let Some(v) = r.get(k) {
                *h.entry(v.as_str()).or_default() += 1;
            }
        }

        let mut top: Vec<(&str, usize)> = h.into_iter().collect();
        top.sort_by_key(|t| std::cmp::Reverse(t.1));
        let n = top.len();
        let shown: Vec<String> = top.iter().take(10).map(|(v, c)| format!("{v} x{c}")).collect();
        println!("leaf {k}: {n} values: {}", shown.join(", "));
    }
}
