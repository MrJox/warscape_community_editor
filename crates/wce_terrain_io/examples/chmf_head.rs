//! Exploration: a CHMF file's header and first regions.
//!
//!   cargo run --release -p wce_terrain_io --example chmf_head -- <lf_height_map.data>
fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let f = wce_formats::chmf::ChmfFile::parse(&std::fs::read(&a[0]).expect("file")).expect("chmf");
    println!("v{} {} x {} cells {} x {} min {} max {} spacing {} regions {}", f.version, f.width, f.height, f.cells_x, f.cells_y, f.min, f.max, f.spacing, f.regions.len());
    for r in f.regions.iter().take(3) {
        println!("  rect {:?} tag {:?} data {}", r.rect, r.tag, r.data.len());
    }
}
