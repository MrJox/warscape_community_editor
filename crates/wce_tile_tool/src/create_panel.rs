//! The Create panel and its Shift+A popup: everything that can be put in a tile, in one list searched by words. Buildings, trees, decals,
//! models and effects (every valid key, `TileDocument::key_catalogue`), lights, prefabs (`wce_tile::prefabs`), the gameplay markers and
//! areas, the AI regions and hints, rivers and roads. A click on an entry starts placing it (clicks on the terrain, previewed under the
//! mouse; R / Shift+R turn it, Esc stops) or drawing it (clicks, Enter finishes). While it is placed or drawn, the Inspector shows its
//! properties (`Draft`), which every copy takes; they are kept per type until reset.
//!
//! As in TEd, the buildings and the prefabs are in folders: a building in its `battlefield_buildings` category (`Tile::building_db`: the
//! tile's own kit's table, then the projects' kit layout's and the Assembly Kits'), a prefab in its folder below `art/prefabs`; the folders come first, then the entries without one. The
//! prefabs of the projects folder (the editor's, whose prefabs placed inside stay references) are marked.

use std::sync::Arc;

use wce_editor::editor::Editor;
use wce_editor::{gui, icons, theme};
use wce_editor_core::document::Document;
use wce_editor_core::inspector::Inspector;
use wce_foundation::math::{Float3, rotation_y, translation};
use wce_scene::render_scene::DocumentId;
use wce_tile_editor::tile_document::{Create, Draft, turned};

use crate::settings::{RECENT_KEEP, SETTINGS_ID, TileSettings};
use crate::tool::{SetInUse, TileTool, document, primary};

/// The categories, in the panel's order.
pub const CATEGORIES: [&str; 11] =
    ["Buildings", "Vegetation", "Decals", "Models", "Effects", "Lights", "Prefabs", "Gameplay", "AI & regions", "Siege", "Rivers & roads"];

/// The types without a key of each fixed category (category, type, drawn).
const FIXED: [(usize, &str, bool); 17] = [
    (5, "POINT_LIGHT_INSTANCE", false), (7, "CaptureLocationInstance", false), (7, "DeploymentZonePolygonInstanceGroup", false),
    (7, "DEPLOYMENT_ZONE_POLYGON_INSTANCE", true), (7, "CivilianShelter", false), (7, "CivilianDeploymentZone", true), (7, "ZONE_POLYGON_INSTANCE", true),
    (7, "LiteBuildingZone", true), (9, "SiegeAINodeInstance", false), (9, "SiegeAIEdgeInstance", true), (8, "AI_HINT_POLYGON_INSTANCE", true),
    (8, "NOGO_REGION_POLYGON_INSTANCE", true), (8, "GO_REGION_POLYGON_INSTANCE", true), (8, "VegetationExclusionPolygonInstance", true),
    (9, "SIEGE_AI_POLYGON_INSTANCE", true), (8, "AISeparatorInstance", true), (10, "SplineRiverInstance", true),
];

/// The roads, drawn like the rivers.
const ROAD: (usize, &str, bool) = (10, "SEGMENTED_SPLINE_INSTANCE", true);

/// Not TEd's: a procedural wall (docs/plans/procedural_walls.md), drawn with the siege's types.
const WALL: (usize, &str, bool) = (9, wce_tile_editor::tile_document::WALL_SPLINE, true);

/// The keyed types of the first categories.
const KEYED: [(usize, &str); 5] = [(0, "BUILDING_INSTANCE"), (1, "VEGETATION_INSTANCE"), (2, "ProjectedDecalInstance"), (3, "SIMPLE_MESH_INSTANCE"), (4, "VFXInstance")];

/// What an entry makes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Make {
    /// A type placed by clicks, with its key (empty: none).
    Place { kind: String, key: String },

    /// A drawn type.
    Draw { kind: String },

    /// A prefab, by its key.
    Prefab { key: String },

    /// A vegetation set, by its name (its trees placed or painted, a key picked by the weights for each).
    VegSet { name: String },
}

#[derive(Clone, Debug)]
pub struct Entry {
    pub category: usize,
    pub label: String,
    pub make: Make,

    /// Its folder in the category (`/` separated; empty: none): a building's category, a prefab's folder.
    pub folder: String,

    /// A prefab of the projects folder (the editor's: the prefabs placed in it stay references).
    pub project: bool,

    /// Lower case words searched (the label, the type, the folder).
    search: String,
}

impl Entry {
    /// The new vegetation set (not saved) as an entry, to place or paint with it.
    pub fn new_set_entry() -> Self {
        Self { category: 1, label: "new vegetation set".to_owned(), make: Make::VegSet { name: String::new() }, folder: String::new(), project: false, search: String::new() }
    }

    /// Its id in the favourites and the recent entries.
    pub fn id(&self) -> String {
        match &self.make {
            Make::Place { kind, key } => format!("{kind}|{key}"),
            Make::Draw { kind } => format!("{kind}|"),
            Make::Prefab { key } => format!("prefab|{key}"),
            Make::VegSet { name } => format!("vegset|{name}"),
        }
    }

    fn icon(&self) -> &'static str {
        match &self.make {
            Make::Place { kind, .. } | Make::Draw { kind } => crate::entity_types::type_icon(kind),
            Make::Prefab { .. } if self.project => PROJECT_PREFAB_ICON,
            Make::Prefab { .. } => icons::COMPONENT,
            Make::VegSet { .. } => icons::TREES,
        }
    }
}

/// The mark of a prefab of the projects folder.
pub(crate) const PROJECT_PREFAB_ICON: &str = icons::LINK_2;

/// What the tooltip of a prefab of the projects folder says.
pub(crate) const PROJECT_PREFAB_TIP: &str = "A prefab of the projects folder (the editor's .wce): the prefabs placed in it stay references, and edits of it are saved there.";

/// The order of an entry in its category: its folder's parts, each before the entries (folders first), then the entry.
fn folder_order(folder: &str) -> Vec<(u8, String)> {
    folder.split('/').filter(|p| !p.is_empty()).map(|p| (0, p.to_lowercase())).chain([(1, String::new())]).collect()
}

