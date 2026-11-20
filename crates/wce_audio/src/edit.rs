//! Edits of the game's sound data: replace a sound's audio, add or remove a variation in a container, change a property. An
//! [`EditSet`] holds the edited copies of the banks and the loose files to write; the library stays as the game has it.
//!
//! The game's rules kept (docs/formats/wwise_bank.md): media in a bank's `DATA` at 16-byte boundaries; a source held in a bank has
//! `file_id` = that bank's ID, `file_offset` = the media's offset from the start of that bank's file and `in_memory_size` = its size;
//! a streamed source (`audio/<id>.wem`) has `file_id` = its media ID and `in_memory_size` 0. After any change of a bank's media,
//! every source pointing into it is set again ([`EditSet::fix_offsets`]).

use std::collections::{BTreeMap, BTreeSet};

use wce_audio_format::amb::{Ambience, Emitter};
use wce_audio_format::bank::{Bank, HircObject};
use wce_audio_format::fnv;
use wce_audio_format::hirc::{self, Object, Source};
use wce_audio_format::triggers::Triggers;

use crate::library::SoundLibrary;
use crate::{AudioError, Result};

/// Edited banks (by library index) and loose files (by path in the packs).
#[derive(Clone, Default)]
pub struct EditSet {
    pub banks: BTreeMap<usize, Bank>,
    pub loose: BTreeMap<String, Vec<u8>>,
    /// Names given to new objects (new events), by ID.
    pub names: BTreeMap<u32, String>,
}

impl EditSet {
    pub fn is_empty(&self) -> bool {
        self.banks.is_empty() && self.loose.is_empty() && self.names.is_empty()
    }

    /// A bank as edited so far.
    pub fn bank<'a>(&'a self, lib: &'a SoundLibrary, index: usize) -> &'a Bank {
        self.banks.get(&index).unwrap_or(&lib.banks[index].bank)
    }

    pub(crate) fn bank_mut(&mut self, lib: &SoundLibrary, index: usize) -> &mut Bank {
        self.banks.entry(index).or_insert_with(|| lib.banks[index].bank.clone())
    }

    /// Where an object is, as edited: (bank, index in its objects).
    pub(crate) fn find(&self, lib: &SoundLibrary, id: u32) -> Option<(usize, usize)> {
        for (b, bank) in &self.banks {
            if let Some(i) = bank.objects().iter().position(|o| o.id == id) {
                return Some((*b, i));
            }
        }
        lib.locations(id).iter().copied().find(|(b, _)| !self.banks.contains_key(b))
    }

    /// An object's typed view, as edited.
    pub fn object(&self, lib: &SoundLibrary, id: u32) -> Option<Object> {
        let (b, i) = self.find(lib, id)?;
        let bank = self.bank(lib, b);
        let o = &bank.objects()[i];
        hirc::parse(lib.banks[b].version, o.kind, &o.body).and_then(|r| r.ok())
    }

    /// An object as edited, only when it is in an edited bank (an object the edits changed, made, or whose bank they touched).
    pub fn edited_object(&self, lib: &SoundLibrary, id: u32) -> Option<Object> {
        self.banks.values().any(|bank| bank.objects().iter().any(|o| o.id == id)).then(|| self.object(lib, id)).flatten()
    }

    pub(crate) fn put_object(&mut self, lib: &SoundLibrary, id: u32, object: &Object) -> Result<()> {
        self.put_object_copies(lib, id, object).map(|_| ())
    }

    /// Every bank holding an object, with its index there, as edited.
    pub(crate) fn copies(&self, lib: &SoundLibrary, id: u32) -> Vec<(usize, usize)> {
        let mut banks: Vec<usize> = lib.locations(id).iter().map(|(b, _)| *b).chain(self.banks.keys().copied()).collect();
        banks.sort_unstable();
        banks.dedup();
        banks.into_iter().filter_map(|b| self.bank(lib, b).objects().iter().position(|o| o.id == id).map(|i| (b, i))).collect()
    }

