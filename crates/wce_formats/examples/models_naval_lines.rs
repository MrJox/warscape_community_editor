//! Lists a ship logic row's EF lines by purpose: how many, their total length, the men they hold at a spacing, and their heights.
//!
//!   cargo run --release -p wce_formats --example models_naval_lines -- <models_naval table file> <row key> [spacing m]

use std::collections::BTreeMap;

use wce_formats::models_naval::{EF_PURPOSES, decode};

fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let (Some(file), Some(key)) = (a.first(), a.get(1)) else {
        eprintln!("models_naval_lines <table file> <row key> [spacing m]");
        std::process::exit(2);
    };

    let spacing: f32 = a.get(2).and_then(|s| s.parse().ok()).unwrap_or(1.1);
    let table = decode(&std::fs::read(file).expect("read")).expect("decode");
    let Some(row) = table.rows.iter().find(|r| r.key == *key) else {
        eprintln!("no row '{key}'");
        std::process::exit(1);
    };

    let mut by: BTreeMap<u32, (usize, f32, u32, f32, f32)> = BTreeMap::new();
    for l in &row.ef_lines {
        let [p, q] = l.ends;
        let len = ((q[0] - p[0]).powi(2) + (q[1] - p[1]).powi(2) + (q[2] - p[2]).powi(2)).sqrt();
        let e = by.entry(l.purpose).or_insert((0, 0.0, 0, f32::MAX, f32::MIN));
        e.0 += 1;
        e.1 += len;
        e.2 += (len / spacing).floor() as u32 + 1;
        e.3 = e.3.min(p[1].min(q[1]));
        e.4 = e.4.max(p[1].max(q[1]));
    }

    for (purpose, (n, len, men, lo, hi)) in by {
        let name = EF_PURPOSES.get(purpose as usize).copied().unwrap_or("?");
        println!("{purpose:2} {name:32} {n:3} lines, {len:6.1} m, {men:4} men at {spacing} m, y {lo:.2} .. {hi:.2}");
    }
}