/// The rows of a category shown by folder: each run of rows in the same folder, and how many rows each folder holds (its sub-folders' too).
#[derive(Clone, Debug, Default)]
struct FolderRows {
    runs: Vec<(String, Vec<usize>)>,
    counts: std::collections::HashMap<String, usize>,
}

/// What the entries were made for: the tile and the prefab library's version.
type EntriesStamp = (DocumentId, u64, u64);

/// What the Create panel shows: the entries (made again for another tile or another prefab library), its filter, the rows it let through,
/// the favourites and recent entries (the settings' lists, read once), and the Shift+A popup's filter.
#[derive(Default)]
pub struct CreateUi {
    entries: Option<(EntriesStamp, Arc<Vec<Entry>>)>,
    filter: String,
    shown: Vec<Vec<usize>>,
    folders: Vec<FolderRows>,
    shown_for: String,
    marks: Option<(Vec<String>, Vec<String>)>,
    quick_filter: String,
    quick_open: bool,

    /// The Shift+A popup's highlighted row (Up / Down move it while typing; Enter takes it).
    quick_row: usize,

    /// The search (the panel's, the popup's) gets the keyboard back on the next frame: Ctrl + Enter added a tree to the new set.
    filter_refocus: bool,
    quick_refocus: bool,

    /// The sticky drafts of each type (its values and turn), and the prefab's turn.
    pub drafts: std::collections::HashMap<String, Draft>,
    pub prefab_yaw: f32,

    /// The prefab being placed, turned: its key, the turn and the text.
    turned: Option<(String, f32, Vec<u8>)>,
}

fn words(filter: &str) -> Vec<String> {
    filter.to_lowercase().split_whitespace().map(str::to_owned).collect()
}

fn passes(e: &Entry, words: &[String]) -> bool {
    words.iter().all(|w| e.search.contains(w))
}

/// The turn of Alt + mouse while placing, in degrees per pixel the mouse goes left or right.
const ALT_TURN_DEG_PER_PX: f32 = 0.5;

/// What is placed being turned with Alt held (`TileTool::alt_turn_placing`): where it stays, whether that is on the terrain, and the
/// mouse's x and the turn when Alt went down.
#[derive(Clone, Copy, Debug)]
pub(crate) struct AltTurn {
    pub point: Float3,
    pub on_terrain: bool,
    pub mouse_x: f32,
    pub yaw: f32,
}

/// Alt + a left drag in Select mode (`TileTool::alt_rotate_begin`): the selection, the centre it turns around (the gizmo's space: world
/// metres), the press's x, the turn applied so far (degrees) and the drag's undo gesture.
#[derive(Clone, Debug)]
pub(crate) struct AltRotate {
    pub items: Vec<wce_scene::render_scene::SourceRef>,
    pub centre: Float3,
    pub x0: i32,
    pub applied: f32,
    pub gesture: u64,
}

impl TileTool {
    /// The entries of the open tile (with the prefab library).
    fn create_entries(&mut self, e: &mut Editor) -> Arc<Vec<Entry>> {
        let Some(id) = document(e).map(|d| d.id()) else { return Arc::default() };
        let stamp = (id, self.prefab_version, self.veg_sets_version);
        if let Some((s, entries)) = &self.create.entries
            && *s == stamp
        {
            return entries.clone();
        }

        let mut out = vec![];
        let entry = |category: usize, label: String, make: Make, extra: &str| {
            let search = format!("{label} {extra}").to_lowercase();
            Entry { category, label, make, folder: String::new(), project: false, search }
        };

        // The vegetation sets, at the top of the vegetation.
        for set in self.vegetation_sets(e).iter() {
            let label = if set.folder.is_empty() { format!("set: {}", set.name) } else { format!("set: {}   {}", set.name, set.folder) };
            let keys = set.entries.iter().map(|en| en.key.as_str()).collect::<Vec<_>>().join(" ");
            out.push(entry(1, label, Make::VegSet { name: set.name.clone() }, &format!("vegetation set {keys}")));
        }

        for (category, kind) in KEYED {
            for key in self.catalogue(e, kind).iter() {
                // A decal's or a model's path: its file name first, the folder after.
                let label = match key.rsplit_once(['/', '\\']) {
                    Some((folder, file)) => format!("{}   {folder}", file.trim_end_matches(".rigid_model_v2")),
                    None => key.clone(),
                };

                let mut en = entry(category, label, Make::Place { kind: kind.to_owned(), key: key.clone() }, kind);
                if kind == "BUILDING_INSTANCE"
                    && let Some(c) = document(e).and_then(|d| d.tile().building_category(key))
                {
                    en.search.push_str(&format!(" {}", c.to_lowercase()));
                    en.folder = c.to_owned();
                }

                out.push(en);
            }
        }

        // The types the game has (Rome 2's tiles have no civilians or lite building zones).
        let features = e.game().features;
        let has = |kind: &str| match kind {
            "CivilianShelter" | "CivilianDeploymentZone" => features.civilians,
            "LiteBuildingZone" => features.lite_buildings,
            _ => true,
        };
        for (category, kind, drawn) in FIXED.iter().chain([&ROAD, &WALL]).filter(|(_, kind, _)| has(kind)) {
            let make = if *drawn { Make::Draw { kind: (*kind).to_owned() } } else { Make::Place { kind: (*kind).to_owned(), key: String::new() } };
            out.push(entry(*category, crate::entity_types::type_name(kind).to_owned(), make, kind));
        }

        let lib = self.prefab_library(e);
        let projects = self.prefab_folders(e).into_iter().next();
        for p in lib.entries() {
            let project = projects.as_ref().is_some_and(|r| *r == p.root);
            let mut en = entry(6, p.key.clone(), Make::Prefab { key: p.key.clone() }, &format!("prefab {}{}", p.folder, if project { " project" } else { "" }));
            en.folder.clone_from(&p.folder);
            en.project = project;
            out.push(en);
        }

        // By category, then folder (stable: the order within a folder stays the catalogue's).
        out.sort_by_cached_key(|en| (en.category, folder_order(&en.folder)));
        let entries = Arc::new(out);
        self.create.entries = Some((stamp, entries.clone()));
        self.create.shown_for = "\u{1}".to_owned();
        entries
    }

