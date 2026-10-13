//! Gate of the number styles, as the C++ `format_check`: reads and re-writes every attribute value of the given folders, and reports
//! the values whose text would change (where an edit would change the number style).
//!
//!   cargo run --release -p wce_formats --example format_check -- <dir>...

use std::collections::BTreeMap;
use std::path::Path;

use wce_formats::XmlDoc;
use wce_formats::env_value::{ValueKind, format_value, parse_value};

fn walk(dir: &Path, files: &mut Vec<std::path::PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            walk(&path, files);
        } else if path.extension().is_some_and(|ext| ext == "environment" || ext == "lighting" || ext == "xml") {
            files.push(path);
        }
    }
}

fn main() {
    let mut files = vec![];
    for dir in std::env::args().skip(1) {
        walk(Path::new(&dir), &mut files);
    }

    let mut by_kind = BTreeMap::<ValueKind, (usize, usize)>::new();
    let mut examples = BTreeMap::<String, usize>::new();
    for file in files {
        let Ok(doc) = std::fs::read(&file).map_err(|_| ()).and_then(|bytes| XmlDoc::parse(bytes).map_err(|_| ())) else { continue };
        for (e, el) in doc.elements().iter().enumerate() {
            for a in 0..el.attrs.len() {
                let raw = String::from_utf8_lossy(doc.attr_raw(e, a));
                let value = parse_value(&raw);
                if value.kind == ValueKind::String {
                    continue;
                }

                let formatted = format_value(&value);
                let counts = by_kind.entry(value.kind).or_default();
                if formatted == raw {
                    counts.0 += 1;
                } else {
                    counts.1 += 1;
                    if examples.len() < 40 {
                        *examples.entry(format!("{raw}  ->  {formatted}")).or_default() += 1;
                    }
                }
            }
        }
    }

    for (kind, (same, differs)) in by_kind {
        println!("{:<9} same {same:>7}  differs {differs}", format!("{kind:?}").to_lowercase());
    }

    for example in examples.keys() {
        println!("  {example}");
    }
}
