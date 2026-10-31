//! The records of a `prop_marker.markers` file: per record its key (the group's), `is_decal`, the matrix's rows (x, y, z axes times the
//! half scale, in the binary's frame) and translation, and the decal fields; for comparing what the game draws with the tile.ted.
//!
//!   cargo run --release -p wce_formats --example prop_markers_dump -- <prop_marker.markers> [text in the key] [--tilted]
//!
//! `--tilted`: only the records whose y row is not vertical (a decal projected sideways).

use wce_formats::markers::{KeyPart, MarkersFile, Value};

fn main() {
    let args = std::env::args().skip(1).collect::<Vec<_>>();
    let Some(path) = args.first() else {
        eprintln!("usage: prop_markers_dump <prop_marker.markers> [filter] [--tilted]");
        std::process::exit(2);
    };

    let tilted = args.iter().any(|a| a == "--tilted");
    let filter = args.iter().skip(1).find(|a| !a.starts_with("--")).map(|f| f.to_lowercase());
    let f = MarkersFile::parse(&std::fs::read(path).expect("the file")).expect("a markers file");
    println!("{} version {}, {} records", f.class_name, f.version, f.records.len());

    // The record -> key map from the groups.
    let mut key_of = vec![String::new(); f.records.len()];
    for g in f.groups.iter().flatten() {
        let key = g.key.iter().map(|k| match k {
            KeyPart::Wide(w) => String::from_utf16_lossy(w),
            KeyPart::Narrow(n) => String::from_utf8_lossy(n).into_owned(),
            KeyPart::U32(u) => u.to_string(),
        }).collect::<Vec<_>>().join(" ");
        for &i in &g.indices {
            if let Some(k) = key_of.get_mut(i as usize) {
                k.clone_from(&key);
            }
        }
    }

    for (i, key) in key_of.iter().enumerate() {
        if filter.as_ref().is_some_and(|flt| !key.to_lowercase().contains(flt.as_str())) {
            continue;
        }

        let Ok(fields) = f.fields(i) else { continue };
        let Some(Value::Matrix(m)) = fields.iter().find(|(n, _)| *n == "transform").map(|(_, v)| v.clone()) else { continue };
        let y = [m[4], m[5], m[6]];
        let n = (y[0] * y[0] + y[1] * y[1] + y[2] * y[2]).sqrt().max(1e-9);
        if tilted && (y[1] / n).abs() > 0.98 {
            continue;
        }

        let rest = fields.iter().filter(|(n, _)| *n != "transform").map(|(n, v)| match v {
            Value::F32(x) => format!("{n} {x}"),
            Value::U8(x) => format!("{n} {x}"),
            Value::U32(x) => format!("{n} {x}"),
            _ => String::new(),
        }).filter(|s| !s.is_empty()).collect::<Vec<_>>().join(", ");
        println!(
            "#{i} {key}: x ({:.2} {:.2} {:.2}) y ({:.2} {:.2} {:.2}) z ({:.2} {:.2} {:.2}) at ({:.1} {:.1} {:.1}) | {rest}",
            m[0], m[1], m[2], m[4], m[5], m[6], m[8], m[9], m[10], m[12], m[13], m[14],
        );
    }
}
