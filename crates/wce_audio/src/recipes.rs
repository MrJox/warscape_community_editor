//! Recipes (docs/plans/audio.md, step 8): one action doing every edit a common job needs, built from the edit operations.
//!
//! - Music: the game's playlists named by the events that play them ([`music_playlists`]); adding or replacing a track is
//!   `EditSet::add_playlist_entry` / `replace_segment_audio`.
//! - A voice line: the sounds an event plays (Audio Object's Replace audio).
//! - An animation sound ([`animation_sound`]): a frame in a `.snd.meta`, firing an existing event, or a new sound made like another
//!   event's (a new sound, a new event playing it, a trigger row firing the event).
//! - A culture's voice lines ([`culture_lines`]): what a dialogue event plays for one value of a state group (e.g. `Roman` of
//!   `vo_battle_subculture`, whose values are the DB's voice-over cultures), to replace them. A new culture (a new branch in every dialogue tree, and the units' `voiceover`) is a
//!   follow-up.

use std::collections::BTreeMap;

use wce_audio_format::fnv;
use wce_audio_format::hirc::{self, Object};
use wce_audio_format::triggers::Triggers;
use wce_formats::anim_meta::Meta;
use wce_vfs::files::Files;

use crate::edit::EditSet;
use crate::library::{PlayNode, SoundLibrary};
use crate::{AudioError, Result, anim_sounds};

/// The game's music playlists, each named by how the music switches above it choose it: the state values along the switch tree's
/// paths (e.g. `Hunnic / Battle / Threat`), else the playlist's own label. Sorted by name, named ones first.
/// A music state group in plain words for the music list.
pub fn music_group_word(group: &str) -> String {
    match group.to_lowercase().as_str() {
        "music_culture" => "culture",
        "game_mode" => "mode",
        "campaign_music_state" => "campaign",
        "battle_intensity" => "intensity",
        "battle_state" => "battle",
        "battle_result_state" => "result",
        "music_key" => "key",
        "music_dynamic" => "moment",
        other => return other.to_owned(),
    }
    .to_owned()
}

pub fn music_playlists(lib: &SoundLibrary, files: &dyn Files) -> Vec<(u32, Vec<String>)> {
    let _ = files;
    let mut by_playlist: BTreeMap<u32, Vec<String>> = BTreeMap::new();
    // Every playlist, and the switch paths reaching it (a switch's paths name its children; nested switches add their own).
    let mut parent_path: BTreeMap<u32, String> = BTreeMap::new();
    let mut switches = vec![];
    for b in &lib.banks {
        for (o, t) in b.bank.objects().iter().zip(&b.typed) {
            match t {
                Some(Object::MusicPlaylist(_)) => {
                    by_playlist.entry(o.id).or_default();
                }
                Some(Object::MusicSwitch(s)) => switches.push((o.id, s.clone())),
                _ => {}
            }
        }
    }
    // Switches in the tree's order: resolve a switch's own path from its parent's first.
    for _ in 0..4 {
        for (id, s) in &switches {
            let own = parent_path.get(id).cloned().unwrap_or_default();
            for (keys, child) in hirc::tree_paths(s.arguments.len(), &s.nodes) {
                // Each state with its group in plain words: "culture Barbarian", "key Bb" (0: any, left out).
                let names: Vec<String> = keys
                    .iter()
                    .zip(&s.arguments)
                    .filter(|(k, _)| **k != 0)
                    .map(|(k, (g, _))| format!("{} {}", music_group_word(&lib.label(*g, 0)), lib.label(*k, 0)))
                    .collect();
                // A nested switch repeats a state of its parent's path: once.
                let mut parts: Vec<String> = own.split(", ").filter(|p| !p.is_empty()).map(str::to_owned).collect();
                for n in names {
                    if !parts.contains(&n) {
                        parts.push(n);
                    }
                }
                let path = parts.join(", ");
                parent_path.entry(child).or_insert_with(|| path.clone());
                let shown = if path.is_empty() { "any situation".to_owned() } else { path };
                if let Some(list) = by_playlist.get_mut(&child)
                    && !list.contains(&shown)
                {
                    list.push(shown);
                }
            }
        }
    }
    // A playlist no switch chooses is played by an event directly: named by it.
    let mut played_by: BTreeMap<u32, String> = BTreeMap::new();
    for b in &lib.banks {
        for (o, t) in b.bank.objects().iter().zip(&b.typed) {
            if let Some(Object::Event(e)) = t {
                for a in &e.actions {
                    if let Some((_, _, Some(Object::Action(action)))) = lib.object(*a) {
                        played_by.entry(action.target_id).or_insert_with(|| lib.label(o.id, 4));
                    }
                }
            }
        }
    }
    let mut out: Vec<(u32, Vec<String>)> = by_playlist
        .into_iter()
        .map(|(id, mut names)| {
            if names.is_empty() {
                names.push(played_by.get(&id).map_or_else(|| lib.label(id, 13), |e| format!("played by the event {e}")));
            }
            // The most specific situation first (it names the playlist in the list).
            names.sort_by_key(|n| (std::cmp::Reverse(if n == "any situation" { 0 } else { n.split(", ").count() }), n.clone()));
            (id, names)
        })
        .collect();
    // Named ones first: a path of unknown state IDs (#...) goes last.
    out.sort_by_key(|(_, names)| (names.first().is_some_and(|n| n.starts_with('#')), names.first().cloned()));
    out
}

