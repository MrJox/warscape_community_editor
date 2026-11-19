//! Edits of the game's sound data: replace a sound's audio, add or remove a variation in a container, change a property. An
//! [`EditSet`] holds the edited copies of the banks and the loose files to write; the library stays as the game has it.
//!
//! The game's rules kept (docs/formats/wwise_bank.md): media in a bank's `DATA` at 16-byte boundaries; a source held in a bank has
//! `file_id` = that bank's ID, `file_offset` = the media's offset from the start of that bank's file and `in_memory_size` = its size;
//! a streamed source (`audio/<id>.wem`) has `file_id` = its media ID and `in_memory_size` 0. After any change of a bank's media,
//! every source pointing into it is set again ([`EditSet::fix_offsets`]).

use std::collections::BTreeMap;

use wce_audio_format::bank::Bank;
use wce_audio_format::hirc::{self, Object, Source};

use crate::library::SoundLibrary;
use crate::{AudioError, Result};

/// Edited banks (by library index) and loose files (by path in the packs).
#[derive(Clone, Default)]
pub struct EditSet {
    pub banks: BTreeMap<usize, Bank>,
    pub loose: BTreeMap<String, Vec<u8>>,
}

impl EditSet {
    pub fn is_empty(&self) -> bool {
        self.banks.is_empty() && self.loose.is_empty()
    }

    /// A bank as edited so far.
    pub fn bank<'a>(&'a self, lib: &'a SoundLibrary, index: usize) -> &'a Bank {
        self.banks.get(&index).unwrap_or(&lib.banks[index].bank)
    }

    fn bank_mut(&mut self, lib: &SoundLibrary, index: usize) -> &mut Bank {
        self.banks.entry(index).or_insert_with(|| lib.banks[index].bank.clone())
    }

    /// Where an object is, as edited: (bank, index in its objects).
    fn find(&self, lib: &SoundLibrary, id: u32) -> Option<(usize, usize)> {
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

    fn put_object(&mut self, lib: &SoundLibrary, id: u32, object: &Object) -> Result<()> {
        let (b, i) = self.find(lib, id).ok_or_else(|| AudioError::Invalid(format!("#{id} is in no bank")))?;
        self.bank_mut(lib, b).objects_mut()[i].body = object.write();
        Ok(())
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
    fn new_id(&self, lib: &SoundLibrary, salt: u32) -> u32 {
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

        let at = container.children.partition_point(|c| *c < sound_id);
        container.children.insert(at, sound_id);
        if !container.playlist.is_empty() || container.children.len() == 1 {
            container.playlist.push((sound_id, 50_000));
        }
        self.put_object(lib, container_id, &Object::RandomSequence(container))?;
        Ok(sound_id)
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
}
