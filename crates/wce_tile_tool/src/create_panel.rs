//! The Create panel and its Shift+A popup: everything that can be put in a tile, in one list searched by words. Buildings, trees, decals,
//! models and effects (every valid key, `TileDocument::key_catalogue`), lights, prefabs (`wce_tile::prefabs`), the gameplay markers and
//! areas, the AI regions and hints, rivers and roads. A click on an entry starts placing it (clicks on the terrain, previewed under the
//! mouse; R / Shift+R turn it, Esc stops) or drawing it (clicks, Enter finishes). While it is placed or drawn, the Inspector shows its
//! properties (`Draft`), which every copy takes; they are kept per type until reset.

use std::sync::Arc;

use wce_editor::editor::Editor;
use wce_editor::{gui, icons, theme};
use wce_editor_core::document::Document;
use wce_editor_core::inspector::Inspector;
use wce_foundation::math::Float3;
use wce_scene::render_scene::DocumentId;
use wce_tile_editor::tile_document::{Create, Draft, turned};

use crate::settings::{RECENT_KEEP, SETTINGS_ID, TileSettings};
use crate::tool::{TileTool, document, primary};

/// The categories, in the panel's order.
pub const CATEGORIES: [&str; 10] = ["Buildings", "Vegetation", "Decals", "Models", "Effects", "Lights", "Prefabs", "Gameplay", "AI & regions", "Rivers & roads"];

/// The types without a key of each fixed category (category, type, drawn).
const FIXED: [(usize, &str, bool); 17] = [
    (5, "POINT_LIGHT_INSTANCE", false), (7, "CaptureLocationInstance", false), (7, "DeploymentZonePolygonInstanceGroup", false),
    (7, "DEPLOYMENT_ZONE_POLYGON_INSTANCE", true), (7, "CivilianShelter", false), (7, "CivilianDeploymentZone", true), (7, "ZONE_POLYGON_INSTANCE", true),
    (7, "LiteBuildingZone", true), (7, "SiegeAINodeInstance", false), (7, "SiegeAIEdgeInstance", true), (8, "AI_HINT_POLYGON_INSTANCE", true),
    (8, "NOGO_REGION_POLYGON_INSTANCE", true), (8, "GO_REGION_POLYGON_INSTANCE", true), (8, "VegetationExclusionPolygonInstance", true),
    (8, "SIEGE_AI_POLYGON_INSTANCE", true), (8, "AISeparatorInstance", true), (9, "SplineRiverInstance", true),
];

/// The roads, drawn like the rivers.
const ROAD: (usize, &str, bool) = (9, "SEGMENTED_SPLINE_INSTANCE", true);

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

    /// Lower case words searched (the label, the type, the folder).
    search: String,
}

