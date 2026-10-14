//! Checks the battle weather selection data (warscape_asset_variation_db.bin / .xml). Port of `cli/variation_check.cpp`.
//!
//!   cargo run --release -p wce_weather --example variation_check        (the game folder: game_dir in wce.ini or WCE_GAME)
//!
//! Gates, for every pack that holds the database (vanilla data.pack and the TDD weather pack ship both a .bin and an .xml):
//!   * decode(bin) encodes to the .bin byte for byte;
//!   * from_xml(xml) equals decode(bin) (names, values, filenames, flags);
//!   * from_xml(xml) encodes to the .bin;
//!   * decode(bin).to_xml() equals the .xml, or differs only by blank lines (reported).
//!
//! Gates on the editing operations (on the database the game uses): adding a value to a category and removing it again gives back the same
//! database, and while the new value is not queried every one of the condition combinations keeps its candidate list.
//!
//! Report: referenced files missing from every pack, variations that can never match, conditions (every combination of the category values)
//! for which the game finds no environment file even after the "default_map" retry, and weather/*.environment files no variation refers to.

use std::collections::{BTreeMap, BTreeSet};
use std::io::Cursor;
use std::path::PathBuf;

use rpfm_lib::files::asset_variation_db::{AssetVariationDb, Variation, VariationQuery};
use rpfm_lib::files::{Decodeable, Encodeable};
use wce_foundation::settings::Settings;
use wce_vfs::{Config, GameProfile, Vfs, normalize};

const BIN: &str = "warscape_asset_variation_db/warscape_asset_variation_db.bin";
const XML: &str = "warscape_asset_variation_db/warscape_asset_variation_db.xml";

fn decode(bytes: &[u8]) -> Result<AssetVariationDb, String> {
    AssetVariationDb::decode(&mut Cursor::new(bytes), &None).map_err(|e| e.to_string())
}

fn encode(db: &AssetVariationDb) -> Vec<u8> {
    let mut out = vec![];
    let _ = db.clone().encode(&mut out, &None);
    out
}

fn from_xml(bytes: &[u8]) -> Result<AssetVariationDb, String> {
    AssetVariationDb::from_xml(&String::from_utf8_lossy(bytes)).map_err(|e| e.to_string())
}

fn without_blank_lines(t: &str) -> String {
    let mut out = String::new();
    let mut rest = t;
    loop {
        let (line, next) = match rest.find("\r\n") {
            Some(nl) => (&rest[..nl], Some(&rest[nl + 2..])),
            None => (rest, None),
        };

        if !line.is_empty() {
            out += line;
            out += "\r\n";
        }

        match next {
            Some(n) if !n.is_empty() => rest = n,
            _ => break,
        }
    }

    out
}

/// Pack-by-pack file gates; the number of failures.
fn check_pack_files(vfs: &Vfs) -> Result<i32, String> {
    let mut bad = 0;
    for &pack in vfs.owners(BIN) {
        let name = &vfs.packs()[pack].name;
        let Ok(bin) = vfs.read_from(pack, BIN) else { continue };
        let from_bin = decode(&bin)?;
        if encode(&from_bin) != bin {
            println!("{name}: parse(bin).to_bytes() differs from the .bin");
            bad += 1;
        }

        let Ok(xml) = vfs.read_from(pack, XML) else {
            println!("{name}: bin writer identical ({} bytes); no .xml in this pack", bin.len());
            continue;
        };

        let from_x = from_xml(&xml)?;
        let same = from_x == from_bin;
        if !same {
            println!("{name}: the .xml and the .bin hold different data");
            bad += 1;
        }

        if encode(&from_x) != bin {
            println!("{name}: parse_xml(xml).to_bytes() differs from the .bin");
            bad += 1;
        }

        let text = String::from_utf8_lossy(&xml).into_owned();
        let written = from_bin.to_xml();
        let xml_state = if written == text {
            "identical"
        } else if without_blank_lines(&text) == written {
            "identical apart from blank lines in the original"
        } else {
            "DIFFERENT"
        };

        if xml_state == "DIFFERENT" {
            bad += 1;
        }

        println!(
            "{name}: bin {} bytes (writer identical), xml {} bytes (data {}, to_xml {xml_state}, xml->bin identical)", bin.len(), xml.len(),
            if same { "equal to the bin" } else { "DIFFERENT" },
        );
        for problem in from_bin.validate() {
            println!("  validate: {problem}");
        }
    }

    Ok(bad)
}

