//! Differential dump of the tile database entries, climates and index of an Assembly Kit's raw_data folder, for comparing with the
//! C++ code fed the same folder.
//!
//!   tile_entry_dump <raw_data> <cache file>

use std::path::{Path, PathBuf};

use wce_formats::tile_climate::{ClimateList, climate_texture_path};
use wce_formats::tile_entry::TileEntry;
use wce_formats::tile_index::TileIndex;

fn d(v: f64) -> String {
    format!("{:016x}", v.to_bits())
}

fn rel(p: &Path, root: &Path) -> String {
    p.strip_prefix(root).unwrap_or(p).to_string_lossy().into_owned()
}

fn index_of(index: Option<usize>) -> i64 {
    index.map_or(-1, |index| index as i64)
}

fn main() {
    let args = std::env::args().collect::<Vec<_>>();
    let raw = PathBuf::from(&args[1]);
    let cache = PathBuf::from(&args[2]);
    let mut files = vec![];
    for kind in ["battle", "campaign"] {
        let dir = raw.join("terrain/tiles").join(kind).join("_tile_database/TILES");
        files.extend(std::fs::read_dir(dir).unwrap().flatten().map(|entry| entry.path()));
    }

    files.sort();
    for p in &files {
        println!("== {}", rel(p, &raw));
        let t = match TileEntry::load(p) {
            Ok(t) => t,
            Err(error) => {
                println!("error {error}");
                continue;
            }
        };

        println!("roundtrip {} root {} sv {} loc {} gen {} kind {} set {} mask {} w {} h {} rgb {} {} {}", i32::from(t.save() == std::fs::read(p).unwrap()),
            t.root(), t.serialise_version(), t.location(), t.location_generic(), t.kind(), t.tile_set(), t.mask(), t.width(), t.height(), d(t.red()), d(t.green()), d(t.blue()));
        println!("flags {} {} {} {} alpha {} blend {}", i32::from(t.requires_infield_lodding()), i32::from(t.random_rotatable()), i32::from(t.scalable()),
            i32::from(t.encampable()), t.custom_alpha_blend_texture(), t.custom_blend_tile());
        let g = t.texture_group();
        println!("group {} {} {} {}", g.red, g.green, g.blue, g.alpha);
        for (i, v) in t.variations().iter().enumerate() {
            println!("var {} {} {} {} {} {} {} {} {} {} {} {} {} {} {} | {} | {} | find {}", v.elem, v.serialise_version, v.location, d(v.min_height), d(v.scale),
                d(v.normal_strength), d(v.overlap_border_size), v.raw_data_tri_density, v.blend_common, v.index_common, v.normal_common, d(v.red), d(v.green),
                d(v.blue), i32::from(v.barbarian), rel(&t.variation_folder(&raw, i), &raw), rel(&t.ted_path(&raw, i), &raw),
                index_of(t.find_variation(&format!("{}/", v.location.to_ascii_uppercase()))));
        }

        for l in t.link_targets() {
            println!("target {} {} {} {} {}", l.elem, l.serialise_version, l.tile_set, l.x, l.y);
        }

        for l in t.links() {
            println!("link {} {} {} {} {} {} {} {} {} {} {} {}", l.elem, l.serialise_version, l.link_set, l.x, l.y, l.base_x, l.base_y, i32::from(l.is_entry),
                l.blend_size, i32::from(l.no_offline_blend), l.test, i32::from(l.has_blend_quad));
        }

        for u in t.unknown_names() {
            println!("unknown {u}");
        }

        let raw_root = match t.raw_root() {
            Some(root) if root == raw => "same".to_owned(),
            Some(root) => root.to_string_lossy().into_owned(),
            None => String::new(),
        };

        println!("raw_root {raw_root} tile_folder {} missing {}", rel(&t.tile_folder(&raw), &raw), index_of(t.find_variation("nope")));
    }

    for kind in ["battle", "campaign"] {
        let cl = ClimateList::load(&raw.join("terrain/tiles").join(kind).join("_tile_database/_settings.xml")).unwrap();
        println!("== climates {kind} {}", cl.climates.len());
        for c in &cl.climates {
            println!("climate {} {} {} {} find {}", c.name, c.texture_set, c.destruction_climate, c.folder(),
                i32::from(cl.find(&c.name.to_ascii_uppercase()).is_some_and(|found| std::ptr::eq(found, c))));
            for x in &c.textures {
                print!("  tex {} {} {} {}", x.name, d(x.blend_pixel_scale), d(x.outfield_blend_pixel_scale), d(x.mid_distance_strength));
                for v in 0..4 {
                    print!(" {} {}", d(x.lerp_smoothness[v]), d(x.lerp_reflectivity[v]));
                }

                println!(" find {}", i32::from(c.find(&x.name).is_some_and(|found| std::ptr::eq(found, x))));
                for v in 0..4 {
                    for s in ["su", "au", "wi"] {
                        for n in [false, true] {
                            println!("    path {}", climate_texture_path(&c.folder(), &x.name, v, s, n, |p| p.len() % 3 == 0).unwrap_or_default());
                        }
                    }
                }
            }
        }
    }

    let _ = std::fs::remove_file(&cache);
    for run in ["none", "cold", "warm"] {
        let idx = TileIndex::build(&raw, (run != "none").then_some(cache.as_path()));
        println!("== index {run} parsed {} cache {} removed {} rows {}", idx.parsed, idx.from_cache, idx.removed, idx.entries().len());
        for p in &idx.problems {
            println!("problem {p}");
        }

        for p in &idx.duplicate_locations {
            println!("duplicate {p}");
        }

        for e in idx.entries() {
            let vars = e.variations.iter().map(|v| format!("{v}|")).collect::<String>();
            println!("row {} {} {} {} {} {} {} {} {} {} {} {} {} {} {}", e.name, rel(&e.xml, &raw), e.kind, e.location, e.key, e.tile_set, e.width, e.height,
                d(f64::from(e.red)), d(f64::from(e.green)), d(f64::from(e.blue)), vars, e.links, e.targets, e.size);
            if run != "warm" {
                continue;
            }

            let show = |q: PathBuf| {
                let r = idx.resolve(&q);
                println!("  resolve {} -> {} {}", rel(&q, &raw), r.entry.map_or("-", |entry| entry.name.as_str()), index_of(r.variation));
            };

            let loc = raw.join(e.location.replace('\\', "/"));
            show(loc.clone());
            for v in &e.variations {
                show(loc.join(v));
                show(loc.join(v).join("tile.ted"));
                show(loc.join(format!("{}_x", v.to_ascii_uppercase())));
            }

            let last = e.key.rfind('/').unwrap_or_default();
            let cut = e.key[..last].rfind('/').unwrap_or_default();
            let by = |q: &str| println!("  find_name {q} -> {}", idx.find_name(q).map_or("-", |entry| entry.name.as_str()));
            by(&e.name.to_ascii_uppercase());
            by(&e.key[cut + 1..]);
            by(&format!("{}\\", e.location.to_ascii_uppercase()));
            println!("  find_location -> {}", idx.find_location(&e.location.to_ascii_uppercase()).map_or("-", |entry| entry.name.as_str()));
        }

        println!("resolve outside {}", i32::from(idx.resolve(&raw.join("terrain/other")).entry.is_some()));
    }
}
