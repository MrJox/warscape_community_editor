//! Who uses a sound object: before deleting one, or to see why it plays. The game's own references are found by ID (an event's ID is
//! the FNV-1 of its name): `event_data.dat` (events the game's code triggers by name), `sound_animation_triggers.csv` (what the
//! animations' `.snd.meta` frames fire), campaign `.amb` emitters and the UI layouts (a component names the event it fires). Inside
//! the banks: parents, actions, switch assignments, dialogue and music trees, playlists.

use std::collections::HashMap;

use wce_audio_format::amb::Ambience;
use wce_audio_format::event_data::EventData;
use wce_audio_format::fnv;
use wce_audio_format::hirc::Object;
use wce_audio_format::triggers::Triggers;
use wce_vfs::files::Files;

use crate::edit::EditSet;
use crate::library::SoundLibrary;

/// One use of an object.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Reference {
    /// Another object of the banks uses it (`how`: "child of", "played by", "switch value of"...).
    Object { bank: usize, id: u32, kind: u8, how: &'static str },
    /// Listed in `event_data.dat`: the game's code may trigger it by name.
    EventData { name: String },
    /// Named by a row of `sound_animation_triggers.csv` (fired by the animations whose `.snd.meta` holds that row).
    Trigger { row: usize, trigger: String, column: String },
    /// Played by emitters of a campaign map's ambience.
    Ambience { path: String, emitters: usize },
    /// Named by a UI layout.
    UiLayout { path: String },
    /// Named by a DB table's row (`audio_projectiles`, `audio_explosions`).
    Db { table: String, key: String, column: String },
    /// Fired by animations: `count` `.snd.meta` files hold a trigger row naming it.
    Animations { count: usize },
}

impl Reference {
    /// One line for the user.
    pub fn describe(&self, lib: &SoundLibrary) -> String {
        match self {
            Reference::Object { bank, id, kind, how } => format!("{how} {} ({})", lib.label(*id, *kind), lib.banks[*bank].name),
            Reference::EventData { name } => format!("event_data.dat lists {name}: the game's code may trigger it"),
            Reference::Trigger { row, trigger, column } => {
                format!("sound_animation_triggers.csv row {row} ({trigger}, {column}): animations fire it")
            }
            Reference::Ambience { path, emitters } => format!("{path}: {emitters} campaign ambience emitters"),
            Reference::UiLayout { path } => format!("UI layout {path}"),
            Reference::Db { table, key, column } => format!("DB {table}: row {key}, {column}"),
            Reference::Animations { count } => format!("{count} animations (.snd.meta) fire it"),
        }
    }

    /// True for a use by the game itself (not by another object of the banks).
    pub fn from_game(&self) -> bool {
        !matches!(self, Reference::Object { .. })
    }
}

/// The game's references outside the banks, by ID; built once ([`ReferenceIndex::build`]).
#[derive(Default)]
pub struct ReferenceIndex {
    by_id: HashMap<u32, Vec<Reference>>,
}

/// Strings of letters, digits and `_` of at least 4 characters (UI layouts hold their sound events as such).
fn words(bytes: &[u8]) -> impl Iterator<Item = &str> {
    bytes.split(|b| !(b.is_ascii_alphanumeric() || *b == b'_')).filter(|w| w.len() >= 4).filter_map(|w| std::str::from_utf8(w).ok())
}

