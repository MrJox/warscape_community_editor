//! Battle unit voices (Attila; docs/plans/audio.md, step 10): `Battle_Order_VO_Play` plays a switch on `VO_battle_culture`
//! (Barbarian, Eastern, Nomad, Roman), each value a switch on `VO_battle_actor` (the voice actors: Inf1 to Inf12 for foot, Mtd1 to
//! Mtd7 for mounted units), each actor a switch on `VO_battle_order` (about 517 lines: orders, `selected_<unit>` lines, attack
//! orders by culture...), each line a random container of takes, in its own bank (`battle_vo_orders_<culture>_<actor>`).
//!
//! A unit's culture is its `main_units.voiceover` (barbarian, barbarian_christian... by its first word); its actors are those of
//! its family (`land_units.category`: cavalry, elephants and chariots are mounted, the rest on foot); its selection lines are
//! `land_units.selection_vo`, `selected_vo_secondary` and `selected_vo_tertiary`, values of `VO_battle_order`. A new line for a
//! unit is a new `VO_battle_order` value under every actor of its culture and family, and the unit's `selection_vo` set to it.

use wce_audio_format::bank::HircObject;
use wce_audio_format::fnv;
use wce_audio_format::hirc::{Object, SwitchChildParams};

use crate::edit::EditSet;
use crate::library::SoundLibrary;
use crate::{AudioError, Result};

/// The event playing every order line.
pub const ORDERS_EVENT: &str = "battle_order_vo_play";

/// One culture's voice actors: (actor value, its `VO_battle_order` switch).
#[derive(Clone, Debug)]
pub struct CultureActors {
    pub culture: u32,
    pub actors: Vec<(u32, u32)>,
    /// The switch's default: what plays for a culture value it does not have (TDD's orders bank has a single set, its default,
    /// so every unit speaks it whatever its voice-over).
    pub default: bool,
}

/// The order voices as built: the cultures and their actors.
pub fn cultures(edits: &EditSet, lib: &SoundLibrary) -> Vec<CultureActors> {
    let culture_group = fnv::id("vo_battle_culture");
    let Some(Object::Event(event)) = lib.merged(fnv::id(ORDERS_EVENT)) else { return vec![] };
    let mut out = vec![];
    for action in &event.actions {
        let Some(Object::Action(a)) = edits.object(lib, *action) else { continue };
        // Every bank's copy joined: each voice bank (and a mod's) holds only its branch of the switches above its own.
        let Some(Object::Switch(top)) = lib.merged(a.target_id) else { continue };
        if top.group_id != culture_group {
            continue;
        }
        for (culture, children) in &top.assignments {
            let Some(Object::Switch(actor_switch)) = children.first().and_then(|c| lib.merged(*c)) else { continue };
            let actors = actor_switch
                .assignments
                .iter()
                .filter_map(|(actor, c)| {
                    let orders = *c.first()?;
                    matches!(edits.object(lib, orders), Some(Object::Switch(_))).then_some((*actor, orders))
                })
                .collect();
            out.push(CultureActors { culture: *culture, actors, default: *culture == top.default_switch });
        }
    }
    out
}

/// The culture value a unit's `main_units.voiceover` speaks: its first word, matched to the switch's value names, else the switch's
/// default (as the game does).
pub fn culture_of(lib: &SoundLibrary, cultures: &[CultureActors], voiceover: &str) -> Option<u32> {
    let base = voiceover.split('_').next()?.to_lowercase();
    cultures
        .iter()
        .find(|c| lib.label(c.culture, 0).to_lowercase() == base)
        .or_else(|| cultures.iter().find(|c| c.default))
        .map(|c| c.culture)
}

/// Whether a `land_units.category` is mounted (its actors are the Mtd ones).
pub fn mounted(category: &str) -> bool {
    matches!(category, "cavalry" | "elephants" | "chariots" | "camels")
}

/// The actors of a family among a culture's: mounted ones are named `Mtd...`, the others `Inf...`; every actor when the set
/// has none of the family (a mod's set with its own actors).
pub fn family_actors(lib: &SoundLibrary, culture: &CultureActors, mounted: bool) -> Vec<(u32, u32)> {
    let prefix = if mounted { "mtd" } else { "inf" };
    let family: Vec<(u32, u32)> =
        culture.actors.iter().copied().filter(|(a, _)| lib.label(*a, 0).to_lowercase().starts_with(prefix)).collect();
    // A set without actors of this family (TDD's has a single actor for every unit): all its actors.
    if family.is_empty() { culture.actors.clone() } else { family }
}

/// An actor's lines: (`VO_battle_order` value, what it plays), in the switch's order.
pub fn lines(edits: &EditSet, lib: &SoundLibrary, orders_switch: u32) -> Vec<(u32, u32)> {
    let Some(Object::Switch(s)) = edits.object(lib, orders_switch) else { return vec![] };
    s.assignments.iter().filter_map(|(v, c)| Some((*v, *c.first()?))).collect()
}

