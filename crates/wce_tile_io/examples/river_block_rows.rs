//! Exploration: the rows of a terrain mesh's river block (`attila::river::river_rows`): per river its first, middle and last rows' centre
//! and width, as the file has them (no frame change).
//!
//!   cargo run --release -p wce_tile_io --example river_block_rows -- <mesh.rigid_model_v2>

fn main() {
    let b = std::fs::read(std::env::args().nth(1).expect("mesh")).expect("read");
    let block = wce_tile_io::attila::river::read_river_block(&b).expect("a river block");
    for (n, rows) in wce_tile_io::attila::river::river_rows(&block).iter().enumerate() {
        println!("river {n}: {} rows", rows.len());
        for r in rows.iter().step_by((rows.len() / 8).max(1)) {
            println!("  centre {:8.2} {:6.2} {:8.2} width {:.1}", r.centre[0], r.centre[1], r.centre[2], r.width);
        }
    }
}
