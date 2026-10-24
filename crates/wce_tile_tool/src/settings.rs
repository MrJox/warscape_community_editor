//! The Tile tool's settings: the `[tools.tile]` table.

use std::collections::BTreeMap;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

/// The table's name.
pub const SETTINGS_ID: &str = "tile";

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct TileSettings {
    /// The raw_data folders the tile browser lists; empty: the assembly kits in the game folder.
    pub kits: Vec<PathBuf>,

    /// The sea at 0 m (the tile's plane and the map context's sea).
    pub show_sea: bool,

    /// The VFX boxes (the VFXInstance markers; the effects still play).
    pub show_vfx_boxes: bool,

    /// Where opened exported tiles are decompiled to (TEd projects); none: `tile_workspace` in the data folder.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub workspace: Option<PathBuf>,

    /// The battle wind the sea is drawn with (a key of `wind_levels_tables`); empty: the renderer's default.
    pub sea_state: String,

    /// Each tile's weather, by the tile's folder (lower case, `/` separators).
    pub weathers: BTreeMap<String, TileWeather>,
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

impl Default for TileSettings {
    fn default() -> Self {
        Self { kits: vec![], workspace: None, show_sea: true, show_vfx_boxes: true, sea_state: String::new(), weathers: BTreeMap::new() }
    }
}
