//! Every bank the game sees, indexed: objects by ID across banks, media by ID, names. [`SoundLibrary::resolve`] turns an event (or
//! any object) into the tree of what it plays: actions, containers, switches, dialogue trees, music, down to each sound's media.

use std::collections::HashMap;

use wce_audio_format::bank::{Bank, HircObject, kind_name};
use wce_audio_format::hirc::{self, Object, Source};
use wce_vfs::files::Files;

use crate::names::{NameSource, Names};

/// One bank file of the game.
pub struct BankEntry {
    pub path: String,
    /// The file name without `.bnk`.
    pub name: String,
    pub bank: Bank,
    pub version: u32,
    /// The typed view of each object (`None`: not typed for this version or kind, or it failed to parse).
    pub typed: Vec<Option<Object>>,
}

/// Where a sound's media is.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MediaRef {
    /// Inside a bank's `DATA`.
    InBank { bank: usize, id: u32 },
    /// A loose file of the packs (`audio/<id>.wem`).
    Loose { path: String, id: u32 },
    /// Neither found.
    Missing { id: u32 },
}

impl MediaRef {
    pub fn id(&self) -> u32 {
        match self {
            MediaRef::InBank { id, .. } | MediaRef::Loose { id, .. } | MediaRef::Missing { id } => *id,
        }
    }
}

/// A node of a "what plays" tree.
#[derive(Clone, Debug)]
pub struct PlayNode {
    pub id: u32,
    /// The hierarchy kind, 0 for a node that is not an object (a switch value, a media file).
    pub kind: u8,
    pub label: String,
    pub children: Vec<PlayNode>,
    pub media: Option<MediaRef>,
}

#[derive(Default)]
pub struct SoundLibrary {
    pub banks: Vec<BankEntry>,
    pub names: Names,
    /// Object ID -> every (bank, object index) holding it.
    objects: HashMap<u32, Vec<(usize, usize)>>,
    /// Media ID -> the banks holding it in `DATA`.
    media: HashMap<u32, Vec<usize>>,
    /// Bank load failures: (path, why).
    pub failures: Vec<(String, String)>,
}

const MAX_DEPTH: usize = 24;

impl SoundLibrary {
    /// Reads every bank of the files (the game's packs and mods, the project's loose files) and the names.
    pub fn load(files: &dyn Files) -> Self {
        let mut lib = Self { names: Names::from_game(files), ..Self::default() };
        for path in files.list("", ".bnk") {
            let bytes = match files.read(&path) {
                Ok(b) if !b.is_empty() => b,
                Ok(_) => continue,
                Err(e) => {
                    lib.failures.push((path, e));
                    continue;
                }
            };
            match Bank::read(&bytes) {
                Ok(bank) => lib.add_bank(path, bank),
                Err(e) => lib.failures.push((path, e.to_string())),
            }
        }

        lib
    }

    /// Adds a bank (also used for a bank opened from disk).
    pub fn add_bank(&mut self, path: String, bank: Bank) {
        let index = self.banks.len();
        let name = path.rsplit('/').next().unwrap_or(&path).trim_end_matches(".bnk").to_owned();
        let version = bank.header().map_or(0, |h| h.version);
        for c in &bank.chunks {
            if let wce_audio_format::bank::Chunk::BankNames(n) = c {
                for (id, name) in &n.names {
                    self.names.add_id(*id, &String::from_utf8_lossy(name), NameSource::Bank);
                }
            }
        }

        let typed = bank.objects().iter().map(|o| hirc::parse(version, o.kind, &o.body).and_then(Result::ok)).collect();
        for (i, o) in bank.objects().iter().enumerate() {
            self.objects.entry(o.id).or_default().push((index, i));
        }
        for m in bank.media_index() {
            self.media.entry(m.id).or_default().push(index);
        }

        self.banks.push(BankEntry { path, name, bank, version, typed });
    }

    /// An object's first copy: its bank, raw object and typed view.
    pub fn object(&self, id: u32) -> Option<(usize, &HircObject, Option<&Object>)> {
        let &(b, i) = self.objects.get(&id)?.first()?;
        let entry = &self.banks[b];
        Some((b, &entry.bank.objects()[i], entry.typed[i].as_ref()))
    }

