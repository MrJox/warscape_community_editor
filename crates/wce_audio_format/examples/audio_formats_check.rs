//! Gate of the sound formats: reads every copy (every pack that has it, not only the winning one) of every bank, WEM and
//! `event_data.dat` of a game and the given mod packs, writes each back and compares the bytes. Also checks each Vorbis WEM's codebook
//! hash against its setup packet. Prints a tally and the failures; exits non-zero on a failure.
//!
//!   audio_formats_check <game folder> [mod pack...]

use std::collections::BTreeMap;
use std::path::PathBuf;

use wce_audio_format::amb::Ambience;
use wce_audio_format::bank::{Bank, kind_name};
use wce_audio_format::event_data::EventData;
use wce_audio_format::triggers::Triggers;
use wce_audio_format::wem::Wem;
use wce_audio_format::{fnv, hirc};
use wce_vfs::{Config, GameProfile, Vfs};

#[derive(Default)]
struct Tally {
    read: usize,
    empty: usize,
    failed: Vec<String>,
}

impl Tally {
    fn report(&self, name: &str) -> bool {
        println!("{name}: {} read, {} empty (removals), {} failed", self.read, self.empty, self.failed.len());
        for f in self.failed.iter().take(20) {
            println!("  {f}");
        }

        self.failed.is_empty()
    }
}

/// Every (pack name, bytes) copy of a path.
fn copies<'a>(vfs: &'a Vfs, path: &'a str) -> impl Iterator<Item = (String, Vec<u8>)> + 'a {
    vfs.owners(path).iter().map(move |&pack| {
        let bytes = vfs.read_from(pack, path).unwrap_or_else(|e| panic!("{path}: {e}"));
        (vfs.packs()[pack].name.clone(), bytes)
    })
}