/// What an animation frame plays.
pub enum AnimationSound {
    /// An existing event, by name.
    Event(String),
    /// A new sound (`wem`) made like the first sound `like_event` plays, played by a new event `name`.
    NewSound { wem: Vec<u8>, like_event: String, name: String },
}

/// The first sound object a tree reaches.
fn first_sound(n: &PlayNode) -> Option<u32> {
    if n.kind == 2 {
        return Some(n.id);
    }
    n.children.iter().find_map(first_sound)
}

/// Adds a sound frame at `time` (seconds) to an animation's `.snd.meta`, firing `sound` in battle; makes what it needs (a new
/// sound and event, a trigger row). Returns the trigger row.
///
/// # Errors
///
/// When the `.snd.meta` or the triggers CSV can't be read, or the events named are not in the banks.
pub fn animation_sound(
    edits: &mut EditSet,
    lib: &SoundLibrary,
    files: &dyn Files,
    snd_meta: &str,
    time: f32,
    sound: AnimationSound,
) -> Result<usize> {
    let event = match sound {
        AnimationSound::Event(name) => {
            if !lib.has_object(fnv::id(&name)) && edits.object(lib, fnv::id(&name)).is_none() {
                return Err(AudioError::Invalid(format!("no event {name}")));
            }
            name
        }
        AnimationSound::NewSound { wem, like_event, name } => {
            let like = fnv::id(&like_event);
            let template =
                first_sound(&lib.resolve(like, files)).ok_or_else(|| AudioError::Invalid(format!("{like_event} plays no sound")))?;
            let sound = edits.new_sound_like(lib, template, &wem)?;
            let bank = *lib.banks_of(template).first().ok_or_else(|| AudioError::Invalid("the template is in no bank".into()))?;
            edits.new_event(lib, bank, &name, sound)?;
            name
        }
    };

    // A trigger row firing the event in battle: an existing one, else a new one.
    const CSV: &str = "audio/sound_animation_triggers.csv";
    let csv = edits.loose.get(CSV).cloned().map_or_else(|| files.read(CSV).map_err(AudioError::Invalid), Ok)?;
    let triggers = Triggers::read(&csv)?;
    let row = match (0..triggers.rows.len()).find(|&r| triggers.get(r, "sound_event_name_battle") == Some(event.as_str())) {
        Some(r) => r,
        None => edits.add_trigger(files, &format!("wce_{event}"), &event, "", 0)?,
    };

    let bytes = edits.loose.get(snd_meta).cloned().map_or_else(|| files.read(snd_meta).map_err(AudioError::Invalid), Ok)?;
    let mut meta = Meta::decode(&bytes).ok_or_else(|| AudioError::Invalid(format!("{snd_meta} does not read")))?;
    anim_sounds::add_frame(&mut meta, time, row);
    edits.loose.insert(snd_meta.to_owned(), meta.encode());
    Ok(row)
}