    /// The favourite and the recent entries' ids, read from the settings once.
    fn create_marks(&mut self, e: &Editor) -> (Vec<String>, Vec<String>) {
        if self.create.marks.is_none() {
            let s = e.tool_settings::<TileSettings>(SETTINGS_ID);
            self.create.marks = Some((s.create_favourites, s.create_recent));
        }

        self.create.marks.clone().unwrap_or_default()
    }

    fn set_create_marks(&mut self, e: &mut Editor, favourites: Vec<String>, recent: Vec<String>) {
        self.create.marks = Some((favourites.clone(), recent.clone()));
        e.update_tool_settings(SETTINGS_ID, |s: &mut TileSettings| {
            s.create_favourites = favourites;
            s.create_recent = recent;
        });
    }

    /// Starts making an entry: placing it, or drawing it.
    pub fn start_making(&mut self, e: &mut Editor, en: &Entry) {
        // The new set (not saved) is not one to come back to.
        if !matches!(&en.make, Make::VegSet { name } if name.is_empty()) {
            let (favourites, mut recent) = self.create_marks(e);
            let id = en.id();
            recent.retain(|r| *r != id);
            recent.insert(0, id);
            recent.truncate(RECENT_KEEP);
            self.set_create_marks(e, favourites, recent);
        }

        self.drawing = false;
        self.draw_pts.clear();
        self.placing = None;
        self.veg_set = None;
        self.making = Some(en.label.clone());
        match &en.make {
            Make::VegSet { name } => {
                // No name: the new set, not saved yet.
                let set = if name.is_empty() { self.scratch_set.clone() } else { self.vegetation_sets(e).iter().find(|s| s.name == *name).cloned() };
                let Some(set) = set.filter(|s| !s.entries.is_empty()) else {
                    self.status = format!("the vegetation set {name} has no trees");
                    self.making = None;
                    return;
                };

                let kind = "VEGETATION_INSTANCE";
                let mut d = self.create.drafts.get(kind).cloned().unwrap_or_else(|| Draft::new(kind, ""));
                d.key = set.pick(self.random.unit()).map(|en| en.key.clone()).unwrap_or_default();
                self.draft = Some(d);
                self.veg_set = Some(if name.is_empty() { SetInUse::New } else { SetInUse::Saved(name.clone()) });
                self.placing = Some(crate::tool::Placing::Draft);
            }

            Make::Place { kind, key } => {
                let mut d = self.create.drafts.get(kind).cloned().unwrap_or_else(|| Draft::new(kind, key));
                d.key.clone_from(key);
                self.draft = Some(d);
                self.placing = Some(crate::tool::Placing::Draft);
            }

            Make::Draw { kind } => {
                self.draft = Some(self.create.drafts.get(kind).cloned().unwrap_or_else(|| Draft::new(kind, "")));
                self.start_drawing();
            }

            Make::Prefab { key } => {
                self.draft = None;
                self.placing = Some(crate::tool::Placing::Prefab(key.clone()));
            }
        }

        // Its values, before the first click.
        e.pending_focus = Some("Inspector".to_owned());
        self.status = match &en.make {
            Make::Draw { .. } => format!("{}: click the terrain for its points, Enter finishes, Esc cancels", en.label),
            _ => format!("{}: click the terrain to place (R / Shift+R turn it, Esc stops)", en.label),
        };
    }

    /// Stops placing or drawing.
    pub fn stop_making(&mut self) {
        self.veg_set = None;
        self.placing = None;
        self.drawing = false;
        self.draw_pts.clear();
        self.making = None;
    }

    /// Keeps the draft's values for the next time its type is made.
    fn keep_draft(&mut self) {
        if let Some(d) = &self.draft {
            self.create.drafts.insert(d.kind.clone(), d.clone());
        }
    }

    /// Turns what is being placed by `deg`.
    pub fn turn_placing(&mut self, deg: f32) {
        if let Some(yaw) = self.placing_yaw() {
            self.set_placing_yaw(yaw + deg);
        }
    }

    /// The turn of what is being placed (degrees).
    fn placing_yaw(&self) -> Option<f32> {
        match self.placing {
            Some(crate::tool::Placing::Prefab(_)) => Some(self.create.prefab_yaw),
            Some(crate::tool::Placing::Draft) => self.draft.as_ref().map(|d| d.yaw),
            None => None,
        }
    }

    fn set_placing_yaw(&mut self, deg: f32) {
        let deg = deg.rem_euclid(360.0);
        match self.placing {
            Some(crate::tool::Placing::Prefab(_)) => self.create.prefab_yaw = deg,
            Some(crate::tool::Placing::Draft) => {
                if let Some(d) = &mut self.draft {
                    d.yaw = deg;
                }

                self.keep_draft();
            }

            None => {}
        }
    }

    /// Alt pressed with the left button over the viewport in Select mode: the selection starts turning around its centre with the mouse
    /// (left / right). False without a selection that can turn.
    pub(crate) fn alt_rotate_begin(&mut self, e: &mut Editor, x: i32) -> bool {
        let Some(id) = crate::tool::document_id(e) else { return false };
        let items = e.selection.items().iter().copied().filter(|r| r.doc == id).collect::<Vec<_>>();
        let Some(d) = e.docs.find_as_mut::<wce_tile_editor::tile_document::TileDocument>(id) else { return false };
        let at = items.iter().filter_map(|&r| d.item_transform(r)).map(|m| Float3::new(m.m[0][3], m.m[1][3], m.m[2][3])).collect::<Vec<_>>();
        if at.is_empty() {
            return false;
        }

        let n = at.len() as f32;
        let centre = at.iter().fold(Float3::default(), |c, &p| c + p) * (1.0 / n);
        let gesture = d.next_gesture();
        self.alt_rotate = Some(AltRotate { items, centre, x0: x, applied: 0.0, gesture });
        self.status = "turning the selection (Alt + drag; Shift: by the rotate step)".to_owned();
        true
    }