/// Every combination of the values of all categories of `names_from`, in the C++ order (the first category counts fastest).
fn for_each_combination(names_from: &AssetVariationDb, mut f: impl FnMut(&VariationQuery, &[usize])) {
    let cats = names_from.categories();
    let mut idx = vec![0usize; cats.len()];
    loop {
        let q = cats.iter().zip(&idx).map(|(c, &i)| (c.name().clone(), c.values()[i].clone())).collect::<VariationQuery>();
        f(&q, &idx);
        let mut d = 0;
        while d < idx.len() {
            idx[d] += 1;
            if idx[d] == cats[d].values().len() {
                idx[d] = 0;
                d += 1;
            } else {
                break;
            }
        }

        if d == idx.len() {
            break;
        }
    }
}

fn candidate_lists(db: &AssetVariationDb, names_from: &AssetVariationDb) -> Vec<Vec<String>> {
    let mut all = vec![];
    for_each_combination(names_from, |q, _| all.push(db.matches("environment", q).variations.iter().map(|v| v.filename().clone()).collect()));
    all
}

/// Editing gates on the database the game uses; the number of failures.
fn check_editing(original: &AssetVariationDb) -> i32 {
    let mut bad = 0;
    let before = candidate_lists(original, original);
    for m in original.categories() {
        let mut edited = original.clone();
        let full = original.value_count() >= 32;
        let added = edited.add_value(m.name(), "__test_value__");
        if full {
            if added.is_ok() {
                println!("add_value succeeded with all 32 bits in use ({})", m.name());
                bad += 1;
            }

            continue;
        }

        if let Err(why) = added {
            println!("add_value({}) failed: {why}", m.name());
            bad += 1;
            continue;
        }

        if candidate_lists(&edited, original) != before {
            println!("add_value({}) changed the candidates of existing conditions", m.name());
            bad += 1;
        }

        if let Err(why) = edited.remove_value(m.name(), "__test_value__") {
            println!("remove_value({}) failed: {why}", m.name());
            bad += 1;
            continue;
        }

        if edited != *original {
            println!("add_value + remove_value({}) did not restore the database", m.name());
            bad += 1;
        }
    }

    // Removing a value that is not the last of its category must keep every other condition's candidates.
    for m in original.categories() {
        if m.values().len() < 2 {
            continue;
        }

        let mut edited = original.clone();
        let victim = m.values()[0].clone();

        // Refusing is the documented behaviour when a variation would become unconstrained.
        if edited.remove_value(m.name(), &victim).is_err() {
            continue;
        }

        let after = candidate_lists(&edited, &edited);

        // The original restricted to conditions that do not use the removed value.
        let mut expected = vec![];
        for_each_combination(original, |q, _| {
            if q.get(m.name()) != Some(&victim) {
                expected.push(original.matches("environment", q).variations.iter().map(|v| v.filename().clone()).collect::<Vec<_>>());
            }
        });

        if after != expected {
            println!("remove_value({}={victim}) changed the candidates of the remaining conditions", m.name());
            bad += 1;
        }
    }

    println!("editing gates: {bad} failures");
    bad
}

