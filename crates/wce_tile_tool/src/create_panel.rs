//! The Create panel and its Space popup: everything that can be put in a tile, in one list searched by words. Buildings, trees, decals,
//! models and effects (every valid key, `TileDocument::key_catalogue`), lights, prefabs (`wce_tile::prefabs`), the gameplay markers and
//! areas, the AI regions and hints, rivers and roads. A click on an entry starts placing it (clicks on the terrain, previewed under the
//! mouse; R / Shift+R turn it, Esc stops) or drawing it (clicks; a closed shape ends on its first point, an open line with Enter or a
//! middle click). The shapes of a holder (a deployment zone's regions, a siege AI node's or edge's, a capture location's boundary) are not
//! entries: they are drawn from the holder's Inspector (`inspect_holder`). While it is placed or drawn, the Inspector shows its
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
use wce_tile_editor::tile_document::{BoundaryKind, Create, Draft, turned};

use crate::settings::{RECENT_KEEP, SETTINGS_ID, TileSettings};
use crate::tool::{SetInUse, TileTool, document, primary};

/// The categories, in the panel's order.
pub const CATEGORIES: [&str; 12] = [
    "Buildings",
    "Vegetation",
    "Decals",
    "Models",
    "Effects",
    "Lights",
    "Prefabs",
    "Gameplay",
    "AI & regions",
    "Siege",
    "Rivers & roads",
    "References",
];

/// The categories' icons, for the rail.
const CATEGORY_ICONS: [&str; CATEGORIES.len()] = [
    icons::HOUSE,
    icons::TREE_PINE,
    icons::IMAGE,
    icons::BOX,
    icons::SPARKLES,
    icons::LIGHTBULB,
    icons::GROUP,
    icons::FLAG,
    icons::BRAIN,
    icons::SWORDS,
    icons::WAVES,
    icons::USER,
];

/// The rail's width.
/// A colour of the style, as the draw list takes it.
pub(crate) fn style_u32(idx: i32) -> u32 {
    let c = gui::style_color(idx);
    theme::u32([c.x, c.y, c.z, c.w])
}

/// The side of a picture in the grid view (pixels; the thumbnails are 128).
const GRID_PICTURE: f32 = 96.0;

const RAIL_WIDTH: f32 = 40.0;

/// The folder of the buildings no table has a category for.
const NO_CATEGORY: &str = "no category";

/// The buildings' catch-all category, with subfolders by the key's first word: its culture or style (1 487 buildings with TDD).
const GENERIC: &str = "generic";

/// A first word of fewer keys than this is not a subfolder of its own: they go to [`OTHER_BUILDINGS`].
const MIN_GROUP: usize = 5;
const OTHER_BUILDINGS: &str = "other";

/// The first word of a building's key, for its subfolder: the numbered blocks (`30_30_10`) together, the kit's two spellings as one.
fn building_group(key: &str) -> String {
    let word = key.split('_').next().unwrap_or_default().to_lowercase();
    match word.as_str() {
        "" => "blocks".to_owned(),
        w if w.starts_with(|c: char| c.is_ascii_digit()) => "blocks".to_owned(),
        "west" => "western".to_owned(),
        "cinematics" => "cinematic".to_owned(),
        _ => word,
    }
}

/// How many of the generic buildings (their keys) each first word has.
fn generic_counts<'a>(keys: impl Iterator<Item = &'a str>) -> std::collections::HashMap<String, usize> {
    let mut counts = std::collections::HashMap::new();
    for key in keys {
        *counts.entry(building_group(key)).or_default() += 1;
    }

    counts
}

/// A generic building's subfolder: its first word's, [`OTHER_BUILDINGS`] for the words with few keys.
fn generic_folder(counts: &std::collections::HashMap<String, usize>, key: &str) -> String {
    let group = building_group(key);
    if counts.get(&group).is_some_and(|&n| n >= MIN_GROUP) { group } else { OTHER_BUILDINGS.to_owned() }
}

/// The types without a key of each fixed category (category, type, drawn).
const FIXED: [(usize, &str, bool); 20] = [
    (11, "WceRefHuman", false),
    (11, "WceRefUnit", false),
    (11, "WceRefShip", false),
    (11, "WceMeasureLine", true),
    (11, "WceAngleFinder", true),
    (5, "POINT_LIGHT_INSTANCE", false),
    (7, "CaptureLocationInstance", false),
    (7, "DeploymentZonePolygonInstanceGroup", false),
    (7, "CivilianShelter", false),
    (7, "CivilianDeploymentZone", true),
    (7, "ZONE_POLYGON_INSTANCE", true),
    (7, "LiteBuildingZone", true),
    (9, "SiegeAINodeInstance", false),
    (9, "SiegeAIEdgeInstance", true),
    (8, "AI_HINT_POLYGON_INSTANCE", true),
    (8, "NOGO_REGION_POLYGON_INSTANCE", true),
    (8, "GO_REGION_POLYGON_INSTANCE", true),
    (8, "VegetationExclusionPolygonInstance", true),
    (8, "AISeparatorInstance", true),
    (10, "SplineRiverInstance", true),
];

/// The types made only inside a holder (`wce_tile::parents`), drawn from the holder's Inspector (`TileTool::inspect_holder`): not entries of
/// the Create panel.
const SHAPES: [&str; 3] = ["DEPLOYMENT_ZONE_POLYGON_INSTANCE", "SIEGE_AI_POLYGON_INSTANCE", "CaptureLocationPolygonInstance"];

/// How a drawn Bounding Box becomes the rectangle the game reads (`bounding_rectangle`, docs/re/siege_ai.md).
const BOUNDING_BOX_NOTE: &str = "The game reads the Bounding Box as a rectangle (any size, turned any way): click its first side's two corners, then a     point at its depth (a 4th click is not needed). With 2 points, or more than 4, it is the box around them. Its area is how many units the node holds.";

/// The roads, drawn like the rivers.
const ROAD: (usize, &str, bool) = (10, "SEGMENTED_SPLINE_INSTANCE", true);

/// Not TEd's: a procedural wall (docs/plans/procedural_walls.md), drawn with the siege's types.
const WALL: (usize, &str, bool) = (9, wce_tile_editor::tile_document::WALL_SPLINE, true);

/// Not TEd's: a procedural road (docs/plans/procedural_roads.md), with the roads.
const PROCEDURAL_ROAD: (usize, &str, bool) = (10, wce_tile_editor::tile_document::ROAD_SPLINE, true);

/// The keyed types of the first categories.
const KEYED: [(usize, &str); 5] =
    [(0, "BUILDING_INSTANCE"), (1, "VEGETATION_INSTANCE"), (2, "ProjectedDecalInstance"), (3, "SIMPLE_MESH_INSTANCE"), (4, "VFXInstance")];

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
        Self {
            category: 1,
            label: "new vegetation set".to_owned(),
            make: Make::VegSet { name: String::new() },
            folder: String::new(),
            project: false,
            search: String::new(),
        }
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
pub(crate) const PROJECT_PREFAB_TIP: &str =
    "A prefab of the projects folder (the editor's .wce): the prefabs placed in it stay references, and edits of it are saved there.";

/// The order of an entry in its category: its folder's parts, each before the entries (folders first), then the entry.
pub(crate) fn folder_order(folder: &str) -> Vec<(u8, String)> {
    folder.split('/').filter(|p| !p.is_empty()).map(|p| (0, p.to_lowercase())).chain([(1, String::new())]).collect()
}

