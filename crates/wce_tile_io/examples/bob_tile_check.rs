//! Stage B against a fresh BOB run: a raw tile folder (kit layout) through `wce_tile_io::attila::bob_tile::textures`, compared with BOB's
//! files in its working folder: whole files, and for the DXT5 ones level by level and block by block.
//!
//!   cargo run --release -p wce_tile_io --example bob_tile_check -- <raw tile folder> <BOB's working tile folder>
//!
//! `WCE_SHOW=1` prints the first differing blocks.

use std::path::PathBuf;

use wce_tile_io::attila::TedProject;
use wce_tile_io::attila::bob_tile::textures;
use wce_tile_io::{TileFiles, TileFormat};

fn main() {
    let args = std::env::args().skip(1).map(PathBuf::from).collect::<Vec<_>>();
    let (raw, work) = (&args[0], &args[1]);
    let files = TileFiles::from_kit_dir(raw).expect("the raw tile");
    let (project, _) = TedProject.read(&files).expect("the project");
    for (name, ours) in textures(&project).expect("stage B") {
        let Ok(theirs) = std::fs::read(work.join(&name)) else {
            println!("{name}: BOB wrote none");
            continue;
        };
        if ours == theirs {
            println!("{name}: identical ({} bytes)", ours.len());
            continue;
        }

        println!("{name}: {} bytes ours, {} BOB's; header {}", ours.len(), theirs.len(), if ours[..128] == theirs[..128] { "same" } else { "DIFFERS" });
        if &theirs[84..88] != b"DXT5" {
            let n = ours.iter().zip(&theirs).skip(128).filter(|(a, b)| a != b).count();
            println!("  {n} bytes differ");
            continue;
        }

        let side = u32::from_le_bytes(theirs[16..20].try_into().unwrap()) as usize;
        let (mut off, mut lw, mut lh, mut shown) = (128usize, side, side, 0);
        while off < ours.len().min(theirs.len()) {
            let n = lw.div_ceil(4) * lh.div_ceil(4);
            let (mut alpha, mut colour) = (0, 0);
            for b in 0..n {
                let (o, t) = (&ours[off + b * 16..off + b * 16 + 16], &theirs[off + b * 16..off + b * 16 + 16]);
                alpha += usize::from(o[..8] != t[..8]);
                colour += usize::from(o[8..] != t[8..]);
                if o != t && shown < 3 && std::env::var_os("WCE_SHOW").is_some() {
                    shown += 1;
                    let (bx, by) = ((b % lw.div_ceil(4)) * 4, (b / lw.div_ceil(4)) * 4);
                    println!("  level {lw}x{lh} block ({bx},{by}): ours {o:02x?}\n                           BOB  {t:02x?}");
                }
            }

            if alpha + colour > 0 {
                println!("  level {lw}x{lh}: {n} blocks, alpha differs {alpha}, colour differs {colour}");
            }

            off += n * 16;
            (lw, lh) = ((lw / 2).max(1), (lh / 2).max(1));
        }
    }
}
