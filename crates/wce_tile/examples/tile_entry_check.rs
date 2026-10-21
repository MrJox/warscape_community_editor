//! Gate of Phase 0b (docs/plans/tile_ted_parity.md): the tile database entry as the root of a tile. Port of `cli/tile_entry_check.cpp`.
//!
//!   cargo run --release -p wce_tile --example tile_entry_check -- <game dir> [--all] [--time] [tile folder...]
//!
//!  1. Entry round trip: every _tile_database/TILES/*.xml of both kits parses (TileEntry) and saves back byte for byte; the typed access covers
//!     every attribute and child (unknown names: 0); every entry has variations.
//!  2. The index (TileIndex): built without a cache, with a cold cache and with the warm cache; the three give the same rows, the warm one
//!     parses nothing; every variation folder, tile.ted path and tile folder resolves to its entry; no two entries claim one location.
//!  3. Open by entry equals open by folder: for each given tile folder (--all: every kit tile that has a tile.ted and an entry)
//!     TileProject::open(folder), ::open(tile.ted) and ::open_entry(entry, variation) give the same document (same folder, entry bytes, size,
//!     heights, scene bytes).
//!  4. A tile.ted outside the kit opens with a warning and no entry (a copy of the first tile folder given).

use std::path::{Path, PathBuf};
use std::time::Instant;

use wce_formats::tile_entry::TileEntry;
use wce_formats::tile_index::TileIndex;
use wce_tile::height_field::HeightField;
use wce_tile::tile_project::{TileProject, kit_raw_root};

#[derive(Default)]
struct Gate {
    failed: usize,
}

impl Gate {
    fn check(&mut self, ok: bool, what: &str) {
        if !ok {
            self.failed += 1;
            println!("FAILED: {what}");
        }
    }
}

fn same_rows(a: &TileIndex, b: &TileIndex) -> bool {
    a.entries().len() == b.entries().len()
        && a.entries().iter().zip(b.entries()).all(|(x, y)| {
            x.name == y.name
                && x.xml == y.xml
                && x.kind == y.kind
                && x.location == y.location
                && x.key == y.key
                && x.tile_set == y.tile_set
                && x.width == y.width
                && x.height == y.height
                && x.variations == y.variations
                && x.links == y.links
                && x.targets == y.targets
                && x.size == y.size
                && x.mtime == y.mtime
                && (x.red - y.red).abs() <= 1e-4
                && (x.green - y.green).abs() <= 1e-4
                && (x.blue - y.blue).abs() <= 1e-4
        })
}

fn hash_samples(h: &HeightField) -> u64 {
    h.samples().iter().fold(1_469_598_103_934_665_603u64, |x, &s| (x ^ u64::from(s)).wrapping_mul(1_099_511_628_211))
}

/// Same document: the folder, the entry (bytes and what is read from it), the numbers, the height map and the scene. Empty when the same.
fn differ(a: &TileProject, b: &TileProject) -> String {
    if a.dir != b.dir {
        return format!("dir {} / {}", a.dir.display(), b.dir.display());
    }

    if a.entry.is_some() != b.entry.is_some() {
        return "entry present in one only".to_owned();
    }

    if let (Some(x), Some(y)) = (&a.entry, &b.entry)
        && x.save() != y.save()
    {
        return "entry bytes".to_owned();
    }

    if a.variation != b.variation {
        return "variation index".to_owned();
    }

    if a.raw_root != b.raw_root {
        return "raw root".to_owned();
    }

    let (x, y) = (&a.info, &b.info);
    if x.found != y.found
        || x.location != y.location
        || x.variation != y.variation
        || x.cells_x != y.cells_x
        || x.cells_y != y.cells_y
        || x.min_height != y.min_height
        || x.scale != y.scale
        || x.normal_strength != y.normal_strength
        || x.overlap_border_size != y.overlap_border_size
        || x.raw_data_tri_density != y.raw_data_tri_density
    {
        return "tile info".to_owned();
    }

    match (&a.height, &b.height) {
        (Some(_), None) | (None, Some(_)) => return "height map presence".to_owned(),
        (Some(h), Some(g)) if h.width() != g.width() || h.height() != g.height() || hash_samples(h) != hash_samples(g) => return "height map".to_owned(),
        _ => {}
    }

    if a.scene.doc().elements().len() != b.scene.doc().elements().len() {
        return "instance count".to_owned();
    }

    if a.scene.save() != b.scene.save() {
        return "scene bytes".to_owned();
    }

    String::new()
}