impl Entry {
    /// The new vegetation set (not saved) as an entry, to place or paint with it.
    pub fn new_set_entry() -> Self {
        Self { category: 1, label: "new vegetation set".to_owned(), make: Make::VegSet { name: String::new() }, search: String::new() }
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
            Make::Prefab { .. } => icons::COMPONENT,
            Make::VegSet { .. } => icons::TREES,
        }
    }
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
    shown_for: String,
    marks: Option<(Vec<String>, Vec<String>)>,
    quick_filter: String,
    quick_open: bool,

    /// The Shift+A popup's highlighted row (Up / Down move it while typing; Enter takes it).
    quick_row: usize,

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
            Entry { category, label, make, search }
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

                out.push(entry(category, label, Make::Place { kind: kind.to_owned(), key: key.clone() }, kind));
            }
        }

        for (category, kind, drawn) in FIXED.iter().chain([&ROAD]) {
            let make = if *drawn { Make::Draw { kind: (*kind).to_owned() } } else { Make::Place { kind: (*kind).to_owned(), key: String::new() } };
            out.push(entry(*category, crate::entity_types::type_name(kind).to_owned(), make, kind));
        }

        let lib = self.prefab_library(e);
        for p in lib.entries() {
            let label = if p.folder.is_empty() { p.key.clone() } else { format!("{}   {}", p.key, p.folder) };
            out.push(entry(6, label, Make::Prefab { key: p.key.clone() }, "prefab"));
        }

        out.sort_by_key(|en| en.category);
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
        let (favourites, mut recent) = self.create_marks(e);
        let id = en.id();
        recent.retain(|r| *r != id);
        recent.insert(0, id);
        recent.truncate(RECENT_KEEP);
        self.set_create_marks(e, favourites, recent);

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
                self.veg_set = Some(set);
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
        match self.placing {
            Some(crate::tool::Placing::Prefab(_)) => self.create.prefab_yaw = (self.create.prefab_yaw + deg).rem_euclid(360.0),
            Some(crate::tool::Placing::Draft) => {
                if let Some(d) = &mut self.draft {
                    d.yaw = (d.yaw + deg).rem_euclid(360.0);
                }

                self.keep_draft();
            }

            None => {}
        }
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
        let text = self.placing_text(e).ok_or("nothing to place (no key?)")?;
        let (what, name) = match self.placing.clone() {
            Some(crate::tool::Placing::Prefab(key)) => (key.clone(), Some(key)),
            _ => (self.draft.as_ref().map(|d| if d.key.is_empty() { d.kind.clone() } else { d.key.clone() }).unwrap_or_default(), None),
        };

        let d = document(e).ok_or("no tile open")?;
        let into = self.new_items_layer(d).ok_or("this tile has no scene graph")?;
        let (at, on_terrain) = if self.preview_forced { (at, true) } else { self.place_point() };
        let placed = d.place_text(&format!("Add {what}"), &text, into, at, name.as_deref(), on_terrain).map(|_| format!("added {what}"));

        // From a vegetation set: the next one is another of its trees.
        if let (Some(set), Some(draft)) = (self.veg_set.as_ref(), self.draft.as_mut())
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
            }

            let mut yaw = self.create.prefab_yaw;
            if ins.edit_float("yaw", "turn (deg; R / Shift+R)", &mut yaw, 1.0, -360.0, 360.0) {
                self.create.prefab_yaw = yaw;
            }

            return true;
        }

        // The brush's settings first while it paints trees; the set in use.
        if self.painting_trees(e) {
            self.inspect_veg_brush(e, ins);
        }

        if self.veg_set.is_some() {
            self.inspect_veg_set(e, ins);
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

        self.create.shown_for = stamp;
    }

    /// A row of the list: the entry, lit while it is made, its star at the right. Returns (clicked, star clicked).
    fn create_row(&self, en: &Entry, favourite: bool, uid: &str) -> (bool, bool) {
        let on = self.making.as_deref() == Some(en.label.as_str());
        let width = (gui::content_region_avail().x - gui::frame_height_with_spacing()).max(10.0);
        let clicked = gui::selectable_sized(&format!("{}  {}##{uid}", en.icon(), en.label), on, 0, gui::v2(width, 0.0));
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
        (clicked, star)
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
        gui::input_text("##createfilter", Some(&format!("{}  search everything (Shift+A in the viewport; Ctrl + click a tree: into a new set)", icons::SEARCH)), &mut self.create.filter, 128, 0);
        let entries = self.create_entries(e);
        self.filter_entries(&entries);
        let words = words(&self.create.filter);
        let (favourites, recent) = self.create_marks(e);
        let by_id = |id: &str| entries.iter().position(|en| en.id() == id);

        let mut start = None;
        let mut star = None;
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
                        let (c, s) = self.create_row(en, favourites.contains(&en.id()), &format!("{title}{i}"));
                        if c {
                            start = Some(i);
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

                gui::clipped(rows.len(), |r| {
                    let i = rows[r];
                    let en = &entries[i];
                    let (cl, s) = self.create_row(en, favourites.contains(&en.id()), &i.to_string());
                    if cl {
                        start = Some(i);
                    }

                    if s {
                        star = Some(i);
                    }
                });

                gui::tree_pop();
            }
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
            self.make_or_add(e, &entries[i]);
        }
    }

    /// A click on an entry: makes it; with Ctrl, a tree (or a set) goes into the new vegetation set instead.
    fn make_or_add(&mut self, e: &mut Editor, en: &Entry) {
        match self.set_entries_of(e, en).filter(|_| gui::io().KeyCtrl) {
            Some(keys) => {
                self.add_to_new_set(&keys);
                e.pending_focus = Some("Inspector".to_owned());
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

        if gui::is_window_appearing() {
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
                gui::text_disabled(CATEGORIES[en.category]);
            });
        }

        gui::end_child();

        // It stays open while trees are added to the new set (Ctrl + click), to add more.
        let adding = start.is_some_and(|i| gui::io().KeyCtrl && self.set_entries_of(e, &entries[i]).is_some());
        if (start.is_some() && !adding) || gui::is_key_pressed(gui::KEY_ESCAPE, false) {
            gui::close_current_popup();
        }

        gui::end_popup();
        if let Some(i) = start {
            self.make_or_add(e, &entries[i]);
        }
    }
}

/// Places what the Create panel makes now from a script: `kind` (a type, `building` with a key, `prefab` with its key).
pub fn make_for_script(t: &mut TileTool, e: &mut Editor, what: &str, key: &str) -> Result<(), String> {
    let make = match what {
        "building" => Make::Place { kind: "BUILDING_INSTANCE".to_owned(), key: key.to_owned() },
        "prefab" => Make::Prefab { key: key.to_owned() },
        kind if FIXED.iter().chain([&ROAD]).any(|(_, k, drawn)| *k == kind && *drawn) => Make::Draw { kind: kind.to_owned() },
        kind if FIXED.iter().any(|(_, k, _)| *k == kind) || KEYED.iter().any(|(_, k)| *k == kind) => Make::Place { kind: kind.to_owned(), key: key.to_owned() },
        kind => return Err(format!("{kind} can't be created here")),
    };

    let label = if key.is_empty() { crate::entity_types::type_name(what).to_owned() } else { key.to_owned() };
    let en = Entry { category: 0, label, make, search: String::new() };
    t.start_making(e, &en);
    Ok(())
}

/// Places or paints a vegetation set from a script, as its Create panel entry does.
pub fn make_set_for_script(t: &mut TileTool, e: &mut Editor, name: &str) -> Result<(), String> {
    if !t.vegetation_sets(e).iter().any(|s| s.name == name) {
        return Err(format!("no vegetation set '{name}'"));
    }

    let en = Entry { category: 1, label: format!("set: {name}"), make: Make::VegSet { name: name.to_owned() }, search: String::new() };
    t.start_making(e, &en);
    Ok(())
}