    /// The drag of `alt_rotate_begin`: the selection turned about the vertical through its centre by the mouse's way left or right since the
    /// press (0.5 degree a pixel; Shift: whole rotate steps), one undo entry for the drag.
    pub(crate) fn alt_rotate_drag(&mut self, e: &mut Editor, x: i32, shift: bool) {
        let Some(mut r) = self.alt_rotate.clone() else { return };
        let density = gui::io().DisplayFramebufferScale.x.max(1e-3);
        let mut total = (x - r.x0) as f32 / density * ALT_TURN_DEG_PER_PX;
        if shift && self.angle_step > 0.0 {
            total = (total / self.angle_step).round() * self.angle_step;
        }

        let step = total - r.applied;
        if step.abs() < 1e-4 {
            return;
        }

        // The turn as the gizmo's world-space delta: about the vertical through the centre.
        let c = r.centre;
        let delta = translation(c) * rotation_y(step.to_radians()) * translation(Float3::new(-c.x, -c.y, -c.z));
        if let Some(d) = e.docs.find_mut(r.items[0].doc) {
            d.transform_items(&r.items, &delta, r.gesture);
        }

        r.applied = total;
        self.status = format!("turned {total:.1} deg (Alt + drag; Shift: steps of {} deg)", self.angle_step);
        self.alt_rotate = Some(r);
    }

    /// Per frame while placing: Alt held keeps what is placed where it was when Alt went down and turns it with the mouse (left / right;
    /// Shift: by the rotate step), so that it is placed already turned; a click places it there, letting Alt go puts it back under the
    /// mouse, turned.
    pub(crate) fn alt_turn_placing(&mut self, e: &Editor) {
        let io = gui::io();
        let Some(yaw) = self.placing_yaw().filter(|_| io.KeyAlt && !io.KeyCtrl && (e.viewport.hovered || self.alt_turn.is_some())) else {
            self.alt_turn = None;
            return;
        };

        // Alt with the mouse is not a trip to the menu bar.
        gui::cancel_alt_menu_toggle();
        let x = io.MousePos.x;
        let Some(t) = self.alt_turn else {
            if self.terrain_hit {
                let (point, on_terrain) = self.place_point();
                self.alt_turn = Some(AltTurn { point, on_terrain, mouse_x: x, yaw });
            }

            return;
        };

        let mut turned = t.yaw + (x - t.mouse_x) * ALT_TURN_DEG_PER_PX;
        if io.KeyShift && self.angle_step > 0.0 {
            turned = (turned / self.angle_step).round() * self.angle_step;
        }

        self.set_placing_yaw(turned);
        self.status = format!("turn {:.1} deg (Alt + mouse; Shift: steps of {} deg; click places it here)", turned.rem_euclid(360.0), self.angle_step);
    }

    /// The text of what a click places now (the preview shows it).
    pub(crate) fn placing_text(&mut self, e: &mut Editor) -> Option<Vec<u8>> {
        let text = self.placing_text_unmoded(e)?;
        self.placement_modes(e);
        let d = document(e)?;
        Some(self.apply_placement_modes(d, text))
    }

    fn placing_text_unmoded(&mut self, e: &mut Editor) -> Option<Vec<u8>> {
        match self.placing.clone()? {
            crate::tool::Placing::Draft => document(e)?.draft_text(self.draft.as_ref()?),
            crate::tool::Placing::Prefab(key) => {
                if self.placing_prefab_text.as_ref().is_none_or(|(k, _)| *k != key) {
                    let lib = self.prefab_library(e);
                    let text = lib.load(&key).map(|p| wce_tile_editor::tile_document::prefab_reference_text(&p.group_text, &key)).unwrap_or_default();
                    self.placing_prefab_text = Some((key.clone(), text));
                }

                // Turned once per turn, not every frame (a prefab can be large).
                let yaw = self.create.prefab_yaw;
                if self.create.turned.as_ref().is_none_or(|(k, y, _)| *k != key || *y != yaw) {
                    let text = self.placing_prefab_text.as_ref().map(|(_, t)| turned(t, yaw)).unwrap_or_default();
                    self.create.turned = Some((key, yaw, text));
                }

                self.create.turned.as_ref().map(|(_, _, t)| t.clone()).filter(|t| !t.is_empty())
            }
        }
    }

    /// Places what is being placed at a point on the ground (in the working layer).
    pub fn place_at(&mut self, e: &mut Editor, at: Float3) -> Result<String, String> {
        // From a vegetation set: one of its trees (another if the one picked left the set).
        let set = self.set_in_use(e);
        if let Some(set) = &set {
            let draft = self.draft.as_mut().ok_or("nothing to place")?;
            if !set.entries.iter().any(|en| en.key == draft.key) {
                draft.key = set.pick(self.random.unit()).map(|en| en.key.clone()).ok_or("the vegetation set has no trees")?;
            }
        }

        let text = self.placing_text(e).ok_or("nothing to place (no key?)")?;
        let (what, name) = match self.placing.clone() {
            Some(crate::tool::Placing::Prefab(key)) => (key.clone(), Some(key)),
            _ => (self.draft.as_ref().map(|d| if d.key.is_empty() { d.kind.clone() } else { d.key.clone() }).unwrap_or_default(), None),
        };

        let d = document(e).ok_or("no tile open")?;
        let into = self.new_items_layer(d).ok_or("this tile has no scene graph")?;
        let (at, on_terrain) = if self.preview_forced { (at, true) } else { self.place_point() };
        let placed = d.place_text(&format!("Add {what}"), &text, into, at, name.as_deref(), on_terrain).map(|_| format!("added {what}"));

        // Still placing (TEd's way: as many as there are clicks, Esc stops): the new one is not selected, and the selection there was
        // goes (the insert renumbers the elements after it).
        if placed.is_ok() {
            d.take_selection_hint();
            e.selection.clear();
        }

        // From a vegetation set: the next one is another of its trees.
        if let (Some(set), Some(draft)) = (set, self.draft.as_mut())
            && let Some(en) = set.pick(self.random.unit())
        {
            draft.key.clone_from(&en.key);
        }

        placed
    }

