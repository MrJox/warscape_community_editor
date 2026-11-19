//! `audio/event_data.dat`: the game's list of sound events, state groups, dialogue events and game parameters (Attila; both of Rome
//! 2's copies have the same layout). Integers are u32, strings a u32 length and UTF-8. Six sections, in order (docs/formats/sound_data.md):
//!
//! 1. events: name + a 4-byte value read as f32 (0 for most; its meaning is not known yet);
//! 2. and 3. two lists of state groups: name + state names;
//! 4. dialogue events: name + the IDs of the state groups they switch on;
//! 5. game parameters (settings): names;
//! 6. dialogue trees: a dialogue event's ID, then (state ID, child state IDs) pairs.
//!
//! Every string hashes ([`crate::fnv::id`]) to an ID the banks use: the file is a source of names.

use crate::io::{Put, Reader};
use crate::{FormatError, Result};

#[derive(Clone, Debug, PartialEq)]
pub struct EventData {
    pub events: Vec<(String, f32)>,
    pub state_groups: Vec<StateGroup>,
    /// The second list of state groups (mixer states, music, animation pools...).
    pub state_groups_2: Vec<StateGroup>,
    pub dialogue_events: Vec<(String, Vec<u32>)>,
    pub game_parameters: Vec<String>,
    pub dialogue_trees: Vec<DialogueTree>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct StateGroup {
    pub name: String,
    pub states: Vec<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct DialogueTree {
    pub dialogue_event: u32,
    pub nodes: Vec<(u32, Vec<u32>)>,
}

fn string(r: &mut Reader) -> Result<String> {
    let n = r.count(1)?;
    String::from_utf8(r.take(n)?.to_vec()).map_err(|_| FormatError::Invalid("a name is not UTF-8".into()))
}

fn ids(r: &mut Reader) -> Result<Vec<u32>> {
    let n = r.count(4)?;
    (0..n).map(|_| r.u32()).collect()
}

fn groups(r: &mut Reader) -> Result<Vec<StateGroup>> {
    let n = r.count(8)?;
    (0..n)
        .map(|_| {
            let name = string(r)?;
            let count = r.count(4)?;
            Ok(StateGroup { name, states: (0..count).map(|_| string(r)).collect::<Result<_>>()? })
        })
        .collect()
}

impl EventData {
    /// Reads the file.
    ///
    /// # Errors
    ///
    /// Fails when a section runs past the end of the file or bytes are left after the last one.
    pub fn read(bytes: &[u8]) -> Result<Self> {
        let mut r = Reader::new(bytes);
        let n = r.count(8)?;
        let events = (0..n).map(|_| Ok((string(&mut r)?, r.f32()?))).collect::<Result<_>>()?;
        let state_groups = groups(&mut r)?;
        let state_groups_2 = groups(&mut r)?;
        let n = r.count(8)?;
        let dialogue_events = (0..n).map(|_| Ok((string(&mut r)?, ids(&mut r)?))).collect::<Result<_>>()?;
        let n = r.count(4)?;
        let game_parameters = (0..n).map(|_| string(&mut r)).collect::<Result<_>>()?;
        let n = r.count(8)?;
        let dialogue_trees = (0..n)
            .map(|_| {
                let dialogue_event = r.u32()?;
                let count = r.count(8)?;
                let nodes = (0..count).map(|_| Ok((r.u32()?, ids(&mut r)?))).collect::<Result<_>>()?;
                Ok(DialogueTree { dialogue_event, nodes })
            })
            .collect::<Result<_>>()?;
        if r.left() != 0 {
            return Err(FormatError::Invalid(format!("{} bytes left after the last section", r.left())));
        }

        Ok(Self { events, state_groups, state_groups_2, dialogue_events, game_parameters, dialogue_trees })
    }

    /// Writes the file. What [`EventData::read`] read comes back byte for byte.
    pub fn write(&self) -> Vec<u8> {
        fn put_str(out: &mut Vec<u8>, s: &str) {
            out.put_u32(s.len() as u32);
            out.extend_from_slice(s.as_bytes());
        }
        fn put_groups(out: &mut Vec<u8>, groups: &[StateGroup]) {
            out.put_u32(groups.len() as u32);
            for g in groups {
                put_str(out, &g.name);
                out.put_u32(g.states.len() as u32);
                g.states.iter().for_each(|s| put_str(out, s));
            }
        }

        let mut out = vec![];
        out.put_u32(self.events.len() as u32);
        for (name, value) in &self.events {
            put_str(&mut out, name);
            out.put_f32(*value);
        }

        put_groups(&mut out, &self.state_groups);
        put_groups(&mut out, &self.state_groups_2);
        out.put_u32(self.dialogue_events.len() as u32);
        for (name, groups) in &self.dialogue_events {
            put_str(&mut out, name);
            out.put_u32(groups.len() as u32);
            groups.iter().for_each(|g| out.put_u32(*g));
        }

        out.put_u32(self.game_parameters.len() as u32);
        self.game_parameters.iter().for_each(|p| put_str(&mut out, p));
        out.put_u32(self.dialogue_trees.len() as u32);
        for tree in &self.dialogue_trees {
            out.put_u32(tree.dialogue_event);
            out.put_u32(tree.nodes.len() as u32);
            for (state, children) in &tree.nodes {
                out.put_u32(*state);
                out.put_u32(children.len() as u32);
                children.iter().for_each(|c| out.put_u32(*c));
            }
        }

        out
    }

    /// Every name the file holds (events, groups, states, dialogue events, game parameters), for the name index.
    pub fn names(&self) -> impl Iterator<Item = &str> {
        let groups = self.state_groups.iter().chain(&self.state_groups_2);
        self.events
            .iter()
            .map(|(n, _)| n.as_str())
            .chain(groups.flat_map(|g| std::iter::once(g.name.as_str()).chain(g.states.iter().map(String::as_str))))
            .chain(self.dialogue_events.iter().map(|(n, _)| n.as_str()))
            .chain(self.game_parameters.iter().map(String::as_str))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip() {
        let data = EventData {
            events: vec![("Battle_Order_VO_Mute".into(), 0.0), ("Some_Event".into(), 133.0)],
            state_groups: vec![StateGroup { name: "VO_battle_culture".into(), states: vec!["Roman".into(), "Nomad".into()] }],
            state_groups_2: vec![],
            dialogue_events: vec![("battle_vo_disembarking".into(), vec![720_515_004, 2_851_864_456])],
            game_parameters: vec!["meter_advice".into()],
            dialogue_trees: vec![DialogueTree { dialogue_event: 3_427_422_450, nodes: vec![(1_787_064_993, vec![3_206_234_756])] }],
        };
        let bytes = data.write();
        assert_eq!(EventData::read(&bytes).unwrap(), data);
        assert!(EventData::read(&bytes[..bytes.len() - 1]).is_err());
        // 2 events, a group and its 2 states, a dialogue event, a game parameter.
        assert_eq!(data.names().count(), 7);
    }
}
