//! The built map files (BOB_Terrain's outputs, docs/re/terry.md) read and written back, byte for byte: `tile_map.index` + `.tiles`,
//! `terrain.tile_list`, `locations.blm`, `lf_height_map.data` (CHMF) and the deployment XML, from the game's packs (every folder of
//! `terrain/battles/`) and from loose map folders (a BOB scratch kit's `working_data/terrain/battles/<map>`).
//!
//!   cargo run --release -p wce_terrain_io --example map_built_roundtrip_check -- [--game <game dir> [--vanilla]] [<map folder>...]
//!
//! `$WCE_GAME_KEY` chooses the game (Attila by default). Exit code 1 when a file does not come back.

use std::collections::BTreeMap;
use std::path::PathBuf;

use wce_formats::chmf::ChmfFile;
use wce_formats::xml_doc::XmlDoc;
use wce_tile::battle_map::locations::BattleLocations;
use wce_tile::battle_map::tile_list::TileList;
use wce_tile::battle_map::tile_map::TileMap;
use wce_vfs::{Config, GameProfile, Vfs};

#[derive(Default)]
struct Totals {
    kinds: BTreeMap<&'static str, (usize, usize)>,
    failures: Vec<String>,
}

impl Totals {
    fn add(&mut self, kind: &'static str, label: &str, ok: Result<bool, String>) {
        let e = self.kinds.entry(kind).or_default();
        match ok {
            Ok(true) => e.0 += 1,
            Ok(false) => {
                e.1 += 1;
                self.failures.push(format!("{label}: written back, differs"));
            }

            Err(err) => {
                e.1 += 1;
                self.failures.push(format!("{label}: {err}"));
            }
        }
    }
}

/// Checks the files of one map folder, given as (name in the folder, bytes).
fn check(label: &str, files: &BTreeMap<String, Vec<u8>>, t: &mut Totals) {
    if let (Some(index), Some(tiles)) = (files.get("tile_map.index"), files.get("tile_map.tiles")) {
        t.add("tile_map", &format!("{label}/tile_map"), TileMap::parse(index, tiles).map(|m| m.write() == (index.clone(), tiles.clone())));
    }

    for (name, b) in files {
        let path = format!("{label}/{name}");
        let lower = name.to_ascii_lowercase();
        if lower == "terrain.tile_list" {
            t.add("tile_list", &path, TileList::parse(b).map(|l| l.write() == *b));
        } else if lower == "locations.blm" {
            t.add("locations.blm", &path, BattleLocations::parse(b).map(|l| l.write() == *b));
        } else if lower.ends_with(".data") && b.starts_with(b"CHMF") {
            t.add("chmf", &path, ChmfFile::parse(b).map(|c| c.write() == *b).map_err(|e| e.0));
        } else if lower.starts_with("deployment/") && lower.ends_with(".xml") {
            t.add("deployment xml", &path, XmlDoc::parse(b.clone()).map(|d| d.save() == *b).map_err(|e| e.to_string()));
        }
    }
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut t = Totals::default();
    let mut folders: Vec<PathBuf> = vec![];
    let mut game = None;
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--game" => {
                game = args.get(i + 1).cloned();
                i += 1;
            }

            "--vanilla" => {}
            a => folders.push(PathBuf::from(a)),
        }

        i += 1;
    }

    if let Some(g) = game {
        let mut config = Config::new(PathBuf::from(g), GameProfile::from_env().clone());
        config.use_mods = !args.iter().any(|a| a == "--vanilla");
        let vfs = Vfs::mount(&config).expect("game files");
        let mut maps: BTreeMap<String, BTreeMap<String, Vec<u8>>> = BTreeMap::new();
        for path in vfs.list("terrain/battles/", "") {
            let rest = &path["terrain/battles/".len()..];
            let Some((map, name)) = rest.split_once('/') else { continue };
            let lower = name.to_ascii_lowercase();
            let wanted = matches!(lower.as_str(), "tile_map.index" | "tile_map.tiles" | "terrain.tile_list" | "locations.blm")
                || lower.ends_with(".data")
                || (lower.starts_with("deployment/") && lower.ends_with(".xml"));
            if wanted && let Ok(b) = vfs.read(path) {
                maps.entry(map.to_owned()).or_default().insert(name.to_owned(), b);
            }
        }

        println!("packs: {} map folders", maps.len());
        for (map, files) in &maps {
            check(&format!("packs:{map}"), files, &mut t);
        }
    }

    for f in &folders {
        let mut files = BTreeMap::new();
        for e in walk(f) {
            if let Ok(rel) = e.strip_prefix(f)
                && let Ok(b) = std::fs::read(&e)
            {
                files.insert(rel.to_string_lossy().replace('\\', "/"), b);
            }
        }

        check(&f.display().to_string(), &files, &mut t);
    }

    for (k, (ok, bad)) in &t.kinds {
        println!("{k}: {ok} byte for byte, {bad} not");
    }

    for f in t.failures.iter().take(30) {
        println!("  FAIL {f}");
    }

    if !t.failures.is_empty() {
        std::process::exit(1);
    }
}

fn walk(dir: &std::path::Path) -> Vec<PathBuf> {
    let mut out = vec![];
    if let Ok(rd) = std::fs::read_dir(dir) {
        for e in rd.filter_map(Result::ok) {
            let p = e.path();
            if p.is_dir() {
                out.extend(walk(&p));
            } else {
                out.push(p);
            }
        }
    }

    out
}
