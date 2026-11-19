//! Loads every bank of a game (and the given mod packs) into a `SoundLibrary` and reports how readable it is: names found per
//! source, events and objects with a name, and what resolving every event reaches (sounds, media found, missing). Prints the tree of
//! a few events. Exits non-zero if a bank fails to load. Missing media (sounds whose media is in no bank and no pack: 195 sound
//! objects in Attila's own data, silent in the game too) are reported, not failures.
//!
//!   audio_library_check <game folder> [mod pack...]

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::time::Instant;

use wce_audio::library::{MediaRef, PlayNode, SoundLibrary};
use wce_vfs::{Config, GameProfile, Vfs};

fn walk(node: &PlayNode, sounds: &mut usize, media: &mut BTreeMap<&'static str, usize>) {
    if let Some(m) = &node.media {
        *sounds += 1;
        *media
            .entry(match m {
                MediaRef::InBank { .. } => "in a bank",
                MediaRef::Loose { .. } => "loose",
                MediaRef::Missing { .. } => "missing",
            })
            .or_default() += 1;
    }
    node.children.iter().for_each(|c| walk(c, sounds, media));
}

fn print(node: &PlayNode, depth: usize, lines: &mut usize) {
    if *lines > 25 {
        return;
    }
    *lines += 1;
    let media = node.media.as_ref().map(|m| format!("  [{m:?}]")).unwrap_or_default();
    println!("    {}{}{media}", "  ".repeat(depth), node.label);
    node.children.iter().for_each(|c| print(c, depth + 1, lines));
}

fn main() {
    let mut args = std::env::args().skip(1);
    let Some(game) = args.next() else {
        eprintln!("audio_library_check <game folder> [mod pack...]");
        std::process::exit(2);
    };

    let mut config = Config::new(PathBuf::from(game), GameProfile::from_env().clone());
    config.use_mods = true;
    config.mod_packs = Some(args.collect());
    let vfs = Vfs::mount(&config).expect("the game's packs");

    let t = Instant::now();
    let lib = SoundLibrary::load(&vfs);
    let objects: usize = lib.banks.iter().map(|b| b.bank.objects().len()).sum();
    println!("{} banks, {objects} objects, {} names, loaded in {:.1} s", lib.banks.len(), lib.names.len(), t.elapsed().as_secs_f64());
    for (path, why) in &lib.failures {
        println!("  failed: {path}: {why}");
    }

    let (mut named, mut by_kind) = (0usize, BTreeMap::<u8, (usize, usize)>::new());
    for b in &lib.banks {
        for o in b.bank.objects() {
            let e = by_kind.entry(o.kind).or_default();
            e.0 += 1;
            if lib.names.get(o.id).is_some() {
                named += 1;
                e.1 += 1;
            }
        }
    }
    println!("objects with a name: {named} of {objects}");
    for (kind, (n, with)) in &by_kind {
        println!("  {:<28} {with:>6} of {n:>6} named", wce_audio_format::bank::kind_name(*kind));
    }

    let t = Instant::now();
    let (mut events, mut sounds, mut media, mut silent) = (0usize, 0usize, BTreeMap::new(), 0usize);
    let mut samples = vec![];
    for b in &lib.banks {
        for o in b.bank.objects().iter().filter(|o| o.kind == 4) {
            events += 1;
            let tree = lib.resolve(o.id, &vfs);
            let before = sounds;
            walk(&tree, &mut sounds, &mut media);
            if sounds == before {
                silent += 1;
            }
            if samples.len() < 3 && sounds - before >= 2 && lib.names.get(o.id).is_some() {
                samples.push(tree);
            }
        }
    }
    println!(
        "{events} events resolved in {:.1} s: {sounds} sound leaves {media:?}; {silent} events reach no sound (stop, set state...)",
        t.elapsed().as_secs_f64()
    );
    for tree in &samples {
        let mut lines = 0;
        print(tree, 0, &mut lines);
    }

    std::process::exit(if lib.failures.is_empty() { 0 } else { 1 });
}
