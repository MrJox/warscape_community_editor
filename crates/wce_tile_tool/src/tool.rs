//! The tool: documents, the mouse and the keys in the viewport, the per-frame update, and the Tile and Tiles panels.

use std::any::Any;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use wce_editor::editor::Editor;
use wce_editor::{gui, icons, settings_window, theme};
use wce_editor::steam_detect::file_dialog;
use wce_editor::tool::{Tool, ViewportEvent, ViewportEventKind};
use wce_editor_core::command_registry::{CommandRegistry, CommandResult};
use wce_editor_core::document::Document;
use wce_formats::tile_entry::TileEntry;
use wce_formats::tile_index::TileIndex;
use wce_foundation::log;
use wce_foundation::settings::{Settings, env_value};
use wce_foundation::math::{Float3, dot, length};
use wce_render::model_cache::State as ModelState;
use wce_scene::render_scene::{DocumentId, Overlay, SourceRef, overlay_colour};
use wce_tile::tile_project::{TileProject, kit_raw_root};
use wce_tile_editor::terrain_bake::{TerrainLook, TerrainView};
use wce_tile_editor::tile_document::siege::AUTOGEN_LAYER;
use wce_tile_editor::tile_document::{GROUND_ERASE, WALL_SPLINE, WallLayout, BrushKind, BrushSettings, GENERATED_PREFIX, OffTerrain, PieceState, PieceStatus, TERRAIN, TileDocument, elem_of, piece_of};

use crate::settings::{PACKS_SOURCE, RECENT_KEEP, SETTINGS_ID, TileRef, TileSettings, source_of};

/// The polygon types `add_polygon` makes (the `add_polygon` command; the Create panel makes them with `create_with`); the last is not
/// TEd's: a procedural wall (docs/plans/procedural_walls.md).
pub const POLYGON_TYPES: [&str; 6] = [
    "AI_HINT_POLYGON_INSTANCE", "NOGO_REGION_POLYGON_INSTANCE", "GO_REGION_POLYGON_INSTANCE", "VegetationExclusionPolygonInstance", "SIEGE_AI_POLYGON_INSTANCE",
    WALL_SPLINE,
];

pub const HINTS: [&str; 16] = [
    "AIH_FORT_APPROACH", "AIH_DEFENSIVE_HILL", "AIH_FORT_PERIMETER", "AIH_AMBUSH_CAVALRY_LINE", "AIH_AMBUSH_MELEE_LINE", "AIH_AMBUSH_MISSILE_LINE",
    "AIH_AMBUSH_ASSAULT_AREA", "AIH_AMBUSH_DEFENDER_INITIAL_PATH", "AIH_AMBUSH_FOREST", "AIH_AMBUSH_FIREBALL_LINE", "AIH_BEACH_LANDING",
    "AIH_DEFAULT_DEPLOYMENT_LINE", "AIH_RIVER_EDGE", "AIH_BRIDGE_CROSSING", "AIH_COVER_LINE", "AIH_BEACH_LANDING_FORT_APPROACH",
];

/// What the Scene tree's override marks were found for: the tile, its history's version (every edit moves it, a merged one too) and the
/// library's version.
pub(crate) type OverrideMarksKey = (DocumentId, u64, u64);

/// The vegetation set being placed or painted: the new one (not saved) or a saved one by its name. It is read each time a tree is picked, so
/// it follows the edits.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum SetInUse {
    New,
    Saved(String),
}

/// What clicks on the terrain place, with a preview under the mouse until then (the Create panel).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Placing {
    /// The draft (`TileTool::draft`): a type with its key and values.
    Draft,

    /// A prefab of the library.
    Prefab(String),
}

/// What the left mouse button does in the viewport.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum TileMode {
    #[default]
    Select,
    Raise,
    Lower,
    Smooth,
    Flatten,
    Level,

    /// Roughens the ground with fractal noise.
    Noise,

    /// Stamps the brush's shape as a height, one click at a time.
    Stamp,

    /// Paints the blend map (the ground's textures).
    Paint,

    /// Paints ground_type_map.png (the ground types).
    Ground,
}

/// What the water depth view shows (its tooltip).
const WATER_VIEW_TIP: &str = "The water's depth over the ground, as BOB works out the tile's water map (hf_water_map.data) from the rivers and \
    water planes: cyan shallow to dark blue 6 m and deeper, dark where there is no water. The edges of the areas painted as deep water \
    (ground types) are white, of shallow water yellow: where units can cross is what the painted ground types say, not the water.";

/// What Paint water ground types does (its tooltip).
const PAINT_WATER_TIP: &str = "Paints the water ground types from the depth: shallow water between the two depths, deep water from the second; \
    water painted where the water is shallower than the first (or gone) is cleared, other ground types where there is no water are kept. \
    The defaults come from the kit's river tiles (painted shallow water 0.2 .. 1.25 m deep, deep water 4.4 .. 5.7 m, quartiles). One undo step.";

/// What Snap river ends to link targets does (its tooltip).
static RIVER_SNAP_TIP: std::sync::LazyLock<String> = std::sync::LazyLock::new(|| format!(
    "Moves every river end at the tile's edge whose cell the tile offers as a river link: level with the cell's centre along the edge, and \
     {} m past the edge unless it is past it already (as the kit's rivers end, so that the neighbour's river meets it). The ends' handles move \
     with them; their height stays. An end already within a few metres of its cell's centre is centred too. Ends at cells without a river \
     link are left alone. One undo step.",
    wce_tile_editor::tile_document::river_links::RIVER_END_OVERSHOOT_M
));

/// What the Tint freeform view filter does (its tooltip).
const FREEFORM_TINT_TIP: &str = "View filter: lays a blue tint over every freeform instance (MSM_FREEFORM, in effect through its groups: it keeps its own height \
     instead of following the terrain), to tell them from the landscape ones. Only the viewport shows it; the tile and the game do not change.";

pub const MODE_NAMES: [&str; 10] = ["select", "raise", "lower", "smooth", "flatten", "level", "noise", "stamp", "paint", "ground"];
const MODES: [TileMode; 10] = [
    TileMode::Select, TileMode::Raise, TileMode::Lower, TileMode::Smooth, TileMode::Flatten, TileMode::Level, TileMode::Noise, TileMode::Stamp,
    TileMode::Paint, TileMode::Ground,
];

impl TileMode {
    pub fn from_index(i: usize) -> Self {
        MODES[i.min(MODES.len() - 1)]
    }

    pub fn index(self) -> usize {
        MODES.iter().position(|&m| m == self).unwrap_or(0)
    }

    /// The brush of a terrain mode (the channel of a paint brush is the tool's, `TileTool::brush_kind`).
    fn brush(self) -> Option<BrushKind> {
        match self {
            Self::Select => None,
            Self::Paint => Some(BrushKind::Paint(0)),
            Self::Ground => Some(BrushKind::Ground(GROUND_ERASE)),
            Self::Raise => Some(BrushKind::Raise),
            Self::Lower => Some(BrushKind::Lower),
            Self::Smooth => Some(BrushKind::Smooth),
            Self::Flatten => Some(BrushKind::Flatten),
            Self::Level => Some(BrushKind::Level),
            Self::Noise => Some(BrushKind::Noise),
            Self::Stamp => Some(BrushKind::Stamp),
        }
    }
}


/// A row of the Scene panel's flattened tree.
#[derive(Clone, Debug)]
pub(crate) struct TreeRow {
    pub elem: usize,
    pub depth: usize,
    pub key: String,
    pub layer: bool,
    pub open: bool,
    pub kids: usize,
}

/// A tile saved as an exported tile (`TileTool::export_to`).
pub(crate) struct Exported {
    /// The exported folder (`<out>/terrain/tiles/...`).
    pub dir: PathBuf,

    /// The pack of a tile with procedural walls (the tile, its database and the walls' pieces and rows).
    pub pack: Option<PathBuf>,

    /// The compile's notes (in the log).
    pub notes: usize,
}

/// An export's inputs (`TileTool::export_inputs`): everything the compile reads, owned, so that it can run on another thread.
pub(crate) struct ExportInputs {
    game_dir: PathBuf,

    /// The game's tile data if it was read before (else read by `run`).
    pub game: Option<Arc<wce_tile_io::attila::workflow::Game>>,
    project_dir: PathBuf,

    /// The scene as tile.wce text.
    text: Vec<u8>,

    /// A tile with procedural walls: the game files and rpfm's schema (its path).
    walls: Option<(Arc<dyn wce_vfs::files::Files>, PathBuf)>,
}

impl ExportInputs {
    /// Whether the tile has procedural walls (exported with their pieces and database rows into a pack).
    pub fn has_walls(&self) -> bool {
        self.walls.is_some()
    }

    /// The game's tile data: the one read before, else read now (slow the first time: the packs' databases, buildings and models).
    ///
    /// # Errors
    ///
    /// The game's data doesn't read.
    pub fn game(&mut self) -> Result<Arc<wce_tile_io::attila::workflow::Game>, String> {
        if let Some(g) = &self.game {
            return Ok(g.clone());
        }

        let game = Arc::new(wce_tile_io::attila::workflow::Game::load(&self.game_dir)?);
        for w in &game.warnings {
            log::warn(format!("exported tiles: {w}"));
        }

        self.game = Some(game.clone());
        Ok(game)
    }

    /// Compiles the tile into an exported tile under `out` (as `TileTool::save_exported`); also returns the game's tile data, to keep.
    ///
    /// # Errors
    ///
    /// The game's data or the project doesn't read, the compile fails, or a file can't be written.
    pub fn run(mut self, out: &Path) -> Result<(Exported, Arc<wce_tile_io::attila::workflow::Game>), String> {
        let game = self.game()?;

        // A tile with procedural walls: lowered to buildings of their own, which go into a pack with the tile.
        if let Some((files, schema)) = &self.walls {
            let schema = rpfm_lib::schema::Schema::load(schema, None).map_err(|err| format!("{}: {err}", schema.display()))?;
            let (pack, saved, notes) = wce_walls::export::save_exported_walls(&game, files.as_ref(), &schema, &self.project_dir, self.text, out)?;
            for n in &notes {
                log::info(format!("{}: {n}", saved.display()));
            }

            return Ok((Exported { dir: saved, pack: Some(pack), notes: notes.len() }, game));
        }

        let (scene, _) = wce_tile::project_files::to_ted(self.text)?;
        let (saved, notes) = wce_tile_io::attila::workflow::save_exported_scene(&game, &self.project_dir, Some(scene), out)?;
        for n in &notes.notes {
            log::info(format!("{}: {n}", saved.display()));
        }

        Ok((Exported { dir: saved, pack: None, notes: notes.notes.len() }, game))
    }
}

pub struct TileTool {
    pub status: String,
    pub(crate) mode: TileMode,
    pub(crate) brush: BrushSettings,
    pub(crate) snap_grid: bool,
    pub(crate) grid_m: f32,

    /// TEd's grid_snap_tolerance: 0 snaps to the grid always; above, an axis snaps only when within this of a grid line (metres).
    pub(crate) snap_tolerance: f32,

    /// TEd's SF_SNAP_TO_VERTICES: onto the nearest instance or polygon point within the tolerance (1.5 m when it is 0).
    pub(crate) snap_vertices: bool,

    /// The cursor on the ground with the snapping applied (what placing uses).
    pub(crate) snapped_cursor: Float3,

    /// A grab's offset along the surface normal (PageUp / PageDown; freeform items only), metres.
    pub(crate) grab_normal_offset: f32,
    pub(crate) angle_step: f32,

    // The off-terrain report of the Tile panel.
    off_tol: f32,
    off_list: Vec<OffTerrain>,
    off_doc_version: Option<u64>,
    off_doc: DocumentId,

    // Scene panel: the flattened tree (open layers by name path), rename, delete confirmation, anchor of a shift-click range.
    pub(crate) rename_buf: String,
    pub(crate) tree_rows: Vec<TreeRow>,
    pub(crate) tree_open: HashSet<String>,
    pub(crate) tree_doc: DocumentId,
    pub(crate) tree_version: Option<u64>,
    pub(crate) tree_open_version: u64,
    pub(crate) tree_open_version_seen: Option<u64>,

    /// The Scene panel's filter (name or type, case-insensitive) and the one the rows were built for.
    pub(crate) tree_filter: String,
    pub(crate) tree_filter_seen: String,

    /// The hidden types the tree was built with (the type toggles).
    pub(crate) tree_hidden_seen: Option<Vec<String>>,

    /// TEd's working layer: the tile (document) and the layer's name path (`scene_panel::layer_key`). New items go into it (placed buildings,
    /// drawn polygons, groups, ungrouped items); without one they go to the scene graph.
    pub(crate) working_layer: Option<(DocumentId, String)>,

    /// The prefab library (`prefab_panel.rs`), scanned on first use; its changes (a rescan, a new prefab, an edit started or ended) are
    /// counted so that the Scene tree is built again.
    pub(crate) prefabs: Option<Arc<wce_tile::prefabs::PrefabLibrary>>,
    pub(crate) prefab_version: u64,

    /// The library's prefab contents, read once each and shared with the open tiles' projects (`TileProject::prefab_base`: a save keeps
    /// what the references differ by as their overrides), and the index of their contents (`prefab_refs::ContentIndex`).
    pub(crate) prefab_base: crate::prefab_panel::SharedBase,
    pub(crate) tree_prefab_seen: u64,

    /// The prefab copy being edited: the tile, the copy's name path (`scene_panel::layer_key`) and the prefab's key.
    pub(crate) prefab_edit: Option<(DocumentId, String, String)>,

    /// The copy opened for its items (`prefab_panel::opened_copy`): the tile and its name path; the selection the frame before (a click on an
    /// item of a copy selected then opens it).
    pub(crate) prefab_open: Option<(DocumentId, String)>,
    pub(crate) prefab_sel_before: Vec<usize>,

    /// The selection came from a click on a Scene tree row this frame: an item of a copy picked there opens the copy.
    pub(crate) tree_picked: bool,

    /// The Scene tree's override marks: each reference's overridden items (`TileDocument::overridden_items`), found when a row of it is
    /// drawn, for the (tile, history, library) they were found for.
    pub(crate) override_rows: (OverrideMarksKey, HashMap<usize, HashSet<usize>>),

    /// "Save as prefab": the group's name path, the name and the folder typed.
    pub(crate) prefab_save: Option<(String, String, String)>,

    /// The first selected element the tree last followed (a new one picked elsewhere is opened up to and scrolled into view), and the row
    /// to scroll to.
    pub(crate) tree_synced: Option<usize>,
    pub(crate) tree_scroll_to: Option<usize>,
    pub(crate) rename_elem: Option<usize>,
    pub(crate) anchor: Option<usize>,
    pub(crate) rename_wait: i32,
    pub(crate) rename_focus: bool,
    pub(crate) rename_active_seen: bool,
    pub(crate) begin_rename_next: bool,
    pub(crate) delete_confirm: bool,
    pub(crate) delete_weight: usize,

    // The cursor on the terrain (from the last hover / press).
    pub(crate) terrain_hit: bool,
    pub(crate) cursor: Float3,

    /// The blend channel the paint brush lays (4 * group + variation), and whether the stroke going on erases (Ctrl: paints channel 0).
    pub(crate) paint_channel: u8,
    pub(crate) paint_erase: bool,

    /// The ground type the ground types brush paints (`wce_tile::ground_types`), and the tile and look the ground types view replaced
    /// while it is active.
    pub(crate) ground_type: u8,
    view_before_ground: Option<(DocumentId, TerrainLook)>,

    /// The brushes' falloff curve (read from the settings when first needed), the Brush Falloff window, the point dragged in it, and
    /// whether the curve changed since it was last saved (a drag going on).
    pub(crate) curve: Option<crate::settings::BrushCurve>,
    pub(crate) falloff_open: bool,
    pub(crate) falloff_drag: Option<usize>,
    pub(crate) falloff_unsaved: bool,

    /// The image brush setting (read from the settings when first needed).
    pub(crate) image_brush: Option<crate::settings::ImageBrush>,

    /// The Create panel's thumbnails.
    pub(crate) thumbs: crate::thumbnails::Thumbnails,

    /// The name typed for a camera view to save.
    pub(crate) view_name: String,

    /// The box select, the selections saved for the session and the name typed for one.
    pub(crate) box_select: crate::selection_tools::BoxSelect,
    pub(crate) saved_selections: Vec<crate::selection_tools::SavedSelection>,
    pub(crate) selection_name: String,

    /// The Tile links editor: the picked target (true) or rule and its index, the kit's link set names, whether a number is being typed.
    pub(crate) link_pick: Option<(bool, usize)>,
    pub(crate) link_sets: Option<(std::path::PathBuf, Vec<String>)>,
    pub(crate) link_typing: bool,
    pub(crate) prop_typing: bool,

    /// The Save tile as and Resize tile dialogs.
    pub(crate) save_as: Option<crate::save_tile_as::SaveAsUi>,
    pub(crate) resize: Option<crate::resize_tile::ResizeUi>,

    /// The Tile database window.
    pub(crate) tile_db: Option<crate::tile_database::DbUi>,

    /// The tiles that can follow (Tile links): for which tile and history version, the open tile as the database reads it, the candidates,
    /// the database.
    #[allow(clippy::type_complexity)]
    pub(crate) link_candidates: Option<((DocumentId, u64), wce_tile::battle_map::tile_db::Tile, Vec<wce_tile::battle_map::link_candidates::Candidate>, std::sync::Arc<wce_tile::battle_map::tile_db::TileDatabase>)>,

    /// What Copy / Cut took (any tile of the session can paste it).
    pub(crate) clipboard: Vec<wce_tile_editor::tile_document::CopiedItem>,

    /// The point light whose radius handle is being dragged, and the drag's undo gesture.
    light_drag: Option<usize>,
    light_gesture: u64,

    /// The radius minus where the mouse grabbed the handle (the handle does not jump to the mouse).
    light_grab: f32,

    /// The civilian shelter whose target handle is being dragged, and the drag's undo gesture.
    target_drag: Option<usize>,
    target_gesture: u64,

    /// The water depth view (`water_view.rs`): the tile and version its depth was worked out for, and the version last seen with when it
    /// started being (the settling after an edit).
    pub(crate) water_for: Option<(DocumentId, u64)>,
    pub(crate) water_seen: Option<(DocumentId, u64)>,
    pub(crate) water_since: f64,

    /// Paint water ground types' depths: shallow water from the first, deep water from the second (metres).
    pub(crate) water_shallow_m: f32,
    pub(crate) water_deep_m: f32,

    /// The paint brush's Blendmap Filters: each on or off, with its range (kept while off), as TEd's dialog has them.
    pub(crate) filter_height_on: bool,
    pub(crate) filter_height: [f32; 2],
    pub(crate) filter_slope_on: bool,
    pub(crate) filter_slope: [f32; 2],

    /// Auto-Flange's threshold and Normal (Grouped)'s variation, kept while another paint mode is picked.
    pub(crate) flange_threshold: f32,
    pub(crate) grouped_variation: f32,

    // Gestures.
    stroke_active: bool,
    pub(crate) grabbing: bool,
    pub(crate) grab_elem: Option<usize>,
    pub(crate) grab_y_offset: f64,
    grab_history: usize,
    pub(crate) grab_gesture: u64,
    pub(crate) poly_point: Option<usize>,

    /// A spline's handle of `poly_point` is dragged, not the point: its controlOut (true) or controlIn.
    spline_handle: Option<bool>,

    /// The spline values panel edits every point, not only the selected one; the gesture of the value being dragged.
    spline_all: bool,
    value_gesture: u64,
    point_drag: bool,
    point_gesture: u64,
    pub(crate) drawing: bool,
    pub(crate) draw_pts: Vec<Float3>,

    /// What the Create panel makes: clicks on the terrain place it now (`placing`) or draw it (`drawing`); the draft (its type, key and
    /// values; none for a prefab) and its label.
    pub(crate) placing: Option<Placing>,
    pub(crate) draft: Option<wce_tile_editor::tile_document::Draft>,
    pub(crate) making: Option<String>,
    pub(crate) create: crate::create_panel::CreateUi,

    /// The dock node of the Tiles panel (where the Create panel goes the first time).
    pub(crate) left_dock: Option<gui::ImGuiID>,

    /// The viewport's right-click menu opens on the next frame.
    pub(crate) context_menu: bool,

    /// The placement chips' modes (read from the settings on first use), the surface under the mouse for Entity: Above (and the pixel it
    /// was found at), and the chips' width last frame.
    pub(crate) modes: Option<(crate::settings::SurfaceMode, crate::settings::RotationMode)>,
    pub(crate) surface_hit: Option<crate::placement_modes::SurfacePoint>,
    pub(crate) surface_px: Option<(i32, i32)>,
    pub(crate) place_chips_width: f32,

    /// The vegetation brush's settings (read from the settings on first use), its stroke, and its random numbers.
    pub(crate) veg: Option<crate::settings::VegBrush>,
    pub(crate) stroke: Option<crate::veg_brush::Stroke>,
    pub(crate) random: crate::veg_brush::Random,

