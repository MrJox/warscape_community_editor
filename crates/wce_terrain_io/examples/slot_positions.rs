//! The candidate positions of a campaign map's settlement slots (`map_data.esf`: `SETTLEMENT_INFO/SLOT_ARRAY_BLOCK` groups'
//! `SLOT_POSITIONS_BLOCK`, `PORT_POSITIONS_BLOCK`): how many a slot has, and where the others lie from the first (the neighbour
//! direction in the game's order, RPFM's `HexCell::neighbour`, or the cells' distance).
//!
//!   cargo run --release -p wce_terrain_io --example slot_positions -- <map_data.esf> [region filter]

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

fn child<'a>(r: &'a RecordNode, name: &str) -> Option<&'a RecordNode> {
    r.children().iter().flatten().filter_map(rec).find(|c| c.name() == name)
}

/// Every group's (u16, u16) cell of a record.
fn cells(r: &RecordNode) -> Vec<(i32, i32)> {
    r.children().iter().filter_map(|g| {
        let mut v = g.iter().filter_map(|n| match n {
            NodeType::U16(x) => Some(i32::from(*x)),
            _ => None,
        });
        Some((v.next()?, v.next()?))
    }).collect()
}

/// RPFM's neighbour order (`coords.rs`).
fn direction(a: (i32, i32), b: (i32, i32)) -> Option<usize> {
    const ODD: [(i32, i32); 6] = [(0, 1), (1, 1), (1, 0), (0, -1), (-1, 0), (-1, 1)];
    const EVEN: [(i32, i32); 6] = [(0, 1), (1, 0), (1, -1), (0, -1), (-1, -1), (-1, 0)];
    let offsets = if a.0 % 2 == 1 { ODD } else { EVEN };
    offsets.iter().position(|&(dx, dy)| (a.0 + dx, a.1 + dy) == b)
}

fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let bytes = std::fs::read(&a[0]).expect("map_data");
    let filter = a.get(1).cloned().unwrap_or_default();
    let esf = ESF::decode(&mut Cursor::new(&bytes), &None).expect("esf");
    let root = rec(esf.root_node()).expect("root");
    let block = child(root, "REGIONS_DATA").and_then(|r| child(r, "REGIONS_BLOCK")).expect("regions");
    let mut counts: BTreeMap<usize, usize> = BTreeMap::new();
    let mut dirs: BTreeMap<String, usize> = BTreeMap::new();
    for region in block.children().iter().flatten().filter_map(rec).filter(|r| r.name() == "REGION_DATA") {
        let key = region.children().first().and_then(|g| g.iter().find_map(|n| match n {
            NodeType::Ascii(s) => Some(s.clone()),
            _ => None,
        })).unwrap_or_default();
        let Some(info) = child(region, "SETTLEMENT_INFO") else { continue };
        let mut slots: Vec<(String, Vec<(i32, i32)>)> = vec![];
        if let Some(b) = child(info, "SLOT_ARRAY_BLOCK") {
            for g in b.children() {
                for r in g.iter().filter_map(rec).filter(|r| r.name() == "SLOT_POSITIONS_BLOCK") {
                    slots.push(("slot".to_owned(), cells(r)));
                }
            }
        }

        if let Some(p) = child(info, "PORT_POSITIONS_BLOCK") {
            slots.push(("port".to_owned(), cells(p)));
        }

        for (kind, c) in &slots {
            *counts.entry(c.len()).or_default() += 1;
            for other in c.iter().skip(1) {
                let d = direction(c[0], *other).map_or_else(|| format!("{kind} not a neighbour"), |d| format!("{kind} direction {d}"));
                *dirs.entry(d).or_default() += 1;
            }

            if !filter.is_empty() && key.contains(&filter) {
                println!("{key} {kind}: {c:?}");
            }
        }
    }

    println!("positions a slot: {counts:?}");
    println!("the others from the first: {dirs:?}");
}
