//! A campaign's diplomacy at its start, from the startpos: the factions, their stances to each other, the regions' owners, and the
//! colour a region owner's borders take for a chosen faction (docs/formats/campaign_borders.md, "Colours").
//!
//! The startpos records (RPFM's leaf names: `^+n` the n-th leaf of a record's group before any record, `NAME+n` after the record):
//! - `CAMPAIGN_ENV/CAMPAIGN_MODEL/WORLD/FACTION_ARRAY/FACTION`: `^+1` the faction's runtime id, `^+2` its key; its
//!   `OLD_DIPLOMACY_MANAGER/DIPLOMACY_RELATIONSHIPS_ARRAY/OLD_DIPLOMACY_RELATIONSHIP`s: `^+1` the other faction's runtime id, `^+4` the
//!   stance (`war` 0, `neutral` 1, `defensive_allies` 2, `military_allies` 3, `protector` 4, `client_state` 5, `master` 6, `vassal` 7);
//! - `WORLD/REBEL_FACTION/FACTION`: the rebels (an empty key: no `factions` record, the game's faction +0x800 null);
//! - `WORLD/REGION_MANAGER/REGIONS_ARRAY/REGION`: `^+1` the region's runtime id, `^+2` its key, `SETTLEMENT+9` its owner's runtime id;
//! - `CAMPAIGN_PREOPEN_MAP_INFO/FACTION_INFOS`: per faction its key, two flags (the second: playable), its order on the faction list and
//!   the regions it holds at the start.

use std::collections::HashMap;

use rpfm_lib::files::esf::{NodeType, RecordNode};

use super::cities::{as_record, child, leaf, str_of, u32_of};

/// A stance of one faction to another (the startpos's strings, the game's enum order).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Stance {
    War,
    Neutral,
    DefensiveAllies,
    MilitaryAllies,
    Protector,
    ClientState,
    Master,
    Vassal,
}

impl Stance {
    #[must_use]
    pub fn from_key(key: &str) -> Option<Self> {
        Some(match key {
            "war" => Self::War,
            "neutral" => Self::Neutral,
            "defensive_allies" => Self::DefensiveAllies,
            "military_allies" => Self::MilitaryAllies,
            "protector" => Self::Protector,
            "client_state" => Self::ClientState,
            "master" => Self::Master,
            "vassal" => Self::Vassal,
            _ => return None,
        })
    }

    /// Allied (the game's fn 0x10829140: stances 2 to 7).
    #[must_use]
    pub fn is_allied(self) -> bool {
        !matches!(self, Self::War | Self::Neutral)
    }
}

/// The colour class of a region owner's borders for a chosen faction (fn 0x10946810).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ColourClass {
    /// The rebels (no `factions` record): black.
    Rebel,
    /// At war with the chosen faction (or any faction when the chosen one is the rebels): red.
    War,
    /// The chosen faction's own.
    Own,
    /// Allied (the chosen faction's stance 2 to 7).
    Allied,
    /// Anything else: grey.
    Neutral,
}

impl ColourClass {
    /// The colour, 0 to 1 (the game's bytes / 255).
    #[must_use]
    pub fn rgb(self) -> [f32; 3] {
        let b: [u8; 3] = match self {
            Self::Rebel => [0, 0, 0],
            Self::War => [255, 0, 0],
            Self::Own => [180, 60, 0],
            Self::Allied => [0, 90, 190],
            Self::Neutral => [178, 178, 178],
        };
        b.map(|v| f32::from(v) / 255.0)
    }
}

/// A faction of the startpos's world.
#[derive(Clone, Debug, PartialEq)]
pub struct StartFaction {
    pub id: u32,
    /// Empty for the rebels.
    pub key: String,
}

/// A region of the startpos, in the startpos's order.
#[derive(Clone, Debug, PartialEq)]
pub struct StartRegion {
    pub id: u32,
    pub key: String,
    /// Its owner's runtime id.
    pub owner: u32,
}

/// A faction of the campaign's faction list (`FACTION_INFOS`).
#[derive(Clone, Debug, PartialEq)]
pub struct FactionInfo {
    pub key: String,
    pub playable: bool,
    /// Its order on the list (the game sorts by it; ties keep the file's order).
    pub order: i32,
    /// The regions it holds at the start.
    pub regions: i32,
}

/// The diplomacy of a startpos.
#[derive(Clone, Debug, Default)]
pub struct Diplomacy {
    /// `FACTION_ARRAY`'s, then the rebels.
    pub factions: Vec<StartFaction>,
    /// (faction, other) -> the faction's stance to the other.
    pub stances: HashMap<(u32, u32), Stance>,
    pub regions: Vec<StartRegion>,
}

fn records<'a>(r: &'a RecordNode, name: &'a str) -> impl Iterator<Item = &'a RecordNode> + 'a {
    r.children().iter().flatten().filter_map(as_record).filter(move |c| c.name() == name)
}

/// `CAMPAIGN_ENV/CAMPAIGN_MODEL/WORLD` of a startpos's root.
pub(crate) fn world(root: &RecordNode) -> Option<&RecordNode> {
    ["CAMPAIGN_ENV", "CAMPAIGN_MODEL", "WORLD"].iter().try_fold(root, |r, n| child(r, n))
}

