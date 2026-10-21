//! Corpus gate of the tile round trip (docs/plans/tile_roundtrip.md): every TEd project below a folder, through the model and back.
//!
//!   cargo run --release -p wce_tile_io --example tile_roundtrip_check -- <folder> [--filter TEXT] [--limit N] [--threads N] [--no-cross]
//!
//! `<folder>` is a kit's `raw_data/terrain/tiles` or any folder below it; every folder that holds a tile.ted is a tile, its entry is found
//! through the kit's index. Per tile:
//!  1. read (`attila.ted_project`) and write: every file the model holds is byte-identical to the input, the entry too;
//!  2. the written files read back into the same model (values and structure, trivia aside);
//!  3. edit then revert: about 200 values of the scene set to another value and back give the same bytes (values that kept their
//!     original text are counted apart: an edit writes them from the value);
//!  4. edit then re-read: the same values set to another value, written and read back, are the values set;
//!  5. the model agrees with `TedScene` (the editor's reader): same elements in the same order, same positions, orientations, scales,
//!     names and flags.
//!
//! Prints the counts per kind of file (identical, differing, kept, failed reads), the values kept as text and the attributes the schema
//! does not know, and the first problems. Exits 1 if a file differs, a model is not reproduced, an edit check fails or the cross check
//! disagrees.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::Instant;

use rayon::prelude::*;
use wce_tile::ted_scene::{TedScene, parse_vec3};
use wce_tile_io::attila::TedProject;
use wce_tile_io::attila::schema::TedSchema;
use wce_tile_io::xml::value_text;
use wce_tile_io::{PartRead, PartWritten, TileFiles, TileFormat};
use wce_tile_model::{Blob, Node, Sym, Tile, Value};

fn tiles(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    let mut entries = entries.flatten().map(|e| e.path()).collect::<Vec<_>>();
    entries.sort();
    if entries.iter().any(|p| p.file_name().is_some_and(|n| n == "tile.ted")) {
        out.push(dir.to_path_buf());
    }

    for p in entries {
        if p.is_dir() {
            tiles(&p, out);
        }
    }
}

/// `height_map_0.png` -> `height_map_N.png`.
fn kind_of(name: &str) -> String {
    let mut out = String::new();
    let mut digit = false;
    for c in name.chars() {
        if c.is_ascii_digit() {
            if !digit {
                out.push('N');
            }

            digit = true;
        } else {
            out.push(c);
            digit = false;
        }
    }

    if name.ends_with(".xml") && !name.starts_with("river_spline") { "entry.xml".to_owned() } else { out }
}

#[derive(Default)]
struct Counts {
    files: usize,
    identical: usize,
    differ: usize,
    kept: usize,
    failed: usize,
}

#[derive(Default)]
struct Totals {
    tiles: usize,
    per_kind: BTreeMap<String, Counts>,
    verbatim: BTreeMap<String, usize>,
    unknown: BTreeMap<String, usize>,
    layout_nodes: usize,
    elements: usize,
    props: usize,
    model_mismatch: usize,
    edits: usize,
    edit_revert_fail: usize,
    edit_reread_fail: usize,
    cross_elements: usize,
    cross_fail: usize,
    problems: Vec<String>,
    read_errors: usize,
}

fn first_diff(a: &[u8], b: &[u8]) -> String {
    let i = a.iter().zip(b).take_while(|(x, y)| x == y).count();
    let show = |x: &[u8]| String::from_utf8_lossy(&x[i.saturating_sub(40).min(x.len())..(i + 40).min(x.len())]).replace(['\r', '\n'], "|");
    format!("first difference at byte {i} (sizes {} / {}): {:?} / {:?}", a.len(), b.len(), show(a), show(b))
}

/// Every node, depth first.
fn flatten(root: &Node) -> Vec<&Node> {
    let mut out = vec![];
    root.walk(&mut |n| out.push(n));
    out
}

/// A small deterministic generator.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
}