/// What a dialogue event plays for one value of one of its state groups: (the path's keys, the audio node), the node being a sound
/// or container to replace.
pub fn culture_lines(edits: &EditSet, lib: &SoundLibrary, dialogue_event: u32, group: u32, value: u32) -> Vec<(Vec<u32>, u32)> {
    let Some(Object::DialogueEvent(d)) = edits.object(lib, dialogue_event) else { return vec![] };
    let Some(depth) = d.arguments.iter().position(|(g, _)| *g == group) else { return vec![] };
    d.paths().into_iter().filter(|(keys, _)| keys.get(depth) == Some(&value)).collect()
}

/// The values a dialogue event's state group takes in its tree (e.g. the cultures it has lines for).
pub fn group_values(edits: &EditSet, lib: &SoundLibrary, dialogue_event: u32, group: u32) -> Vec<u32> {
    let Some(Object::DialogueEvent(d)) = edits.object(lib, dialogue_event) else { return vec![] };
    let Some(depth) = d.arguments.iter().position(|(g, _)| *g == group) else { return vec![] };
    let mut values: Vec<u32> = d.paths().iter().filter_map(|(k, _)| k.get(depth).copied()).collect();
    values.sort_unstable();
    values.dedup();
    values
}

/// The voice-over cultures the engine knows (`empire.retail.dll` holds them as a list; any other value is reported as
/// `invalid_voiceover_culture`): the values `main_units.voiceover` and `subculture_religion_voiceover_junctions.voiceover_culture`
/// may take, and of `vo_battle_subculture` in the battle voice trees. A culture without lines of its own in a tree falls to
/// another's; [`new_voice_culture`] gives it its own.
pub const ENGINE_VOICEOVER_CULTURES: [&str; 14] = [
    "barbarian",
    "barbarian_christian",
    "barbarian_pagan",
    "barbarian_generic",
    "nomad",
    "nomad_hunnic",
    "nomad_hunnic_generic",
    "eastern",
    "eastern_desert",
    "eastern_persian",
    "eastern_generic",
    "roman",
    "roman_christian",
    "roman_generic",
];

/// Lines of its own for a battle voice culture (a value of `vo_battle_subculture`, named `name`: one of
/// [`ENGINE_VOICEOVER_CULTURES`], since the engine knows no other), made from `like`'s: in every dialogue event deciding on it,
/// `like`'s branch copied (new containers, the same sound files; change them with Add variation / Remove). Returns (dialogue
/// events, lines). The DB's `subculture_religion_voiceover_junctions` then gives the culture to a subculture and religion.
///
/// # Errors
///
/// When the name is not a valid key or no event has lines for `like`.
pub fn new_voice_culture(edits: &mut EditSet, lib: &SoundLibrary, name: &str, like: u32) -> Result<(usize, usize)> {
    if name.is_empty() || !name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
        return Err(AudioError::Invalid(format!("\"{name}\": letters, digits and _ only")));
    }
    let group = fnv::id("vo_battle_subculture");
    let new = fnv::id(name);
    let mut events: Vec<u32> = vec![];
    for b in &lib.banks {
        for (o, t) in b.bank.objects().iter().zip(&b.typed) {
            if let Some(Object::DialogueEvent(d)) = t
                && d.arguments.iter().any(|(g, _)| *g == group)
                && !events.contains(&o.id)
            {
                events.push(o.id);
            }
        }
    }
    let (mut done, mut lines) = (0, 0);
    for event in events {
        // Only where it has no lines of its own yet.
        let values = group_values(edits, lib, event, group);
        if values.contains(&like) && !values.contains(&new) {
            lines += edits.add_dialogue_value(lib, event, group, like, new)?;
            done += 1;
        }
    }
    if done == 0 {
        return Err(AudioError::Invalid(format!("no voice event has lines for {} that {name} lacks", lib.label(like, 0))));
    }
    edits.names.insert(new, name.to_owned());
    Ok((done, lines))
}