    /// Finishes a drawn type with the drawn points.
    pub fn finish_drawing(&mut self, e: &mut Editor) {
        if !self.drawing {
            return;
        }

        let sel = primary(e);
        let Some(draft) = self.draft.clone() else { return };
        let Some(d) = document(e) else { return };
        self.drawing = false;
        let kind = draft.kind.as_str();
        let layer = self.new_items_layer(d);

        // A deployment polygon goes into the selected deployment group.
        let group = sel.and_then(|s| {
            let sc = d.tile().tscene();
            std::iter::successors(Some(s), |&x| sc.doc().elements()[x].parent).find(|&x| sc.type_of(x) == "DeploymentZonePolygonInstanceGroup")
        });

        let into = if kind == "DEPLOYMENT_ZONE_POLYGON_INSTANCE" { group.or(layer) } else { layer };
        let added = into.ok_or_else(|| "this tile has no scene graph".to_owned())
            .and_then(|into| d.create_with(kind, into, &Create::Line(self.draw_pts.clone()), &draft.key, &draft.attrs));
        self.draw_pts.clear();
        self.poly_point = None;
        self.making = None;
        self.status = match added {
            Ok(el) => format!("added {kind} in {}", Self::layer_label(d, d.tile().layer_of(el).unwrap_or(el))),
            Err(err) => err,
        };
    }

    /// The Inspector while something is placed or drawn: its properties (a prefab: what it is). False when nothing is.
    pub fn inspect_making(&mut self, e: &mut Editor, ins: &mut dyn Inspector) -> bool {
        if self.placing.is_none() && !self.drawing {
            return false;
        }

        if let Some(crate::tool::Placing::Prefab(key)) = self.placing.clone() {
            let lib = self.prefab_library(e);
            ins.section(&format!("New prefab {key}"));
            if let Some(p) = lib.get(&key) {
                ins.text("folder", "folder", &p.folder);
                ins.text("file", "file", &p.path.display().to_string());
                let project = self.prefab_folders(e).first() == Some(&p.root);
                ins.text("source", "from", if project { "the projects folder: prefabs placed in it stay references" } else { "an Assembly Kit (TEd's): prefabs placed in it are copies" });
            }

            let mut yaw = self.create.prefab_yaw;
            if ins.edit_float("yaw", "turn (deg; R / Shift+R)", &mut yaw, 1.0, -360.0, 360.0) {
                self.create.prefab_yaw = yaw;
            }

            return true;
        }

        // The saved set in use (the brush's settings are above it, `Tool::inspect`); a painted tree's draft is not shown: the brush picks
        // the keys, scales and turns.
        self.inspect_veg_set(e);
        if self.painting_trees(e) {
            return true;
        }

        let Some(mut draft) = self.draft.clone() else { return false };
        let Some(d) = document(e) else { return false };
        ins.begin(d.id(), u64::MAX - 7);
        if d.inspect_draft(&mut draft, !self.drawing, ins) {
            self.draft = Some(draft);
            self.keep_draft();
        }

        if gui::button(&format!("{}  Reset to the defaults", icons::UNDO_2)) && let Some(dr) = &mut self.draft {
            dr.attrs.clear();
            dr.yaw = 0.0;
            self.keep_draft();
        }

        true
    }

    /// The rows of the entries that pass the filter, per category (made again when the filter or the entries change).
    fn filter_entries(&mut self, entries: &Arc<Vec<Entry>>) {
        let stamp = format!("{}|{:p}", self.create.filter, Arc::as_ptr(entries));
        if stamp == self.create.shown_for {
            return;
        }

        let words = words(&self.create.filter);
        self.create.shown = vec![vec![]; CATEGORIES.len()];
        for (i, en) in entries.iter().enumerate() {
            if passes(en, &words) {
                self.create.shown[en.category].push(i);
            }
        }

        self.create.folders = self.create.shown.iter().map(|rows| {
            let mut f = FolderRows::default();
            for &i in rows {
                let folder = &entries[i].folder;
                match f.runs.last_mut() {
                    Some((last, run)) if last == folder => run.push(i),
                    _ => f.runs.push((folder.clone(), vec![i])),
                }

                let parts = folder.split('/').filter(|p| !p.is_empty()).collect::<Vec<_>>();
                for n in 1..=parts.len() {
                    *f.counts.entry(parts[..n].join("/")).or_default() += 1;
                }
            }

            f
        }).collect();

        self.create.shown_for = stamp;
    }

    /// The models an entry places, for its thumbnail: a building, a tree, a model or a prefab (nothing for the others).
    fn thumbnail_parts(&mut self, e: &mut Editor, en: &Entry) -> Vec<(wce_foundation::math::Float4x4, wce_scene::render_scene::MeshInstance)> {
        let text = match &en.make {
            Make::Place { kind, key } if !key.is_empty() && matches!(kind.as_str(), "BUILDING_INSTANCE" | "VEGETATION_INSTANCE" | "SIMPLE_MESH_INSTANCE") => {
                document(e).and_then(|d| d.draft_text(&Draft::new(kind, key)))
            }

            Make::Prefab { key } => {
                let lib = self.prefab_library(e);
                lib.load(key).map(|p| wce_tile_editor::tile_document::prefab_reference_text(&p.group_text, key)).ok()
            }

            _ => None,
        };

        match (text, document(e)) {
            (Some(text), Some(d)) => d.tile().model_parts(&text),
            _ => vec![],
        }
    }

    /// Whether an entry can have a thumbnail.
    fn has_thumbnail(en: &Entry) -> bool {
        match &en.make {
            Make::Place { kind, key } => !key.is_empty() && matches!(kind.as_str(), "BUILDING_INSTANCE" | "VEGETATION_INSTANCE" | "SIMPLE_MESH_INSTANCE"),
            Make::Prefab { .. } => true,
            _ => false,
        }
    }

    /// An entry's thumbnail when it is ready; asks for it when it was never made (`first`: before the others).
    fn thumbnail(&mut self, e: &mut Editor, en: &Entry, first: bool) -> Option<wce_editor::images::ImageRef> {
        let dir = crate::thumbnails::cache_dir(e);
        if self.thumbs.dir != dir {
            self.thumbs = crate::thumbnails::Thumbnails::default();
            self.thumbs.dir = dir;
        }

        let id = en.id();
        if let Some(img) = self.thumbs.get(e, &id) {
            return Some(img);
        }

        if !self.thumbs.known(&id) {
            let parts = self.thumbnail_parts(e, en);
            self.thumbs.request(&id, parts, first);
        }

        None
    }

