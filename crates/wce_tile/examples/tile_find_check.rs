//! `tile_find_check <location> <variation> <raw_data>...`: where the loaders find a placement's tile (`sources::find_tile`, the roots in
//! order), twice: a variation a combined project serves is written as a project of its own the first time and taken as it is the
//! second (its stamp; docs/plans/tile_variations.md, the loaders). Then the folder opens as a project of one variation.
//!
//!     cargo run --release -p wce_tile --example tile_find_check -- terrain\tiles\battle\<set>\<tile> small <projects raw_data> <kit raw_data>

fn main() {
    let args = std::env::args().skip(1).collect::<Vec<_>>();
    let (Some(location), Some(variation)) = (args.first(), args.get(1)) else {
        eprintln!("usage: tile_find_check <location> <variation> <raw_data>...");
        std::process::exit(2);
    };

    let roots = args[2..].iter().map(std::path::PathBuf::from).collect::<Vec<_>>();
    println!("serves: {}", wce_tile::sources::serves(&roots, location, variation));
    for pass in 0..2 {
        let t = std::time::Instant::now();
        let found = wce_tile::sources::find_tile(&roots, location, variation);
        println!("pass {pass}: {:.2} s -> {:?}", t.elapsed().as_secs_f64(), found);
        if pass == 1
            && let Some(dir) = found
        {
            let p = wce_tile::tile_project::TileProject::open(&dir).unwrap_or_else(|e| panic!("{}: {e}", dir.display()));
            println!(
                "opens: combined {}, variation {:?}, height range {:.3} {:.3}, {} elements",
                p.is_combined(),
                p.info.variation,
                p.info.min_height,
                p.info.scale,
                p.scene.doc().elements().len()
            );
        }
    }
}
