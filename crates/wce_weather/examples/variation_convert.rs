//! Converts the battle weather selection database between its XML form and FASTBIN0. Port of `cli/variation_convert.cpp`.
//!
//!   cargo run -p wce_weather --example variation_convert -- <in.xml|in.bin> <out.bin|out.xml> [--force]
//!
//! The direction follows the file extensions. The data is validated first (more than 32 values, duplicate values, flag bits that belong to no
//! value, variations that can never match, no 'default_map'); problems stop the conversion unless --force is given. XML output uses the
//! canonical layout (CRLF, tabs, single quotes), which is byte-identical to the vanilla file.

use std::io::Cursor;
use std::path::Path;

use rpfm_lib::files::asset_variation_db::AssetVariationDb;
use rpfm_lib::files::{Decodeable, Encodeable};

fn ext(p: &str) -> String {
    Path::new(p).extension().map(|e| format!(".{}", e.to_string_lossy().to_lowercase())).unwrap_or_default()
}

fn run(input: &str, output: &str, force: bool) -> Result<i32, String> {
    let bytes = std::fs::read(input).map_err(|e| format!("cannot read {input}: {e}"))?;
    let to_bin = ext(output) == ".bin";
    let mut db = if to_bin {
        AssetVariationDb::from_xml(&String::from_utf8_lossy(&bytes)).map_err(|e| e.to_string())?
    } else {
        AssetVariationDb::decode(&mut Cursor::new(&bytes), &None).map_err(|e| e.to_string())?
    };

    let problems = db.validate();
    for p in &problems {
        println!("{}: {p}", if force { "warning" } else { "problem" });
    }

    if !problems.is_empty() && !force {
        println!("nothing written (use --force to write it anyway)");
        return Ok(1);
    }

    let out = if to_bin {
        let mut out = vec![];
        db.encode(&mut out, &None).map_err(|e| e.to_string())?;
        out
    } else {
        db.to_xml().into_bytes()
    };

    std::fs::write(output, &out).map_err(|_| format!("cannot write {output}"))?;
    let variations = db.entries().iter().map(|e| e.variations().len()).sum::<usize>();
    println!(
        "{input} -> {output}: {} categories ({} values), {variations} variations, {} bytes", db.categories().len(), db.value_count(), out.len(),
    );
    Ok(0)
}

fn main() {
    let mut force = false;
    let mut args = vec![];
    for a in std::env::args().skip(1) {
        if a == "--force" {
            force = true;
        } else {
            args.push(a);
        }
    }

    let ok_dirs = args.len() == 2 && ((ext(&args[0]) == ".xml" && ext(&args[1]) == ".bin") || (ext(&args[0]) == ".bin" && ext(&args[1]) == ".xml"));
    if !ok_dirs {
        println!("usage: variation_convert <in.xml|in.bin> <out.bin|out.xml> [--force]");
        std::process::exit(2);
    }

    match run(&args[0], &args[1], force) {
        Ok(code) => std::process::exit(code),
        Err(e) => {
            println!("error: {e}");
            std::process::exit(1);
        }
    }
}
