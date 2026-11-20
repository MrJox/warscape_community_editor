//! Names for Wwise IDs. Wwise keeps IDs, not names: an ID is the FNV-1 hash of a lower-case name, so every name found anywhere is
//! hashed and kept against its ID. Sources, first found wins: the user's labels, `event_data.dat`, the banks' `STID` and file names,
//! the animation triggers CSV, the built-in list (TheTollski's `TWA_Names.txt`, `third_party/soundbank_editor`).

use std::collections::HashMap;

use wce_audio_format::event_data::EventData;
use wce_audio_format::fnv;
use wce_audio_format::triggers::Triggers;
use wce_vfs::files::Files;

static BUILTIN: &str = include_str!("../../../third_party/soundbank_editor/TWA_Names.txt");

/// Where a name came from.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum NameSource {
    Label,
    EventData,
    Bank,
    Trigger,
    /// The DB tables' sound keys: units' selection lines, voice-overs, the sound tables' events.
    Db,
    BuiltIn,
}

impl NameSource {
    pub fn describe(self) -> &'static str {
        match self {
            NameSource::Label => "your label",
            NameSource::EventData => "event_data.dat",
            NameSource::Bank => "a bank",
            NameSource::Trigger => "sound_animation_triggers.csv",
            NameSource::Db => "the DB tables",
            NameSource::BuiltIn => "the built-in list (TWA_Names.txt)",
        }
    }
}

#[derive(Clone, Debug, Default)]
pub struct Names {
    by_id: HashMap<u32, (String, NameSource)>,
}

impl Names {
    /// Names by hashing: keeps `name` for its FNV-1 ID unless the ID already has a name.
    pub fn add(&mut self, name: &str, source: NameSource) {
        let name = name.trim();
        if !name.is_empty() {
            self.add_id(fnv::id(name), name, source);
        }
    }

    /// A name known with its ID (a bank's `STID`, a user's label: no hashing).
    pub fn add_id(&mut self, id: u32, name: &str, source: NameSource) {
        match self.by_id.get(&id) {
            Some((_, old)) if *old <= source => {}
            _ => {
                self.by_id.insert(id, (name.to_owned(), source));
            }
        }
    }

    pub fn get(&self, id: u32) -> Option<&str> {
        self.by_id.get(&id).map(|(n, _)| n.as_str())
    }

    pub fn source(&self, id: u32) -> Option<NameSource> {
        self.by_id.get(&id).map(|(_, s)| *s)
    }

    pub fn len(&self) -> usize {
        self.by_id.len()
    }

    pub fn is_empty(&self) -> bool {
        self.by_id.is_empty()
    }

    /// The built-in list only.
    pub fn built_in() -> Self {
        let mut names = Self::default();
        for line in BUILTIN.trim_start_matches('\u{feff}').lines() {
            names.add(line, NameSource::BuiltIn);
        }

        names
    }

    /// The built-in list plus the game's own: `event_data.dat`, the triggers CSV and the banks' file names (the banks' `STID` are
    /// added when the banks are read, `SoundLibrary::load`).
    pub fn from_game(files: &dyn Files) -> Self {
        let mut names = Self::built_in();
        if let Ok(bytes) = files.read("audio/event_data.dat")
            && let Ok(data) = EventData::read(&bytes)
        {
            data.names().for_each(|n| names.add(n, NameSource::EventData));
        }

        if let Ok(bytes) = files.read("audio/sound_animation_triggers.csv")
            && let Ok(triggers) = Triggers::read(&bytes)
        {
            for row in &triggers.rows {
                row.iter().skip(1).take(4).for_each(|n| names.add(n, NameSource::Trigger));
            }
        }

        for path in files.list("audio/", ".bnk") {
            let stem = path.rsplit('/').next().unwrap_or(&path).trim_end_matches(".bnk");
            names.add(stem, NameSource::Bank);
        }

        // The DB's sound keys (with the game's schema, when set): the order lines units select with, voice-overs, events.
        let db = crate::sound_db::SoundDb::read(files);
        for unit in &db.units {
            unit.selection.iter().for_each(|n| names.add(n, NameSource::Db));
            names.add(&unit.voiceover, NameSource::Db);
        }
        for (_, _, voiceover) in &db.voiceovers {
            names.add(voiceover, NameSource::Db);
        }
        for (_, _, _, event) in db.events() {
            names.add(&event, NameSource::Db);
        }

        names
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn built_in_names_hash() {
        let names = Names::built_in();
        assert!(names.len() > 3000);
        assert_eq!(names.get(fnv::id("Battle_Order_VO_Mute")), Some("Battle_Order_VO_Mute"));
    }

    #[test]
    fn earlier_sources_win() {
        let mut names = Names::default();
        names.add("Some_Event", NameSource::BuiltIn);
        names.add_id(fnv::id("Some_Event"), "my label", NameSource::Label);
        names.add("some_event", NameSource::EventData);
        assert_eq!(names.get(fnv::id("some_event")), Some("my label"));
    }
}