    /// The vegetation sets (read on first use, again after a change, counted by the version) and the one being placed or painted.
    pub(crate) veg_sets: Option<std::sync::Arc<Vec<wce_tile::vegetation_sets::VegetationSet>>>,
    pub(crate) veg_sets_version: u64,
    pub(crate) veg_set: Option<SetInUse>,

    /// The "New vegetation set" dialog's name (and whether it opens on the next frame).
    pub(crate) new_set_name: Option<String>,
    pub(crate) new_set_open: bool,

    /// The "New tile" dialog (and whether it opens on the next frame).
    pub(crate) new_tile: Option<crate::new_tile_dialog::NewTileDialog>,

    /// Tile > Import height map...'s dialog, and whether it opens at the next frame.
    pub(crate) height_import: Option<crate::height_import_dialog::HeightImportDialog>,
    pub(crate) height_import_open: bool,
    pub(crate) new_tile_open: bool,

    /// The question before an exported tile is extracted over a project (and whether it opens on the next frame).
    pub(crate) extract_prompt: Option<crate::extract_prompt::ExtractPrompt>,
    pub(crate) extract_prompt_open: bool,

    /// The question which of several kits "Save for Assembly Kit" writes to (`kit_prompt.rs`).
    pub(crate) kit_prompt: Option<crate::kit_prompt::KitPrompt>,
    pub(crate) kit_prompt_open: bool,

    /// Launch in game (`launch_prompt.rs`): the game launched (waited for, then its launch pack removed), whether a pack left by an earlier
    /// run was looked for, and the question about stray movie-type packs (their names; opens on the next frame).
    pub(crate) launched: Option<crate::launch::Running>,
    pub(crate) launch_swept: bool,
    pub(crate) launch_prompt: Option<Vec<String>>,
    pub(crate) launch_prompt_open: bool,

    /// The launch being prepared on a thread, and whether its window opens on the next frame.
    pub(crate) launch_job: Option<crate::launch_prompt::LaunchJob>,
    pub(crate) launch_worker: Option<std::thread::JoinHandle<()>>,
    pub(crate) launch_progress_open: bool,

    /// The Tiles panel's extracted tiles (keys of the projects' rows), found when the rows are.
    browse_extracted: std::collections::HashSet<String>,

    /// The new vegetation set (not saved; Ctrl + click on trees in the Create panel) and the name it will be saved with.
    pub(crate) scratch_set: Option<wce_tile::vegetation_sets::VegetationSet>,
    pub(crate) scratch_name: String,

    /// The Problems section's trees, and the (tile, history, structure) they were found for.
    problems: Option<((DocumentId, u64, u64), Vec<usize>)>,

    /// The rivers against the link targets (`TileDocument::river_link_problems`), for the same stamp.
    river_problems: Option<((DocumentId, u64, u64), Vec<(Option<usize>, String)>)>,
    link_problems: Option<((DocumentId, u64, u64), Vec<wce_tile::link_checks::LinkProblem>)>,

    /// The next open takes a kit's tile as it is in the kit, not its project (Import from Assembly Kit).
    pub(crate) open_kit_copy: bool,

    /// The weather chips' width last frame (they go beside the compass when there is room).
    chips_width: f32,

    /// The right click ended a grab: its release opens no menu.
    right_used: bool,

    /// The dock node of the Tile panel (where the Weather panel goes the first time).
    pub(crate) right_dock: Option<gui::ImGuiID>,

    /// The keys of each type, per tile.
    catalogues: HashMap<(DocumentId, String), Arc<Vec<String>>>,

    /// The text of the prefab being placed (read once per placing).
    pub(crate) placing_prefab_text: Option<(String, Vec<u8>)>,

    /// The preview is shown at `cursor` without the mouse over the viewport (`place_preview`, for scripts).
    pub(crate) preview_forced: bool,
    camera_set_for: DocumentId,

    /// mark_height values for the tests.
    pub(crate) marks: HashMap<String, f32>,

    // The tile browser (Tiles panel), and the entry a pending open comes from (consumed by `open`).
    browse_filter: String,

    /// Frames left in which the Tiles panel takes the focus (the first time the tool is shown).
    browse_focus_frames: u8,

    /// Frames left to bring the Scene tree to the front (a tile opened).
    pub(crate) scene_focus_frames: u8,
    browse_kit: usize,
    browse_kind: i32,

    /// The entry's key (normalised location) selected in the list.
    browse_selected: String,
    pending_entry: Option<PathBuf>,
    pending_variation: Option<usize>,

    /// kit_roots, looked up at most every two seconds (the panel draws every frame).
    browse_roots: Vec<PathBuf>,
    browse_roots_time: f64,

    /// The filtered, listed rows: indices of the index's entries (rebuilt when the kit, kind, filter or index changed).
    browse_rows: Vec<usize>,
    browse_rows_key: String,
    browse_index: Option<Arc<TileIndex>>,

    /// The kit index's rows, shared as the packs' are (copied when the index changes).
    browse_kit_rows: Arc<Vec<wce_formats::tile_index::TileIndexEntry>>,

    /// The favourite and the recent tiles (the settings' lists: read on first use, written through), and a count of their changes.
    browse_marks: Option<(Vec<TileRef>, Vec<TileRef>)>,
    browse_marks_version: u64,

    /// The listed favourites and recent tiles of the source, filtered as the list: rows of the source (and, for a recent one, the variation
    /// opened), and the keys of the source's favourites (the stars of every row).
    browse_fav_rows: Vec<usize>,
    browse_recent_rows: Vec<(usize, String)>,
    browse_fav_keys: HashSet<String>,

    /// The raw_data folders of the workspaces exported tiles were opened into this session (besides the settings' one): their tiles are the
    /// packs' in the recent tiles.
    workspaces: HashSet<String>,

    /// The Weather section (which environment file lights the tile).
    pub(crate) weather: crate::weather_panel::WeatherUi,

    /// The Map context section (the tiles around the open one).
    pub(crate) context: crate::context_panel::ContextUi,

    /// The game's tile data for opening and saving exported tiles (read once per game folder).
    pub(crate) exported_game: Option<Arc<wce_tile_io::attila::workflow::Game>>,

    /// The Tiles panel's "game packs" source: the tile databases in effect as index rows (no entry file), or why they could not load.
    browse_packs: Option<Result<Arc<Vec<wce_formats::tile_index::TileIndexEntry>>, String>>,
}

impl Default for TileTool {
    fn default() -> Self {
        Self {
            status: String::new(), mode: TileMode::Select, brush: BrushSettings::default(), snap_grid: false, grid_m: 6.0, snap_tolerance: 0.0, snap_vertices: false, snapped_cursor: Float3::default(), grab_normal_offset: 0.0, angle_step: 15.0,
            off_tol: 0.5, off_list: vec![], off_doc_version: None, off_doc: 0,
            rename_buf: String::new(), tree_rows: vec![], tree_open: HashSet::new(), tree_doc: 0, tree_version: None, tree_open_version: 0,
            tree_open_version_seen: None, tree_filter: String::new(), tree_filter_seen: String::new(), tree_hidden_seen: None, working_layer: None, prefabs: None, prefab_version: 0, prefab_base: Default::default(), tree_prefab_seen: 0, prefab_edit: None, prefab_open: None, prefab_sel_before: vec![], tree_picked: false, override_rows: Default::default(),
            prefab_save: None, tree_synced: None, tree_scroll_to: None, rename_elem: None, anchor: None, rename_wait: 0, rename_focus: false, rename_active_seen: false,
            begin_rename_next: false, delete_confirm: false, delete_weight: 0, terrain_hit: false, cursor: Float3::default(), paint_channel: 0, paint_erase: false, ground_type: 1, view_before_ground: None, curve: None, falloff_open: false, falloff_drag: None, falloff_unsaved: false, image_brush: None, thumbs: Default::default(), view_name: String::new(), box_select: Default::default(), saved_selections: vec![], selection_name: String::new(), link_pick: None, link_sets: None, link_typing: false, prop_typing: false, save_as: None, resize: None, tile_db: None, link_candidates: None, clipboard: vec![], light_drag: None, light_gesture: 0, light_grab: 0.0, target_drag: None, target_gesture: 0, water_for: None, water_seen: None, water_since: 0.0, water_shallow_m: 0.3, water_deep_m: 2.5, filter_height_on: false, filter_height: [-1000.0, 1000.0], filter_slope_on: false,
            filter_slope: [wce_tile::blend_field::NOGO_SLOPE_DEGREES, 90.0], flange_threshold: 0.0, grouped_variation: 1.5, stroke_active: false,
            grabbing: false, grab_elem: None, grab_y_offset: 0.0, grab_history: 0, grab_gesture: 0, poly_point: None, spline_handle: None, spline_all: false, value_gesture: 0, point_drag: false, point_gesture: 0,
            drawing: false, draw_pts: vec![], placing: None, draft: None, making: None, create: Default::default(), left_dock: None, right_dock: None, context_menu: false, modes: None, surface_hit: None, surface_px: None, place_chips_width: 0.0, veg: None, stroke: None, random: crate::veg_brush::Random::new(), veg_sets: None, veg_sets_version: 0, veg_set: None, new_set_name: None, new_set_open: false, new_tile: None, new_tile_open: false, height_import: None, height_import_open: false, extract_prompt: None, extract_prompt_open: false, kit_prompt: None, kit_prompt_open: false, launched: None, launch_swept: false, launch_prompt: None, launch_prompt_open: false, launch_job: None, launch_worker: None, launch_progress_open: false, browse_extracted: Default::default(), scratch_set: None, scratch_name: String::new(), problems: None, river_problems: None, link_problems: None, open_kit_copy: false, chips_width: 0.0, right_used: false, catalogues: HashMap::new(), placing_prefab_text: None, preview_forced: false, camera_set_for: 0, marks: HashMap::new(), browse_filter: String::new(), browse_focus_frames: 3, scene_focus_frames: 0,
            browse_kit: 0, browse_kind: 0, browse_selected: String::new(), pending_entry: None, pending_variation: None, browse_roots: vec![],
            browse_roots_time: -1e9, browse_rows: vec![], browse_rows_key: String::new(), browse_index: None, browse_kit_rows: Arc::default(),
            browse_marks: None, browse_marks_version: 0, browse_fav_rows: vec![], browse_recent_rows: vec![], browse_fav_keys: HashSet::new(), workspaces: HashSet::new(),
            weather: Default::default(), context: Default::default(), exported_game: None, browse_packs: None,
        }
    }
}

/// The tile the tool works on: the active document when it is a tile, else the first open one.
pub fn document_id(e: &Editor) -> Option<DocumentId> {
    let active = e.docs.active_id();
    if e.docs.find_as::<TileDocument>(active).is_some() {
        return Some(active);
    }

    e.docs.order().iter().copied().find(|&id| e.docs.find_as::<TileDocument>(id).is_some())
}

pub fn document(e: &mut Editor) -> Option<&mut TileDocument> {
    let id = document_id(e)?;
    e.docs.find_as_mut::<TileDocument>(id)
}

/// The selected elements of the tile (not the terrain).
pub fn selected(e: &Editor) -> Vec<usize> {
    let Some(id) = document_id(e) else { return vec![] };
    let count = e.docs.find_as::<TileDocument>(id).map_or(0, |d| d.tile().tscene().doc().elements().len());

    // A piece of a building stands for its building (the viewport shows the building's box); an element that is gone (removed, undone) is
    // not selected.
    e.selection.items().iter().filter(|r| r.doc == id && r.item != TERRAIN).filter_map(|r| elem_of(r.item).or_else(|| piece_of(r.item).map(|(e, _)| e))).filter(|&el| el < count).collect()
}

/// Takes what is gone (removed, undone: its index is past the tile's elements) out of the editor's selection.
pub(crate) fn drop_gone_from_selection(e: &mut Editor) {
    let Some(id) = document_id(e) else { return };
    let count = e.docs.find_as::<TileDocument>(id).map_or(0, |d| d.tile().tscene().doc().elements().len());
    let gone = |r: &SourceRef| r.doc == id && r.item != TERRAIN && elem_of(r.item).or_else(|| piece_of(r.item).map(|(e, _)| e)).is_some_and(|el| el >= count);
    if e.selection.items().iter().any(gone) {
        let keep = e.selection.items().iter().filter(|r| !gone(r)).copied().collect::<Vec<_>>();
        e.selection.clear();
        for r in keep {
            e.selection.add(r);
        }
    }
}

/// The first selected instance element.
pub fn primary(e: &Editor) -> Option<usize> {
    let d = e.docs.find_as::<TileDocument>(document_id(e)?)?;
    selected(e).into_iter().find(|&el| d.tile().is_instance(el))
}

pub fn select_elem(e: &mut Editor, elem: Option<usize>, add: bool) {
    let Some(id) = document_id(e) else { return };
    let Some(elem) = elem else {
        if !add {
            e.selection.clear();
        }

        return;
    };

    let r = SourceRef { doc: id, item: wce_tile_editor::tile_document::item_of(elem) };
    if add {
        e.selection.add(r);
    } else {
        e.selection.set(r);
    }

    e.docs.set_active(id);
}

fn tile_dir_of(path: &str) -> PathBuf {
    let p = PathBuf::from(path);
    if p.is_dir() { p } else { p.parent().map(Path::to_path_buf).unwrap_or_default() }
}

/// The entries (file, bytes) of a projects' raw_data whose location is `location` (`terrain/tiles/<kind>/...`, `/` separated).
fn project_entries(raw: &Path, location: &str) -> Vec<(PathBuf, Vec<u8>)> {
    let kind = location.split('/').nth(2).unwrap_or("battle");
    let Ok(read) = std::fs::read_dir(raw.join("terrain/tiles").join(kind).join("_tile_database/TILES")) else { return vec![] };
    read.flatten()
        .map(|f| f.path())
        .filter_map(|f| {
            let bytes = std::fs::read(&f).ok()?;
            let entry = TileEntry::parse(bytes.clone()).ok()?;
            (wce_formats::tile_entry::normalize_location(&entry.location()) == location).then_some((f, bytes))
        })
        .collect()
}

/// Runs `f` on the tile tool (a command reaching its tool).
pub(crate) fn with_tile(e: &mut Editor, f: impl FnOnce(&mut TileTool, &mut Editor) -> CommandResult) -> CommandResult {
    e.with_tool::<TileTool, _>(f).unwrap_or_else(|| CommandResult::fail("the tile tool is not available"))
}

impl TileTool {
    pub fn mode(&self) -> TileMode {
        self.mode
    }

    /// The Blendmap Filters into the brush.
    pub(crate) fn sync_paint_filter(&mut self) {
        let f = wce_tile::blend_field::PaintFilter {
            height: self.filter_height_on.then_some(self.filter_height), slope: self.filter_slope_on.then_some(self.filter_slope),
        };

        self.brush.filter = f.is_active().then_some(f);
    }

    /// The brush the left button strokes with: the mode's, a paint brush with its channel (channel 0 while erasing).
    pub(crate) fn brush_kind(&self) -> Option<BrushKind> {
        match self.mode.brush()? {
            BrushKind::Paint(_) => Some(BrushKind::Paint(if self.paint_erase { 0 } else { self.paint_channel })),
            BrushKind::Ground(_) => Some(BrushKind::Ground(if self.paint_erase { GROUND_ERASE } else { self.ground_type })),
            kind => Some(kind),
        }
    }

    pub fn set_mode(&mut self, m: TileMode) {
        self.mode = m;
        if m != TileMode::Select {
            self.drawing = false;
        }
    }

    pub fn start_drawing(&mut self) {
        self.drawing = true;
        self.draw_pts.clear();
    }

    /// The raw_data folders the tile browser lists: `$WCE_TILE_KITS` (';' separated, for the tests), else the game's Assembly Kits
    /// (`Settings::assembly_kit_raw_dirs`) that have tiles, and the projects in the kit's layout (`tile.wce` tiles, extracted ones among them).
    pub fn kit_roots(&self, e: &mut Editor) -> Vec<PathBuf> {
        // The index of the kits' tile database entries keeps its rows in the cache folder between runs.
        TileIndex::set_cache_dir(&e.settings_ref().paths().cache_dir().join("tile_index"));
        if let Some(kits) = env_value("WCE_TILE_KITS") {
            return kits.split(';').filter(|k| !k.is_empty()).map(PathBuf::from).collect();
        }

        let mut roots = e.settings_ref().assembly_kit_raw_dirs();
        roots.retain(|raw| raw.join("terrain").join("tiles").is_dir());
        let projects = Self::workspace_dir(e).join("raw_data");
        if projects.join("terrain").join("tiles").is_dir() {
            roots.insert(0, projects);
        }

        roots
    }

    /// The working layer of the tile `id`, if one is set and still there.
    pub(crate) fn working_layer_of(&self, d: &TileDocument) -> Option<usize> {
        let (doc, key) = self.working_layer.as_ref().filter(|(doc, _)| *doc == d.id())?;
        let _ = doc;
        crate::scene_panel::layer_by_key(d.tile().tscene(), key)
    }

    /// The keys of a type in the open tile's files (read once per tile).
    pub(crate) fn catalogue(&mut self, e: &mut Editor, kind: &str) -> Arc<Vec<String>> {
        let Some(d) = document(e) else { return Arc::default() };
        self.catalogues.entry((d.id(), kind.to_owned())).or_insert_with(|| Arc::new(d.tile().key_catalogue(kind))).clone()
    }

    /// Where new items go in the tile: the working layer, else the scene graph.
    pub(crate) fn new_items_layer(&self, d: &TileDocument) -> Option<usize> {
        self.working_layer_of(d).or_else(|| d.tile().tscene().scene_graph())
    }

    /// A layer's path including itself (`root` for the scene graph).
    pub(crate) fn layer_label(d: &TileDocument, layer: usize) -> String {
        let sc = d.tile().tscene();
        if sc.type_of(layer) == "SCENE_GRAPH" {
            return "root".to_owned();
        }

        let above = sc.layer_path(layer);
        if above.is_empty() { sc.name(layer) } else { format!("{above}/{}", sc.name(layer)) }
    }

    /// Makes a layer the working layer (`None`: none, new items go to the scene graph).
    pub fn set_working_layer(&mut self, d: &TileDocument, layer: Option<usize>) -> Result<String, String> {
        let Some(layer) = layer else {
            self.working_layer = None;
            return Ok("no working layer: new items go to the root".to_owned());
        };

        let sc = d.tile().tscene();
        if !matches!(sc.type_of(layer), "LayerInstance" | "SCENE_GRAPH") {
            return Err("only a layer can be the working layer".to_owned());
        }

        let key = crate::scene_panel::layer_key(sc, layer).ok_or("that layer is not in the scene graph")?;
        self.working_layer = Some((d.id(), key));
        Ok(format!("working layer: {}", Self::layer_label(d, layer)))
    }

    /// Groups the selection into an instance group in the working layer (Ctrl+G).
    pub fn group_selection(&mut self, e: &mut Editor) -> Result<String, String> {
        let sel = selected(e);
        let Some(d) = document(e) else { return Err("no tile open".to_owned()) };
        let into = self.new_items_layer(d).ok_or("this tile has no scene graph")?;
        d.group_instances(&sel, into, "").map(|_| "grouped".to_owned())
    }

    /// Takes the selected instance groups apart into the working layer (Ctrl+U).
    pub fn ungroup_selection(&mut self, e: &mut Editor) -> Result<String, String> {
        let sel = selected(e);
        let Some(d) = document(e) else { return Err("no tile open".to_owned()) };
        let into = self.new_items_layer(d).ok_or("this tile has no scene graph")?;
        d.ungroup(&sel, into).map(|freed| format!("ungrouped: {} items", freed.len()))
    }

    /// The Tiles panel's source as a [`TileRef`] source: the kit's raw_data folder, or the packs.
    pub(crate) fn browse_source(&self) -> String {
        match self.browse_roots.get(self.browse_kit) {
            Some(root) => source_of(root),
            None => PACKS_SOURCE.to_owned(),
        }
    }

    /// The favourite and the recent tiles, read from the settings on first use.
    pub(crate) fn marks(&mut self, e: &Editor) -> &(Vec<TileRef>, Vec<TileRef>) {
        if self.browse_marks.is_none() {
            let s = e.tool_settings::<TileSettings>(SETTINGS_ID);
            self.browse_marks = Some((s.favourites, s.recent));
        }

        self.browse_marks.as_ref().expect("read above")
    }

    fn save_marks(&mut self, e: &mut Editor) {
        let Some((favourites, recent)) = self.browse_marks.clone() else { return };
        self.browse_marks_version += 1;
        e.update_tool_settings(SETTINGS_ID, |s: &mut TileSettings| {
            s.favourites = favourites;
            s.recent = recent;
        });
    }