impl ReferenceIndex {
    /// Reads `event_data.dat`, the triggers CSV, every `.amb` and every UI layout of the files (a few seconds: on a worker).
    pub fn build(files: &dyn Files, lib: &SoundLibrary) -> Self {
        let mut index = Self::default();
        let mut add = |id: u32, r: Reference| {
            let list = index.by_id.entry(id).or_default();
            if !list.contains(&r) {
                list.push(r);
            }
        };

        if let Ok(bytes) = files.read("audio/event_data.dat")
            && let Ok(data) = EventData::read(&bytes)
        {
            for name in data.events.iter().map(|(n, _)| n).chain(data.dialogue_events.iter().map(|(n, _)| n)) {
                add(fnv::id(name), Reference::EventData { name: name.clone() });
            }
        }

        if let Ok(bytes) = files.read("audio/sound_animation_triggers.csv")
            && let Ok(t) = Triggers::read(&bytes)
        {
            for (row, cells) in t.rows.iter().enumerate() {
                for (c, cell) in cells.iter().enumerate().skip(1) {
                    if !cell.is_empty() {
                        let column = t.header.get(c).cloned().unwrap_or_default();
                        add(fnv::id(cell), Reference::Trigger { row, trigger: cells[0].clone(), column });
                    }
                }
            }
        }

        for path in files.list("", ".amb") {
            let Ok(amb) = files.read(&path).map_err(|_| ()).and_then(|b| Ambience::read(&b).map_err(|_| ())) else { continue };
            let mut counts: HashMap<u32, usize> = HashMap::new();
            amb.emitters.iter().for_each(|e| *counts.entry(e.event_id).or_default() += 1);
            for (id, emitters) in counts {
                add(id, Reference::Ambience { path: path.clone(), emitters });
            }
        }

        // UI layouts: only the words that are names of objects the banks have (events mostly).
        for path in files.list("ui/", "") {
            let Ok(bytes) = files.read(&path) else { continue };
            for w in words(&bytes) {
                let id = fnv::id(w);
                if lib.has_object(id) {
                    add(id, Reference::UiLayout { path: path.clone() });
                }
            }
        }

        // The DB tables naming events.
        for (table, key, column, event) in crate::sound_db::SoundDb::read(files).events() {
            add(fnv::id(&event), Reference::Db { table, key, column });
        }

        // Animations: each .snd.meta frame's trigger row names events.
        if let Ok(bytes) = files.read("audio/sound_animation_triggers.csv")
            && let Ok(triggers) = Triggers::read(&bytes)
        {
            let mut counts: HashMap<u32, usize> = HashMap::new();
            for path in files.list("animations/", ".snd.meta") {
                let Some(meta) = files.read(&path).ok().and_then(|b| wce_formats::anim_meta::Meta::decode(&b)) else { continue };
                let mut ids: Vec<u32> = vec![];
                for f in crate::anim_sounds::frames(&meta) {
                    let (_, battle, campaign) = crate::anim_sounds::trigger_events(&triggers, f.trigger);
                    ids.extend([battle, campaign].iter().filter(|e| !e.is_empty()).map(|e| fnv::id(e)));
                }
                ids.sort_unstable();
                ids.dedup();
                ids.into_iter().for_each(|id| *counts.entry(id).or_default() += 1);
            }
            for (id, count) in counts {
                add(id, Reference::Animations { count });
            }
        }

        index
    }

    /// The game's references to an ID.
    pub fn game(&self, id: u32) -> &[Reference] {
        self.by_id.get(&id).map_or(&[], Vec::as_slice)
    }
}

/// Every use of an object: the game's (from the index) and the banks' (as edited).
pub fn find(lib: &SoundLibrary, edits: &EditSet, index: &ReferenceIndex, id: u32) -> Vec<Reference> {
    let mut out = index.game(id).to_vec();
    for b in 0..lib.banks.len() {
        for o in edits.bank(lib, b).objects() {
            if o.id == id {
                continue;
            }
            let Some(Ok(object)) = wce_audio_format::hirc::parse(lib.banks[b].version, o.kind, &o.body) else { continue };
            let how = match &object {
                Object::Event(e) if e.actions.contains(&id) => Some("action of"),
                Object::Action(a) if a.target_id == id => Some("target of"),
                Object::Switch(s) if s.assignments.iter().any(|(v, nodes)| *v == id || nodes.contains(&id)) => Some("switch value of"),
                Object::DialogueEvent(d) if d.nodes.iter().any(|n| n.value == id || n.key == id) => Some("in the tree of"),
                Object::MusicSwitch(m) if m.nodes.iter().any(|n| n.value == id) => Some("in the tree of"),
                Object::MusicPlaylist(p) if p.items.iter().any(|i| i.segment_id == id) => Some("in the playlist of"),
                other if other.children().contains(&id) => Some("child of"),
                _ => None,
            };
            if let Some(how) = how {
                out.push(Reference::Object { bank: b, id: o.id, kind: o.kind, how });
            }
        }
    }
    out
}
