//! Reads every unit file of a game (and its enabled mods) with the `wce_formats` parsers: clips, `.meta`s (which must encode back to their
//! bytes), variant mesh definitions (saved back unchanged, with every element and attribute known) and the animation tables. Prints what
//! failed and what it found, and exits non-zero on a failure. A definition that isn't well-formed XML is listed but is no failure: TDD ships
//! 11 (doubled quotes, stray end tags), and the loader skips them.
//!
//!   unit_formats_check <game folder> [mod pack...]

use std::collections::BTreeMap;
use std::path::PathBuf;

use wce_formats::anim::Animation;
use wce_formats::anim_meta::Meta;
use wce_formats::anim_tables::{parse_fragment, parse_tables};
use wce_formats::variant_mesh::VariantMeshFile;
use wce_vfs::{Config, GameProfile, Vfs};

#[derive(Default)]
struct Tally {
    read: usize,
    failed: Vec<String>,
}

impl Tally {
    fn add(&mut self, path: &str, ok: bool) {
        self.read += 1;
        if !ok {
            self.failed.push(path.to_owned());
        }
    }

    fn report(&self, name: &str) -> bool {
        println!("{name}: {} read, {} failed", self.read, self.failed.len());
        for f in self.failed.iter().take(20) {
            println!("  {f}");
        }

        self.failed.is_empty()
    }
}

fn main() {
    let mut args = std::env::args().skip(1);
    let Some(game) = args.next() else {
        eprintln!("unit_formats_check <game folder> [mod pack...]");
        std::process::exit(2);
    };

    let mut config = Config::new(PathBuf::from(game), GameProfile::from_env().clone());
    config.use_mods = true;
    config.mod_packs = Some(args.collect());
    let vfs = Vfs::mount(&config).expect("the game's packs");
    println!("{} packs, {} missing mods", vfs.packs().len(), vfs.missing_mods().len());

    let (mut clips, mut versions) = (Tally::default(), BTreeMap::<u32, usize>::new());
    let mut untouched = 0usize;
    for path in vfs.list("", ".anim") {
        let bytes = vfs.read(path).expect("readable");
        if let Some(v) = bytes.get(..4) {
            *versions.entry(u32::from_le_bytes([v[0], v[1], v[2], v[3]])).or_default() += 1;
        }

        let a = Animation::decode(&bytes, usize::MAX);
        untouched += a.as_ref().is_some_and(|a| a.frames.is_empty()) as usize;
        clips.add(path, a.is_some());
    }

    let (mut metas, mut kinds) = (Tally::default(), BTreeMap::<String, usize>::new());
    for path in vfs.list("", ".meta") {
        let bytes = vfs.read(path).expect("readable");
        let meta = Meta::decode(&bytes);
        for e in meta.iter().flat_map(|m| &m.entries) {
            *kinds.entry(e.kind.clone()).or_default() += 1;
        }

        // A file too short for a header holds nothing (a few buildings'): meta_effects treats it as no effects.
        metas.add(path, bytes.len() < 8 || meta.is_some_and(|m| m.encode() == bytes));
    }

    let (mut meshes, mut names) = (Tally::default(), BTreeMap::<String, usize>::new());
    let mut repaired = 0;
    for path in vfs.list("", ".variantmeshdefinition") {
        let bytes = vfs.read(path).expect("readable");
        match VariantMeshFile::parse(bytes.clone()) {
            Ok(f) => {
                for e in f.doc.elements() {
                    *names.entry(e.name.clone()).or_default() += 1;
                    for a in &e.attrs {
                        *names.entry(format!("{}@{}", e.name, a.name)).or_default() += 1;
                    }
                }

                // Nothing but what isn't XML is changed, and what is has a tree to show for it.
                let walked = f.root.slots.iter().any(|s| !s.alternatives.is_empty()) || f.root.model.is_some();
                repaired += usize::from(f.repaired);
                meshes.add(path, if f.repaired { walked } else { f.doc.save() == bytes });
            }

            Err(e) => meshes.add(&format!("{path}: {e}"), false),
        }
    }

    let mut fragments = Tally::default();
    let mut slots = BTreeMap::<String, usize>::new();
    for path in vfs.list("animations/animation_fragments/", ".txt") {
        let text = String::from_utf8_lossy(&vfs.read(path).expect("readable")).into_owned();
        let f = parse_fragment(&text);
        for e in f.iter().flat_map(|f| &f.entries) {
            *slots.entry(e.slot.clone()).or_default() += 1;
        }

        fragments.add(path, f.is_some());
    }

    let mut tables = Tally::default();
    let mut dangling = 0;
    for path in vfs.list("animations/animation_tables/", ".txt") {
        let text = String::from_utf8_lossy(&vfs.read(path).expect("readable")).into_owned();
        let t = parse_tables(&text);
        for table in t.iter().flatten() {
            dangling += table.fragments.iter().filter(|f| vfs.owner(&format!("animations/animation_fragments/{f}.txt")).is_none()).count();
        }

        println!("{path}: {} tables", t.as_ref().map_or(0, Vec::len));
        tables.add(path, t.is_some());
    }

    let ok = [clips.report("clips"), metas.report("metas"), meshes.report("variant meshes"), fragments.report("fragments"), tables.report("tables")];
    println!("clip versions {versions:?}, {untouched} with no frames, {dangling} table fragments without a file");
    println!("meta kinds {kinds:?}");
    println!("{} slots", slots.len());
    println!("variant meshes repaired, not being XML: {repaired}");
    println!("variant mesh elements and attributes {names:?}");
    if !ok.iter().all(|o| *o) {
        std::process::exit(1);
    }
}