    /// Stars a tile of a source in the Tiles panel, or takes its star away; true when it is a favourite now.
    pub fn toggle_favourite(&mut self, e: &mut Editor, source: &str, key: &str) -> bool {
        self.marks(e);
        let (favourites, _) = self.browse_marks.as_mut().expect("read above");
        let now = match favourites.iter().position(|r| r.source == source && r.key == key) {
            Some(i) => {
                favourites.remove(i);
                false
            }

            None => {
                favourites.push(TileRef { source: source.to_owned(), key: key.to_owned(), variation: String::new() });
                true
            }
        };

        self.save_marks(e);
        now
    }

    /// Puts a tile that opened first in the recent tiles: its kit's source (the packs for an exported tile in the workspace), its entry and
    /// the variation. A tile without an entry or a kit is not listed in the Tiles panel and is not kept.
    fn note_recent(&mut self, e: &mut Editor, id: DocumentId) {
        let Some(d) = e.docs.find_as::<TileDocument>(id) else { return };
        let p = d.project();
        let (Some(entry), Some(raw_root)) = (&p.entry, &p.raw_root) else { return };
        let key = wce_formats::tile_entry::normalize_location(&entry.location());
        let variation = p.dir.file_name().map(|f| f.to_string_lossy().into_owned()).unwrap_or_default();
        // A tile of the projects (extracted ones too) is listed with the projects' source.
        let raw_root = source_of(raw_root);
        let exported = self.workspaces.contains(&raw_root) && raw_root != source_of(&Self::workspace_dir(e).join("raw_data"));
        let source = if exported { PACKS_SOURCE.to_owned() } else { raw_root };
        self.marks(e);
        let (_, recent) = self.browse_marks.as_mut().expect("read above");
        recent.retain(|r| r.source != source || r.key != key);
        recent.insert(0, TileRef { source, key, variation });
        recent.truncate(RECENT_KEEP);
        self.save_marks(e);
    }

    /// Shows or hides the sea at 0 m: the tile's plane and the map context's sea; kept in the settings for the next tiles.
    pub fn set_show_sea(&mut self, e: &mut Editor, id: DocumentId, on: bool) {
        if let Some(d) = e.docs.find_as_mut::<TileDocument>(id) {
            d.tile_mut().display_mut().show_sea = on;
            d.tile_mut().display_changed();
        }

        self.context.show_sea(e, on);
        e.update_tool_settings(SETTINGS_ID, |s: &mut TileSettings| s.show_sea = on);
    }

    /// Opens a tile by its database entry (an _tile_database/TILES/*.xml path, or a name like "assembly_kit/osgiliath_test" / the entry's
    /// file name), by a variation folder, a tile folder or a tile.ted. `variation` picks one of the entry's variations (a leaf folder name or a
    /// number; empty = the first).
    ///
    /// # Errors
    ///
    /// What could not be found or opened.
    pub fn open_tile(&mut self, e: &mut Editor, target: &str, variation: &str) -> Result<String, String> {
        let p = Path::new(target);
        let ext = p.extension().map(|x| format!(".{}", x.to_string_lossy().to_lowercase())).unwrap_or_default();
        let mut entry_xml = None;
        if p.is_file() && ext == ".xml" {
            entry_xml = Some(p.to_path_buf());
        } else if !p.exists() {
            // A name: the entry's file name, "<set>/<tile>" or its location.
            for root in self.kit_roots(e) {
                if let Some(entry) = TileIndex::shared(&root).find_name(target) {
                    entry_xml = Some(entry.xml.clone());
                    break;
                }
            }

            if entry_xml.is_none() {
                return Err(format!("no file, folder or tile entry '{target}'"));
            }
        } else if p.is_dir() && !wce_tile::tile_project::has_tile(p) {
            // A tile folder: its entry lists the variations.
            let found = kit_raw_root(p).and_then(|root| TileIndex::shared(&root).resolve(p).entry.map(|entry| entry.xml.clone()));
            match found {
                Some(xml) => entry_xml = Some(xml),
                None => return Err(format!("no tile.ted in {target} and no tile database entry for it")),
            }
        }

        let Some(entry_xml) = entry_xml else { return self.open_folder(e, target) };
        let entry = TileEntry::load(&entry_xml).map_err(|err| err.to_string())?;
        let file_name = entry_xml.file_name().map(|f| f.to_string_lossy().into_owned()).unwrap_or_default();
        let Some(root) = entry.raw_root() else {
            return Err(format!("{} is not inside <kit>/raw_data/terrain/tiles/<kind>/_tile_database/TILES", entry_xml.display()));
        };

        let vars = entry.variations();
        let names = vars.iter().map(|v| v.location.as_str()).collect::<Vec<_>>().join(", ");
        let mut v = 0;
        if !variation.is_empty() {
            let n = match variation.parse::<i64>() {
                Ok(n) => usize::try_from(n).ok(),
                Err(_) => entry.find_variation(variation),
            };

            match n.filter(|&n| n < vars.len()) {
                Some(n) => v = n,
                None => return Err(format!("the entry {file_name} has no variation '{variation}' (it has: {names})")),
            }
        }

        if vars.is_empty() {
            return Err(format!("{file_name} lists no variations"));
        }

        // A variation the project does not have yet (its entry lists every variation; a save brings one in): it opens from where it comes
        // from, the packs for an extracted tile (one of its variations in the project is marked), else the kit's copy.
        let mut entry_xml = entry_xml;
        let mut ted = entry.ted_path(&root, v);
        let projects = Self::workspace_dir(e).join("raw_data");
        if !wce_tile::tile_project::has_tile(&entry.variation_folder(&root, v)) && root == projects {
            let folder = wce_tile_editor::tile_document::kit::tile_folder(&entry.variation_folder(&root, v)).unwrap_or_default();
            if (0..vars.len()).any(|i| wce_tile::project_files::is_extracted_dir(&entry.variation_folder(&root, i))) {
                return self.open_exported(e, &folder, None, false);
            }

            let kind = folder.split('/').nth(2).unwrap_or("battle").to_owned();
            // The first kit that has the tile and its entry.
            let rel = entry_xml.strip_prefix(&root).ok().map(Path::to_path_buf);
            let found = self.kits_for(e, &kind).into_iter().find_map(|kit| {
                let kit_entry = kit.join(rel.as_ref()?);
                (kit_entry.is_file() && wce_tile::tile_project::has_tile(&kit.join(&folder))).then_some((kit, kit_entry))
            });
            match found {
                Some((kit, kit_entry)) => {
                    ted = entry.ted_path(&kit, v);
                    entry_xml = kit_entry;
                }

                None => return Err(format!("the project has no {folder} and no Assembly Kit has a copy of it")),
            }
        }

        // A project's variation folder holds tile.wce.
        let wce = ted.with_file_name(wce_tile::project_files::WCE_FILE);
        if !ted.exists() && wce.exists() {
            ted = wce;
        }

        self.pending_entry = Some(entry_xml.clone());
        self.pending_variation = Some(v);
        let opened = e.open_path_from(self, &ted.display().to_string(), false, true);
        self.pending_entry = None;
        self.pending_variation = None;
        let mut msg = opened?;
        if vars.len() > 1 {
            msg += &format!("  (entry {file_name}, variation {} of: {names})", vars[v].location);
        }

        Ok(msg)
    }

    /// The game's tile data for exported tiles (the packs' databases, buildings and models, the kit's vegetation parameters), read on first
    /// use from the game folder.
    pub(crate) fn exported_game(&mut self, e: &mut Editor) -> Result<Arc<wce_tile_io::attila::workflow::Game>, String> {
        let dir = e.settings_ref().game_dir().ok_or("no game folder (setup <game folder>)")?;
        if let Some(g) = self.exported_game.as_ref().filter(|g| g.dir == Path::new(&dir)) {
            return Ok(g.clone());
        }

        let game = Arc::new(wce_tile_io::attila::workflow::Game::load(Path::new(&dir))?);
        for w in &game.warnings {
            log::warn(format!("exported tiles: {w}"));
        }

        self.exported_game = Some(game.clone());
        Ok(game)
    }

    /// The tiles of the game's packs (the tile databases in effect, battle and campaign) as index rows, read on first use.
    fn pack_entries(&mut self, e: &mut Editor) -> Result<Arc<Vec<wce_formats::tile_index::TileIndexEntry>>, String> {
        if let Some(r) = &self.browse_packs {
            return r.clone();
        }

        let r = self.exported_game(e).map(|game| {
            let mut rows = vec![];
            for kind in ["battle", "campaign"] {
                let Some(db) = game.data.database(kind) else { continue };
                for t in db.tiles() {
                    let location = t.location().clone();
                    let key = wce_formats::tile_entry::normalize_location(&location);
                    let name = key.trim_start_matches(&format!("terrain/tiles/{kind}/")).replace('/', "_");
                    rows.push(wce_formats::tile_index::TileIndexEntry {
                        name,
                        kind: kind.to_owned(),
                        location,
                        key,
                        tile_set: t.tile_set().clone(),
                        width: *t.width(),
                        height: *t.height(),
                        red: *t.red(),
                        green: *t.green(),
                        blue: *t.blue(),
                        variations: t.variations().iter().map(|v| v.location().clone()).collect(),
                        links: t.links().len() as u32,
                        targets: t.link_targets().len() as u32,
                        ..Default::default()
                    });
                }
            }

            rows.sort_by(|a, b| a.key.cmp(&b.key));
            Arc::new(rows)
        });
        self.browse_packs = Some(r.clone());
        r
    }

    /// The Tiles panel's source (`packs` or a kit's number, from 0) and filter, as the panel's combo and text field set them.
    ///
    /// # Errors
    ///
    /// No such kit.
    pub fn browse(&mut self, e: &mut Editor, source: &str, filter: &str) -> Result<String, String> {
        let roots = self.kit_roots(e);
        self.browse_kit = match source {
            "packs" => roots.len(),
            n => n.parse::<usize>().ok().filter(|&n| n < roots.len()).ok_or_else(|| format!("no kit {n}: {} kits, or 'packs'", roots.len()))?,
        };
        self.browse_filter = filter.to_owned();
        gui::set_window_focus("Tiles");
        let rows = if source == "packs" { self.pack_entries(e)?.len() } else { TileIndex::shared(&roots[self.browse_kit]).entries().len() };
        Ok(format!("{rows} tiles in the source"))
    }

    /// The projects in the Assembly Kit's layout (`<projects>/assembly_kit`): where tiles are saved (`tile.wce`) and exported tiles are
    /// extracted to; `$WCE_TILE_WORKSPACE` for the tests.
    pub fn workspace_dir(e: &mut Editor) -> PathBuf {
        env_value("WCE_TILE_WORKSPACE").map(PathBuf::from).unwrap_or_else(|| e.settings_ref().kit_projects_dir())
    }

    /// The tile folder (`terrain/tiles/<kind>/.../<variation>`) of an exported tile: a folder on disk or the packs' folder.
    ///
    /// # Errors
    ///
    /// A folder on disk that is not under `terrain/tiles/`.
    pub fn exported_folder(target: &str) -> Result<String, String> {
        let p = Path::new(target);
        if p.is_dir() {
            wce_tile_io::attila::workflow::tile_folder(p).ok_or_else(|| format!("{target} is not under a terrain/tiles/ folder"))
        } else {
            Ok(target.replace('\\', "/").trim_matches('/').to_lowercase())
        }
    }

    /// The project an exported tile would be extracted into, when there is one already (extracted before, or the kit's tile of that name
    /// saved): what extracting it again would replace.
    pub fn extracted_project(e: &mut Editor, target: &str) -> Option<PathBuf> {
        let dir = Self::workspace_dir(e).join("raw_data").join(Self::exported_folder(target).ok()?);
        wce_tile::tile_project::has_tile(&dir).then_some(dir)
    }

    /// Opens an exported tile: a folder on disk (`.../terrain/tiles/<kind>/.../<variation>`) or the packs' (`terrain/tiles/...`). It is
    /// decompiled into a project in the workspace (`workspace`, else [`Self::workspace_dir`]) at its own folder, its `tile.wce` marked as an
    /// extracted tile, which then opens. A project there already (extracted before, or the kit's tile of that name saved) opens instead,
    /// unless `fresh`: then the tile is extracted over it (its variation folder replaced; the entry keeps the other variations' numbers).
    ///
    /// # Errors
    ///
    /// The tile can't be found, read, decompiled or opened, or the project to replace is open with unsaved changes.
    pub fn open_exported(&mut self, e: &mut Editor, target: &str, workspace: Option<&Path>, fresh: bool) -> Result<String, String> {
        use wce_tile::project_files::{TED_FILE, WCE_FILE, mark_extracted_text, to_wce};
        let workspace = workspace.map_or_else(|| Self::workspace_dir(e), Path::to_path_buf);
        self.workspaces.insert(source_of(&workspace.join("raw_data")));
        let folder = Self::exported_folder(target)?;
        let dir = workspace.join("raw_data").join(&folder);
        if wce_tile::tile_project::has_tile(&dir) {
            if !fresh {
                let msg = self.open_folder(e, &dir.display().to_string())?;
                return Ok(format!("{msg}  (the projects' copy; open_exported {folder} --fresh extracts the tile over it)"));
            }

            // Replaced: the document of it goes first (not with unsaved changes), then the folder.
            let open = e.docs.order().iter().copied().find(|&id| e.docs.find(id).is_some_and(|d| d.opened_from(&dir.display().to_string())));
            if let Some(id) = open {
                if e.docs.find(id).is_some_and(|d| d.modified()) {
                    return Err(format!("{folder} is open with unsaved changes: save or undo them before extracting the tile over it"));
                }

                e.close_document(id);
            }

            std::fs::remove_dir_all(&dir).map_err(|err| format!("{}: {err}", dir.display()))?;
        }

        let game = self.exported_game(e)?;
        let p = Path::new(target);
        let files = if p.is_dir() { wce_tile_io::TileFiles::from_dir(p).map_err(|err| err.to_string())? } else { game.exported_from_packs(&folder)? };

        // The decompile writes the whole entry: a project's entry keeps the other variations' numbers.
        let tile_location = folder.rsplit_once('/').map_or(folder.as_str(), |(t, _)| t).to_owned();
        let variation = folder.rsplit('/').next().unwrap_or_default().to_owned();
        let before = project_entries(&workspace.join("raw_data"), &tile_location);
        let (dir, losses) = wce_tile_io::attila::workflow::open_exported(&game, &files, &folder, &workspace)?;
        for n in &losses.notes {
            log::info(format!("{folder}: {n}"));
        }

        for (file, old) in before {
            if let (Ok(mut old), Ok(new)) = (TileEntry::parse(old), TileEntry::load(&file))
                && let (Some(o), Some(n)) = (old.find_variation(&variation), new.find_variation(&variation))
            {
                let v = &new.variations()[n];
                old.set_height_range(o, v.min_height, v.scale);
                std::fs::write(&file, old.save()).map_err(|err| format!("{}: {err}", file.display()))?;
            }
        }

        TileIndex::forget_shared();

        // The decompile writes TEd's tile.ted: the project keeps tile.wce, marked as an extracted tile.
        let ted = dir.join(TED_FILE);
        if ted.is_file() {
            let text = std::fs::read(&ted).map_err(|err| format!("{}: {err}", ted.display()))?;
            std::fs::write(dir.join(WCE_FILE), mark_extracted_text(to_wce(text)?)).map_err(|err| err.to_string())?;
            std::fs::remove_file(&ted).map_err(|err| err.to_string())?;
        }

        let msg = self.open_folder(e, &dir.display().to_string())?;
        Ok(format!("{msg}  (extracted into {}; {} notes in the log)", dir.display(), losses.notes.len()))
    }

    /// Moves the projects of tiles extracted by older versions (`<tile>_extracted`) to their own names (`project_files::migrate_extracted`);
    /// the open tiles' are left alone. The log lists every tile.
    ///
    /// # Errors
    ///
    /// The projects can't be read.
    pub fn migrate_extracted(&mut self, e: &mut Editor) -> Result<String, String> {
        let raw = Self::workspace_dir(e).join("raw_data");
        let busy = e.docs.order().iter().filter_map(|&id| e.docs.find_as::<TileDocument>(id).map(|d| d.project().dir.clone())).collect::<Vec<_>>();
        let m = wce_tile::project_files::migrate_extracted(&raw, &busy)?;
        for line in &m.moved {
            log::info(format!("migrated: {line}"));
        }

        for line in &m.skipped {
            log::warn(format!("not migrated: {line}"));
        }

        self.browse_rows_key.clear();
        Ok(format!("{} old extracted project(s) moved, {} left as they are (see the log)", m.moved.len(), m.skipped.len()))
    }

    /// Asks before an exported tile is extracted over a project (`extract_prompt.rs`), else extracts it.
    pub fn extract_or_ask(&mut self, e: &mut Editor, target: &str) -> String {
        if let Some(dir) = Self::extracted_project(e, target) {
            // Open already: shown.
            let path = dir.display().to_string();
            if e.docs.order().iter().any(|&id| e.docs.find(id).is_some_and(|d| d.opened_from(&path))) {
                return self.open_folder(e, &path).unwrap_or_else(|m| format!("open failed: {m}"));
            }

            self.extract_prompt = Some(crate::extract_prompt::ExtractPrompt::new(target, &dir));
            self.extract_prompt_open = true;
            return format!("{} is a project already: open it, or extract the tile over it?", dir.display());
        }

        self.open_exported(e, target, None, false).unwrap_or_else(|m| format!("open failed: {m}"))
    }

    /// Saves the open tile as an exported tile under `out` (`<out>/terrain/tiles/...` and the tile database), after saving its project.
    ///
    /// # Errors
    ///
    /// No tile is open, the project can't be saved, or the compile fails.
    pub fn save_exported(&mut self, e: &mut Editor, out: &Path) -> Result<String, String> {
        let x = self.export_to(e, out)?;
        Ok(match &x.pack {
            Some(pack) => format!("saved as an exported tile in {} and packed with its walls in {} ({} notes in the log)", x.dir.display(), pack.display(), x.notes),
            None => format!("saved as an exported tile in {} ({} notes in the log)", x.dir.display(), x.notes),
        })
    }

    /// [`Self::save_exported`]'s work: the exported folder, the pack of a tile with procedural walls, the count of notes (logged).
    ///
    /// # Errors
    ///
    /// As `save_exported`.
    pub(crate) fn export_to(&mut self, e: &mut Editor, out: &Path) -> Result<Exported, String> {
        let inputs = self.export_inputs(e)?;
        let (x, game) = inputs.run(out)?;
        self.exported_game = Some(game);
        Ok(x)
    }

    /// What an export needs from the editor, taken on the main thread (the tile saved first if it is modified) so that the compile can run
    /// on another ([`ExportInputs::run`]).
    ///
    /// # Errors
    ///
    /// No tile or game folder, the project can't be saved, or a tile with procedural walls has no game files or schema.
    pub(crate) fn export_inputs(&mut self, e: &mut Editor) -> Result<ExportInputs, String> {
        let game_dir = e.settings_ref().game_dir().ok_or("no game folder (setup <game folder>)")?;
        let game = self.exported_game.clone().filter(|g| g.dir == game_dir);
        let id = document_id(e).ok_or("no tile is open")?;
        let d = e.docs.find_as::<TileDocument>(id).ok_or("no tile is open")?;
        if d.modified() {
            e.docs.save_one(id)?;
        }

        // The scene as the editor holds it (the prefab references with their contents), as TEd's.
        let d = e.docs.find_as::<TileDocument>(id).ok_or("no tile is open")?;
        let project_dir = d.project().dir.clone();
        let text = d.tile().tscene().save();
        let walls = if d.tile().has_walls() { Some((e.files().ok_or("no game files are mounted")?, walls_schema_path(e)?)) } else { None };
        Ok(ExportInputs { game_dir, game, project_dir, text, walls })
    }

    /// The kits a tile of `kind` (`battle`, `campaign`) can be saved for: those of the kits the Tiles panel lists (not the projects) that
    /// have that kind's tile database, in the settings' order.
    pub(crate) fn kits_for(&self, e: &mut Editor, kind: &str) -> Vec<PathBuf> {
        let projects = Self::workspace_dir(e).join("raw_data");
        self.kit_roots(e).into_iter().filter(|r| *r != projects && r.join("terrain/tiles").join(kind).join("_tile_database").is_dir()).collect()
    }

    /// The first of [`Self::kits_for`].
    pub(crate) fn kit_for(&self, e: &mut Editor, kind: &str) -> Option<PathBuf> {
        self.kits_for(e, kind).into_iter().next()
    }

    /// The kind (`battle`, `campaign`) of the open tile, from its folder under terrain/tiles.
    fn open_tile_kind(e: &mut Editor) -> Result<String, String> {
        let d = document(e).ok_or("no tile is open")?;
        let folder = wce_tile_editor::tile_document::kit::tile_folder(&d.project().dir).ok_or("the tile is not under terrain/tiles/")?;
        Ok(folder.split('/').nth(2).unwrap_or("battle").to_owned())
    }

