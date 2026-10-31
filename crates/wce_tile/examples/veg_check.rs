//! Generates the procedural trees of Assembly Kit tiles and compares them with the game's packed tree lists (what BOB wrote when CA built
//! them): per tile and category, the list's generated trees (those on the 0.5 m grid), ours, and how many are the same (position, model,
//! scale byte).
//!
//!   cargo run --release -p wce_tile --example veg_check -- <game folder> [filter] [--diff] [--full] [--oracle-dir <working_data dir>]
//!
//! `--full` also compares the whole list as BOB writes it (static trees included, every value exact, each model's order); `WCE_DY_EXACT`
//! prints the height difference unrounded.

use std::collections::HashMap;
use std::io::Cursor;
use std::path::PathBuf;

use rayon::prelude::*;
use rpfm_lib::files::Decodeable;
use rpfm_lib::files::fastbin_lists::TreeListFile;
use wce_tile::tile_project::TileProject;
use wce_tile::vegetation::{self, GenerationParams};
use wce_vfs::files::Files;
use wce_vfs::{Config, GameProfile, Vfs};

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let game = PathBuf::from(&args[0]);
    let filter = args.get(1).filter(|a| !a.starts_with("--")).cloned().unwrap_or_default();
    let diff = args.iter().any(|a| a == "--diff");
    let mask_mode: Option<String> = args.iter().position(|a| a == "--mask").and_then(|i| args.get(i + 1)).cloned();
    let excl: Option<String> = args.iter().position(|a| a == "--excl").and_then(|i| args.get(i + 1)).cloned();
    let only: Option<String> = args.iter().position(|a| a == "--category").and_then(|i| args.get(i + 1)).cloned();
    let oracle_dir: Option<PathBuf> = args.iter().position(|a| a == "--oracle-dir").and_then(|i| args.get(i + 1)).map(PathBuf::from);
    let raw = game.join("assembly_kit").join("raw_data");
    let vfs = Vfs::mount(&Config::new(game.clone(), GameProfile::from_env().clone())).expect("vfs");
    let params = GenerationParams::load(&game.join("assembly_kit/working_data/BattleTerrain/vegetation")).expect("params");
    let lists = vfs.list("terrain/tiles/battle/", ".tree_list.bin");
    let mut tiles: HashMap<String, Vec<String>> = HashMap::new();
    for l in lists {
        let (folder, file) = l.rsplit_once('/').expect("a file");
        tiles.entry(folder.to_owned()).or_default().push(file.trim_end_matches(".tree_list.bin").to_owned());
    }

    let mut folders: Vec<(String, Vec<String>)> = tiles.into_iter().filter(|(f, _)| f.contains(&filter) && raw.join(f).join("tile.ted").is_file()).collect();
    folders.sort();
    let results: Vec<String> = folders
        .par_iter()
        .map(|(folder, names)| {
            let project = match TileProject::open(&raw.join(folder)) {
                Ok(p) => p,
                Err(e) => return format!("{folder}\topen failed: {e}"),
            };

            let mut inputs = match vegetation::tile_inputs(&project) {
                Ok(i) => i,
                Err(e) => return format!("{folder}\tinputs: {e}"),
            };

            if let (Some(m), Some(entry)) = (mask_mode.as_deref(), project.entry.as_ref()) {
                let (w, h) = (entry.width(), entry.height());
                let bits = entry.mask();
                let b = bits.as_bytes();
                let mut cells = std::collections::HashSet::new();
                for x in 0..w {
                    for y in 0..h {
                        let raw = b.get((y * w + x) as usize).copied();
                        let ok = b.is_empty() || match m { "zero" => raw == Some(b'0'), "one_noflip" | "one" => raw == Some(b'1'), "zero_noflip" => raw == Some(b'0'), _ => true };
                        if ok {
                            let yy = if m.ends_with("noflip") { y } else { h - 1 - y };
                            cells.insert((x as i32, yy as i32));
                        }
                    }
                }

                inputs.cells = cells;
            }

            match excl.as_deref() {
                Some("none") => inputs.exclusions.clear(),
                Some("visible") => {
                    let scene = &project.scene;
                    let doc = scene.doc();
                    let vis: Vec<usize> = doc.elements().iter().enumerate().filter(|(_, e)| e.name == "VegetationExclusionPolygonInstance").map(|(i, _)| i).collect();
                    let keep: Vec<bool> = vis.iter().map(|&i| scene.visible(i)).collect();
                    let polys = std::mem::take(&mut inputs.exclusions);
                    inputs.exclusions = polys.into_iter().zip(keep).filter(|(_, k)| *k).map(|(p, _)| p).collect();
                }
                _ => {}
            }

            let Some(seeds) = vegetation::seeds(&project.scene) else { return format!("{folder}\tno seeds") };
            let mut lines = vec![];
            let mut names = names.clone();
            names.sort();
            for name in names {
                if only.as_ref().is_some_and(|o| *o != name) {
                    continue;
                }

                let Some(category) = params.category(&name) else { continue };
                // --oracle-dir: BOB's fresh output (a kit's working_data) instead of the packs; file names keep the category's case.
                let bytes = match oracle_dir.as_ref() {
                    Some(dir) => std::fs::read_dir(dir.join(&folder)).ok().and_then(|rd| {
                        rd.flatten().find(|e| e.file_name().to_string_lossy().to_lowercase() == format!("{name}.tree_list.bin")).and_then(|e| std::fs::read(e.path()).ok())
                    }),
                    None => vfs.read(&format!("{folder}/{name}.tree_list.bin")).ok(),
                };
                let Some(bytes) = bytes else { continue };
                let Ok(list) = TreeListFile::decode(&mut Cursor::new(bytes), &None) else { continue };
                let mut theirs: HashMap<(i64, i64), (String, u8, f32)> = HashMap::new();
                for v in list.tree_list().tree_list() {
                    let model = v.key().replace('\\', "/").to_lowercase();
                    for t in v.value() {
                        // Generated trees sit on the generator's grid: x * scale / cell - 0.5 is a whole number.
                        let on_grid = |v: f32| {
                            let g = v * inputs.render_scale / inputs.cell() - 0.5;
                            (g - g.round()).abs() < 1e-4
                        };
                        if on_grid(*t.x()) && on_grid(*t.z()) {
                            theirs.insert(((t.x() * 2.0) as i64, (t.z() * 2.0) as i64), (model.clone(), (t.scale() * 127.0).round() as u8, *t.y()));
                        }
                    }
                }

                if args.iter().any(|a| a == "--probe") {
                    let h_m = inputs.cells_y as f32 * 128.0 * inputs.render_scale;
                    let cell = inputs.cell();
                    let oracle: HashMap<(u32, u32), (String, u8)> = theirs
                        .iter()
                        .map(|(k, (m, b, _))| {
                            let x = k.0 as f32 / 2.0;
                            let z = k.1 as f32 / 2.0;
                            let px = (x * inputs.render_scale / cell - 0.5).round() as u32;
                            let py = ((h_m - z * inputs.render_scale) / cell - 0.5).round() as u32;
                            let model = m.trim_start_matches("battleterrain/vegetation/").trim_end_matches(".rigid_model_v2").to_owned();
                            ((px, py), (model, *b))
                        })
                        .collect();
                    vegetation::generate::probe::ORACLE.with_borrow_mut(|o| *o = Some(oracle));
                }

                let exists = |p: &str| vfs.exists(&p.replace('\\', "/").to_lowercase());
                if let Some(i) = args.iter().position(|a| a == "--at") {
                    let v: Vec<f32> = args[i + 1].split(',').map(|x| x.parse().unwrap()).collect();
                    let density = params.densities[category].trees;
                    let chosen = vegetation::generate::generate(&params, &inputs, category, vegetation::params::TREE | vegetation::params::PROP, seeds.trees, density);
                    let h_m = inputs.cells_y as f32 * 128.0 * inputs.render_scale;
                    for c in &chosen {
                        let (lx, lz) = (c.x / inputs.render_scale, (h_m - c.y) / inputs.render_scale);
                        if (lx - v[0]).abs() <= v[2] && (lz - v[1]).abs() <= v[2] {
                            lines.push(format!("  chosen ({lx}, {lz}) {} group {} minsep {} width {} size {}", c.model, c.group, c.min_separation, c.width, c.size));
                        }
                    }
                }
                let mine = vegetation::generated_trees(&inputs, &params, category, seeds, &exists);
                if args.iter().any(|a| a == "--inside") {
                    let h_m = inputs.cells_y as f32 * 128.0 * inputs.render_scale;
                    let mut inside = 0;
                    let mut ex = vec![];
                    for (k, _) in &theirs {
                        let (x, z) = (k.0 as f32 / 2.0, k.1 as f32 / 2.0);
                        let gx = x * inputs.render_scale;
                        let gy = h_m - z * inputs.render_scale;
                        if inputs.exclusions.iter().any(|poly| vegetation::generate::point_in(poly, gx, gy)) {
                            inside += 1;
                            if ex.len() < 5 {
                                ex.push((x, z));
                            }
                        }
                    }

                    lines.push(format!("  theirs inside my exclusions: {inside} {ex:?}"));
                    let scene = &project.scene;
                    let doc = scene.doc();
                    let polys: Vec<usize> = doc.elements().iter().enumerate().filter(|(_, e)| e.name == "VegetationExclusionPolygonInstance").map(|(i, _)| i).collect();
                    for &(x, z) in &ex {
                        let (gx, gy) = (x * inputs.render_scale, h_m - z * inputs.render_scale);
                        for (k, poly) in inputs.exclusions.iter().enumerate() {
                            if vegetation::generate::point_in(poly, gx, gy) {
                                let e = polys.get(k).copied().unwrap_or(0);
                                lines.push(format!("    in polygon {k} (elem {e}) visible {} points {} attrs {:?}", scene.visible(e), poly.len(), doc.elements()[e].attrs.iter().map(|a| format!("{:?}", a)).collect::<Vec<_>>()));
                            }
                        }
                    }
                }

                if args.iter().any(|a| a == "--order") {
                    let mut sorted = mine.clone();
                    vegetation::msvc_sort::sort(&mut sorted, &|a: &vegetation::ListTree, b: &vegetation::ListTree| a.path < b.path);
                    let (mut ok, mut total) = (0, 0);
                    for v in list.tree_list().tree_list() {
                        let model = v.key().replace('\\', "/").to_lowercase();
                        let listed: Vec<(i64, i64)> = v.value().iter().map(|t| ((t.x() * 2.0) as i64, (t.z() * 2.0) as i64)).collect();
                        let ours: Vec<(i64, i64)> = sorted.iter().filter(|t| t.path.replace('\\', "/").to_lowercase() == model).map(|t| ((t.x * 2.0) as i64, (t.z * 2.0) as i64)).collect();
                        total += 1;
                        if listed == ours {
                            ok += 1;
                        } else {
                            lines.push(format!("  {model}: list {:?} ours {:?}", &listed[..listed.len().min(5)], &ours[..ours.len().min(5)]));
                        }
                    }
                    lines.push(format!("  order: {ok} of {total} models"));
                }

                // --full: the whole list as BOB writes it (static trees too), every value exact, and each path's order (the yaws).
                if args.iter().any(|a| a == "--full") {
                    let statics = vegetation::static_trees(&project.scene, &inputs);
                    let full = vegetation::tree_list(&inputs, &params, category, seeds, &statics, &exists);
                    let entry = |p: &str, x: f32, y: f32, z: f32, s: u8, f: u8| format!("{p} {:08x} {:08x} {:08x} {s} {f}", (x + 0.0).to_bits(), (y + 0.0).to_bits(), (z + 0.0).to_bits());
                    let theirs_all: Vec<String> = list
                        .tree_list()
                        .tree_list()
                        .iter()
                        .flat_map(|v| v.value().iter().map(move |t| entry(v.key(), *t.x(), *t.y(), *t.z(), (t.scale() * 127.0).round() as u8, *t.flags())))
                        .collect();
                    let mine_all: Vec<String> = full.iter().map(|t| entry(&t.path, t.x, t.y, t.z, t.scale_byte, u8::from(t.freeform))).collect();
                    let mut pool: HashMap<&String, usize> = HashMap::new();
                    for t in &theirs_all {
                        *pool.entry(t).or_default() += 1;
                    }
                    let mut same_all = 0;
                    for m in &mine_all {
                        if let Some(n) = pool.get_mut(m).filter(|n| **n > 0) {
                            *n -= 1;
                            same_all += 1;
                        }
                    }
                    let per_path = |l: &[String]| {
                        let mut m: Vec<(String, Vec<String>)> = vec![];
                        for e in l {
                            let p = e.split(' ').next().unwrap_or_default().to_owned();
                            match m.iter_mut().find(|(k, _)| *k == p) {
                                Some((_, v)) => v.push(e.clone()),
                                None => m.push((p, vec![e.clone()])),
                            }
                        }
                        m.sort();
                        m
                    };
                    let order = if per_path(&theirs_all) == per_path(&mine_all) { "same" } else { "differs" };
                    lines.push(format!("{folder}\t{name}\tfull\ttheirs {}\tmine {}\tsame {same_all}\tstatic {}\torder {order}", theirs_all.len(), mine_all.len(), statics.len()));
                    if diff {
                        let mut pool: Vec<&String> = mine_all.iter().collect();
                        let mut only_theirs = vec![];
                        for t in &theirs_all {
                            match pool.iter().position(|m| *m == t) {
                                Some(i) => {
                                    pool.swap_remove(i);
                                }
                                None => only_theirs.push(t),
                            }
                        }
                        let parse = |e: &str| {
                            let f: Vec<&str> = e.split(' ').collect();
                            let h = |i: usize| f32::from_bits(u32::from_str_radix(f[i], 16).unwrap_or(0));
                            (f[0].to_owned(), h(1), h(2), h(3))
                        };
                        for t in only_theirs.iter().take(6) {
                            let (tp, tx, ty, tz) = parse(t);
                            let near = pool.iter().map(|m| parse(m)).filter(|m| m.0 == tp).min_by(|a, b| ((a.1 - tx).abs() + (a.3 - tz).abs()).total_cmp(&((b.1 - tx).abs() + (b.3 - tz).abs())));
                            lines.push(format!("  only theirs {tp} ({tx}, {ty}, {tz}) nearest mine {:?}", near.map(|n| (n.1, n.2, n.3))));
                        }
                        lines.push(format!("  only mine {} e.g. {:?}", pool.len(), pool.iter().take(2).map(|m| parse(m)).collect::<Vec<_>>()));
                    }
                }

                let mut same = 0;
                let mut dy = 0.0f32;
                for t in &mine {
                    if let Some((m, s, y)) = theirs.get(&((t.x * 2.0) as i64, (t.z * 2.0) as i64))
                        && *m == t.path.replace('\\', "/").to_lowercase()
                        && *s == t.scale_byte
                    {
                        same += 1;
                        dy = dy.max((y - t.y).abs());
                    }
                }

                lines.push(format!("{folder}\t{name}\ttheirs {}\tmine {}\tsame {same}\tmax dy {}", theirs.len(), mine.len(), if std::env::var_os("WCE_DY_EXACT").is_some() { format!("{dy:e}") } else { format!("{dy:.3}") }));
                vegetation::generate::probe::REPORT.with_borrow_mut(|r| {
                    for l in r.drain(..) {
                        lines.push(format!("  {l}"));
                    }
                });
                vegetation::generate::probe::ORACLE.with_borrow_mut(|o| *o = None);
                if diff {
                    let mine_keys: HashMap<(i64, i64), (String, u8)> = mine.iter().map(|t| (((t.x * 2.0) as i64, (t.z * 2.0) as i64), (t.path.replace('\\', "/").to_lowercase(), t.scale_byte))).collect();
                    let mut keys: Vec<(i64, i64)> = theirs.keys().chain(mine_keys.keys()).copied().collect();
                    keys.sort();
                    keys.dedup();
                    for k in keys {
                        let a = mine_keys.get(&k);
                        let b = theirs.get(&k).map(|(m, s, _)| (m.clone(), *s));
                        if a.cloned() != b {
                            lines.push(format!("  ({}, {}) mine {:?} theirs {:?}", k.0 as f32 / 2.0, k.1 as f32 / 2.0, a, b));
                        }
                    }
                }
            }

            lines.join("\n")
        })
        .collect();

    for r in results.iter().filter(|r| !r.is_empty()) {
        println!("{r}");
    }
}
