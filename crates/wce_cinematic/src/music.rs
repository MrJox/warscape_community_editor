//! The game's battle music, which is interactive: one event (`game_music_play`) plays a music switch tree whose branches the music
//! states pick (Faction: Roman, hunnic...; Key; the battle's moments), and the `music_set_*` events only set those states (their
//! actions set a state group to a value: params `0000`, group id, value id). A music cue of a scene is a set of moods (those
//! events): the battle plays them (the game's music follows), the editor and the movies render the branch they lead to: the switch
//! trees walked with the moods' states (a group not set takes the default branch), the playlist's segments one after another, each
//! segment its tracks mixed over its length.

use std::collections::HashMap;

use wce_audio::library::SoundLibrary;
use wce_audio_format::hirc::{MusicSwitch, Object};
use wce_vfs::files::Files;

/// The event that plays the game's music.
pub const MUSIC_PLAY: &str = "game_music_play";

/// A mood: a `music_set_<Group>_<Value>` event and the states it sets.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Mood {
    pub event: String,
    /// From its name: `Faction`, `Hunnic`.
    pub group: String,
    pub value: String,
    pub states: Vec<(u32, u32)>,
}

/// The states an event sets (its set-state actions: type 0x12xx).
fn states_of(lib: &SoundLibrary, event: u32) -> Vec<(u32, u32)> {
    let Some(Object::Event(e)) = lib.merged(event) else { return vec![] };
    e.actions
        .iter()
        .filter_map(|a| match lib.merged(*a) {
            Some(Object::Action(act)) if act.action_type >> 8 == 0x12 && act.params.len() >= 10 => {
                let u = |o: usize| u32::from_le_bytes([act.params[o], act.params[o + 1], act.params[o + 2], act.params[o + 3]]);
                Some((u(2), u(6)))
            }
            _ => None,
        })
        .collect()
}

/// Every mood of the banks: the named `music_set_*` events that set states, by group then value.
pub fn moods(lib: &SoundLibrary) -> Vec<Mood> {
    let mut out: Vec<Mood> = vec![];
    for b in &lib.banks {
        for o in b.bank.objects().iter().filter(|o| o.kind == 4) {
            let Some(name) = lib.names.get(o.id) else { continue };
            let Some(rest) = name.strip_prefix("music_set_") else { continue };
            if out.iter().any(|m| m.event == name) {
                continue;
            }
            let states = states_of(lib, o.id);
            if states.is_empty() {
                continue;
            }
            let (group, value) = rest.split_once('_').map_or((rest.to_owned(), String::new()), |(g, v)| (g.to_owned(), v.to_owned()));
            out.push(Mood { event: name.to_owned(), group, value, states });
        }
    }
    out.sort_by(|a, b| a.group.to_lowercase().cmp(&b.group.to_lowercase()).then(a.value.to_lowercase().cmp(&b.value.to_lowercase())));
    out
}

/// A battle's music always plays in the game mode Battle (the root switch's first choice); the moods come on top.
pub const BATTLE_MODE: &str = "music_set_gamemode_Battle";

fn battle_states(lib: &SoundLibrary) -> HashMap<u32, u32> {
    states_of(lib, wce_audio_format::fnv::id(BATTLE_MODE)).into_iter().collect()
}

/// The branch a music switch's decision tree gives for the states (a group not set, or a value it has no branch for: the default).
fn pick(sw: &MusicSwitch, states: &HashMap<u32, u32>) -> Option<u32> {
    let mut node = *sw.nodes.first()?;
    for (group, _) in &sw.arguments {
        let (first, count) = ((node.value & 0xFFFF) as usize, (node.value >> 16) as usize);
        let kids = sw.nodes.get(first..first + count)?;
        let want = states.get(group).copied();
        node = *want.and_then(|w| kids.iter().find(|k| k.key == w)).or_else(|| kids.iter().find(|k| k.key == 0)).or(kids.first())?;
    }
    Some(node.value)
}

/// The segments the music plays, in order, from an object of the tree.
fn segments(lib: &SoundLibrary, id: u32, states: &HashMap<u32, u32>, out: &mut Vec<u32>, depth: usize) {
    if depth > 24 {
        return;
    }
    match lib.merged(id) {
        Some(Object::MusicSwitch(sw)) => {
            if let Some(next) = pick(&sw, states) {
                segments(lib, next, states, out, depth + 1);
            }
        }
        Some(Object::MusicPlaylist(pl)) => {
            for item in pl.items.iter().filter(|i| i.children == 0 && i.segment_id != 0) {
                segments(lib, item.segment_id, states, out, depth + 1);
            }
        }
        Some(Object::MusicSegment(_)) => out.push(id),
        _ => {}
    }
}

