//! The heights of a CHMF file (`hf_height_map.data`): the header, then (with an output file) every sample of the first region as f32
//! metres, row-major, little endian.
//!
//!   cargo run --release -p wce_tile_io --example tile_chmf_dump -- <file.data> [out.f32]

use wce_formats::chmf::ChmfFile;

fn main() {
    let args = std::env::args().skip(1).collect::<Vec<_>>();
    let Some(path) = args.first() else {
        eprintln!("usage: tile_chmf_dump <file.data> [out.f32]");
        std::process::exit(2);
    };

    let f = ChmfFile::parse(&std::fs::read(path).expect("read")).expect("CHMF");
    println!(
        "version {}, {} x {} samples, cells {} x {}, min {}, range {}, spacing {} m, {} regions",
        f.version, f.width, f.height, f.cells_x, f.cells_y, f.min, f.range, f.spacing, f.regions.len()
    );

    for (i, r) in f.regions.iter().enumerate() {
        let (w, h, samples) = f.region_samples(i).expect("region");
        let (lo, hi) = samples.iter().fold((u16::MAX, 0), |(lo, hi), &s| (lo.min(s), hi.max(s)));
        println!("region {i}: rect {:?}, tag {:?}, {} blocks, {w} x {h}, samples {lo} ..= {hi} ({} .. {} m)", r.rect, r.tag, r.offsets.len(), f.metres(lo), f.metres(hi));
        if i == 0
            && let Some(out) = args.get(1)
        {
            std::fs::write(out, samples.iter().flat_map(|&s| f.metres(s).to_le_bytes()).collect::<Vec<_>>()).expect("write");
        }
    }
}
