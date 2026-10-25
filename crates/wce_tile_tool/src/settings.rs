//! The Tile tool's settings: the `[tools.tile]` table.

use std::collections::BTreeMap;
use std::path::Path;

use serde::{Deserialize, Serialize};

/// The table's name.
pub const SETTINGS_ID: &str = "tile";

/// The source of the game's exported tiles in a [`TileRef`] (the Tiles panel's "game packs").
pub const PACKS_SOURCE: &str = "packs";

/// Recently opened tiles kept.
pub const RECENT_KEEP: usize = 10;

/// A tile of the Tiles panel: where it is listed and its database entry.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct TileRef {
    /// A kit's raw_data folder ([`source_of`]), or [`PACKS_SOURCE`].
    pub source: String,

    /// The entry's normalized location (`terrain/tiles/battle/...`).
    pub key: String,

    /// The variation folder opened (recent tiles); empty: the first.
    #[serde(skip_serializing_if = "String::is_empty")]
    pub variation: String,
}

/// A kit's raw_data folder as a [`TileRef`] source: lower case, `/` separators, no trailing one.
pub fn source_of(raw_root: &Path) -> String {
    let s = raw_root.display().to_string().replace('\\', "/").to_lowercase();
    s.trim_end_matches('/').to_owned()
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct TileSettings {
    /// The sea at 0 m (the tile's plane and the map context's sea).
    pub show_sea: bool,

    /// The VFX boxes (the VFXInstance markers; the effects still play).
    pub show_vfx_boxes: bool,

    /// The battle wind the sea is drawn with (a key of `wind_levels_tables`); empty: the renderer's default.
    pub sea_state: String,

    /// Each tile's weather, by the tile's folder (lower case, `/` separators).
    pub weathers: BTreeMap<String, TileWeather>,

    /// The tiles starred in the Tiles panel.
    pub favourites: Vec<TileRef>,

    /// The last tiles opened, the latest first (at most [`RECENT_KEEP`]).
    pub recent: Vec<TileRef>,

    /// The Create panel's starred entries and the last ones used, the latest first (at most [`RECENT_KEEP`]); ids like `kind|key`.
    pub create_favourites: Vec<String>,
    pub create_recent: Vec<String>,

    /// The placement chips: what new things stand on and how they are turned.
    pub place_surface: SurfaceMode,
    pub place_rotation: RotationMode,

    /// The vegetation brush.
    pub veg_brush: VegBrush,
}

/// What new things stand on (`placement_modes.rs`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SurfaceMode {
    Freeform,
    #[default]
    Landscape,
    Entity,
}

/// How new things are turned (`placement_modes.rs`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RotationMode {
    #[default]
    None,
    Surface,
}

/// The vegetation brush (`veg_brush.rs`): how big, how dense, how far from what is there, how varied.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct VegBrush {
    /// Paint with the brush (else a click places one tree).
    pub on: bool,
    pub radius: f32,

    /// The least distance between two trunks (metres).
    pub spacing: f32,

    /// The least distance from the other things' footprints (metres).
    pub clearance: f32,

    /// How much of the room the spacing leaves is planted (0..1).
    pub fill: f32,

    /// The trees' scale, at random between these.
    pub scale_min: f32,
    pub scale_max: f32,

    /// Each tree turned at random.
    pub random_yaw: bool,
}

impl Default for VegBrush {
    fn default() -> Self {
        Self { on: false, radius: 15.0, spacing: 4.0, clearance: 1.0, fill: 0.6, scale_min: 0.85, scale_max: 1.15, random_yaw: true }
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

impl Default for TileSettings {
    fn default() -> Self {
        Self { show_sea: true, show_vfx_boxes: true, sea_state: String::new(), weathers: BTreeMap::new(), favourites: vec![], recent: vec![], create_favourites: vec![], create_recent: vec![], place_surface: SurfaceMode::default(), place_rotation: RotationMode::default(), veg_brush: VegBrush::default() }
    }
}
