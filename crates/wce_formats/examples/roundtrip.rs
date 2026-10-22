//! Round-trip gate over the corpus manifest (scripts/corpus_inventory.py). Port of `cli/roundtrip.cpp` with its format registry
//! (`libs/formats/registry.cpp` and the register_* functions): the editor's own formats from wce_formats, the game's from rpfm_lib.
//!
//!   cargo run --release -p wce_formats --example roundtrip -- <manifest.tsv> <kits_parent_dir> [--format NAME] [--max-size MB] [--limit N]
//!       [--all-failures]
//!
//! For every in-scope manifest row whose file a registered format claims: parse, write back, compare byte for byte. Prints one line per format
//! (files, passed, failed, skipped for size) and the first failures. Files no format claims are counted per extension so unimplemented
//! formats stay visible. Parse errors carry each reader's own message, so the text of a failure line may differ from the C++ one.

use std::collections::BTreeMap;
use std::io::{Cursor, Read};
use std::path::PathBuf;

use rpfm_lib::files::bmd::Bmd;
use rpfm_lib::files::fastbin_lists::{CivilianDeploymentFile, CivilianShelterFile, GrassListFile, TreeListFile};
use rpfm_lib::files::tile_database::TileDatabase;
use rpfm_lib::files::{Decodeable, Encodeable};
use wce_formats::chmf::ChmfFile;
use wce_formats::image_containers::{DdsFile, TgaFile, TiffFile};
use wce_formats::markers::MarkersFile;
use wce_formats::png_file::PngFile;
use wce_formats::xml_doc::XmlDoc;

/// What a round trip gave.
struct RoundTrip {
    ok: bool,

    /// Parse failure or first difference.
    error: String,
}

/// First differing offset message for two byte strings.
fn describe_diff(a: &[u8], b: &[u8]) -> String {
    let i = a.iter().zip(b).take_while(|(x, y)| x == y).count();
    format!("first difference at byte {i}, sizes {}/{}", a.len(), b.len())
}

/// Compares what a writer produced with the input.
fn compare(bytes: &[u8], written: Result<Vec<u8>, String>) -> RoundTrip {
    match written {
        Ok(out) if out == bytes => RoundTrip { ok: true, error: String::new() },
        Ok(out) => RoundTrip { ok: false, error: describe_diff(bytes, &out) },
        Err(e) => RoundTrip { ok: false, error: format!("parse: {e}") },
    }
}

/// decode + encode with rpfm_lib.
fn rpfm<T: Decodeable + Encodeable>(bytes: &[u8]) -> Result<Vec<u8>, String> {
    let mut v = T::decode(&mut Cursor::new(bytes), &None).map_err(|e| e.to_string())?;
    let mut out = vec![];
    v.encode(&mut out, &None).map_err(|e| e.to_string())?;
    Ok(out)
}

type Claims = Box<dyn Fn(&str, &[u8]) -> bool>;
type Trip = Box<dyn Fn(&[u8]) -> RoundTrip>;

struct Format {
    name: &'static str,
    claims: Claims,
    round_trip: Trip,
}

fn lower_ext(path: &str) -> String {
    let name = path.rsplit(['/', '\\']).next().unwrap_or(path);
    name.rfind('.').map(|d| name[d..].to_ascii_lowercase()).unwrap_or_default()
}

fn ends_with_ci(s: &str, suffix: &str) -> bool {
    s.len() >= suffix.len() && s.as_bytes()[s.len() - suffix.len()..].eq_ignore_ascii_case(suffix.as_bytes())
}

const FASTBIN: &[u8; 8] = b"FASTBIN0";

// ---- CHMF and .markers: their framings (wce_formats::chmf, wce_formats::markers), the data kept as bytes.

fn chmf_round_trip(b: &[u8]) -> Result<Vec<u8>, String> {
    ChmfFile::parse(b).map(|f| f.write()).map_err(|e| e.0)
}

fn markers_round_trip(b: &[u8]) -> Result<Vec<u8>, String> {
    MarkersFile::parse(b).map(|f| f.write()).map_err(|e| e.0)
}

fn dds_round_trip(bytes: &[u8]) -> RoundTrip {
    let mut d = match DdsFile::parse(bytes) {
        Ok(d) => d,
        Err(e) => return RoundTrip { ok: false, error: format!("parse: {}", e.0) },
    };

    if d.flags() == 0 && d.fourcc() == 114 && d.pixel_format_flags() == 4 {
        // TEd/BOB single-channel float texture: the header must be the generated one.
        if !d.is_r32f_template() {
            return RoundTrip { ok: false, error: "R32F header differs from the generated one".to_owned() };
        }

        d = DdsFile::make_r32f(d.width(), d.height(), d.payload.clone());
    }

    compare(bytes, Ok(d.write()))
}

