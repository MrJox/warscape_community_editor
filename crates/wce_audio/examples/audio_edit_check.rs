//! Gate of the bank edits on a game's banks (docs/plans/audio.md, step 4):
//! 1. every in-bank source points at its media (file offset and size) in the game as shipped;
//! 2. replacing a media file with its own bytes changes no byte;
//! 3. replacing it with a new sound keeps every source consistent, and the new media decodes;
//! 4. adding a variation to a random container, then removing it, gives the banks back byte for byte;
//! 5. a property set on the container reads back.
//!
//!   audio_edit_check <game folder> [mod pack...]

use std::path::PathBuf;

use wce_audio::decode::{Pcm, decode_wem};
use wce_audio::edit::EditSet;
use wce_audio::encode::{DEFAULT_QUALITY, encode_wem};
use wce_audio::library::SoundLibrary;
use wce_audio_format::bank::Bank;
use wce_audio_format::hirc::{self, Object};
use wce_vfs::{Config, GameProfile, Vfs};

/// In-bank sources whose offset or size does not match their media bank.
fn inconsistent(lib: &SoundLibrary, edits: &EditSet) -> Vec<String> {
    let mut bad = vec![];
    for b in 0..lib.banks.len() {
        for o in edits.bank(lib, b).objects() {
            let Some(Ok(object)) = hirc::parse(lib.banks[b].version, o.kind, &o.body) else { continue };
            let sources = match &object {
                Object::Sound(s) => vec![s.source.clone()],
                Object::MusicTrack(t) => t.sources.clone(),
                _ => continue,
            };
            for s in sources.iter().filter(|s| s.stream_type == 0) {
                // The source's media bank: the bank with its file ID that holds the media.
                let ok = (0..lib.banks.len()).any(|m| {
                    let bank = edits.bank(lib, m);
                    bank.header().is_some_and(|h| h.bank_id == s.file_id)
                        && bank.media_index().iter().any(|e| {
                            e.id == s.source_id
                                && s.in_memory_size == e.size
                                && s.file_offset == bank.data_position().map(|d| d as u32 + e.offset)
                        })
                });
                let held = (0..lib.banks.len()).any(|m| edits.bank(lib, m).media(s.source_id).is_some());
                if !ok && held {
                    bad.push(format!("{}: object {} source {}", lib.banks[b].name, o.id, s.source_id));
                }
            }
        }
    }
    bad
}

fn tone(seconds: f32) -> Pcm {
    let rate = 28_000;
    let samples =
        (0..(rate as f32 * seconds) as usize).map(|n| 0.3 * (std::f32::consts::TAU * 523.0 * n as f32 / rate as f32).sin()).collect();
    Pcm { channels: 1, sample_rate: rate, samples }
}

fn same_as_game(lib: &SoundLibrary, edits: &EditSet) -> bool {
    edits.banks.iter().all(|(b, bank)| bank.write() == lib.banks[*b].bank.write())
}