    /// "Save for Assembly Kit" from the menus and buttons: into the only kit that fits, or after asking which of several (`kit_prompt.rs`).
    pub fn save_for_kit_ask(&mut self, e: &mut Editor) -> Result<String, String> {
        let kind = Self::open_tile_kind(e)?;
        let kits = self.kits_for(e, &kind);
        if kits.len() > 1 {
            return Ok(self.ask_kit(crate::kit_prompt::KitSave::Tile, kits));
        }

        self.save_for_kit(e, None)
    }

    /// "Save for Assembly Kit" of a prefab from the buttons: into the only kit there is, or after asking which of several.
    pub fn save_prefab_for_kit_ask(&mut self, e: &mut Editor, key: &str) -> Result<String, String> {
        let projects = Self::workspace_dir(e).join("raw_data");
        let kits = self.kit_roots(e).into_iter().filter(|r| *r != projects).collect::<Vec<_>>();
        if kits.len() > 1 {
            return Ok(self.ask_kit(crate::kit_prompt::KitSave::Prefab(key.to_owned()), kits));
        }

        self.save_prefab_for_kit(e, key, None)
    }

    /// "Save for Assembly Kit": the open tile (saved first when modified) as TEd reads it, into `kit_raw` (a raw_data folder) or the first kit
    /// of its kind.
    ///
    /// # Errors
    ///
    /// No tile, no kit, or the save fails.
    pub fn save_for_kit(&mut self, e: &mut Editor, kit_raw: Option<PathBuf>) -> Result<String, String> {
        let id = document_id(e).ok_or("no tile is open")?;
        if e.docs.find(id).is_some_and(|d| d.modified()) {
            e.docs.save_one(id)?;
        }

        let kind = Self::open_tile_kind(e)?;
        let kit = match kit_raw {
            Some(k) => k,
            None => self.kit_for(e, &kind).ok_or("no Assembly Kit (set it in the game's settings)")?,
        };

        // The procedural walls are written as their pieces, which go into the game's data (and a tile without walls takes its old ones out).
        let data = e.settings_ref().game_dir().map(|g| g.join("data"));
        let has_walls = e.docs.find_as::<TileDocument>(id).is_some_and(|d| d.tile().has_walls());
        let pack_there = data.as_ref().is_some_and(|d| d.join(wce_walls::pack::PROC_PACK).is_file());
        let files = e.files();
        let schema = if has_walls || pack_there { Some(walls_schema(e)) } else { None };
        let sources = match (&files, schema, &data) {
            (Some(f), Some(Ok(schema)), Some(data)) => Some((f.clone(), schema, data.clone())),
            (_, Some(Err(err)), _) if has_walls => return Err(err),
            _ if has_walls => return Err("the procedural walls need the game's files and its folder (set them in the game's settings)".to_owned()),
            _ => None,
        };

        let d = e.docs.find_as_mut::<TileDocument>(id).ok_or("no tile is open")?;
        let walls = sources.as_ref().map(|(f, schema, data)| wce_tile_editor::tile_document::kit::WallSources { files: f.as_ref(), schema, data });
        d.save_for_kit(&kit, walls)
    }

    /// "Import from Assembly Kit": opens the kit's copy of the open project (its next save replaces the project); an extracted tile is
    /// extracted again from the game.
    ///
    /// # Errors
    ///
    /// No project is open, it has unsaved changes, or the kit has no such tile.
    pub fn import_from_kit(&mut self, e: &mut Editor) -> Result<String, String> {
        let id = document_id(e).ok_or("no tile is open")?;
        let d = e.docs.find_as::<TileDocument>(id).ok_or("no tile is open")?;
        if d.modified() {
            return Err("the project has unsaved changes: save or undo them first".to_owned());
        }

        let kit_folder = wce_tile_editor::tile_document::kit::tile_folder(&d.project().dir).ok_or("the tile is not under terrain/tiles/")?;
        if wce_tile::project_files::is_extracted_dir(&d.project().dir) {
            return self.open_exported(e, &kit_folder, None, true);
        }

        let kind = kit_folder.split('/').nth(2).unwrap_or("battle").to_owned();
        let kits = self.kits_for(e, &kind);
        if kits.is_empty() {
            return Err("no Assembly Kit (set it in the game's settings)".to_owned());
        }

        // The first kit that has the tile.
        let dir = kits.iter().map(|k| k.join(&kit_folder)).find(|d| wce_tile::tile_project::has_tile(d)).ok_or_else(|| format!("no Assembly Kit has {kit_folder}"))?;

        // TEd has the walls' pieces, the project the walls: the pieces go and the walls come back (edits of the pieces in TEd are lost).
        let walls = e.docs.find_as::<TileDocument>(id).map(TileDocument::wall_texts).unwrap_or_default();
        e.close_document(id);
        self.open_kit_copy = true;
        let mut msg = self.open_folder(e, &dir.display().to_string())?;
        if let Some(d) = document(e) {
            match d.restore_walls(&walls) {
                Ok(0) => {}
                Ok(n) => {
                    log::info(format!("Import from Assembly Kit: the layer '{}' replaced by the project's {n} procedural wall(s); edits of their pieces in TEd are not kept", wce_walls::export::KIT_LAYER));
                    msg += &format!("; {n} procedural wall(s) put back in place of their pieces");
                }
                Err(err) => log::warn(format!("Import from Assembly Kit: the procedural walls could not be put back: {err}")),
            }
        }

        Ok(format!("{msg}  (the Assembly Kit's copy: saving it replaces the project)"))
    }

    /// Opens a tile folder (its tile.wce, else its tile.ted) or either file.
    ///
    /// # Errors
    ///
    /// The tile can't be opened.
    pub fn open_folder(&mut self, e: &mut Editor, dir: &str) -> Result<String, String> {
        use wce_tile::project_files::{TED_FILE, WCE_FILE};

        let tile = Path::new(dir);
        let path = if tile.is_dir() { tile.join(if tile.join(WCE_FILE).exists() { WCE_FILE } else { TED_FILE }) } else { tile.to_path_buf() };
        let msg = e.open_path_from(self, &path.display().to_string(), false, true)?;

        // A kit's tile with a project opened as the project (`open`): say so.
        let opened = document(e).map(|d| d.project().dir.clone());
        Ok(match opened {
            Some(d) if !path.starts_with(&d) && !msg.starts_with("already open") => format!("opened the project {} (tile.wce) of {}", d.display(), path.display()),
            _ => msg,
        })
    }

    fn handle_size(e: &Editor) -> f32 {
        let c = e.viewport.camera();
        (length(c.eye - c.target) * 0.012).max(0.8)
    }

    fn tool_overlay(&self, e: &Editor, d: &TileDocument) -> Overlay {
        let mut o = Overlay { depth_test: false, pickable: false, ..Default::default() };
        let t = d.tile();
        let gold = overlay_colour(1.0, 0.85, 0.2, 1.0);
        let veg = self.placing == Some(Placing::Draft) && self.draft.as_ref().is_some_and(|d| d.kind == "VEGETATION_INSTANCE") && self.veg.is_some_and(|b| b.on);
        let radius = if veg { self.veg.map_or(1.0, |b| b.radius) } else { self.brush.radius_m };
        if (self.mode != TileMode::Select || veg) && self.terrain_hit && e.viewport.hovered {
            let n = 64;
            let mut prev = Float3::default();
            for i in 0..=n {
                let a = i as f32 * std::f32::consts::TAU / n as f32;
                let mut p = Float3::new(self.cursor.x + a.cos() * radius, 0.0, self.cursor.z + a.sin() * radius);
                p.y = t.terrain_height(p.x, p.z) + 0.4;
                if i != 0 {
                    o.line(prev, p, gold);
                }

                prev = p;
            }
        }

        // The trees an erasing stroke takes away: red crosses.
        let red = overlay_colour(1.0, 0.25, 0.2, 1.0);
        for p in self.erased_points(d) {
            o.line(Float3::new(p.x - 1.0, p.y + 0.5, p.z - 1.0), Float3::new(p.x + 1.0, p.y + 0.5, p.z + 1.0), red);
            o.line(Float3::new(p.x - 1.0, p.y + 0.5, p.z + 1.0), Float3::new(p.x + 1.0, p.y + 0.5, p.z - 1.0), red);
        }

        if self.drawing {
            let r = Self::handle_size(e);
            let hot = overlay_colour(1.0, 0.9, 0.1, 1.0);
            for (i, p) in self.draw_pts.iter().enumerate() {
                o.line(Float3::new(p.x - r, p.y + 0.4, p.z), Float3::new(p.x + r, p.y + 0.4, p.z), hot);
                o.line(Float3::new(p.x, p.y + 0.4, p.z - r), Float3::new(p.x, p.y + 0.4, p.z + r), hot);
                o.line(*p, Float3::new(p.x, p.y + r * 1.5, p.z), hot);
                if i != 0 {
                    let q = self.draw_pts[i - 1];
                    o.line(Float3::new(q.x, q.y + 0.4, q.z), Float3::new(p.x, p.y + 0.4, p.z), hot);
                }
            }
        }

        if t.display().show_border {
            let (ex, ez) = (t.extent_x() as f32, t.extent_z() as f32);
            let white = overlay_colour(0.9, 0.9, 0.9, 1.0);
            let seg = 32;
            let mut edge = |x0: f32, z0: f32, x1: f32, z1: f32| {
                let mut prev = Float3::default();
                for i in 0..=seg {
                    let s = i as f32 / seg as f32;
                    let mut p = Float3::new(x0 + (x1 - x0) * s, 0.0, z0 + (z1 - z0) * s);
                    p.y = t.terrain_height(p.x, p.z) + 0.3;
                    if i != 0 {
                        o.line(prev, p, white);
                    }

                    prev = p;
                }
            };

            edge(0.0, 0.0, ex, 0.0);
            edge(ex, 0.0, ex, ez);
            edge(ex, ez, 0.0, ez);
            edge(0.0, ez, 0.0, 0.0);
        }

        o
    }

    // ---- mouse

    fn on_hover(&mut self, e: &mut Editor, ev: &ViewportEvent) {
        self.hover_surface(e, ev);
        self.terrain_hit = false;
        let Some((o, dir)) = e.viewport.ray(ev.x, ev.y) else { return };
        let Some(d) = document(e) else { return };
        if let Some(hit) = d.tile().pick_terrain(o, dir) {
            self.terrain_hit = true;
            self.cursor = hit;
            let snapped = self.snap_point(d.tile(), hit, self.grab_elem);
            self.snapped_cursor = Float3::new(snapped.x, d.tile().terrain_height(snapped.x, snapped.z), snapped.z);
        }

        if let (true, true, Some(g)) = (self.grabbing, self.terrain_hit, self.grab_elem) {
            let (gx, gz) = (self.snapped_cursor.x, self.snapped_cursor.z);

            // Entity: Above: on what is under the mouse; else at the height it had above the terrain. A freeform one moved along the
            // normal (PageUp / PageDown) is offset along the terrain's normal there.
            let gy = match (self.modes.unwrap_or_default().0, self.surface_hit) {
                (crate::settings::SurfaceMode::Entity, Some(h)) => h.point.y,
                _ => d.tile().terrain_height(gx, gz) + self.grab_y_offset as f32,
            };

            let n = d.tile().terrain_normal(gx, gz);
            let k = if d.tile().placement(g) == wce_tile_editor::tile_document::Placement::Freeform { self.grab_normal_offset } else { 0.0 };
            let at = Float3::new(gx + n.x as f32 * k, gy + n.y as f32 * k, gz + n.z as f32 * k);

            // Snap to points: a fort piece joins the nearest end of another (it takes that one's rotation), within the tolerance (4 m at 0).
            let piece = if self.snap_vertices { d.tile().snap_piece(g, Float3::new(self.cursor.x, gy, self.cursor.z), self.piece_reach()) } else { None };
            match piece {
                Some(w) => {
                    d.set_world_transform(g, &w, self.grab_gesture);
                }

                None => {
                    d.set_world_metres(g, at, self.grab_gesture);
                }
            }

            // An item that follows its surface is tilted to the one it is moved onto.
            if d.tile().tscene().doc().get(g, wce_tile::project_files::ROTATION_ATTR).is_some_and(|v| v == "surface") {
                let n = match (self.modes.unwrap_or_default().0, self.surface_hit) {
                    (crate::settings::SurfaceMode::Entity, Some(h)) => wce_foundation::math::Double3::new(f64::from(h.normal.x), f64::from(h.normal.y), f64::from(h.normal.z)),
                    _ => d.tile().terrain_normal(gx, gz),
                };

                d.tilt_to_surface(g, n, self.grab_gesture);
            }
        }
    }

    // ---- grab, polygons

    pub fn begin_grab(&mut self, e: &mut Editor) -> bool {
        let el = primary(e);
        let Some(d) = document(e) else { return false };
        let Some(el) = el.filter(|_| !self.grabbing) else { return false };
        let p = d.tile().world_position(el);
        self.grab_elem = Some(el);
        self.grab_y_offset = f64::from(p.y - d.tile().terrain_height(p.x, p.z));
        self.grab_normal_offset = 0.0;
        self.grab_history = d.history().position();
        self.grab_gesture = d.next_gesture();
        self.grabbing = true;
        self.status = "grab: move the mouse over the ground, click to confirm, Esc / right click to cancel; PageUp / PageDown: along the ground's normal (freeform)".to_owned();
        true
    }

    /// Copies the selection into the tool's clipboard; `cut` deletes it after (one undo entry, as Delete).
    pub(crate) fn copy_selection(&mut self, e: &mut Editor, cut: bool) {
        let all = selected(e);
        let Some(d) = document(e) else { return };
        let items = d.copy_items(&all);
        if items.is_empty() {
            self.status = "nothing to copy".to_owned();
            return;
        }

        let n = items.len();
        self.clipboard = items;
        if cut {
            d.remove_items(&all);
            select_elem(e, None, false);
        }

        self.status = format!("{} {n} item(s)", if cut { "cut" } else { "copied" });
    }

    /// Pastes the clipboard into the working layer: at the mouse on the ground (`at_mouse`; the first item where the mouse is, the rest
    /// around it as they were), else where they were. The pasted items are selected.
    pub(crate) fn paste(&mut self, e: &mut Editor, at_mouse: bool) -> bool {
        let Some(first) = self.clipboard.first().map(|c| c.at) else {
            self.status = "nothing to paste".to_owned();
            return false;
        };

        let offset = if at_mouse && self.terrain_hit { self.snapped_cursor - first } else { Float3::default() };
        let Some(d) = document(e) else { return false };
        let Some(into) = self.new_items_layer(d) else { return false };
        let items = self.clipboard.clone();
        match d.paste(&items, into, offset) {
            Ok(new) => {
                select_elem(e, None, false);
                for g in &new {
                    select_elem(e, Some(*g), true);
                }

                self.status = format!("pasted {} item(s)", new.len());
                true
            }

            Err(err) => {
                self.status = err;
                false
            }
        }
    }

    /// How near a fort piece's end must come to another's to join it: the snap tolerance, 4 m when it is 0.
    pub(crate) fn piece_reach(&self) -> f32 {
        if self.snap_tolerance > 0.0 { self.snap_tolerance } else { 4.0 }
    }

    /// A ground point with the snapping applied (x, z): onto the nearest instance or polygon point (snap to vertices; not `skip`'s own),
    /// else to the grid (each axis, within the tolerance when there is one).
    pub(crate) fn snap_point(&self, t: &wce_tile_editor::tile_document::Tile, p: Float3, skip: Option<usize>) -> Float3 {
        if self.snap_vertices {
            let reach = if self.snap_tolerance > 0.0 { self.snap_tolerance } else { 1.5 };
            let mut best = (reach * reach, None);
            let mut try_point = |q: Float3| {
                let d2 = (q.x - p.x) * (q.x - p.x) + (q.z - p.z) * (q.z - p.z);
                if d2 < best.0 {
                    best = (d2, Some(q));
                }
            };

            for &e in t.instances() {
                if Some(e) == skip || !t.shown(e) {
                    continue;
                }

                if t.is_polygon(e) {
                    t.polygon_world_points(e).into_iter().for_each(&mut try_point);
                } else {
                    try_point(t.world_position(e));
                }
            }

            if let Some(q) = best.1 {
                return Float3::new(q.x, p.y, q.z);
            }
        }

        if self.snap_grid && self.grid_m > 0.01 {
            let g = self.grid_m;
            let axis = |v: f32| {
                let s = (v / g).round() * g;
                if self.snap_tolerance > 0.0 && (s - v).abs() > self.snap_tolerance { v } else { s }
            };

            return Float3::new(axis(p.x), p.y, axis(p.z));
        }

        p
    }

    pub fn end_grab(&mut self, e: &mut Editor, commit: bool) {
        if !self.grabbing {
            return;
        }

        self.grabbing = false;
        let Some(d) = document(e) else { return };
        let h = d.history_mut();
        h.break_merge();
        if !commit {
            h.jump_to(self.grab_history);
            h.drop_redo();
        }

        self.grab_elem = None;
    }

    /// Generate SiegeAI (docs/plans/siege_ai.md): the layout into its own layer, the old one out of the export; the warnings in the log.
    pub fn generate_siege_ai(&mut self, e: &mut Editor) {
        let Some(d) = document(e) else { return };
        self.status = match d.generate_siege_ai() {
            Ok(l) => {
                for w in &l.warnings {
                    log::warn(format!("Generate SiegeAI: {w}"));
                }

                format!("SiegeAI generated: {}{}", l.summary(), if l.warnings.is_empty() { "" } else { " (the warnings are in the log)" })
            }
            Err(err) => {
                log::warn(format!("Generate SiegeAI: {err}"));
                format!("Generate SiegeAI failed: {err}")
            }
        };
    }

    fn handle_keys(&mut self, e: &mut Editor) {
        let io = gui::io();
        if document_id(e).is_none() || io.WantTextInput {
            return;
        }

        let sel = primary(e);
        if gui::is_key_pressed(gui::KEY_ESCAPE, false) && self.box_select.armed {
            self.box_select = Default::default();
            self.status = "box select cancelled".to_owned();
        } else if gui::is_key_pressed(gui::KEY_ESCAPE, false) {
            if self.grabbing {
                self.end_grab(e, false);
            } else if self.drawing || self.placing.is_some() {
                self.stop_making();
            } else if self.mode != TileMode::Select {
                self.mode = TileMode::Select;
            }
        }

        // While grabbing: PageUp / PageDown move a freeform item along the ground's normal (a quarter metre; Shift: a metre).
        if self.grabbing {
            let step = if io.KeyShift { 1.0 } else { 0.25 };
            let dk = if gui::is_key_pressed(gui::KEY_PAGE_UP, true) { step } else if gui::is_key_pressed(gui::KEY_PAGE_DOWN, true) { -step } else { 0.0 };
            if dk != 0.0 {
                self.grab_normal_offset += dk;
                self.status = format!("grab: {:+.2} m along the ground's normal", self.grab_normal_offset);
            }
        }

        if self.grabbing && gui::is_mouse_clicked(1) {
            self.right_used = true;
            self.end_grab(e, false);
        }

        // While placing: R / Shift+R turn it. Shift+A over the viewport: the quick-add popup.
        if !io.KeyCtrl && self.placing.is_some() && gui::is_key_pressed(gui::KEY_R, false) {
            self.turn_placing(if io.KeyShift { -self.angle_step } else { self.angle_step });
        }

        if io.KeyShift && !io.KeyCtrl && e.viewport.hovered && !gui::is_mouse_down(1) && gui::is_key_pressed(gui::KEY_A, false) {
            self.request_quick_add();
        }

        if io.KeyCtrl {
            let grabbing = self.grabbing;
            if gui::is_key_pressed(gui::KEY_D, false)
                && let (Some(s), false) = (sel, grabbing)
            {
                let d = document(e).expect("a tile");
                self.status = match d.duplicate(s) {
                    Ok(_) => "duplicated".to_owned(),
                    Err(err) => err,
                };
            }

            if gui::is_key_pressed(gui::KEY_R, false)
                && let (Some(s), false) = (sel, grabbing)
            {
                let step = if io.KeyShift { -self.angle_step } else { self.angle_step };
                document(e).expect("a tile").rotate(s, step);
            }

            let all = selected(e);
            if gui::is_key_pressed(gui::KEY_G, false) && !all.is_empty() && !grabbing {
                self.status = self.group_selection(e).unwrap_or_else(|err| err);
            }

            if gui::is_key_pressed(gui::KEY_U, false) && !all.is_empty() && !grabbing {
                self.status = self.ungroup_selection(e).unwrap_or_else(|err| err);
            }

            // Copy / cut / paste (at the mouse; Shift: where they were).
            if !grabbing && gui::is_key_pressed(gui::KEY_C, false) {
                self.copy_selection(e, false);
            }

            if !grabbing && gui::is_key_pressed(gui::KEY_X, false) {
                self.copy_selection(e, true);
            }

            if !grabbing && gui::is_key_pressed(gui::KEY_V, false) {
                self.paste(e, !io.KeyShift);
            }

            return;
        }

        if gui::is_key_pressed(gui::KEY_G, false) && sel.is_some() {
            self.begin_grab(e);
        }

        // B: box select (the next drag draws the rectangle).
        if gui::is_key_pressed(gui::KEY_B, false) && !self.grabbing && self.mode == TileMode::Select && !self.drawing && self.placing.is_none() {
            self.box_select = crate::selection_tools::BoxSelect { armed: true, ..Default::default() };
            self.status = "box select: drag a rectangle (Shift adds, Ctrl takes away; Esc cancels)".to_owned();
        }

        if gui::is_key_pressed(gui::KEY_DELETE, false) && !self.grabbing {
            let is_polygon = sel.is_some_and(|s| document(e).is_some_and(|d| d.tile().is_polygon(s)));
            if let (Some(s), Some(pt), true) = (sel, self.poly_point, is_polygon) {
                let d = document(e).expect("a tile");
                self.status = match d.polygon_point(s, "remove", pt, Float3::default(), 0) {
                    Ok(()) => "point removed".to_owned(),
                    Err(err) => err,
                };
                self.poly_point = None;
            } else if !selected(e).is_empty() {
                // Layers and instances together, one undo entry (asks before a big layer goes).
                self.request_delete(e);
            }
        }

        if self.drawing && (gui::is_key_pressed(gui::KEY_ENTER, false) || gui::is_key_pressed(gui::KEY_KEYPAD_ENTER, false)) {
            self.finish_drawing(e);
        }
    }

