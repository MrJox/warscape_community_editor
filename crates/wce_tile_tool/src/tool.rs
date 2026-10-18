//! The tool: documents, the mouse and the keys in the viewport, the per-frame update, and the Tile and Tiles panels.

use std::any::Any;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use rpfm_lib::files::asset_variation_db::AssetVariationDb;
use wce_editor::editor::Editor;
use wce_editor::gui;
use wce_editor::steam_detect::file_dialog;
use wce_editor::tool::{Tool, ViewportEvent, ViewportEventKind};
use wce_editor_core::command_registry::{CommandRegistry, CommandResult};
use wce_editor_core::document::Document;
use wce_formats::tile_entry::TileEntry;
use wce_formats::tile_index::TileIndex;
use wce_foundation::log;
use wce_foundation::math::{Float3, dot, length};
use wce_render::model_cache::State as ModelState;
use wce_scene::render_scene::{DocumentId, Overlay, SourceRef, overlay_colour};
use wce_tile::tile_project::{TileProject, kit_raw_root};
use wce_tile_editor::terrain_bake::TerrainView;
use wce_tile_editor::tile_document::{BrushKind, BrushSettings, OffTerrain, PieceState, PieceStatus, Placement, TERRAIN, TileDocument, elem_of};
use wce_vfs::normalize;

pub const POLYGON_TYPES: [&str; 5] =
    ["AI_HINT_POLYGON_INSTANCE", "NOGO_REGION_POLYGON_INSTANCE", "GO_REGION_POLYGON_INSTANCE", "VegetationExclusionPolygonInstance", "SIEGE_AI_POLYGON_INSTANCE"];

pub const HINTS: [&str; 16] = [
    "AIH_FORT_APPROACH", "AIH_DEFENSIVE_HILL", "AIH_FORT_PERIMETER", "AIH_AMBUSH_CAVALRY_LINE", "AIH_AMBUSH_MELEE_LINE", "AIH_AMBUSH_MISSILE_LINE",
    "AIH_AMBUSH_ASSAULT_AREA", "AIH_AMBUSH_DEFENDER_INITIAL_PATH", "AIH_AMBUSH_FOREST", "AIH_AMBUSH_FIREBALL_LINE", "AIH_BEACH_LANDING",
    "AIH_DEFAULT_DEPLOYMENT_LINE", "AIH_RIVER_EDGE", "AIH_BRIDGE_CROSSING", "AIH_COVER_LINE", "AIH_BEACH_LANDING_FORT_APPROACH",
];

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
}

pub const MODE_NAMES: [&str; 6] = ["select", "raise", "lower", "smooth", "flatten", "level"];
const MODES: [TileMode; 6] = [TileMode::Select, TileMode::Raise, TileMode::Lower, TileMode::Smooth, TileMode::Flatten, TileMode::Level];

impl TileMode {
    pub fn from_index(i: usize) -> Self {
        MODES[i.min(5)]
    }

    pub fn index(self) -> usize {
        MODES.iter().position(|&m| m == self).unwrap_or(0)
    }

    /// The brush of a terrain mode.
    fn brush(self) -> Option<BrushKind> {
        match self {
            Self::Select => None,
            Self::Raise => Some(BrushKind::Raise),
            Self::Lower => Some(BrushKind::Lower),
            Self::Smooth => Some(BrushKind::Smooth),
            Self::Flatten => Some(BrushKind::Flatten),
            Self::Level => Some(BrushKind::Level),
        }
    }
}

const WARN: [f32; 4] = [1.0, 0.7, 0.3, 1.0];
const BAD: [f32; 4] = [1.0, 0.45, 0.4, 1.0];

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

pub struct TileTool {
    pub status: String,
    pub(crate) mode: TileMode,
    pub(crate) brush: BrushSettings,
    pub(crate) snap_grid: bool,
    pub(crate) grid_m: f32,
    angle_step: f32,
    building_key: String,

    // Placement: the mode of new buildings, the off-terrain report of the Tile panel.
    new_placement: Placement,
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

    // Gestures.
    stroke_active: bool,
    pub(crate) grabbing: bool,
    pub(crate) grab_elem: Option<usize>,
    pub(crate) grab_y_offset: f64,
    grab_history: usize,
    pub(crate) grab_gesture: u64,
    poly_point: Option<usize>,
    point_drag: bool,
    point_gesture: u64,
    pub(crate) drawing: bool,
    draw_pts: Vec<Float3>,
    pub(crate) new_poly_type: usize,
    pub(crate) new_hint: usize,
    camera_set_for: DocumentId,

    /// mark_height values for the tests.
    pub(crate) marks: HashMap<String, f32>,

    // The tile browser (Tiles panel), and the entry a pending open comes from (consumed by `open`).
    browse_filter: String,
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

    // Weather picker (data only: which .environment file the game picks for this map).
    env: Option<AssetVariationDb>,
    wq: BTreeMap<String, String>,
    env_status: String,
}

