//! lake_planes: how the kit's lakes are built (docs/plans/rivers.md, L1). For every tile with `water_plane` instances under the given
//! folders: each plane's place (the editor's metres), height (the game's units), scale and turn, and the ground under it: the share of a
//! rectangle of the plane's scale (in the editor's metres, as a model 1 unit wide and as one 2 units wide) that is under its water, the
//! ground's lowest and highest there, and the ground along the shore (where the ground crosses the water's height) against the water.
//!
//!   cargo run --release -p wce_tile --example lake_planes -- <kit raw_data/terrain/tiles/battle/lakes> [...]

use std::path::{Path, PathBuf};

use wce_tile::tile_project::TileProject;

fn walk(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(read) = std::fs::read_dir(dir) else { return };
    for e in read.flatten() {
        let p = e.path();
        if p.is_dir() {
            walk(&p, out);
        } else if p.file_name().is_some_and(|n| n == "tile.ted") {
            out.push(dir.to_owned());
        }
    }
}

fn vec3(tag: &str, name: &str) -> Option<[f64; 3]> {
    let key = format!(" {name}=\"");
    let i = tag.find(&key)? + key.len();
    let inner = tag[i..].split('"').next()?;
    let inner = inner.trim_start_matches("vector(").trim_start_matches("euler(").trim_end_matches(')');
    let v = inner.split(',').filter_map(|x| x.trim().parse::<f64>().ok()).collect::<Vec<_>>();
    (v.len() == 3).then(|| [v[0], v[1], v[2]])
}

fn main() {
    let mut dirs = vec![];
    for a in std::env::args().skip(1) {
        walk(Path::new(&a), &mut dirs);
    }
    dirs.sort();

    for dir in dirs {
        let Ok(text) = std::fs::read_to_string(dir.join("tile.ted")) else { continue };
        let planes = text
            .split('<')
            .filter(|t| t.starts_with("SIMPLE_MESH_INSTANCE") && t.to_lowercase().contains("water_plane"))
            .map(str::to_owned)
            .collect::<Vec<_>>();
        if planes.is_empty() {
            continue;
        }

        let Ok(p) = TileProject::open(&dir) else { continue };
        let (ex, ez) = (p.extent_x(), p.extent_z());
        println!("{} ({ex} x {ez} m): {} plane(s)", dir.display(), planes.len());
        for tag in planes {
            let (Some(pos), Some(scale)) = (vec3(&tag, "position"), vec3(&tag, "scale")) else { continue };
            let turn = vec3(&tag, "orientation").unwrap_or_default();
            let (cx, cz, y) = (pos[0] / 2.0, pos[2] / 2.0, pos[1]);
            print!("  plane at ({cx:.0}, {cz:.0}) m, water {y:.2}, scale {:.0} x {:.0}, turn {:.2}", scale[0], scale[2], turn[0]);
            for half in [0.25, 0.5] {
                // Half its scale in the editor's metres: a model 1 unit wide (0.25), 2 units (0.5).
                let (hx, hz) = (scale[0] * half, scale[2] * half);
                let (mut n, mut under, mut lo, mut hi) = (0, 0, f64::INFINITY, f64::NEG_INFINITY);
                let mut z = cz - hz;
                while z <= cz + hz {
                    let mut x = cx - hx;
                    while x <= cx + hx {
                        if x >= 0.0 && z >= 0.0 && x <= ex && z <= ez {
                            let g = p.height_at(x, z);
                            n += 1;
                            under += usize::from(g < y);
                            lo = lo.min(g);
                            hi = hi.max(g);
                        }

                        x += 8.0;
                    }

                    z += 8.0;
                }

                print!("; half {half}: {:.0} % under, ground {lo:.1} .. {hi:.1}", 100.0 * under as f64 / n.max(1) as f64);
            }

            // The shore: places of the whole tile next to one under the water and one over it; how steep it is there.
            let (mut shore, mut slope) = (0, 0.0);
            let mut z = 4.0;
            while z < ez - 4.0 {
                let mut x = 4.0;
                while x < ex - 4.0 {
                    let (a, b) = (p.height_at(x, z), p.height_at(x + 4.0, z));
                    if (a - y) * (b - y) < 0.0 {
                        shore += 1;
                        slope += ((a - b) / 4.0).abs();
                    }

                    x += 4.0;
                }

                z += 4.0;
            }

            println!("; shore crossings {shore}, mean slope {:.2} (game units a metre)", slope / f64::from(shore.max(1)));
        }
    }
}
