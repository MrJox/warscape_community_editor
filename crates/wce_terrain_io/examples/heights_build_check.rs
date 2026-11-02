//! BOB_Terrain's `Heights & Normals` outputs made from a kit map's LF fields (`wce_terrain_io::attila::heights`) against BOB's own
//! (`working_data/terrain/battles/<map>` of a BOB run): `lf_height_map.dds`, `lf_sea_height_map.dds`, `lf_height_map.data`, byte for byte.
//!
//!   cargo run --release -p wce_terrain_io --example heights_build_check -- <raw map folder> <BOB's output folder>
//!
//! Exit code 1 when a file differs.

use std::path::PathBuf;

use wce_terrain::{LfRole, MapKind};
use wce_terrain_io::attila::heights;
use wce_terrain_io::{MapFiles, attila};

fn main() {
    let a: Vec<PathBuf> = std::env::args().skip(1).map(PathBuf::from).collect();
    let (Some(raw), Some(bob)) = (a.first(), a.get(1)) else {
        eprintln!("heights_build_check <raw map folder> <BOB's output folder>");
        std::process::exit(2);
    };

    let files = MapFiles::from_dir(raw).expect("the map folder");
    let m = attila::read("map", MapKind::Battle, &files).expect("the map");
    let cells = m.cells().expect("a tile map");
    let mut bad = 0;
    let mut check = |name: &str, ours: Vec<u8>| {
        let want = std::fs::read(bob.join(name)).unwrap_or_default();
        let first = ours.iter().zip(&want).position(|(x, y)| x != y);
        if ours == want {
            println!("{name}: byte for byte ({} bytes)", ours.len());
        } else {
            bad += 1;
            let n = ours.iter().zip(&want).filter(|(x, y)| x != y).count();
            println!("{name}: DIFFERS: sizes {} / {}, {n} bytes differ, the first at {first:?}", ours.len(), want.len());
        }
    };

    let land = m.lf(LfRole::Land).map(|i| i.get().clone());
    let sea = m.lf(LfRole::Sea).map(|i| i.get().clone());
    if let Some(f) = &land {
        check("lf_height_map.dds", heights::lf_height_dds(f));
        check("lf_height_map.data", heights::lf_height_data(f, cells));
    }

    if let Some(f) = &sea {
        check("lf_sea_height_map.dds", heights::lf_sea_height_dds(f));
    }

    if bad > 0 {
        std::process::exit(1);
    }
}
