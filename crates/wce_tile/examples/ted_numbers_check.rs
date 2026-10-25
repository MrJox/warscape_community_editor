//! Whether TEd's numbers come back as they are when read and formatted again (`project_files::ted_number_text`): every attribute value of
//! the kit's tile.ted files that holds numbers, by its shape and attribute. `to_ted` re-formats the full-precision numbers of tile.wce
//! this way, so a value TEd wrote must not change.
//!
//!     cargo run --release -p wce_tile --example ted_numbers_check -- <kit raw_data/terrain/tiles> [max files]

use std::collections::BTreeMap;

use wce_formats::xml_doc::XmlDoc;
use wce_tile::project_files::ted_number_text;

fn main() {
    let args = std::env::args().collect::<Vec<_>>();
    let root = std::path::PathBuf::from(args.get(1).expect("usage: ted_numbers_check <tiles> [max]"));
    let max = args.get(2).and_then(|m| m.parse().ok()).unwrap_or(usize::MAX);
    let mut files = vec![];
    let mut stack = vec![root];
    while let Some(d) = stack.pop() {
        for e in std::fs::read_dir(&d).into_iter().flatten().flatten() {
            let p = e.path();
            if p.is_dir() {
                stack.push(p);
            } else if p.file_name().is_some_and(|n| n == "tile.ted") {
                files.push(p);
            }
        }
    }

    files.sort();
    files.truncate(max);
    let mut seen = BTreeMap::<String, (u64, u64, String)>::new();
    for f in &files {
        let Ok(doc) = XmlDoc::parse(std::fs::read(f).unwrap()) else { continue };
        for (e, el) in doc.elements().iter().enumerate() {
            for a in &el.attrs {
                let Some(v) = doc.get(e, &a.name) else { continue };
                let Some(again) = ted_number_text(&el.name, &a.name, &v) else { continue };
                let key = format!("{}.{}", el.name, a.name);
                let s = seen.entry(key).or_default();
                s.0 += 1;
                if again != v {
                    s.1 += 1;
                    if s.2.is_empty() {
                        s.2 = format!("{v} -> {again}");
                    }
                }
            }
        }
    }

    let (mut all, mut bad) = (0, 0);
    for (k, (n, b, ex)) in &seen {
        all += n;
        bad += b;
        if *b != 0 {
            println!("{k}: {b} of {n} change, e.g. {ex}");
        }
    }

    println!("{} files, {all} numeric values, {bad} change", files.len());
}