    /// Writes an object as edited into its first copy and every copy the same as it (a voice set's actors can be in two banks
    /// whole; the game takes whichever loads first). Copies that differ (partial copies of a switch above a voice bank's own,
    /// holding only its branch) are left as they are. Returns the banks written, the first copy's first.
    pub(crate) fn put_object_copies(&mut self, lib: &SoundLibrary, id: u32, object: &Object) -> Result<Vec<usize>> {
        let (b0, i0) = self.find(lib, id).ok_or_else(|| AudioError::Invalid(format!("#{id} is in no bank")))?;
        let original = self.bank(lib, b0).objects()[i0].body.clone();
        let body = object.write();
        let mut written = vec![b0];
        self.bank_mut(lib, b0).objects_mut()[i0].body = body.clone();
        for (b, i) in self.copies(lib, id) {
            if b != b0 && self.bank(lib, b).objects()[i].body == original {
                self.bank_mut(lib, b).objects_mut()[i].body = body.clone();
                written.push(b);
            }
        }
        Ok(written)
    }

    /// Puts new objects into each of `banks`, right before `parent` there (children before their parent, as in the game's banks).
    pub(crate) fn insert_before(&mut self, lib: &SoundLibrary, banks: &[usize], parent: u32, objects: &[HircObject]) {
        for &b in banks {
            let bank = self.bank_mut(lib, b);
            let at = bank.objects().iter().position(|o| o.id == parent).unwrap_or(bank.objects().len());
            for (k, o) in objects.iter().enumerate() {
                bank.objects_mut().insert(at + k, o.clone());
            }
        }
    }

    /// Sets again the offset and size of every source held in the banks whose media changed (`media_banks`: library indices).
    pub fn fix_offsets(&mut self, lib: &SoundLibrary, media_banks: &[usize]) {
        let mut layout = BTreeMap::new();
        for &m in media_banks {
            let bank = self.bank(lib, m);
            let (Some(header), Some(data_at)) = (bank.header(), bank.data_position()) else { continue };
            let entries: BTreeMap<u32, (u32, u32)> =
                bank.media_index().iter().map(|e| (e.id, (data_at as u32 + e.offset, e.size))).collect();
            layout.insert(header.bank_id, entries);
        }

        for b in 0..lib.banks.len() {
            let version = lib.banks[b].version;
            let mut changed = vec![];
            for (i, o) in self.bank(lib, b).objects().iter().enumerate() {
                if o.kind != 2 && o.kind != 11 {
                    continue;
                }
                let Some(Ok(mut object)) = hirc::parse(version, o.kind, &o.body) else { continue };
                let sources: Vec<&mut Source> = match &mut object {
                    Object::Sound(s) => vec![&mut s.source],
                    Object::MusicTrack(t) => t.sources.iter_mut().collect(),
                    _ => vec![],
                };
                let mut any = false;
                for s in sources {
                    if s.stream_type == 2 {
                        continue;
                    }
                    let Some((offset, size)) = layout.get(&s.file_id).and_then(|m| m.get(&s.source_id)).copied() else { continue };
                    if s.file_offset != Some(offset) || (s.stream_type == 0 && s.in_memory_size != size) {
                        s.file_offset = Some(offset);
                        if s.stream_type == 0 {
                            s.in_memory_size = size;
                        }
                        any = true;
                    }
                }
                if any {
                    changed.push((i, object.write()));
                }
            }
            if !changed.is_empty() {
                let bank = self.bank_mut(lib, b);
                for (i, body) in changed {
                    bank.objects_mut()[i].body = body;
                }
            }
        }
    }

