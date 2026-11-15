//! `tile_combine <entry xml> <projects raw_data> [--ref <variation>] <variation>...` combines variations of a tile into a combined
//! project in the projects (`variations::combine::combine`, what Tile > Variations > Combine... does; docs/plans/tile_variations.md): each
//! read from the projects' folder when it has one, else the entry's kit. `tile_combine --separate <combined tile folder> <variation>`
//! makes one a project of its own again (`combine::separate`).

use std::path::{Path, PathBuf};

use wce_tile::variations::combine::{Input, Origin, combine, separate};

fn main() {
    let args = std::env::args().skip(1).collect::<Vec<_>>();
    if args.first().is_some_and(|a| a == "--separate") {
        let (Some(dir), Some(v)) = (args.get(1), args.get(2)) else {
            eprintln!("usage: tile_combine --separate <combined tile folder> <variation>");
            std::process::exit(2);
        };

        match separate(Path::new(dir), v) {
            Ok(m) => println!("{m}"),
            Err(e) => {
                println!("not separated: {e}");
                std::process::exit(1);
            }
        }

        return;
    }

    let (Some(entry), Some(projects)) = (args.first(), args.get(1)) else {
        eprintln!("usage: tile_combine <entry xml> <projects raw_data> [--ref <variation>] <variation>...");
        std::process::exit(2);
    };

    let rest = &args[2..];
    let reference = rest.iter().position(|a| a == "--ref").and_then(|i| rest.get(i + 1)).cloned();
    let names = rest.iter().enumerate().filter(|(i, a)| *a != "--ref" && (*i == 0 || rest[i - 1] != "--ref")).map(|(_, a)| a.clone());
    let entry = wce_formats::tile_entry::TileEntry::load(Path::new(entry)).unwrap_or_else(|e| panic!("{entry}: {e}"));
    let kit = entry.raw_root().expect("the entry is in a kit");
    let projects = PathBuf::from(projects);
    let inputs = names
        .map(|v| {
            let row = entry.find_variation(&v).unwrap_or_else(|| panic!("the entry has no {v}"));
            let own = entry.variation_folder(&projects, row);
            if wce_tile::tile_project::has_tile(&own) {
                Input { variation: v, dir: own, origin: Origin::Project }
            } else {
                Input { variation: v, dir: entry.variation_folder(&kit, row), origin: Origin::Kit }
            }
        })
        .collect::<Vec<_>>();
    let reference = reference.and_then(|r| inputs.iter().position(|i| i.variation == r)).unwrap_or(0);
    let t = std::time::Instant::now();
    match combine(&inputs, reference, &entry, &projects) {
        Ok(r) => {
            println!("{} ({:.1} s)", r.message, t.elapsed().as_secs_f64());
            // The terrain layers it made: each type's, with their variations and how many 64 x 64 blocks they hold.
            let bytes = std::fs::read(r.tile_dir.join(wce_tile::terrain_layers::FILE)).expect("its layers");
            let layers = wce_tile::terrain_layers::TerrainLayers::from_bytes(&bytes).expect("they read");
            let list = |name: &str, l: Vec<(String, Option<[u32; 4]>)>| {
                println!("  {name}: {}", l.iter().map(|(n, e)| format!("{n} {e:?}")).collect::<Vec<_>>().join("; "));
            };
            if let Some(s) = &layers.height {
                list("height", s.layers.iter().map(|l| (l.name.clone(), l.extent())).collect());
            }

            if let Some(s) = &layers.blend {
                list("blend", s.layers.iter().map(|l| (l.name.clone(), l.extent())).collect());
            }

            if let Some(s) = &layers.ground {
                list("ground type", s.layers.iter().map(|l| (l.name.clone(), l.extent())).collect());
            }
        }
        Err(e) => {
            println!("not combined: {e}");
            std::process::exit(1);
        }
    }
}
