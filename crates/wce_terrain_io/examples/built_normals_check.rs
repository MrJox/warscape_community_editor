//! A built map's LF normal chunks after an LF edit (Rome 2's: `wce_terrain_io::built::write`, from its own shipped chunks): a hill raised
//! on the map's LF, the files written, and for the chunks written: that they are the ones the edit reaches, their pixels away from the
//! edit the shipped chunk's (the mean difference of x and y, 0 .. 255, after DXT5 again), and the edited pixels' normals moved out of the
//! hill from the shipped ones (x, y against the hill's slope). An unedited LF writes no chunk; a map whose chunks are laid out otherwise
//! (`normals::Layout::of`) keeps them. Exit 1 when one fails.
//!
//!   WCE_GAME_KEY=rome_2 cargo run --release -p wce_terrain_io --example built_normals_check -- <game dir> <map folder, e.g. terrain/battles/main_gaul>

use wce_formats::dds_decode::dds_decode_mip;
use wce_terrain::LfRole;
use wce_vfs::{Config, GameProfile, Vfs};

fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let mut cfg = Config::new(&a[0], GameProfile::from_env().clone());
    cfg.mod_packs = Some(vec![]);
    let vfs = Vfs::mount(&cfg).expect("vfs");
    let folder = a[1].trim_end_matches('/');
    let prefix = format!("{folder}/");
    let files: Vec<(String, Vec<u8>)> = vfs.list(&prefix, "").iter().map(|p| (p[prefix.len()..].to_owned(), vfs.read(p).expect("read").to_vec())).collect();
    let name = folder.rsplit('/').next().unwrap_or(folder);
    let mut m = wce_terrain_io::built::read(name, &wce_terrain_io::MapFiles::from_bytes(files.clone())).expect("model");
    let chunk_names = |f: &wce_terrain_io::MapFiles| f.folder.keys().filter(|k| k.starts_with("normals/")).cloned().collect::<Vec<_>>();
    let unedited = wce_terrain_io::built::write(&m, None).expect("write");
    let same = unedited.folder.iter().filter(|(k, _)| k.starts_with("normals/")).all(|(k, b)| files.iter().any(|(n, s)| n == k && b.bytes().ok().as_deref() == Some(s.as_slice())));
    println!("unedited: {} chunks, all as shipped: {same}", chunk_names(&unedited).len());

    // A hill: radius r samples at (cx, cy), up to 3000 raw units.
    let item = m.lf_mut(LfRole::Land).expect("an LF");
    let (w, h) = (item.get().width(), item.get().height());
    let (cx, cy, r) = (w / 2 + 37, h / 2 + 11, 40i64);
    let field = item.edit();
    println!("the LF at the hill's centre before: {}", field.at(cx, cy));
    for y in (cy as i64 - r).max(0)..(cy as i64 + r).min(i64::from(h)) {
        for x in (cx as i64 - r).max(0)..(cx as i64 + r).min(i64::from(w)) {
            let d = ((x - i64::from(cx)).pow(2) as f64 + (y - i64::from(cy)).pow(2) as f64).sqrt();
            if d < r as f64 {
                let v = field.at(x as u32, y as u32);
                field.set(x as u32, y as u32, v.saturating_add((3000.0 * (1.0 - d / r as f64)) as u16));
            }
        }
    }

    let out = wce_terrain_io::built::write(&m, None).expect("write");
    let changed: Vec<String> = out.folder.iter().filter(|(k, b)| k.starts_with("normals/") && files.iter().all(|(n, s)| n != *k || b.bytes().ok().as_deref() != Some(s.as_slice()))).map(|(k, _)| k.clone()).collect();
    println!("hill at ({cx}, {cy}) radius {r} on {w} x {h}: {} chunks written: {changed:?}", changed.len());
    let mut ok = same;
    let shipped = |k: &str| files.iter().find(|(n, _)| n == k).map(|(_, b)| b.clone());
    for k in &changed {
        let (Some(old), Some(new)) = (shipped(k), out.folder[k].bytes().ok()) else { ok = false; continue };
        let (Ok(o), Ok(n)) = (dds_decode_mip(&old, 0), dds_decode_mip(&new, 0)) else { ok = false; continue };
        let side = o.w;
        // The chunk's origin from its number (the 256-sample grid, a 32-sample margin; or one chunk of the whole LF).
        let num: i64 = k.trim_start_matches("normals/normal_").trim_end_matches(".dds").parse().unwrap_or(0);
        let step = if side == 576 && w > 512 { 256 } else { i64::from(w) };
        let cols = (i64::from(w) + step - 1) / step;
        let (ox, oy) = ((num % cols) * step - 32, (num / cols) * step - 32);
        let (mut far, mut nfar, mut lean, mut nlean, mut flat) = (0u64, 0u64, 0i64, 0i64, 0i64);
        for sy in 0..side {
            for sx in 0..side {
                let (x, y) = (ox + i64::from(sx), oy + i64::from(sy));
                let d = (((x - i64::from(cx)).pow(2) + (y - i64::from(cy)).pow(2)) as f64).sqrt();
                let (p, q) = (o.at(sx, sy), n.at(sx, sy));
                if d > (r + 8) as f64 {
                    far += u64::from(p[3].abs_diff(q[3])) + u64::from(p[1].abs_diff(q[1]));
                    nfar += 2;
                } else if d > 4.0 && d < (r - 4) as f64 {
                    // Out of the hill: x against -dx (east of the centre leans east), y against the south-minus-north slope.
                    // The change the hill makes to the shipped normal (the ground under it may slope more than the hill).
                    let (ex, ey) = (i64::from(q[3]) - i64::from(p[3]), i64::from(q[1]) - i64::from(p[1]));
                    flat += i64::from(ex == 0) + i64::from(ey == 0);
                    // Over the components that changed: +1 out of the hill, -1 into it.
                    for (e, d) in [(ex, x - i64::from(cx)), (ey, -(y - i64::from(cy)))] {
                        if e != 0 && d != 0 {
                            lean += (e * d).signum();
                            nlean += 1;
                        }
                    }
                }
            }
        }

        let (far_mean, lean_share) = (far as f64 / nfar.max(1) as f64, (lean + nlean) as f64 / (2 * nlean.max(1)) as f64);
        let flat_share = flat as f64 / (nlean + flat).max(1) as f64;
        println!("  {k}: away from the hill {far_mean:.2} off the shipped chunk; on it {:.0} % of the changed x and y move out of it ({:.0} % unchanged)", 100.0 * lean_share, 100.0 * flat_share);
        ok &= far_mean < 2.0 && (nlean == 0 || lean_share > 0.9);
    }

    // A layout the writer does not know: the shipped chunks kept, nothing written.
    let count = files.iter().filter(|(k, _)| k.starts_with("normals/")).count();
    let side = shipped("normals/normal_0.dds").map_or(0, |b| i64::from(u32::from_le_bytes(b[16..20].try_into().unwrap())));
    let known = wce_terrain_io::attila::normals::Layout::of(count, side, i64::from(w), i64::from(h)).is_some();
    if !known {
        println!("chunks of {side} over {w} x {h}: a layout not handled, the shipped chunks kept");
    }

    ok &= changed.is_empty() != known;
    println!("{}", if ok { "ok" } else { "FAILED" });
    if !ok {
        std::process::exit(1);
    }
}