impl Diplomacy {
    /// From a startpos's root record.
    ///
    /// # Errors
    ///
    /// The startpos has no world or no regions.
    pub fn read(root: &RecordNode) -> Result<Self, String> {
        let world = world(root).ok_or("startpos.esf: no WORLD")?;
        let mut d = Self::default();
        let faction = |f: &RecordNode, d: &mut Self| {
            for g in f.children() {
                let Some(id) = leaf(g, None, 1).and_then(u32_of) else { continue };
                let key = leaf(g, None, 2).and_then(str_of).unwrap_or_default().to_owned();
                d.factions.push(StartFaction { id, key });
                for m in g.iter().filter_map(as_record).filter(|r| r.name() == "OLD_DIPLOMACY_MANAGER") {
                    for arr in records(m, "DIPLOMACY_RELATIONSHIPS_ARRAY") {
                        for rel in records(arr, "OLD_DIPLOMACY_RELATIONSHIP") {
                            for rg in rel.children() {
                                let other = leaf(rg, None, 1).and_then(u32_of);
                                let stance = leaf(rg, None, 4).and_then(str_of).and_then(Stance::from_key);
                                if let (Some(o), Some(s)) = (other, stance) {
                                    d.stances.insert((id, o), s);
                                }
                            }
                        }
                    }
                }
            }
        };
        for array in records(world, "FACTION_ARRAY") {
            for f in records(array, "FACTION") {
                faction(f, &mut d);
            }
        }
        for rebels in records(world, "REBEL_FACTION") {
            for f in records(rebels, "FACTION") {
                faction(f, &mut d);
            }
        }

        let regions = child(world, "REGION_MANAGER").and_then(|m| child(m, "REGIONS_ARRAY")).ok_or("startpos.esf: no REGIONS_ARRAY")?;
        for region in records(regions, "REGION") {
            for g in region.children() {
                let (Some(id), Some(key)) = (leaf(g, None, 1).and_then(u32_of), leaf(g, None, 2).and_then(str_of)) else { continue };
                let owner = leaf(g, Some("SETTLEMENT"), 9).and_then(u32_of).unwrap_or(0);
                d.regions.push(StartRegion { id, key: key.to_owned(), owner });
            }
        }

        Ok(d)
    }

    /// A faction's runtime id by its key.
    #[must_use]
    pub fn faction_id(&self, key: &str) -> Option<u32> {
        self.factions.iter().find(|f| !f.key.is_empty() && f.key == key).map(|f| f.id)
    }

    /// A faction's key (empty for the rebels and an unknown id).
    #[must_use]
    pub fn faction_key(&self, id: u32) -> &str {
        self.factions.iter().find(|f| f.id == id).map_or("", |f| f.key.as_str())
    }

    /// The rebels: no `factions` record (an empty key), or not a faction of the world.
    #[must_use]
    pub fn is_rebel(&self, id: u32) -> bool {
        self.faction_key(id).is_empty()
    }

    /// A faction's stance to another (neutral without a record).
    #[must_use]
    pub fn stance(&self, from: u32, to: u32) -> Stance {
        self.stances.get(&(from, to)).copied().unwrap_or(Stance::Neutral)
    }

    /// The colour class of `owner`'s borders for the chosen faction `player`, the first rule that holds: the rebels black; at war with
    /// the player (every faction when the player is the rebels) red; the player's own; the player's stance allied (2 to 7) blue; grey.
    #[must_use]
    pub fn colour_class(&self, player: u32, owner: u32) -> ColourClass {
        if self.is_rebel(owner) {
            ColourClass::Rebel
        } else if owner != player && (self.is_rebel(player) || self.stance(player, owner) == Stance::War) {
            ColourClass::War
        } else if owner == player {
            ColourClass::Own
        } else if self.stance(player, owner).is_allied() {
            ColourClass::Allied
        } else {
            ColourClass::Neutral
        }
    }
}

/// The campaign's faction list (`CAMPAIGN_PREOPEN_MAP_INFO/FACTION_INFOS`), in the file's order.
#[must_use]
pub fn faction_infos(root: &RecordNode) -> Vec<FactionInfo> {
    let mut out = vec![];
    let Some(info) = child(root, "CAMPAIGN_PREOPEN_MAP_INFO") else { return out };
    for r in records(info, "FACTION_INFOS") {
        for g in r.children() {
            let Some(key) = leaf(g, None, 1).and_then(str_of) else { continue };
            let flag = |n: usize| matches!(leaf(g, None, n), Some(NodeType::Bool(b)) if *b.value());
            let int = |n: usize| match leaf(g, None, n) {
                Some(NodeType::I32(v)) => *v.value(),
                Some(other) => u32_of(other).map_or(0, |v| v as i32),
                None => 0,
            };
            out.push(FactionInfo { key: key.to_owned(), playable: flag(3), order: int(4), regions: int(7) });
        }
    }

    out
}

/// The faction picked by default: the playable faction holding the most regions at the start (ASSUMPTION: the startpos and the db
/// name no default; on Attila's `main_attila` this is the Western Roman Empire, the player of the captures), else the first faction.
#[must_use]
pub fn default_faction(infos: &[FactionInfo]) -> Option<&FactionInfo> {
    let mut best: Option<&FactionInfo> = None;
    for i in infos.iter().filter(|i| i.playable) {
        if best.is_none_or(|b| i.regions > b.regions) {
            best = Some(i);
        }
    }

    best.or(infos.first())
}