impl Default for TileTool {
    fn default() -> Self {
        Self {
            status: String::new(), mode: TileMode::Select, brush: BrushSettings::default(), snap_grid: false, grid_m: 6.0, angle_step: 15.0,
            building_key: String::new(), new_placement: Placement::Landscape, off_tol: 0.5, off_list: vec![], off_doc_version: None, off_doc: 0,
            rename_buf: String::new(), tree_rows: vec![], tree_open: HashSet::new(), tree_doc: 0, tree_version: None, tree_open_version: 0,
            tree_open_version_seen: None, tree_filter: String::new(), tree_filter_seen: String::new(), tree_synced: None, tree_scroll_to: None, rename_elem: None, anchor: None, rename_wait: 0, rename_focus: false, rename_active_seen: false,
            begin_rename_next: false, delete_confirm: false, delete_weight: 0, terrain_hit: false, cursor: Float3::default(), stroke_active: false,
            grabbing: false, grab_elem: None, grab_y_offset: 0.0, grab_history: 0, grab_gesture: 0, poly_point: None, point_drag: false, point_gesture: 0,
            drawing: false, draw_pts: vec![], new_poly_type: 0, new_hint: 0, camera_set_for: 0, marks: HashMap::new(), browse_filter: String::new(),
            browse_kit: 0, browse_kind: 0, browse_selected: String::new(), pending_entry: None, pending_variation: None, browse_roots: vec![],
            browse_roots_time: -1e9, browse_rows: vec![], browse_rows_key: String::new(), browse_index: None, env: None,
            wq: BTreeMap::from([("map".to_owned(), "default_map".to_owned())]), env_status: String::new(),
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
    e.selection.items().iter().filter(|r| r.doc == id && r.item != TERRAIN).filter_map(|r| elem_of(r.item)).collect()
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

/// Runs `f` on the tile tool (a command reaching its tool).
pub(crate) fn with_tile(e: &mut Editor, f: impl FnOnce(&mut TileTool, &mut Editor) -> CommandResult) -> CommandResult {
    e.with_tool::<TileTool, _>(f).unwrap_or_else(|| CommandResult::fail("the tile tool is not available"))
}

impl TileTool {
    pub fn mode(&self) -> TileMode {
        self.mode
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

    /// The raw_data folders the tile browser lists: `tile_kits` in wce.ini (';' separated), else the assembly kits under game_dir.
    pub fn kit_roots(&self, e: &mut Editor) -> Vec<PathBuf> {
        // The index of the kits' tile database entries keeps its rows in the settings folder between runs.
        if let Some(dir) = e.settings().file().parent() {
            TileIndex::set_cache_dir(&dir.join("tile_index"));
        }

        if let Some(kits) = e.settings().get("tile_kits", Some("WCE_TILE_KITS")) {
            return kits.split(';').filter(|k| !k.is_empty()).map(PathBuf::from).collect();
        }

        let mut roots = vec![];
        if let Some(game) = e.settings().get("game_dir", Some("WCE_GAME")) {
            for kit in ["assembly_kit_battle_tilemap", "assembly_kit_campaign_tilemap"] {
                let raw = Path::new(&game).join(kit).join("raw_data");
                if raw.join("terrain").join("tiles").is_dir() {
                    roots.push(raw);
                }
            }
        }

        roots
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
        } else if p.is_dir() && !p.join("tile.ted").exists() {
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

        let ted = entry.ted_path(&root, v);
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

    /// Opens a tile folder (its tile.ted) or a tile.ted.
    ///
    /// # Errors
    ///
    /// The tile can't be opened.
    pub fn open_folder(&mut self, e: &mut Editor, dir: &str) -> Result<String, String> {
        let tile = Path::new(dir);
        let path = if tile.is_dir() { tile.join("tile.ted") } else { tile.to_path_buf() };
        e.open_path_from(self, &path.display().to_string(), false, true)
    }

    /// The kit's environment_map.xml: which environment file the game picks for this tile's map.
    fn load_environment_map(&mut self, tile_dir: &Path) {
        self.env = None;
        let g = tile_dir.display().to_string().replace('\\', "/");
        let Some(at) = g.find("/raw_data/") else {
            self.env_status = "no raw_data folder in the tile path".to_owned();
            return;
        };

        let p = Path::new(&g[..at + 10]).join("warscape_asset_variation_db").join("environment_map.xml");
        let loaded = std::fs::read(&p)
            .map_err(|_| format!("cannot read {}", p.display()))
            .and_then(|b| AssetVariationDb::from_environment_map(&String::from_utf8_lossy(&b)).map_err(|err| err.to_string()));
        match loaded {
            Ok(db) => {
                self.env_status = format!("{} environment files", db.entries().first().map_or(0, |entry| entry.variations().len()));
                self.env = Some(db);
            }

            Err(err) => self.env_status = format!("environment_map.xml: {err}"),
        }
    }

    fn handle_size(e: &Editor) -> f32 {
        let c = e.viewport.camera();
        (length(c.eye - c.target) * 0.012).max(0.8)
    }

    fn tool_overlay(&self, e: &Editor, d: &TileDocument) -> Overlay {
        let mut o = Overlay { lines: vec![], triangles: vec![], depth_test: false, pickable: false };
        let t = d.tile();
        let gold = overlay_colour(1.0, 0.85, 0.2, 1.0);
        if self.mode != TileMode::Select && self.terrain_hit && e.viewport.hovered {
            let n = 64;
            let mut prev = Float3::default();
            for i in 0..=n {
                let a = i as f32 * std::f32::consts::TAU / n as f32;
                let mut p = Float3::new(self.cursor.x + a.cos() * self.brush.radius_m, 0.0, self.cursor.z + a.sin() * self.brush.radius_m);
                p.y = t.terrain_height(p.x, p.z) + 0.4;
                if i != 0 {
                    o.line(prev, p, gold);
                }

                prev = p;
            }
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
        self.terrain_hit = false;
        let Some((o, dir)) = e.viewport.ray(ev.x, ev.y) else { return };
        let Some(d) = document(e) else { return };
        if let Some(hit) = d.tile().pick_terrain(o, dir) {
            self.terrain_hit = true;
            self.cursor = hit;
        }

        if let (true, true, Some(g)) = (self.grabbing, self.terrain_hit, self.grab_elem) {
            let (mut gx, mut gz) = (self.cursor.x, self.cursor.z);
            if self.snap_grid && self.grid_m > 0.01 {
                gx = (gx / self.grid_m).round() * self.grid_m;
                gz = (gz / self.grid_m).round() * self.grid_m;
            }

            let gy = d.tile().terrain_height(gx, gz) + self.grab_y_offset as f32;
            d.set_world_metres(g, Float3::new(gx, gy, gz), self.grab_gesture);
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
        self.grab_history = d.history().position();
        self.grab_gesture = d.next_gesture();
        self.grabbing = true;
        self.status = "grab: move the mouse over the ground, click to confirm, Esc / right click to cancel".to_owned();
        true
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

    pub fn finish_polygon(&mut self, e: &mut Editor) {
        if !self.drawing {
            return;
        }

        let sel = primary(e);
        let Some(d) = document(e) else { return };
        self.drawing = false;
        let layer = match sel {
            Some(s) => d.tile().layer_of(s),
            None => d.tile().tscene().scene_graph(),
        };

        let added = d.add_polygon(layer, POLYGON_TYPES[self.new_poly_type], HINTS[self.new_hint], &self.draw_pts);
        self.draw_pts.clear();
        self.poly_point = None;
        self.status = match added {
            Ok(el) => format!("added {} in {}", POLYGON_TYPES[self.new_poly_type], d.tile().tscene().layer_path(el)),
            Err(err) => err,
        };
    }

    fn handle_keys(&mut self, e: &mut Editor) {
        let io = gui::io();
        if document_id(e).is_none() || io.WantTextInput {
            return;
        }

        let sel = primary(e);
        if gui::is_key_pressed(gui::KEY_ESCAPE, false) {
            if self.grabbing {
                self.end_grab(e, false);
            } else if self.drawing {
                self.drawing = false;
                self.draw_pts.clear();
            } else if self.mode != TileMode::Select {
                self.mode = TileMode::Select;
            }
        }

        if self.grabbing && gui::is_mouse_clicked(1) {
            self.end_grab(e, false);
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
                match document(e).expect("a tile").group_items(&all, "New layer") {
                    Ok(_) => {
                        self.status = "grouped into a new layer".to_owned();
                        self.begin_rename_next = true;
                    }

                    Err(err) => self.status = err,
                }
            }

            return;
        }

        if gui::is_key_pressed(gui::KEY_G, false) && sel.is_some() {
            self.begin_grab(e);
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
            self.finish_polygon(e);
        }
    }

    // ---- panels

    fn draw_tile_panel(&mut self, e: &mut Editor) {
        gui::set_next_window_size(gui::v2(460.0, 640.0), gui::COND_FIRST_USE_EVER);
        if !gui::begin("Tile", None, 0) {
            gui::end();
            return;
        }

        let Some(id) = document_id(e) else {
            gui::text_wrapped("No tile open. File > Open file... (a tile.ted), Tile > Open tile..., or --tile <folder>.");
            gui::end();
            return;
        };

        let sel = primary(e);
        let all = selected(e);
        {
            let d = e.docs.find_as_mut::<TileDocument>(id).expect("a tile");
            let t = d.tile();
            let p = d.project();
            let file_name = |p: &Path| p.file_name().map(|f| f.to_string_lossy().into_owned()).unwrap_or_default();
            gui::text(&file_name(&p.dir));
            gui::text_disabled(&p.dir.parent().map(file_name).unwrap_or_default());
            if !p.warning.is_empty() {
                gui::text_colored(WARN, &p.warning);
            }

            gui::separator();
            let names = ["Select", "Raise", "Lower", "Smooth", "Flatten", "Level"];
            for (i, name) in names.iter().enumerate() {
                if i != 0 {
                    gui::same_line();
                }

                if gui::radio_button(name, self.mode == TileMode::from_index(i)) {
                    self.set_mode(TileMode::from_index(i));
                }
            }

            gui::slider_float("radius (m)", &mut self.brush.radius_m, 1.0, 120.0, "%.3f");
            gui::slider_float("strength", &mut self.brush.strength, 0.5, 30.0, "%.3f");
            gui::slider_float("falloff", &mut self.brush.falloff, 0.0, 1.0, "%.3f");
            gui::drag_float("flatten height (m)", &mut self.brush.flatten_height, 0.1, 0.0, 0.0, "%.3f");
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
                gui::text_colored(WARN, &format!("{} building keys have no model (boxes), see the log", t.model_fallbacks().len()));
            }

            gui::separator();
        }

        self.draw_entry_section(e, id);
        gui::separator();
        gui::checkbox("snap to grid", &mut self.snap_grid);
        gui::same_line();
        gui::set_next_item_width(90.0);
        gui::drag_float("grid (m)", &mut self.grid_m, 0.5, 0.5, 64.0, "%.3f");
        gui::set_next_item_width(90.0);
        gui::drag_float("rotate step (deg)", &mut self.angle_step, 1.0, 1.0, 180.0, "%.3f");
        gui::input_text("building key", None, &mut self.building_key, 128, 0);
        gui::same_line();
        let mut np = i32::from(self.new_placement == Placement::Freeform);
        gui::set_next_item_width(120.0);
        if gui::combo("##new_placement", &mut np, &["Landscape", "Freeform"]) {
            self.new_placement = if np == 1 { Placement::Freeform } else { Placement::Landscape };
        }

        if gui::is_item_hovered() {
            gui::set_tooltip("placement of a new building: landscape stands on the terrain, freeform keeps the height it is given");
        }

        gui::same_line();
        let d = e.docs.find_as_mut::<TileDocument>(id).expect("a tile");
        if gui::button("Add") {
            let at = self.terrain_hit.then_some(self.cursor);
            self.status = match d.add_building(&self.building_key, sel, at, self.new_placement) {
                Ok(_) => format!("added {}", self.building_key),
                Err(err) => err,
            };
        }

        gui::begin_disabled(sel.is_none());
        let grab = gui::button("Grab on ground (G)");
        gui::same_line();

        // Landscape instances stand on it by themselves; a freeform one is put on it (its mode stays).
        if gui::button("Drop to terrain") && sel.is_some() {
            let n = d.snap_to_terrain(&all, true);
            self.status = if n != 0 { format!("on the terrain: {n} item(s)") } else { "already on the terrain".to_owned() };
        }

        if gui::button("Duplicate (Ctrl+D)")
            && let Some(s) = sel
            && let Err(err) = d.duplicate(s)
        {
            self.status = err;
        }

        gui::same_line();
        if gui::button("Delete (Del)")
            && let Some(s) = sel
        {
            d.remove(s);
        }

        gui::same_line();
        if gui::button("Rotate (Ctrl+R)")
            && let Some(s) = sel
        {
            d.rotate(s, self.angle_step);
        }

        gui::end_disabled();
        if grab {
            self.begin_grab(e);
        }

        gui::separator();
        self.draw_placement_section(e, id);
        gui::separator();
        self.draw_ground_section(e, id);
        gui::separator();
        self.draw_links_section(e, id);
        gui::separator();
        self.draw_polygon_panel(e, id);
        gui::separator();
        self.draw_weather_panel(e);
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

            let seasons = ["su", "au", "wi"];
            let mut si = match look.season.as_str() {
                "au" => 1,
                "wi" => 2,
                _ => 0,
            };

            gui::set_next_item_width(80.0);
            if gui::combo("season##ground", &mut si, &["summer", "autumn", "winter"]) {
                look.season = seasons[si as usize].to_owned();
                changed = true;
            }
        }

        if look.blended {
            let views = [TerrainView::Colour, TerrainView::BlendGroups, TerrainView::GroundTypes, TerrainView::Protection];
            let mut vi = views.iter().position(|&v| v == look.view).unwrap_or(0) as i32;
            gui::set_next_item_width(160.0);
            if gui::combo("view##ground", &mut vi, &["colour", "blend groups", "ground types", "protection"]) {
                look.view = views[vi as usize];
                changed = true;
            }
        }

        if changed {
            d.tile_mut().set_terrain_look(&mut e.scene, look);
        }

        let d = e.docs.find_as_mut::<TileDocument>(id).expect("a tile");
        gui::text_wrapped(d.tile().terrain_note());
        let mut water = d.tile().display().show_water;
        if gui::checkbox("water (rivers, sea / lake)", &mut water) {
            d.tile_mut().display_mut().show_water = water;
            d.tile_mut().display_changed();
        }

        gui::text_disabled(d.tile().water_note());
        for f in &shader_failures {
            gui::text_colored(BAD, &format!("game shader not loaded: {f}"));
        }
    }

    /// Tile links (read-only): the entry's link targets and rules, and a viewport view of the cells they name. Editing them is a later step
    /// (they are part of the database entry, saved to TILES/<tile>.xml and tile_database.bin).
    fn draw_links_section(&mut self, e: &mut Editor, id: DocumentId) {
        if !gui::collapsing_header("Tile links", 0) {
            return;
        }

        let fly = e.viewport.is_fly();
        let d = e.docs.find_as_mut::<TileDocument>(id).expect("a tile");
        let cells = d.tile().link_cells();
        if cells.is_empty() {
            gui::text_disabled(if d.project().has_entry() { "this tile has no links or link targets" } else { "no tile database entry: no links" });
            return;
        }

        let mut show = d.tile().display().show_links;
        if gui::checkbox("show on the ground", &mut show) {
            d.tile_mut().display_mut().show_links = show;
            d.tile_mut().display_changed();
        }

        let targets = cells.iter().filter(|c| c.target).count();
        gui::text(&format!("{targets} link target(s), {} rule(s)", cells.len() - targets));
        let mut look_at = None;
        for c in &cells {
            gui::bullet_text(&format!("{}{}", if c.target { "target: " } else if c.equals { "= " } else { "!= " }, c.text));

            // Look at the cell.
            if gui::is_item_clicked() {
                let (x0, z0, x1, z1) = d.tile().cell_rect(c.x, c.y);
                let (cx, cz) = ((x0 + x1) * 0.5, (z0 + z1) * 0.5);
                look_at = Some(Float3::new(cx, d.tile().terrain_height(cx, cz), cz));
            }
        }

        if let (Some(t), false) = (look_at, fly) {
            e.viewport.orbit_mut().set_orbit(0.8, 0.9, 350.0, t);
        }
    }

    /// Placement (docs/formats/tile_ted.md): landscape instances stand on the terrain, freeform ones keep any height. The buttons act on the
    /// selection (layers: everything below); the report lists the landscape instances that are off the terrain.
    fn draw_placement_section(&mut self, e: &mut Editor, id: DocumentId) {
        if !gui::collapsing_header("Placement", 0) {
            return;
        }

        let sel = selected(e);
        let d = e.docs.find_as_mut::<TileDocument>(id).expect("a tile");
        let c = d.tile().placement_counts();
        gui::text(&format!("{} landscape, {} freeform, {} without a mode", c.landscape, c.freeform, c.other));
        gui::begin_disabled(sel.is_empty());
        if gui::button("Landscape") {
            let n = d.set_placement(&sel, Placement::Landscape);
            self.status = format!("{n} set to landscape (snapped to the terrain)");
        }

        gui::same_line();
        if gui::button("Freeform") {
            let n = d.set_placement(&sel, Placement::Freeform);
            self.status = format!("{n} set to freeform");
        }

        gui::same_line();
        if gui::button("Snap to terrain") {
            let n = d.snap_to_terrain(&sel, false);
            self.status = format!("{n} landscape item(s) snapped");
        }

        gui::end_disabled();
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

    /// The tile browser: the entries of the kit's tile database (an index, not a scan), a swatch of the tile's colour, its size and
    /// variations. A double-click opens the first variation, the buttons under the list open one.
    fn draw_browser_panel(&mut self, e: &mut Editor) {
        gui::set_next_window_size(gui::v2(520.0, 340.0), gui::COND_FIRST_USE_EVER);
        if !gui::begin("Tiles", None, 0) {
            gui::end();
            return;
        }

        if gui::time() - self.browse_roots_time > 2.0 {
            self.browse_roots = self.kit_roots(e);
            self.browse_roots_time = gui::time();
        }

        if self.browse_roots.is_empty() {
            gui::text_wrapped("No assembly kit found. Set game_dir (the folder with assembly_kit_battle_tilemap) or tile_kits in wce.ini.");
            gui::end();
            return;
        }

        let kit_label = |r: &Path| r.parent().and_then(Path::file_name).map(|f| f.to_string_lossy().into_owned()).unwrap_or_default();
        self.browse_kit = self.browse_kit.min(self.browse_roots.len() - 1);
        gui::set_next_item_width(220.0);
        if gui::begin_combo("kit", &kit_label(&self.browse_roots[self.browse_kit])) {
            for (i, r) in self.browse_roots.iter().enumerate() {
                if gui::selectable(&kit_label(r), i == self.browse_kit, 0) {
                    self.browse_kit = i;
                }
            }

            gui::end_combo();
        }

        gui::same_line();
        gui::set_next_item_width(110.0);
        gui::combo("##kind", &mut self.browse_kind, &["all kinds", "battle", "campaign"]);
        gui::set_next_item_width(-1.0);
        gui::input_text("##tilefilter", Some("filter: set, tile, variation (words are and-ed)"), &mut self.browse_filter, 96, 0);
        let index = TileIndex::shared(&self.browse_roots[self.browse_kit]);
        let key = format!("{}|{}|{}|{:p}", self.browse_kit, self.browse_kind, self.browse_filter, Arc::as_ptr(&index));
        if key != self.browse_rows_key {
            self.browse_rows_key = key;
            let words = self.browse_filter.split(' ').filter(|w| !w.is_empty()).map(str::to_lowercase).collect::<Vec<_>>();
            self.browse_rows = index
                .entries()
                .iter()
                .enumerate()
                .filter(|(_, en)| match self.browse_kind {
                    1 => en.kind == "battle",
                    2 => en.kind == "campaign",
                    _ => true,
                })
                .filter(|(_, en)| matches_words(en, &words))
                .map(|(i, _)| i)
                .collect();
            self.browse_index = Some(index.clone());
        }

        gui::text_disabled(&format!("{} of {} tiles", self.browse_rows.len(), index.entries().len()));
        let footer = gui::frame_height_with_spacing() * 4.5;
        let mut open = None;
        let entries = self.browse_index.as_ref().map_or(&[][..], |i| i.entries());
        if gui::begin_child("tilelist", gui::v2(0.0, -footer), gui::CHILD_BORDERS, 0) {
            let rows = &self.browse_rows;
            let selected_key = &mut self.browse_selected;
            gui::clipped(rows.len(), |i| {
                let en = &entries[rows[i]];
                gui::push_id_int(i as i32);
                gui::color_swatch("##c", [en.red / 255.0, en.green / 255.0, en.blue / 255.0, 1.0], gui::v2(12.0, 12.0));
                gui::same_line();
                let prefix = format!("terrain/tiles/{}/", en.kind);
                let shown = en.key.strip_prefix(&prefix).unwrap_or(&en.key);
                let vars = if en.variations.len() > 1 { format!("  +{}v", en.variations.len()) } else { String::new() };
                if gui::selectable(&format!("{shown}  {}x{}{vars}", en.width, en.height), *selected_key == en.key, gui::SELECTABLE_ALLOW_DOUBLE_CLICK) {
                    *selected_key = en.key.clone();
                    if gui::is_mouse_double_clicked(0) {
                        open = Some((en.xml.display().to_string(), String::new()));
                    }
                }

                gui::pop_id();
            });
        }

        gui::end_child();
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
                        open = Some((sel.xml.display().to_string(), v.to_string()));
                    }
                }
            }

            None => gui::text_disabled("Select a tile; double-click opens its first variation."),
        }

        gui::end();
        if let Some((xml, variation)) = open {
            let msg = self.open_tile(e, &xml, &variation).unwrap_or_else(|m| format!("open failed: {m}"));
            e.status(&msg, false);
        }
    }

    /// The tile's database entry, read-only: where it is, what the tile is, the ground groups, the variation's height / normal values, the
    /// links.
    fn draw_entry_section(&mut self, e: &mut Editor, id: DocumentId) {
        if !gui::collapsing_header("Tile database entry", gui::TREE_DEFAULT_OPEN) {
            return;
        }

        let d = e.docs.find_as::<TileDocument>(id).expect("a tile");
        let p = d.project();
        let Some(en) = &p.entry else {
            gui::text_colored(WARN, "This tile has no database entry (it is not inside an assembly kit's raw_data/terrain/tiles/<kind>).");
            return;
        };

        let vars = en.variations();
        let current = p.variation;
        gui::text_disabled(&en.file().file_name().map(|f| f.to_string_lossy().into_owned()).unwrap_or_default());
        if gui::is_item_hovered() {
            gui::set_tooltip(&en.file().display().to_string());
        }

        gui::text(&en.location_generic());
        gui::color_swatch("##tilecolour", [en.red() as f32 / 255.0, en.green() as f32 / 255.0, en.blue() as f32 / 255.0, 1.0], gui::v2(14.0, 14.0));
        gui::same_line();
        gui::text(&format!("colour {:.0} {:.0} {:.0}   set {}", en.red(), en.green(), en.blue(), en.tile_set()));
        gui::text(&format!(
            "{} x {} cells = {} x {} m{}", en.width(), en.height(), en.width() * 128, en.height() * 128, if en.mask().is_empty() { "" } else { "   (footprint mask)" },
        ));
        gui::text(&format!(
            "{}{}{}{}", if en.scalable() { "scalable  " } else { "" }, if en.random_rotatable() { "random rotatable  " } else { "" },
            if en.requires_infield_lodding() { "infield lodding  " } else { "" }, if en.encampable() { "encampable" } else { "" },
        ));
        if !en.custom_alpha_blend_texture().is_empty() {
            gui::text(&format!("custom alpha blend texture: {}", en.custom_alpha_blend_texture()));
        }

        if !en.custom_blend_tile().is_empty() {
            gui::text(&format!("custom blend tile: {}", en.custom_blend_tile()));
        }

        let g = en.texture_group();
        gui::text(&format!("ground groups  R {}  G {}  B {}  A {}", g.red, g.green, g.blue, g.alpha));
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

        let targets = en.link_targets();
        let links = en.links();
        if gui::tree_node(&format!("Links ({}) and link targets ({})###entrylinks", links.len(), targets.len())) {
            for t in &targets {
                gui::text(&format!("offers {} at cell ({}, {})", t.tile_set, t.x, t.y));
            }

            for l in &links {
                gui::text(&format!(
                    "cell ({}, {}) {} {}   base ({}, {}){}   blend {} px{}", l.x, l.y, if l.test == "TLT_NOT_EQUALS" { "must not be" } else { "must be" }, l.link_set,
                    l.base_x, l.base_y, if l.is_entry { "   entry" } else { "" }, l.blend_size, if l.no_offline_blend { "   no offline blend" } else { "" },
                ));
            }

            gui::tree_pop();
        }

        if let Some(i) = reopen {
            let file = en.file().display().to_string();
            let msg = self.open_tile(e, &file, &i.to_string()).unwrap_or_else(|m| format!("open failed: {m}"));
            e.status(&msg, false);
        }
    }

    fn draw_polygon_panel(&mut self, e: &mut Editor, id: DocumentId) {
        if !gui::collapsing_header("Polygons / AI hints", gui::TREE_DEFAULT_OPEN) {
            return;
        }

        gui::set_next_item_width(260.0);
        if gui::begin_combo("type", POLYGON_TYPES[self.new_poly_type]) {
            for (i, t) in POLYGON_TYPES.iter().enumerate() {
                if gui::selectable(t, i == self.new_poly_type, 0) {
                    self.new_poly_type = i;
                }
            }

            gui::end_combo();
        }

        if self.new_poly_type == 0 {
            gui::set_next_item_width(260.0);
            if gui::begin_combo("hint", HINTS[self.new_hint]) {
                for (i, h) in HINTS.iter().enumerate() {
                    if gui::selectable(h, i == self.new_hint, 0) {
                        self.new_hint = i;
                    }
                }

                gui::end_combo();
            }
        }

        if !self.drawing {
            if gui::button("Draw polygon") {
                self.start_drawing();
                self.status = "click the terrain to add points; Enter finishes, Esc cancels".to_owned();
            }
        } else {
            gui::text(&format!("{} points", self.draw_pts.len()));
            gui::same_line();
            if gui::button("Finish") {
                self.finish_polygon(e);
            }

            gui::same_line();
            if gui::button("Cancel") {
                self.drawing = false;
                self.draw_pts.clear();
            }
        }

        let sel = primary(e);
        let Some(d) = e.docs.find_as::<TileDocument>(id) else { return };
        if let Some(s) = sel.filter(|&s| d.tile().is_polygon(s)) {
            let sc = d.tile().tscene();
            gui::text(&format!("selected: {} points, area {:.1}", sc.polygon_points(s).len(), sc.polygon_area(s)));
            match self.poly_point {
                Some(pt) => gui::text(&format!("point {pt}: drag to move, Delete removes, shift+click edge inserts")),
                None => gui::text_disabled("click a point handle to edit; shift+click near an edge inserts a point"),
            }
        }
    }

    fn draw_weather_panel(&mut self, e: &mut Editor) {
        if !gui::collapsing_header("Weather / climate (which environment the game picks)", 0) {
            return;
        }

        // "Preview this weather on this tile": any open .environment / .lighting file lights the tile, live while it is edited.
        let now = e.weather_preview();
        let shown = (now != 0).then(|| e.docs.find(now).map(|d| d.title())).flatten();
        gui::set_next_item_width(300.0);
        if gui::begin_combo("preview weather", shown.as_deref().unwrap_or("(none: default light)")) {
            if gui::selectable("(none: default light)", shown.is_none(), 0) {
                let _ = e.set_weather_preview(0);
            }

            for id in e.docs.order().to_vec() {
                let Some(o) = e.docs.find(id).filter(|o| o.environment().is_some()) else { continue };
                if gui::selectable(&format!("{}##pv{id}", o.title()), id == now, 0) {
                    let _ = e.set_weather_preview(id);
                }
            }

            gui::end_combo();
        }

        gui::text_disabled("Open a weather file (Assets, or the Weather tool) to list it here. Double-click a file below to open and preview it.");
        let Some(env) = &self.env else {
            gui::text_disabled(&self.env_status);
            return;
        };

        for (label, category) in [
            ("map", "map"), ("climate", "climate"), ("season", "season"), ("climate change", "climate_change_index"), ("precipitation", "precipitation"),
            ("time of day", "time_of_day"), ("battle type", "battle_types"), ("snow attrition", "snow_attrition"),
        ] {
            let value = self.wq.entry(category.to_owned()).or_default();
            let vals = env.category(category).map(|m| m.values().clone()).unwrap_or_default();
            if gui::begin_combo(label, if value.is_empty() { "(any)" } else { value }) {
                if gui::selectable("(any)", value.is_empty(), 0) {
                    value.clear();
                }

                for v in &vals {
                    if gui::selectable(v, v == value, 0) {
                        *value = v.clone();
                    }
                }

                gui::end_combo();
            }
        }

        let query = self.wq.iter().filter(|(_, v)| !v.is_empty()).map(|(k, v)| (k.clone(), v.clone())).collect();
        let res = env.matches_listed_map("environment", &query);
        let m = &res.variations;
        gui::text(&format!(
            "{} matching files{}, each picked with probability 1/{}", m.len(), if res.used_default_map { " (after the default_map retry)" } else { "" }, m.len().max(1),
        ));
        gui::begin_child("envlist", gui::v2(0.0, 140.0), gui::CHILD_BORDERS, 0);
        let mut open = None;
        for c in m {
            let file = c.filename();
            if gui::selectable(file, false, gui::SELECTABLE_ALLOW_DOUBLE_CLICK) && gui::is_mouse_double_clicked(0) {
                open = Some(file.clone());
            }
        }

        gui::end_child();
        if let Some(file) = open {
            let key = normalize(&file);

            // The tile stays the document being edited.
            let before = e.docs.active_id();
            let mut found = e.docs.order().iter().copied().rfind(|&id| e.docs.find(id).is_some_and(|o| o.source_key() == key));
            let mut msg = String::new();
            if found.is_none() {
                match e.open_path(&file, true, false) {
                    Ok(m) => {
                        msg = m;
                        found = Some(e.docs.active_id());
                    }

                    Err(m) => msg = m,
                }
            }

            let previewing = match found {
                Some(id) => match e.set_weather_preview(id) {
                    Ok(m) => {
                        msg = m;
                        true
                    }

                    Err(m) => {
                        msg = m;
                        false
                    }
                },

                None => false,
            };

            if before != 0 {
                e.docs.set_active(before);
            }

            e.status(&msg, !previewing);
        }
    }
}

/// A browser row matches when every word is in its location, file name, set or variation names, in any case.
pub(crate) fn matches_words(en: &wce_formats::tile_index::TileIndexEntry, words: &[String]) -> bool {
    let mut hay = format!("{} {} {}", en.key, en.name, en.tile_set);
    for v in &en.variations {
        hay += " ";
        hay += v;
    }

    let hay = hay.to_lowercase();
    words.iter().all(|w| hay.contains(&w.to_lowercase()))
}

impl Tool for TileTool {
    fn id(&self) -> &str {
        "tile"
    }

    fn title(&self) -> &str {
        "Tile"
    }

    fn opens(&self) -> Vec<String> {
        vec![".ted".to_owned()]
    }

    fn default_layout(&self, dockspace: gui::ImGuiID) {
        let (left, center) = gui::dock_builder_split(dockspace, gui::DIR_LEFT, 0.22);
        let (right, center) = gui::dock_builder_split(center, gui::DIR_RIGHT, 0.30);
        let (bottom, center) = gui::dock_builder_split(center, gui::DIR_DOWN, 0.18);
        gui::dock_builder_dock_window("Viewport", center);
        gui::dock_builder_dock_window("Tiles", left);
        gui::dock_builder_dock_window("Scene", left);
        gui::dock_builder_dock_window("Outliner", left);
        gui::dock_builder_dock_window("Tile", right);
        gui::dock_builder_dock_window("Inspector", right);
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
    }

    fn update(&mut self, e: &mut Editor) {
        let Some(id) = document_id(e) else { return };

        // A new tile: frame it, and read the environment map of its kit.
        if self.camera_set_for != id {
            self.camera_set_for = id;
            let d = e.docs.find_as::<TileDocument>(id).expect("a tile");
            let (ex, ez) = (d.tile().extent_x(), d.tile().extent_z());
            let dir = d.project().dir.clone();
            if !e.viewport.is_fly() {
                e.viewport.orbit_mut().set_orbit(0.8, 0.6, (ex.max(ez) * 1.2) as f32, Float3::new((ex * 0.5) as f32, 0.0, (ez * 0.5) as f32));
            }

            self.load_environment_map(&dir);
            self.poly_point = None;
            self.drawing = false;
            self.grabbing = false;
            self.stroke_active = false;
            self.mode = TileMode::Select;
        }

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
        }

        let eye = e.viewport.camera().eye;
        let sel = selected(e);
        let handle = Self::handle_size(e);
        let d = e.docs.find_as::<TileDocument>(id).expect("a tile");
        let overlay = self.tool_overlay(e, d);
        let d = e.docs.find_as_mut::<TileDocument>(id).expect("a tile");
        d.tile_mut().update_view(&mut e.scene, eye);
        d.tile_mut().set_highlight(&mut e.scene, &sel, self.poly_point, handle, &overlay);
    }

    fn draw_panels(&mut self, e: &mut Editor) {
        self.handle_keys(e);
        self.draw_tile_panel(e);
        crate::scene_panel::draw_layers_panel(self, e);
        self.draw_browser_panel(e);
    }

    fn draw_menus(&mut self, e: &mut Editor) {
        if !gui::begin_menu("Tile") {
            return;
        }

        if gui::menu_item("Open tile...", None, false, true) {
            let filters = [
                ("Tile database entry or tile.ted".to_owned(), "*.xml tile.ted *.ted".to_owned()), ("Tile (tile.ted)".to_owned(), "tile.ted *.ted".to_owned()),
                ("All files".to_owned(), "*.*".to_owned()),
            ];
            if let Some(p) = file_dialog(false, Path::new(""), &filters) {
                let msg = self.open_tile(e, &p.display().to_string(), "").unwrap_or_else(|m| format!("open failed: {m}"));
                e.status(&msg, false);
            }
        }

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
        let ray = e.viewport.ray(ev.x, ev.y);
        match ev.kind {
            ViewportEventKind::Hover => {
                self.on_hover(e, ev);
                false
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

                if let Some(kind) = self.mode.brush() {
                    let dt = e.clock.dt();
                    let d = e.docs.find_as_mut::<TileDocument>(id).expect("a tile");
                    if self.terrain_hit && d.project().has_height() {
                        self.stroke_active = true;
                        d.brush_step(kind, self.cursor.x, self.cursor.z, self.brush, dt);
                    }

                    return true;
                }

                // Select mode: a polygon point handle starts a drag, shift + click near an edge inserts a point.
                let sel = primary(e);
                let d = e.docs.find_as_mut::<TileDocument>(id).expect("a tile");
                let (Some((o, dir)), Some(sel)) = (ray, sel.filter(|&s| d.tile().is_polygon(s))) else { return false };
                let pts = d.tile().polygon_world_points(sel);
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
                    let closed = d.tile().tscene().doc().get(sel, "polygon_closed").is_none_or(|v| v != "0");
                    let mut at = 0;
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
                        }
                    }

                    if d.polygon_point(sel, "insert", at, cur, 0).is_ok() {
                        self.poly_point = Some(at + 1);
                        self.status = "point inserted".to_owned();
                    }

                    return true;
                }

                false
            }

