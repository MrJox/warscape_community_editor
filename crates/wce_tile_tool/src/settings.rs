//! The Tile tool's settings: the `[tools.tile]` table.

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

    /// The battle wind the sea is drawn with (a key of `wind_levels_tables`); empty: the renderer's default.
    pub sea_state: String,
}

impl Default for TileSettings {
    fn default() -> Self {
        Self { kits: vec![], show_sea: true, show_vfx_boxes: true, sea_state: String::new() }
    }
}