impl Entry {
    /// The key it places.
    fn place_key(&self) -> Option<&str> {
        match &self.make {
            Make::Place { key, .. } => Some(key),
            _ => None,
        }
    }
}

/// The rows of a category shown by folder: each run of rows in the same folder, and how many rows each folder holds (its sub-folders' too).
#[derive(Clone, Debug, Default)]
pub(crate) struct FolderRows {
    runs: Vec<(String, Vec<usize>)>,
    counts: std::collections::HashMap<String, usize>,
}

impl FolderRows {
    /// The rows (an index and its folder each, in `folder_order`), by folder.
    pub(crate) fn of<'a>(rows: impl IntoIterator<Item = (usize, &'a str)>) -> Self {
        let mut f = Self::default();
        for (i, folder) in rows {
            match f.runs.last_mut() {
                Some((last, run)) if last == folder => run.push(i),
                _ => f.runs.push((folder.to_owned(), vec![i])),
            }

            let parts = folder.split('/').filter(|p| !p.is_empty()).collect::<Vec<_>>();
            for n in 1..=parts.len() {
                *f.counts.entry(parts[..n].join("/")).or_default() += 1;
            }
        }

        f
    }
}

/// Rows in their folders: a tree node per folder (`id`: what their ids start with; open while a filter is typed), `rows` drawing each run of
/// rows.
pub(crate) fn folder_tree(id: &str, folders: &FolderRows, filtering: bool, mut rows: impl FnMut(&[usize])) {
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
            if !gui::tree_node_ex_id(&format!("{id}/{path}"), gui::TREE_SPAN_AVAIL_WIDTH, &label) {
                closed = Some(parts[..=open.len()].to_vec());
                break;
            }

            open.push(parts[open.len()]);
        }

        if closed.is_none() {
            rows(run);
        }
    }

    for _ in open {
        gui::tree_pop();
    }
}

/// What the entries were made for: the tile and the prefab library's version.
type EntriesStamp = (DocumentId, u64, u64);

/// What the Create panel shows: the entries (made again for another tile or another prefab library), its filter, the rows it let through,
/// the favourites and recent entries (the settings' lists, read once), and the Space popup's filter.
#[derive(Default)]
pub struct CreateUi {
    entries: Option<(EntriesStamp, Arc<Vec<Entry>>)>,

    /// The entries' indices by their ids, made with them (a favourite or a recent one looked for through all of them, each id formatted
    /// again, cost 15 ms a frame with a mod's thousands of buildings: user 2026-11-08).
    entry_ids: Arc<std::collections::HashMap<String, usize>>,
    filter: String,

    /// The type of item the list shows (the rail, `CATEGORIES`); none: only the favourites and the recent ones.
    category: Option<usize>,
    shown: Vec<Vec<usize>>,
    folders: Vec<FolderRows>,
    shown_for: String,
    marks: Option<(Vec<String>, Vec<String>)>,
    quick_filter: String,
    quick_open: bool,

    /// The Space popup's highlighted row (Up / Down move it while typing; Enter takes it).
    quick_row: usize,

    /// The search (the panel's, the popup's) gets the keyboard back on the next frame: Ctrl + Enter added a tree to the new set.
    filter_refocus: bool,
    quick_refocus: bool,

    /// The grid view (`TileSettings::create_grid`, read on first use), the entries it drew last frame (their thumbnails are asked for after
    /// the list: drawing it, the panel cannot), and the thumbnails it has.
    grid: Option<bool>,
    grid_seen: std::cell::RefCell<Vec<usize>>,
    grid_images: std::collections::HashMap<String, wce_editor::images::ImageRef>,

    /// The sticky drafts of each type (its values and turn), and the prefab's turn.
    pub drafts: std::collections::HashMap<String, Draft>,
    pub prefab_yaw: f32,

    /// The prefab being placed, turned: its key, the turn and the text.
    turned: Option<(String, f32, Vec<u8>)>,

    /// What the type being drawn goes into (a holder, `wce_tile::parents`), taken from the selection when it started.
    pub holder: Option<usize>,
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