fn tiff_round_trip(bytes: &[u8]) -> RoundTrip {
    let mut t = match TiffFile::parse(bytes) {
        Ok(t) => t,
        Err(e) => return RoundTrip { ok: false, error: format!("parse: {}", e.0) },
    };

    if t.ted_layout {
        // TEd/BOB layout: the header must be the generated one, so a writer needs only the pixels.
        if !t.header_is_regenerable() {
            return RoundTrip { ok: false, error: "TEd-layout header differs from the generated one".to_owned() };
        }

        t.header = TiffFile::ted_header(t.width, t.height);
    }

    compare(bytes, Ok(t.write()))
}

/// The gate checks the kit-encoder reproduction (decode, then encode), not just pass-through.
fn png_round_trip(bytes: &[u8]) -> RoundTrip {
    let p = match PngFile::parse(bytes) {
        Ok(p) => p,
        Err(e) => return RoundTrip { ok: false, error: format!("parse: {}", e.0) },
    };

    match p.encode() {
        Ok(out) if out == bytes => RoundTrip { ok: true, error: String::new() },
        Ok(out) => RoundTrip {
            ok: false, error: format!("{}{}", if p.has_ancillary { "ancillary chunks (not kit encoder); " } else { "encoder differs; " }, describe_diff(bytes, &out)),
        },

        Err(e) => RoundTrip { ok: false, error: format!("parse: {}", e.0) },
    }
}

fn identity(name: &'static str, claims: impl Fn(&str, &[u8]) -> bool + 'static) -> Format {
    Format { name, claims: Box::new(claims), round_trip: Box::new(|_| RoundTrip { ok: true, error: String::new() }) }
}

fn simple(name: &'static str, claims: impl Fn(&str, &[u8]) -> bool + 'static, trip: fn(&[u8]) -> Result<Vec<u8>, String>) -> Format {
    Format { name, claims: Box::new(claims), round_trip: Box::new(move |b| compare(b, trip(b))) }
}

/// The formats in the C++ registration order (the first that claims a file takes it).
fn formats() -> Vec<Format> {
    let fastbin = |head: &[u8]| head.starts_with(FASTBIN);
    let mut f = vec![];

    // All tile XML: .xml and tile.ted, through the lossless XmlDoc. It must look like text XML: optional BOM / whitespace, then '<'.
    f.push(simple(
        "xml",
        |path, head| {
            if !(ends_with_ci(path, ".xml") || ends_with_ci(path, ".ted")) {
                return false;
            }

            if head.starts_with(&[0xFF, 0xFE]) {
                return true;
            }

            let mut i = if head.starts_with(&[0xEF, 0xBB, 0xBF]) { 3 } else { 0 };
            while i < head.len() && matches!(head[i], b' ' | b'\t' | b'\r' | b'\n') {
                i += 1;
            }

            i < head.len() && head[i] == b'<'
        },
        |b| XmlDoc::parse(b.to_vec()).map(|d| d.save()).map_err(|e| e.to_string()),
    ));

    f.push(simple("tree_list_bin", move |p, h| p.ends_with(".tree_list.bin") && fastbin(h), rpfm::<TreeListFile>));
    f.push(simple("grass_list_bin", move |p, h| p.ends_with("grass_list.bin") && fastbin(h), rpfm::<GrassListFile>));
    f.push(simple("civilian_deployment", move |p, h| p.ends_with("civilian_deployment.bin") && fastbin(h), rpfm::<CivilianDeploymentFile>));
    f.push(simple("civilian_shelter", move |p, h| p.ends_with("civilian_shelter.bin") && fastbin(h), rpfm::<CivilianShelterFile>));
    f.push(simple(
        "bmd_bin",
        move |p, h| (p.ends_with("_procedural_bmd_data.bin") || (p.contains("bmd_data_escalation") && p.ends_with(".bin"))) && fastbin(h),
        rpfm::<Bmd>,
    ));
    f.push(simple("markers", |p, _| p.ends_with(".markers"), markers_round_trip));
    f.push(Format { name: "dds", claims: Box::new(|p, h| ends_with_ci(p, ".dds") && h.starts_with(b"DDS ")), round_trip: Box::new(dds_round_trip) });
    f.push(simple("tga", |p, _| ends_with_ci(p, ".tga"), |b| TgaFile::parse(b).map(|t| t.write()).map_err(|e| e.0)));
    f.push(Format { name: "tiff", claims: Box::new(|p, h| ends_with_ci(p, ".tif") && h.starts_with(b"II*\0")), round_trip: Box::new(tiff_round_trip) });
    f.push(simple("chmf", |_, h| h.starts_with(b"CHMF"), chmf_round_trip));
    f.push(Format {
        name: "png",
        claims: Box::new(|p, h| (p.ends_with(".png") || p.ends_with(".PNG")) && p.len() > 4 && h.len() >= 8 && h[1] == b'P'),
        round_trip: Box::new(png_round_trip),
    });

    // Files the editor carries through unchanged: authoring leftovers, text not interpreted yet, binaries checked by magic only.
    f.push(identity("opaque_authoring", |p, _| [".jpg", ".max", ".psd", ".session", ".tmd", ".tmp", ".editor", ".outlines", ".cs2"].contains(&lower_ext(p).as_str())));
    f.push(identity("opaque_text", |p, _| [".agf", ".bob", ".txt"].contains(&lower_ext(p).as_str())));
    f.push(identity("rmv2_magic", |p, h| lower_ext(p) == ".rigid_model_v2" && h.starts_with(b"RMV2")));
    f.push(identity("opaque_binary", |p, _| {
        let ext = lower_ext(p);
        ext == ".tree_list" || ext.is_empty()
    }));

    // tile_database.bin: a structural read of the whole file (the builder is checked by tdb_build).
    f.push(Format {
        name: "tile_database_bin",
        claims: Box::new(move |p, h| p.ends_with("tile_database.bin") && fastbin(h)),
        round_trip: Box::new(|b| match TileDatabase::decode(&mut Cursor::new(b), &None) {
            Ok(db) if !db.tiles().is_empty() => RoundTrip { ok: true, error: String::new() },
            Ok(_) => RoundTrip { ok: false, error: "no tiles found".to_owned() },
            Err(e) => RoundTrip { ok: false, error: format!("walk: {e}") },
        }),
    });

    f
}

