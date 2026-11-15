//! `tile_materialize_check <combined tile folder> <kit tile folder> <out raw_data>`: writes every variation of a combined project as a
//! project of its own (`TileProject::materialize`, what every export of a combined project goes through; docs/plans/tile_variations.md)
//! and checks each against the kit's variation it was combined from: tile.ted through `to_ted` byte for byte, the height map's 16-bit
//! samples and its range (tile.ted against the kit's read and written back by the editor), the blend and ground type maps byte for byte.
//!
//!     cargo run --release -p wce_tile --example tile_materialize_check -- <projects>/raw_data/terrain/tiles/battle/<set>/<tile> \
//!         <kit>/raw_data/terrain/tiles/battle/<set>/<tile> <scratch>/raw_data
//!
//! Exits with 1 when a variation differs.

use std::path::Path;

use wce_formats::png_file::PngFile;
use wce_tile::tile_project::TileProject;

fn samples(path: &Path) -> Result<Vec<u8>, String> {
    let bytes = std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
    Ok(PngFile::parse(&bytes).map_err(|e| format!("{}: {e}", path.display()))?.pixels)
}

fn main() {
    let args = std::env::args().collect::<Vec<_>>();
    let [_, combined, kit, out] = &args[..] else {
        eprintln!("usage: tile_materialize_check <combined tile folder> <kit tile folder> <out raw_data>");
        std::process::exit(2);
    };

    let start = std::time::Instant::now();
    let project = TileProject::open(Path::new(combined)).unwrap_or_else(|e| panic!("{combined}: {e}"));
    let text = project.scene.save();
    let entry = project.entry.clone().expect("an entry");
    let mut bad = 0;
    for v in project.variations.clone() {
        let t = std::time::Instant::now();
        let m = project.materialize(&v, &text, Path::new(out)).unwrap_or_else(|e| panic!("{v}: {e}"));
        let row = &entry.variations()[entry.find_variation(&v).expect("a row")];
        let theirs = Path::new(kit).join(&v);
        let mut notes = vec![];

        let ours = std::fs::read(m.dir.join("tile.wce")).expect("tile.wce");
        let (ted, _) = wce_tile::project_files::to_ted(ours).unwrap_or_else(|e| panic!("{v}: {e}"));
        // Against the kit's file as the editor reads and writes it back (`to_wce`, `to_ted`): what TEd wrote that the editor writes
        // otherwise (`parallax_scale="-0"`) is not the combination's.
        let kit_ted = std::fs::read(theirs.join("tile.ted")).expect("the kit's tile.ted");
        let kit_back = wce_tile::project_files::to_ted(wce_tile::project_files::to_wce(kit_ted).expect("to_wce")).expect("to_ted").0;
        if kit_back != ted {
            let keep = Path::new(out).join(format!("{v}.ted"));
            std::fs::write(&keep, &ted).unwrap_or_else(|e| panic!("{}: {e}", keep.display()));
            notes.push(format!("tile.ted differs (ours in {})", keep.display()));
        }

        match (samples(&m.dir.join("height_map_0.png")), samples(&theirs.join("height_map_0.png"))) {
            (Ok(a), Ok(b)) if a == b => {}
            (Ok(a), Ok(b)) => notes.push(format!(
                "the height map differs in {} of {} bytes",
                a.iter().zip(&b).filter(|(x, y)| x != y).count() + a.len().abs_diff(b.len()),
                b.len()
            )),
            (a, b) => notes.push(format!("the height map: {:?} / {:?}", a.err(), b.err())),
        }

        if m.refit || m.range != (row.min_height, row.scale) {
            notes.push(format!("the range is {:?}, the row's ({}, {})", m.range, row.min_height, row.scale));
        }

        for name in ["blend_map.tif", "ground_type_map.png"] {
            let (a, b) = (std::fs::read(m.dir.join(name)).ok(), std::fs::read(theirs.join(name)).ok());
            if a != b {
                notes.push(format!("{name} differs (ours {:?} bytes, the kit's {:?})", a.map(|x| x.len()), b.map(|x| x.len())));
            }
        }

        let files =
            std::fs::read_dir(&m.dir).map(|r| r.flatten().map(|f| f.file_name().to_string_lossy().into_owned()).collect::<Vec<_>>());
        println!("{v}: {:.1} s, {} -> {}", t.elapsed().as_secs_f64(), files.unwrap_or_default().join(" "), m.dir.display());
        for n in &notes {
            println!("  {n}");
        }

        bad += usize::from(!notes.is_empty());
    }

    println!("{} variation(s), {bad} differ; {:.1} s", project.variations.len(), start.elapsed().as_secs_f64());
    std::process::exit(i32::from(bad != 0));
}
