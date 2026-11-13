//! Exploration: a CHMF file's header and first regions; with a second file, the regions that differ (rect, tag, samples, encoding).
//!
//!   cargo run --release -p wce_terrain_io --example chmf_head -- <lf_height_map.data> [other lf_height_map.data]
use wce_formats::chmf::ChmfFile;

fn head(f: &ChmfFile) {
    println!(
        "v{} {} x {} cells {} x {} min {} max {} spacing {} regions {}",
        f.version,
        f.width,
        f.height,
        f.cells_x,
        f.cells_y,
        f.min,
        f.max,
        f.spacing,
        f.regions.len()
    );
    for r in f.regions.iter().take(3) {
        println!("  rect {:?} tag {:?} data {}", r.rect, r.tag, r.data.len());
    }
}

fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let f = ChmfFile::parse(&std::fs::read(&a[0]).expect("file")).expect("chmf");
    head(&f);
    let Some(other) = a.get(1) else { return };
    let g = ChmfFile::parse(&std::fs::read(other).expect("file")).expect("chmf");
    head(&g);
    let mut shown = 0;
    let (mut samples, mut encoding) = (0, 0);
    for (i, (x, y)) in f.regions.iter().zip(&g.regions).enumerate() {
        if x == y {
            continue;
        }

        let (sx, sy) = (f.region_samples(i).expect("ours"), g.region_samples(i).expect("theirs"));
        if sx == sy {
            encoding += 1
        } else {
            samples += 1
        }
        if shown < 3 {
            shown += 1;
            let first = sx.2.iter().zip(&sy.2).position(|(p, q)| p != q);
            println!(
                "region {i}: rects {:?} / {:?} tags {:?} / {:?} sizes {} x {} / {} x {} data {} / {}; first differing sample {first:?} {:?}",
                x.rect,
                y.rect,
                x.tag,
                y.tag,
                sx.0,
                sx.1,
                sy.0,
                sy.1,
                x.data.len(),
                y.data.len(),
                first.map(|k| (sx.2[k], sy.2[k]))
            );
        }
    }

    println!("regions differing: {samples} in samples, {encoding} in encoding only");
}