    /// An object as the banks have it together: a switch's values, children and child settings, and an event's actions, joined over
    /// every copy (each bank of a voice set holds a partial copy of the switches above its own, with only its branch; a mod's bank
    /// may hold the event again playing its own switch); any other object its first copy.
    pub fn merged(&self, id: u32) -> Option<Object> {
        let mut object = self.object(id)?.2?.clone();
        if let Object::Event(e) = &mut object {
            for &(b, i) in self.locations(id).iter().skip(1) {
                let Some(Object::Event(other)) = &self.banks[b].typed[i] else { continue };
                for a in &other.actions {
                    if !e.actions.contains(a) {
                        e.actions.push(*a);
                    }
                }
            }
        }
        if let Object::Switch(s) = &mut object {
            for &(b, i) in self.locations(id).iter().skip(1) {
                let Some(Object::Switch(other)) = &self.banks[b].typed[i] else { continue };
                for (value, children) in &other.assignments {
                    match s.assignments.iter_mut().find(|(v, _)| v == value) {
                        Some((_, mine)) => {
                            for c in children {
                                if !mine.contains(c) {
                                    mine.push(*c);
                                }
                            }
                        }
                        None => {
                            let at = s.assignments.partition_point(|(v, _)| v < value);
                            s.assignments.insert(at, (*value, children.clone()));
                        }
                    }
                }
                for c in &other.children {
                    if let Err(at) = s.children.binary_search(c) {
                        s.children.insert(at, *c);
                    }
                }
                for p in &other.child_params {
                    if !s.child_params.iter().any(|q| q.node_id == p.node_id) {
                        let at = s.child_params.partition_point(|q| q.node_id < p.node_id);
                        s.child_params.insert(at, p.clone());
                    }
                }
            }
        }
        Some(object)
    }

    /// If any bank has an object with this ID.
    pub fn has_object(&self, id: u32) -> bool {
        self.objects.contains_key(&id)
    }

    /// If any bank holds media with this ID.
    pub fn has_media(&self, id: u32) -> bool {
        self.media.contains_key(&id)
    }

    /// The bank with this bank ID (its `BKHD` ID).
    pub fn bank_by_id(&self, bank_id: u32) -> Option<usize> {
        self.banks.iter().position(|b| b.bank.header().is_some_and(|h| h.bank_id == bank_id))
    }

    /// Every (bank, object index) holding an object.
    pub fn locations(&self, id: u32) -> &[(usize, usize)] {
        self.objects.get(&id).map_or(&[], Vec::as_slice)
    }

    /// Every bank holding an object.
    pub fn banks_of(&self, id: u32) -> Vec<usize> {
        self.objects.get(&id).map(|v| v.iter().map(|(b, _)| *b).collect()).unwrap_or_default()
    }

    /// A readable name: the name if known, else the kind and the ID.
    pub fn label(&self, id: u32, kind: u8) -> String {
        match self.names.get(id) {
            Some(name) => name.to_owned(),
            None if kind == 0 => format!("#{id}"),
            None => format!("{} #{id}", kind_name(kind)),
        }
    }

    /// Where a sound source's media is.
    pub fn media_of(&self, source: &Source, files: &dyn Files) -> MediaRef {
        let id = source.source_id;
        if source.stream_type == 0
            && let Some(&bank) = self.media.get(&id).and_then(|v| v.first())
        {
            return MediaRef::InBank { bank, id };
        }

        let path = format!("audio/{id}.wem");
        if files.exists(&path) {
            return MediaRef::Loose { path, id };
        }

        match self.media.get(&id).and_then(|v| v.first()) {
            Some(&bank) => MediaRef::InBank { bank, id },
            None => MediaRef::Missing { id },
        }
    }

    /// The bytes of a media file.
    pub fn media_bytes(&self, media: &MediaRef, files: &dyn Files) -> Option<Vec<u8>> {
        match media {
            MediaRef::InBank { bank, id } => self.banks.get(*bank)?.bank.media(*id).map(<[u8]>::to_vec),
            MediaRef::Loose { path, .. } => files.read(path).ok(),
            MediaRef::Missing { .. } => None,
        }
    }

