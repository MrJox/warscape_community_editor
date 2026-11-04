//! The startpos's settlement sprawl cells (`SETTLEMENT_EXPANSION_MAP_DATA/SPRAWL_BLOCK/SPRAWL_HEX_MAP_DATA`: u16 column, u16 row, then
//! bool, bool, u8, u8, bool, u8, i32, u8, u32, u8, i32, u8): each field's values and counts, the combinations of the small ones, and a
//! region's cells in full.
//!
//!   cargo run --release -p wce_terrain_io --example sprawl_fields -- <startpos.esf> [region]

use std::collections::BTreeMap;
use std::io::Cursor;

use rpfm_lib::files::Decodeable;
use rpfm_lib::files::esf::{ESF, NodeType, RecordNode};

fn rec(n: &NodeType) -> Option<&RecordNode> {
    match n {
        NodeType::Record(r) => Some(r),
        _ => None,
    }
}

fn val(n: &NodeType) -> Option<i64> {
    Some(match n {
        NodeType::Bool(b) => i64::from(*b.value()),
        NodeType::U8(v) => i64::from(*v),
        NodeType::U16(v) => i64::from(*v),
        NodeType::U32(v) => i64::from(*v.value()),
        NodeType::I32(v) => i64::from(*v.value()),
        _ => return None,
    })
}

fn walk<'a>(r: &'a RecordNode, region: &str, out: &mut Vec<(String, Vec<i64>)>) {
    let mut here = region.to_owned();
    for g in r.children() {
        if r.name() == "SETTLEMENT_EXPANSION_MAP_DATA"
            && let Some(NodeType::Ascii(s)) = g.first()
        {
            here = s.clone();
        }

        if r.name() == "SPRAWL_HEX_MAP_DATA" {
            out.push((here.clone(), g.iter().filter_map(val).collect()));
        }

        for c in g.iter().filter_map(rec) {
            walk(c, &here, out);
        }
    }
}

fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let bytes = std::fs::read(&a[0]).expect("startpos");
    let esf = ESF::decode(&mut Cursor::new(&bytes), &None).expect("esf");
    let mut cells = vec![];
    walk(rec(esf.root_node()).expect("root"), "", &mut cells);
    println!("{} sprawl cells", cells.len());
    for k in 2..14 {
        let mut h: BTreeMap<i64, usize> = BTreeMap::new();
        for (_, v) in &cells {
            if let Some(&x) = v.get(k) {
                *h.entry(x).or_default() += 1;
            }
        }

        let top: Vec<String> = h.iter().take(24).map(|(x, n)| format!("{x}:{n}")).collect();
        println!("field {k}: {} values: {}", h.len(), top.join(" "));
    }

    let mut combos: BTreeMap<Vec<i64>, usize> = BTreeMap::new();
    for (_, v) in &cells {
        *combos.entry(v.iter().skip(2).copied().collect()).or_default() += 1;
    }

    let mut c: Vec<_> = combos.into_iter().collect();
    c.sort_by(|x, y| y.1.cmp(&x.1));
    for (v, n) in c.iter().take(25) {
        println!("  {v:?} x{n}");
    }

    if let Some(region) = a.get(1) {
        for (r, v) in cells.iter().filter(|(r, _)| r.contains(region.as_str())) {
            println!("{r} {v:?}");
        }
    }
}
