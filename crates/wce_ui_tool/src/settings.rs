//! The UI tool's settings: the `[tools.ui]` table. The skins, layouts and pictures are in the game's projects folder's `loose/` (`Settings::loose_dir`): every tool reads it.

use serde::{Deserialize, Serialize};

/// The table's name.
pub const SETTINGS_ID: &str = "ui";

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct UiSettings {
    /// The last pack the project was exported to.
    pub export_pack: String,

    /// The last mod pack a layout was saved into.
    pub mod_pack: String,
}