/// The segments the moods lead to (with their tracks' count), in play order: for checks.
pub fn branch(lib: &SoundLibrary, moods: &[String]) -> Vec<(u32, usize)> {
    let mut states = battle_states(lib);
    for m in moods {
        for (g, v) in states_of(lib, wce_audio_format::fnv::id(m)) {
            states.insert(g, v);
        }
    }
    let Some(Object::Event(e)) = lib.merged(wce_audio_format::fnv::id(MUSIC_PLAY)) else { return vec![] };
    let Some(root) = e.actions.iter().find_map(|a| match lib.merged(*a) {
        Some(Object::Action(act)) if act.target_id != 0 => Some(act.target_id),
        _ => None,
    }) else {
        return vec![];
    };
    let mut list = vec![];
    segments(lib, root, &states, &mut list, 0);
    list.into_iter()
        .map(|s| {
            (
                s,
                match lib.merged(s) {
                    Some(Object::MusicSegment(m)) => m.base.children.len(),
                    _ => 0,
                },
            )
        })
        .collect()
}

/// The music the moods lead to, `seconds` long (its segments again from the first when they end sooner), as 48 kHz stereo frames.
/// None when the banks have no music or the branch plays nothing.
pub fn render(
    lib: &SoundLibrary,
    files: &dyn Files,
    moods: &[String],
    seconds: f64,
    decode: &dyn Fn(&[u8]) -> Option<Vec<[f32; 2]>>,
) -> Option<Vec<[f32; 2]>> {
    let rate = f64::from(crate::movie_export::MIX_RATE);
    let mut states = battle_states(lib);
    for m in moods {
        for (g, v) in states_of(lib, wce_audio_format::fnv::id(m)) {
            states.insert(g, v);
        }
    }
    // The music the play event starts.
    let Some(Object::Event(e)) = lib.merged(wce_audio_format::fnv::id(MUSIC_PLAY)) else { return None };
    let root = e.actions.iter().find_map(|a| match lib.merged(*a) {
        Some(Object::Action(act)) if act.target_id != 0 => Some(act.target_id),
        _ => None,
    })?;
    let mut list = vec![];
    segments(lib, root, &states, &mut list, 0);
    if list.is_empty() {
        return None;
    }
    let total = (seconds * rate).round() as usize;
    let mut out: Vec<[f32; 2]> = Vec::with_capacity(total);
    let mut cache: HashMap<u32, Vec<[f32; 2]>> = HashMap::new();
    let mut silent_rounds = 0;
    while out.len() < total && silent_rounds < 2 {
        let before = out.len();
        for &seg in &list {
            let pcm = cache.entry(seg).or_insert_with(|| segment_pcm(lib, files, seg, decode)).clone();
            out.extend(pcm);
            if out.len() >= total {
                break;
            }
        }
        silent_rounds = if out.len() == before { silent_rounds + 1 } else { 0 };
    }
    out.truncate(total);
    out.resize(total, [0.0; 2]);
    Some(out)
}

/// A segment's tracks mixed over its length (the longest track's when it gives none).
fn segment_pcm(lib: &SoundLibrary, files: &dyn Files, seg: u32, decode: &dyn Fn(&[u8]) -> Option<Vec<[f32; 2]>>) -> Vec<[f32; 2]> {
    let Some(Object::MusicSegment(s)) = lib.merged(seg) else { return vec![] };
    let rate = f64::from(crate::movie_export::MIX_RATE);
    let mut tracks: Vec<Vec<[f32; 2]>> = vec![];
    for &t in &s.base.children {
        let tree = lib.resolve(t, files);
        fn first(n: &wce_audio::library::PlayNode) -> Option<&wce_audio::library::MediaRef> {
            n.media.as_ref().or_else(|| n.children.iter().find_map(first))
        }
        if let Some(m) = first(&tree)
            && let Some(pcm) = lib.media_bytes(m, files).and_then(|b| decode(&b))
        {
            tracks.push(pcm);
        }
    }
    let len = if s.duration > 0.0 { (s.duration / 1000.0 * rate).round() as usize } else { tracks.iter().map(Vec::len).max().unwrap_or(0) };
    let mut mix = vec![[0f32; 2]; len];
    for t in &tracks {
        for (m, v) in mix.iter_mut().zip(t) {
            m[0] += v[0];
            m[1] += v[1];
        }
    }
    mix
}