    /// The surface's normal there: the mouse goes elsewhere meanwhile.
    pub normal: wce_foundation::math::Double3,
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
                if kind == "BUILDING_INSTANCE" {
                    let c = document(e).and_then(|d| d.tile().building_category(key)).unwrap_or_else(|| NO_CATEGORY.to_owned());
                    en.search.push_str(&format!(" {}", c.to_lowercase()));
                    en.folder = c;
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
        for (category, kind, drawn) in FIXED.iter().chain([&ROAD, &WALL, &PROCEDURAL_ROAD]).filter(|(_, kind, _)| has(kind)) {
            let make =
                if *drawn { Make::Draw { kind: (*kind).to_owned() } } else { Make::Place { kind: (*kind).to_owned(), key: String::new() } };
            out.push(entry(*category, crate::entity_types::type_name(kind).to_owned(), make, kind));
        }

        let lib = self.prefab_library(e);
        let projects = self.prefab_folders(e).into_iter().next();
        for p in lib.entries() {
            let project = projects.as_ref().is_some_and(|r| *r == p.root);
            let mut en = entry(
                6,
                p.key.clone(),
                Make::Prefab { key: p.key.clone() },
                &format!("prefab {}{}", p.folder, if project { " project" } else { "" }),
            );
            en.folder.clone_from(&p.folder);
            en.project = project;
            out.push(en);
        }

        // The generic buildings' subfolders.
        let counts = generic_counts(out.iter().filter(|en| en.folder == GENERIC).filter_map(Entry::place_key));
        for en in out.iter_mut().filter(|en| en.folder == GENERIC) {
            if let Some(folder) = en.place_key().map(|key| generic_folder(&counts, key)) {
                en.folder = format!("{GENERIC}/{folder}");
            }
        }

        // By category, then folder (stable: the order within a folder stays the catalogue's).
        out.sort_by_cached_key(|en| (en.category, folder_order(&en.folder)));
        self.create.entry_ids = Arc::new(out.iter().enumerate().map(|(i, en)| (en.id(), i)).collect());
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

    /// The holder a new `kind` goes into: the selected item, or the item above it, that can hold one (`wce_tile::parents`).
    pub(crate) fn selected_holder(e: &Editor, kind: &str) -> Option<usize> {
        let sel = primary(e)?;
        let id = crate::tool::document_id(e)?;
        e.docs.find_as::<wce_tile_editor::tile_document::TileDocument>(id)?.holder_for(kind, sel)
    }

    /// Starts drawing a shape of a holder, from the buttons of the holder's Inspector (`inspect_holder`): `kind` in the role `role` (`None`:
    /// the first the holder lacks), the holder alone selected. The holders' shapes are made only this way (not in the Create panel).
    pub(crate) fn start_shape(&mut self, e: &mut Editor, holder: usize, kind: &str, role: Option<&str>) {
        let Some(id) = crate::tool::document_id(e) else { return };
        e.selection.set(wce_scene::render_scene::SourceRef { doc: id, item: wce_tile_editor::tile_document::item_of(holder) });
        let label = crate::entity_types::type_name(kind).to_owned();
        let en = Entry {
            category: 0,
            label,
            make: Make::Draw { kind: kind.to_owned() },
            folder: String::new(),
            project: false,
            search: String::new(),
        };
        self.start_making(e, &en);
        if let (Some(role), Some(d)) = (role, &mut self.draft) {
            d.attrs.insert("name".to_owned(), role.to_owned());
        }
    }

    /// The buttons of a holder's Inspector (one holder alone selected): one per shape it can be given (`shape_items`).
    pub(crate) fn inspect_holder(&mut self, e: &mut Editor) {
        let sel = crate::tool::selected(e);
        let ([holder], Some(d)) = (sel.as_slice(), document(e)) else { return };
        let holder = *holder;
        if wce_tile::parents::child_kind(d.tile().tscene().type_of(holder)).is_none() {
            return;
        }

        let items = shape_items(d, holder);
        if items.is_empty() {
            return;
        }

        gui::separator();
        gui::text_disabled("Shapes (they are made only here):");
        let mut pick = None;
        for (i, it) in items.iter().enumerate() {
            gui::begin_disabled(it.full.is_some());
            if gui::button(&format!("{}  {}##shape{i}", icons::PENCIL, it.label)) {
                pick = Some(i);
            }

            gui::end_disabled();
            if let Some(why) = it.full.as_deref().filter(|_| gui::is_item_hovered_allow_disabled()) {
                gui::set_tooltip(why);
            }
        }

        // A node: its Boundary made for the ground around it, and how its Bounding Box is read.
        // A capture location: its boundary as a preset shape (TEd's), keeping its size (the boundary's Inspector edits size and turn).
        let mut preset = None;
        if d.tile().tscene().type_of(holder) == "CaptureLocationInstance"
            && let Some(b) = d.capture_boundary(holder)
        {
            // Buttons sized to their names, the boundary's own shape lit (selectables of no width took the whole line: the others were
            // past the panel's edge).
            let now = d.boundary_shape(b).map(|s| s.kind);
            gui::text_disabled("Boundary preset:");
            for kind in [BoundaryKind::Circle, BoundaryKind::Semicircle, BoundaryKind::Square] {
                gui::same_line();
                let on = now == Some(kind);
                if on {
                    gui::push_style_color(gui::COL_BUTTON, gui::style_color(gui::COL_BUTTON_ACTIVE));
                }

                if gui::small_button(&format!("{}##preset", kind.label())) {
                    preset = Some((b, kind));
                }

                if gui::is_item_hovered() {
                    gui::set_tooltip("Makes the capture location's boundary this preset shape (TEd's).");
                }

                if on {
                    gui::pop_style_color(1);
                }
            }
        }

        // A street edge: its two Street boundaries fitted between the nodes it joins.
        let mut fit = false;
        let sc = d.tile().tscene();
        if sc.type_of(holder) == "SiegeAIEdgeInstance" && sc.doc().get(holder, "type").is_none_or(|ty| ty == "SAET_STREET") {
            let joins = d.street_nodes(holder);
            gui::begin_disabled(joins.is_none());
            fit = gui::button(&format!("{}  Fit the street boundaries to the nodes it joins", icons::SQUARE_DASHED));
            gui::end_disabled();
            if gui::is_item_hovered_allow_disabled() {
                gui::set_tooltip(match joins {
                    Some(_) => {
                        "Its two Street boundaries made again as the generator makes them: along its Street between the two nodes it joins, at the corridor's faces (the no-go regions, buildings, fort pieces, steep ground, sea and the other nodes' Boundaries), each end ticked into the node's Boundary. One undo entry."
                    }
                    None => {
                        "It joins no two nodes: its Street's first and last points must be in two nodes' Bounding Boxes (as the game finds them)."
                    }
                });
            }
        }

        let mut adjust = None;
        if d.tile().tscene().type_of(holder) == "SiegeAINodeInstance" {
            if gui::button(&format!("{}  Adjust the Boundary to what is around it", icons::SQUARE_DASHED)) {
                adjust = Some(());
            }

            if gui::is_item_hovered() {
                gui::set_tooltip(
                    "The Boundary made again as the generator makes it: the free ground nearest to the node, within the range, out to the no-go regions, buildings, fort pieces, steep ground and sea around it, the other nodes' Boundaries and halfway to the nodes without one; grown onto the buildings' faces, stopping at the no-go regions' and the other Boundaries' edges. One undo entry.",
                );
            }

            // Its range (kept in the Tile settings).
            let mut reach = e.tool_settings::<TileSettings>(SETTINGS_ID).boundary_reach;
            gui::set_next_item_width(160.0);
            let format = if reach > 0.0 { "%.1f m" } else { "automatic" };
            if gui::drag_float("range (m from its centre; 0: automatic)##boundary_reach", &mut reach, 0.25, 0.0, 200.0, format) {
                e.update_tool_settings(SETTINGS_ID, |s: &mut TileSettings| s.boundary_reach = reach.max(0.0));
            }

            if gui::is_item_hovered() {
                gui::set_tooltip(
                    "How far the adjusted Boundary may reach from the node's centre (at least to its Bounding Box's corners). Automatic: as the generator, 30 to 60 units for an area node by the room around it, 18 to 32 for an intersection, 24 for an entry.",
                );
            }

            gui::text_wrapped(BOUNDING_BOX_NOTE);
        }

        if let Some(it) = pick.map(|i| &items[i]) {
            self.start_shape(e, it.holder, it.kind, it.role);
        }

        let reach = e.tool_settings::<TileSettings>(SETTINGS_ID).boundary_reach;
        if let (Some((b, kind)), Some(d)) = (preset, document(e))
            && let Some(shape) = d.boundary_preset(b, kind)
        {
            self.status = match d.set_boundary_shape(b, shape, Some(holder)) {
                Ok(_) => format!("the boundary is a {} now", kind.label().to_lowercase()),
                Err(err) => err,
            };
        }

        if fit && let Some(d) = document(e) {
            self.status = match d.fit_street_sides(holder) {
                Ok(()) => "the street boundaries fitted between its nodes".to_owned(),
                Err(err) => err,
            };
        }

        if adjust.is_some()
            && let Some(d) = document(e)
        {
            self.status = match d.adjust_node_boundary(holder, Some(reach).filter(|&r| r > 0.0)) {
                Ok(_) => "the Boundary adjusted to what is around the node".to_owned(),
                Err(err) => err,
            };
        }
    }

    /// What is being drawn looks like once made: its colour (a deployment polygon's by its zone's alliance and its type) and whether it is
    /// closed.
    pub(crate) fn drawing_look(&self, e: &Editor, d: &wce_tile_editor::tile_document::TileDocument) -> ([f32; 3], bool) {
        use wce_tile_editor::tile_document::{deployment_colour, deployment_variant, polygon_colour};

        let Some(draft) = &self.draft else { return ([1.0, 0.9, 0.1], false) };
        let kind = draft.kind.as_str();
        let closed = match kind {
            "SIEGE_AI_POLYGON_INSTANCE" => matches!(draft.attrs.get("name").map(String::as_str), Some("Bounding Box" | "Boundary")),
            k => wce_tile_editor::tile_document::FILLED_TYPES.contains(&k),
        };

        let colour = match kind {
            "DEPLOYMENT_ZONE_POLYGON_INSTANCE" => {
                let holder = primary(e).and_then(|s| d.holder_for(kind, s)).or(self.create.holder);
                let alliance = holder.and_then(|h| d.tile().tscene().doc().get(h, "alliance_id"));

                let ty = draft.attrs.get("deployment_polygon_type").map_or("DZPT_ADDITIVE", String::as_str);
                deployment_colour(deployment_variant(&alliance.unwrap_or_default(), ty))
            }

            "SiegeAIEdgeInstance" => polygon_colour("SIEGE_AI_POLYGON_INSTANCE"),
            k => polygon_colour(k),
        };

        (colour, closed)
    }

    /// How what is drawn is finished: a closed shape by a click on its first point (or the finishing shortcut), an open line by the
    /// shortcut (Enter, a middle click: `tile.finish_drawing`).
    fn finish_hint(&self, e: &Editor) -> String {
        let keys = wce_editor::shortcuts::keys("tile.finish_drawing").iter().flatten().map(|c| c.text()).collect::<Vec<_>>().join(" or ");
        let closed = crate::tool::document_id(e)
            .and_then(|id| e.docs.find_as::<wce_tile_editor::tile_document::TileDocument>(id))
            .is_some_and(|d| self.drawing_look(e, d).1);
        match (closed, keys.is_empty()) {
            (true, true) => "click its first point to close it".to_owned(),
            (true, false) => format!("click its first point to close it ({keys} too)"),
            (false, true) => "set a key to finish it in Settings > Shortcuts".to_owned(),
            (false, false) => format!("{keys} finishes it"),
        }
    }

    /// Starts making an entry: placing it, or drawing it.
    pub fn start_making(&mut self, e: &mut Editor, en: &Entry) {
        // A holder's shape goes into the holder selected (`start_shape`).
        if let Make::Draw { kind } = &en.make
            && !wce_tile::parents::holders(kind).is_empty()
            && Self::selected_holder(e, kind).is_none()
        {
            self.status = format!(
                "select {} and use its Inspector's buttons to draw a {}",
                wce_tile::parents::holders_label(kind),
                wce_tile::parents::label(kind)
            );
            return;
        }

        // The new set (not saved) is not one to come back to; nor a holder's shape (not a Create entry).
        let shape = matches!(&en.make, Make::Draw { kind } if !wce_tile::parents::holders(kind).is_empty());
        if !shape && !matches!(&en.make, Make::VegSet { name } if name.is_empty()) {
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
                let set = if name.is_empty() {
                    self.scratch_set.clone()
                } else {
                    self.vegetation_sets(e).iter().find(|s| s.name == *name).cloned()
                };
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
                let mut draft = self.create.drafts.get(kind).cloned().unwrap_or_else(|| Draft::new(kind, ""));

                // Inside the selected holder; a shape's role (its name) one of the holder's, the first it lacks unless one was chosen.
                self.create.holder = Self::selected_holder(e, kind);
                if let (Some(h), Some(d)) = (self.create.holder, document(e))
                    && !d.holder_roles(h).is_empty()
                    && kind != "DEPLOYMENT_ZONE_POLYGON_INSTANCE"
                {
                    let named = draft.attrs.get("name").filter(|n| d.holder_roles(h).iter().any(|r| r.name == n.as_str())).cloned();
                    match d.shape_role(h, named.as_deref()).or_else(|_| d.shape_role(h, None)) {
                        Ok(role) => {
                            draft.attrs.insert("name".to_owned(), role.name.to_owned());
                        }

                        Err(_) => {
                            draft.attrs.remove("name");
                        }
                    }
                }

                self.draft = Some(draft);
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
            Make::Draw { .. } => format!("{}: click the terrain for its points; {}; Esc cancels", en.label, self.finish_hint(e)),
            _ => format!("{}: click the terrain to place (R / Shift+R turn it, Esc stops)", en.label),
        };
    }

    /// Something was picked in the Scene panel: what was being made ends (a line being drawn is finished with the points it has, what was
    /// being placed or painted stops), and the Inspector shows the pick.
    pub fn leave_for_selection(&mut self, e: &mut Editor) {
        if self.drawing && self.draw_pts.len() >= 2 {
            // Finishing selects what it made (a hint the tool takes next frame) and inserts it, which moves the elements after it along:
            // the pick stays, moved with them.
            let picked: Vec<wce_scene::render_scene::SourceRef> = e.selection.items().to_vec();
            let before = document(e).map_or(0, |d| d.tile().tscene().doc().elements().len());
            self.finish_drawing(e);
            if let Some(d) = document(e) {
                let added = d.tile().tscene().doc().elements().len().saturating_sub(before) as u64;
                let at = d.take_selection_hint().and_then(|h| h.first().copied());
                e.selection.clear();
                for mut r in picked {
                    // (A building's piece is not an element: it does not move.)
                    if let Some(at) = at
                        && r.item >= at
                        && wce_tile_editor::tile_document::elem_of(r.item).is_some()
                    {
                        r.item += added;
                    }

                    e.selection.add(r);
                }
            }
        }

        if self.making.is_some() || self.placing.is_some() || self.drawing {
            self.stop_making();
        }

        if self.mode != crate::tool::TileMode::Select {
            self.set_mode(crate::tool::TileMode::Select);
        }

        e.pending_focus = Some("Inspector".to_owned());
    }

    /// Stops placing or drawing.
    pub fn stop_making(&mut self) {
        self.veg_set = None;
        self.placing = None;
        self.drawing = false;
        self.draw_pts.clear();
        self.making = None;
        self.create.holder = None;
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
        let at =
            items.iter().filter_map(|&r| d.item_transform(r)).map(|m| Float3::new(m.m[0][3], m.m[1][3], m.m[2][3])).collect::<Vec<_>>();
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
                let d = crate::tool::document_id(e).and_then(|id| e.docs.find_as::<wce_tile_editor::tile_document::TileDocument>(id));
                let normal = d.map_or(wce_foundation::math::Double3::new(0.0, 1.0, 0.0), |d| self.surface_normal(d));
                self.alt_turn = Some(AltTurn { point, on_terrain, normal, mouse_x: x, yaw });
            }

            return;
        };

