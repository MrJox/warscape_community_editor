//! The mean RGBA of DDS textures of the game's packs (their mip of 64 texels or the nearest), and their size and format.
//!
//!   cargo run --release -p wce_terrain_io --example dds_mean -- <game dir> <dds path>...

use wce_vfs::{Config, GameProfile, Vfs};

fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let mut cfg = Config::new(&a[0], GameProfile::from_env().clone());
    cfg.mod_packs = Some(vec![]);
    let vfs = Vfs::mount(&cfg).expect("vfs");
    for p in &a[1..] {
        let Ok(b) = vfs.read(p) else {
            println!("{p}: not in the packs");
            continue;
        };

        let info = wce_formats::dds_decode::dds_info(&b);
        let mip = (0..info.mips.max(1)).find(|&m| (info.width >> m).max(1) <= 64).unwrap_or(0);
        match wce_formats::dds_decode::dds_decode_mip(&b, mip) {
            Ok(img) => {
                let n = (img.px.len() / 4).max(1) as u64;
                let mean: Vec<u64> = (0..4).map(|i| img.px.as_chunks::<4>().0.iter().map(|q| u64::from(q[i])).sum::<u64>() / n).collect();
                println!("{p}: {} x {} {} ({} mips), mean rgba {mean:?}", info.width, info.height, info.format, info.mips);
            }

            Err(e) => println!("{p}: {} x {} {:?}: {e}", info.width, info.height, info.format),
        }
    }
}