    // ---- panels

    fn draw_tile_panel(&mut self, e: &mut Editor) {
        gui::set_next_window_size(gui::v2(460.0, 640.0), gui::COND_FIRST_USE_EVER);
        if !gui::begin("Tile", None, 0) {
            gui::end();
            return;
        }

        self.right_dock = Some(gui::window_dock_id());

        let Some(id) = document_id(e) else {
            gui::text_wrapped("No tile open. File > Open file... (a tile.ted), Tile > Open tile..., or --tile <folder>.");
            gui::end();
            return;
        };

        let mut action = None;
        {
            let d = e.docs.find_as::<TileDocument>(id).expect("a tile");
            let p = d.project();
            let file_name = |p: &Path| p.file_name().map(|f| f.to_string_lossy().into_owned()).unwrap_or_default();
            gui::text(&file_name(&p.dir));
            gui::text_disabled(&p.dir.parent().map(file_name).unwrap_or_default());
            if !p.warning.is_empty() {
                gui::text_colored(theme::warning(), &p.warning);
            }

            // Where the tile is: the Assembly Kit's copy (saving makes its project) or a project (tile.wce).
            if d.is_project() {
                gui::text_disabled(&format!("{}  project (tile.wce)", icons::FOLDER));
                gui::same_line();
                if gui::small_button("Save for Assembly Kit") {
                    action = Some(true);
                }
            } else {
                gui::text_colored(theme::warning(), &format!("{}  Assembly Kit copy", icons::PACKAGE));
                gui::same_line();
                if gui::small_button("Save as project (Ctrl+S)") {
                    action = Some(false);
                }

                if gui::is_item_hovered() {
                    gui::set_tooltip(&format!("Saves it as tile.wce in {}", d.project_folder().map(|f| f.display().to_string()).unwrap_or_default()));
                }
            }
        }

        match action {
            Some(true) => {
                let msg = self.save_for_kit_ask(e).unwrap_or_else(|m| format!("save for the Assembly Kit failed: {m}"));
                e.status(&msg, false);
            }

            Some(false) => {
                let msg = e.docs.save_one(id).unwrap_or_else(|m| format!("save failed: {m}"));
                e.status(&msg, false);
            }

            None => {}
        }

        if gui::collapsing_header("Info", gui::TREE_DEFAULT_OPEN) {
            let d = e.docs.find_as_mut::<TileDocument>(id).expect("a tile");
            let t = d.tile();
            let p = d.project();
            if self.terrain_hit {
                gui::text(&format!(
                    "cursor: {:.1}, {:.1}  height {:.2} m", self.cursor.x, self.cursor.z, t.terrain_height(self.cursor.x, self.cursor.z),
                ));
            } else {
                gui::text_disabled("cursor: -");
            }

            match &p.height {
                Some(h) => gui::text(&format!(
                    "height map {}x{}, {:.2}..{:.2} m (min {:.3} scale {:.3}){}", h.width(), h.height(), h.min_height(), h.min_height() + h.scale(), p.info.min_height,
                    p.info.scale, if p.info.found { "" } else { "  [no tile database entry: heights stretched over the tile]" },
                )),
                None => gui::text_disabled("no height_map_0.png"),
            }

            gui::text(&format!("tile {} x {} cells, {:.0} x {:.0} m", p.info.cells_x, p.info.cells_y, t.extent_x(), t.extent_z()));
            gui::text(&format!(
                "tile.ted v{}, {} instances{}", t.tscene().project_version(), t.instances().len(), if d.modified() { "  (modified)" } else { "" },
            ));
            let h = d.history();
            gui::text(&format!("undo {} / redo {}", h.position(), h.size() - h.position()));
            if !t.model_fallbacks().is_empty() {
                gui::text_colored(theme::warning(), &format!("{} building keys have no model (boxes), see the log", t.model_fallbacks().len()));
            }
        }

        self.draw_entry_section(e, id);
        self.context.draw(e, id);
        gui::separator();
        self.draw_placement_section(e, id);
        gui::separator();
        self.draw_prefabs_section(e, id);
        gui::separator();
        self.draw_ground_section(e, id);
        gui::separator();
        self.draw_links_section(e, id);
        gui::separator();
        self.draw_problems_section(e, id);
        gui::separator();
        self.draw_polygon_panel(e, id);
        gui::separator();
        if !self.status.is_empty() {
            gui::separator();
            gui::text_wrapped(&self.status);
        }

        gui::end();
    }

    /// Ground: the terrain colour is the tile's blend map mixed from the textures of a climate. The climate is not stored in the tile (a
    /// map's climate map says it), so it is a setting here; a wrong guess looks different, not broken.
    fn draw_ground_section(&mut self, e: &mut Editor, id: DocumentId) {
        if !gui::collapsing_header("Ground", gui::TREE_DEFAULT_OPEN) {
            return;
        }

        let shader_failures = e.renderer.as_ref().map(|r| r.shader_failures().to_vec()).unwrap_or_default();
        let d = e.docs.find_as_mut::<TileDocument>(id).expect("a tile");
        let mut look = d.tile().terrain_look().clone();
        let mut changed = gui::checkbox("blended ground (blend map + climate textures)", &mut look.blended);
        let climates = d.tile().climates().climates.iter().map(|c| c.name.clone()).collect::<Vec<_>>();
        if look.blended && !climates.is_empty() {
            let mut ci = climates.iter().position(|c| *c == look.climate).unwrap_or(0) as i32;
            gui::set_next_item_width(200.0);
            if gui::combo("climate##ground", &mut ci, &climates.iter().map(String::as_str).collect::<Vec<_>>()) {
                look.climate = climates[ci as usize].clone();
                changed = true;
            }

            // The game's four battle seasons; a texture without the season's variant falls back to summer (fn 0x1180cb20).
            let seasons = ["sp", "su", "au", "wi"];
            let mut si = match look.season.as_str() {
                "sp" => 0,
                "au" => 2,
                "wi" => 3,
                _ => 1,
            };

            gui::set_next_item_width(80.0);
            if gui::combo("season##ground", &mut si, &["spring", "summer", "autumn", "winter"]) {
                look.season = seasons[si as usize].to_owned();
                changed = true;
            }
        }

        if look.blended {
            let views = [TerrainView::Colour, TerrainView::BlendGroups, TerrainView::GroundTypes, TerrainView::Protection, TerrainView::WaterDepth];
            let mut vi = views.iter().position(|&v| v == look.view).unwrap_or(0) as i32;
            gui::set_next_item_width(160.0);
            if gui::combo("view##ground", &mut vi, &["colour", "blend groups", "ground types", "protection", "water depth"]) {
                look.view = views[vi as usize];
                changed = true;
            }

            if gui::is_item_hovered() && look.view == TerrainView::WaterDepth {
                gui::set_tooltip(WATER_VIEW_TIP);
            }
        }

        let water_view = look.view == TerrainView::WaterDepth;
        if changed {
            d.tile_mut().set_terrain_look(&mut e.scene, look);
        }

        // The water depth view: the water ground types painted from the depth.
        if water_view {
            gui::set_next_item_width(70.0);
            gui::drag_float("shallow from (m)##water", &mut self.water_shallow_m, 0.05, 0.0, 20.0, "%.2f");
            if gui::is_item_hovered() {
                gui::set_tooltip("Water shallower than this is left unpainted (the water at a river's banks, which lies under the ground or barely over it).");
            }

            gui::same_line();
            gui::set_next_item_width(70.0);
            gui::drag_float("deep from (m)##water", &mut self.water_deep_m, 0.05, 0.0, 50.0, "%.2f");
            if gui::is_item_hovered() {
                gui::set_tooltip("Water at least this deep is painted deep water (units cannot cross it); shallower water is shallow water (units wade).");
            }

            self.water_deep_m = self.water_deep_m.max(self.water_shallow_m);
            if gui::button("Paint water ground types") {
                let d = e.docs.find_as_mut::<TileDocument>(id).expect("a tile");
                self.status = match d.paint_water_ground_types(self.water_shallow_m, self.water_deep_m) {
                    Ok(n) => format!("{n} ground type pixel(s) painted from the water depth"),
                    Err(why) => why,
                };
            }

            if gui::is_item_hovered() {
                gui::set_tooltip(PAINT_WATER_TIP);
            }
        }

        let d = e.docs.find_as_mut::<TileDocument>(id).expect("a tile");
        gui::text_wrapped(d.tile().terrain_note());
        let mut water = d.tile().display().show_water;
        if gui::checkbox("water (rivers)", &mut water) {
            d.tile_mut().display_mut().show_water = water;
            d.tile_mut().display_changed();
        }

        gui::same_line();
        let mut sea = d.tile().display().show_sea;
        if gui::checkbox("sea at 0 m", &mut sea) {
            self.set_show_sea(e, id, sea);
        }

        // The battle's wind picks the sea's waves and whitecaps.
        if let Some(r) = e.renderer() {
            let states = r.sea_states();
            if !states.is_empty() {
                let mut k = states.iter().position(|s| *s == r.settings.sea_state).unwrap_or(0) as i32;
                gui::set_next_item_width(170.0);
                if gui::combo("sea state (battle wind)", &mut k, &states.iter().map(String::as_str).collect::<Vec<_>>()) {
                    let key = states[k as usize].clone();
                    set_sea_state(e, &key);
                }
            }
        }

        let d = e.docs.find_as_mut::<TileDocument>(id).expect("a tile");

        gui::text_disabled(d.tile().water_note());

        // BOB's procedural trees: one list per climate key, plus default; gen:<list> generates them from the seeds.
        let mut lists = vec!["none".to_owned()];
        lists.extend(d.tile().tree_lists().iter().cloned());
        let shown = d.tile().display().tree_list.clone();
        let generated = format!("{GENERATED_PREFIX}{shown}");
        let mut li = lists.iter().position(|l| *l == shown).or_else(|| lists.iter().position(|l| *l == generated)).unwrap_or(0) as i32;
        let items = lists.iter().map(String::as_str).collect::<Vec<_>>();
        gui::set_next_item_width(160.0);
        if gui::combo("procedural trees", &mut li, &items) {
            d.tile_mut().display_mut().tree_list = if li == 0 { String::new() } else { lists[li as usize].clone() };
        }

        let mut grass = d.tile().display().show_grass;
        if gui::checkbox("grass", &mut grass) {
            d.tile_mut().display_mut().show_grass = grass;
        }

        gui::same_line();
        let mut generated = d.tile().display().generate_grass;
        if gui::checkbox("generated##grass", &mut generated) {
            d.tile_mut().display_mut().generate_grass = generated;
        }

        if generated {
            gui::same_line();
            if gui::button("regenerate##grass") {
                d.tile_mut().reload_grass();
            }
        }

        if lists[li as usize].starts_with(GENERATED_PREFIX) {
            gui::same_line();
            if gui::button("regenerate") {
                d.tile_mut().reload_tree_list();
            }
        }
        for f in &shader_failures {
            gui::text_colored(theme::error(), &format!("game shader not loaded: {f}"));
        }
    }

    /// Tile links: the entry's link targets and rules, edited (`links_editor`), and a viewport view of the cells they name. They are part
    /// of the database entry, saved to TILES/<tile>.xml (tile_database.bin is compiled from the entries at export).
    fn draw_links_section(&mut self, e: &mut Editor, id: DocumentId) {
        if !gui::collapsing_header("Tile links", 0) {
            return;
        }

        let fly = e.viewport.is_fly();
        let d = e.docs.find_as_mut::<TileDocument>(id).expect("a tile");
        if !d.project().has_entry() {
            gui::text_disabled("no tile database entry: no links");
            return;
        }

        let mut show = d.tile().display().show_links;
        if gui::checkbox("show on the ground", &mut show) {
            d.tile_mut().display_mut().show_links = show;
            d.tile_mut().display_changed();
        }

        // The editor; a click on a link looks at its cell. Then the tiles that can follow.
        let look = self.draw_link_editor(e, id);
        self.draw_link_candidates(e, id);
        let d = e.docs.find_as_mut::<TileDocument>(id).expect("a tile");
        if let (Some((x, y)), false) = (look, fly) {
            let (x0, z0, x1, z1) = d.tile().cell_rect(x, y);
            let (cx, cz) = ((x0 + x1) * 0.5, (z0 + z1) * 0.5);
            let t = Float3::new(cx, d.tile().terrain_height(cx, cz), cz);
            e.viewport.orbit_mut().set_orbit(0.8, 0.9, 350.0, t);
        }
    }

    /// Problems: what the export or the game will not take. The manual trees outside every vegetation exclusion polygon (BOB leaves them
    /// out), counted again after each edit; a click selects one and frames it.
    fn draw_problems_section(&mut self, e: &mut Editor, id: DocumentId) {
        let Some(d) = e.docs.find_as::<TileDocument>(id) else { return };
        let stamp = (id, d.history().version(), d.tile().structure_version());
        if self.problems.as_ref().is_none_or(|(s, _)| *s != stamp) {
            self.problems = Some((stamp, d.trees_outside_exclusions()));
            self.river_problems = Some((stamp, d.river_link_problems()));
            self.link_problems = Some((stamp, d.link_problems()));
        }

        let trees = self.problems.as_ref().map(|(_, t)| t.clone()).unwrap_or_default();
        let rivers = self.river_problems.as_ref().map(|(_, r)| r.clone()).unwrap_or_default();
        let links = self.link_problems.as_ref().map(|(_, r)| r.clone()).unwrap_or_default();
        let n = trees.len() + rivers.len() + links.len();
        let title = if n == 0 { "Problems (none)###problems".to_owned() } else { format!("{}  Problems ({n})###problems", icons::TRIANGLE_ALERT) };
        if !gui::collapsing_header(&title, 0) {
            return;
        }

        if n == 0 {
            gui::text_disabled("Nothing the game would leave out, the rivers meet their link targets, and the links are as the kit makes them.");
            return;
        }

        // The links: a click picks the link in the Tile links section.
        if !links.is_empty() {
            gui::text_wrapped(&format!("{} link problem(s): the tile's links (Tile links) as the game would not use them, or as the kit never makes them.", links.len()));
            for (i, p) in links.iter().enumerate() {
                if gui::selectable(&format!("{}  {}##link{i}", icons::ROUTE, p.message), false, 0) {
                    self.link_pick = Some((p.target, p.index));
                    self.status = "picked in the Tile panel's Tile links".to_owned();
                }
            }

            gui::separator();
        }

        let mut pick = None;
        if !rivers.is_empty() {
            gui::text_wrapped(&format!("{} river link problem(s): the rivers must end just past the edge, level with the centre of the cells the tile offers as river links, for the neighbours' rivers to meet them.", rivers.len()));
            for (i, (el, msg)) in rivers.iter().enumerate() {
                if gui::selectable(&format!("{}  {msg}##river{i}", icons::WAVES), false, 0) {
                    pick = *el;
                }
            }

            if gui::button("Snap river ends to link targets") {
                let d = e.docs.find_as_mut::<TileDocument>(id).expect("a tile");
                self.status = match d.snap_river_ends() {
                    Ok(0) => "no river end at a target cell needed moving".to_owned(),
                    Ok(k) => format!("{k} river end(s) snapped to their link targets"),
                    Err(why) => why,
                };

                return;
            }

            if gui::is_item_hovered() {
                gui::set_tooltip(&RIVER_SNAP_TIP);
            }

            if trees.is_empty() {
                if let Some(el) = pick {
                    e.selection.set(SourceRef { doc: id, item: wce_tile_editor::tile_document::item_of(el) });
                    let _ = wce_editor_core::command_registry::CommandRegistry::run(e, "focus");
                }

                return;
            }

            gui::separator();
        }

        let Some(d) = e.docs.find_as::<TileDocument>(id) else { return };

        gui::text_wrapped(&format!("{} manual tree(s) are outside every vegetation exclusion polygon: the game leaves them out. Put them inside one (or draw one around them).", trees.len()));
        let sc = d.tile().tscene();
        for &el in trees.iter().take(200) {
            let p = d.tile().world_position(el);
            if gui::selectable(&format!("{}  {}  ({:.0}, {:.0})##problem{el}", icons::TREE_PINE, sc.doc().get(el, "key").unwrap_or_default(), p.x, p.z), false, 0) {
                pick = Some(el);
            }
        }

        if trees.len() > 200 {
            gui::text_disabled(&format!("... {} more", trees.len() - 200));
        }

        if let Some(el) = pick {
            e.selection.set(SourceRef { doc: id, item: wce_tile_editor::tile_document::item_of(el) });
            let _ = wce_editor_core::command_registry::CommandRegistry::run(e, "focus");
        }
    }

    /// Placement (docs/formats/tile_ted.md): landscape instances stand on the terrain, freeform ones keep any height. The counts, and the report
    /// of the landscape instances that are off the terrain (the selection's placement is in the right-click and Edit menus).
    fn draw_placement_section(&mut self, e: &mut Editor, id: DocumentId) {
        if !gui::collapsing_header("Placement", 0) {
            return;
        }

        let d = e.docs.find_as_mut::<TileDocument>(id).expect("a tile");
        let c = d.tile().placement_counts();
        gui::text(&format!("{} landscape, {} freeform, {} without a mode", c.landscape, c.freeform, c.other));
        gui::same_line();
        let v = d.tile_mut().display_mut();
        gui::checkbox("tint freeform", &mut v.tint_freeform);
        if gui::is_item_hovered() {
            gui::set_tooltip(FREEFORM_TINT_TIP);
        }

        gui::set_next_item_width(90.0);
        gui::drag_float("tolerance (m)", &mut self.off_tol, 0.05, 0.001, 50.0, "%.3f");
        gui::same_line();
        if gui::button("Report off-terrain") {
            self.off_list = d.tile().off_terrain(self.off_tol, None);
            self.off_doc = id;
            self.off_doc_version = Some(d.history().version());
        }

        let Some(version) = self.off_doc_version.filter(|_| self.off_doc == id) else { return };
        if version != d.history().version() {
            gui::text_disabled("(the tile changed since: press Report again)");
        }

        gui::text(&format!(
            "{} landscape instance(s) more than {} m off the terrain", self.off_list.len(), wce_formats::c_num::printf_g_prec(f64::from(self.off_tol), 3),
        ));
        let elems = self.off_list.iter().map(|o| o.elem).collect::<Vec<_>>();
        gui::begin_disabled(elems.is_empty());
        let select_them = gui::button("Select them");
        gui::same_line();
        if gui::button("Snap them") {
            let n = d.snap_to_terrain(&elems, false);
            self.status = format!("{n} snapped to the terrain");
            self.off_list.clear();
            self.off_doc_version = Some(d.history().version());
        }

        gui::end_disabled();
        let shown = self.off_list.len().min(12);
        let mut pick = None;
        for o in &self.off_list[..shown] {
            let sc = d.tile().tscene();
            let mut n = sc.name(o.elem);
            if n.is_empty() {
                n = sc.type_of(o.elem).to_owned();
            }

            if gui::selectable(&format!("{:+.2} m  {n}##off{}", o.dy, o.elem), false, 0) {
                pick = Some(o.elem);
            }
        }

        if self.off_list.len() > shown {
            gui::text_disabled(&format!("... {} more", self.off_list.len() - shown));
        }

        if select_them {
            e.selection.clear();
            for &el in &elems {
                e.selection.add(d.ref_of(el));
            }
        }

        if let Some(el) = pick {
            e.selection.set(d.ref_of(el));
        }
    }