    /// Replaces the audio of a media file (a WEM) wherever the game has it: in banks' `DATA` and as a loose file. A prefetched start
    /// in a bank keeps its length (cut from the new file). Note: the game does not take edited data banks from a mod pack (see
    /// [`EditSet::redirect_media`]); the tool uses [`EditSet::replace_audio`].
    ///
    /// # Errors
    ///
    /// When no bank and no source has this media.
    pub fn replace_media(&mut self, lib: &SoundLibrary, files: &dyn wce_vfs::files::Files, media_id: u32, wem: &[u8]) -> Result<()> {
        let path = format!("audio/{media_id}.wem");
        let loose = files.exists(&path) || self.loose.contains_key(&path);
        let holders: Vec<usize> = (0..lib.banks.len()).filter(|&b| self.bank(lib, b).media(media_id).is_some()).collect();
        if holders.is_empty() && !loose {
            return Err(AudioError::Invalid(format!("media #{media_id} is in no bank and no pack")));
        }

        for &b in &holders {
            let old = self.bank(lib, b).media(media_id).map(<[u8]>::len).unwrap_or(0);
            let riff = |m: &[u8]| m.get(4..8).map_or(0, |s| u32::from_le_bytes(s.try_into().expect("4")) as usize + 8);
            let prefetch = loose && riff(self.bank(lib, b).media(media_id).unwrap_or_default()) > old;
            let bytes = if prefetch { &wem[..old.min(wem.len())] } else { wem };
            self.bank_mut(lib, b).set_media(media_id, bytes);
        }
        if loose {
            self.loose.insert(path, wem.to_vec());
        }

        self.fix_offsets(lib, &holders);
        Ok(())
    }

    /// Replaces a sound's audio without touching the banks that hold media (`*_data.bnk`): every source using `media_id` is pointed
    /// at a new media ID, streamed from a loose `audio/<new id>.wem`. Returns the new ID.
    ///
    /// The game takes in-bank media from its own data banks even when a mod pack ships edited copies (a replace inside
    /// `battle_ui_data` / `global_ui_data` played the original sounds, 2026-11-19), while loose WEMs from a mod pack do play.
    ///
    /// # Errors
    ///
    /// When no source uses this media.
    pub fn redirect_media(&mut self, lib: &SoundLibrary, media_id: u32, wem: &[u8]) -> Result<u32> {
        let new_id = self.new_id(lib, media_id);
        let mut found = false;
        for b in 0..lib.banks.len() {
            let version = lib.banks[b].version;
            let mut changed = vec![];
            for (i, o) in self.bank(lib, b).objects().iter().enumerate() {
                if o.kind != 2 && o.kind != 11 {
                    continue;
                }
                let Some(Ok(mut object)) = hirc::parse(version, o.kind, &o.body) else { continue };
                let sources: Vec<&mut Source> = match &mut object {
                    Object::Sound(s) => vec![&mut s.source],
                    Object::MusicTrack(t) => t.sources.iter_mut().collect(),
                    _ => vec![],
                };
                let mut any = false;
                for s in sources.into_iter().filter(|s| s.source_id == media_id) {
                    s.source_id = new_id;
                    s.stream_type = 2;
                    s.file_id = new_id;
                    s.file_offset = None;
                    s.in_memory_size = 0;
                    any = true;
                }
                if any {
                    changed.push((i, object.write()));
                }
            }
            if !changed.is_empty() {
                found = true;
                let bank = self.bank_mut(lib, b);
                for (i, body) in changed {
                    bank.objects_mut()[i].body = body;
                }
            }
        }
        if !found {
            return Err(AudioError::Invalid(format!("no sound uses media #{media_id}")));
        }
        self.loose.insert(format!("audio/{new_id}.wem"), wem.to_vec());
        Ok(new_id)
    }

    /// Replaces a sound's audio the way the game takes it: a streamed (loose) media file is written again under its ID; media held
    /// in a data bank is redirected to a new loose file ([`EditSet::redirect_media`]). Returns the ID now playing.
    ///
    /// # Errors
    ///
    /// When no source or file has this media.
    pub fn replace_audio(&mut self, lib: &SoundLibrary, files: &dyn wce_vfs::files::Files, media_id: u32, wem: &[u8]) -> Result<u32> {
        let path = format!("audio/{media_id}.wem");
        let in_banks = (0..lib.banks.len()).any(|b| self.bank(lib, b).media(media_id).is_some());
        if !in_banks && (files.exists(&path) || self.loose.contains_key(&path)) {
            self.loose.insert(path, wem.to_vec());
            return Ok(media_id);
        }
        self.redirect_media(lib, media_id, wem)
    }

