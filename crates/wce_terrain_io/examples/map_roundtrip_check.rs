//! The map round trip (docs/plans/terrain_editor.md, Phase 1) over every map folder of the given kits (`<raw_data>/terrain/battles/*` and
//! `<raw_data>/terrain/campaigns/*`), in memory:
//!
//! 1. read then write, nothing edited: every file byte for byte;
//! 2. every decoded file made again from the model: the LF TIFFs, explicit tiles, `.terry` and XML byte for byte; the PNGs byte for byte
//!    when the kit's encoder wrote them, else (Photoshop's) the same pixels, the other chunks kept;
//! 3. a minimal edit: one LF sample changed changes its two bytes only; one cell painted changes only the PNG's IDAT chunks.
//!
//!   cargo run --release -p wce_terrain_io --example map_roundtrip_check -- <raw_data>... [--filter <substring>]
//!
//! Exit code 1 when a check fails.

use std::path::{Path, PathBuf};
use std::time::Instant;

use wce_formats::png_file::PngFile;
use wce_terrain::{LfRole, MapKind, MapModel};
use wce_terrain_io::{MapFiles, attila};

#[derive(Default)]
struct Totals {
    maps: usize,
    files: usize,
    failures: Vec<String>,
    png_exact: usize,
    png_pixels: usize,
    tiff: usize,
    text: usize,
    edits: usize,
}

fn bytes(b: &wce_tile_model::Blob) -> Vec<u8> {
    b.bytes().map(|a| a.to_vec()).unwrap_or_default()
}

/// The chunks of a PNG other than IDAT, in order.
fn non_idat_chunks(b: &[u8]) -> Vec<Vec<u8>> {
    let mut out = vec![];
    let mut i = 8;
    while i + 12 <= b.len() {
        let n = u32::from_be_bytes([b[i], b[i + 1], b[i + 2], b[i + 3]]) as usize;
        if i + 12 + n > b.len() {
            break;
        }

        if &b[i + 4..i + 8] != b"IDAT" {
            out.push(b[i..i + 12 + n].to_vec());
        }

        i += 12 + n;
    }

    out
}