        let mut turned = t.yaw + (x - t.mouse_x) * ALT_TURN_DEG_PER_PX;
        if io.KeyShift && self.angle_step > 0.0 {
            turned = (turned / self.angle_step).round() * self.angle_step;
        }

        self.set_placing_yaw(turned);
        self.status = format!(
            "turn {:.1} deg (Alt + mouse; Shift: steps of {} deg; click places it here)",
            turned.rem_euclid(360.0),
            self.angle_step
        );
    }

    /// What the preview turns its text by: the turn being placed (`placing_preview`).
    pub(crate) fn preview_yaw(&self) -> f32 {
        self.placing_yaw().unwrap_or(0.0)
    }

    /// The text of the preview: what a click places, neither turned nor tilted, for the preview to do both by a matrix
    /// ([`Self::preview_yaw`], `preview_normal`): every step of a turn, and of the mouse over the ground, would make it again otherwise.
    pub(crate) fn placing_preview(&mut self, e: &mut Editor) -> Option<Vec<u8>> {
        self.placement_modes(e);
        let text = match self.placing.clone()? {
            crate::tool::Placing::Draft => document(e)?.draft_text(&Draft { yaw: 0.0, ..self.draft.clone()? })?,
            crate::tool::Placing::Prefab(key) => self.prefab_text(e, &key).filter(|t| !t.is_empty())?,
        };

        Some(self.apply_placement_modes_on(text, None))
    }

    /// A prefab's reference text, not turned.
    fn prefab_text(&mut self, e: &mut Editor, key: &str) -> Option<Vec<u8>> {
        if self.placing_prefab_text.as_ref().is_none_or(|(k, _)| k != key) {
            let lib = self.prefab_library(e);
            let text = lib.load(key).map(|p| wce_tile_editor::tile_document::prefab_reference_text(&p.group_text, key)).unwrap_or_default();
            self.placing_prefab_text = Some((key.to_owned(), text));
        }

        self.placing_prefab_text.as_ref().map(|(_, t)| t.clone())
    }

    /// The text of what a click places now.
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
                self.prefab_text(e, &key);

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

        // A holder's shape (a deployment zone's region, a siege AI node's or edge's shape, a capture location's boundary) goes into the
        // holder it was started from (its Inspector's buttons), while it is still one; the selected one else.
        let needs_holder = !wce_tile::parents::holders(kind).is_empty();
        let holder = self.create.holder.filter(|&h| d.holder_for(kind, h) == Some(h)).or_else(|| sel.and_then(|s| d.holder_for(kind, s)));
        let into = match (needs_holder, holder) {
            (true, Some(h)) => Ok(h),
            (true, None) => Err(format!("select {} to draw it in", wce_tile::parents::holders_label(kind))),
            _ => layer.ok_or_else(|| "this tile has no scene graph".to_owned()),
        };

        let added = into.and_then(|into| d.create_with(kind, into, &Create::Line(self.draw_pts.clone()), &draft.key, &draft.attrs));
        self.create.holder = None;
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
                ins.text(
                    "source",
                    "from",
                    if project {
                        "the projects folder: prefabs placed in it stay references"
                    } else {
                        "an Assembly Kit (TEd's): prefabs placed in it are copies"
                    },
                );
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
        let holder = (self.drawing && !wce_tile::parents::holders(&draft.kind).is_empty())
            .then(|| Self::selected_holder(e, &draft.kind).or(self.create.holder))
            .flatten();
        let Some(d) = document(e) else { return false };
        ins.begin(d.id(), u64::MAX - 7);

        // A holder's shape: which of its shapes it is (TEd tells them apart by their names; one of a role it has once replaces it).
        let roles = holder.filter(|_| draft.kind != "DEPLOYMENT_ZONE_POLYGON_INSTANCE").map(|h| d.holder_roles(h)).unwrap_or_default();
        let mut role_changed = false;
        if !roles.is_empty() {
            ins.section(&format!("New {}", wce_tile::parents::label(&draft.kind)));
            let names = roles.iter().map(|r| r.name.to_owned()).collect::<Vec<_>>();
            let tips = roles
                .iter()
                .map(|r| match (r.most, r.closed) {
                    (1, _) => format!("{}: it has one (drawing it again replaces it)", r.name),
                    (n, _) if n > 1 => format!("{}: it has up to {n}", r.name),
                    (_, true) => format!("{}: a closed shape", r.name),
                    _ => format!("{}: an open line", r.name),
                })
                .collect::<Vec<_>>();
            let mut idx = draft.attrs.get("name").and_then(|n| names.iter().position(|x| x == n)).unwrap_or(0) as i32;
            if ins.edit_choice_tips("wce_role", "shape", &mut idx, &names, &tips) {
                draft.attrs.insert("name".to_owned(), names[idx.max(0) as usize].clone());
                role_changed = true;
            }

            if draft.attrs.get("name").is_some_and(|n| n == "Bounding Box") {
                gui::text_wrapped(BOUNDING_BOX_NOTE);
            }
        }

        if d.inspect_draft(&mut draft, !self.drawing, ins) || role_changed {
            self.draft = Some(draft.clone());
            self.keep_draft();
        }

        if gui::button(&format!("{}  Reset to the defaults", icons::UNDO_2))
            && let Some(dr) = &mut self.draft
        {
            let role = dr.attrs.get("name").cloned().filter(|_| !roles.is_empty());
            dr.attrs.clear();
            dr.attrs.extend(role.map(|r| ("name".to_owned(), r)));
            dr.yaw = 0.0;
            self.keep_draft();
        }

        if gui::is_item_hovered() {
            gui::set_tooltip("Puts the settings of what you are making back to their defaults, its turn too.");
        }

        // Where it goes: the holder's own values, as they are now (changed here as when it is selected).
        match holder {
            Some(h) if self.drawing => {
                let sc = d.tile().tscene();
                gui::separator();
                gui::text_disabled(&format!("Inside the {} {}", wce_tile::parents::label(sc.type_of(h)), sc.name(h)));
                let item = wce_scene::render_scene::SourceRef { doc: d.id(), item: wce_tile_editor::tile_document::item_of(h) };
                ins.begin(item.doc, item.item);
                d.inspect(item, ins);
            }

            _ => {}
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

        self.create.folders =
            self.create.shown.iter().map(|rows| FolderRows::of(rows.iter().map(|&i| (i, entries[i].folder.as_str())))).collect();

        self.create.shown_for = stamp;
    }

    /// The models an entry places, for its thumbnail: a building, a tree, a model or a prefab (nothing for the others).
    fn thumbnail_parts(&mut self, e: &mut Editor, en: &Entry) -> Vec<(wce_foundation::math::Float4x4, wce_scene::render_scene::Payload)> {
        let text = match &en.make {
            Make::Place { kind, key }
                if !key.is_empty()
                    && matches!(kind.as_str(), "BUILDING_INSTANCE" | "VEGETATION_INSTANCE" | "SIMPLE_MESH_INSTANCE" | "VFXInstance") =>
            {
                document(e).and_then(|d| d.draft_text(&Draft::new(kind, key)))
            }

            Make::Prefab { key } => {
                let lib = self.prefab_library(e);
                lib.load(key).map(|p| wce_tile_editor::tile_document::prefab_reference_text(&p.group_text, key)).ok()
            }

            _ => None,
        };

        match (text, document(e)) {
            (Some(text), Some(d)) => d.tile().item_parts(&text),
            _ => vec![],
        }
    }

    /// Whether an entry can have a thumbnail.
    fn has_thumbnail(en: &Entry) -> bool {
        match &en.make {
            Make::Place { kind, key } => {
                !key.is_empty()
                    && matches!(kind.as_str(), "BUILDING_INSTANCE" | "VEGETATION_INSTANCE" | "SIMPLE_MESH_INSTANCE" | "VFXInstance")
            }
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

    /// A prefab's thumbnail, the Create panel's (one cache, `prefab|<key>`): `Some` once it is on the GPU. A missing one is made only while a
    /// tile document is open (the stage, or the Tile tool's tile: drawing it needs one); the ones made before show either way.
    pub(crate) fn prefab_thumbnail(&mut self, e: &mut Editor, key: &str) -> Option<wce_editor::images::ImageRef> {
        let en = Entry {
            category: 0,
            label: key.to_owned(),
            make: Make::Prefab { key: key.to_owned() },
            folder: String::new(),
            project: false,
            search: String::new(),
        };
        if document(e).is_some() {
            return self.thumbnail(e, &en, false);
        }

        let dir = crate::thumbnails::cache_dir(e);
        if self.thumbs.dir != dir {
            self.thumbs = crate::thumbnails::Thumbnails::default();
            self.thumbs.dir = dir;
        }

        self.thumbs.get(e, &en.id())
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

    /// A cell of the grid: the entry's picture (its icon until there is one) over its name, lit while it is made, starred in a corner. A right
    /// click stars or unstars it. Returns (clicked, star clicked, hovered).
    fn create_cell(&self, i: usize, en: &Entry, favourite: bool, uid: &str) -> (bool, bool, bool) {
        let on = self.making.as_deref() == Some(en.label.as_str());
        let (side, line) = (GRID_PICTURE, gui::text_line_height_with_spacing());
        let at = gui::cursor_screen_pos();
        let clicked = gui::selectable_sized(&format!("##cell{uid}"), on, 0, gui::v2(side, side + line));

        let hovered = gui::is_item_hovered();
        let star = hovered && gui::is_item_clicked_with(1);
        self.create.grid_seen.borrow_mut().push(i);
        let dl = gui::window_draw_list();
        let (pmin, pmax) = (gui::v2(at.x + 2.0, at.y + 2.0), gui::v2(at.x + side - 2.0, at.y + side - 2.0));
        match self.create.grid_images.get(&en.id()) {
            Some(img) => dl.image(img.id, pmin, pmax, gui::v2(0.0, 0.0), gui::v2(1.0, 1.0), 0xFFFF_FFFF),
            // No model to picture (a line, a polygon, a marker): its icon over its name, wrapped in the picture.
            None if !Self::has_thumbnail(en) => {
                dl.rect_filled(pmin, pmax, style_u32(gui::COL_FRAME_BG), 4.0);
                let lines = wrap_name(&en.label, side - 12.0, 4);
                let (icon, lh) = (gui::calc_text_size(en.icon()), gui::text_line_height());
                let block = icon.y + 4.0 + lh * lines.len() as f32;
                let mut y = at.y + (side - block) * 0.5;
                dl.text(gui::v2(at.x + (side - icon.x) * 0.5, y), style_u32(gui::COL_TEXT_DISABLED), en.icon());
                y += icon.y + 4.0;
                for l in &lines {
                    let w = gui::calc_text_size(l).x;
                    dl.text(gui::v2(at.x + (side - w) * 0.5, y), style_u32(gui::COL_TEXT), l);
                    y += lh;
                }
            }

            None => {
                dl.rect_filled(pmin, pmax, style_u32(gui::COL_FRAME_BG), 4.0);
                let size = gui::calc_text_size(en.icon());
                dl.text(gui::v2(at.x + (side - size.x) * 0.5, at.y + (side - size.y) * 0.5), style_u32(gui::COL_TEXT_DISABLED), en.icon());
            }
        }

        // The name, cut to the cell.
        let mut name = en.label.clone();
        while name.chars().count() > 3 && gui::calc_text_size(&name).x > side - 4.0 {
            name.pop();
            name.pop();
            name.push('…');
        }

        let colour = if en.project { theme::palette().success } else { theme::text() };
        dl.text(gui::v2(at.x + 2.0, at.y + side), theme::u32(colour), &name);
        if favourite {
            let h = theme::palette().highlight;
            // On a dark disc: over the pictures' pale skies it did not show.
            let size = gui::calc_text_size(icons::STAR);
            let c = gui::v2(at.x + side - 4.0 - size.x * 0.5, at.y + 4.0 + size.y * 0.5);
            dl.circle_filled(c, size.x.max(size.y) * 0.5 + 3.0, theme::u32([0.08, 0.08, 0.09, 0.85]));
            dl.text(gui::v2(c.x - size.x * 0.5, c.y - size.y * 0.5), theme::u32(h), icons::STAR);
        }

        (clicked, star, hovered)
    }

    /// Rows as cells, as many to a line as the panel fits.
    fn create_cells(
        &self,
        rows: &[usize],
        entries: &[Entry],
        favourites: &[String],
        uid: &str,
        mut take: impl FnMut(usize, (bool, bool, bool)),
    ) {
        let spacing = gui::item_spacing().x;
        let across = (((gui::content_region_avail().x + spacing) / (GRID_PICTURE + spacing)) as usize).max(1);
        let lines = rows.len().div_ceil(across);
        gui::clipped(lines, |l| {
            for (k, &i) in rows[l * across..((l + 1) * across).min(rows.len())].iter().enumerate() {
                if k > 0 {
                    gui::same_line();
                }

                let en = &entries[i];
                take(i, self.create_cell(i, en, favourites.contains(&en.id()), &format!("{uid}{i}")));
            }
        });
    }

    /// Shows the Create panel as a grid of pictures or a list, and keeps that.
    pub fn set_create_grid(&mut self, e: &mut Editor, on: bool) {
        self.create.grid = Some(on);
        e.update_tool_settings(SETTINGS_ID, |s: &mut TileSettings| s.create_grid = on);
    }

    /// Asks for the thumbnails of the cells drawn: those in view get theirs.
    fn grid_thumbnails(&mut self, e: &mut Editor, entries: &[Entry]) {
        let seen = std::mem::take(&mut *self.create.grid_seen.borrow_mut());
        for i in seen {
            let en = &entries[i];
            if !Self::has_thumbnail(en) || self.create.grid_images.contains_key(&en.id()) {
                continue;
            }

            if let Some(img) = self.thumbnail(e, en, false) {
                self.create.grid_images.insert(en.id(), img);
            }
        }
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
        if gui::is_item_hovered() {
            gui::set_tooltip(if favourite { "Takes it out of the favourites." } else { "Adds it to the favourites." });
        }

        gui::pop_style_color(2);
        (clicked, star, hovered)
    }

    /// Sets the Create panel's search (a script).
    /// Chooses the type of item the Create panel lists, by its name (none: only the favourites and the recent ones).
    ///
    /// # Errors
    ///
    /// No such type.
    pub fn set_create_category(&mut self, name: Option<&str>) -> Result<(), String> {
        self.create.category = match name {
            Some(name) => Some(
                CATEGORIES
                    .iter()
                    .position(|c| c.eq_ignore_ascii_case(name))
                    .ok_or_else(|| format!("no type '{name}': {}", CATEGORIES.join(", ")))?,
            ),
            None => None,
        };

        Ok(())
    }

    /// The rail: an icon per type of item (its name and count in the tooltip). One chooses what the list shows, again goes back to none.
    fn draw_category_rail(&mut self) {
        for (c, title) in CATEGORIES.iter().enumerate() {
            let on = self.create.category == Some(c);
            if on {
                gui::push_style_color(gui::COL_BUTTON, gui::style_color(gui::COL_BUTTON_ACTIVE));
            }

            if gui::button_sized(&format!("{}##createtype{c}", CATEGORY_ICONS[c]), gui::v2(-1.0, 0.0)) {
                self.create.category = (!on).then_some(c);
            }

            if on {
                gui::pop_style_color(1);
            }

            if gui::is_item_hovered() {
                gui::set_tooltip(&format!("{title} ({})", self.create.shown.get(c).map_or(0, Vec::len)));
            }
        }
    }

    pub fn set_create_filter(&mut self, words: &str) {
        words.clone_into(&mut self.create.filter);
    }

    /// The Create panel.
    pub(crate) fn draw_create_panel(&mut self, e: &mut Editor) {
        gui::set_next_window_size(gui::v2(360.0, 480.0), gui::COND_FIRST_USE_EVER);
        if let Some(dock) = self.left_dock.filter(|&d| d != 0) {
            gui::set_next_window_dock_id(dock, gui::COND_FIRST_USE_EVER);
        }

        if self.create_focus_frames > 0 {
            self.create_focus_frames -= 1;
            gui::set_window_focus("Create");
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

        // A set from the selected trees.
        if self.trees_selected(e) {
            if gui::small_button(&format!("{}  New vegetation set from the selected trees", icons::TREES)) {
                self.request_new_vegetation_set();
            }

            if gui::is_item_hovered() {
                gui::set_tooltip("Saves the selected trees as a vegetation set, each kind weighted by how many there are.");
            }
        }

        gui::set_next_item_width(-1.0);
        if std::mem::take(&mut self.create.filter_refocus) {
            gui::set_keyboard_focus_here(0);
        }

        let enter = gui::input_text(
            "##createfilter",
            Some(&format!(
                "{}  search everything (Enter: the first; Space in the viewport; Ctrl + click / Ctrl + Enter a tree: into a new set)",
                icons::SEARCH
            )),
            &mut self.create.filter,
            128,
            gui::INPUT_ENTER_RETURNS_TRUE,
        );
        let entries = self.create_entries(e);
        self.filter_entries(&entries);

        // Enter takes the first entry found (Ctrl + Enter on a tree: into the new set, and the search keeps the keyboard).
        let first = (enter && !self.create.filter.trim().is_empty())
            .then(|| self.create.shown.iter().find_map(|rows| rows.first().copied()))
            .flatten();
        if let Some(i) = first {
            self.create.filter_refocus = gui::io().KeyCtrl && self.set_entries_of(e, &entries[i]).is_some();
        }

        let words = words(&self.create.filter);
        let (favourites, recent) = self.create_marks(e);
        let ids = self.create.entry_ids.clone();
        let by_id = |id: &str| ids.get(id).copied();

        // A search looks through everything; otherwise the list is the chosen type's.
        let filtering = !words.is_empty();
        let only = self.create.category.filter(|_| !filtering);

        // The view: a list, or a grid of pictures.
        let grid = *self.create.grid.get_or_insert_with(|| e.tool_settings::<TileSettings>(SETTINGS_ID).create_grid);
        for (on, icon, tip) in [
            (false, icons::LIST_TREE, "A list"),
            (true, icons::LAYOUT_GRID, "Pictures: each entry with its thumbnail, made as it comes into view"),
        ] {
            if gui::selectable_sized(&format!("{icon}##view{on}"), grid == on, 0, gui::v2(gui::frame_height(), 0.0)) && grid != on {
                self.create.grid = Some(on);
                e.update_tool_settings(SETTINGS_ID, |s: &mut TileSettings| s.create_grid = on);
            }

            if gui::is_item_hovered() {
                gui::set_tooltip(tip);
            }

            gui::same_line();
        }

        // Thumbnails of the list (made in the background, kept in the cache folder).
        if self.thumbs.busy() {
            gui::text_disabled(&format!("{}  thumbnails: {} of {}", icons::IMAGE, self.thumbs.made, self.thumbs.asked));
            gui::same_line();
            if gui::small_button("Cancel##thumbs") {
                self.thumbs.cancel();
            }

            if gui::is_item_hovered() {
                gui::set_tooltip("Stops making the thumbnails; the ones made are kept.");
            }
        } else if gui::small_button(&format!("{}  Make thumbnails of the list", icons::IMAGE)) {
            let rows = self
                .create
                .shown
                .iter()
                .flatten()
                .copied()
                .filter(|&i| Self::has_thumbnail(&entries[i]) && only.is_none_or(|c| entries[i].category == c))
                .collect::<Vec<_>>();
            for i in rows {
                self.thumbnail(e, &entries[i], false);
            }
        }

        if gui::is_item_hovered() {
            gui::set_tooltip(
                "Renders a picture of every building, tree, model and prefab the list shows (filter it first: there are thousands), kept in the cache folder. Hover an entry to see its picture; one hovered without a picture is made then.",
            );
        }

        let mut start = first;
        let mut star = None;
        let mut hovered = None;

        // What is being made goes under the list, which gives up the room at its end: what was clicked stays under the mouse.
        let making = self.making.is_some() && (self.placing.is_some() || self.drawing);
        let footer = if making { 2.0 * gui::text_line_height_with_spacing() + gui::frame_height_with_spacing() + 8.0 } else { 0.0 };
        if gui::begin_child("createrail", gui::v2(RAIL_WIDTH, -footer), 0, gui::WINDOW_NO_SCROLLBAR) {
            self.draw_category_rail();
        }

        gui::end_child();
        gui::same_line_touching();
        if gui::begin_child("createlist", gui::v2(0.0, -footer), 0, 0) {
            wce_foundation::wce_prof!("tile.panel.create.list");
            // Favourites and recent entries (filtered alike), then the categories (open while a filter is typed).
            for (title, icon, ids) in [("Favourites", icons::STAR, &favourites), ("Recent", icons::HISTORY, &recent)] {
                let rows = ids
                    .iter()
                    .filter_map(|id| by_id(id))
                    .filter(|&i| passes(&entries[i], &words) && only.is_none_or(|c| entries[i].category == c))
                    .collect::<Vec<_>>();
                if rows.is_empty() {
                    continue;
                }

                if grid
                    && gui::tree_node_ex_id(
                        &format!("create_{title}"),
                        gui::TREE_DEFAULT_OPEN | gui::TREE_SPAN_AVAIL_WIDTH,
                        &format!("{icon}  {title} ({})", rows.len()),
                    )
                {
                    self.create_cells(&rows, &entries, &favourites, title, |i, (c, s, h)| {
                        if c {
                            start = Some(i);
                        }

                        if h {
                            hovered = Some(i);
                        }

                        if s {
                            star = Some(i);
                        }
                    });

                    gui::tree_pop();
                } else if !grid
                    && gui::tree_node_ex_id(
                        &format!("create_{title}"),
                        gui::TREE_DEFAULT_OPEN | gui::TREE_SPAN_AVAIL_WIDTH,
                        &format!("{icon}  {title} ({})", rows.len()),
                    )
                {
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

            if !filtering && only.is_none() && favourites.is_empty() && recent.is_empty() {
                gui::text_disabled("Choose a type of item on the left, or search.");
            }

            for (c, title) in CATEGORIES.iter().enumerate() {
                if !filtering && only != Some(c) {
                    continue;
                }

                let rows = self.create.shown.get(c).cloned().unwrap_or_default();
                if rows.is_empty() && !(c == 6 && !filtering) {
                    continue;
                }

                // Only a search has several types to tell apart.
                if filtering {
                    gui::set_next_item_open(true, gui::COND_ALWAYS);
                    if !gui::tree_node_ex_id(&format!("create_cat{c}"), gui::TREE_SPAN_AVAIL_WIDTH, &format!("{title} ({})", rows.len())) {
                        continue;
                    }
                }

                if c == 6 && rows.is_empty() {
                    gui::text_wrapped(
                        "No prefabs. TEd keeps them in <Assembly Kit>/raw_data/art/prefabs; new ones are saved in the projects folder's assembly_kit/raw_data/art/prefabs.",
                    );
                }

                let folders = self.create.folders.get(c).cloned().unwrap_or_default();
                let t = &*self;
                folder_tree(&format!("create_cat{c}"), &folders, filtering, |rows| {
                    if grid {
                        t.create_cells(rows, &entries, &favourites, "", |i, (cl, s, h)| {
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

                        return;
                    }

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

                if filtering {
                    gui::tree_pop();
                }
            }
        }

        if grid {
            self.grid_thumbnails(e, &entries);
            if let Some(i) = hovered
                && gui::begin_tooltip()
            {
                gui::text(&entries[i].label);
                gui::text_disabled("right click: star");
                gui::end_tooltip();
            }
        } else if let Some(i) = hovered {
            self.thumbnail_tooltip(e, &entries[i]);
        }

        gui::end_child();
        if let Some(label) = self.making.clone().filter(|_| self.placing.is_some() || self.drawing) {
            gui::text_colored(
                theme::palette().highlight,
                &format!("{}  {label}", if self.drawing { icons::SQUARE_PEN } else { icons::MOUSE_POINTER_2 }),
            );
            if self.drawing {
                gui::text_disabled(&format!("{} points: {}", self.draw_pts.len(), self.finish_hint(e)));
            } else {
                gui::text_disabled("click the terrain to place; R / Shift+R turn it");
            }

            if gui::button("Stop (Esc)") {
                self.stop_making();
            }

            if gui::is_item_hovered() {
                gui::set_tooltip(if self.drawing {
                    "Stops drawing; points not finished are dropped."
                } else {
                    "Stops placing; what was placed stays."
                });
            }
        }
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
    /// With `keep_focus` (the Space popup staying open, a search after Ctrl + Enter) the Inspector is not focused: that would close the
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
            Make::Place { kind, key } if kind == "VEGETATION_INSTANCE" && !key.is_empty() => {
                Some(vec![wce_tile::vegetation_sets::SetEntry { key: key.clone(), weight: 1.0, scale: None }])
            }
            Make::VegSet { name } => self.vegetation_sets(e).iter().find(|s| s.name == *name).map(|s| s.entries.clone()),
            _ => None,
        }
    }

    /// Asks for the Space popup (it opens on the next frame, at the mouse).
    pub fn request_quick_add(&mut self) {
        self.create.quick_open = true;
        self.create.quick_filter.clear();
        self.create.quick_row = 0;
    }

    /// The Space popup: the same search, the first rows; Enter takes the first.
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
        let enter = gui::input_text(
            "##quickfilter",
            Some(&format!("{}  add... (words are and-ed; Up / Down, Enter)", icons::SEARCH)),
            &mut self.create.quick_filter,
            128,
            gui::INPUT_ENTER_RETURNS_TRUE,
        );
        if self.create.quick_filter != before {
            self.create.quick_row = 0;
        }

        let entries = self.create_entries(e);
        let words = words(&self.create.quick_filter);
        let (_, recent) = self.create_marks(e);

        // With no words: the recent entries; else every match.
        let rows = if words.is_empty() {
            recent.iter().filter_map(|id| self.create.entry_ids.get(id).copied()).collect::<Vec<_>>()
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
        kind if FIXED.iter().chain([&ROAD, &WALL, &PROCEDURAL_ROAD]).any(|(_, k, drawn)| *k == kind && *drawn)
            || SHAPES.contains(&kind) =>
        {
            Make::Draw { kind: kind.to_owned() }
        }
        kind if FIXED.iter().any(|(_, k, _)| *k == kind) || KEYED.iter().any(|(_, k)| *k == kind) => {
            Make::Place { kind: kind.to_owned(), key: key.to_owned() }
        }
        kind => return Err(format!("{kind} can't be created here")),
    };

    let label = if key.is_empty() { crate::entity_types::type_name(what).to_owned() } else { key.to_owned() };
    let en = Entry { category: 0, label, make, folder: String::new(), project: false, search: String::new() };
    t.start_making(e, &en);

    // A holder's shape goes only into the holder selected (as its Inspector's buttons start it).
    if SHAPES.contains(&what) && !t.drawing {
        return Err(t.status.clone());
    }

    Ok(())
}

/// Places or paints a vegetation set from a script, as its Create panel entry does.
pub fn make_set_for_script(t: &mut TileTool, e: &mut Editor, name: &str) -> Result<(), String> {
    if !t.vegetation_sets(e).iter().any(|s| s.name == name) {
        return Err(format!("no vegetation set '{name}'"));
    }

    let en = Entry {
        category: 1,
        label: format!("set: {name}"),
        make: Make::VegSet { name: name.to_owned() },
        folder: String::new(),
        project: false,
        search: String::new(),
    };
    t.start_making(e, &en);
    Ok(())
}

#[cfg(test)]
mod generic_tests {
    use super::*;

    #[test]
    fn generic_buildings_go_by_their_first_word_and_the_rare_ones_together() {
        let mut keys = (0..5).map(|n| format!("roman_villa_{n}")).collect::<Vec<_>>();
        keys.extend((0..3).map(|n| format!("west_house_{n}")));
        keys.extend((0..2).map(|n| format!("western_house_{n}")));
        keys.extend(["30_30_10", "60_60_30", "90_30_10", "120_30_10", "_odd"].map(str::to_owned));
        keys.push("colossus_of_rhodes".to_owned());
        let counts = generic_counts(keys.iter().map(String::as_str));
        let group = |key: &str| generic_folder(&counts, key);
        assert_eq!(group("roman_villa_3"), "roman");
        assert_eq!(group("west_house_1"), "western", "one word for both spellings");
        assert_eq!(group("western_house_0"), "western");
        assert_eq!(group("30_30_10"), "blocks");
        assert_eq!(group("colossus_of_rhodes"), OTHER_BUILDINGS);
    }
}

/// A button drawing a shape of a holder (`shape_items`).
pub(crate) struct ShapeItem {
    pub label: String,
    pub holder: usize,
    pub kind: &'static str,
    pub role: Option<&'static str>,

    /// It has as many of that shape as it may (a street edge's two boundaries): why it can't be drawn.
    pub full: Option<String>,
}

/// What can be drawn in the holder `elem` is, or is in (a deployment zone's regions, a siege AI node's or edge's shapes, a capture
/// location's boundary): one item per role, a button of the holder's Inspector (`TileTool::inspect_holder`).
pub(crate) fn shape_items(d: &wce_tile_editor::tile_document::TileDocument, elem: usize) -> Vec<ShapeItem> {
    const KINDS: [&str; 3] = ["DEPLOYMENT_ZONE_POLYGON_INSTANCE", "SIEGE_AI_POLYGON_INSTANCE", "CaptureLocationPolygonInstance"];
    let Some((holder, kind)) = KINDS.iter().find_map(|&k| d.holder_for(k, elem).map(|h| (h, k))) else { return vec![] };
    if kind == "DEPLOYMENT_ZONE_POLYGON_INSTANCE" {
        return vec![ShapeItem { label: "Draw a deployment region".to_owned(), holder, kind, role: None, full: None }];
    }

    let sc = d.tile().tscene();
    let kids = sc.children(holder);
    d.holder_roles(holder)
        .iter()
        .map(|r| {
            let count = kids.iter().filter(|&&c| sc.name(c) == r.name).count();
            let has = count > 0;
            let full = (r.most > 1 && count >= r.most)
                .then(|| format!("It has its {} {}s: delete one to draw another.", r.most, r.name.to_lowercase()));
            let label = match r.most {
                1 if has => format!("Draw its {} again (replaces it)", r.name),
                1 => format!("Draw its {}", r.name),
                _ => format!("Draw a {}", r.name.to_lowercase()),
            };

            ShapeItem { label, holder, kind, role: Some(r.name), full }
        })
        .collect()
}

/// A name broken into lines of at most `width` pixels at its spaces (a word longer than a line is cut), `max` lines at most, the last one
/// ending in `…` when the name goes on.
fn wrap_name(name: &str, width: f32, max: usize) -> Vec<String> {
    let fits = |t: &str| gui::calc_text_size(t).x <= width;
    let cut = |t: &str| {
        let mut t = t.to_owned();
        while t.chars().count() > 1 && !fits(&format!("{t}…")) {
            t.pop();
        }

        format!("{t}…")
    };

    let mut lines: Vec<String> = vec![];
    for w in name.split_whitespace() {
        let joined = lines.last().map(|l| format!("{l} {w}"));
        match joined {
            Some(j) if fits(&j) => *lines.last_mut().expect("a line") = j,
            _ if lines.len() == max => {
                let last = lines.pop().unwrap_or_default();
                lines.push(cut(&format!("{last} {w}")));
                return lines;
            }

            _ => lines.push(if fits(w) { w.to_owned() } else { cut(w) }),
        }
    }

    lines
}