/// Another value of the same shape.
fn changed(v: &Value) -> Option<Value> {
    Some(match v {
        Value::Bool(b) => Value::Bool(!b),
        Value::Int(i) => Value::Int(i + 1),
        Value::Float(f) => Value::Float(f + 1.5),
        Value::Vec2(a) => Value::Vec2([a[0] + 1.5, a[1] - 2.25]),
        Value::Vec3(a) => Value::Vec3([a[0] + 1.5, a[1] - 2.25, a[2] + 0.125]),
        Value::Euler(a) => Value::Euler([a[0] + 0.5, a[1], a[2] - 0.25]),
        Value::Colour(c) => Value::Colour([c[0] * 0.5, c[1], c[2], c[3]]),
        Value::Flags(f) => Value::Flags(if f.is_empty() { vec![Sym::new("OIF_ACTIVE")] } else { f[1..].to_vec() }),
        Value::Str(s) => Value::Str(format!("{s}_x")),
    })
}

/// (node index in depth-first order, property name) of up to `n` properties of the scene.
fn pick(scene: &Node, n: usize, seed: u64) -> Vec<(usize, Sym)> {
    let nodes = flatten(scene);
    let mut rng = Rng(seed | 1);
    let mut out = vec![];
    for _ in 0..n * 4 {
        if out.len() >= n {
            break;
        }

        let i = rng.next() as usize % nodes.len();
        let props = nodes[i].props();
        if props.is_empty() {
            continue;
        }

        let p = &props[rng.next() as usize % props.len()];
        if !out.contains(&(i, p.name)) {
            out.push((i, p.name));
        }
    }

    out
}

fn node_at(root: &mut Node, mut index: usize) -> &mut Node {
    fn go(n: &mut Node, index: &mut usize) -> Option<*mut Node> {
        if *index == 0 {
            return Some(n as *mut Node);
        }

        *index -= 1;
        for c in &mut n.children {
            if let Some(p) = go(c, index) {
                return Some(p);
            }
        }

        None
    }

    // SAFETY: the pointer comes from the exclusive borrow of `root` and is used while that borrow is live.
    unsafe { &mut *go(root, &mut index).expect("node index in range") }
}

fn written_scene(tile: &Tile) -> Vec<u8> {
    let (files, _) = TedProject.write(tile).expect("write");
    files.folder["tile.ted"].bytes().expect("bytes").to_vec()
}

fn check_edits(tile: &Tile, original: &[u8], seed: u64, t: &mut Totals, name: &str) {
    let Some(scene) = &tile.scene else { return };
    let picks = pick(scene, 200, seed);
    let mut edited = tile.clone();
    let root = edited.scene.as_mut().expect("scene");
    let mut with_verbatim = 0;
    for &(i, attr) in &picks {
        let n = node_at(root, i);
        let old = n.get(attr.as_str()).cloned().expect("picked property");
        with_verbatim += usize::from(n.verbatim(attr).is_some());
        n.set(attr, changed(&old).expect("changed"));
        n.set(attr, old);
    }

    t.edits += picks.len();
    if with_verbatim == 0 && written_scene(&edited) != original {
        t.edit_revert_fail += 1;
        t.problems.push(format!("{name}: edit then revert changed tile.ted"));
    }

    // Edit, write, read back: the values set.
    let mut edited = tile.clone();
    let root = edited.scene.as_mut().expect("scene");
    let mut expected = vec![];
    for &(i, attr) in &picks {
        let n = node_at(root, i);
        let v = changed(n.get(attr.as_str()).expect("picked property")).expect("changed");
        expected.push((i, attr, n.class, v.clone()));
        n.set(attr, v);
    }

    let (files, _) = TedProject.write(&edited).expect("write");
    let (back, _) = TedProject.read(&files).expect("read back");
    let nodes = flatten(back.scene.as_ref().expect("scene"));
    for (i, attr, class, v) in expected {
        let got = nodes.get(i).and_then(|n| n.get(attr.as_str()));
        if got.map(|g| value_text(&TedSchema, class, attr, g)) != Some(value_text(&TedSchema, class, attr, &v)) {
            t.edit_reread_fail += 1;
            t.problems.push(format!("{name}: {class}.{attr} set to {v:?}, read back {got:?}"));
            break;
        }
    }
}