            ViewportEventKind::Drag => {
                if self.stroke_active {
                    self.on_hover(e, ev);
                    let dt = e.clock.dt();
                    let d = e.docs.find_as_mut::<TileDocument>(id).expect("a tile");
                    if let (true, Some(kind)) = (self.terrain_hit, self.mode.brush()) {
                        d.brush_step(kind, self.cursor.x, self.cursor.z, self.brush, dt);
                    }
                } else if self.point_drag {
                    self.on_hover(e, ev);
                    let sel = primary(e);
                    let d = e.docs.find_as_mut::<TileDocument>(id).expect("a tile");
                    if let (true, Some(sel), Some(pt)) = (self.terrain_hit, sel, self.poly_point) {
                        let sc = d.tile().tscene();
                        let pts = sc.polygon_points(sel);
                        if let Some(&point) = pts.get(pt) {
                            let before = sc.point_position(point).unwrap_or_default();
                            let after = d.tile().to_local(sel, self.cursor);
                            if before != after {
                                let _ = d.polygon_point(sel, "move", pt, self.cursor, self.point_gesture);
                            }
                        }
                    }
                }

                self.stroke_active || self.point_drag
            }

            ViewportEventKind::Release => {
                let d = e.docs.find_as_mut::<TileDocument>(id).expect("a tile");
                if self.stroke_active {
                    self.stroke_active = false;
                    d.end_stroke();
                }

                if self.point_drag {
                    self.point_drag = false;
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

    fn claims_left_button(&self, _e: &Editor) -> bool {
        self.mode != TileMode::Select || self.drawing || self.grabbing
    }

    fn open(&mut self, e: &mut Editor, path: &str, from_packs: bool) -> Result<Box<dyn Document>, String> {
        if from_packs {
            return Err("tiles are opened from the assembly kit folders on disk".to_owned());
        }

        let dir = tile_dir_of(path);

        // Opened by its database entry: the entry file is the root, the folder is what it names.
        let project = match self.pending_entry.take() {
            Some(entry) => TileProject::open_entry(&entry, self.pending_variation.take()),
            None => TileProject::open(&dir),
        }
        .map_err(|err| err.to_string())?;

        if !project.warning.is_empty() {
            log::warn(project.warning.clone());
            self.status = project.warning.clone();
        }

        Ok(Box::new(TileDocument::new(project, e.files())))
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}
