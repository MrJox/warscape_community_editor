//! Edits of decision trees (docs/plans/audio.md, step 10): what a music switch plays for a path of states, and a new value of a
//! dialogue event's state group made like another (a new voice-over culture: the copied culture's lines as new containers, their
//! takes playing the same sound files). Trees are laid out again with `hirc::tree_from_paths`, which gives every game tree back
//! byte for byte.

use wce_audio_format::bank::HircObject;
use wce_audio_format::hirc::{Object, tree_all_paths, tree_from_paths, tree_leaf};

use crate::edit::EditSet;
use crate::library::SoundLibrary;
use crate::{AudioError, Result};

impl EditSet {
    /// Sets what a music switch plays for a path of states (its keys, one per state group, 0 for any): `target`, a music
    /// playlist, segment or switch. The path is added when the tree does not have it.
    ///
    /// # Errors
    ///
    /// When the object is not a music switch, the path has the wrong length, or the target is not a music object.
    pub fn set_music_path(&mut self, lib: &SoundLibrary, switch_id: u32, keys: &[u32], target: u32) -> Result<()> {
        let Some(Object::MusicSwitch(mut switch)) = self.object(lib, switch_id) else {
            return Err(AudioError::Invalid(format!("#{switch_id} is not a music switch")));
        };
        let depth = switch.arguments.len();
        if keys.len() != depth {
            return Err(AudioError::Invalid(format!("a path of {} states for a tree of {depth}", keys.len())));
        }
        if !matches!(self.object(lib, target), Some(Object::MusicPlaylist(_) | Object::MusicSegment(_) | Object::MusicSwitch(_))) {
            return Err(AudioError::Invalid(format!("#{target} is not a music playlist, segment or switch")));
        }
        match tree_leaf(depth, &switch.nodes, keys) {
            Some(i) => switch.nodes[i].value = target,
            None => {
                let mut paths = tree_all_paths(depth, &switch.nodes);
                paths.push((keys.to_vec(), target));
                switch.nodes = tree_from_paths(depth, &paths, &switch.nodes);
            }
        }
        // The switch lists what it can play among its children.
        if !switch.base.children.contains(&target) {
            let at = switch.base.children.partition_point(|c| *c < target);
            switch.base.children.insert(at, target);
        }
        self.put_object(lib, switch_id, &Object::MusicSwitch(switch))
    }

    /// Gives a dialogue event's state group a new value (`new`) playing what `like` plays: each of `like`'s paths again with the
    /// new value, its line a new container whose takes are new sounds playing the same files (so the new value's lines change
    /// apart: Add variation / Remove; Replace audio changes a file both use). Returns how many lines were copied.
    ///
    /// # Errors
    ///
    /// When the object is not a dialogue event deciding on `group`, `like` has no lines, or `new` has some already.
    pub fn add_dialogue_value(&mut self, lib: &SoundLibrary, dialogue_id: u32, group: u32, like: u32, new: u32) -> Result<usize> {
        let Some(Object::DialogueEvent(mut event)) = self.object(lib, dialogue_id) else {
            return Err(AudioError::Invalid(format!("#{dialogue_id} is not a dialogue event")));
        };
        let depth = event.arguments.len();
        let at = event
            .arguments
            .iter()
            .position(|(g, _)| *g == group)
            .ok_or_else(|| AudioError::Invalid(format!("#{dialogue_id} does not decide on #{group}")))?;
        let mut paths = tree_all_paths(depth, &event.nodes);
        if paths.iter().any(|(k, _)| k.len() == depth && k[at] == new) {
            return Err(AudioError::Invalid(format!("#{dialogue_id} has lines for #{new} already")));
        }
        let copied: Vec<(Vec<u32>, u32)> = paths.iter().filter(|(k, v)| k.len() == depth && k[at] == like && *v != 0).cloned().collect();
        if copied.is_empty() {
            return Err(AudioError::Invalid(format!("#{dialogue_id} has no lines for #{like}")));
        }
        for (mut keys, line) in copied.iter().cloned() {
            keys[at] = new;
            let copy = self.copy_line(lib, line)?;
            paths.push((keys, copy));
        }
        event.nodes = tree_from_paths(depth, &paths, &event.nodes);
        self.put_object(lib, dialogue_id, &Object::DialogueEvent(event))?;
        Ok(copied.len())
    }

    /// A copy of a line (a random/sequence container of sounds, or a sound) with new IDs, its sounds playing the same files, in the
    /// same bank right after it.
    fn copy_line(&mut self, lib: &SoundLibrary, line: u32) -> Result<u32> {
        let (bank, at) = self.find(lib, line).ok_or_else(|| AudioError::Invalid(format!("#{line} is in no bank")))?;
        match self.object(lib, line) {
            Some(Object::Sound(sound)) => {
                let id = self.new_id(lib, line);
                self.bank_mut(lib, bank).objects_mut().insert(at + 1, HircObject { kind: 2, id, body: Object::Sound(sound).write() });
                Ok(id)
            }
            Some(Object::RandomSequence(mut container)) => {
                let id = self.new_id(lib, line);
                let mut takes = vec![];
                for child in container.children.clone() {
                    let Some(Object::Sound(mut sound)) = self.object(lib, child) else { continue };
                    let take = self.new_id(lib, child ^ id);
                    sound.node.parent_id = id;
                    takes.push((child, take, sound));
                }
                // Children before their parent, as in the game's banks.
                let mut i = at + 1;
                for (_, take, sound) in &takes {
                    self.bank_mut(lib, bank)
                        .objects_mut()
                        .insert(i, HircObject { kind: 2, id: *take, body: Object::Sound(sound.clone()).write() });
                    i += 1;
                }
                let map = |c: u32| takes.iter().find(|(old, _, _)| *old == c).map(|(_, new, _)| *new);
                container.children = container.children.iter().filter_map(|c| map(*c)).collect();
                container.children.sort_unstable();
                container.playlist = container.playlist.iter().filter_map(|(c, w)| Some((map(*c)?, *w))).collect();
                self.bank_mut(lib, bank)
                    .objects_mut()
                    .insert(i, HircObject { kind: 5, id, body: Object::RandomSequence(container).write() });
                Ok(id)
            }
            _ => Err(AudioError::Invalid(format!("#{line} is neither a sound nor a random/sequence container"))),
        }
    }
}
