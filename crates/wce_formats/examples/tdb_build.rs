//! Rebuilds tile_database.bin from its XML sources with rpfm_lib and compares it with a shipped file. Port of `cli/tdb_build.cpp`.
//!
//!   cargo run --release -p wce_formats --example tdb_build -- <_tile_database dir> <extra tiles dir or -> <shipped tile_database.bin or ->
//!       [--out file]
//!
//! With a shipped file the tile order is taken from it; the result is compared byte for byte and the first difference is reported. Without
//! one the default order is used.

use std::io::Cursor;
use std::path::{Path, PathBuf};

use rpfm_lib::files::tile_database::TileDatabase;
use rpfm_lib::files::{Decodeable, Encodeable};

fn xml_files(dir: &Path) -> Vec<PathBuf> {
    let mut files = std::fs::read_dir(dir)
        .map(|rd| rd.flatten().map(|e| e.path()).filter(|p| p.extension().is_some_and(|x| x == "xml")).collect::<Vec<_>>())
        .unwrap_or_default();
    files.sort();
    files
}

fn text(p: &Path) -> Result<String, String> {
    std::fs::read(p).map(|b| String::from_utf8_lossy(&b).into_owned()).map_err(|e| format!("{}: {e}", p.display()))
}

fn run(args: &[String]) -> Result<(), String> {
    let dir = PathBuf::from(&args[0]);
    let settings = text(&dir.join("_settings.xml"))?;
    let mut files = xml_files(&dir.join("TILES"));
    if args[1] != "-" && Path::new(&args[1]).exists() {
        files.extend(xml_files(Path::new(&args[1])));
    }

    let tiles = files.iter().map(|f| text(f)).collect::<Result<Vec<_>, _>>()?;
    let add_user = dir.display().to_string().contains("battle");
    let reference = if args[2] == "-" { None } else { Some(std::fs::read(&args[2]).map_err(|e| format!("{}: {e}", args[2]))?) };
    let order = match &reference {
        Some(r) => {
            let shipped = TileDatabase::decode(&mut Cursor::new(r), &None).map_err(|e| e.to_string())?;
            Some(shipped.tiles().iter().map(|t| t.location().to_owned()).collect::<Vec<_>>())
        }

        None => None,
    };

    let refs = tiles.iter().map(String::as_str).collect::<Vec<_>>();
    let mut db = TileDatabase::from_assembly_kit(&settings, &refs, order.as_deref(), add_user).map_err(|e| e.to_string())?;
    let mut out = vec![];
    db.encode(&mut out, &None).map_err(|e| e.to_string())?;
    println!("built {} bytes from {} tiles", out.len(), files.len());
    for w in args[3..].windows(2) {
        if w[0] == "--out" {
            std::fs::write(&w[1], &out).map_err(|e| format!("{}: {e}", w[1]))?;
        }
    }

    if let Some(r) = reference {
        let n = r.len().min(out.len());
        let diff = (0..n).filter(|&i| r[i] != out[i]).count();
        let first = (0..n).find(|&i| r[i] != out[i]).unwrap_or(0);
        println!("shipped {} bytes; {} differing bytes, first at {first}", r.len(), diff + (r.len() - n) + (out.len() - n));
    }

    Ok(())
}

fn main() {
    let args = std::env::args().skip(1).collect::<Vec<_>>();
    if args.len() < 3 {
        eprintln!("usage: tdb_build <_tile_database dir> <extra tiles dir|-> <shipped bin|-> [--out file]");
        std::process::exit(2);
    }

    if let Err(e) = run(&args) {
        eprintln!("error: {e}");
        std::process::exit(1);
    }
}
