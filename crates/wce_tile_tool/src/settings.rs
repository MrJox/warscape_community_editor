//! The Tile tool's settings: the `[tools.tile]` table.

use std::collections::BTreeMap;
use std::path::Path;

use serde::{Deserialize, Serialize};
pub use wce_launch::{LaunchArmy, TileWeather};

/// The table's name.
pub const SETTINGS_ID: &str = "tile";

/// The fields about the game's files, kept per game: tiles, weathers and Create entries are one game's (the same tile path can be in two
/// games), and so are the sea states (keys of the game's wind table), the packs written and Launch in game's armies (keys of its tables).
pub const GAME_FIELDS: &[&str] =
    &["sea_state", "weathers", "favourites", "recent", "create_favourites", "create_recent", "mod_packs", "launch_army"];

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

    /// The Areas chip: the polygons' fill and outline opacity, and the fill's factor per type (1 when not listed).
    pub polygon_fill: f32,
    pub polygon_outline: f32,
    pub polygon_type_fill: BTreeMap<String, f32>,

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

    /// The Create panel shows its entries as a grid of pictures instead of a list.
    pub create_grid: bool,

    /// The Prefab tool's Prefabs panel shows its prefabs as a grid of pictures instead of a list.
    pub prefab_grid: bool,

    /// The Scene panel's variations column's width (a combined project's), as its header's edge was dragged; 0: as wide as the
    /// variations' names.
    pub scene_variations_width: f32,

    /// How far a siege AI node's adjusted Boundary reaches from its centre (metres; 0: the generator's reach, from the room around it).
    pub boundary_reach: f32,

    /// The placement chips: what new things stand on and how they are turned.
    pub place_surface: SurfaceMode,
    pub place_rotation: RotationMode,

    /// The vegetation brush.
    pub veg_brush: VegBrush,

    /// The height and paint brushes' falloff curve (the Brush Falloff window).
    pub brush_curve: BrushCurve,

    /// The image brush.
    pub brush_image: ImageBrush,

    /// Each tile's saved camera views (bookmarks), by the tile's folder (lower case, `/` separators).
    pub camera_views: BTreeMap<String, Vec<CameraView>>,

    /// Each tile's reference image (a plan or a picture to lay a city out over), by the tile's folder (lower case, `/` separators).
    pub reference_images: BTreeMap<String, ReferenceImage>,

    /// Where Tile > Screenshot saves the viewport's picture; none: `<projects folder>/screenshots`.
    pub screenshot_folder: Option<std::path::PathBuf>,

    /// Launch in game (`launch.rs`): the battle the game starts into, and its armies (the game's).
    pub launch: LaunchSettings,
    pub launch_army: LaunchArmy,

    /// The mod pack "Save to mod pack" wrote last, by game key.
    pub mod_packs: BTreeMap<String, String>,

    /// The ground coverage's last values.
    pub ground_coverage: GroundCoverage,
}

/// Where Launch in game puts the tile.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LaunchMap {
    /// The tile alone is the battle map (any tile, painted on a map or not), drawn in the climate and season the editor draws its ground
    /// with (its terrain look): the game draws a tile alone in the `default` climate, so the launch gives `default` that climate's
    /// ground textures, grass and trees (`launch_climate`; nothing when the editor's climate is `default`).
    #[default]
    #[serde(alias = "climate")]
    Tile,

    /// The Map context's placement in its map (the neighbours and seams; the map's climate), the tile alone without one.
    MapContext,
}

/// How Launch in game hands the tile to the game.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LaunchDelivery {
    /// A mod pack `data\wce_launch.pack` named first, with every mod pack chosen in the editor; removed when the game exits.
    #[default]
    Pack,

    /// Loose files in a working directory (`add_working_directory`), with only the mod packs the tile needs. The game's packs win over
    /// loose files, and the release's tiles.pack has the tile database, so a tile falls back to the pack (kept for files no pack has).
    Loose,
}

/// How Launch in game makes its battle: the delivery, the map, and the last launch's times (the armies are each game's: [`LaunchArmy`]).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct LaunchSettings {
    pub delivery: LaunchDelivery,
    pub map: LaunchMap,

    /// Outside Windows: the Proton folder the game's Windows build runs with (none: the one Steam runs the game with).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub proton: Option<std::path::PathBuf>,

    /// How long each step of the last launch took (seconds, by the step's key): the next launch's estimate.
    pub step_seconds: BTreeMap<String, f32>,
}

/// The ground coverage's window (`ground_coverage_panel.rs`): the last preset of each kind (by the kinds' names: `cliffs`, `coast`,
/// `riverbank`).
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct GroundCoverage {
    pub last_preset: BTreeMap<String, String>,
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

    /// Upright on the slope: pitched along its length only (`wce_rotation="slope"`, `Tile::slope_rotation`).
    Slope,
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

/// The brushes' falloff curve: whether it replaces the falloff slider, and its points (distance from the centre, weight; both 0..1).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct BrushCurve {
    pub on: bool,
    pub points: Vec<[f32; 2]>,
}

impl Default for BrushCurve {
    fn default() -> Self {
        Self { on: false, points: wce_tile::falloff::FalloffCurve::default().points }
    }
}

/// The image brush (`image_brush.rs`): on or off, its picture, its turn in degrees, or a turn at random at every step.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ImageBrush {
    pub on: bool,
    pub path: String,
    pub angle: f32,
    pub random_angle: bool,
}

/// A saved camera view of a tile: where the camera is and what it looks at (world metres).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct CameraView {
    pub name: String,
    pub eye: [f32; 3],
    pub target: [f32; 3],
}

/// How a tile's reference image is shown.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReferenceShow {
    /// Not shown.
    #[default]
    Off,

    /// Over the viewport, fitted to it keeping its proportions (it does not follow the camera).
    Screen,

    /// Laid on the tile's ground from above, in metres (it moves with the world).
    Ground,
}

/// A tile's reference image: the PNG, how it is shown and where it lies on the ground.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ReferenceImage {
    pub path: String,
    pub show: ReferenceShow,
    pub opacity: f32,

    /// On the ground: the picture's centre (world metres, x and z), its width (metres; the height follows its proportions) and its turn
    /// (degrees, clockwise seen from above).
    pub centre: [f32; 2],
    pub width: f32,
    pub rotation: f32,
}

impl Default for ReferenceImage {
    fn default() -> Self {
        Self { path: String::new(), show: ReferenceShow::Off, opacity: 0.5, centre: [0.0, 0.0], width: 0.0, rotation: 0.0 }
    }
}

impl Default for TileSettings {
    fn default() -> Self {
        Self {
            show_sea: true,
            show_vfx_boxes: true,
            polygon_fill: 0.3,
            polygon_outline: 0.85,
            polygon_type_fill: BTreeMap::new(),
            sea_state: String::new(),
            weathers: BTreeMap::new(),
            favourites: vec![],
            recent: vec![],
            create_favourites: vec![],
            create_recent: vec![],
            create_grid: false,
            prefab_grid: false,
            scene_variations_width: 0.0,
            boundary_reach: 0.0,
            place_surface: SurfaceMode::default(),
            place_rotation: RotationMode::default(),
            veg_brush: VegBrush::default(),
            brush_curve: BrushCurve::default(),
            brush_image: ImageBrush::default(),
            camera_views: BTreeMap::new(),
            reference_images: BTreeMap::new(),
            screenshot_folder: None,
            mod_packs: BTreeMap::new(),
            launch: LaunchSettings::default(),
            launch_army: LaunchArmy::default(),
            ground_coverage: GroundCoverage::default(),
        }
    }
}
