//! The built files the Terrain tool makes from a kit map against BOB_Terrain's outputs for it (a scratch kit's working_data): the
//! deployment files, `locations.blm` and its two masks, from the map's `.terry`, its images (`snow_map.png`, `culture_map.png`,
//! `nogo.dds`), BOB's tile map, the tile database, `environment_map.xml` and `tile_upgrades.xml`. Exit 1 unless all agree byte for byte.
//!
//!   cargo run --release -p wce_terrain --example built_check -- <kit raw_data> <map> <BOB's output folder>

use std::path::{Path, PathBuf};

use wce_terrain::locations::{Image, MaskInputs};
use wce_terrain::terry::TerryProject;
use wce_tile::battle_map::tile_db::TileDatabase;
use wce_tile::battle_map::tile_map::TileMap;

/// A PNG as RGBA (none when missing or not 8-bit RGB / RGBA).
fn png(path: &Path) -> Option<(u32, u32, Vec<[u8; 4]>)> {
    let p = wce_formats::png_file::PngFile::parse(&std::fs::read(path).ok()?).ok()?;
    let bpp = p.bytes_per_pixel();
    (p.bit_depth == 8 && (bpp == 3 || bpp == 4))
        .then(|| (p.width, p.height, p.pixels.chunks_exact(bpp).map(|c| [c[0], c[1], c[2], if bpp == 4 { c[3] } else { 255 }]).collect()))
}

/// An uncompressed 32-bit DDS as RGBA.
fn dds32(path: &Path) -> Option<(u32, u32, Vec<[u8; 4]>)> {
    let b = std::fs::read(path).ok()?;
    let u = |o: usize| u32::from_le_bytes(b[o..o + 4].try_into().unwrap_or_default());
    if b.get(..4) != Some(b"DDS ") || u(88) != 32 {
        return None;
    }

    let (h, w) = (u(12), u(16));
    let px = b.get(128..128 + (w * h * 4) as usize)?;
    Some((w, h, px.chunks_exact(4).map(|c| [c[2], c[1], c[0], c[3]]).collect()))
}

fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let [raw, map, out] = &a[..] else {
        eprintln!("built_check <kit raw_data> <map> <BOB's output folder>");
        std::process::exit(2);
    };

    let (raw, out) = (PathBuf::from(raw), PathBuf::from(out));
    let folder = raw.join("terrain/battles").join(map);
    let p = TerryProject::parse(std::fs::read(folder.join(format!("{map}.terry"))).expect("the .terry")).expect("xml");
    let mut failed = false;

    // Deployment files.
    let ours = wce_terrain::deployment::build(&p);
    let theirs = std::fs::read_dir(out.join("deployment")).map(|d| d.count()).unwrap_or(0);
    let same = ours.iter().filter(|(name, bytes)| std::fs::read(out.join("deployment").join(name)).ok().as_ref() == Some(bytes)).count();
    println!("deployment: {same} of {} the same (BOB wrote {theirs})", ours.len());
    failed |= same != ours.len() || ours.len() != theirs;

    // locations.blm.
    let db = TileDatabase::load(&raw.join("terrain/tiles/battle/_tile_database")).expect("tile database");
    let env = std::fs::read(raw.join("warscape_asset_variation_db/environment_map.xml")).unwrap_or_default();
    let climates = wce_terrain::locations::climate_environments(&db, &env);
    let tm = TileMap::parse(
        &std::fs::read(out.join("tile_map.index")).expect("index"),
        &std::fs::read(out.join("tile_map.tiles")).expect("tiles"),
    )
    .expect("tile map");
    let want = std::fs::read(out.join("locations.blm")).expect("locations.blm");
    let blm = wce_terrain::locations::build(&p, tm.width, tm.height, &climates);
    if blm.write().expect("representable locations.blm") == want {
        println!("locations.blm: the same ({} bytes)", want.len());
    } else {
        failed = true;
        let bob = wce_tile::battle_map::locations::BattleLocations::parse(&want).expect("blm");
        let order = |b: &wce_tile::battle_map::locations::BattleLocations| {
            b.types.iter().map(|t| format!("{} {}", t.0, t.1.len())).collect::<Vec<_>>()
        };
        println!("locations.blm differs:\n  types ours  {:?}\n  types BOB's {:?}", order(&blm), order(&bob));
        println!("  records the same: {}", blm.types == bob.types);
        println!("  tail ours  {:?}\n  tail BOB's {:?}", &blm.tail, &bob.tail);
    }

    // The masks.
    let snow = png(&folder.join("snow_map.png"));
    let culture = png(&folder.join("culture_map.png"));
    let nogo = dds32(&folder.join("nogo.dds"));
    let upgrades = std::fs::read(raw.join("terrain/tiles/battle/tile_upgrades.xml")).unwrap_or_default();
    fn img(i: &Option<(u32, u32, Vec<[u8; 4]>)>) -> Option<Image<'_>> {
        i.as_ref().map(|(w, h, px)| Image { width: *w, height: *h, pixels: px })
    }

    let seasons: Vec<_> = ["sp", "su", "au", "wi"].iter().map(|s| png(&folder.join(format!("snow_map_{s}.png")))).collect();
    let inputs = MaskInputs {
        snow: img(&snow),
        culture: img(&culture),
        culture_colours: wce_terrain::locations::culture_colours(&upgrades),
        nogo: img(&nogo),
        seasons: [img(&seasons[0]), img(&seasons[1]), img(&seasons[2]), img(&seasons[3])],
    };
    println!(
        "mask inputs: snow {:?}, culture {:?} ({} colours), nogo {:?}",
        snow.as_ref().map(|s| (s.0, s.1)),
        culture.as_ref().map(|s| (s.0, s.1)),
        inputs.culture_colours.len(),
        nogo.as_ref().map(|s| (s.0, s.1))
    );
    let (m0, m1) = wce_terrain::locations::masks(&db, &tm, &inputs);
    for (name, ours) in [("mask0", m0), ("mask1", m1)] {
        let want = std::fs::read(out.join(format!("locations.blm.{name}"))).unwrap_or_default();
        let bytes = wce_terrain::locations::mask_bytes(&ours);
        if bytes == want {
            println!("{name}: the same");
            continue;
        }

        failed = true;
        let theirs: Vec<u16> = want.chunks_exact(2).map(|c| u16::from_le_bytes([c[0], c[1]])).collect();
        let diff: Vec<usize> = (0..ours.len().min(theirs.len())).filter(|&i| ours[i] != theirs[i]).collect();
        let mut bits = [0usize; 16];
        for &i in &diff {
            for (b, n) in bits.iter_mut().enumerate() {
                if (ours[i] ^ theirs[i]) & (1 << b) != 0 {
                    *n += 1;
                }
            }
        }

        println!(
            "{name}: {} of {} cells differ (lengths {} and {}); by bit {bits:?}; first {:?}",
            diff.len(),
            ours.len(),
            ours.len(),
            theirs.len(),
            diff.iter()
                .take(4)
                .map(|&i| (i % tm.width as usize, i / tm.width as usize, format!("{:#x}", ours[i]), format!("{:#x}", theirs[i])))
                .collect::<Vec<_>>()
        );
    }

    if failed {
        std::process::exit(1);
    }
}