    /// The tooltip of the entry under the mouse: its thumbnail, or why there is none yet.
    fn thumbnail_tooltip(&mut self, e: &mut Editor, en: &Entry) {
        if !Self::has_thumbnail(en) {
            return;
        }

        if en.project && gui::begin_tooltip() {
            gui::text_colored(theme::palette().success, &format!("{PROJECT_PREFAB_ICON}  {PROJECT_PREFAB_TIP}"));
            gui::end_tooltip();
        }

        let img = self.thumbnail(e, en, true);
        if !gui::begin_tooltip() {
            return;
        }

        match img {
            Some(img) => gui::image(img.id, gui::v2(img.w as f32, img.h as f32)),
            None if self.thumbs.failed(&en.id()) => gui::text_disabled("no model to show"),
            None => gui::text_disabled("making the thumbnail..."),
        }

        gui::end_tooltip();
    }

    /// A row of the list: the entry, lit while it is made, its star at the right. Returns (clicked, star clicked, hovered).
    fn create_row(&self, en: &Entry, favourite: bool, uid: &str) -> (bool, bool, bool) {
        let on = self.making.as_deref() == Some(en.label.as_str());
        let width = (gui::content_region_avail().x - gui::frame_height_with_spacing()).max(10.0);
        if en.project {
            let c = theme::palette().success;
            gui::push_style_color(gui::COL_TEXT, gui::v4(c[0], c[1], c[2], c[3]));
        }

        let clicked = gui::selectable_sized(&format!("{}  {}##{uid}", en.icon(), en.label), on, 0, gui::v2(width, 0.0));
        if en.project {
            gui::pop_style_color(1);
        }

        let hovered = gui::is_item_hovered();
        gui::same_line();
        gui::push_style_color(gui::COL_BUTTON, gui::v4(0.0, 0.0, 0.0, 0.0));
        let colour = if favourite {
            let h = theme::palette().highlight;
            gui::v4(h[0], h[1], h[2], h[3])
        } else {
            gui::style_color(gui::COL_TEXT_DISABLED)
        };
        gui::push_style_color(gui::COL_TEXT, colour);
        let star = gui::small_button(&format!("{}##star{uid}", icons::STAR));
        gui::pop_style_color(2);
        (clicked, star, hovered)
    }

    /// Sets the Create panel's search (a script).
    pub fn set_create_filter(&mut self, words: &str) {
        words.clone_into(&mut self.create.filter);
    }

    /// A category's rows in their folders: a tree node per folder (open while a filter is typed), `rows` drawing each run of rows.
    fn folder_tree(&self, category: usize, folders: &FolderRows, filtering: bool, mut rows: impl FnMut(&Self, &[usize])) {
        // The folders open now (their nodes pushed), and the folder whose node is closed (what is below it is not drawn).
        let mut open: Vec<&str> = vec![];
        let mut closed: Option<Vec<&str>> = None;
        for (folder, run) in &folders.runs {
            let parts = folder.split('/').filter(|p| !p.is_empty()).collect::<Vec<_>>();
            if let Some(shut) = &closed {
                if parts.starts_with(shut) {
                    continue;
                }

                closed = None;
            }

            let common = open.iter().zip(&parts).take_while(|(a, b)| a == b).count();
            while open.len() > common {
                gui::tree_pop();
                open.pop();
            }

            while open.len() < parts.len() {
                let path = parts[..=open.len()].join("/");
                if filtering {
                    gui::set_next_item_open(true, gui::COND_ALWAYS);
                }

                let count = folders.counts.get(&path).copied().unwrap_or_default();
                let label = format!("{}  {} ({count})", icons::FOLDER, parts[open.len()]);
                if !gui::tree_node_ex_id(&format!("create_cat{category}/{path}"), gui::TREE_SPAN_AVAIL_WIDTH, &label) {
                    closed = Some(parts[..=open.len()].to_vec());
                    break;
                }

                open.push(parts[open.len()]);
            }

            if closed.is_none() {
                rows(self, run);
            }
        }

        for _ in open {
            gui::tree_pop();
        }
    }

