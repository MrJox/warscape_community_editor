//! Gate of the music edits on a game's music (docs/plans/audio.md, step 6), on the first playlist with nested groups (5 items or more):
//! 1. a new entry (a 3 s sound) after its first leaf: the item tree stays whole, the new segment and track read back, the segment's
//!    duration and exit cue fit the sound, its audio decodes;
//! 2. removing it again gives the banks back byte for byte;
//! 3. a segment's audio replaced: duration and exit cue fit;
//! 4. an item's weight and loop read back.
//!
//!   audio_music_check <game folder> [mod pack...]

use std::path::PathBuf;

use wce_audio::decode::{Pcm, decode_wem};
use wce_audio::edit::EditSet;
use wce_audio::encode::{DEFAULT_QUALITY, encode_wem};
use wce_audio::library::SoundLibrary;
use wce_audio::music::EXIT_CUE;
use wce_audio_format::bank::Bank;
use wce_audio_format::hirc::{Object, PlaylistItem};
use wce_vfs::{Config, GameProfile, Vfs};

/// True when the depth-first item list is one whole tree (every group's count met, nothing left over).
fn whole(items: &[PlaylistItem]) -> bool {
    fn walk(items: &[PlaylistItem], at: &mut usize) -> bool {
        let Some(item) = items.get(*at) else { return false };
        *at += 1;
        (0..item.children).all(|_| walk(items, at))
    }
    let mut at = 0;
    walk(items, &mut at) && at == items.len()
}

