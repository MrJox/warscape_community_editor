//! Exploration: a tile map's cells (Rome 2's version 6): per layer the origin, covered and empty cells, and how a covered cell's fields
//! compare with its origin's (flags, climate, flipped, the unknown bytes).
//!
//!   cargo run --release -p wce_tile --example tile_map_cells -- <tile_map.index> <tile_map.tiles>

use std::collections::BTreeMap;

use wce_tile::battle_map::tile_map::TileMap;

fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let m = TileMap::parse(&std::fs::read(&a[0]).expect("index"), &std::fs::read(&a[1]).expect("tiles")).expect("tile map");
    println!("version {} {} x {}, {} names, chunks {} bytes", m.version, m.width, m.height, m.names.len(), m.chunks.len());
    // The chunk table (u32 count, then x0 y0 x1 y1 i32 each) and the origin cells' bytes 10-11 against the chunks holding the tile.
    let n = u32::from_le_bytes(m.chunks[0..4].try_into().unwrap()) as usize;
    let chunks: Vec<[i32; 4]> = (0..n)
        .map(|k| {
            let o = 4 + 16 * k;
            [0, 4, 8, 12].map(|d| i32::from_le_bytes(m.chunks[o + d..o + d + 4].try_into().unwrap()))
        })
        .collect();
    println!("{n} chunks, first {:?} {:?}", chunks.first(), chunks.get(1));
    let layer = &m.layers[0];
    let mut ext: BTreeMap<usize, [u32; 4]> = BTreeMap::new();
    for (i, c) in layer.iter().enumerate() {
        let o = if c.index > 0 {
            i
        } else if c.index < 0 {
            (-1 - c.index) as usize
        } else {
            continue;
        };
        let (x, y) = (i as u32 % m.width, i as u32 / m.width);
        let e = ext.entry(o).or_insert([x, y, x, y]);
        *e = [e[0].min(x), e[1].min(y), e[2].max(x), e[3].max(y)];
    }

    let (mut first, mut any, mut none, mut shown) = (0, 0, 0, 0);
    let mut covered_old: BTreeMap<String, usize> = BTreeMap::new();
    for (&o, r) in &ext {
        let v = i16::from_le_bytes([layer[o].old[0], layer[o].old[1]]);
        let holding: Vec<usize> = (0..n)
            .filter(|&k| {
                let c = chunks[k];
                c[0] <= r[0] as i32 && c[1] <= r[1] as i32 && r[2] as i32 <= c[2] && r[3] as i32 <= c[3]
            })
            .collect();
        if holding.first() == Some(&(v as usize)) {
            first += 1
        } else if holding.contains(&(v as usize)) {
            any += 1
        } else {
            none += 1;
            if shown < 4 {
                shown += 1;
                println!("  origin {o} rect {r:?} old {v} holding {holding:?} chunk {:?}", chunks.get(v as usize));
            }
        }
    }

    for c in layer.iter().filter(|c| c.index < 0) {
        *covered_old.entry(format!("{:?}", c.old)).or_default() += 1;
    }

    let mut co: Vec<_> = covered_old.into_iter().collect();
    co.sort_by(|a, b| b.1.cmp(&a.1));
    println!(
        "origins: old the first chunk holding the tile {first}, another holding it {any}, none {none}; covered cells' old: {:?}",
        co.iter().take(5).collect::<Vec<_>>()
    );
    for (k, layer) in m.layers.iter().enumerate() {
        let (mut origins, mut covered, mut empty) = (0, 0, 0);
        let mut same: BTreeMap<&str, (usize, usize)> = BTreeMap::new();
        let mut empties: BTreeMap<String, usize> = BTreeMap::new();
        let mut origin_old: BTreeMap<String, usize> = BTreeMap::new();
        for (i, c) in layer.iter().enumerate() {
            if c.index > 0 {
                origins += 1;
                *origin_old.entry(format!("{:?} flags {:#x} flipped {}", c.old, c.flags & 0xff0f, c.flipped)).or_default() += 1;
            } else if c.index < 0 {
                covered += 1;
                let o = &layer[(-1 - c.index) as usize];
                let x = (i as u32 % m.width, i as u32 / m.width);
                for (name, eq) in [
                    ("flags", c.flags == o.flags),
                    ("climate", c.climate == o.climate),
                    ("flipped", c.flipped == o.flipped),
                    ("old", c.old == o.old),
                    ("origin field", c.origin == o.origin),
                    ("origin field is the origin cell", u32::from(c.origin[0]) + u32::from(c.origin[1]) * m.width == (-1 - c.index) as u32),
                    ("flags 0x10", c.flags & 0xf0 == 0x10),
                    ("own cell", x.0 > 0 || x.1 > 0),
                ] {
                    let e = same.entry(name).or_default();
                    if eq { e.0 += 1 } else { e.1 += 1 }
                }
            } else {
                empty += 1;
                *empties
                    .entry(format!(
                        "flags {:#x} origin {:?} climate {} flipped {} old {:?}",
                        c.flags, c.origin, c.climate, c.flipped, c.old
                    ))
                    .or_default() += 1;
            }
        }

        println!("layer {k}: {origins} origins, {covered} covered, {empty} empty");
        println!("  covered vs origin (same, not): {same:?}");
        let mut e: Vec<_> = empties.into_iter().collect();
        e.sort_by(|a, b| b.1.cmp(&a.1));
        println!("  empty cells: {:?}", e.iter().take(4).collect::<Vec<_>>());
        let mut o: Vec<_> = origin_old.into_iter().collect();
        o.sort_by(|a, b| b.1.cmp(&a.1));
        println!("  origin cells' other fields: {:?}", o.iter().take(5).collect::<Vec<_>>());
    }
}