fn main() {
    let mut args = std::env::args().skip(1);
    let Some(game) = args.next() else {
        eprintln!("audio_formats_check <game folder> [mod pack...]");
        std::process::exit(2);
    };

    let mut config = Config::new(PathBuf::from(game), GameProfile::from_env().clone());
    config.use_mods = true;
    config.mod_packs = Some(args.collect());
    let vfs = Vfs::mount(&config).expect("the game's packs");
    println!("{} packs, {} missing mods", vfs.packs().len(), vfs.missing_mods().len());

    let mut banks = Tally::default();
    let (mut typed, mut typed_failed) = (BTreeMap::<u8, (usize, usize)>::new(), Vec::<String>::new());
    let (mut versions, mut kinds, mut chunk_tags) = (BTreeMap::new(), BTreeMap::<u8, usize>::new(), BTreeMap::<String, usize>::new());
    for path in vfs.list("", ".bnk") {
        for (pack, bytes) in copies(&vfs, path) {
            if bytes.is_empty() {
                banks.empty += 1;
                continue;
            }

            banks.read += 1;
            match Bank::read(&bytes) {
                Ok(bank) if bank.write() == bytes => {
                    let version = bank.header().map_or(0, |h| h.version);
                    *versions.entry(version).or_insert(0usize) += 1;
                    for o in bank.objects() {
                        *kinds.entry(o.kind).or_default() += 1;
                        if let Some(parsed) = hirc::parse(version, o.kind, &o.body) {
                            let tally = typed.entry(o.kind).or_default();
                            // A decision tree laid out again from its paths must come out as it was (the edits rebuild trees so).
                            let tree = match &parsed {
                                Ok(hirc::Object::DialogueEvent(d)) => Some((d.arguments.len(), &d.nodes)),
                                Ok(hirc::Object::MusicSwitch(m)) => Some((m.arguments.len(), &m.nodes)),
                                _ => None,
                            };
                            if let Some((depth, nodes)) = tree
                                && hirc::tree_from_paths(depth, &hirc::tree_all_paths(depth, nodes), nodes) != *nodes
                            {
                                typed_failed.push(format!(
                                    "{pack}: {path}: {} {}: its tree is not rebuilt as it was",
                                    kind_name(o.kind),
                                    o.id
                                ));
                            }
                            // An action's parameters read field by field (props, ranged modifiers, its type's fields).
                            if let Ok(hirc::Object::Action(a)) = &parsed
                                && a.parts().is_none()
                            {
                                typed_failed.push(format!("{pack}: {path}: Action {}: its parameters do not read", o.id));
                            }
                            match parsed {
                                Ok(object) if object.write() == o.body => tally.0 += 1,
                                Ok(_) => {
                                    tally.1 += 1;
                                    typed_failed.push(format!("{pack}: {path}: {} {}: written bytes differ", kind_name(o.kind), o.id));
                                }
                                Err(e) => {
                                    tally.1 += 1;
                                    typed_failed.push(format!("{pack}: {path}: {} {}: {e}", kind_name(o.kind), o.id));
                                }
                            }
                        }
                    }
                    for c in &bank.chunks {
                        *chunk_tags.entry(String::from_utf8_lossy(&c.tag()).into_owned()).or_default() += 1;
                    }
                }
                Ok(_) => banks.failed.push(format!("{pack}: {path}: written bytes differ")),
                Err(e) => banks.failed.push(format!("{pack}: {path}: {e}")),
            }
        }
    }

    let mut wems = Tally::default();
    let (mut codecs, mut hash_bad) = (BTreeMap::<u16, usize>::new(), vec![]);
    for path in vfs.list("", ".wem") {
        for (pack, bytes) in copies(&vfs, path) {
            if bytes.is_empty() {
                wems.empty += 1;
                continue;
            }

            wems.read += 1;
            match Wem::read(&bytes) {
                Ok(wem) if wem.write() == bytes => {
                    *codecs.entry(wem.format().map_or(0, |f| f.codec)).or_default() += 1;
                    if let (Some(h), Some(setup)) = (wem.vorbis(), wem.vorbis_setup())
                        && h.codebook_hash != fnv::bytes(setup)
                    {
                        hash_bad.push(format!("{pack}: {path}"));
                    }
                }
                Ok(_) => wems.failed.push(format!("{pack}: {path}: written bytes differ")),
                Err(e) => wems.failed.push(format!("{pack}: {path}: {e}")),
            }
        }
    }

    let mut event_data = Tally::default();
    for path in vfs.list("", ".dat").into_iter().filter(|p| p.ends_with("event_data.dat")) {
        for (pack, bytes) in copies(&vfs, path) {
            event_data.read += 1;
            match EventData::read(&bytes) {
                Ok(data) if data.write() == bytes => {
                    println!(
                        "  {pack}: {path}: {} events, {}+{} state groups, {} dialogue events, {} game parameters, {} dialogue trees",
                        data.events.len(),
                        data.state_groups.len(),
                        data.state_groups_2.len(),
                        data.dialogue_events.len(),
                        data.game_parameters.len(),
                        data.dialogue_trees.len()
                    );
                }
                Ok(_) => event_data.failed.push(format!("{pack}: {path}: written bytes differ")),
                Err(e) => event_data.failed.push(format!("{pack}: {path}: {e}")),
            }
        }
    }

    let mut ambience = Tally::default();
    for path in vfs.list("", ".amb") {
        for (pack, bytes) in copies(&vfs, path) {
            ambience.read += 1;
            match Ambience::read(&bytes) {
                Ok(amb) if amb.write() == bytes => {
                    let points: usize = amb.emitters.iter().map(|e| e.points.len()).sum();
                    println!("  {pack}: {path}: {} emitters, {points} points", amb.emitters.len());
                }
                Ok(_) => ambience.failed.push(format!("{pack}: {path}: written bytes differ")),
                Err(e) => ambience.failed.push(format!("{pack}: {path}: {e}")),
            }
        }
    }

    let mut triggers = Tally::default();
    for path in vfs.list("", ".csv").into_iter().filter(|p| p.ends_with("sound_animation_triggers.csv")) {
        for (pack, bytes) in copies(&vfs, path) {
            triggers.read += 1;
            match Triggers::read(&bytes) {
                Ok(t) if t.rows.iter().all(|r| r.len() == t.header.len()) => {
                    println!("  {pack}: {path}: {} triggers, {} columns", t.rows.len(), t.header.len());
                }
                Ok(_) => triggers.failed.push(format!("{pack}: {path}: a row's field count differs from the header's")),
                Err(e) => triggers.failed.push(format!("{pack}: {path}: {e}")),
            }
        }
    }

    println!("bank versions: {versions:?}");
    println!("chunks: {chunk_tags:?}");
    for (kind, n) in &kinds {
        let typed = typed.get(kind).map_or("raw".to_owned(), |(ok, failed)| format!("typed: {ok} ok, {failed} failed"));
        println!("  HIRC {kind:>2} {:<28} {n:>7}  {typed}", kind_name(*kind));
    }
    for f in typed_failed.iter().take(20) {
        println!("  {f}");
    }
    println!("WEM codecs: {:?}", codecs.iter().map(|(c, n)| (format!("{c:#06x}"), *n)).collect::<Vec<_>>());
    println!("Vorbis codebook hash != FNV-1 of the setup: {}", hash_bad.len());
    for f in hash_bad.iter().take(10) {
        println!("  {f}");
    }

    let ok = [
        banks.report("banks"),
        wems.report("WEMs"),
        event_data.report("event_data.dat"),
        ambience.report("campaign ambience (.amb)"),
        triggers.report("sound_animation_triggers.csv"),
    ]
    .iter()
    .all(|&ok| ok)
        && hash_bad.is_empty()
        && typed_failed.is_empty();
    std::process::exit(if ok { 0 } else { 1 });
}