fn main() {
    let mut args = std::env::args().skip(1);
    let Some(game) = args.next() else {
        eprintln!("audio_edit_check <game folder> [mod pack...]");
        std::process::exit(2);
    };
    let mut config = Config::new(PathBuf::from(game), GameProfile::from_env().clone());
    config.mod_packs = Some(args.collect());
    let vfs = Vfs::mount(&config).expect("the game's packs");
    let lib = SoundLibrary::load(&vfs);
    let mut failures = vec![];

    // 1. The game as shipped.
    let shipped = inconsistent(&lib, &EditSet::default());
    println!("1. in-bank sources not pointing at their media, as shipped: {}", shipped.len());
    shipped.iter().take(5).for_each(|s| println!("   {s}"));
    if !shipped.is_empty() {
        failures.push("sources inconsistent as shipped");
    }

    // A sound held in a media bank, and a random container of in-bank sounds.
    let mut target = None;
    let mut container = None;
    for entry in &lib.banks {
        for (i, o) in entry.bank.objects().iter().enumerate() {
            match &entry.typed[i] {
                Some(Object::Sound(s)) if s.source.stream_type == 0 && target.is_none() && lib.has_media(s.source.source_id) => {
                    target = Some(s.source.source_id);
                }
                Some(Object::RandomSequence(r)) if container.is_none() && r.children.len() >= 2 && !r.playlist.is_empty() => {
                    let in_bank = r
                        .children
                        .iter()
                        .all(|c| matches!(lib.object(*c), Some((_, _, Some(Object::Sound(s)))) if s.source.stream_type == 0));
                    if in_bank {
                        container = Some(o.id);
                    }
                }
                _ => {}
            }
        }
    }
    let (Some(media), Some(container)) = (target, container) else {
        println!("no in-bank sound or random container found");
        std::process::exit(1);
    };

    // 2. Replace with the same bytes.
    let holder = (0..lib.banks.len()).find(|&b| lib.banks[b].bank.media(media).is_some()).expect("held");
    let own = lib.banks[holder].bank.media(media).expect("held").to_vec();
    let mut edits = EditSet::default();
    edits.replace_media(&lib, &vfs, media, &own).expect("replace");
    let unchanged = same_as_game(&lib, &edits);
    println!("2. media #{media} in {} replaced by itself: banks unchanged: {unchanged}", lib.banks[holder].name);
    if !unchanged {
        failures.push("a replace by the same bytes changed bytes");
    }

    // 3. Replace with a new sound.
    let wem = encode_wem(&tone(1.3), DEFAULT_QUALITY).expect("encode");
    let mut edits = EditSet::default();
    edits.replace_media(&lib, &vfs, media, &wem).expect("replace");
    let bad = inconsistent(&lib, &edits);
    let decoded = decode_wem(edits.bank(&lib, holder).media(media).expect("held")).map(|p| p.frames());
    let rereads = edits.banks.values().all(|b| Bank::read(&b.write()).is_ok_and(|r| r == *b));
    println!(
        "3. replaced by a new sound: {} banks edited, inconsistent sources {}, decodes to {decoded:?} frames, re-reads: {rereads}",
        edits.banks.len(),
        bad.len()
    );
    if !bad.is_empty() || decoded.is_err() || !rereads {
        failures.push("a replace broke the banks");
    }

    // 4. Add a variation, then remove it.
    let mut edits = EditSet::default();
    let added = edits.add_variation(&lib, container, &wem).expect("add");
    let children = match edits.object(&lib, container) {
        Some(Object::RandomSequence(r)) => r.children.len(),
        _ => 0,
    };
    let bad = inconsistent(&lib, &edits);
    let new_media = match edits.object(&lib, added) {
        Some(Object::Sound(s)) => s.source.source_id,
        _ => 0,
    };
    let plays = (0..lib.banks.len()).find_map(|b| edits.bank(&lib, b).media(new_media).map(decode_wem)).is_some_and(|r| r.is_ok());
    println!(
        "4. variation {added} added to container {container}: {children} children, inconsistent sources {}, its media decodes: {plays}",
        bad.len()
    );
    edits.remove_variation(&lib, container, added).expect("remove");
    let back = same_as_game(&lib, &edits);
    println!("   removed again: banks as the game's: {back}");
    if !bad.is_empty() || !plays || !back {
        failures.push("add/remove a variation");
    }

    // 5. A property.
    let mut edits = EditSet::default();
    edits.set_property(&lib, container, 0, Some(-3.0)).expect("set");
    let volume = edits.object(&lib, container).and_then(|o| o.node().and_then(|n| n.props.iter().find(|(p, _)| *p == 0).map(|(_, v)| *v)));
    println!("5. container volume set to -3 dB, reads back {volume:?}");
    if volume != Some(-3.0) {
        failures.push("set property");
    }

    for f in &failures {
        println!("FAILED: {f}");
    }
    std::process::exit(if failures.is_empty() { 0 } else { 1 });
}