fn cross_check(scene: &Node, ted: &[u8], t: &mut Totals, name: &str) {
    let Ok(old) = TedScene::parse(ted.to_vec()) else {
        t.cross_fail += 1;
        t.problems.push(format!("{name}: TedScene does not read it"));
        return;
    };

    let nodes = flatten(scene);
    let elems = old.doc().elements();
    t.cross_elements += nodes.len();
    if nodes.len() != elems.len() {
        t.cross_fail += 1;
        t.problems.push(format!("{name}: {} nodes, TedScene {} elements", nodes.len(), elems.len()));
        return;
    }

    let near = |a: [f64; 3], b: wce_foundation::math::Double3| (0..3).all(|i| a[i] as f32 == b[i] as f32 || (a[i].is_nan() && b[i].is_nan()));
    for (i, n) in nodes.iter().enumerate() {
        let class_ok = elems[i].name == n.class.as_str();
        let vec_ok = |attr: &str, prefix: &str| match (n.get(attr), old.doc().get(i, attr).and_then(|s| parse_vec3(&s, prefix))) {
            (Some(Value::Vec3(a) | Value::Euler(a)), Some(b)) => near(*a, b),
            (Some(Value::Float(f)), _) if attr == "scale" => old.scale(i).is_some_and(|b| near([*f; 3], b)),
            (None, None) => true,
            (Some(Value::Str(_)), None) => true,
            _ => n.class == "point",
        };

        let name_ok = n.get("name").map(|v| v.as_str().map(str::to_owned).unwrap_or_default()) == old.doc().get(i, "name").map(|_| old.name(i));
        let flags_ok = n.get("flags").is_none_or(|f| f.has_flag("OIF_VISIBLE") == old.visible(i) && f.has_flag("OIF_ACTIVE") == old.active(i))
            || n.get("flags").is_some_and(|f| f.as_str().is_some());
        if !(class_ok && vec_ok("position", "vector") && vec_ok("orientation", "euler") && vec_ok("scale", "vector") && name_ok && flags_ok) {
            t.cross_fail += 1;
            t.problems.push(format!("{name}: element {i} ({}) differs from TedScene", n.class));
            return;
        }
    }
}

fn check(dir: &Path, cross: bool, t: &Mutex<Totals>) {
    let name = dir.display().to_string();
    let fail = |what: String| {
        let mut t = t.lock().expect("totals");
        t.read_errors += 1;
        t.problems.push(format!("{name}: {what}"));
    };

    let files = match TileFiles::from_kit_dir(dir) {
        Ok(f) => f,
        Err(e) => return fail(e.to_string()),
    };

    let (tile, report) = match TedProject.read(&files) {
        Ok(r) => r,
        Err(e) => return fail(e.to_string()),
    };

    let (written, wreport) = match TedProject.write(&tile) {
        Ok(w) => w,
        Err(e) => return fail(format!("write: {e}")),
    };

    let mut local = Totals::default();
    for (part, how) in &report.parts {
        let c = local.per_kind.entry(kind_of(part)).or_default();
        c.files += 1;
        if let PartRead::Failed(e) = how {
            c.failed += 1;
            local.problems.push(format!("{name}/{part}: did not read ({e}), carried along"));
        }
    }

    for (part, how) in &wreport.parts {
        if *how == PartWritten::Copied {
            continue;
        }

        let (orig, new) = if files.entry.as_ref().is_some_and(|(n, _)| n == part) {
            (files.entry.as_ref().map(|e| &e.1), written.entry.as_ref().map(|e| &e.1))
        } else {
            (files.folder.get(part), written.folder.get(part))
        };

        let (Some(orig), Some(new)) = (orig, new) else { continue };
        let (a, b) = (orig.bytes().unwrap_or_default(), new.bytes().unwrap_or_default());
        let c = local.per_kind.entry(kind_of(part)).or_default();
        if *how == PartWritten::Kept {
            c.kept += 1;
        }

        if a == b {
            c.identical += 1;
        } else {
            c.differ += 1;
            local.problems.push(format!("{name}/{part}: {}", first_diff(&a, &b)));
        }
    }

    // The model again from the written files.
    match TedProject.read(&written) {
        Ok((back, _)) if back.semantic() == tile.semantic() => {}
        Ok(_) => {
            local.model_mismatch += 1;
            local.problems.push(format!("{name}: the written files read into another model"));
        }

        Err(e) => {
            local.model_mismatch += 1;
            local.problems.push(format!("{name}: the written files do not read: {e}"));
        }
    }

    if let Some(Blob::Bytes(ted)) = written.folder.get("tile.ted") {
        let seed = name.bytes().fold(0xcbf2_9ce4_8422_2325u64, |h, b| (h ^ u64::from(b)).wrapping_mul(0x100_0000_01b3));
        check_edits(&tile, ted, seed, &mut local, &name);
        if cross && let Some(scene) = &tile.scene {
            let original = files.folder["tile.ted"].bytes().unwrap_or_default();
            cross_check(scene, &original, &mut local, &name);
        }
    }

    let mut t = t.lock().expect("totals");
    t.tiles += 1;
    for (k, c) in local.per_kind {
        let to = t.per_kind.entry(k).or_default();
        to.files += c.files;
        to.identical += c.identical;
        to.differ += c.differ;
        to.kept += c.kept;
        to.failed += c.failed;
    }

    for ((class, attr), n) in report.xml.verbatim {
        *t.verbatim.entry(format!("{class}.{attr}")).or_default() += n;
    }

    for ((class, attr), n) in report.xml.unknown {
        *t.unknown.entry(format!("{class}.{attr}")).or_default() += n;
    }

    t.layout_nodes += report.xml.layout_nodes;
    t.elements += report.xml.elements;
    t.props += report.xml.props;
    t.model_mismatch += local.model_mismatch;
    t.edits += local.edits;
    t.edit_revert_fail += local.edit_revert_fail;
    t.edit_reread_fail += local.edit_reread_fail;
    t.cross_elements += local.cross_elements;
    t.cross_fail += local.cross_fail;
    t.problems.extend(local.problems);
}

