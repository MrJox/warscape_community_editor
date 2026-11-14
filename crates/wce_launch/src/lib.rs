//! The game launched on a tile or a battle map: the battle XML, the launch pack and the files it ships, the game started (on Linux
//! through Proton) and watched. Its own crate so that the Terrain tool launches maps without the Tile tool.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

pub mod launch;
pub mod proton;

/// Launch in game's two armies of one unit each, a game's (vanilla keys: a mod's units may need packs that are not enabled). An empty key
/// is the game's own ([`wce_vfs::LaunchFacts`]).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct LaunchArmy {
    /// The defender (alliance 0, the player) and the attacker: a faction and a unit each.
    pub defender_faction: String,
    pub defender_unit: String,
    pub attacker_faction: String,
    pub attacker_unit: String,

    /// The battle's subculture (the settlement's look in a settlement battle).
    pub subculture: String,

    /// tile_upgrades.xml keys of a settlement battle in a map (`level1 escalation3`); empty: none.
    pub tile_upgrades: String,
}

impl LaunchArmy {
    /// Every key given: an empty one takes the game's.
    pub fn resolved(&self, game: &wce_vfs::LaunchFacts) -> Self {
        let or = |v: &str, default: &str| if v.trim().is_empty() { default.to_owned() } else { v.trim().to_owned() };
        Self {
            defender_faction: or(&self.defender_faction, game.defender_faction),
            defender_unit: or(&self.defender_unit, game.defender_unit),
            attacker_faction: or(&self.attacker_faction, game.attacker_faction),
            attacker_unit: or(&self.attacker_unit, game.attacker_unit),
            subculture: or(&self.subculture, game.subculture),
            tile_upgrades: self.tile_upgrades.trim().to_owned(),
        }
    }
}

/// The weather a tile is lit with and the filters of its list.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct TileWeather {
    /// An environment file of the packs (normalized); empty: the default light.
    pub file: String,

    /// The list's filters: environment_map.xml categories and their values (empty: any).
    pub query: BTreeMap<String, String>,
}