fn main() {
    let args = std::env::args().skip(1).collect::<Vec<_>>();
    let Some(game) = args.first().map(PathBuf::from) else {
        eprintln!("usage: tile_entry_check <game dir> [--all] [--time] [tile folder...]");
        std::process::exit(2);
    };

    let (mut all, mut show_time, mut tiles) = (false, false, vec![]);
    for a in &args[1..] {
        match a.as_str() {
            "--all" => all = true,
            "--time" => show_time = true,
            _ => tiles.push(PathBuf::from(a)),
        }
    }

    // The two tilemap kits (Windows install), or the one Assembly Kit that holds both (Steam on Linux).
    let roots = ["assembly_kit_battle_tilemap", "assembly_kit_campaign_tilemap", "assembly_kit"]
        .map(|kit| game.join(kit).join("raw_data"))
        .into_iter()
        .filter(|root| root.join("terrain").join("tiles").is_dir())
        .collect::<Vec<_>>();
    let mut g = Gate::default();

    // ---- 1. entry round trip
    let (mut files, mut unknown, mut no_variation, mut with_links, mut with_targets, mut variations) = (0, 0, 0, 0, 0, 0);
    for root in &roots {
        let Ok(kinds) = std::fs::read_dir(root.join("terrain").join("tiles")) else { continue };
        let mut kinds = kinds.flatten().map(|k| k.path()).collect::<Vec<_>>();
        kinds.sort();
        for kind in kinds {
            let Ok(dir) = std::fs::read_dir(kind.join("_tile_database").join("TILES")) else { continue };
            let mut entries = dir.flatten().map(|f| f.path()).filter(|p| p.extension().is_some_and(|x| x == "xml")).collect::<Vec<_>>();
            entries.sort();
            for f in entries {
                files += 1;
                let shown = f.display().to_string();
                let bytes = std::fs::read(&f).unwrap_or_default();
                match TileEntry::load(&f) {
                    Ok(e) => {
                        g.check(e.save() == bytes, &format!("round trip {shown}"));
                        for u in e.unknown_names() {
                            unknown += 1;
                            println!("unknown: {u} in {}", f.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default());
                        }

                        let v = e.variations();
                        variations += v.len();
                        no_variation += usize::from(v.is_empty());
                        with_links += usize::from(!e.links().is_empty());
                        with_targets += usize::from(!e.link_targets().is_empty());
                        g.check(e.width() > 0 && e.height() > 0 && !e.location().is_empty() && !e.tile_set().is_empty(), &format!("typed fields {shown}"));
                        g.check(e.raw_root().as_ref() == Some(root), &format!("raw_root of {shown}"));
                        g.check(!e.kind().is_empty(), &format!("kind of {shown}"));
                    }

                    Err(ex) => g.check(false, &format!("parse {shown}: {ex}")),
                }
            }
        }
    }

    println!(
        "entries: {files} files round-trip byte for byte, {unknown} unknown attributes or children, {no_variation} without variations, {variations} variations, {with_links} with links, {with_targets} with link targets"
    );
    g.check(unknown == 0 && no_variation == 0, "typed access covers every entry");

    // ---- 2. the index
    let cache = std::env::temp_dir().join("wce_tile_entry_check_index.tsv");
    let _ = std::fs::remove_file(&cache);
    let (mut rows, mut resolved, mut dups) = (0, 0, 0);
    for root in &roots {
        if !root.is_dir() {
            continue;
        }

        let t0 = Instant::now();
        let plain = TileIndex::build(root, None);
        let t_plain = t0.elapsed().as_secs_f64();

        // Writes the cache, then reads it.
        let cold = TileIndex::build(root, Some(&cache));
        let t1 = Instant::now();
        let warm = TileIndex::build(root, Some(&cache));
        let t_warm = t1.elapsed().as_secs_f64();
        let kit = root.parent().and_then(Path::file_name).map(|f| f.to_string_lossy().into_owned()).unwrap_or_default();
        g.check(plain.problems.is_empty(), &format!("{kit}: index problems"));
        g.check(cold.parsed == plain.entries().len() && cold.from_cache == 0, &format!("{kit}: cold cache parses every entry"));
        g.check(warm.parsed == 0 && warm.from_cache == plain.entries().len(), &format!("{kit}: the warm cache parses nothing"));
        g.check(same_rows(&plain, &cold) && same_rows(&plain, &warm), &format!("{kit}: plain, cold and warm index give the same rows"));
        for d in &plain.duplicate_locations {
            dups += 1;
            println!("{kit}: location claimed twice: {d}");
        }

        rows += plain.entries().len();
        if show_time {
            println!("time: {kit} index {} rows: scan {t_plain:.3} s, warm cache {t_warm:.3} s", plain.entries().len());
        }

        for e in plain.entries() {
            let Ok(entry) = TileEntry::load(&e.xml) else {
                g.check(false, &format!("{kit}: load {}", e.xml.display()));
                continue;
            };

            g.check(plain.find_location(&entry.location()).is_some(), &format!("{kit}: find_location {}", e.key));
            g.check(plain.find_name(&e.name).is_some(), &format!("{kit}: find_name {}", e.name));
            for v in 0..e.variations.len() {
                let folder = entry.variation_folder(root, v);
                let (r1, r2, r3) = (plain.resolve(&folder), plain.resolve(&folder.join("tile.ted")), plain.resolve(&entry.tile_folder(root)));
                if !plain.duplicate_locations.contains(&e.key) {
                    let shown = folder.display();
                    g.check(r1.entry.is_some_and(|x| x.xml == e.xml) && r1.variation == Some(v), &format!("{kit}: resolve folder {shown}"));
                    g.check(r2.entry.is_some_and(|x| x.xml == e.xml) && r2.variation == Some(v), &format!("{kit}: resolve tile.ted {shown}"));
                    g.check(r3.entry.is_some_and(|x| x.xml == e.xml) && r3.variation.is_none(), &format!("{kit}: resolve tile folder {shown}"));
                }

                resolved += 1;
            }
        }
    }

    let _ = std::fs::remove_file(&cache);
    println!(
        "index: {rows} rows in both kits, the plain, cold-cache and warm-cache builds agree, {resolved} variation folders resolve to their entry, {dups} locations claimed twice"
    );

    // ---- 3. open by folder / tile.ted / entry
    let mut folders = tiles;
    if all {
        for root in &roots {
            if !root.join("terrain").join("tiles").is_dir() {
                continue;
            }

            let idx = TileIndex::build(root, None);
            for e in idx.entries() {
                let Ok(entry) = TileEntry::load(&e.xml) else { continue };
                for v in 0..e.variations.len() {
                    if entry.ted_path(root, v).exists() {
                        folders.push(entry.variation_folder(root, v));
                    }
                }
            }
        }
    }

    let (mut opened, mut different, mut unopenable) = (0, 0, 0);
    let mut first_opened = None;
    let t_open = Instant::now();
    for folder in &folders {
        // Each way gives a project or an error; a tile that cannot be opened (a corrupt height map in the kit) must fail the same way through
        // all three.
        let by_folder = TileProject::open(folder).map_err(|e| e.to_string());
        let by_ted = TileProject::open(&folder.join("tile.ted")).map_err(|e| e.to_string());
        let by_entry = match &by_folder {
            Ok(p) => p.entry.as_ref().map(|en| TileProject::open_entry(en.file(), p.variation).map_err(|e| e.to_string())),
            Err(_) => kit_raw_root(folder).and_then(|root| {
                let index = TileIndex::shared(&root);
                let r = index.resolve(folder);
                r.entry.map(|en| TileProject::open_entry(&en.xml, r.variation).map_err(|e| e.to_string()))
            }),
        };

        let err_of = |r: &Option<Result<TileProject, String>>| r.as_ref().and_then(|r| r.as_ref().err().cloned()).unwrap_or_default();
        let Ok(by_folder) = by_folder else {
            let e_folder = by_folder.err().unwrap_or_default();
            let e_ted = by_ted.as_ref().err().cloned().unwrap_or_default();
            let e_entry = err_of(&by_entry);
            if by_ted.is_err() && e_folder == e_ted && e_folder == e_entry {
                unopenable += 1;
                println!("cannot be opened, the same error through all three ways: {e_folder}");
            } else {
                g.check(false, &format!("open {}: {e_folder} | {e_ted} | {e_entry}", folder.display()));
            }

            continue;
        };

        let mut d = match &by_ted {
            Ok(t) => differ(&by_folder, t),
            Err(e) => format!("tile.ted open: {e}"),
        };

        if d.is_empty() && by_folder.entry.is_some() {
            d = match &by_entry {
                Some(Ok(p)) => differ(&by_folder, p),
                other => format!("entry open: {}", err_of(other)),
            };
        }

        g.check(by_folder.has_entry(), &format!("no tile database entry for {}", folder.display()));
        if !d.is_empty() {
            different += 1;
            g.check(false, &format!("open by folder / tile.ted / entry differ in {d}: {}", folder.display()));
        }

        first_opened.get_or_insert_with(|| folder.clone());
        opened += 1;
    }

    println!("open: {opened} tiles opened by folder, by tile.ted and by entry, {different} differ, {unopenable} cannot be opened (identical errors)");
    if show_time {
        println!("time: opens {:.1} s", t_open.elapsed().as_secs_f64());
    }

    // ---- 4. outside a kit: no entry, a warning
    if let Some(first) = first_opened {
        let scratch = std::env::temp_dir().join("wce_tile_entry_check_outside");
        let _ = std::fs::remove_dir_all(&scratch);
        let _ = std::fs::create_dir_all(&scratch);
        let _ = std::fs::copy(first.join("tile.ted"), scratch.join("tile.ted"));
        if first.join("height_map_0.png").exists() {
            let _ = std::fs::copy(first.join("height_map_0.png"), scratch.join("height_map_0.png"));
        }

        match TileProject::open(&scratch) {
            Ok(p) => {
                g.check(!p.has_entry() && !p.info.found && !p.warning.is_empty(), "a tile.ted outside the kit opens with a warning and no entry");
                println!(
                    "outside a kit: opens without an entry, info.found {}, warning: {}", i32::from(p.info.found), if p.warning.is_empty() { "(none)" } else { "yes" },
                );
            }

            Err(ex) => g.check(false, &format!("open outside the kit: {ex}")),
        }

        let _ = std::fs::remove_dir_all(&scratch);
    }

    if g.failed != 0 {
        println!("{} checks FAILED", g.failed);
        std::process::exit(1);
    }

    println!("all passed");
}