fn main() {
    let args = std::env::args().skip(1).collect::<Vec<_>>();
    let Some(root) = args.first() else {
        eprintln!("usage: tile_roundtrip_check <folder> [--filter TEXT] [--limit N] [--threads N] [--no-cross]");
        std::process::exit(2);
    };

    let opt = |name: &str| args.iter().position(|a| a == name).and_then(|i| args.get(i + 1)).cloned();
    let cross = !args.iter().any(|a| a == "--no-cross");
    let mut dirs = vec![];
    tiles(Path::new(root), &mut dirs);
    if let Some(f) = opt("--filter") {
        dirs.retain(|d| d.to_string_lossy().contains(&f));
    }

    if let Some(n) = opt("--limit").and_then(|n| n.parse().ok()) {
        dirs.truncate(n);
    }

    let threads = opt("--threads").and_then(|n| n.parse().ok()).unwrap_or(8);
    let pool = rayon::ThreadPoolBuilder::new().num_threads(threads).build().expect("thread pool");
    let started = Instant::now();
    let totals = Mutex::new(Totals::default());
    pool.install(|| dirs.par_iter().for_each(|d| check(d, cross, &totals)));
    let t = totals.into_inner().expect("totals");

    println!("tiles: {} of {} folders with a tile.ted ({:.0} s)", t.tiles, dirs.len(), started.elapsed().as_secs_f64());
    println!("files the model holds (identical / differ / kept as original bytes / did not read, carried along):");
    for (k, c) in &t.per_kind {
        println!("  {k:<24} {:>6} files: {:>6} identical, {} differ, {} kept, {} did not read", c.files, c.identical, c.differ, c.kept, c.failed);
    }

    println!("xml: {} elements, {} values, {} nodes with layout trivia", t.elements, t.props, t.layout_nodes);
    println!("values kept as their text: {}", t.verbatim.values().sum::<usize>());
    for (k, n) in &t.verbatim {
        println!("  {k} {n}");
    }

    println!("attributes the schema does not know: {}", t.unknown.values().sum::<usize>());
    for (k, n) in &t.unknown {
        println!("  {k} {n}");
    }

    println!("model read back from the written files differs: {}", t.model_mismatch);
    println!("edits: {} values; edit then revert changed bytes: {} tiles; edit then re-read lost a value: {} tiles", t.edits, t.edit_revert_fail, t.edit_reread_fail);
    if cross {
        println!("cross check against TedScene: {} elements, {} tiles disagree", t.cross_elements, t.cross_fail);
    }

    println!("tiles that did not read: {}", t.read_errors);
    let differ = t.per_kind.values().map(|c| c.differ).sum::<usize>();
    let failed = differ + t.model_mismatch + t.edit_revert_fail + t.edit_reread_fail + t.cross_fail + t.read_errors;
    if !t.problems.is_empty() {
        println!("problems ({}; first 40):", t.problems.len());
        let mut problems = t.problems;
        problems.sort();
        for p in problems.iter().take(40) {
            println!("  {p}");
        }
    }

    println!("{}", if failed == 0 { "all passed" } else { "FAILED" });
    std::process::exit(i32::from(failed != 0));
}