    /// The Weather panel: which environment the game picks for the tile (every filter and file); the usual filters are chips over the viewport.
    fn draw_weather_panel(&mut self, e: &mut Editor) {
        gui::set_next_window_size(gui::v2(420.0, 420.0), gui::COND_FIRST_USE_EVER);
        if let Some(dock) = self.right_dock.filter(|&d| d != 0) {
            gui::set_next_window_dock_id(dock, gui::COND_FIRST_USE_EVER);
        }

        if !gui::begin("Weather", None, 0) {
            gui::end();
            return;
        }

        if document(e).is_some() {
            self.weather.draw(e);
        } else {
            gui::text_disabled("Open a tile first.");
        }

        gui::end();
    }

    /// The tile browser: the entries of the kit's tile database (an index, not a scan), a swatch of the tile's colour, its size and
    /// variations. A double-click opens the first variation, the buttons under the list open one.
    fn draw_browser_panel(&mut self, e: &mut Editor) {
        gui::set_next_window_size(gui::v2(520.0, 340.0), gui::COND_FIRST_USE_EVER);

        // The first time the tool is shown, the Tiles list is the panel in front (a tile is opened from it), its filter ready for typing.
        // A few frames: the dock layout is applied on the first ones and would take the focus back.
        let focus = self.browse_focus_frames > 0;
        if focus {
            gui::set_next_window_focus();
            self.browse_focus_frames -= 1;
        }

        if !gui::begin("Tiles", None, 0) {
            gui::end();
            return;
        }

        self.left_dock = Some(gui::window_dock_id());

        if gui::time() - self.browse_roots_time > 2.0 {
            self.browse_roots = self.kit_roots(e);
            self.browse_roots_time = gui::time();
        }

        if self.browse_roots.is_empty() {
            gui::text_wrapped("No assembly kit found. Set the game folder (the one with assembly_kit_battle_tilemap) or add Assembly Kits in Settings > Game.");
            gui::end();
            return;
        }

        // The kits' raw tiles, then the game's packs (exported tiles, decompiled into the workspace when opened).
        const PACKS_LABEL: &str = "game packs (exported tiles)";
        // The projects are named so; a kit by its folder, or by its whole path when several kits have a folder of that name.
        let projects_raw = Self::workspace_dir(e).join("raw_data");
        let folder_name = |r: &Path| r.parent().and_then(Path::file_name).map(|f| f.to_string_lossy().into_owned()).unwrap_or_default();
        let kit_labels = self.browse_roots.iter().map(|r| {
            if *r == projects_raw {
                return "projects (tile.wce)".to_owned();
            }

            let name = folder_name(r);
            let twins = self.browse_roots.iter().filter(|o| **o != projects_raw && folder_name(o) == name).count();
            if twins > 1 { r.parent().unwrap_or(r).display().to_string() } else { name }
        }).collect::<Vec<_>>();
        self.browse_kit = self.browse_kit.min(self.browse_roots.len());
        let packs = self.browse_kit == self.browse_roots.len();
        gui::set_next_item_width(220.0);
        if gui::begin_combo("kit", &if packs { PACKS_LABEL.to_owned() } else { kit_labels[self.browse_kit].clone() }) {
            for (i, label) in kit_labels.iter().enumerate() {
                if gui::selectable(label, i == self.browse_kit, 0) {
                    self.browse_kit = i;
                }
            }

            if gui::selectable(PACKS_LABEL, packs, 0) {
                self.browse_kit = self.browse_roots.len();
            }

            gui::end_combo();
        }

        let packs = self.browse_kit == self.browse_roots.len();

        gui::same_line();
        gui::set_next_item_width(110.0);
        gui::combo("##kind", &mut self.browse_kind, &["all kinds", "battle", "campaign"]);
        gui::set_next_item_width(-1.0);
        if focus && self.browse_focus_frames == 0 {
            gui::set_keyboard_focus_here(0);
        }

        gui::input_text("##tilefilter", Some("filter: set, tile, variation (words are and-ed)"), &mut self.browse_filter, 96, 0);
        // The rows' source: the kit's index, or the packs' databases (read on first use: the game's packs, a few seconds).
        let source: Arc<Vec<wce_formats::tile_index::TileIndexEntry>> = if packs {
            match self.pack_entries(e) {
                Ok(rows) => rows,
                Err(err) => {
                    gui::text_wrapped(&format!("The game's packs did not load: {err}"));
                    gui::end();
                    return;
                }
            }
        } else {
            let index = TileIndex::shared(&self.browse_roots[self.browse_kit]);
            if self.browse_index.as_ref().is_none_or(|i| !Arc::ptr_eq(i, &index)) {
                self.browse_kit_rows = Arc::new(index.entries().to_vec());
                self.browse_index = Some(index);
            }

            self.browse_kit_rows.clone()
        };

        // The rows, the favourites and the recent tiles of the source, filtered alike (rebuilt when the source, a filter or the marks changed).
        let src = self.browse_source();
        self.marks(e);
        let key = format!("{}|{}|{}|{:p}|{}", self.browse_kit, self.browse_kind, self.browse_filter, Arc::as_ptr(&source), self.browse_marks_version);
        if key != self.browse_rows_key {
            self.browse_rows_key = key;
            let words = self.browse_filter.split(' ').filter(|w| !w.is_empty()).map(str::to_lowercase).collect::<Vec<_>>();
            let kind = self.browse_kind;
            let pass = |en: &wce_formats::tile_index::TileIndexEntry| {
                let kind_ok = match kind {
                    1 => en.kind == "battle",
                    2 => en.kind == "campaign",
                    _ => true,
                };

                kind_ok && matches_words(en, &words)
            };

            self.browse_rows = source.iter().enumerate().filter(|(_, en)| pass(en)).map(|(i, _)| i).collect();

            // The projects' extracted tiles (a variation's tile.wce marked), shown so.
            self.browse_extracted.clear();
            if !packs && self.browse_roots[self.browse_kit] == projects_raw {
                let raw = &self.browse_roots[self.browse_kit];
                self.browse_extracted = source.iter()
                    .filter(|en| en.variations.iter().any(|v| wce_tile::project_files::is_extracted_dir(&raw.join(&en.key).join(v))))
                    .map(|en| en.key.clone())
                    .collect();
            }
            let by_key: HashMap<&str, usize> = source.iter().enumerate().map(|(i, en)| (en.key.as_str(), i)).collect();
            let (fav, recent) = self.browse_marks.as_ref().expect("read above");
            let row = |r: &TileRef| (r.source == src).then(|| by_key.get(r.key.as_str()).copied()).flatten().filter(|&i| pass(&source[i]));
            self.browse_fav_keys = fav.iter().filter(|r| r.source == src).map(|r| r.key.clone()).collect();
            self.browse_fav_rows = fav.iter().filter_map(row).collect();
            self.browse_recent_rows = recent.iter().filter_map(|r| row(r).map(|i| (i, r.variation.clone()))).collect();
        }

        let footer = gui::frame_height_with_spacing() * 4.5;
        let mut open = None;
        let mut star = None;
        let entries = &source[..];
        let filtered = !self.browse_filter.trim().is_empty();
        if gui::begin_child("tilelist", gui::v2(0.0, -footer), gui::CHILD_BORDERS, 0) {
            let selected_key = &mut self.browse_selected;
            let favs = &self.browse_fav_keys;
            let extracted = &self.browse_extracted;

            // Favourites and recent tiles first: a double-click opens a favourite's first variation, a recent tile's variation opened.
            let open_row = |en: &wce_formats::tile_index::TileIndexEntry, variation: &str| {
                let name = if variation.is_empty() { en.variations.first().cloned().unwrap_or_default() } else { variation.to_owned() };
                (en.xml.display().to_string(), variation.to_owned(), en.key.clone(), name)
            };

            let header = |id: &str, title: String| gui::tree_node_ex_id(id, gui::TREE_DEFAULT_OPEN | gui::TREE_SPAN_AVAIL_WIDTH, &title);
            if header("favourites", format!("{}  Favourites ({})", icons::STAR, self.browse_fav_rows.len())) {
                if self.browse_fav_rows.is_empty() {
                    gui::text_disabled(if filtered { "No favourite matches the filter." } else { "Click a tile's star to add it here." });
                }

                for (i, &r) in self.browse_fav_rows.iter().enumerate() {
                    let en = &entries[r];
                    gui::push_id_int(i as i32);
                    let (clicked, double, starred) = tile_row(en, *selected_key == en.key, true, "", extracted.contains(&en.key));
                    if clicked {
                        selected_key.clone_from(&en.key);
                    }

                    if double {
                        open = Some(open_row(en, ""));
                    }

                    if starred {
                        star = Some(en.key.clone());
                    }

                    gui::pop_id();
                }

                gui::tree_pop();
            }

            if header("recent", format!("{}  Recently opened ({})", icons::HISTORY, self.browse_recent_rows.len())) {
                if self.browse_recent_rows.is_empty() {
                    gui::text_disabled(if filtered { "No recent tile matches the filter." } else { "The last tiles you opened show here." });
                }

                for (i, (r, variation)) in self.browse_recent_rows.iter().enumerate() {
                    let en = &entries[*r];
                    gui::push_id_int(i as i32);
                    let note = if variation.is_empty() { String::new() } else { format!("  ({variation})") };
                    let (clicked, double, starred) = tile_row(en, *selected_key == en.key, favs.contains(&en.key), &note, extracted.contains(&en.key));
                    if clicked {
                        selected_key.clone_from(&en.key);
                    }

                    if double {
                        open = Some(open_row(en, variation));
                    }

                    if starred {
                        star = Some(en.key.clone());
                    }

                    gui::pop_id();
                }

                gui::tree_pop();
            }

            let rows = &self.browse_rows;
            gui::separator_text(&format!("All tiles ({} of {})", rows.len(), source.len()));
            gui::push_id("all");
            gui::clipped(rows.len(), |i| {
                let en = &entries[rows[i]];
                gui::push_id_int(i as i32);
                let (clicked, double, starred) = tile_row(en, *selected_key == en.key, favs.contains(&en.key), "", extracted.contains(&en.key));
                if clicked {
                    selected_key.clone_from(&en.key);
                }

                if double {
                    open = Some(open_row(en, ""));
                }

                if starred {
                    star = Some(en.key.clone());
                }

                gui::pop_id();
            });
            gui::pop_id();
        }

        gui::end_child();
        if let Some(key) = star {
            self.toggle_favourite(e, &src, &key);
        }

        match self.browse_rows.iter().map(|&i| &entries[i]).find(|en| en.key == self.browse_selected) {
            Some(sel) => {
                gui::text(&sel.name);
                gui::same_line();
                gui::text_disabled(&format!(
                    "{}, {} x {} cells ({} x {} m), {} links, {} link targets", sel.tile_set, sel.width, sel.height, sel.width * 128, sel.height * 128, sel.links,
                    sel.targets,
                ));
                for (v, name) in sel.variations.iter().enumerate() {
                    if v != 0 {
                        gui::same_line();
                    }

                    if gui::button(&format!("{name}##v{v}")) {
                        open = Some((sel.xml.display().to_string(), v.to_string(), sel.key.clone(), name.clone()));
                    }
                }
            }

            None if packs => gui::text_disabled("Select a tile; double-click opens its first variation (decompiled into the workspace)."),
            None => gui::text_disabled("Select a tile; double-click opens its first variation."),
        }

        gui::end();
        if let Some((xml, variation, key, variation_name)) = open {
            // A pack tile: its folder (`terrain/tiles/<kind>/.../<variation>`) extracted (asked first when a project is there).
            let msg = if packs {
                self.extract_or_ask(e, &format!("{key}/{}", variation_name.to_lowercase()))
            } else {
                self.open_tile(e, &xml, &variation).unwrap_or_else(|m| format!("open failed: {m}"))
            };
            e.status(&msg, false);
        }
    }

    /// The tile's database entry: where it is, what the tile is, the ground's texture groups (editable), the variation's height / normal values, the
    /// links.
    fn draw_entry_section(&mut self, e: &mut Editor, id: DocumentId) {
        if !gui::collapsing_header("Tile database entry", gui::TREE_DEFAULT_OPEN) {
            return;
        }

        let d = e.docs.find_as::<TileDocument>(id).expect("a tile");
        let p = d.project();
        let Some(en) = &p.entry else {
            gui::text_colored(theme::warning(), "This tile has no database entry (it is not inside an assembly kit's raw_data/terrain/tiles/<kind>).");
            return;
        };

        let vars = en.variations();
        let current = p.variation;
        gui::text_disabled(&en.file().file_name().map(|f| f.to_string_lossy().into_owned()).unwrap_or_default());
        if gui::is_item_hovered() {
            gui::set_tooltip(&en.file().display().to_string());
        }

        gui::text(&en.location_generic());
        gui::text(&format!(
            "set {}   {} x {} cells = {} x {} m{}", en.tile_set(), en.width(), en.height(), en.width() * 128, en.height() * 128,
            if en.mask().is_empty() { "" } else { "   (footprint mask)" },
        ));
        if !en.custom_alpha_blend_texture().is_empty() {
            gui::text(&format!("custom alpha blend texture: {}", en.custom_alpha_blend_texture()));
        }

        if !en.custom_blend_tile().is_empty() {
            gui::text(&format!("custom blend tile: {}", en.custom_blend_tile()));
        }

        // The ground's four texture groups (the blend map's groups; the paint brush lays their variations).
        let g = en.texture_group();
        let names = d.texture_group_names();
        let mut set_group = None;
        gui::text("texture groups");
        for (slot, (label, now)) in ["R", "G", "B", "A"].iter().zip([&g.red, &g.green, &g.blue, &g.alpha]).enumerate() {
            gui::same_line();
            gui::set_next_item_width(90.0);
            if gui::begin_combo(&format!("##texgroup{slot}"), now) {
                for n in &names {
                    if gui::selectable(n, n == now, 0) && n != now {
                        set_group = Some((slot, n.clone()));
                    }
                }

                gui::end_combo();
            }

            if gui::is_item_hovered() {
                gui::set_tooltip(&format!("{label}: the blend map's group {}; saved into the entry next to the tile", slot + 1));
            }
        }

        let mut reopen = None;
        if vars.len() > 1 {
            gui::set_next_item_width(160.0);
            let shown = current.and_then(|v| vars.get(v)).map_or("?", |v| v.location.as_str());
            if gui::begin_combo("variation", shown) {
                for (i, v) in vars.iter().enumerate() {
                    if gui::selectable(&v.location, Some(i) == current, 0) && Some(i) != current {
                        reopen = Some(i);
                    }
                }

                gui::end_combo();
            }

            gui::same_line();
            gui::text_disabled("opens in its own document");
        }

        if gui::begin_table("variations", 7, gui::TABLE_SIZING_FIXED_FIT | gui::TABLE_ROW_BG | gui::TABLE_BORDERS_INNER_V, gui::v2(0.0, 0.0)) {
            for c in ["variation", "min h", "scale", "normal", "overlap", "tri density", "common maps"] {
                gui::table_setup_column(c, 0, 0.0);
            }

            gui::table_headers_row();
            for (i, v) in vars.iter().enumerate() {
                gui::table_next_row();
                gui::table_next_column();
                gui::text(&format!("{}{}", v.location, if Some(i) == current { " *" } else { "" }));
                gui::table_next_column();
                gui::text(&format!("{:.3}", v.min_height));
                gui::table_next_column();
                gui::text(&format!("{:.3}", v.scale));
                gui::table_next_column();
                gui::text(&format!("{:.2}", v.normal_strength));
                gui::table_next_column();
                gui::text(&format!("{:.0}", v.overlap_border_size));
                gui::table_next_column();
                gui::text(&v.raw_data_tri_density.to_string());
                gui::table_next_column();
                let common = v.blend_common.is_empty() && v.index_common.is_empty() && v.normal_common.is_empty();
                gui::text_disabled(&if common { "-".to_owned() } else { format!("{} {} {}", v.blend_common, v.index_common, v.normal_common) });
            }

            gui::end_table();
        }

        gui::text_disabled(&format!("{} link target(s), {} link rule(s): edited in Tile links", en.link_targets().len(), en.links().len()));

        if let Some((slot, name)) = set_group {
            let d = e.docs.find_as_mut::<TileDocument>(id).expect("a tile");
            if let Err(m) = d.set_texture_group(slot, &name) {
                e.status(&m, true);
            }

            return;
        }

        if let Some(i) = reopen {
            let file = en.file().display().to_string();
            let msg = self.open_tile(e, &file, &i.to_string()).unwrap_or_else(|m| format!("open failed: {m}"));
            e.status(&msg, false);
            return;
        }

        // The properties TEd's Tile Properties edits (and the flags): colour, normal strength, flags, mask.
        self.draw_tile_properties(e, id);
    }

    /// The selected polygon's or spline's points (shown while one is selected; new ones are made in the Create panel).
    fn draw_polygon_panel(&mut self, e: &mut Editor, id: DocumentId) {
        let sel = primary(e);
        let is_polygon = e.docs.find_as::<TileDocument>(id).is_some_and(|d| sel.is_some_and(|s| d.tile().is_polygon(s)));
        if is_polygon && gui::collapsing_header("Points", gui::TREE_DEFAULT_OPEN) {
            self.show_selected_polygon(e, id);
        }
    }

    /// The selected polygon's points and area.
    fn show_selected_polygon(&mut self, e: &mut Editor, id: DocumentId) {
        let sel = primary(e);
        let Some(d) = e.docs.find_as_mut::<TileDocument>(id) else { return };
        if let Some(s) = sel.filter(|&s| d.is_spline(s)) {
            self.show_selected_spline(d, s);
            return;
        }

        if let Some(s) = sel.filter(|&s| d.tile().is_polygon(s)) {
            let sc = d.tile().tscene();
            gui::text(&format!("selected: {} points, area {:.1}", sc.polygon_points(s).len(), sc.polygon_area(s)));
            match self.poly_point {
                Some(pt) => gui::text(&format!("point {pt}: drag to move, Delete removes, shift+click edge inserts")),
                None => gui::text_disabled("click a point handle to edit; shift+click near an edge inserts a point"),
            }

            if d.tile().is_wall(s) {
                self.draw_wall_rows(e, id, s);
            }
        }
    }

    /// The selected wall: its layout, and the selected point's corner mark.
    fn draw_wall_rows(&mut self, e: &mut Editor, id: DocumentId, wall: usize) {
        let Some(d) = e.docs.find_as_mut::<TileDocument>(id) else { return };
        let Some(l) = d.tile().wall_layout(wall) else { return };
        gui::text(&wall_summary(&l));
        let ps = wce_walls::pieces::pieces(&l.plan, l.profile);
        let (doors, skipped) = d.tile().wall_doors(&l, &ps);
        let (defences, defences_skipped) = wce_walls::pieces::defence_slots(&l.plan, l.profile);
        gui::text(&format!("{} piece(s), {} door section(s), {} tower(s) / bastion(s)", ps.len(), doors.len(), defences.len()));
        for w in l.plan.warnings.iter().chain(&skipped).chain(&defences_skipped) {
            gui::text_colored([1.0, 0.6, 0.2, 1.0], w);
        }

        let Some(pt) = self.poly_point else { return };
        let points = d.tile().tscene().polygon_points(wall);
        let Some(&p) = points.get(pt) else { return };
        let mut sharp = d.tile().tscene().doc().get(p, "sharp").is_some_and(|v| wce_tile::properties::parse_bool(&v));
        if gui::checkbox("sharp corner (a mitre instead of a curve)", &mut sharp) {
            d.set_point_attribute(wall, pt, "sharp", wce_tile::properties::format_bool(sharp));
        }
    }

    /// A river's or road's points: how to edit them, and the values of the selected point (or of all of them).
    fn show_selected_spline(&mut self, d: &mut TileDocument, s: usize) {
        let sc = d.tile().tscene();
        let points = sc.polygon_points(s);
        gui::text(&format!("selected: {} points", points.len()));
        gui::same_line();
        if gui::small_button("Level the ground") {
            self.status = d.level_along(s, &self.brush).map_or_else(|why| why, |n| format!("ground levelled along the spline: {n} dabs"));
            return;
        }

        if gui::is_item_hovered() {
            gui::set_tooltip(crate::actions::LEVEL_ALONG_TIP);
        }

        gui::same_line();
        if gui::small_button("Flip direction") {
            self.status = d.flip_spline(s).map_or_else(|why| why, |()| "flipped: the spline runs the other way".to_owned());
            return;
        }

        if gui::is_item_hovered() {
            gui::set_tooltip(
                "Reverses the spline: a river flows the other way (a road runs the other way). The points keep their places and values \
                 (width, foam, flow speed, ...) in the other order, each point's handles swapped; one undo step.",
            );
        }

        match self.poly_point.filter(|&p| p < points.len()) {
            Some(pt) => gui::text(&format!("point {pt}: drag it or its blue handles, Delete removes, shift+click inserts (beyond an end: grows)")),
            None => gui::text_disabled("click a point to edit it; drag the blue handles to bend; shift+click inserts a point"),
        }

        gui::checkbox("all points", &mut self.spline_all);
        let Some(shown) = (if self.spline_all { Some(0) } else { self.poly_point.filter(|&p| p < points.len()) }) else { return };
        let targets = if self.spline_all { (0..points.len()).collect::<Vec<_>>() } else { vec![shown] };
        let kind = sc.type_of(s).to_owned();
        let Some(t) = wce_tile::instance_templates::template(&kind) else { return };
        for &(attr, _) in t.point_attrs.iter().filter(|(a, _)| !matches!(*a, "position" | "controlIn" | "controlOut")) {
            let sc = d.tile().tscene();
            let mut v = sc.doc().get(points[shown], attr).and_then(|v| v.trim().parse::<f32>().ok()).unwrap_or(0.0);
            let speed = if matches!(attr, "width" | "terrain_offset") { 0.1 } else { 0.01 };
            let changed = gui::drag_float(attr, &mut v, speed, 0.0, 0.0, "%g");
            if gui::is_item_activated() {
                self.value_gesture = d.next_gesture();
            }

            if changed {
                d.set_spline_value(s, &targets, attr, &wce_tile::ted_scene::format_general(f64::from(v)), self.value_gesture);
            }

            if gui::is_item_deactivated() {
                d.history_mut().break_merge();
            }
        }
    }

}