    /// An unused ID for a new object or media file.
    pub(crate) fn new_id(&self, lib: &SoundLibrary, salt: u32) -> u32 {
        let mut id = (std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_nanos() as u32) ^ salt) | 1;
        loop {
            id = (id.wrapping_mul(2_654_435_761) ^ (id >> 15)) & 0x3FFF_FFFF;
            let used = lib.has_object(id)
                || lib.has_media(id)
                || self.find(lib, id).is_some()
                || self.loose.contains_key(&format!("audio/{id}.wem"));
            if id > 1000 && !used {
                return id;
            }
        }
    }

    /// Adds a variation to a random/sequence container: a new sound made like the container's first sound child, playing `wem`
    /// streamed from a loose file. Returns the new sound's ID.
    ///
    /// # Errors
    ///
    /// When the object is not a random/sequence container or has no sound child to copy.
    pub fn add_variation(&mut self, lib: &SoundLibrary, container_id: u32, wem: &[u8]) -> Result<u32> {
        let Some(Object::RandomSequence(mut container)) = self.object(lib, container_id) else {
            return Err(AudioError::Invalid(format!("#{container_id} is not a random/sequence container")));
        };
        let (template_id, template) = container
            .children
            .iter()
            .find_map(|c| match self.object(lib, *c) {
                Some(Object::Sound(s)) => Some((*c, s)),
                _ => None,
            })
            .ok_or_else(|| AudioError::Invalid("the container has no sound to copy".into()))?;

        let sound_id = self.new_id(lib, container_id);
        let media_id = self.new_id(lib, sound_id);
        let mut sound = template.clone();
        sound.source.source_id = media_id;
        sound.node.parent_id = container_id;
        // Streamed from a loose file: the game does not take edited data banks from a mod pack (see `redirect_media`).
        sound.source.stream_type = 2;
        sound.source.file_id = media_id;
        sound.source.file_offset = None;
        sound.source.in_memory_size = 0;
        self.loose.insert(format!("audio/{media_id}.wem"), wem.to_vec());

        // The new sound right after the one it copies (children before their parent, as in the game's banks).
        let (b, i) = self.find(lib, template_id).ok_or_else(|| AudioError::Invalid("the copied sound is in no bank".into()))?;
        let body = Object::Sound(sound).write();
        self.bank_mut(lib, b).objects_mut().insert(i + 1, wce_audio_format::bank::HircObject { kind: 2, id: sound_id, body });

        let new_sound = self.bank(lib, b).objects()[i + 1].clone();
        let at = container.children.partition_point(|c| *c < sound_id);
        container.children.insert(at, sound_id);
        if !container.playlist.is_empty() || container.children.len() == 1 {
            container.playlist.push((sound_id, 50_000));
        }
        // The container's other whole copies gain the sound too, in their banks.
        let banks = self.put_object_copies(lib, container_id, &Object::RandomSequence(container))?;
        let others: Vec<usize> = banks.into_iter().filter(|x| *x != b).collect();
        self.insert_before(lib, &others, container_id, &[new_sound]);
        Ok(sound_id)
    }

    /// Sets (or removes, `None`) one of an action's properties: 14 its delay, 15 its fade time (seconds), 16 its probability (%).
    ///
    /// # Errors
    ///
    /// When the object is not an action or its parameters do not read.
    pub fn set_action_property(&mut self, lib: &SoundLibrary, id: u32, property: u8, value: Option<f32>) -> Result<()> {
        let Some(Object::Action(mut action)) = self.object(lib, id) else {
            return Err(AudioError::Invalid(format!("#{id} is not an action")));
        };
        let mut parts = action.parts().ok_or_else(|| AudioError::Invalid(format!("#{id}'s parameters do not read")))?;
        match (parts.props.iter().position(|(p, _)| *p == property), value) {
            (Some(k), Some(v)) => parts.props[k].1 = v,
            (Some(k), None) => {
                parts.props.remove(k);
            }
            (None, Some(v)) => {
                let at = parts.props.partition_point(|(p, _)| *p < property);
                parts.props.insert(at, (property, v));
            }
            (None, None) => return Ok(()),
        }
        action.set_parts(&parts);
        self.put_object(lib, id, &Object::Action(action))
    }

    /// Sets how a random/sequence container plays: `mode` 0 sequence, 1 random; `random_mode` 0 normal, 1 shuffle (no take again
    /// before all played); how many recent takes random play avoids; and one child's weight (1/1000 %, 50,000 the default).
    ///
    /// # Errors
    ///
    /// When the object is not such a container or the child is not one of its children.
    pub fn set_container(
        &mut self,
        lib: &SoundLibrary,
        container_id: u32,
        mode: u8,
        random_mode: u8,
        avoid_repeat: u16,
        weight: Option<(u32, i32)>,
    ) -> Result<()> {
        let Some(Object::RandomSequence(mut container)) = self.object(lib, container_id) else {
            return Err(AudioError::Invalid(format!("#{container_id} is not a random/sequence container")));
        };
        container.mode = mode.min(1);
        container.random_mode = random_mode.min(1);
        container.avoid_repeat_count = avoid_repeat.min(container.children.len().saturating_sub(1) as u16);
        if let Some((child, w)) = weight {
            if !container.children.contains(&child) {
                return Err(AudioError::Invalid(format!("#{child} is not a child of #{container_id}")));
            }
            match container.playlist.iter_mut().find(|(c, _)| *c == child) {
                Some((_, old)) => *old = w.clamp(0, 100_000),
                None => container.playlist.push((child, w.clamp(0, 100_000))),
            }
        }
        self.put_object(lib, container_id, &Object::RandomSequence(container))
    }

    /// Removes a child from a random/sequence container, and the sound object itself when it was a sound of that container; its
    /// media is dropped from the bank when no other source uses it.
    ///
    /// # Errors
    ///
    /// When the object is not such a container or the child is not one of its children.
    pub fn remove_variation(&mut self, lib: &SoundLibrary, container_id: u32, child_id: u32) -> Result<()> {
        let Some(Object::RandomSequence(mut container)) = self.object(lib, container_id) else {
            return Err(AudioError::Invalid(format!("#{container_id} is not a random/sequence container")));
        };
        if !container.children.contains(&child_id) {
            return Err(AudioError::Invalid(format!("#{child_id} is not a child of #{container_id}")));
        }
        container.children.retain(|c| *c != child_id);
        container.playlist.retain(|(c, _)| *c != child_id);
        self.put_object(lib, container_id, &Object::RandomSequence(container))?;

        if let Some(Object::Sound(sound)) = self.object(lib, child_id)
            && sound.node.parent_id == container_id
        {
            let (b, i) = self.find(lib, child_id).expect("found above");
            self.bank_mut(lib, b).objects_mut().remove(i);
            let media = sound.source.source_id;
            let still_used = (0..lib.banks.len()).any(|bank| {
                self.bank(lib, bank).objects().iter().any(|o| {
                    o.kind == 2
                        && matches!(hirc::parse(lib.banks[bank].version, 2, &o.body), Some(Ok(Object::Sound(s))) if s.source.source_id == media)
                })
            });
            if !still_used && sound.source.stream_type == 0 {
                let holders: Vec<usize> = (0..lib.banks.len()).filter(|&m| self.bank(lib, m).media(media).is_some()).collect();
                for &m in &holders {
                    self.bank_mut(lib, m).remove_media(media);
                }
                self.fix_offsets(lib, &holders);
            }
        }
        Ok(())
    }

    /// Sets (or adds) a property of an object's node parameters (0 volume dB, 2 pitch cents, 3 low-pass...), or removes it (`None`).
    ///
    /// # Errors
    ///
    /// When the object has no node parameters.
    pub fn set_property(&mut self, lib: &SoundLibrary, id: u32, property: u8, value: Option<f32>) -> Result<()> {
        let mut object = self.object(lib, id).ok_or_else(|| AudioError::Invalid(format!("#{id} is not readable")))?;
        let node = match &mut object {
            Object::Sound(o) => &mut o.node,
            Object::RandomSequence(o) => &mut o.node,
            Object::Switch(o) => &mut o.node,
            Object::ActorMixer(o) => &mut o.node,
            Object::Blend(o) => &mut o.node,
            _ => return Err(AudioError::Invalid(format!("#{id} has no properties to set"))),
        };
        match (node.props.iter().position(|(p, _)| *p == property), value) {
            (Some(k), Some(v)) => node.props[k].1 = v,
            (Some(k), None) => {
                node.props.remove(k);
            }
            (None, Some(v)) => {
                let at = node.props.partition_point(|(p, _)| *p < property);
                node.props.insert(at, (property, v));
            }
            (None, None) => return Ok(()),
        }
        self.put_object(lib, id, &object)
    }

    /// A new sound made like `template` (a sound object), playing `wem` streamed from a loose file, placed under the nearest
    /// actor-mixer above the template (so no random or switch container gains a variation), or on its own, sent to the chain's
    /// output bus, when there is none. Returns the new sound's ID.
    ///
    /// # Errors
    ///
    /// When the template is not a sound.
    pub fn new_sound_like(&mut self, lib: &SoundLibrary, template: u32, wem: &[u8]) -> Result<u32> {
        let Some(Object::Sound(mut sound)) = self.object(lib, template) else {
            return Err(AudioError::Invalid(format!("#{template} is not a sound")));
        };
        let mut parent = sound.node.parent_id;
        while let Some(o) = self.object(lib, parent) {
            if matches!(o, Object::ActorMixer(_)) {
                break;
            }
            parent = o.node().map_or(0, |n| n.parent_id);
        }
        // No actor-mixer above: the new sound stands alone, sent to the output bus of the template's chain.
        let mixer = match self.object(lib, parent) {
            Some(Object::ActorMixer(m)) => Some(m),
            _ => None,
        };
        if mixer.is_none() {
            let mut bus = sound.node.override_bus_id;
            let mut up = sound.node.parent_id;
            while bus == 0
                && let Some(o) = self.object(lib, up)
            {
                let node = o.node().cloned();
                bus = node.as_ref().map_or(0, |n| n.override_bus_id);
                up = node.map_or(0, |n| n.parent_id);
            }
            sound.node.override_bus_id = bus;
            parent = 0;
        }

        let sound_id = self.new_id(lib, template);
        let media = self.new_id(lib, sound_id);
        sound.node.parent_id = parent;
        sound.source.source_id = media;
        sound.source.stream_type = 2;
        sound.source.file_id = media;
        sound.source.file_offset = None;
        sound.source.in_memory_size = 0;
        self.loose.insert(format!("audio/{media}.wem"), wem.to_vec());

        let (b, i) = self.find(lib, template).ok_or_else(|| AudioError::Invalid("the template is in no bank".into()))?;
        self.bank_mut(lib, b).objects_mut().insert(i + 1, HircObject { kind: 2, id: sound_id, body: Object::Sound(sound).write() });
        if let Some(mut mixer) = mixer {
            let at = mixer.children.partition_point(|c| *c < sound_id);
            mixer.children.insert(at, sound_id);
            self.put_object(lib, parent, &Object::ActorMixer(mixer))?;
        }
        Ok(sound_id)
    }

    /// Deletes an object: an event with its actions, an action, or a hierarchy object with everything under it (its children,
    /// recursively) and the Play actions that target any of them. Every list naming a deleted object (parents' children and
    /// playlists, switch assignments, blend layers, events' actions) drops it. Media stay where they are (the game's data banks and
    /// packs are not changed; a loose file made by these edits and no longer used is dropped). Returns the deleted IDs.
    ///
    /// # Errors
    ///
    /// For an object in no bank, or a music object (step 6 of the plan).
    pub fn delete_object(&mut self, lib: &SoundLibrary, id: u32) -> Result<Vec<u32>> {
        let root = self.object(lib, id).ok_or_else(|| AudioError::Invalid(format!("#{id} is in no bank or not readable")))?;
        if matches!(root, Object::MusicSegment(_) | Object::MusicTrack(_) | Object::MusicSwitch(_) | Object::MusicPlaylist(_)) {
            return Err(AudioError::Unsupported("deleting music objects (step 6 of the plan)".into()));
        }

        // What goes: the object, its subtree, an event's actions, the Play actions on any of these.
        let mut doomed = BTreeSet::from([id]);
        let mut queue = vec![id];
        while let Some(next) = queue.pop() {
            match self.object(lib, next) {
                Some(Object::Event(e)) => {
                    for a in e.actions {
                        if doomed.insert(a) {
                            queue.push(a);
                        }
                    }
                }
                Some(o) => {
                    for c in o.children().to_vec() {
                        let owned = self.object(lib, c).and_then(|co| co.node().map(|n| n.parent_id == next)).unwrap_or(false);
                        if owned && doomed.insert(c) {
                            queue.push(c);
                        }
                    }
                }
                None => {}
            }
        }
        for b in 0..lib.banks.len() {
            for o in self.bank(lib, b).objects() {
                if o.kind == 3
                    && let Some(Ok(Object::Action(a))) = hirc::parse(lib.banks[b].version, 3, &o.body)
                    && a.action_type >> 8 == 0x04
                    && doomed.contains(&a.target_id)
                {
                    doomed.insert(o.id);
                }
            }
        }

        let media: Vec<u32> = doomed
            .iter()
            .filter_map(|d| match self.object(lib, *d) {
                Some(Object::Sound(s)) => Some(s.source.source_id),
                _ => None,
            })
            .collect();

        for b in 0..lib.banks.len() {
            let version = lib.banks[b].version;
            let mut objects = vec![];
            let mut changed = false;
            for o in self.bank(lib, b).objects() {
                if doomed.contains(&o.id) {
                    changed = true;
                    continue;
                }
                let mut o = o.clone();
                if matches!(o.kind, 4..=7 | 9)
                    && let Some(Ok(mut object)) = hirc::parse(version, o.kind, &o.body)
                    && drop_ids(&mut object, &doomed)
                {
                    o.body = object.write();
                    changed = true;
                }
                objects.push(o);
            }
            if changed {
                *self.bank_mut(lib, b).objects_mut() = objects;
            }
        }

        for m in media {
            let still = (0..lib.banks.len()).any(|b| {
                self.bank(lib, b).objects().iter().any(|o| {
                    o.kind == 2
                        && matches!(hirc::parse(lib.banks[b].version, 2, &o.body), Some(Ok(Object::Sound(s))) if s.source.source_id == m)
                })
            });
            if !still {
                self.loose.remove(&format!("audio/{m}.wem"));
            }
        }
        for d in &doomed {
            self.names.remove(d);
        }
        Ok(doomed.into_iter().collect())
    }

    /// Adds a named event to a bank, with a Play action on `target` (an object of that bank), made like a Play action the bank
    /// already has (its parameters copied). Returns the event's ID (the FNV-1 of the name).
    ///
    /// # Errors
    ///
    /// When the name is not letters, digits and `_` or is taken, the target is not in the bank, or the bank has no Play action.
    pub fn new_event(&mut self, lib: &SoundLibrary, bank: usize, name: &str, target: u32) -> Result<u32> {
        let name = name.trim();
        if name.is_empty() || !name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
            return Err(AudioError::Invalid("an event name is letters, digits and _".into()));
        }
        let id = fnv::id(name);
        if lib.has_object(id) || self.find(lib, id).is_some() {
            return Err(AudioError::Invalid(format!("{name} (#{id}) already exists")));
        }
        let version = lib.banks[bank].version;
        let objects = self.bank(lib, bank).objects();
        if !objects.iter().any(|o| o.id == target) {
            return Err(AudioError::Invalid(format!("#{target} is not in {}", lib.banks[bank].name)));
        }
        let template = objects
            .iter()
            .find_map(|o| match hirc::parse(version, o.kind, &o.body) {
                Some(Ok(Object::Action(a))) if a.action_type >> 8 == 0x04 && objects.iter().any(|t| t.id == a.target_id) => Some(a),
                _ => None,
            })
            .ok_or_else(|| AudioError::Invalid(format!("{} has no Play action to copy", lib.banks[bank].name)))?;

        let action_id = self.new_id(lib, id);
        let mut action = template;
        action.target_id = target;
        let action = HircObject { kind: 3, id: action_id, body: Object::Action(action).write() };
        let event = HircObject { kind: 4, id, body: Object::Event(hirc::Event { actions: vec![action_id] }).write() };
        let objects = self.bank_mut(lib, bank).objects_mut();
        objects.push(action);
        objects.push(event);
        self.names.insert(id, name.to_owned());
        Ok(id)
    }

    /// Adds a row to `sound_animation_triggers.csv` firing events in battle and on the campaign map; the other columns (the
    /// conditions: ground, entity, armour...) are copied from row `like`. Returns the new row's index, which a `.snd.meta`
    /// `SOUND_TRIGGER` holds.
    ///
    /// # Errors
    ///
    /// When the CSV can't be read or has no row `like`.
    pub fn add_trigger(
        &mut self,
        files: &dyn wce_vfs::files::Files,
        name: &str,
        battle: &str,
        campaign: &str,
        like: usize,
    ) -> Result<usize> {
        const PATH: &str = "audio/sound_animation_triggers.csv";
        let bytes = match self.loose.get(PATH) {
            Some(b) => b.clone(),
            None => files.read(PATH).map_err(AudioError::Invalid)?,
        };
        let triggers = Triggers::read(&bytes)?;
        let mut cells = triggers.rows.get(like).cloned().ok_or_else(|| AudioError::Invalid(format!("no trigger row {like}")))?;
        cells[0] = name.to_owned();
        for (column, value) in [
            ("sound_event_name_battle", battle),
            ("sound_event_name_battle_end", ""),
            ("sound_event_name_campaign", campaign),
            ("sound_event_name_campaign_end", ""),
        ] {
            if let Some(c) = triggers.column(column) {
                cells[c] = value.to_owned();
            }
        }
        let quote = |c: &String| if c.contains(',') || c.contains('"') { format!("\"{}\"", c.replace('"', "\"\"")) } else { c.clone() };
        let newline: &[u8] = if bytes.windows(2).any(|w| w == b"\r\n") { b"\r\n" } else { b"\n" };
        let mut out = bytes.clone();
        if !out.ends_with(b"\n") {
            out.extend_from_slice(newline);
        }
        out.extend_from_slice(cells.iter().map(quote).collect::<Vec<_>>().join(",").as_bytes());
        out.extend_from_slice(newline);
        self.loose.insert(PATH.to_owned(), out);
        Ok(triggers.rows.len())
    }

    /// Adds a campaign ambience emitter playing `event_id` along `points` (campaign map coordinates) to an `.amb` file.
    ///
    /// # Errors
    ///
    /// When the file can't be read.
    pub fn add_emitter(&mut self, files: &dyn wce_vfs::files::Files, path: &str, event_id: u32, points: Vec<[f32; 3]>) -> Result<()> {
        let bytes = match self.loose.get(path) {
            Some(b) => b.clone(),
            None => files.read(path).map_err(AudioError::Invalid)?,
        };
        let mut amb = Ambience::read(&bytes)?;
        amb.emitters.push(Emitter { event_id, points });
        self.loose.insert(path.to_owned(), amb.write());
        Ok(())
    }
}

/// Drops deleted IDs from an object's lists; true if it changed.
fn drop_ids(object: &mut Object, doomed: &BTreeSet<u32>) -> bool {
    let gone = |id: &u32| doomed.contains(id);
    let before = object.write();
    match object {
        Object::Event(e) => e.actions.retain(|a| !gone(a)),
        Object::RandomSequence(r) => {
            r.children.retain(|c| !gone(c));
            r.playlist.retain(|(c, _)| !gone(c));
        }
        Object::Switch(s) => {
            s.children.retain(|c| !gone(c));
            s.assignments.iter_mut().for_each(|(_, nodes)| nodes.retain(|n| !gone(n)));
            s.child_params.retain(|p| !gone(&p.node_id));
        }
        Object::ActorMixer(a) => a.children.retain(|c| !gone(c)),
        Object::Blend(b) => {
            b.children.retain(|c| !gone(c));
            b.layers.iter_mut().for_each(|l| l.associations.retain(|(c, _)| !gone(c)));
        }
        _ => return false,
    }
    object.write() != before
}