fn run() -> Result<i32, String> {
    let game = Settings::load(&Settings::default_dir()).get("game_dir", Some("WCE_GAME")).ok_or("the game folder is not set (game_dir in wce.ini or WCE_GAME)")?;
    let vfs = Vfs::mount(&Config::new(PathBuf::from(game), GameProfile::ATTILA)).map_err(|e| e.to_string())?;
    let Some(owner) = vfs.owner(BIN) else {
        println!("{BIN} is in no pack");
        return Ok(1);
    };

    let db = decode(&vfs.read(BIN).map_err(|e| e.to_string())?)?;
    let Some(env) = db.entry("environment") else {
        println!("no 'environment' entry");
        return Ok(1);
    };

    let bits = db.value_count();
    println!(
        "{BIN} from {}: {} metas ({bits} values, {} of 32 bits), {} variations", vfs.packs()[owner].name, db.categories().len(), bits.min(32),
        env.variations().len(),
    );
    let mut rc = check_pack_files(&vfs)?;
    rc |= check_editing(&db);

    let mut referenced = BTreeSet::new();
    let (mut missing, mut impossible) = (0, 0);
    for v in env.variations() {
        let p = normalize(v.filename());
        if vfs.owner(&p).is_none() {
            println!("MISSING file: {}", v.filename());
            missing += 1;
        }

        referenced.insert(p);
        if let Some((cat, _)) = db.allowed_values(*v.flags()).into_iter().find(|(_, allowed)| allowed.is_empty()) {
            println!("NEVER matches (no {cat} allowed): {}", v.filename());
            impossible += 1;
        }
    }

    // Self-consistency: the query made of each variation's own first allowed value per category must find it.
    let mut self_bad = 0;
    for v in env.variations() {
        let q = db.allowed_values(*v.flags()).into_iter().filter_map(|(cat, allowed)| allowed.first().map(|a| (cat.to_owned(), (*a).to_owned()))).collect();
        let r = db.matches("environment", &q);
        if !r.variations.iter().any(|&c| std::ptr::eq::<Variation>(c, v)) {
            println!("NOT found by its own conditions: {}", v.filename());
            self_bad += 1;
        }
    }

    println!("self-consistency: {} variations, {self_bad} not found by their own conditions", env.variations().len());
    if self_bad != 0 {
        rc = 1;
    }

    // Every combination of the values of all categories.
    let (mut combos, mut empty, mut fallback) = (0, 0, 0);
    let mut uses: BTreeMap<String, usize> = BTreeMap::new();

    // "category=value" -> (combinations, without a file)
    let mut by_value: BTreeMap<String, (usize, usize)> = BTreeMap::new();
    for_each_combination(&db, |q, _| {
        let r = db.matches("environment", q);
        combos += 1;
        for (cat, value) in q.iter() {
            let c = by_value.entry(format!("{cat}={value}")).or_default();
            c.0 += 1;
            if r.variations.is_empty() {
                c.1 += 1;
            }
        }

        if r.variations.is_empty() {
            empty += 1;
        } else {
            if r.used_default_map {
                fallback += 1;
            }

            for c in &r.variations {
                *uses.entry(normalize(c.filename())).or_default() += 1;
            }
        }
    });

    println!("{combos} condition combinations: {empty} find no file, {fallback} need the default_map retry");
    println!("combinations without a file, by value (a hole is data, not a bug: the database has no variation for it):");
    for (k, (n, holes)) in &by_value {
        println!("  {k:<40} {holes:>5} / {n}");
    }

    let mut unused = 0;
    for f in vfs.list("weather/", ".environment") {
        if !referenced.contains(f) {
            unused += 1;
            if unused <= 10 {
                println!("not referenced by any variation: {f}");
            }
        }
    }

    println!(
        "{unused} .environment files under weather/ are not referenced; {} referenced files never selectable by any condition",
        referenced.iter().filter(|f| !uses.contains_key(*f)).count(),
    );
    println!("missing files {missing}, never-matching variations {impossible}");
    Ok(if rc != 0 { rc } else if missing != 0 || impossible != 0 { 2 } else { 0 })
}

fn main() {
    match run() {
        Ok(code) => std::process::exit(code),
        Err(e) => {
            println!("error: {e}");
            std::process::exit(1);
        }
    }
}