/// A browser row matches when every word is in its location, file name, set or variation names, in any case.
/// A row of the Tiles panel: the tile's colour, its location, size and variations (and `note`), a package when it is an extracted tile, and
/// its star on the right. Returns whether the row was clicked, double-clicked, and whether the star was.
fn tile_row(en: &wce_formats::tile_index::TileIndexEntry, selected: bool, favourite: bool, note: &str, extracted: bool) -> (bool, bool, bool) {
    gui::color_swatch("##c", [en.red / 255.0, en.green / 255.0, en.blue / 255.0, 1.0], gui::v2(12.0, 12.0));
    gui::same_line();
    let prefix = format!("terrain/tiles/{}/", en.kind);
    let shown = en.key.strip_prefix(&prefix).unwrap_or(&en.key);
    let vars = if en.variations.len() > 1 { format!("  +{}v", en.variations.len()) } else { String::new() };
    let width = (gui::content_region_avail().x - gui::frame_height_with_spacing()).max(10.0);
    let mark = if extracted { format!("  {}", icons::PACKAGE) } else { String::new() };
    let clicked = gui::selectable_sized(&format!("{shown}  {}x{}{vars}{note}{mark}", en.width, en.height), selected, gui::SELECTABLE_ALLOW_DOUBLE_CLICK, gui::v2(width, 0.0));
    if extracted && gui::is_item_hovered() {
        gui::set_tooltip("Extracted from the game's packs (wce_extracted_tile)");
    }

    let double = clicked && gui::is_mouse_double_clicked(0);
    gui::same_line();
    gui::push_style_color(gui::COL_BUTTON, gui::v4(0.0, 0.0, 0.0, 0.0));
    gui::push_style_color(gui::COL_TEXT, theme::v4(if favourite { theme::palette().highlight } else { theme::mix(theme::dim(), theme::palette_background(), 0.4) }));
    let starred = gui::small_button(&format!("{}##star", icons::STAR));
    gui::pop_style_color(2);
    if gui::is_item_hovered() {
        gui::set_tooltip(if favourite { "Remove from the favourites" } else { "Add to the favourites" });
    }

    (clicked, double, starred)
}

pub(crate) fn matches_words(en: &wce_formats::tile_index::TileIndexEntry, words: &[String]) -> bool {
    let mut hay = format!("{} {} {}", en.key, en.name, en.tile_set);
    for v in &en.variations {
        hay += " ";
        hay += v;
    }

    let hay = hay.to_lowercase();
    words.iter().all(|w| hay.contains(&w.to_lowercase()))
}

/// The instance type whose markers are the VFX boxes.
const VFX_TYPE: &str = "VFXInstance";

/// Shows or hides the VFX boxes (the VFXInstance markers; the effects still play) of the open tile, and keeps it in the settings for the next
/// tiles. The same switch as the type's eye in the Scene panel.
pub fn set_vfx_boxes(e: &mut Editor, on: bool) {
    if let Some(d) = document(e) {
        let t = d.tile_mut();
        if on {
            t.display_mut().hidden_types.remove(VFX_TYPE);
        } else {
            t.display_mut().hidden_types.insert(VFX_TYPE.to_owned());
        }

        t.display_changed();
    }

    e.update_tool_settings(SETTINGS_ID, |s: &mut TileSettings| s.show_vfx_boxes = on);
}

/// Draws the sea with a battle wind's waves and whitecaps, and keeps it in the settings.
pub fn set_sea_state(e: &mut Editor, key: &str) {
    if let Some(r) = e.renderer() {
        r.settings.sea_state = key.to_owned();
    }

    e.update_tool_settings(SETTINGS_ID, |s: &mut TileSettings| s.sea_state = key.to_owned());
}

impl Tool for TileTool {
    fn id(&self) -> &str {
        "tile"
    }

    fn title(&self) -> &str {
        "Tile"
    }

    fn icon(&self) -> &str {
        wce_editor::icons::MAP
    }

    fn has_settings(&self) -> bool {
        true
    }

    fn settings_applied(&mut self, _: &mut Editor) {
        // The favourites and recent tiles are read again (the window writes the whole table).
        self.browse_marks = None;
        self.browse_marks_version += 1;

        // The Assembly Kit or the projects folder may have changed.
        self.prefabs = None;
    }

    fn draw_settings(&mut self, draft: &mut Settings) {
        let mut s: TileSettings = draft.tool(SETTINGS_ID);
        let before = s.clone();
        // The folders are the game's (Settings > Game): shown here for reference.
        settings_window::label("Assembly Kits");
        let kits = draft.assembly_kit_dirs();
        if kits.is_empty() {
            gui::text_disabled("none (set them in the game's settings)");
        }

        for (i, kit) in kits.iter().enumerate() {
            if i > 0 {
                gui::set_cursor_pos_x(settings_window::LABEL_WIDTH);
            }

            gui::text_disabled(&kit.display().to_string());
        }

        settings_window::label("Tile projects");
        gui::text_disabled(&draft.kit_projects_dir().display().to_string());
        settings_window::help("Tiles are saved as tile.wce in the projects folder (assembly_kit/, in the kit's layout); extracted tiles go there too. Prefabs are read from both raw_data/art/prefabs, new ones go to the projects'. Save for Assembly Kit writes a tile TEd reads into the kit.");

        gui::separator_text("New tiles");
        gui::set_cursor_pos_x(settings_window::LABEL_WIDTH);
        gui::checkbox("Show the sea at 0 m", &mut s.show_sea);
        gui::set_cursor_pos_x(settings_window::LABEL_WIDTH);
        gui::checkbox("Show the VFX boxes", &mut s.show_vfx_boxes);
        settings_window::help("What a tile shows when it opens; the Tile panel and the Tile menu change the open tile.");

        gui::separator_text("Launch in game");
        settings_window::label("Delivery");
        if gui::radio_button("Launch pack", s.launch.delivery == crate::settings::LaunchDelivery::Pack) {
            s.launch.delivery = crate::settings::LaunchDelivery::Pack;
        }

        gui::same_line();
        if gui::radio_button("Loose files", s.launch.delivery == crate::settings::LaunchDelivery::Loose) {
            s.launch.delivery = crate::settings::LaunchDelivery::Loose;
        }

        settings_window::help("Loose files lose to the game's packs; a tile falls back to the pack.");
        settings_window::label("Map");
        if gui::radio_button("The tile alone", s.launch.map == crate::settings::LaunchMap::Tile) {
            s.launch.map = crate::settings::LaunchMap::Tile;
        }

        gui::same_line();
        if gui::radio_button("The Map context's placement", s.launch.map == crate::settings::LaunchMap::MapContext) {
            s.launch.map = crate::settings::LaunchMap::MapContext;
        }

        settings_window::help("The placement works in the game's campaign map only (the kit's maps are not in the game's packs); without one the tile alone is used.");
        for (label, value, hint) in [
            ("Defender faction", &mut s.launch.defender_faction, "att_fact_western_roman_empire"),
            ("Defender unit", &mut s.launch.defender_unit, "att_rom_legio_comitatenses"),
            ("Attacker faction", &mut s.launch.attacker_faction, "att_fact_saxones"),
            ("Attacker unit", &mut s.launch.attacker_unit, "att_nor_saxon_spears"),
            ("Subculture", &mut s.launch.subculture, "att_sub_cult_roman_west"),
            ("Tile upgrades", &mut s.launch.tile_upgrades, "level1 escalation3 (a settlement in the map)"),
        ] {
            settings_window::label(label);
            gui::set_next_item_width(-1.0);
            gui::input_text(&format!("##launch {label}"), Some(hint), value, 256, 0);
        }

        settings_window::help("One unit a side. Vanilla keys are safest: a mod's units may need packs that are not enabled.");
        if s != before {
            draft.set_tool(SETTINGS_ID, &s);
        }
    }

    fn opens(&self) -> Vec<String> {
        vec![".ted".to_owned(), ".wce".to_owned()]
    }

    fn default_layout(&self, dockspace: gui::ImGuiID) {
        let (left, center) = gui::dock_builder_split(dockspace, gui::DIR_LEFT, 0.22);
        let (right, center) = gui::dock_builder_split(center, gui::DIR_RIGHT, 0.30);
        let (bottom, center) = gui::dock_builder_split(center, gui::DIR_DOWN, 0.18);
        gui::dock_builder_dock_window("Viewport", center);
        gui::dock_builder_dock_window("Tiles", left);
        gui::dock_builder_dock_window("Create", left);
        gui::dock_builder_dock_window("Scene", left);
        gui::dock_builder_dock_window("Outliner", left);
        gui::dock_builder_dock_window("Tile", right);
        gui::dock_builder_dock_window("Inspector", right);
        gui::dock_builder_dock_window("Weather", right);
        gui::dock_builder_dock_window("Assets", bottom);
        gui::dock_builder_dock_window("Log", bottom);
        gui::dock_builder_dock_window("History", bottom);
    }

    fn register_commands(&self, commands: &mut CommandRegistry<Editor>) {
        crate::commands::register(commands);
    }

    fn activated(&mut self, e: &mut Editor) {
        if e.viewport.is_fly() {
            e.viewport.use_orbit();
        }

        e.viewport.orbit_mut().min_eye_height = -1e6;
        let key = e.tool_settings::<TileSettings>(SETTINGS_ID).sea_state;
        if !key.is_empty()
            && let Some(r) = e.renderer()
        {
            r.settings.sea_state = key;
        }
    }

    fn update(&mut self, e: &mut Editor) {
        self.update_launch(e);
        let Some(id) = document_id(e) else {
            // The last tile closed: its weather goes with it.
            if self.camera_set_for != 0 {
                self.camera_set_for = 0;
                let _ = e.set_weather_preview(None);
            }

            return;
        };

        // A new tile: frame it, and light it with its weather.
        if self.camera_set_for != id {
            self.camera_set_for = id;
            let d = e.docs.find_as::<TileDocument>(id).expect("a tile");
            let (ex, ez) = (d.tile().extent_x(), d.tile().extent_z());
            if !e.viewport.is_fly() {
                e.viewport.orbit_mut().set_orbit(0.8, 0.6, (ex.max(ez) * 1.2) as f32, Float3::new((ex * 0.5) as f32, 0.0, (ez * 0.5) as f32));
            }

            self.weather.tile_changed(e, id);
            self.note_recent(e, id);

            // The library holds the tile's kit's prefabs too.
            self.prefabs = None;
            self.poly_point = None;
            self.drawing = false;
            self.grabbing = false;
            self.stroke_active = false;
            self.mode = TileMode::Select;
            self.context.tile_changed(e, id);

            // The Scene tree in front, not the Tiles list.
            self.browse_focus_frames = 0;
            self.scene_focus_frames = 3;
        }

        self.sync_brush_curve(e);
        self.sync_image_brush(e);
        self.update_water_depth(e, id);
        self.thumbs.update(e);

        // The ground types brush paints over the ground types view; the view before comes back after.
        if self.mode == TileMode::Ground && self.view_before_ground.is_none() {
            let d = e.docs.find_as_mut::<TileDocument>(id).expect("a tile");
            let look = d.tile().terrain_look().clone();
            self.view_before_ground = Some((id, look.clone()));
            d.tile_mut().set_terrain_look(&mut e.scene, TerrainLook { blended: true, view: TerrainView::GroundTypes, ..look });
        } else if self.mode != TileMode::Ground
            && let Some((doc, look)) = self.view_before_ground.take()
            && let Some(d) = e.docs.find_as_mut::<TileDocument>(doc)
        {
            d.tile_mut().set_terrain_look(&mut e.scene, look);
        }

        drop_gone_from_selection(e);
        self.select_whole_prefabs(e);
        self.context.poll(e, id);
        e.viewport.north = self.context.north();
        e.viewport.map_frame = self.context.map_frame();

        // Structural edits move element indices: the command says what is selected afterwards.
        let d = e.docs.find_as_mut::<TileDocument>(id).expect("a tile");
        if let Some(hint) = d.take_selection_hint() {
            e.selection.clear();
            for item in hint {
                e.selection.add(SourceRef { doc: id, item });
            }

            self.poly_point = None;
        }

        if let Some(r) = e.renderer.as_mut() {
            let models = r.models();
            d.tile_mut().poll_models(&|piece: &str| {
                let (m, st) = models.peek(piece);
                match m {
                    Some(m) => PieceState { status: PieceStatus::Ready, bmin: m.bmin, bmax: m.bmax },
                    None if st == ModelState::Failed => PieceState { status: PieceStatus::Failed, ..Default::default() },
                    None => PieceState::default(),
                }
            });

            // The collapsing buildings' fragments (queued now: they play once loaded).
            d.tile_mut().poll_collapses(&|model: &str| match models.try_get(model, 0.0) {
                Some(_) => PieceStatus::Ready,
                None if models.state(model) == ModelState::Failed => PieceStatus::Failed,
                None => PieceStatus::Loading,
            });
        }

        let eye = e.viewport.camera().eye;
        let sel = selected(e);
        let handle = Self::handle_size(e);
        let d = e.docs.find_as::<TileDocument>(id).expect("a tile");
        let overlay = self.tool_overlay(e, d);
        // The preview: the stroke's trees while the brush paints, else what a click places.
        let (preview, at) = if self.painting_trees(e) {
            (self.stroke_preview(e), Float3::default())
        } else if self.terrain_hit && (e.viewport.hovered || self.preview_forced) {
            (self.placing_text(e), self.place_point().0)
        } else {
            (None, self.place_point().0)
        };

        let d = e.docs.find_as_mut::<TileDocument>(id).expect("a tile");
        d.tile_mut().update_view(&mut e.scene, eye);
        d.tile_mut().set_preview(&mut e.scene, preview.as_deref(), at);
        let hot = if self.light_drag.is_some() {
            Some(wce_tile_editor::tile_document::LIGHT_RADIUS_HANDLE)
        } else if self.target_drag.is_some() {
            Some(wce_tile_editor::tile_document::SHELTER_TARGET_HANDLE)
        } else {
            self.poly_point
        };
        d.tile_mut().set_highlight(&mut e.scene, &sel, hot, handle, &overlay);
    }

    fn draw_panels(&mut self, e: &mut Editor) {
        drop_gone_from_selection(e);
        self.handle_keys(e);
        {
            wce_foundation::wce_prof!("tile.panel.tile");
            self.draw_tile_panel(e);
        }

        {
            wce_foundation::wce_prof!("tile.panel.scene");
            crate::scene_panel::draw_layers_panel(self, e);
        }

        {
            wce_foundation::wce_prof!("tile.panel.tiles");
            self.draw_browser_panel(e);
        }

        {
            wce_foundation::wce_prof!("tile.panel.weather");
            self.draw_weather_panel(e);
        }

        wce_foundation::wce_prof!("tile.panel.create");
        self.draw_create_panel(e);
        self.draw_new_set_popup(e);
        self.draw_new_tile_dialog(e);
        self.draw_save_tile_as(e);
        self.draw_resize_tile(e);
        self.draw_tile_database(e);
        self.draw_height_import_dialog(e);
        self.draw_falloff_window(e);
        self.draw_extract_prompt(e);
        self.draw_kit_prompt(e);
        self.draw_launch_prompt(e);
        self.draw_launch_progress(e);
        self.draw_quick_add(e);
    }

    fn draw_menus(&mut self, e: &mut Editor) {
        if !gui::begin_menu("Tile") {
            return;
        }

        if gui::menu_item("New tile...", None, false, true) {
            self.open_new_tile_dialog();
        }

        if gui::is_item_hovered() {
            gui::set_tooltip("A new battle tile, flat, as a project in the projects folder (as TEd's New Tile, named at once).");
        }

        if gui::menu_item("Save tile as...", None, false, document(e).is_some()) {
            self.open_save_tile_as(e);
        }

        if gui::is_item_hovered() {
            gui::set_tooltip("TEd's Save Tile As: the tile as another battle tile (tile set, name, variation) in the projects folder, with its own database entry; the document goes on as it.");
        }

        if gui::menu_item("Tile database...", None, false, true) {
            self.open_tile_database(e);
        }

        if gui::is_item_hovered() {
            gui::set_tooltip("TEd's Tile Database Management: the battle tile sets of the kit and of the projects with their tiles, and New tile set (into the projects' tile database).");
        }

        if gui::menu_item("Resize tile...", None, false, document(e).is_some()) {
            self.open_resize_tile(e);
        }

        if gui::is_item_hovered() {
            gui::set_tooltip("TEd's Resize Tile: a new size in cells, the old tile centred in it (or at a cell you give), the new area flat or carried out from the edges. Saved and opened again; no undo.");
        }

        if gui::menu_item("Import height map...", None, false, document(e).is_some()) {
            self.open_height_import_dialog(e);
        }

        if gui::is_item_hovered() {
            gui::set_tooltip("Replace the open tile's heights with a picture's (or add them): black and white at the heights you give. One undo step.");
        }

        if gui::menu_item("Open tile...", None, false, true) {
            let filters = [
                ("Tile database entry, tile.wce or tile.ted".to_owned(), "*.xml tile.wce tile.ted *.ted".to_owned()), ("Tile (tile.wce, tile.ted)".to_owned(), "tile.wce tile.ted *.ted".to_owned()),
                ("All files".to_owned(), "*.*".to_owned()),
            ];
            if let Some(p) = file_dialog(false, Path::new(""), &filters) {
                let msg = self.open_tile(e, &p.display().to_string(), "").unwrap_or_else(|m| format!("open failed: {m}"));
                e.status(&msg, false);
            }
        }

        if gui::menu_item("Migrate old extracted projects", None, false, true) {
            let msg = self.migrate_extracted(e).unwrap_or_else(|m| format!("migration failed: {m}"));
            e.status(&msg, false);
        }

        if gui::is_item_hovered() {
            gui::set_tooltip("Moves the projects of tiles extracted by older versions (<tile>_extracted folders) to their own names, marked as extracted tiles; one whose place has a project, or that is open, stays (see the log).");
        }

        if gui::menu_item("Open exported tile...", None, false, true)
            && let Some(dir) = wce_editor::steam_detect::pick_folder("Exported tile folder (terrain/tiles/.../<variation>)")
        {
            let msg = self.extract_or_ask(e, &dir.display().to_string());
            e.status(&msg, false);
        }

        if gui::menu_item("Save as exported tile", None, false, document(e).is_some()) {
            let out = e.output_dir();
            let msg = self.save_exported(e, &out).unwrap_or_else(|m| format!("save failed: {m}"));
            e.status(&msg, false);
        }

        let label = if self.launch_job.is_some() { "Launch in game (preparing)" } else if self.launched.is_some() { "Launch in game (running)" } else { "Launch in game" };
        if gui::menu_item(label, None, false, document(e).is_some() && self.launched.is_none() && self.launch_job.is_none()) {
            let msg = self.launch_in_game(e, false, false, true).unwrap_or_else(|m| format!("launch failed: {m}"));
            e.status(&msg, false);
        }

        if gui::is_item_hovered() {
            gui::set_tooltip("Exports the tile into a launch pack (data\\wce_launch.pack, removed when the game exits) and starts the game straight into a battle on it, with the mod packs chosen in the editor; used_mods.txt is not touched. The map and the armies: Settings > Tile.");
        }

        gui::separator();
        let project = document(e).map(|d| (d.is_project(), wce_tile::project_files::is_extracted_dir(&d.project().dir)));
        if gui::menu_item("Save for Assembly Kit", None, false, project.is_some()) {
            let msg = self.save_for_kit_ask(e).unwrap_or_else(|m| format!("save for the Assembly Kit failed: {m}"));
            e.status(&msg, false);
        }

        if gui::is_item_hovered() {
            gui::set_tooltip("Writes the tile as TEd reads it (tile.ted, the river files, the maps, its database entry) into the Assembly Kit; what TEd lacks is converted or dropped (see the log). Files it replaces keep a .bak.");
        }

        if gui::menu_item("Import from Assembly Kit", None, false, project.is_some_and(|p| p.0)) {
            let msg = self.import_from_kit(e).unwrap_or_else(|m| format!("import failed: {m}"));
            e.status(&msg, false);
        }

        if gui::is_item_hovered() {
            gui::set_tooltip("Opens the kit's copy of this tile (an extracted tile: the game's, extracted again); saving it replaces the project.");
        }

        gui::separator();
        if gui::menu_item("Generate SiegeAI", None, false, document(e).is_some()) {
            self.generate_siege_ai(e);
        }

        if gui::is_item_hovered() {
            gui::set_tooltip(&format!("The street graph, wall edges, fort perimeter, approach lines and deployment zones, into '{AUTOGEN_LAYER}'; the old layout stops being exported (docs/plans/siege_ai.md)."));
        }

        gui::separator();
        if gui::menu_item("Tile browser", None, false, true) {
            gui::set_window_focus("Tiles");
        }

        if let Some(d) = document(e) {
            let t = d.tile_mut();
            let mut changed = false;
            let v = t.display_mut();
            changed |= gui::menu_item_toggle("Polygon outlines", &mut v.show_polygons);
            changed |= gui::menu_item_toggle("Distance culling of markers", &mut v.lod);
            gui::menu_item_toggle("Tile border", &mut v.show_border);
            gui::menu_item_toggle("Tint freeform instances", &mut v.tint_freeform);
            if gui::is_item_hovered() {
                gui::set_tooltip(FREEFORM_TINT_TIP);
            }

            let mut vfx = !v.hidden_types.contains(VFX_TYPE);
            if gui::menu_item_toggle("VFX boxes", &mut vfx) {
                set_vfx_boxes(e, vfx);
            }

            let Some(d) = document(e) else { return gui::end_menu() };
            let t = d.tile_mut();
            let v = t.display_mut();
            gui::separator();
            gui::text("Draw distances (m)");
            gui::set_next_item_width(160.0);
            changed |= gui::drag_float("vegetation", &mut v.veg_distance, 5.0, 20.0, 20000.0, "%.3f");
            gui::set_next_item_width(160.0);
            changed |= gui::drag_float("decals", &mut v.decal_distance, 5.0, 20.0, 20000.0, "%.3f");
            gui::set_next_item_width(160.0);
            changed |= gui::drag_float("others", &mut v.other_distance, 5.0, 20.0, 40000.0, "%.3f");
            if changed {
                t.display_changed();
            }
        }

        gui::end_menu();
    }