    /// The tree of what an object plays.
    pub fn resolve(&self, id: u32, files: &dyn Files) -> PlayNode {
        self.resolve_edited(id, files, &|_| None)
    }

    /// As [`Self::resolve`], with objects as edited where `edited` has them (unsaved edits shown in the trees).
    pub fn resolve_edited(&self, id: u32, files: &dyn Files, edited: &dyn Fn(u32) -> Option<Object>) -> PlayNode {
        let mut stack = vec![];
        self.resolve_inner(id, files, edited, &mut stack)
    }

    fn resolve_inner(&self, id: u32, files: &dyn Files, edited: &dyn Fn(u32) -> Option<Object>, stack: &mut Vec<u32>) -> PlayNode {
        let owned = edited(id).or_else(|| self.merged(id));
        let kind = owned.as_ref().map(Object::kind).or_else(|| self.object(id).map(|(_, raw, _)| raw.kind));
        let Some(kind) = kind else {
            return PlayNode { id, kind: 0, label: format!("{} (not in any bank)", self.label(id, 0)), children: vec![], media: None };
        };
        let typed = owned.as_ref();
        let mut node = PlayNode { id, kind, label: self.label(id, kind), children: vec![], media: None };
        if stack.contains(&id) || stack.len() >= MAX_DEPTH {
            node.label.push_str(" (repeats)");
            return node;
        }

        stack.push(id);
        let child = |id: u32, stack: &mut Vec<u32>| self.resolve_inner(id, files, edited, stack);
        match typed {
            Some(Object::Event(e)) => node.children = e.actions.iter().map(|a| child(*a, stack)).collect(),
            Some(Object::Action(a)) => {
                let target_kind = self.object(a.target_id).map_or(0, |(_, o, _)| o.kind);
                node.label = format!("{} {}", action_name(a.action_type), self.label(a.target_id, target_kind));
                if a.action_type >> 8 == 0x04 {
                    node.children.push(child(a.target_id, stack));
                }
            }
            Some(Object::Sound(s)) => node.media = Some(self.media_of(&s.source, files)),
            Some(Object::Switch(s)) => {
                for (value, nodes) in &s.assignments {
                    let mut group = PlayNode { id: *value, kind: 0, label: self.label(*value, 0), children: vec![], media: None };
                    group.children = nodes.iter().map(|n| child(*n, stack)).collect();
                    node.children.push(group);
                }
            }
            Some(Object::DialogueEvent(d)) => node.children = d.leaves().iter().map(|n| child(*n, stack)).collect(),
            Some(Object::MusicTrack(t)) => {
                node.children = t
                    .sources
                    .iter()
                    .map(|s| PlayNode {
                        id: s.source_id,
                        kind: 0,
                        label: format!("source #{}", s.source_id),
                        children: vec![],
                        media: Some(self.media_of(s, files)),
                    })
                    .collect();
            }
            Some(other) => node.children = other.children().iter().map(|c| child(*c, stack)).collect(),
            None => {}
        }

        stack.pop();
        node
    }
}

/// An action type's name (its high byte; the low byte is its scope). Wwise's action list; checked in the games' banks for Play
/// (0x04, 951 in Attila), Stop (0x01) and Set game parameter (0x13, whose targets are game parameters such as `Fire_Intensity`).
pub fn action_name(action_type: u16) -> &'static str {
    match action_type >> 8 {
        0x01 => "Stop",
        0x02 => "Pause",
        0x03 => "Resume",
        0x04 => "Play",
        0x05 => "Play and continue",
        0x06 => "Mute",
        0x07 => "Unmute",
        0x08 => "Set pitch",
        0x09 => "Reset pitch",
        0x0A => "Set volume",
        0x0B => "Reset volume",
        0x0C => "Set bus volume",
        0x0D => "Reset bus volume",
        0x0E => "Set low-pass",
        0x0F => "Reset low-pass",
        0x10 => "Use state",
        0x11 => "Unuse state",
        0x12 => "Set state",
        0x13 => "Set game parameter",
        0x14 => "Reset game parameter",
        0x19 => "Set switch",
        0x1A => "Bypass effect",
        0x1B => "Reset bypass effect",
        0x1C => "Break",
        0x1D => "Trigger",
        0x1E => "Seek",
        0x1F => "Release",
        _ => "Action",
    }
}
