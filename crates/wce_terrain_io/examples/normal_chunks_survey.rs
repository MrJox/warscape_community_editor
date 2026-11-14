//! Exploration: the LF normal chunks of a game's battle maps (`normals/normal_N.dds`): per map its LF's size, the chunks' side and count,
//! the grid that count fits (chunks every `step` samples of the LF over it, `side = 2 * step + 2 * margin`), and how far two neighbouring
//! chunks agree where the grid says they overlap (the mean difference of their decoded pixels' x and y, 0 .. 255).
//!
//!   WCE_GAME_KEY=rome_2 cargo run --release -p wce_terrain_io --example normal_chunks_survey -- <game dir> [text in the map's name]

use wce_formats::dds_decode::dds_decode_mip;
use wce_vfs::{Config, GameProfile, Vfs};

fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let mut cfg = Config::new(&a[0], GameProfile::from_env().clone());
    cfg.mod_packs = Some(vec![]);
    let vfs = Vfs::mount(&cfg).expect("vfs");
    let filter = a.get(1).cloned().unwrap_or_default();
    let mut maps: Vec<String> = vfs
        .list("terrain/battles/", ".dds")
        .iter()
        .filter(|p| p.contains("/normals/normal_0.dds"))
        .map(|p| p.trim_end_matches("/normals/normal_0.dds").to_owned())
        .collect();
    maps.sort();
    let u32_at = |b: &[u8], o: usize| u32::from_le_bytes([b[o], b[o + 1], b[o + 2], b[o + 3]]);
    for m in maps.iter().filter(|m| m.contains(&filter)) {
        let Ok(lf) = vfs.read(&format!("{m}/lf_height_map.dds")) else { continue };
        let (lh, lw) = (u32_at(&lf, 12), u32_at(&lf, 16));
        let count = vfs.list(&format!("{m}/normals/"), ".dds").len() as u32;
        let Ok(c0) = vfs.read(&format!("{m}/normals/normal_0.dds")) else { continue };
        let side = u32_at(&c0, 16);
        // The grids that give this count: step s, the normal map k times the LF.
        let mut fits = vec![];
        for k in [1u32, 2, 4] {
            for s in [128u32, 256, 512] {
                if lw.saturating_mul(k).div_ceil(s) * lh.saturating_mul(k).div_ceil(s) == count {
                    fits.push(format!("x{k} step {s}"));
                }
            }
        }

        // Chunk 0 against chunk 1 (to its right) at the offsets of the steps.
        let mut agree = String::new();
        if count > 1
            && let (Ok(a0), Ok(a1)) = (
                dds_decode_mip(&c0, 0),
                vfs.read(&format!("{m}/normals/normal_1.dds"))
                    .map_err(|e| e.to_string())
                    .and_then(|b| dds_decode_mip(&b, 0).map_err(str::to_owned)),
            )
        {
            for s in [128u32, 256, 512] {
                let (mut sum, mut n) = (0u64, 0u64);
                for y in (0..side).step_by(7) {
                    for x in (s..side).step_by(7) {
                        let (p, q) = (a0.at(x, y), a1.at(x - s, y));
                        sum += u64::from(p[3].abs_diff(q[3])) + u64::from(p[1].abs_diff(q[1]));
                        n += 2;
                    }
                }

                agree += &format!(" step {s}: {:.1}", sum as f64 / n.max(1) as f64);
            }
        }

        println!("{m}: LF {lw} x {lh}, {count} chunks of {side}; fits {fits:?};{agree}");
    }
}