    fn viewport_input(&mut self, e: &mut Editor, ev: &ViewportEvent) -> bool {
        let Some(id) = document_id(e) else { return false };
        if self.box_select_input(e, ev.kind, ev.x, ev.y) {
            return true;
        }

        let ray = e.viewport.ray(ev.x, ev.y);
        match ev.kind {
            ViewportEventKind::Hover => {
                self.on_hover(e, ev);
                false
            }

            // The menu of the selection; not while placing or drawing (only Esc stops them), nor for the click that cancelled a grab.
            ViewportEventKind::ContextMenu => {
                if !std::mem::take(&mut self.right_used) && self.placing.is_none() && !self.drawing {
                    self.context_click(e, ev.hit_source);
                }

                true
            }

            ViewportEventKind::Press => {
                self.on_hover(e, ev);
                if self.grabbing {
                    self.end_grab(e, true);
                    return true;
                }

                if self.drawing {
                    if self.terrain_hit {
                        self.draw_pts.push(self.cursor);
                    }

                    return true;
                }

                // The vegetation brush paints (Ctrl: takes away) instead of placing one.
                if self.painting_trees(e) {
                    if self.terrain_hit {
                        self.begin_stroke(e, ev.ctrl);
                    }

                    return true;
                }

                if let Some(placing) = self.placing.clone() {
                    if self.terrain_hit {
                        let _ = placing;
                        self.status = self.place_at(e, self.cursor).unwrap_or_else(|err| err);
                    }

                    return true;
                }

                if self.mode.brush().is_some() {
                    self.paint_erase = ev.ctrl;
                    let kind = self.brush_kind().expect("a brush mode");
                    self.turn_image_brush();
                    let dt = e.clock.dt();
                    let d = e.docs.find_as_mut::<TileDocument>(id).expect("a tile");
                    if self.terrain_hit && d.project().has_height() {
                        self.stroke_active = true;
                        d.brush_step(kind, self.cursor.x, self.cursor.z, self.brush.clone(), dt);

                        // A stamp is one click.
                        if self.mode == TileMode::Stamp {
                            d.end_stroke();
                            self.stroke_active = false;
                        }
                    }

                    return true;
                }

                // Select mode: a point light's radius handle starts a drag of the radius.
                let sel = primary(e);
                let d = e.docs.find_as_mut::<TileDocument>(id).expect("a tile");
                if let (Some((o, dir)), Some(sel)) = (ray, sel)
                    && let Some((k, r)) = d.tile().light_radius_handle(sel)
                {
                    let t = dot(k - o, dir);
                    if t > 0.0 && length(k - (o + dir * t)) < 1.2f32.max(t * 0.015) {
                        let c = d.tile().world_position(sel);
                        self.light_grab = light_plane_distance(c, o, dir).map_or(0.0, |at| r - at);
                        self.light_drag = Some(sel);
                        self.light_gesture = d.next_gesture();
                        return true;
                    }
                }

                // A civilian shelter's target handle starts a drag of the target over the ground.
                if let (Some((o, dir)), Some(sel)) = (ray, sel)
                    && let Some(k) = d.tile().shelter_target(sel)
                {
                    let k = Float3::new(k.x, k.y + 0.3, k.z);
                    let t = dot(k - o, dir);
                    if t > 0.0 && length(k - (o + dir * t)) < 1.2f32.max(t * 0.015) {
                        self.target_drag = Some(sel);
                        self.target_gesture = d.next_gesture();
                        return true;
                    }
                }

                // A polygon point handle starts a drag, shift + click near an edge inserts a point.
                let (Some((o, dir)), Some(sel)) = (ray, sel.filter(|&s| d.tile().is_polygon(s))) else { return false };
                let pts = d.tile().polygon_world_points(sel);
                let spline = d.is_spline(sel);
                let near = |c: Float3| {
                    let t = dot(c - o, dir);
                    (t > 0.0 && length(c - (o + dir * t)) < 1.2f32.max(t * 0.015)).then_some(t)
                };

                // A spline's handles (drawn at their point's height) before its points.
                if spline {
                    let mut best = None;
                    let mut best_t = 1e30f32;
                    for (i, (hin, hout)) in d.tile().spline_world_handles(sel).into_iter().enumerate() {
                        for (h, out) in [(hin, false), (hout, true)] {
                            if let Some(t) = h.and_then(|h| near(Float3::new(h.x, pts[i].y + 0.4, h.z))).filter(|&t| t < best_t) {
                                best_t = t;
                                best = Some((i, out));
                            }
                        }
                    }

                    if let Some((i, out)) = best {
                        self.poly_point = Some(i);
                        self.spline_handle = Some(out);
                        self.point_drag = true;
                        self.point_gesture = d.next_gesture();
                        return true;
                    }
                }

                self.spline_handle = None;
                let mut best = None;
                let mut best_t = 1e30f32;
                for (i, &c) in pts.iter().enumerate() {
                    let t = dot(c - o, dir);
                    if t <= 0.0 {
                        continue;
                    }

                    let perp = length(c - (o + dir * t));
                    if perp < 1.2f32.max(t * 0.015) && t < best_t {
                        best_t = t;
                        best = Some(i);
                    }
                }

                if best.is_some() {
                    self.poly_point = best;
                    self.point_drag = true;
                    self.point_gesture = d.next_gesture();
                    return true;
                }

                if ev.shift && self.terrain_hit {
                    let closed = if spline {
                        d.tile().tscene().doc().get(sel, "closed").is_some_and(|v| v == "1")
                    } else {
                        d.tile().tscene().doc().get(sel, "polygon_closed").is_none_or(|v| v != "0")
                    };

                    let mut at = 0;
                    let mut at_t = 0.5;
                    let mut bd = 1e30f32;
                    let cur = self.cursor;
                    for i in 0..pts.len() {
                        if i + 1 == pts.len() && !closed {
                            break;
                        }

                        let (a, b) = (pts[i], pts[(i + 1) % pts.len()]);
                        let (abx, abz) = (b.x - a.x, b.z - a.z);
                        let l2 = abx * abx + abz * abz;
                        let t = if l2 > 1e-6 { (((cur.x - a.x) * abx + (cur.z - a.z) * abz) / l2).clamp(0.0, 1.0) } else { 0.0 };
                        let (dx, dz) = (a.x + abx * t - cur.x, a.z + abz * t - cur.z);
                        let dd = dx * dx + dz * dz;
                        if dd < bd {
                            bd = dd;
                            at = i;
                            at_t = t;
                        }
                    }

                    // Beyond an end of an open spline, it grows there.
                    let (op, index, new_point) = match (spline && !closed, at_t) {
                        (true, t) if t <= 0.0 && at == 0 => ("prepend", 0, 0),
                        (true, t) if t >= 1.0 && at + 2 == pts.len() => ("insert", at + 1, at + 2),
                        _ => ("insert", at, at + 1),
                    };

                    if d.polygon_point(sel, op, index, cur, 0).is_ok() {
                        self.poly_point = Some(new_point);
                        self.status = "point inserted".to_owned();
                    }

                    return true;
                }

                false
            }

            ViewportEventKind::Drag => {
                if self.stroke.is_some() {
                    self.on_hover(e, ev);
                    if self.terrain_hit {
                        self.dab(e);
                    }

                    return true;
                }

                if self.stroke_active {
                    self.on_hover(e, ev);
                    let dt = e.clock.dt();
                    let d = e.docs.find_as_mut::<TileDocument>(id).expect("a tile");
                    self.turn_image_brush();
                    if let (true, Some(kind)) = (self.terrain_hit, self.brush_kind()) {
                        d.brush_step(kind, self.cursor.x, self.cursor.z, self.brush.clone(), dt);
                    }
                } else if let Some(shelter) = self.target_drag {
                    self.on_hover(e, ev);
                    let d = e.docs.find_as_mut::<TileDocument>(id).expect("a tile");
                    if self.terrain_hit {
                        d.set_shelter_target(shelter, self.cursor, self.target_gesture);
                    }
                } else if let Some(light) = self.light_drag {
                    // The radius: how far from the light, across, the mouse ray meets the light's horizontal plane.
                    let d = e.docs.find_as_mut::<TileDocument>(id).expect("a tile");
                    let c = d.tile().world_position(light);
                    if let Some(at) = ray.and_then(|(o, dir)| light_plane_distance(c, o, dir)) {
                        d.set_light_radius(light, at + self.light_grab, self.light_gesture);
                    }
                } else if self.point_drag {
                    self.on_hover(e, ev);
                    let sel = primary(e);
                    let d = e.docs.find_as_mut::<TileDocument>(id).expect("a tile");
                    if let (true, Some(sel), Some(pt)) = (self.terrain_hit, sel, self.poly_point) {
                        let sc = d.tile().tscene();
                        let pts = sc.polygon_points(sel);
                        if let Some(out) = self.spline_handle {
                            let _ = d.spline_point(sel, if out { "out" } else { "in" }, pt, self.cursor, self.point_gesture);
                        } else if let Some(&point) = pts.get(pt) {
                            let before = sc.point_position(point).unwrap_or_default();
                            let after = d.tile().to_local(sel, self.cursor);
                            if before != after {
                                let _ = d.polygon_point(sel, "move", pt, self.cursor, self.point_gesture);
                            }
                        }
                    }
                }

                self.stroke_active || self.point_drag || self.light_drag.is_some() || self.target_drag.is_some()
            }

            ViewportEventKind::Release => {
                if self.stroke.is_some() {
                    self.status = self.end_stroke_veg(e);
                    return true;
                }

                let d = e.docs.find_as_mut::<TileDocument>(id).expect("a tile");
                if self.stroke_active {
                    self.stroke_active = false;
                    d.end_stroke();
                }

                if self.point_drag || self.light_drag.is_some() || self.target_drag.is_some() {
                    self.point_drag = false;
                    self.light_drag = None;
                    self.target_drag = None;
                    d.history_mut().break_merge();
                }

                true
            }

            ViewportEventKind::Click => {
                // Select: the model piece under the cursor (exact), else the nearest instance sphere along the ray.
                let d = e.docs.find_as::<TileDocument>(id).expect("a tile");
                let mut hit = (ev.hit_source.doc == id && ev.hit_source.item != TERRAIN).then(|| elem_of(ev.hit_source.item)).flatten();
                if hit.is_none()
                    && let Some((o, dir)) = ray
                {
                    hit = d.tile().pick_instance(o, dir, true);
                }

                let Some(hit) = hit else {
                    if !ev.ctrl {
                        e.selection.clear();
                    }

                    self.poly_point = None;
                    return true;
                };

                let r = d.ref_of(hit);
                if ev.ctrl {
                    e.selection.toggle(r);
                } else if ev.shift {
                    e.selection.add(r);
                } else {
                    e.selection.set(r);
                }

                e.docs.set_active(id);
                self.poly_point = None;
                true
            }
        }
    }

    fn viewport_overlay(&mut self, e: &mut Editor, origin: gui::Vec2, size: gui::Vec2) {
        if document(e).is_none() || size.x < 200.0 {
            return;
        }

        // The weather chips at the top left of the image, beside the compass (under it when the image is too narrow for both).
        let compass_left = origin.x + size.x * 0.5 - (size.x - 16.0).min(360.0) * 0.5;
        let beside = !e.viewport.show_compass || origin.x + 8.0 + self.chips_width + 8.0 <= compass_left;
        gui::set_cursor_screen_pos(gui::v2(origin.x + 8.0, origin.y + if beside { 6.0 } else { 58.0 }));
        gui::push_style_color(gui::COL_CHILD_BG, gui::v4(0.08, 0.08, 0.09, 0.78));
        if gui::begin_child("##weatherchips", gui::v2(0.0, 0.0), gui::CHILD_AUTO_RESIZE, gui::WINDOW_NO_SCROLLBAR) {
            self.weather.draw_chips(e);
        }

        gui::end_child();
        let (lo, hi) = gui::item_rect();
        self.chips_width = hi.x - lo.x;
        gui::pop_style_color(1);

        // The placement chips at the top right, beside the compass too.
        let compass_right = origin.x + size.x * 0.5 + (size.x - 16.0).min(360.0) * 0.5;
        let beside = !e.viewport.show_compass || origin.x + size.x - 8.0 - self.place_chips_width - 8.0 >= compass_right;
        self.draw_placement_chips(e, origin, size, if beside { 6.0 } else { 58.0 });
        self.draw_box_select(origin);
        self.draw_context_menu(e);
    }

    fn inspect(&mut self, e: &mut Editor, ins: &mut dyn wce_editor_core::inspector::Inspector) -> bool {
        // The brush's settings at the top while it paints trees; then the new vegetation set (not saved), placing or not.
        if self.painting_trees(e) {
            self.inspect_veg_brush(e, ins);
        }

        self.inspect_new_set(e);

        self.inspect_making(e, ins) || self.inspect_brush(e, ins)
    }

    fn toolbar(&mut self, e: &mut Editor) {
        self.draw_toolbar(e);
    }

    fn unsaved_work(&self) -> Vec<String> {
        self.scratch_set.iter().filter(|s| !s.entries.is_empty()).map(|s| format!("the new vegetation set ({} kind(s) of tree; Tile tool, the Inspector)", s.entries.len())).collect()
    }

    fn edit_menu(&mut self, e: &mut Editor) {
        if document_id(e).is_some() {
            gui::separator();
            self.selection_items(e);
        }
    }

    fn claims_left_button(&self, _e: &Editor) -> bool {
        self.mode != TileMode::Select || self.drawing || self.grabbing || self.placing.is_some() || self.box_select.armed
    }

    fn open(&mut self, e: &mut Editor, path: &str, from_packs: bool) -> Result<Box<dyn Document>, String> {
        if from_packs {
            return Err("tiles are opened from the assembly kit folders on disk".to_owned());
        }

        let dir = tile_dir_of(path);

        // Opened by its database entry: the entry file is the root, the folder is what it names.
        let mut project = match self.pending_entry.take() {
            Some(entry) => TileProject::open_entry(&entry, self.pending_variation.take()),
            None => TileProject::open(&dir),
        }
        .map_err(|err| err.to_string())?;

        // A kit's tile with a project (saved before) opens as the project, unless the kit's copy is asked for (Import from Assembly Kit).
        let projects = Self::workspace_dir(e);
        let kit_copy = std::mem::take(&mut self.open_kit_copy);
        if !kit_copy && !project.dir.starts_with(&projects)
            && let Some(folder) = wce_tile_editor::tile_document::kit::tile_folder(&project.dir)
        {
            let mirror = projects.join("raw_data").join(&folder);
            if wce_tile::tile_project::has_tile(&mirror) {
                project = TileProject::open(&mirror).map_err(|err| err.to_string())?;
                self.status = format!("opened the project of {folder} (its tile.wce); Tile > Import from Assembly Kit takes the kit's copy again");
                log::info(self.status.clone());
            }
        }

        if !project.warning.is_empty() {
            log::warn(project.warning.clone());
            self.status = project.warning.clone();
        }

        // The prefabs placed as references get their contents (and their overrides); a save keeps what they differ by.
        let lib = self.prefab_library(e);
        let missing = project.fill_prefab_references(&|k| lib.contents(k)).map_err(|err| err.to_string())?;
        if !missing.is_empty() {
            log::warn(format!("no prefab {}: their references are kept empty", missing.join(", ")));
        }

        project.prefab_base = Some(self.prefab_source());

        // From the Assembly Kit: the groups that are exact copies of a prefab become references to it (Tile > Prefabs does it again).
        if !project.dir.starts_with(&projects) {
            let index = self.content_index(e);
            let found = wce_tile::prefab_refs::recognise(&project.scene, &index, &|n| lib.get(n).is_some());
            if !found.copies.is_empty() || found.named_but_different != 0 {
                project.make_references(&found.copies).map_err(|err| err.to_string())?;
                log::info(crate::prefab_panel::recognised_message(found.copies.len(), found.named_but_different));
            }
        }

        let mut doc = TileDocument::new(project, e.files());
        doc.set_projects_root(Some(projects));
        if let Some(w) = doc.exclusion_warning() {
            log::error(format!("{}: {w}", doc.title()));
        }

        let settings = e.tool_settings::<TileSettings>(SETTINGS_ID);
        doc.tile_mut().display_mut().show_sea = settings.show_sea;
        if !settings.show_vfx_boxes {
            doc.tile_mut().display_mut().hidden_types.insert(VFX_TYPE.to_owned());
        }
        Ok(Box::new(doc))
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}

/// rpfm's Attila schema, for the procedural walls' database rows: `WCE_RPFM_SCHEMA`, else the Tile settings' `rpfm_schema`, else rpfm's own.
///
/// # Errors
///
/// None is found, or it doesn't read.
fn walls_schema(e: &Editor) -> Result<rpfm_lib::schema::Schema, String> {
    let path = walls_schema_path(e)?;
    rpfm_lib::schema::Schema::load(&path, None).map_err(|err| format!("{}: {err}", path.display()))
}

/// Where rpfm's Attila schema for the walls' database rows is: `WCE_RPFM_SCHEMA`, the settings, rpfm's own.
fn walls_schema_path(e: &Editor) -> Result<PathBuf, String> {
    let configured = e.tool_settings::<TileSettings>(SETTINGS_ID).rpfm_schema;
    wce_foundation::settings::env_value("WCE_RPFM_SCHEMA").map(PathBuf::from).or(configured)
        .or_else(wce_tile_io::attila::game_data::default_schema_path)
        .ok_or_else(|| "the walls' database rows need rpfm's Attila schema: set rpfm_schema in the [tools.tile] settings (a schema_att.ron rpfm_lib reads)".to_owned())
}

/// A wall's layout in one line: base, length, corners, runs, steps, attached ends (scene units).
pub fn wall_summary(l: &WallLayout) -> String {
    let ends = l.ends.iter().map(|a| a.as_ref().map_or("free".to_owned(), |a| format!("on {}", a.key))).collect::<Vec<_>>();
    format!(
        "{} wall on {}: {:.0} long, {} corner(s), {} run(s), {} step section(s); ends {} / {}",
        if l.profile.walkway > 10.0 { "15 m" } else { "8 m" }, l.profile.key, l.plan.length, l.plan.corners.len(), l.plan.runs.len(), l.plan.steps.len(), ends[0], ends[1],
    )
}

/// How far from a light, across, a ray meets the light's horizontal plane (`None` when it does not, ahead).
fn light_plane_distance(c: Float3, o: Float3, dir: Float3) -> Option<f32> {
    if dir.y.abs() < 1e-4 {
        return None;
    }

    let t = (c.y - o.y) / dir.y;
    (t > 0.0).then(|| {
        let p = o + dir * t;
        (p.x - c.x).hypot(p.z - c.z)
    })
}
