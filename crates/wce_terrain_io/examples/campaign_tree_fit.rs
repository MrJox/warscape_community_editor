//! Exploration: where a campaign's `trees.campaign_tree_list` puts its trees. Per mapping of its (x, z) to map cells, the share of trees on
//! land (the LF above the sea at their cell, from BOB's `lf_height_map_lq.dds`, 4 samples a cell... its own size over the map's cells), and
//! the trees' ranges. The file: u32 version, f32 x0 z0 x1 z1 (bounds), u32 models, per model a u16-length path and u32 count of 15-byte
//! trees (f32 x y z, u8 a, b, c).
//!
//!   cargo run --release -p wce_terrain_io --example campaign_tree_fit -- <trees.campaign_tree_list> <lf_height_map_lq.dds> <cells w> <cells h> <lf_height> <vertical_offset>

fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let b = std::fs::read(&a[0]).expect("tree list");
    let lf = std::fs::read(&a[1]).expect("lf");
    let (cw, ch): (f64, f64) = (a[2].parse().expect("w"), a[3].parse().expect("h"));
    let (lf_height, offset): (f64, f64) = (a[4].parse().expect("lf_height"), a[5].parse().expect("offset"));
    let u32_at = |b: &[u8], o: usize| u32::from_le_bytes(b[o..o + 4].try_into().unwrap());
    let f32_at = |b: &[u8], o: usize| f32::from_le_bytes(b[o..o + 4].try_into().unwrap());
    let bounds = [f32_at(&b, 4), f32_at(&b, 8), f32_at(&b, 12), f32_at(&b, 16)];
    let models = u32_at(&b, 20) as usize;
    let mut at = 24;
    let mut trees: Vec<[f32; 3]> = vec![];
    let mut bytes = [std::collections::BTreeMap::<u8, usize>::new(), Default::default(), Default::default()];
    for _ in 0..models {
        let n = usize::from(u16::from_le_bytes([b[at], b[at + 1]]));
        let path = String::from_utf8_lossy(&b[at + 2..at + 2 + n]).into_owned();
        at += 2 + n;
        let count = u32_at(&b, at) as usize;
        at += 4;
        println!("{count:6} {path}");
        for k in 0..count {
            let o = at + k * 15;
            trees.push([f32_at(&b, o), f32_at(&b, o + 4), f32_at(&b, o + 8)]);
            for j in 0..3 {
                *bytes[j].entry(b[o + 12 + j]).or_default() += 1;
            }
        }

        at += count * 15;
    }

    println!("version {} bounds {bounds:?}, {} trees, {} bytes left", u32_at(&b, 0), trees.len(), b.len() - at);
    for (k, axis) in ["x", "y", "z"].iter().enumerate() {
        let (lo, hi) = trees.iter().fold((f32::MAX, f32::MIN), |(l, h), t| (l.min(t[k]), h.max(t[k])));
        println!("  {axis} {lo:.3} .. {hi:.3}");
    }

    for (j, m) in bytes.iter().enumerate() {
        println!("  byte {j}: {:?}", m.iter().take(12).collect::<Vec<_>>());
    }

    let (lh, lw) = (u32_at(&lf, 12) as usize, u32_at(&lf, 16) as usize);
    let land = |cx: f64, cy: f64| -> Option<bool> {
        let (sx, sy) = ((cx / cw * lw as f64) as isize, (cy / ch * lh as f64) as isize);
        if sx < 0 || sy < 0 || sx as usize >= lw || sy as usize >= lh {
            return None;
        }

        let o = 128 + 2 * (sy as usize * lw + sx as usize);
        let v = f64::from(u16::from_le_bytes([lf[o], lf[o + 1]])) / 65535.0 * lf_height - offset;
        Some(v > 0.5)
    };

    let tests: Vec<(&str, Box<dyn Fn(f32, f32) -> (f64, f64)>)> = vec![
        ("terry cell 0.32, z north", Box::new(|x, z| (f64::from(x) / 0.32, ch - f64::from(z) / 0.32))),
        ("terry cell 0.32, z south", Box::new(|x, z| (f64::from(x) / 0.32, f64::from(z) / 0.32))),
        ("bounds, z north", Box::new(move |x, z| (f64::from(x) / f64::from(bounds[2]) * cw, (1.0 - f64::from(z) / f64::from(bounds[3])) * ch))),
        ("bounds, z south", Box::new(move |x, z| (f64::from(x) / f64::from(bounds[2]) * cw, f64::from(z) / f64::from(bounds[3]) * ch))),
    ];
    for (name, f) in &tests {
        let (mut on, mut off, mut out) = (0, 0, 0);
        for t in trees.iter().step_by(7) {
            let (cx, cy) = f(t[0], t[2]);
            match land(cx, cy) {
                Some(true) => on += 1,
                Some(false) => off += 1,
                None => out += 1,
            }
        }

        println!("{name:28} land {on} sea {off} outside {out}: {:.1} % on land", 100.0 * on as f64 / (on + off + out).max(1) as f64);
    }
}