impl EditSet {
    /// Adds a line (a new `VO_battle_order` value named `name`) to each of these order switches: a random container made like the
    /// one `like` plays there, with one take, a sound streaming `wem` (one loose file shared by every actor). Returns the new
    /// value's ID.
    ///
    /// # Errors
    ///
    /// When the name is not a valid key, the value already exists, or a switch has no `like` line to copy.
    pub fn add_order_line(&mut self, lib: &SoundLibrary, orders_switches: &[u32], name: &str, like: u32, wem: &[u8]) -> Result<u32> {
        if name.is_empty() || !name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
            return Err(AudioError::Invalid(format!("\"{name}\": letters, digits and _ only")));
        }
        let value = fnv::id(name);
        let media = self.new_id(lib, value);
        self.loose.insert(format!("audio/{media}.wem"), wem.to_vec());
        // A switch listed twice (voice sets sharing an actor's bank) gets the line once.
        let mut switches = orders_switches.to_vec();
        switches.sort_unstable();
        switches.dedup();
        for &switch_id in &switches {
            let Some(Object::Switch(mut switch)) = self.object(lib, switch_id) else {
                return Err(AudioError::Invalid(format!("#{switch_id} is not a switch")));
            };
            if switch.assignments.iter().any(|(v, _)| *v == value) {
                return Err(AudioError::Invalid(format!("{name} is already a line of #{switch_id}")));
            }
            let like_child = switch
                .assignments
                .iter()
                .find(|(v, _)| *v == like)
                .and_then(|(_, c)| c.first().copied())
                .ok_or_else(|| AudioError::Invalid(format!("#{switch_id} has no line {}", lib.label(like, 0))))?;

            // The take: a sound made like the copied line's first one (or the line itself, when it is a sound).
            let (container, template_sound) = match self.object(lib, like_child) {
                Some(Object::RandomSequence(r)) => {
                    let sound = r.children.iter().find(|c| matches!(self.object(lib, **c), Some(Object::Sound(_)))).copied();
                    (Some(r), sound.ok_or_else(|| AudioError::Invalid("the copied line has no take".into()))?)
                }
                Some(Object::Sound(_)) => (None, like_child),
                _ => return Err(AudioError::Invalid("the copied line is neither a container nor a sound".into())),
            };
            let Some(Object::Sound(mut sound)) = self.object(lib, template_sound) else { unreachable!("checked above") };
            let sound_id = self.new_id(lib, template_sound);
            sound.source.source_id = media;
            sound.source.stream_type = 2;
            sound.source.file_id = media;
            sound.source.file_offset = None;
            sound.source.in_memory_size = 0;

            let (bank, at) = self.find(lib, like_child).ok_or_else(|| AudioError::Invalid("the copied line is in no bank".into()))?;
            // The new objects, children before their parent (as in the game's banks).
            let (line_id, objects) = match container {
                Some(mut r) => {
                    let container_id = self.new_id(lib, sound_id);
                    sound.node.parent_id = container_id;
                    r.node.parent_id = switch_id;
                    r.children = vec![sound_id];
                    r.playlist = if r.playlist.is_empty() { vec![] } else { vec![(sound_id, 50_000)] };
                    let sound = HircObject { kind: 2, id: sound_id, body: Object::Sound(sound).write() };
                    (container_id, vec![sound, HircObject { kind: 5, id: container_id, body: Object::RandomSequence(r).write() }])
                }
                None => {
                    sound.node.parent_id = switch_id;
                    (sound_id, vec![HircObject { kind: 2, id: sound_id, body: Object::Sound(sound).write() }])
                }
            };
            for (k, o) in objects.iter().enumerate() {
                self.bank_mut(lib, bank).objects_mut().insert(at + 1 + k, o.clone());
            }

            let place = switch.children.partition_point(|c| *c < line_id);
            switch.children.insert(place, line_id);
            let place = switch.assignments.partition_point(|(v, _)| *v < value);
            switch.assignments.insert(place, (value, vec![line_id]));
            let params = switch.child_params.iter().find(|p| p.node_id == like_child).cloned().unwrap_or(SwitchChildParams {
                node_id: 0,
                bits_0: 0,
                bits_1: 0,
                fade_out_time: 0.0,
                fade_in_time: 0.0,
            });
            let place = switch.child_params.partition_point(|p| p.node_id < line_id);
            switch.child_params.insert(place, SwitchChildParams { node_id: line_id, ..params });
            // Every whole copy of the switch gains the line, and its bank the line's objects.
            let banks = self.put_object_copies(lib, switch_id, &Object::Switch(switch))?;
            let others: Vec<usize> = banks.into_iter().filter(|b| *b != bank).collect();
            self.insert_before(lib, &others, switch_id, &objects);
        }
        self.names.insert(value, name.to_owned());
        Ok(value)
    }
}