fn read_head(p: &std::path::Path, max: u64) -> Vec<u8> {
    let mut v = vec![];
    if let Ok(f) = std::fs::File::open(p) {
        let _ = f.take(max).read_to_end(&mut v);
    }

    v
}

#[derive(Default)]
struct Stat {
    files: usize,
    ok: usize,
    failed: usize,
    skipped: usize,
    failures: Vec<String>,
}

fn main() {
    let args = std::env::args().skip(1).collect::<Vec<_>>();
    if args.len() < 2 {
        eprintln!("usage: roundtrip <manifest.tsv> <kits_parent_dir> [--format NAME] [--max-size MB] [--limit N]");
        std::process::exit(2);
    }

    let (mut only_format, mut max_mb, mut limit, mut max_failures) = (String::new(), 64.0, usize::MAX, 25usize);
    let mut i = 2;
    while i < args.len() {
        let more = i + 1 < args.len();
        match args[i].as_str() {
            "--format" if more => {
                i += 1;
                only_format = args[i].clone();
            }

            "--max-size" if more => {
                i += 1;
                max_mb = wce_formats::c_num::strtod_prefix(&args[i]).map_or(0.0, |(v, _)| v);
            }

            "--limit" if more => {
                i += 1;
                limit = args[i].parse().unwrap_or(0);
            }

            "--all-failures" => max_failures = usize::MAX,
            _ => {}
        }

        i += 1;
    }

    let formats = formats();
    let mut stats: BTreeMap<&str, Stat> = BTreeMap::new();
    let mut unclaimed: BTreeMap<String, usize> = BTreeMap::new();
    let manifest = std::fs::read_to_string(&args[0]).unwrap_or_default();
    let parent = PathBuf::from(&args[1]);
    let mut seen = 0;
    for line in manifest.lines().skip(1) {
        let c = line.split('\t').collect::<Vec<_>>();
        if c.len() < 12 || c[11] != "in" {
            continue;
        }

        let p = parent.join(c[0]).join(c[1]).join(c[2]);
        let head = read_head(&p, 4096);
        let Some(f) = formats.iter().find(|f| (f.claims)(c[2], &head)) else {
            *unclaimed.entry(if c[5].is_empty() { "(none)".to_owned() } else { c[5].to_owned() }).or_default() += 1;
            continue;
        };

        if !only_format.is_empty() && f.name != only_format {
            continue;
        }

        if seen >= limit {
            break;
        }

        seen += 1;
        let s = stats.entry(f.name).or_default();
        s.files += 1;
        if c[3].parse::<f64>().unwrap_or(0.0) > max_mb * 1_048_576.0 {
            s.skipped += 1;
            continue;
        }

        let r = (f.round_trip)(&std::fs::read(&p).unwrap_or_default());
        if r.ok {
            s.ok += 1;
        } else {
            s.failed += 1;
            if s.failures.len() < max_failures {
                s.failures.push(format!("{}/{}/{}: {}", c[0], c[1], c[2], r.error));
            }
        }
    }

    let mut rc = 0;
    for (name, s) in &stats {
        println!("{name:<12} files {:7}  ok {:7}  failed {:5}  skipped(size) {:4}", s.files, s.ok, s.failed, s.skipped);
        for f in &s.failures {
            println!("    FAIL {f}");
        }

        if s.failed != 0 {
            rc = 1;
        }
    }

    print!("files no registered format claims, by extension:");
    for (ext, n) in &unclaimed {
        print!(" {ext}={n}");
    }

    println!();
    std::process::exit(rc);
}
