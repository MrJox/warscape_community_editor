//! Exploration: where a Rome 2 location's deployment points sit on the map. For the land bridges' and coastal battles' zones (their
//! boundaries follow rivers and coasts), the boundary points off the battle's square edge (|v| < 950) mapped to cells as
//! `centre + offset + (x, s * y) / 256` for offsets (in half cells) and s = +1 / -1 (y as north or as south); per candidate the share of
//! points on or next to a river / sea / coast tile cell. The best candidates printed.
//!
//!   WCE_GAME_KEY=rome_2 cargo run --release -p wce_terrain_io --example deployment_fit -- <game dir> <map folder, e.g. terrain/battles/main_gaul>

use wce_tile::battle_map::locations::BattleLocations;
use wce_tile::battle_map::tile_map::TileMap;
use wce_vfs::{Config, GameProfile, Vfs};

fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let mut cfg = Config::new(&a[0], GameProfile::from_env().clone());
    cfg.mod_packs = Some(vec![]);
    let vfs = Vfs::mount(&cfg).expect("vfs");
    let folder = &a[1];
    let read = |n: &str| vfs.read(&format!("{folder}/{n}")).expect(n);
    let map = TileMap::parse(&read("tile_map.index"), &read("tile_map.tiles")).expect("tile map");
    let blm = BattleLocations::parse(&read("locations.blm")).expect("blm");
    let (w, h) = (map.width as i32, map.height as i32);
    // Cells of a river tile.
    let wet: Vec<bool> = map.layers[0]
        .iter()
        .map(|c| {
            let o = if c.index > 0 {
                c.index
            } else if c.index < 0 {
                map.layers[0][(-1 - c.index) as usize].index
            } else {
                0
            };
            o > 0 && map.names.get(o as usize - 1).is_some_and(|n| n.to_lowercase().contains("river"))
        })
        .collect();
    let near_wet = |x: i32, y: i32| x >= 0 && y >= 0 && x < w && y < h && wet[(y * w + x) as usize];

    // (centre cell, rect, points)
    let mut sets: Vec<([i32; 2], [i32; 4], Vec<(f64, f64)>)> = vec![];
    for (t, locs) in &blm.types {
        if t != "land_bridge" {
            continue;
        }

        for l in locs.iter().filter(|l| l.id != -1).take(150) {
            let n = (l.id as u32).wrapping_sub(0xffff_0000);
            let Ok(x) = vfs.read(&format!("{folder}/deployment/deployment_{n:04}.xml")) else { continue };
            let text = String::from_utf8_lossy(&x).into_owned();
            let mut pts = vec![];
            for part in text.split("<position ").skip(1) {
                let v =
                    |k: &str| part.split(&format!("{k}='")).nth(1).and_then(|r| r.split('\'').next()).and_then(|s| s.parse::<f64>().ok());
                if let (Some(px), Some(py)) = (v("x"), v("y"))
                    && px.abs() < 950.0
                    && py.abs() < 950.0
                {
                    pts.push((px, py));
                }
            }

            sets.push((l.centre, l.rect, pts));
        }
    }

    let total: usize = sets.iter().map(|s| s.2.len()).sum();
    println!("{} locations, {total} inner points; wet cells {}", sets.len(), wet.iter().filter(|&&b| b).count());
    let mut results = vec![];
    for s in [1.0, -1.0] {
        for anchor in ["centre", "rect centre"] {
            for oy in -16..=16 {
                for ox in -16..=16 {
                    let (dx, dy) = (f64::from(ox) / 2.0, f64::from(oy) / 2.0);
                    let mut hits = 0;
                    for (c, r, pts) in &sets {
                        let (bx, by) = if anchor == "centre" {
                            (f64::from(c[0]), f64::from(c[1]))
                        } else {
                            (f64::from(r[0] + r[2] + 1) / 2.0, f64::from(r[1] + r[3] + 1) / 2.0)
                        };
                        for &(px, py) in pts {
                            let (x, y) = (bx + dx + px / 256.0, by + dy - s * py / 256.0);
                            if near_wet(x.floor() as i32, y.floor() as i32) {
                                hits += 1;
                            }
                        }
                    }

                    results.push((hits, s, anchor, dx, dy));
                }
            }
        }
    }

    results.sort_by(|a, b| b.0.cmp(&a.0));
    for r in results.iter().take(8) {
        println!(
            "  {:.1} %  y {}  from the {}  offset {:+.1}, {:+.1} cells",
            100.0 * r.0 as f64 / total.max(1) as f64,
            if r.1 > 0.0 { "north" } else { "south" },
            r.2,
            r.3,
            r.4
        );
    }

    if let Some(r) = results.iter().find(|r| r.1 > 0.0 && r.2 == "centre" && r.3 == 0.5 && r.4 == -0.5) {
        println!("  (the scene's anchor today: the centre cell's middle, y north: {:.1} %)", 100.0 * r.0 as f64 / total.max(1) as f64);
    }
}