    /// The Create panel.
    pub(crate) fn draw_create_panel(&mut self, e: &mut Editor) {
        gui::set_next_window_size(gui::v2(360.0, 480.0), gui::COND_FIRST_USE_EVER);
        if let Some(dock) = self.left_dock.filter(|&d| d != 0) {
            gui::set_next_window_dock_id(dock, gui::COND_FIRST_USE_EVER);
        }

        if !gui::begin("Create", None, 0) {
            gui::end();
            return;
        }

        if document(e).is_none() {
            gui::text_disabled("Open a tile first.");
            gui::end();
            return;
        }

        // What is being made, and how to stop.
        if let Some(label) = self.making.clone().filter(|_| self.placing.is_some() || self.drawing) {
            gui::text_colored(theme::palette().highlight, &format!("{}  {label}", if self.drawing { icons::SQUARE_PEN } else { icons::MOUSE_POINTER_2 }));
            if self.drawing {
                gui::text_disabled(&format!("{} points: click the terrain; Enter finishes", self.draw_pts.len()));
                if gui::button("Finish") {
                    self.finish_drawing(e);
                }

                gui::same_line();
            } else {
                gui::text_disabled("click the terrain to place; R / Shift+R turn it");
            }

            if gui::button("Stop (Esc)") {
                self.stop_making();
            }

            gui::separator();
        }

        // A set from the selected trees.
        if self.trees_selected(e) && gui::small_button(&format!("{}  New vegetation set from the selected trees", icons::TREES)) {
            self.request_new_vegetation_set();
        }

        gui::set_next_item_width(-1.0);
        if std::mem::take(&mut self.create.filter_refocus) {
            gui::set_keyboard_focus_here(0);
        }

        let enter = gui::input_text("##createfilter", Some(&format!("{}  search everything (Enter: the first; Shift+A in the viewport; Ctrl + click / Ctrl + Enter a tree: into a new set)", icons::SEARCH)), &mut self.create.filter, 128, gui::INPUT_ENTER_RETURNS_TRUE);
        let entries = self.create_entries(e);
        self.filter_entries(&entries);

        // Enter takes the first entry found (Ctrl + Enter on a tree: into the new set, and the search keeps the keyboard).
        let first = (enter && !self.create.filter.trim().is_empty()).then(|| self.create.shown.iter().find_map(|rows| rows.first().copied())).flatten();
        if let Some(i) = first {
            self.create.filter_refocus = gui::io().KeyCtrl && self.set_entries_of(e, &entries[i]).is_some();
        }

        let words = words(&self.create.filter);
        let (favourites, recent) = self.create_marks(e);
        let by_id = |id: &str| entries.iter().position(|en| en.id() == id);

        // Thumbnails of the list (made in the background, kept in the cache folder).
        if self.thumbs.busy() {
            gui::text_disabled(&format!("{}  thumbnails: {} of {}", icons::IMAGE, self.thumbs.made, self.thumbs.asked));
            gui::same_line();
            if gui::small_button("Cancel##thumbs") {
                self.thumbs.cancel();
            }
        } else if gui::small_button(&format!("{}  Make thumbnails of the list", icons::IMAGE)) {
            let rows = self.create.shown.iter().flatten().copied().filter(|&i| Self::has_thumbnail(&entries[i])).collect::<Vec<_>>();
            for i in rows {
                self.thumbnail(e, &entries[i], false);
            }
        }

        if gui::is_item_hovered() {
            gui::set_tooltip("Renders a picture of every building, tree, model and prefab the list shows (filter it first: there are thousands), kept in the cache folder. Hover an entry to see its picture; one hovered without a picture is made then.");
        }

        let mut start = first;
        let mut star = None;
        let mut hovered = None;
        if gui::begin_child("createlist", gui::v2(0.0, 0.0), 0, 0) {
            // Favourites and recent entries (filtered alike), then the categories (open while a filter is typed).
            for (title, icon, ids) in [("Favourites", icons::STAR, &favourites), ("Recent", icons::HISTORY, &recent)] {
                let rows = ids.iter().filter_map(|id| by_id(id)).filter(|&i| passes(&entries[i], &words)).collect::<Vec<_>>();
                if rows.is_empty() {
                    continue;
                }

                if gui::tree_node_ex_id(&format!("create_{title}"), gui::TREE_DEFAULT_OPEN | gui::TREE_SPAN_AVAIL_WIDTH, &format!("{icon}  {title} ({})", rows.len())) {
                    for i in rows {
                        let en = &entries[i];
                        let (c, s, h) = self.create_row(en, favourites.contains(&en.id()), &format!("{title}{i}"));
                        if c {
                            start = Some(i);
                        }

                        if h {
                            hovered = Some(i);
                        }

                        if s {
                            star = Some(i);
                        }
                    }

                    gui::tree_pop();
                }
            }

            let filtering = !words.is_empty();
            for (c, title) in CATEGORIES.iter().enumerate() {
                let rows = self.create.shown.get(c).cloned().unwrap_or_default();
                if rows.is_empty() && !(c == 6 && !filtering) {
                    continue;
                }

                if filtering {
                    gui::set_next_item_open(true, gui::COND_ALWAYS);
                }

                if !gui::tree_node_ex_id(&format!("create_cat{c}"), gui::TREE_SPAN_AVAIL_WIDTH, &format!("{title} ({})", rows.len())) {
                    continue;
                }

                if c == 6 && rows.is_empty() {
                    gui::text_wrapped("No prefabs. TEd keeps them in <Assembly Kit>/raw_data/art/prefabs; new ones are saved in the projects folder's assembly_kit/raw_data/art/prefabs.");
                }

                let folders = self.create.folders.get(c).cloned().unwrap_or_default();
                self.folder_tree(c, &folders, filtering, |t, rows| {
                    gui::clipped(rows.len(), |r| {
                        let i = rows[r];
                        let en = &entries[i];
                        let (cl, s, h) = t.create_row(en, favourites.contains(&en.id()), &i.to_string());
                        if cl {
                            start = Some(i);
                        }

                        if h {
                            hovered = Some(i);
                        }

                        if s {
                            star = Some(i);
                        }
                    });
                });

                gui::tree_pop();
            }
        }

        if let Some(i) = hovered {
            self.thumbnail_tooltip(e, &entries[i]);
        }

        gui::end_child();
        gui::end();

        if let Some(i) = star {
            let (mut favourites, recent) = self.create_marks(e);
            let id = entries[i].id();
            match favourites.iter().position(|f| *f == id) {
                Some(k) => {
                    favourites.remove(k);
                }

                None => favourites.push(id),
            }

            self.set_create_marks(e, favourites, recent);
        }

        if let Some(i) = start {
            self.make_or_add(e, &entries[i], self.create.filter_refocus);
        }
    }

    /// A click on an entry: makes it; with Ctrl, a tree (or a set) goes into the new vegetation set instead.
    /// With `keep_focus` (the Shift+A popup staying open, a search after Ctrl + Enter) the Inspector is not focused: that would close the
    /// popup or take the keyboard from the search.
    fn make_or_add(&mut self, e: &mut Editor, en: &Entry, keep_focus: bool) {
        match self.set_entries_of(e, en).filter(|_| gui::io().KeyCtrl) {
            Some(keys) => {
                self.add_to_new_set(e, &keys);
                e.pending_focus = (!keep_focus).then(|| "Inspector".to_owned());
            }

            None => self.start_making(e, en),
        }
    }

    /// What an entry adds to the new vegetation set: a tree, or a set's trees; `None` for anything else.
    fn set_entries_of(&mut self, e: &mut Editor, en: &Entry) -> Option<Vec<wce_tile::vegetation_sets::SetEntry>> {
        match &en.make {
            Make::Place { kind, key } if kind == "VEGETATION_INSTANCE" && !key.is_empty() => Some(vec![wce_tile::vegetation_sets::SetEntry { key: key.clone(), weight: 1.0, scale: None }]),
            Make::VegSet { name } => self.vegetation_sets(e).iter().find(|s| s.name == *name).map(|s| s.entries.clone()),
            _ => None,
        }
    }

    /// Asks for the Shift+A popup (it opens on the next frame, at the mouse).
    pub fn request_quick_add(&mut self) {
        self.create.quick_open = true;
        self.create.quick_filter.clear();
        self.create.quick_row = 0;
    }