fn main() {
    let mut args = std::env::args().skip(1);
    let Some(game) = args.next() else {
        eprintln!("audio_music_check <game folder> [mod pack...]");
        std::process::exit(2);
    };
    let mut config = Config::new(PathBuf::from(game), GameProfile::from_env().clone());
    config.mod_packs = Some(args.collect());
    let vfs = Vfs::mount(&config).expect("the game's packs");
    let lib = SoundLibrary::load(&vfs);
    let mut failures = vec![];

    let (playlist_id, leaf) = lib
        .banks
        .iter()
        .flat_map(|b| b.bank.objects().iter().zip(&b.typed))
        .find_map(|(o, t)| match t {
            Some(Object::MusicPlaylist(p)) if p.items.len() >= 5 => {
                p.items.iter().find(|i| i.children == 0 && i.segment_id != 0).map(|i| (o.id, *i))
            }
            _ => None,
        })
        .expect("a playlist with a segment");
    let items_before = match lib.object(playlist_id) {
        Some((_, _, Some(Object::MusicPlaylist(p)))) => p.items.len(),
        _ => 0,
    };
    println!("playlist {playlist_id} ({}), {items_before} items, entry after item {}", lib.label(playlist_id, 13), leaf.item_id);

    let rate = 44_100u32;
    let tone: Vec<f32> = (0..rate * 3).map(|n| 0.3 * (std::f32::consts::TAU * 330.0 * n as f32 / rate as f32).sin()).collect();
    let wem = encode_wem(&Pcm { channels: 2, sample_rate: rate, samples: tone.iter().flat_map(|s| [*s, *s]).collect() }, DEFAULT_QUALITY)
        .expect("encode");

    // 1. Add.
    let mut edits = EditSet::default();
    let segment_id = edits.add_playlist_entry(&lib, playlist_id, leaf.item_id, &wem).expect("add");
    let Some(Object::MusicPlaylist(p)) = edits.object(&lib, playlist_id) else { panic!("the playlist reads back") };
    let Some(Object::MusicSegment(seg)) = edits.object(&lib, segment_id) else { panic!("the segment reads back") };
    let track_ok = matches!(edits.object(&lib, seg.base.children[0]), Some(Object::MusicTrack(t)) if t.node.parent_id == segment_id
        && t.clips.first().is_some_and(|c| (c.source_duration - 3000.0).abs() < 1.0));
    let exit = seg.markers.iter().find(|m| m.id == EXIT_CUE).map(|m| m.position);
    let media = match edits.object(&lib, seg.base.children[0]) {
        Some(Object::MusicTrack(t)) => t.sources[0].source_id,
        _ => 0,
    };
    let decodes = edits.loose.get(&format!("audio/{media}.wem")).is_some_and(|w| decode_wem(w).is_ok());
    let rereads = edits.banks.values().all(|b| Bank::read(&b.write()).is_ok_and(|r| r == *b));
    let ok = whole(&p.items)
        && p.items.len() == items_before + 1
        && p.base.children.contains(&segment_id)
        && track_ok
        && exit == Some(seg.duration)
        && decodes
        && rereads;
    println!(
        "1. entry {segment_id} added: {} items, tree whole: {}, segment {:.0} ms (exit cue {exit:?}), track fits: {track_ok}, decodes: {decodes}, re-reads: {rereads}",
        p.items.len(),
        whole(&p.items),
        seg.duration
    );
    if !ok {
        failures.push("add a playlist entry");
    }

    // 2. Remove it again.
    let new_item = p.items.iter().find(|i| i.segment_id == segment_id).map(|i| i.item_id).expect("the new item");
    edits.remove_playlist_entry(&lib, playlist_id, new_item).expect("remove");
    let back = edits.banks.iter().all(|(b, bank)| bank.write() == lib.banks[*b].bank.write());
    println!("2. removed again: banks as the game's: {back}");
    if !back {
        failures.push("remove a playlist entry");
    }

    // 3. Replace a segment's audio.
    let mut edits = EditSet::default();
    edits.replace_segment_audio(&lib, leaf.segment_id, &wem).expect("replace");
    let Some(Object::MusicSegment(seg)) = edits.object(&lib, leaf.segment_id) else { panic!("the segment reads back") };
    let entry = seg.markers.iter().find(|m| m.id == wce_audio::music::ENTRY_CUE).map_or(0.0, |m| m.position);
    let fits = (seg.duration - entry - 3000.0).abs() < 1.0
        && seg.markers.iter().find(|m| m.id == EXIT_CUE).map(|m| m.position) == Some(seg.duration);
    println!("3. segment {} replaced: {:.0} ms with entry at {entry:.0}: fits: {fits}", leaf.segment_id, seg.duration);
    if !fits {
        failures.push("replace a segment's audio");
    }

    // 4. Weight and loop.
    let mut edits = EditSet::default();
    edits.set_playlist_item(&lib, playlist_id, leaf.item_id, 20_000, 2).expect("set");
    let read = match edits.object(&lib, playlist_id) {
        Some(Object::MusicPlaylist(p)) => p.items.iter().find(|i| i.item_id == leaf.item_id).map(|i| (i.weight, i.loop_count)),
        _ => None,
    };
    println!("4. item {} weight and loop read back {read:?}", leaf.item_id);
    if read != Some((20_000, 2)) {
        failures.push("weight and loop");
    }

    // 5. A transition rule's exit and entry, and a stinger added then removed.
    let mut edits = EditSet::default();
    let ruled = lib.banks.iter().flat_map(|b| b.bank.objects().iter().zip(&b.typed)).find_map(|(o, t)| match t {
        Some(Object::MusicPlaylist(p)) if !p.rules.is_empty() => Some((o.id, p.rules[0].clone())),
        _ => None,
    });
    let rule_ok = ruled.is_some_and(|(id, mut rule)| {
        rule.source_rule.fade.time = 1234;
        rule.source_rule.sync = 2;
        rule.destination_rule.entry = 1;
        edits.set_transition_rule(&lib, id, 0, rule.clone()).is_ok()
            && matches!(edits.object(&lib, id), Some(Object::MusicPlaylist(p)) if p.rules[0] == rule)
    });
    let mut stinger_ok = false;
    if let Some(Object::MusicPlaylist(p)) = edits.object(&lib, playlist_id) {
        let mut edits = EditSet::default();
        let before = p.base.stingers.clone();
        let mut more = before.clone();
        more.push(wce_audio_format::hirc::Stinger {
            trigger_id: wce_audio_format::fnv::id("wce_test_trigger"),
            segment_id: leaf.segment_id,
            sync_play_at: 2,
            cue_filter_hash: 0,
            dont_repeat_time: 5000,
            segment_look_ahead: 0,
        });
        let added = edits.set_stingers(&lib, playlist_id, more.clone()).is_ok()
            && matches!(edits.object(&lib, playlist_id), Some(Object::MusicPlaylist(q)) if q.base.stingers == more);
        let removed = edits.set_stingers(&lib, playlist_id, before).is_ok()
            && edits.banks.iter().all(|(b, bank)| bank.write() == lib.banks[*b].bank.write());
        stinger_ok = added && removed;
    }
    println!(
        "5. a transition rule's fade, sync and entry read back: {rule_ok}; a stinger added, then removed (banks as the game's): {stinger_ok}"
    );
    if !rule_ok || !stinger_ok {
        failures.push("transition rule and stinger");
    }

    // 6. A new playlist like this one, with one new segment, played for a music switch's path.
    let mut edits = EditSet::default();
    let new_playlist = edits.new_playlist_like(&lib, playlist_id, &wem);
    let shape = new_playlist.as_ref().ok().and_then(|id| match edits.object(&lib, *id) {
        Some(Object::MusicPlaylist(p)) if p.items.len() == 2 && p.items[0].children == 1 && p.items[1].segment_id != 0 => {
            let Some(Object::MusicSegment(s)) = edits.object(&lib, p.items[1].segment_id) else { return None };
            let Some(Object::MusicTrack(t)) = edits.object(&lib, s.base.children[0]) else { return None };
            let media = t.sources[0].source_id;
            edits.loose.get(&format!("audio/{media}.wem")).and_then(|w| decode_wem(w).ok()).map(|_| *id)
        }
        _ => None,
    });
    let switch = lib.banks.iter().flat_map(|b| b.bank.objects().iter().zip(&b.typed)).find_map(|(o, t)| match t {
        Some(Object::MusicSwitch(m)) => {
            wce_audio_format::hirc::tree_paths(m.arguments.len(), &m.nodes).first().map(|(k, _)| (o.id, k.clone()))
        }
        _ => None,
    });
    let pointed = match (shape, switch) {
        (Some(p), Some((s, keys))) => edits.set_music_path(&lib, s, &keys, p).is_ok(),
        _ => false,
    };
    println!(
        "6. a new playlist like #{playlist_id}: {new_playlist:?}, one segment whose audio decodes: {}; a switch path plays it: {pointed}",
        shape.is_some()
    );
    if shape.is_none() || !pointed {
        failures.push("new playlist");
    }

    for f in &failures {
        println!("FAILED: {f}");
    }
    std::process::exit(if failures.is_empty() { 0 } else { 1 });
}