fn check_map(dir: &Path, kind: MapKind, t: &mut Totals) {
    let name = dir.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let label = format!("{}/{name}", if kind == MapKind::Battle { "battles" } else { "campaigns" });
    let fail = |t: &mut Totals, what: String| {
        println!("  FAIL {label}: {what}");
        t.failures.push(format!("{label}: {what}"));
    };

    let start = Instant::now();
    let files = match MapFiles::from_dir(dir) {
        Ok(f) => f,
        Err(e) => return fail(t, format!("list: {e}")),
    };

    let mut m: MapModel = match attila::read(&name, kind, &files) {
        Ok(m) => m,
        Err(e) => return fail(t, format!("read: {e}")),
    };

    let read_ms = start.elapsed().as_millis();
    t.maps += 1;
    t.files += files.folder.len();

    // 1. Unedited.
    match attila::write(&m) {
        Ok(out) => {
            if out.folder.len() != files.folder.len() {
                fail(t, format!("{} files written, {} read", out.folder.len(), files.folder.len()));
            }

            for (k, v) in &files.folder {
                if out.folder.get(k).map(bytes) != Some(bytes(v)) {
                    fail(t, format!("unedited {k} differs"));
                }
            }
        }

        Err(e) => fail(t, format!("write: {e}")),
    }

    // 2. Everything made again.
    for (_, l) in &mut m.layers {
        l.edit();
    }

    for (_, f) in &mut m.lf {
        f.edit();
    }

    for x in &mut m.xml {
        x.edit();
    }

    if let Some(e) = &mut m.explicit {
        e.edit();
    }

    if let Some(p) = &mut m.project {
        p.edit();
    }

    let out = match attila::write(&m) {
        Ok(o) => o,
        Err(e) => return fail(t, format!("write again: {e}")),
    };

    for (_, l) in &m.layers {
        let (a, b) = (bytes(&files.folder[&l.path]), bytes(&out.folder[&l.path]));
        if a == b {
            t.png_exact += 1;
            continue;
        }

        let (pa, pb) = (PngFile::parse(&a), PngFile::parse(&b));
        match (pa, pb) {
            (Ok(pa), Ok(pb)) if pa.pixels == pb.pixels && non_idat_chunks(&a) == non_idat_chunks(&b) => t.png_pixels += 1,
            _ => fail(t, format!("{}: made again, its pixels or chunks differ", l.path)),
        }
    }

    for (_, f) in &m.lf {
        if bytes(&files.folder[&f.path]) == bytes(&out.folder[&f.path]) {
            t.tiff += 1;
        } else {
            fail(t, format!("{}: made again, differs", f.path));
        }
    }

    let texts = m
        .xml
        .iter()
        .map(|x| x.path.clone())
        .chain(m.explicit.iter().map(|e| e.path.clone()))
        .chain(m.project.iter().map(|p| p.path.clone()));
    for path in texts.collect::<Vec<_>>() {
        if bytes(&files.folder[&path]) == bytes(&out.folder[&path]) {
            t.text += 1;
        } else {
            fail(t, format!("{path}: made again, differs"));
        }
    }

    // 3. Minimal edits.
    let mut m = match attila::read(&name, kind, &files) {
        Ok(m) => m,
        Err(e) => return fail(t, format!("read again: {e}")),
    };

    if let Some(f) = m.lf_mut(LfRole::Land) {
        let (x, y) = (f.get().width() / 2, f.get().height() / 2);
        let v = f.get().at(x, y);
        f.edit().set(x, y, v ^ 0x0101);
        let path = f.path.clone();
        match attila::write(&m) {
            Ok(out) => {
                let (a, b) = (bytes(&files.folder[&path]), bytes(&out.folder[&path]));
                let diff = a.iter().zip(&b).filter(|(p, q)| p != q).count();
                if a.len() == b.len() && diff == 2 {
                    t.edits += 1;
                } else {
                    fail(t, format!("{path}: one sample edited, {diff} bytes differ (sizes {} / {})", a.len(), b.len()));
                }
            }

            Err(e) => fail(t, format!("{path}: edit: {e}")),
        }
    }

    if let Some((_, l)) = m.layers.first_mut() {
        let c = l.get().colour(0, 0);
        l.edit().paint(0, 0, [c[0] ^ 1, c[1], c[2], c[3]]);
        let path = l.path.clone();
        match attila::write(&m) {
            Ok(out) => {
                let (a, b) = (bytes(&files.folder[&path]), bytes(&out.folder[&path]));
                let pb = PngFile::parse(&b);
                if non_idat_chunks(&a) == non_idat_chunks(&b) && pb.is_ok_and(|p| p.pixels[0] == c[0] ^ 1) {
                    t.edits += 1;
                } else {
                    fail(t, format!("{path}: one cell painted, other chunks changed or the cell did not"));
                }
            }

            Err(e) => fail(t, format!("{path}: edit: {e}")),
        }
    }

    println!(
        "{label}: {} files, {} layers, {} LF, explicit {}, terry {}, {} xml, {} carried; {:.1} MB decoded; read {read_ms} ms, all {} ms",
        files.folder.len(),
        m.layers.len(),
        m.lf.len(),
        m.explicit.is_some(),
        m.project.as_ref().map_or(0, |p| p.get().entities.len()),
        m.xml.len(),
        m.carried.len(),
        m.decoded_bytes() as f64 / 1e6,
        start.elapsed().as_millis()
    );
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let filter = args.iter().position(|a| a == "--filter").and_then(|i| args.get(i + 1)).cloned().unwrap_or_default();
    let roots: Vec<PathBuf> = args
        .iter()
        .enumerate()
        .filter(|(i, a)| !a.starts_with("--") && (*i == 0 || args[i - 1] != "--filter"))
        .map(|(_, a)| PathBuf::from(a))
        .collect();
    if roots.is_empty() {
        eprintln!("map_roundtrip_check <raw_data>... [--filter <substring>]");
        std::process::exit(2);
    }

    let mut t = Totals::default();
    for root in &roots {
        for (sub, kind) in [("battles", MapKind::Battle), ("campaigns", MapKind::Campaign)] {
            let Ok(rd) = std::fs::read_dir(root.join("terrain").join(sub)) else { continue };
            let mut dirs: Vec<PathBuf> = rd.filter_map(Result::ok).map(|e| e.path()).filter(|p| p.is_dir()).collect();
            dirs.sort();
            for d in dirs {
                if d.to_string_lossy().contains(&filter) {
                    check_map(&d, kind, &mut t);
                }
            }
        }
    }

    println!(
        "{} maps, {} files; made again: {} PNGs byte for byte, {} the same pixels (another encoder), {} TIFFs, {} text files; {} minimal edits; {} failures",
        t.maps,
        t.files,
        t.png_exact,
        t.png_pixels,
        t.tiff,
        t.text,
        t.edits,
        t.failures.len()
    );

    if !t.failures.is_empty() {
        std::process::exit(1);
    }
}