    /// The Shift+A popup: the same search, the first rows; Enter takes the first.
    pub(crate) fn draw_quick_add(&mut self, e: &mut Editor) {
        if std::mem::take(&mut self.create.quick_open) {
            gui::open_popup("quick_add");
        }

        // No keyboard navigation in it: Up / Down are the list's, and the search keeps the keyboard.
        if !gui::begin_popup_flags("quick_add", gui::WINDOW_NO_NAV_INPUTS) {
            return;
        }

        if gui::is_window_appearing() || std::mem::take(&mut self.create.quick_refocus) {
            gui::set_keyboard_focus_here(0);
        }

        gui::set_next_item_width(380.0);
        let before = self.create.quick_filter.clone();
        let enter = gui::input_text("##quickfilter", Some(&format!("{}  add... (words are and-ed; Up / Down, Enter)", icons::SEARCH)), &mut self.create.quick_filter, 128, gui::INPUT_ENTER_RETURNS_TRUE);
        if self.create.quick_filter != before {
            self.create.quick_row = 0;
        }

        let entries = self.create_entries(e);
        let words = words(&self.create.quick_filter);
        let (_, recent) = self.create_marks(e);

        // With no words: the recent entries; else every match.
        let rows = if words.is_empty() {
            recent.iter().filter_map(|id| entries.iter().position(|en| en.id() == *id)).collect::<Vec<_>>()
        } else {
            entries.iter().enumerate().filter(|(_, en)| passes(en, &words)).map(|(i, _)| i).collect()
        };

        gui::text_disabled(&if words.is_empty() { "recent".to_owned() } else { format!("{} found", rows.len()) });
        let moved = if gui::is_key_pressed(gui::KEY_DOWN_ARROW, true) {
            self.create.quick_row = (self.create.quick_row + 1).min(rows.len().saturating_sub(1));
            true
        } else if gui::is_key_pressed(gui::KEY_UP_ARROW, true) {
            self.create.quick_row = self.create.quick_row.saturating_sub(1);
            true
        } else {
            false
        };

        let row = self.create.quick_row.min(rows.len().saturating_sub(1));
        let mut start = enter.then(|| rows.get(row).copied()).flatten();
        if gui::begin_child("quickrows", gui::v2(380.0, 300.0), 0, gui::WINDOW_NO_NAV) {
            // The highlighted row in view (the list draws only the rows on screen, so it is scrolled to by its height).
            if moved {
                let line = gui::text_line_height_with_spacing();
                let (top, height) = (row as f32 * line, gui::window_height());
                if top < gui::scroll_y() {
                    gui::set_scroll_y(top);
                } else if top + line > gui::scroll_y() + height {
                    gui::set_scroll_y(top + line - height);
                }
            }

            gui::clipped(rows.len(), |r| {
                let en = &entries[rows[r]];
                if gui::selectable(&format!("{}  {}   ##q{}", en.icon(), en.label, rows[r]), r == row, gui::SELECTABLE_NO_AUTO_CLOSE) {
                    start = Some(rows[r]);
                }

                gui::same_line();
                if en.folder.is_empty() {
                    gui::text_disabled(CATEGORIES[en.category]);
                } else {
                    gui::text_disabled(&format!("{} / {}", CATEGORIES[en.category], en.folder));
                }
            });
        }

        gui::end_child();

        // It stays open while trees are added to the new set (Ctrl + click), to add more.
        let adding = start.is_some_and(|i| gui::io().KeyCtrl && self.set_entries_of(e, &entries[i]).is_some());
        self.create.quick_refocus = adding && enter;
        if (start.is_some() && !adding) || gui::is_key_pressed(gui::KEY_ESCAPE, false) {
            gui::close_current_popup();
        }

        gui::end_popup();
        if let Some(i) = start {
            self.make_or_add(e, &entries[i], adding);
        }
    }
}

/// Places what the Create panel makes now from a script: `kind` (a type, `building` with a key, `prefab` with its key).
/// Script: asks for an entry's thumbnail (`building`, a keyed type or `prefab`, and its key); its file when it is made.
pub fn thumbnail_for_script(t: &mut TileTool, e: &mut Editor, what: &str, key: &str) -> Result<Option<std::path::PathBuf>, String> {
    let make = match what {
        "building" => Make::Place { kind: "BUILDING_INSTANCE".to_owned(), key: key.to_owned() },
        "prefab" => Make::Prefab { key: key.to_owned() },
        kind => Make::Place { kind: kind.to_owned(), key: key.to_owned() },
    };

    let en = Entry { category: 0, label: key.to_owned(), make, folder: String::new(), project: false, search: String::new() };
    if !TileTool::has_thumbnail(&en) {
        return Err(format!("{what} has no thumbnails"));
    }

    t.thumbnail(e, &en, true);
    if t.thumbs.failed(&en.id()) {
        return Err(format!("no model to show for {key}"));
    }

    Ok(t.thumbs.file(&en.id()))
}

pub fn make_for_script(t: &mut TileTool, e: &mut Editor, what: &str, key: &str) -> Result<(), String> {
    let make = match what {
        "building" => Make::Place { kind: "BUILDING_INSTANCE".to_owned(), key: key.to_owned() },
        "prefab" => Make::Prefab { key: key.to_owned() },
        kind if FIXED.iter().chain([&ROAD, &WALL]).any(|(_, k, drawn)| *k == kind && *drawn) => Make::Draw { kind: kind.to_owned() },
        kind if FIXED.iter().any(|(_, k, _)| *k == kind) || KEYED.iter().any(|(_, k)| *k == kind) => Make::Place { kind: kind.to_owned(), key: key.to_owned() },
        kind => return Err(format!("{kind} can't be created here")),
    };

    let label = if key.is_empty() { crate::entity_types::type_name(what).to_owned() } else { key.to_owned() };
    let en = Entry { category: 0, label, make, folder: String::new(), project: false, search: String::new() };
    t.start_making(e, &en);
    Ok(())
}

/// Places or paints a vegetation set from a script, as its Create panel entry does.
pub fn make_set_for_script(t: &mut TileTool, e: &mut Editor, name: &str) -> Result<(), String> {
    if !t.vegetation_sets(e).iter().any(|s| s.name == name) {
        return Err(format!("no vegetation set '{name}'"));
    }

    let en = Entry { category: 1, label: format!("set: {name}"), make: Make::VegSet { name: name.to_owned() }, folder: String::new(), project: false, search: String::new() };
    t.start_making(e, &en);
    Ok(())
}
