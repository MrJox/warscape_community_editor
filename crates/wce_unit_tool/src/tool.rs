//! The Unit tool: a variant mesh definition (VMD) open as the document, worn by a unit of the DB standing on a plain ground, looping one of
//! its animations. What the unit is ("Preview as") gives the skeleton, the animations, the mount, the scale and the colours; the edited file
//! takes the place of the unit's own (or its mount's, when it is the mount's). The soldiers are picked by a seed as the game picks them (the
//! editor's generator, not the game's), and pins force what a slot shows, so the node being edited is always in view.
//!
//! The scene is shared by every tool: while this one is active, the other tools' layers are hidden, and shown again when another tool is.

use std::any::Any;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::mpsc::{Receiver, TryRecvError, channel};

use rpfm_lib::schema::Schema;
use serde::{Deserialize, Serialize};
use wce_editor::editor::Editor;
use wce_editor::gui::{self, v2};
use wce_editor::pickers::{pick_button, search_combo};
use wce_editor::theme;
use wce_editor::tool::Tool;
use wce_editor_core::command_registry::{CommandRegistry, CommandResult};
use wce_editor_core::document::Document;
use wce_editor_core::inspector::Inspector;
use wce_formats::variant_mesh::{Alternative, Node, NodePath, VariantMesh, VariantMeshFile};
use wce_formats::xml_doc::XmlDoc;
use wce_foundation::log;
use wce_foundation::math::{Float3, Float4x4, translation};
use wce_foundation::settings::rpfm_schema_path;
use wce_render::skinned_geometry::{PartGeometry, ray_triangle};
use wce_render::unit_animation::UnitModel;
use wce_render::unit_assembly::{Pins, SlotPin, assemble_with, attachment_points, decals};
use wce_render::unit_catalog::{UnitCatalog, variant_path};
use wce_scene::environment::{EnvironmentParams, read_environment};
use wce_scene::render_scene::{DocumentId, EntityId, LayerId, Overlay, Payload, SourceRef};
use wce_vfs::normalize;

#[path = "ship.rs"]
mod ship;
#[path = "structure.rs"]
mod structure;
#[path = "thumbs.rs"]
mod thumbs;
#[path = "weather.rs"]
mod weather;

use crate::db::{LooksTables, Row, project_path};
use crate::document::{Source, Structural, VmdDocument};
use crate::problems::{Problem, WEAPON_POINTS, check};
use crate::slot_kinds::{Field, SlotKinds};

/// The Unit tool's id: the owner of its documents, its settings' table.
pub const UNIT_TOOL_ID: &str = "unit";

/// Where the units' definitions are; the parts' own (what references name) are in the rest of `variantmeshes/`.
const DEFINITIONS: &str = "variantmeshes/variantmeshdefinitions/";

/// Each game's default unit and faction, for a definition no unit wears.
fn default_unit(game: &str) -> (&'static str, &'static str) {
    match game {
        "attila" => ("att_rom_legio_comitatenses", "att_fact_western_roman_empire"),
        "rome_2" => ("Rom_Hastati", "rom_rome"),
        _ => ("", ""),
    }
}

/// The tool's settings, per game.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
struct UnitSettings {
    /// The weather that lights the stage (empty: the game's default).
    weather: String,

    /// The weather chips' filters (category, value; empty: any): climate and season pick the ground's grass too.
    weather_query: BTreeMap<String, String>,

    /// Per definition (its path in the packs), the unit and faction it was last previewed as.
    preview: BTreeMap<String, (String, String)>,
}

/// The soldiers to load: a unit as a faction has it, `count` seeds from `seed`, playing `slot`.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
struct Wanted {
    unit: String,
    faction: String,
    seed: u64,
    count: u64,
    slot: String,
}

type Loaded = (Wanted, Result<Vec<UnitModel>, String>);

/// Lists of the packs' files, for the pickers: read once a game.
#[derive(Default)]
struct Listings {
    definitions: Vec<String>,
    models: Vec<String>,
    textures: Vec<String>,
    weathers: Vec<String>,
}

/// A change to the previewed unit's DB rows.
enum LooksEdit {
    /// Columns of its `unit_variants` row.
    Set(Vec<(&'static str, String)>),

    /// Its own colours for the faction (primary, secondary, tertiary), or none.
    Colours(Option<[[f32; 3]; 3]>),

    /// The project's rows of it taken away.
    Revert,
}

/// The item of the selection outline's mask in the document's scene (the soldiers and the ground are item 0).
const MASK_ITEM: u64 = 1;

/// One instance drawn this frame: where, what, and the figure's parts its parts show.
struct Posed {
    world: Float4x4,
    instance: wce_scene::render_scene::SkinnedInstance,
    sources: Vec<Option<usize>>,
    mount: bool,
    soldier: usize,
}

/// What the panels asked for this frame, done after them.
enum Action {
    Select(Option<NodePath>),
    Edit(Structural, usize),

    /// A slot's new name.
    Rename(usize, String),

    /// A node to another place among its siblings.
    Reorder(NodePath, usize),

    /// What a slot shows in the preview (`None`: the seed's pick).
    Pin(NodePath, Option<SlotPin>),
    Open(String),
    OpenUnit(String, String),
}

pub struct UnitTool {
    /// The weather chips' database.
    weather: weather::StageWeather,

    /// The grass the stage was last given, and for which document.
    ground: Option<(DocumentId, String)>,

    settings: UnitSettings,
    catalog: Option<Arc<UnitCatalog>>,
    catalog_rx: Option<Receiver<Result<(UnitCatalog, Arc<Schema>), String>>>,
    schema: Option<Arc<Schema>>,

    /// The DB rows of the unit's looks (read when the section opens), and the height being dragged.
    looks: Option<LooksTables>,
    looks_error: String,
    height_draft: Option<[f32; 2]>,
    copy_name: String,
    catalog_failed: bool,
    status: String,

    /// The document shown, its path in the packs, and the version of it the soldiers wear.
    doc: DocumentId,
    preview_path: String,
    worn: Option<u64>,

    /// Preview as.
    unit: String,
    faction: String,
    seed: u64,
    grid: u64,
    slot: String,
    playing: bool,
    time: f32,
    clock: Option<f64>,

    /// The colours instead of the unit's (primary, secondary, uniform).
    colours: Option<[[f32; 3]; 3]>,
    pins: Pins,

    /// Pin the slots above the selected node to it.
    show_selection: bool,

    /// Only the selected node's parts.
    isolate: bool,

    selected: Option<NodePath>,

    models: Vec<UnitModel>,
    models_for: Option<Wanted>,
    loading: Option<Receiver<Loaded>>,
    failed: HashSet<Wanted>,

    /// What the soldiers' parts were made from: document version, pins, selection, models.
    parts_key: Option<u64>,
    missing: Vec<String>,

    /// How the soldiers wear the document.
    worn_how: String,

    entities: Vec<EntityId>,
    layer: Option<(DocumentId, LayerId)>,

    /// What this frame drew, for picking and the outline; whether the mount wears the document (else the man).
    posed: Vec<Posed>,
    dressed_mount: bool,

    /// The parts' models as triangles, read when first picked or outlined.
    geometry: HashMap<String, Option<Arc<PartGeometry>>>,

    /// The selection outline's mask (`MASK_ITEM`), and the selection it shows.
    mask: Option<EntityId>,
    mask_for: Option<(Option<NodePath>, u64)>,

    /// Other tools' layers this tool hid, and its own layers while another tool is active.
    hidden: Vec<LayerId>,
    own_hidden: Vec<LayerId>,

    environment: Option<(String, EnvironmentParams)>,
    listings: Option<Listings>,
    problems: Vec<Problem>,
    problems_for: Option<(DocumentId, u64)>,

    /// The camera was aimed at this document.
    aimed: DocumentId,

    /// The unit is the game's default because the tables weren't read yet: chosen again once they are.
    default_pending: bool,

    /// The search for a unit that wears a part file: the file, and the unit and faction found.
    wearer_rx: Option<Receiver<(String, Option<(String, String)>)>>,

    /// A unit asked for with its file (Units panel), applied when that file is open.
    pending_preview: Option<(String, String, String)>,

    /// The Structure panel's view, and the definition dragged from Unit files.
    tree: structure::TreeState,
    dragging_file: Option<String>,

    /// The pictures of models, definitions, textures and alternatives.
    pics: thumbs::ThumbState,

    /// The open definition as a ship, when it is one.
    ship: ship::ShipState,

    /// What each slot name takes, read from the game's definitions on a thread.
    kinds: Option<Arc<SlotKinds>>,
    kinds_rx: Option<Receiver<SlotKinds>>,

    // The filters of the lists and pickers.
    file_filter: String,
    unit_filter: String,
    combo_filters: BTreeMap<String, String>,
}

impl Default for UnitTool {
    fn default() -> Self {
        Self {
            settings: UnitSettings::default(),
            catalog: None,
            catalog_rx: None,
            schema: None,
            looks: None,
            looks_error: String::new(),
            height_draft: None,
            copy_name: String::new(),
            catalog_failed: false,
            status: String::new(),
            doc: 0,
            preview_path: String::new(),
            worn: None,
            unit: String::new(),
            faction: String::new(),
            seed: 1,
            grid: 1,
            slot: String::new(),
            playing: true,
            time: 0.0,
            clock: None,
            colours: None,
            pins: Pins::default(),
            show_selection: true,
            isolate: false,
            selected: None,
            models: vec![],
            models_for: None,
            loading: None,
            failed: HashSet::new(),
            parts_key: None,
            missing: vec![],
            worn_how: String::new(),
            entities: vec![],
            layer: None,
            posed: vec![],
            dressed_mount: false,
            geometry: HashMap::new(),
            mask: None,
            mask_for: None,
            hidden: vec![],
            own_hidden: vec![],
            environment: None,
            listings: None,
            weather: weather::StageWeather::default(),
            ground: None,
            problems: vec![],
            problems_for: None,
            aimed: 0,
            default_pending: false,
            wearer_rx: None,
            pending_preview: None,
            tree: structure::TreeState::default(),
            dragging_file: None,
            pics: thumbs::ThumbState::new(),
            ship: ship::ShipState::default(),
            kinds: None,
            kinds_rx: None,
            file_filter: String::new(),
            unit_filter: String::new(),
            combo_filters: BTreeMap::new(),
        }
    }
}

fn document(e: &mut Editor) -> Option<&mut VmdDocument> {
    let id = e.docs.of_tool(UNIT_TOOL_ID)?;
    e.docs.find_as_mut::<VmdDocument>(id)
}

/// A float as the files write them: at least one decimal.
fn number(v: f32) -> String {
    let s = format!("{v:.4}");
    let s = s.trim_end_matches('0');
    if s.ends_with('.') { format!("{s}0") } else { s.to_owned() }
}

/// `RRGGBB` as linear-free 0..1 components (the colour pickers' space, as the tables' bytes).
fn hex_to_rgb(hex: &str) -> [f32; 3] {
    let hex = hex.trim().trim_start_matches('#');
    std::array::from_fn(|i| hex.get(i * 2..i * 2 + 2).and_then(|h| u8::from_str_radix(h, 16).ok()).map_or(0.0, |b| f32::from(b) / 255.0))
}

fn rgb_to_hex(rgb: [f32; 3]) -> String {
    rgb.iter().map(|c| format!("{:02X}", (c.clamp(0.0, 1.0) * 255.0).round() as u8)).collect()
}

/// The `unit_variants_colours` row of a unit's soldiers for a faction.
fn colours_key<'a>(unit_variant: &'a str, faction: &'a str) -> [(&'static str, &'a str); 3] {
    [("unit_variant", unit_variant), ("faction", faction), ("soldier_type", "soldier")]
}

/// A table's number, plainly.
fn number_text(v: f32) -> String {
    let s = format!("{v:.4}");
    s.trim_end_matches('0').trim_end_matches('.').to_owned()
}

fn file_name(path: &str) -> &str {
    path.rsplit(['/', '\\']).next().unwrap_or(path)
}

/// A gesture's merge key: one per field of an element.
fn gesture(elem: usize, field: &str) -> u64 {
    let mut h = 0xcbf2_9ce4_8422_2325u64 ^ elem as u64;
    for b in field.bytes() {
        h = (h ^ u64::from(b)).wrapping_mul(0x0100_0000_01b3);
    }

    h | 1
}

fn hash_of(value: &impl std::hash::Hash) -> u64 {
    use std::hash::Hasher;
    let mut h = std::collections::hash_map::DefaultHasher::new();
    value.hash(&mut h);
    h.finish()
}

impl UnitTool {
    // ---- loading

    fn load_catalog(&mut self, e: &Editor) {
        if self.catalog.is_some() || self.catalog_rx.is_some() || self.catalog_failed {
            return;
        }

        let Some(vfs) = e.vfs().cloned() else { return };
        let (tx, rx) = channel();
        self.catalog_rx = Some(rx);
        "reading the unit tables...".clone_into(&mut self.status);
        std::thread::spawn(move || {
            let result = rpfm_schema_path(vfs.profile())
                .ok_or_else(|| "no rpfm schema of the game (Help > Update Manager downloads it)".to_owned())
                .and_then(|path| Schema::load(&path, None).map_err(|e| e.to_string()))
                .and_then(|schema| UnitCatalog::load(&vfs, &schema).map(|c| (c, Arc::new(schema))));
            let _ = tx.send(result);
        });
    }

    /// What each slot name takes: every definition of the packs read once, on a thread.
    fn load_kinds(&mut self, e: &Editor) {
        if self.kinds.is_some() || self.kinds_rx.is_some() {
            return;
        }

        let Some(vfs) = e.vfs().cloned() else { return };
        let (tx, rx) = channel();
        self.kinds_rx = Some(rx);
        std::thread::spawn(move || {
            let mut kinds = SlotKinds::default();
            for p in vfs.list("variantmeshes/", ".variantmeshdefinition") {
                if let Some(f) = vfs.read(p).ok().and_then(|b| VariantMeshFile::parse(b).ok()) {
                    kinds.add(&f);
                }
            }

            let _ = tx.send(kinds);
        });
    }

    fn poll(&mut self) {
        if let Some(rx) = &self.kinds_rx
            && let Ok(kinds) = rx.try_recv()
        {
            self.kinds = Some(Arc::new(kinds));
            self.kinds_rx = None;
        }

        if let Some(rx) = &self.catalog_rx {
            match rx.try_recv() {
                Ok(Ok((catalog, schema))) => {
                    self.catalog = Some(Arc::new(catalog));
                    self.schema = Some(schema);
                    self.catalog_rx = None;
                    self.status.clear();
                }

                Ok(Err(error)) => {
                    self.catalog_rx = None;
                    self.catalog_failed = true;
                    self.status = error;
                }

                Err(TryRecvError::Disconnected) => self.catalog_rx = None,
                Err(TryRecvError::Empty) => {}
            }
        }

        if let Some(rx) = &self.wearer_rx {
            match rx.try_recv() {
                Ok((path, found)) => {
                    self.wearer_rx = None;
                    if let Some((unit, faction)) = found
                        && self.preview_path == path
                    {
                        (self.unit, self.faction) = (unit, faction);
                        self.status.clear();
                    } else if self.preview_path == path {
                        "no unit wears this file: the game's default unit shows it alone".clone_into(&mut self.status);
                    }
                }

                Err(TryRecvError::Disconnected) => self.wearer_rx = None,
                Err(TryRecvError::Empty) => {}
            }
        }

        if let Some(rx) = &self.loading {
            match rx.try_recv() {
                Ok((wanted, Ok(models))) => {
                    self.status.clear();
                    self.models = models;
                    self.models_for = Some(wanted);
                    self.parts_key = None;
                    self.loading = None;
                }

                Ok((wanted, Err(error))) => {
                    self.status = format!("{}: {error}", wanted.unit);
                    self.failed.insert(wanted);
                    self.loading = None;
                }

                Err(TryRecvError::Disconnected) => self.loading = None,
                Err(TryRecvError::Empty) => {}
            }
        }
    }

    fn wanted(&self) -> Wanted {
        Wanted {
            unit: self.unit.clone(),
            faction: self.faction.clone(),
            seed: self.seed,
            count: self.grid.clamp(1, 16),
            slot: self.slot.clone(),
        }
    }

    fn load_models(&mut self, e: &Editor, wanted: Wanted) {
        let (Some(vfs), Some(catalog)) = (e.vfs().cloned(), self.catalog.clone()) else { return };
        let (tx, rx) = channel();
        self.loading = Some(rx);
        format!("loading {}...", wanted.unit).clone_into(&mut self.status);
        std::thread::spawn(move || {
            let read = |p: &str| vfs.read(p).ok();
            let models = (0..wanted.count)
                .map(|k| {
                    let mut model = UnitModel::load(&read, &catalog, &wanted.unit, &wanted.faction, wanted.seed.wrapping_add(k))?;
                    if !wanted.slot.is_empty() {
                        model.play(&read, &wanted.slot);
                    }

                    Ok(model)
                })
                .collect();
            let _ = tx.send((wanted, models));
        });
    }

    fn listings(&mut self, e: &Editor) -> &Listings {
        if self.listings.is_none()
            && let Some(vfs) = e.vfs()
        {
            let owned = |v: Vec<&str>| v.into_iter().map(str::to_owned).collect::<Vec<_>>();
            let mut textures = owned(vfs.list("variantmeshes/", ".dds"));
            textures
                .extend(vfs.list("variantmeshes/", ".texture_array").into_iter().map(|p| p.trim_end_matches(".texture_array").to_owned()));
            textures.sort();
            self.listings = Some(Listings {
                definitions: owned(vfs.list("variantmeshes/", ".variantmeshdefinition")),
                models: owned(vfs.list("variantmeshes/", ".rigid_model_v2")),
                textures,
                weathers: owned(vfs.list("weather/", ".environment")),
            });
        }

        static EMPTY: std::sync::OnceLock<Listings> = std::sync::OnceLock::new();
        self.listings.as_ref().unwrap_or_else(|| EMPTY.get_or_init(Listings::default))
    }

    // ---- the scene

    /// The other tools' layers hidden, this tool's shown. Done every frame: a document another tool opens meanwhile has a layer too.
    fn hide_others(&mut self, e: &mut Editor) {
        for id in std::mem::take(&mut self.own_hidden) {
            e.scene.set_layer_visible(id, true);
        }

        let others: Vec<LayerId> = e
            .scene
            .layers()
            .iter()
            .filter(|l| l.visible && l.doc != 0 && e.docs.find(l.doc).is_some() && e.docs.tool_of(l.doc) != UNIT_TOOL_ID)
            .map(|l| l.id)
            .collect();
        for id in others {
            e.scene.set_layer_visible(id, false);
            self.hidden.push(id);
        }
    }

    fn show_others(&mut self, e: &mut Editor) {
        for id in std::mem::take(&mut self.hidden) {
            e.scene.set_layer_visible(id, true);
        }

        let own: Vec<LayerId> =
            e.scene.layers().iter().filter(|l| l.visible && l.doc != 0 && e.docs.tool_of(l.doc) == UNIT_TOOL_ID).map(|l| l.id).collect();
        for id in own {
            e.scene.set_layer_visible(id, false);
            self.own_hidden.push(id);
        }
    }

    fn clear(&mut self, e: &mut Editor) {
        e.scene.remove_ids(&self.entities);
        self.entities.clear();
        if let Some(m) = self.mask.take() {
            e.scene.remove_ids(&[m]);
        }

        self.mask_for = None;
        self.posed.clear();
    }

    /// The stage's grass follows the weather chips' climate and season.
    fn update_ground(&mut self, e: &mut Editor, id: DocumentId) {
        let texture = self.ground_texture(e);
        if self.ground.as_ref().is_some_and(|(d, t)| *d == id && *t == texture) {
            return;
        }

        self.ground = Some((id, texture.clone()));
        let Some(d) = e.docs.find_as_mut::<VmdDocument>(id) else { return };
        let ground = d.set_ground(&texture, e.scene.game_units);
        for entity in e.scene.entities_of(SourceRef { doc: id, item: 0 }) {
            if e.scene.find(entity).is_some_and(|en| matches!(en.payload, Payload::Terrain(_))) {
                e.scene.update_payload(entity, Payload::Terrain(ground.clone()));
            }
        }
    }

    /// The stage's light: the chosen weather, else the game's default.
    fn light(&mut self, e: &mut Editor) {
        let path = if self.settings.weather.is_empty() { e.game().default_environment.to_owned() } else { self.settings.weather.clone() };
        if self.environment.as_ref().is_none_or(|(p, _)| *p != path) {
            let read = e.vfs().ok_or_else(|| "no packs".to_owned()).and_then(|v| v.read(&path).map_err(|e| e.to_string()));
            match read.and_then(|b| XmlDoc::parse(b).map_err(|e| e.to_string())) {
                Ok(doc) => self.environment = Some((path, read_environment(&doc))),
                Err(error) => {
                    log::warn(format!("unit tool weather {path}: {error}"));
                    self.environment = Some((path, EnvironmentParams::default()));
                }
            }
        }

        let Some((_, params)) = &self.environment else { return };
        let mut params = params.clone();
        let go = e.graphics_options();
        params.output_gamma = go.gamma;
        params.output_brightness = go.brightness;
        let slot = &mut e.scene.environment;
        slot.active = Some(params);
        slot.blend = None;
        slot.blend_t = 0.0;
        slot.owner = 0;
    }

    /// A unit that wears `path` somewhere below its own definition (or its mount's), searched on a thread: every definition's references
    /// read once, then walked up from the file.
    fn search_wearer(&mut self, e: &Editor, path: &str) {
        let (Some(vfs), Some(catalog)) = (e.vfs().cloned(), self.catalog.clone()) else { return };
        let (tx, rx) = channel();
        self.wearer_rx = Some(rx);
        "looking for a unit that wears this file...".clone_into(&mut self.status);
        let (path, default) = (path.to_owned(), default_unit(e.game().key));
        let default = (default.0.to_owned(), default.1.to_owned());
        std::thread::spawn(move || {
            // Who references whom.
            let mut referenced_by: std::collections::HashMap<String, Vec<String>> = std::collections::HashMap::new();
            for file in vfs.list("variantmeshes/", ".variantmeshdefinition") {
                let Ok(bytes) = vfs.read(file) else { continue };
                let text = String::from_utf8_lossy(&bytes).to_lowercase();
                let mut rest = text.as_str();
                while let Some(at) = rest.find("definition=") {
                    rest = &rest[at + 11..];
                    let quote = rest.chars().next().unwrap_or('"');
                    let Some(end) = rest[1..].find(quote) else { break };
                    referenced_by.entry(normalize(&rest[1..=end])).or_default().push(file.to_owned());
                    rest = &rest[end + 1..];
                }
            }

            let mut wearing: HashSet<String> = HashSet::from([path.clone()]);
            let mut pending = vec![path.clone()];
            while let Some(f) = pending.pop() {
                for by in referenced_by.get(&f).into_iter().flatten() {
                    if wearing.insert(by.clone()) {
                        pending.push(by.clone());
                    }
                }
            }

            let wears = |u: &wce_render::unit_catalog::UnitEntry| {
                catalog.land_units.get(&u.unit).is_some_and(|land| {
                    wearing.contains(&variant_path(&u.variant))
                        || catalog.mounts.get(&land.mount).is_some_and(|m| wearing.contains(&variant_path(&m.variant)))
                })
            };
            let found = catalog
                .entries
                .iter()
                .find(|u| u.unit == default.0 && wears(u))
                .or_else(|| catalog.entries.iter().find(|u| wears(u)))
                .map(|u| (u.unit.clone(), u.faction.clone()));
            let _ = tx.send((path, found));
        });
    }

    /// A document opened: its unit (remembered, else one that wears it, else the game's default) and the camera on it.
    fn document_opened(&mut self, e: &mut Editor, id: DocumentId) {
        self.doc = id;
        self.selected = None;
        self.pins = Pins::default();
        self.parts_key = None;
        self.problems_for = None;
        let Some(path) = e.docs.find_as::<VmdDocument>(id).map(|d| d.source().vfs_path.clone()) else { return };
        self.preview_path.clone_from(&path);
        self.wearer_rx = None;
        if let Some((p, unit, faction)) = self.pending_preview.take()
            && p == path
        {
            self.set_preview(e, &path, unit, faction);
        } else if let Some((unit, faction)) = self.settings.preview.get(&path).cloned() {
            self.unit = unit;
            self.faction = faction;
        } else {
            self.preview_default(e, &path);
        }

        if self.aimed != id {
            self.aimed = id;
            let units = e.scene.game_units.max(1e-3);
            // In front of the soldiers (they face +z), a little to their right.
            e.viewport.controller().set_orbit(std::f32::consts::PI + 0.6, 0.2, 4.5 / units, Float3::new(0.0, 1.0 / units, 0.0));
        }
    }

    /// The first unit that wears the file (or rides it), else the game's default while a unit that wears it below its own definition is
    /// searched for.
    fn preview_default(&mut self, e: &Editor, path: &str) {
        let (unit, faction) = default_unit(e.game().key);
        (self.unit, self.faction) = (unit.to_owned(), faction.to_owned());
        self.default_pending = self.catalog.is_none();
        let Some(c) = &self.catalog else { return };

        // The game's default if it wears the file, else a row of its unit, else any.
        let wears = |u: &&wce_render::unit_catalog::UnitEntry| variant_path(&u.variant) == path && c.land_units.contains_key(&u.unit);
        let found = c
            .entries
            .iter()
            .filter(wears)
            .find(|u| u.unit == unit && u.faction == faction)
            .or_else(|| c.entries.iter().filter(wears).find(|u| u.unit == unit))
            .or_else(|| c.entries.iter().find(wears));
        if let Some(entry) = found {
            (self.unit, self.faction) = (entry.unit.clone(), entry.faction.clone());
            return;
        }

        let mounts: Vec<&String> = c.mounts.iter().filter(|(_, m)| variant_path(&m.variant) == path).map(|(k, _)| k).collect();
        if let Some((unit, _)) = c.land_units.iter().find(|(_, l)| mounts.contains(&&l.mount))
            && let Some(entry) = c.entries.iter().find(|u| u.unit == *unit)
        {
            (self.unit, self.faction) = (entry.unit.clone(), entry.faction.clone());
            return;
        }

        self.search_wearer(e, path);
    }

    fn set_preview(&mut self, e: &mut Editor, path: &str, unit: String, faction: String) {
        self.wearer_rx = None;
        self.unit.clone_from(&unit);
        self.faction.clone_from(&faction);
        self.slot.clear();
        let path = path.to_owned();
        e.update_tool_settings::<UnitSettings>(UNIT_TOOL_ID, |s| {
            s.preview.insert(path, (unit, faction));
        });
        self.settings = e.tool_settings(UNIT_TOOL_ID);
    }

    /// The pins and the selection's: the slots above the selected node show it.
    fn effective_pins(&self) -> Pins {
        let mut pins = self.pins.clone();
        if self.show_selection
            && let Some(sel) = &self.selected
        {
            for k in (0..sel.len()).step_by(2) {
                if k + 1 < sel.len() {
                    pins.slots.entry(sel[..=k].to_vec()).or_insert(SlotPin::Alternative(sel[k + 1]));
                }
            }
        }

        pins
    }

    /// The definitions of the unit as previewed: its own and its mount's, in the packs.
    fn unit_files(&self) -> (Option<String>, Option<String>) {
        let Some(c) = &self.catalog else { return (None, None) };
        let own = c.entry(&self.unit, &self.faction).map(|u| variant_path(&u.variant));
        let mount = c.land_units.get(&self.unit).and_then(|l| c.mounts.get(&l.mount)).map(|m| variant_path(&m.variant));
        (own, mount)
    }

    /// The soldiers wear the document as it is now: as their own definition (or their mount's) when it is; else their own definitions with
    /// it where they reference it, at any depth; else it alone, when they never do.
    fn dress(&mut self, e: &mut Editor) {
        let Some(vfs) = e.vfs().cloned() else { return };
        let Some(d) = document(e) else { return };
        let (tree, version, path) = (d.tree(), d.version(), d.source().vfs_path.clone());
        let pins = self.effective_pins();
        let key = hash_of(&(
            version,
            self.doc,
            format!("{:?}", pins.slots.iter().collect::<BTreeMap<_, _>>()),
            format!("{:?}{:?}", pins.tints.iter().collect::<BTreeMap<_, _>>(), pins.decals.iter().collect::<BTreeMap<_, _>>()),
            &self.selected,
            self.isolate,
            &self.models_for,
        ));
        if self.parts_key == Some(key) || self.models.is_empty() {
            return;
        }

        let edited = d.xml().save();
        self.parts_key = Some(key);
        self.worn = Some(version);
        let (own, mount_file) = self.unit_files();
        let read = |p: &str| if p == path { Some(edited.clone()) } else { vfs.read(p).ok() };
        let parse = |p: &Option<String>| p.as_ref().and_then(|p| vfs.read(p).ok()).and_then(|b| VariantMeshFile::parse(b).ok());
        let (man_tree, mount_tree) = (parse(&own), parse(&mount_file));
        let focus = Pins { focus: Some(path.clone()), show_focus: true, ..pins.clone() };
        self.missing.clear();
        let mut how = "";
        for (k, model) in self.models.iter_mut().enumerate() {
            let seed = self.seed.wrapping_add(k as u64);
            let from_focus = |u: &wce_render::unit_assembly::AssembledUnit| u.parts.iter().any(|p| p.node.is_some());
            let (mut parts, on_mount) = if own.as_deref() == Some(path.as_str()) {
                how = "the unit's own definition";
                (assemble_with(&read, &tree, seed, &pins), false)
            } else if mount_file.as_deref() == Some(path.as_str()) && model.mount.is_some() {
                how = "the unit's mount's definition";
                (assemble_with(&read, &tree, seed, &pins), true)
            } else {
                let man = man_tree.as_ref().map(|t| assemble_with(&read, t, seed, &focus)).filter(from_focus);
                let mount = if man.is_none() && model.mount.is_some() {
                    mount_tree.as_ref().map(|t| assemble_with(&read, t, seed, &focus)).filter(from_focus)
                } else {
                    None
                };
                match (man, mount) {
                    (Some(parts), _) => {
                        how = "worn where the unit's definition references it";
                        (parts, false)
                    }

                    (None, Some(parts)) => {
                        how = "worn where the mount's definition references it";
                        (parts, true)
                    }

                    (None, None) => {
                        how = "alone: the unit's definitions never reference it";
                        (assemble_with(&read, &tree, seed, &pins), false)
                    }
                }
            };

            if self.isolate
                && let Some(sel) = &self.selected
            {
                parts.parts.retain(|p| p.node.as_ref().is_some_and(|n| n.starts_with(sel)));
            }

            self.missing.extend(parts.missing.iter().cloned());
            self.dressed_mount = on_mount && model.mount.is_some();
            match (&mut model.mount, on_mount) {
                (Some(m), true) => m.parts = parts,
                _ => model.man.parts = parts,
            }
        }

        how.clone_into(&mut self.worn_how);
        self.missing.sort();
        self.missing.dedup();
    }

    /// The soldiers in a row along x, centred, each posed at the clock's time.
    fn pose(&mut self, e: &mut Editor) {
        let now = e.clock.time();
        let dt = self.clock.map_or(0.0, |was| (now - was).max(0.0)) as f32;
        self.clock = Some(now);
        if self.playing {
            self.time += dt;
        }

        let Some(id) = (self.doc != 0).then_some(self.doc) else { return };
        let layer = match self.layer {
            Some((doc, layer)) if doc == id && e.scene.layer(layer).is_some() => layer,
            _ => {
                self.clear(e);
                let layer = e.scene.add_layer(id, "unit preview");
                self.layer = Some((id, layer));
                layer
            }
        };

        let units = e.scene.game_units.max(1e-3);
        self.posed.clear();
        let n = self.models.len();
        let gap = if self.models.first().is_some_and(|m| m.mount.is_some()) { 2.2 } else { 1.1 };
        let mut placed = vec![];
        for (k, model) in self.models.iter().enumerate() {
            let x = (k as f32 - (n as f32 - 1.0) / 2.0) * gap / units;
            for figure in model.instances(self.time, true) {
                let mut world = translation(Float3::new(x, 0.0, 0.0));
                let s = figure.scale / units;
                for row in 0..3 {
                    world.m[row][row] *= s;
                }

                let mut instance = figure.instance;
                if let Some(c) = self.colours {
                    instance.faction_colours[..3].copy_from_slice(&c);
                }

                self.posed.push(Posed {
                    world: world * figure.local,
                    instance: instance.clone(),
                    sources: figure.sources.clone(),
                    mount: figure.mount,
                    soldier: k,
                });
                placed.push((world * figure.local, instance));
            }
        }

        if self.entities.len() != placed.len() || self.entities.iter().any(|&id| e.scene.find(id).is_none()) {
            self.clear(e);
            self.entities = placed
                .iter()
                .map(|_| e.scene.add(layer, Float4x4::IDENTITY, Payload::CameraRig(Default::default()), SourceRef { doc: id, item: 0 }))
                .collect();
        }

        for (&entity, (world, instance)) in self.entities.iter().zip(placed) {
            e.scene.update_pose(entity, world, instance);
        }

        self.outline(e, id, layer);
    }

    fn update_problems(&mut self, e: &mut Editor) {
        let Some(vfs) = e.vfs().cloned() else { return };
        let Some(d) = document(e) else { return };
        let key = (d.id(), d.version());
        if self.problems_for == Some(key) {
            return;
        }

        let (tree, path) = (d.tree(), d.source().vfs_path.clone());
        let read = |p: &str| vfs.read(p).ok();
        let exists = |p: &str| vfs.owner(p).is_some();
        self.problems = check(&read, &exists, &tree, &path);
        self.problems_for = Some(key);
    }

    fn geometry_of(&mut self, e: &Editor, model: &str) -> Option<Arc<PartGeometry>> {
        if !self.geometry.contains_key(model) {
            let g = e.vfs().and_then(|v| v.read(model).ok()).and_then(|b| PartGeometry::decode(&b)).map(Arc::new);
            self.geometry.insert(model.to_owned(), g);
        }

        self.geometry[model].clone()
    }

    /// The document's node a drawn part shows: none for a prop, or the figure that doesn't wear the document.
    fn node_of(&self, posed: &Posed, part: usize) -> Option<NodePath> {
        let model = self.models.get(posed.soldier)?;
        let figure = if posed.mount { model.mount.as_ref()? } else { &model.man };
        if posed.mount != self.dressed_mount {
            return None;
        }

        figure.parts.parts.get((*posed.sources.get(part)?)?)?.node.clone()
    }

    /// The node of the part under a pixel of the viewport, the nearest one.
    fn pick_part(&mut self, e: &Editor, x: i32, y: i32) -> Option<(NodePath, String)> {
        let (origin, dir) = e.viewport.ray(x, y)?;
        let mut best: Option<(f32, NodePath, String)> = None;
        let posed = std::mem::take(&mut self.posed);
        for p in &posed {
            for (k, part) in p.instance.parts.iter().enumerate() {
                let Some(node) = self.node_of(p, k) else { continue };
                let Some(g) = self.geometry_of(e, &part.model) else { continue };
                for t in g.triangles(part, &p.world, &p.instance.palette, &p.instance.bones) {
                    if let Some(d) = ray_triangle(origin, dir, &t)
                        && best.as_ref().is_none_or(|(b, ..)| d < *b)
                    {
                        best = Some((d, node.clone(), part.model.clone()));
                    }
                }
            }
        }

        self.posed = posed;
        best.map(|(_, n, m)| (n, m))
    }

    /// The selected node's parts outlined as a building is: their triangles, as posed this frame, in a mask overlay the editor's selection
    /// names. Nothing selected, or nothing of it drawn: no mask, and the selection is the editor's again.
    fn outline(&mut self, e: &mut Editor, id: DocumentId, layer: LayerId) {
        let mask_ref = SourceRef { doc: id, item: MASK_ITEM };
        let mut tris = vec![];
        if let Some(sel) = self.selected.clone() {
            let posed = std::mem::take(&mut self.posed);
            for p in &posed {
                for (k, part) in p.instance.parts.iter().enumerate() {
                    if self.node_of(p, k).is_some_and(|n| n.starts_with(&sel))
                        && let Some(g) = self.geometry_of(e, &part.model)
                    {
                        tris.extend(g.triangles(part, &p.world, &p.instance.palette, &p.instance.bones));
                    }
                }
            }

            self.posed = posed;
        }

        if tris.is_empty() {
            if let Some(m) = self.mask.take() {
                e.scene.remove_ids(&[m]);
            }

            if e.selection.contains(mask_ref) {
                e.selection.clear();
            }

            return;
        }

        let mut overlay = Overlay { pickable: false, selection_mask: true, own_buffer: true, ..Default::default() };
        for t in &tris {
            overlay.triangle(t[0], t[1], t[2], 0xffff_ffff);
        }

        match self.mask.filter(|&m| e.scene.find(m).is_some()) {
            Some(m) => e.scene.update_payload(m, Payload::Overlay(overlay)),
            None => self.mask = Some(e.scene.add(layer, Float4x4::IDENTITY, Payload::Overlay(overlay), mask_ref)),
        }

        if e.selection.items() != [mask_ref] {
            e.selection.set(mask_ref);
        }
    }

    // ---- actions

    fn apply(&mut self, e: &mut Editor, actions: Vec<Action>) {
        for action in actions {
            match action {
                Action::Select(path) => {
                    self.selected = path;
                }

                Action::Edit(op, elem) => {
                    let Some(d) = document(e) else { continue };
                    match d.structural(&op, elem) {
                        Ok(select) => {
                            let tree = d.tree();
                            let path = select.and_then(|s| tree.path_of(s));
                            // A tint is no node: its mesh stays selected.
                            if op != Structural::AddTint {
                                self.selected = path.or_else(|| self.selected.clone().filter(|p| tree.node(p).is_some()));
                            }

                            // A new slot: its name in edit mode.
                            if matches!(op, Structural::AddSlot(_)) {
                                self.rename_new_slot(&tree);
                            }

                            // Pins name places, and the places moved.
                            if matches!(op, Structural::Remove | Structural::MoveUp | Structural::MoveDown) {
                                self.pins = Pins::default();
                            }
                        }

                        Err(error) => e.status(&error, true),
                    }
                }

                Action::Rename(elem, name) => {
                    if let Some(d) = document(e) {
                        d.set_value(elem, "name", name.trim(), 0);
                    }
                }

                Action::Pin(path, pin) => match pin {
                    Some(p) => {
                        self.pins.slots.insert(path, p);
                    }

                    None => {
                        self.pins.slots.remove(&path);
                    }
                },

                Action::Reorder(from, to) => {
                    if let Err(error) = self.reorder(e, &from, to) {
                        e.status(&error, true);
                    }
                }

                Action::Open(path) => {
                    if let Err(error) = e.open_path_from(self, &path, true, true) {
                        e.status(&error, true);
                    }
                }

                Action::OpenUnit(unit, faction) => {
                    let Some(c) = self.catalog.clone() else { continue };
                    let Some(entry) = c.entry(&unit, &faction) else { continue };
                    let path = variant_path(&entry.variant);
                    let open =
                        e.docs.of_tool(UNIT_TOOL_ID).and_then(|id| e.docs.find_as::<VmdDocument>(id)).map(|d| d.source().vfs_path.clone());
                    if open.as_deref() == Some(path.as_str()) {
                        self.set_preview(e, &path, unit, faction);
                        continue;
                    }

                    self.pending_preview = Some((path.clone(), unit, faction));
                    if let Err(error) = e.open_path_from(self, &path, true, true) {
                        e.status(&error, true);
                        self.pending_preview = None;
                    }
                }
            }
        }
    }

    /// A node moved to place `to` among its siblings, one swap at a time, as one undo entry. Pins name places: they go.
    fn reorder(&mut self, e: &mut Editor, from: &NodePath, to: usize) -> Result<(), String> {
        let d = document(e).ok_or("no definition open")?;
        let mut elem = d.tree().node(from).ok_or("no such node")?.element();
        let mut steps = 0;
        for _ in 0..256 {
            let tree = d.tree();
            let Some(at) = tree.path_of(elem).and_then(|p| p.last().copied()) else { break };
            if at == to {
                break;
            }

            let op = if at > to { Structural::MoveUp } else { Structural::MoveDown };
            match d.structural(&op, elem) {
                Ok(Some(next)) => {
                    elem = next;
                    steps += 1;
                }

                Ok(None) => break,
                Err(error) => {
                    d.join_last(steps, "Move");
                    return Err(error);
                }
            }
        }

        d.join_last(steps, &format!("Move {}", from.iter().map(ToString::to_string).collect::<Vec<_>>().join("/")));
        self.selected = d.tree().path_of(elem);
        self.pins = Pins::default();
        Ok(())
    }

    // ---- panels

    fn draw_files(&mut self, e: &mut Editor) -> Vec<Action> {
        let mut actions = vec![];
        if !gui::begin("Unit files", None, 0) {
            gui::end();
            return actions;
        }

        gui::set_next_item_width(-gui::frame_height_with_spacing() - 4.0);
        gui::input_text("##vmd_filter", Some("filter the definitions"), &mut self.file_filter, 128, 0);
        gui::same_line();
        thumbs::grid_toggle("files_grid", &mut self.pics.files_grid);
        let words: Vec<String> = self.file_filter.split_whitespace().map(str::to_lowercase).collect();
        let open = e.docs.of_tool(UNIT_TOOL_ID).and_then(|id| e.docs.find_as::<VmdDocument>(id)).map(|d| d.source().vfs_path.clone());
        let shown: Vec<String> = self.listings(e).definitions.iter().filter(|p| wce_editor::pickers::matches(p, &words)).cloned().collect();
        gui::text_colored(theme::dim(), &format!("{} definitions; drag one onto a slot for a reference to it", shown.len()));
        let mut dragged = None;
        if self.pics.files_grid {
            if gui::begin_child("##vmd_grid", gui::v2(0.0, 0.0), 0, 0) {
                let cells: Vec<(thumbs::Thing, String, bool)> = shown
                    .iter()
                    .map(|p| {
                        let name = file_name(p);
                        let name = name.strip_suffix(".variantmeshdefinition").unwrap_or(name);
                        (thumbs::Thing::Definition(p.clone()), name.to_owned(), open.as_deref() == Some(p.as_str()))
                    })
                    .collect();
                if let Some(k) = self.grid(e, &cells, "files") {
                    actions.push(Action::Open(shown[k].clone()));
                }
            }

            gui::end_child();
            gui::end();
            return actions;
        }

        if gui::begin_child("##vmd_list", gui::v2(0.0, 0.0), 0, 0) {
            gui::clipped(shown.len(), |k| {
                let p = &shown[k];
                let label = p.strip_prefix(DEFINITIONS).or_else(|| p.strip_prefix("variantmeshes/")).unwrap_or(p);
                let ours = e.loose_copy(p).is_some();
                let text = if ours { format!("{label}  (project)") } else { label.to_owned() };
                if gui::selectable(&text, open.as_deref() == Some(p.as_str()), 0) {
                    actions.push(Action::Open(p.clone()));
                }

                if gui::begin_drag_drop_source() {
                    gui::set_drag_drop_payload(structure::FILE_PAYLOAD, &[0]);
                    gui::text(&format!("{} {label}", wce_editor::icons::LINK_2));
                    dragged = Some(p.clone());
                    gui::end_drag_drop_source();
                }
            });
        }

        gui::end_child();
        if dragged.is_some() {
            self.dragging_file = dragged;
        }

        gui::end();
        actions
    }

    fn draw_units(&mut self, e: &mut Editor) -> Vec<Action> {
        let mut actions = vec![];
        if !gui::begin("Units", None, 0) {
            gui::end();
            return actions;
        }

        let Some(c) = self.catalog.clone() else {
            gui::text_colored(theme::dim(), if self.status.is_empty() { "reading the unit tables..." } else { &self.status });
            gui::end();
            return actions;
        };

        gui::set_next_item_width(-gui::frame_height_with_spacing() - 4.0);
        gui::input_text("##unit_filter", Some("filter: unit, faction or definition"), &mut self.unit_filter, 128, 0);
        gui::same_line();
        thumbs::grid_toggle("units_grid", &mut self.pics.units_grid);
        let words: Vec<String> = self.unit_filter.split_whitespace().map(str::to_lowercase).collect();
        let rows: Vec<&wce_render::unit_catalog::UnitEntry> =
            c.entries.iter().filter(|u| wce_editor::pickers::matches(&format!("{} {} {}", u.unit, u.faction, u.variant), &words)).collect();
        gui::text_colored(
            theme::dim(),
            &format!("{} unit_variants rows; a click opens the row's definition, worn by its unit", rows.len()),
        );
        if self.pics.units_grid {
            if gui::begin_child("##units_grid", gui::v2(0.0, 0.0), 0, 0) {
                let cells: Vec<(thumbs::Thing, String, bool)> = rows
                    .iter()
                    .map(|u| {
                        let label = if u.faction.is_empty() { u.unit.clone() } else { format!("{} ({})", u.unit, u.faction) };
                        (thumbs::Thing::Definition(variant_path(&u.variant)), label, u.unit == self.unit && u.faction == self.faction)
                    })
                    .collect();
                if let Some(k) = self.grid(e, &cells, "units") {
                    actions.push(Action::OpenUnit(rows[k].unit.clone(), rows[k].faction.clone()));
                }
            }

            gui::end_child();
            gui::end();
            return actions;
        }

        if gui::begin_table("##units", 3, gui::TABLE_ROW_BG | gui::TABLE_SCROLL_Y | gui::TABLE_RESIZABLE, gui::v2(0.0, 0.0)) {
            gui::table_setup_scroll_freeze(0, 1);
            gui::table_setup_column("Unit", 0, 0.0);
            gui::table_setup_column("Faction", 0, 0.0);
            gui::table_setup_column("Definition", 0, 0.0);
            gui::table_headers_row();
            gui::clipped(rows.len(), |k| {
                let u = rows[k];
                gui::table_next_row();
                gui::table_next_column();
                let current = u.unit == self.unit && u.faction == self.faction;
                if gui::selectable(&format!("{}##row{k}", u.unit), current, gui::SELECTABLE_SPAN_ALL_COLUMNS) {
                    actions.push(Action::OpenUnit(u.unit.clone(), u.faction.clone()));
                }

                gui::table_next_column();
                gui::text(if u.faction.is_empty() { "(any)" } else { &u.faction });
                gui::table_next_column();
                gui::text(&u.variant);
            });
            gui::end_table();
        }

        gui::end();
        actions
    }

    fn combo(&mut self, label: &str, tip: &str, value: &str, items: &[String]) -> Option<String> {
        let filter = self.combo_filters.entry(label.to_owned()).or_default();
        search_combo(label, tip, filter, value, items)
    }

    fn draw_preview(&mut self, e: &mut Editor) -> Vec<Action> {
        let actions = vec![];
        if !gui::begin("Preview", None, 0) {
            gui::end();
            return actions;
        }

        let path = document(e).map(|d| d.source().vfs_path.clone()).unwrap_or_default();
        if self.ship.on
            && let Some(c) = self.catalog.clone()
        {
            gui::separator_text("A ship's definition");
            match &self.ship.host {
                Some(host) => gui::text_wrapped(&format!(
                    "A part of a ship: shown where {} references it, the ship's pieces intact, with the oars and weapons of the naval unit below.",
                    file_name(host)
                )),
                None => gui::text_wrapped("Drawn as a ship: its pieces intact, with the oars and weapons of the naval unit below."),
            }
            if self.ship.guessed {
                gui::text_colored(
                    theme::dim(),
                    "No unit_variants_ships row names this file: the unit below has the ship named most like it.",
                );
            }

            let mut units: Vec<String> = vec!["(none: the pieces alone)".to_owned()];
            units.extend(c.naval().into_iter().map(str::to_owned));
            let current = self.ship.unit.clone().unwrap_or_else(|| units[0].clone());
            if let Some(u) = self.combo("Naval unit", "The naval unit whose ship logic places the oars and weapons.", &current, &units) {
                let unit = (u != units[0]).then_some(u);
                self.set_ship_unit(e, unit);
            }
        }

        if let Some(c) = self.catalog.clone() {
            gui::separator_text("Preview as");
            let units: Vec<String> = c.units().into_iter().map(str::to_owned).collect();
            let unit = self.unit.clone();
            if let Some(u) =
                self.combo("Unit", "The unit wearing the file: its skeleton, animations, mount, size and colours.", &unit, &units)
            {
                let faction = self.faction.clone();
                self.set_preview(e, &path, u, faction);
            }

            let mut factions: Vec<String> = c.colours.keys().cloned().collect();
            factions.sort();
            let faction = self.faction.clone();
            if let Some(f) = self.combo("Faction", "Its colours, and its row of unit_variants when it has one.", &faction, &factions) {
                let unit = self.unit.clone();
                self.set_preview(e, &path, unit, f);
                self.colours = None;
            }

            let wearers: Vec<(String, String)> = c
                .entries
                .iter()
                .filter(|u| !path.is_empty() && variant_path(&u.variant) == path)
                .map(|u| (u.unit.clone(), u.faction.clone()))
                .collect();
            if !wearers.is_empty() && gui::begin_menu("Units wearing this file") {
                for (u, f) in &wearers {
                    if gui::menu_item(&format!("{u}  {f}"), None, *u == self.unit && *f == self.faction, true) {
                        self.set_preview(e, &path, u.clone(), f.clone());
                    }
                }

                gui::end_menu();
            }
        }

        gui::separator_text("Soldiers");
        let mut seed = i32::try_from(self.seed).unwrap_or(i32::MAX);
        gui::set_next_item_width(120.0);
        if gui::input_int("Seed", &mut seed) {
            self.seed = u64::try_from(seed.max(0)).unwrap_or(0);
        }

        if gui::is_item_hovered() {
            gui::set_tooltip(
                "Picks the soldier: each slot's roll and alternative, the tints and the decals (the editor's generator, not the game's).",
            );
        }

        gui::same_line();
        if gui::small_button("<") {
            self.seed = self.seed.saturating_sub(1);
        }

        gui::same_line();
        if gui::small_button(">") {
            self.seed += 1;
        }

        let mut grid = i32::try_from(self.grid).unwrap_or(1);
        if gui::slider_int("Side by side", &mut grid, 1, 16) {
            self.grid = u64::try_from(grid.max(1)).unwrap_or(1);
        }

        if gui::is_item_hovered() {
            gui::set_tooltip("How many soldiers, each the next seed: the variety the unit shows.");
        }

        gui::checkbox("Show the selection", &mut self.show_selection);
        if gui::is_item_hovered() {
            gui::set_tooltip("The slots above the selected node show it, whatever the seed picks.");
        }

        gui::same_line();
        gui::checkbox("Isolate", &mut self.isolate);
        if gui::is_item_hovered() {
            gui::set_tooltip("Only the selected node's parts.");
        }

        let pinned = self.pins.slots.len() + self.pins.tints.len() + self.pins.decals.len();
        if pinned > 0 {
            gui::text_colored(theme::dim(), &format!("{pinned} pins (set in the Inspector)"));
            gui::same_line();
            if gui::small_button("Clear pins") {
                self.pins = Pins::default();
            }
        }

        gui::separator_text("Animation");
        let slots: Vec<String> = self.models.first().map(|m| m.slots().into_iter().map(str::to_owned).collect()).unwrap_or_default();
        let shown = if self.slot.is_empty() { "STAND".to_owned() } else { self.slot.clone() };
        if let Some(s) = self.combo("Plays", "The animation slot the soldiers loop, from their animation table.", &shown, &slots) {
            self.slot = s;
        }

        gui::checkbox("Play", &mut self.playing);
        gui::same_line();
        gui::set_next_item_width(-1.0);
        gui::slider_float("##time", &mut self.time, 0.0, 30.0, "%.2f s");

        gui::separator_text("Colours");
        let mut own = self.colours.is_some();
        if gui::checkbox("Own colours", &mut own) {
            self.colours = own.then(|| {
                let c = self.models.first().map(|m| m.faction_colours).unwrap_or([[1.0; 3]; 4]);
                [c[0], c[1], c[2]]
            });
        }

        if let Some(colours) = &mut self.colours {
            for (k, name) in ["Primary", "Secondary", "Uniform"].iter().enumerate() {
                gui::color_edit3(name, &mut colours[k], 0);
            }
        }

        self.draw_looks(e);

        gui::separator_text("State");
        if let Some(m) = self.models.first() {
            gui::text_colored(theme::dim(), &format!("skeleton {}, scale {:.3}", m.man.rig.skeleton.skeleton, m.scale));
            if let Some(mount) = &m.mount {
                gui::text_colored(theme::dim(), &format!("mount on {}", mount.rig.skeleton.skeleton));
            }

            if !self.worn_how.is_empty() {
                gui::text_colored(theme::dim(), &format!("The file: {}.", self.worn_how));
            }

            let parts: usize =
                self.models.iter().map(|m| m.man.parts.parts.len() + m.mount.as_ref().map_or(0, |x| x.parts.parts.len())).sum();
            gui::text_colored(theme::dim(), &format!("{} soldiers, {parts} parts", self.models.len()));
        }

        for m in &self.missing {
            gui::text_colored(theme::warning(), &format!("missing: {m}"));
        }

        if !self.status.is_empty() {
            gui::text_wrapped(&self.status);
        }

        gui::end();
        actions
    }

    /// The tables read again after the project's rows changed: the catalog, and the soldiers with it.
    fn reload_tables(&mut self, e: &mut Editor, changed: &[String]) {
        for path in changed {
            e.loose_changed(path);
        }

        self.catalog = None;
        self.catalog_rx = None;
        self.catalog_failed = false;
        self.models_for = None;
        self.failed.clear();
        self.load_catalog(e);
    }

    /// Writes the project's rows, then reads the tables again.
    fn save_looks(&mut self, e: &mut Editor) {
        let Some(looks) = &mut self.looks else { return };
        match looks.save(e.game().key, &e.loose_dir()) {
            Ok(changed) => {
                e.status(&format!("the project's DB rows: {} files written", changed.len()), false);
                self.reload_tables(e, &changed);
            }

            Err(error) => e.status(&error, true),
        }
    }

    /// The DB rows of the previewed unit for its faction: which definition it wears, its height, its own colours.
    fn draw_looks(&mut self, e: &mut Editor) {
        if !gui::collapsing_header("The unit's looks (DB)", 0) {
            return;
        }

        if self.looks.is_none() && self.looks_error.is_empty() {
            if self.schema.is_none() {
                gui::text_colored(theme::dim(), "reading the unit tables...");
                return;
            }

            if let Err(error) = self.ensure_looks(e) {
                self.looks_error = error;
            }
        }

        if !self.looks_error.is_empty() {
            gui::text_wrapped(&self.looks_error);
            return;
        }

        let Some(c) = self.catalog.clone() else { return };
        let Some(entry) = c.entry(&self.unit, &self.faction).cloned() else {
            gui::text_colored(theme::dim(), "The unit has no unit_variants row.");
            return;
        };

        let definitions: Vec<String> = self
            .listings(e)
            .definitions
            .iter()
            .filter_map(|p| p.strip_prefix(DEFINITIONS)?.strip_suffix(".variantmeshdefinition").map(str::to_owned))
            .collect();
        let faction = self.faction.clone();
        let this_row = [("unit", entry.unit.as_str()), ("faction", entry.faction.as_str())];
        let key = colours_key(&entry.name, &faction);
        let (row, changed, own, own_changed, project_rows) = {
            let Some(looks) = &self.looks else { return };
            let uv = looks.table("unit_variants");
            let colours = looks.table("unit_variants_colours");
            let project_rows = usize::from(uv.changed(&this_row)) + usize::from(colours.changed(&key));
            (uv.row(&this_row), uv.changed(&this_row), colours.row(&key), colours.changed(&key), project_rows)
        };

        let Some(row) = row else {
            gui::text_colored(theme::dim(), &format!("No unit_variants row named '{}'.", entry.name));
            return;
        };

        gui::text_wrapped(&format!(
            "unit_variants row '{}' ({}){}",
            entry.name,
            if entry.faction.is_empty() { "any faction".to_owned() } else { entry.faction.clone() },
            if changed { ": the project's" } else { "" }
        ));
        if gui::is_item_hovered() {
            gui::set_tooltip(
                "Edits are saved at once into the project's own table files (db/<table>_tables/!wce_unit_tool in its loose folder): \
                 the editor reads them before the game's, File > Export > To mod pack ships them. Not undoable: Revert takes the \
                 project's rows of this unit away.",
            );
        }

        let mut edits: Vec<(&str, String)> = vec![];

        // The definition.
        let variant = row.get("variant").cloned().unwrap_or_default();
        gui::text_colored(theme::dim(), "definition (variant)");
        gui::text(&variant);
        gui::same_line();
        let filter = self.combo_filters.entry("variant#pick".to_owned()).or_default();
        if let Some(v) = pick_button("variant", "Another definition for the unit.", filter, &variant, &definitions) {
            edits.push(("variant", v));
        }

        let open = document(e).map(|d| d.source().vfs_path.clone()).unwrap_or_default();
        let open_name = open.strip_prefix(DEFINITIONS).and_then(|p| p.strip_suffix(".variantmeshdefinition")).map(str::to_owned);
        gui::begin_disabled(open_name.as_ref().is_none_or(|n| *n == variant));
        if gui::small_button("Wear the open file")
            && let Some(n) = &open_name
        {
            edits.push(("variant", n.clone()));
        }

        gui::end_disabled();
        if gui::is_item_hovered_allow_disabled() {
            gui::set_tooltip("The unit wears the definition open in the tool (one of variantmeshes/variantmeshdefinitions).");
        }

        gui::same_line();
        if gui::small_button("Its own copy...") {
            self.copy_name = format!("{variant}_{}", entry.unit.to_lowercase());
            gui::open_popup("##own_copy");
        }

        if gui::is_item_hovered() {
            gui::set_tooltip(
                "A copy of the unit's definition under a new name, worn by this unit only: what to edit without changing the others that wear it.",
            );
        }

        if gui::begin_popup("##own_copy") {
            gui::text("The copy's name:");
            gui::set_next_item_width(360.0);
            gui::input_text("##copy_name", None, &mut self.copy_name, 128, 0);
            let name = self.copy_name.trim().to_lowercase();
            let exists = definitions.contains(&name);
            if exists {
                gui::text_colored(theme::warning(), "A definition of that name exists.");
            }

            gui::begin_disabled(name.is_empty() || exists);
            if gui::button("Copy and wear it") {
                gui::close_current_popup();
                let source = variant_path(&variant);
                let bytes = e.vfs().and_then(|v| v.read(&source).ok());
                let to = format!("{DEFINITIONS}{name}.variantmeshdefinition");
                match bytes.map(|b| wce_foundation::files::write_replacing(e.loose_dir().join(&to), b)) {
                    Some(Ok(())) => {
                        e.loose_changed(&to);
                        self.listings = None;
                        edits.push(("variant", name.clone()));
                        self.pending_preview = Some((to.clone(), self.unit.clone(), self.faction.clone()));
                        if let Err(error) = e.open_path_from(self, &to, true, true) {
                            e.status(&error, true);
                        }
                    }

                    Some(Err(error)) => e.status(&format!("{to}: {error}"), true),
                    None => e.status(&format!("{source} isn't in the packs"), true),
                }
            }

            gui::end_disabled();
            gui::end_popup();
        }

        // The height.
        let number = |r: &Row, c: &str| r.get(c).and_then(|v| v.trim().parse::<f32>().ok()).unwrap_or(0.0);
        let mut height = self.height_draft.unwrap_or([number(&row, "height_scale"), number(&row, "height_variation")]);
        gui::set_next_item_width(-130.0);
        if gui::drag_float("height_scale", &mut height[0], 0.005, 0.1, 5.0, "%.3f") {
            self.height_draft = Some(height);
        }

        if gui::is_item_deactivated_after_edit() {
            edits.push(("height_scale", number_text(height[0])));
            self.height_draft = None;
        }

        gui::set_next_item_width(-130.0);
        if gui::drag_float("height_variation", &mut height[1], 0.001, 0.0, 1.0, "%.3f") {
            self.height_draft = Some(height);
        }

        if gui::is_item_deactivated_after_edit() {
            edits.push(("height_variation", number_text(height[1])));
            self.height_draft = None;
        }

        // The unit's own colours for the faction (its soldiers').
        let mut has_own = own.is_some();
        let mut colour_edit = None;
        if gui::checkbox(&format!("Own colours for {faction}"), &mut has_own) {
            colour_edit = Some(has_own.then(|| {
                let f = c.colours.get(&faction).copied().unwrap_or([[1.0; 3]; 4]);
                [f[0], f[1], f[2]]
            }));
        }

        if gui::is_item_hovered() {
            gui::set_tooltip("unit_variants_colours: the unit's soldiers wear these instead of the faction's colours.");
        }

        if let Some(own) = &own {
            let mut rgb: [[f32; 3]; 3] = std::array::from_fn(|k| {
                hex_to_rgb(own.get(["primary_colour_hex", "secondary_colour_hex", "tertiary_colour_hex"][k]).map_or("", String::as_str))
            });
            let mut changed = false;
            for (k, name) in ["Primary##own", "Secondary##own", "Tertiary##own"].iter().enumerate() {
                gui::color_edit3(name, &mut rgb[k], 0);
                changed |= gui::is_item_deactivated_after_edit();
            }

            if changed {
                colour_edit = Some(Some(rgb));
            }

            if !own_changed {
                gui::text_colored(theme::dim(), "(the game's row)");
            }
        }

        // Revert.
        let mut revert = false;
        if project_rows > 0 {
            revert = gui::small_button(&format!("Revert ({project_rows} rows of the project)"));
        }

        // Applied after the reads above.
        let mut todo = vec![];
        if !edits.is_empty() {
            todo.push(LooksEdit::Set(edits));
        }

        if let Some(colour) = colour_edit {
            todo.push(LooksEdit::Colours(colour));
        }

        if revert {
            todo.push(LooksEdit::Revert);
        }

        if !todo.is_empty()
            && let Err(error) = self.edit_looks(e, todo)
        {
            e.status(&error, true);
        }
    }

    /// Reads the looks' tables if they aren't.
    fn ensure_looks(&mut self, e: &Editor) -> Result<(), String> {
        if self.looks.is_some() {
            return Ok(());
        }

        let (Some(vfs), Some(schema)) = (e.vfs().cloned(), self.schema.clone()) else {
            return Err("the unit tables aren't read yet".to_owned());
        };
        self.looks = Some(LooksTables::load(&vfs, &schema, &e.loose_dir())?);
        Ok(())
    }

    /// Changes the previewed unit's rows for its faction, then saves them.
    fn edit_looks(&mut self, e: &mut Editor, todo: Vec<LooksEdit>) -> Result<(), String> {
        self.ensure_looks(e)?;
        let c = self.catalog.clone().ok_or("the unit tables aren't read yet")?;
        let entry = c.entry(&self.unit, &self.faction).cloned().ok_or("the unit has no unit_variants row")?;
        let faction = self.faction.clone();
        let this_row = [("unit", entry.unit.as_str()), ("faction", entry.faction.as_str())];
        let key = colours_key(&entry.name, &faction);
        let looks = self.looks.as_mut().ok_or("no tables")?;
        for edit in todo {
            match edit {
                LooksEdit::Set(edits) => {
                    let row = looks.table("unit_variants").row(&this_row).ok_or("no unit_variants row")?;
                    let values: Vec<(&str, &str)> = edits.iter().map(|(c, v)| (*c, v.as_str())).collect();
                    looks.table_mut("unit_variants").set(&row, &values)?;

                    // A new definition's name in `variants`, as the game's rows have theirs.
                    if let Some((_, v)) = edits.iter().find(|(c, _)| *c == "variant")
                        && looks.table("variants").row(&[("variant_name", v)]).is_none()
                    {
                        let base = looks.table("variants").game_rows().first().cloned().unwrap_or_default();
                        looks.table_mut("variants").set(&base, &[("variant_name", v)])?;
                    }
                }

                LooksEdit::Colours(None) => looks.table_mut("unit_variants_colours").remove(&key),
                LooksEdit::Colours(Some(rgb)) => {
                    let table = looks.table_mut("unit_variants_colours");
                    let base = table.row(&key).or_else(|| table.game_rows().first().cloned()).unwrap_or_default();
                    let hex: Vec<String> = rgb.iter().map(|c| rgb_to_hex(*c)).collect();
                    let values = [
                        ("unit_variant", entry.name.as_str()),
                        ("faction", faction.as_str()),
                        ("soldier_type", "soldier"),
                        ("primary_colour_hex", hex[0].as_str()),
                        ("secondary_colour_hex", hex[1].as_str()),
                        ("tertiary_colour_hex", hex[2].as_str()),
                    ];
                    table.set(&base, &values)?;
                }

                LooksEdit::Revert => {
                    looks.table_mut("unit_variants").remove(&this_row);
                    looks.table_mut("unit_variants_colours").remove(&key);
                }
            }
        }

        self.save_looks(e);
        Ok(())
    }

    fn draw_problems(&mut self) -> Vec<Action> {
        let mut actions = vec![];
        if !gui::begin("Problems", None, 0) {
            gui::end();
            return actions;
        }

        if self.problems.is_empty() {
            gui::text_colored(theme::dim(), "Nothing wrong found in the file.");
        }

        gui::clipped(self.problems.len(), |k| {
            let p = &self.problems[k];
            let at =
                if p.path.is_empty() { "root".to_owned() } else { p.path.iter().map(ToString::to_string).collect::<Vec<_>>().join("/") };
            if gui::selectable(&format!("{}  [{at}]##problem{k}", p.text), self.selected.as_ref() == Some(&p.path), 0) {
                actions.push(Action::Select(Some(p.path.clone())));
            }
        });
        gui::end();
        actions
    }

    // ---- the Inspector

    /// A file's path: its name above, the text, and a button listing the packs' files.
    /// A file's path: a picture of what it names, its name, the text, and a button browsing the packs' files with pictures.
    /// `slot`: the slot the node is an alternative of, for what the browser lists.
    #[allow(clippy::too_many_arguments, reason = "a field's description, laid out in the call")]
    fn path_field(
        &mut self,
        e: &mut Editor,
        elem: usize,
        name: &str,
        tip: &str,
        items: &[String],
        kind: thumbs::Browse,
        slot: Option<&wce_formats::variant_mesh::Slot>,
    ) {
        let Some(value) = document(e).map(|d| d.xml().get(elem, name).unwrap_or_default()) else { return };
        let side = 2.0 * gui::frame_height_with_spacing() + gui::text_line_height_with_spacing() - gui::item_spacing().y;
        let thing = match kind {
            thumbs::Browse::Model => thumbs::Thing::Model(normalize(&value)),
            thumbs::Browse::Definition => thumbs::Thing::Definition(normalize(&value)),
            thumbs::Browse::Texture => thumbs::Thing::Texture(normalize(&value)),
        };
        let at = gui::cursor_screen_pos();
        gui::dummy(v2(side, side));
        let dl = gui::window_draw_list();
        match (!value.trim().is_empty()).then(|| self.picture(e, &thing, false)).flatten() {
            Some(img) => dl.image(img.id, at, v2(at.x + side, at.y + side), v2(0.0, 0.0), v2(1.0, 1.0), 0xFFFF_FFFF),
            None => dl.rect_filled(
                at,
                v2(at.x + side, at.y + side),
                theme::u32(theme::mix(theme::palette_background(), theme::text(), 0.07)),
                4.0,
            ),
        }

        if gui::is_item_hovered() && !value.is_empty() {
            gui::set_tooltip(&value);
        }

        gui::same_line();
        gui::begin_group();
        gui::text_colored(theme::dim(), name);
        if gui::is_item_hovered() {
            gui::set_tooltip(tip);
        }

        let mut text = value.clone();
        gui::set_next_item_width(-gui::button_width(wce_editor::icons::IMAGE) - 8.0);
        if gui::input_text(&format!("##{name}{elem}"), None, &mut text, 512, 0)
            && let Some(d) = document(e)
        {
            d.set_value(elem, name, &text, gesture(elem, name));
        }

        if gui::is_item_deactivated_after_edit()
            && let Some(d) = document(e)
        {
            d.break_merge();
        }

        if gui::is_item_hovered() {
            gui::set_tooltip(&format!("{tip}\n{value}"));
        }

        gui::same_line();
        let field = if kind == thumbs::Browse::Texture { Field::Decal } else { Field::Part };
        let folders =
            slot.and_then(|s| self.kinds.as_ref().map_or_else(|| SlotKinds::default().folders(s, field), |k| k.folders(s, field)));
        let only = slot.zip(folders.as_deref()).map(|(s, f)| (s.name.as_str(), f));
        if let Some(p) = self.browse(e, &format!("{name}{elem}"), &normalize(&value), items, kind, only)
            && let Some(d) = document(e)
        {
            d.set_value(elem, name, &p, 0);
        }

        gui::end_group();
    }

    fn inspect_mesh(&mut self, e: &mut Editor, path: &NodePath) {
        let listings = {
            let l = self.listings(e);
            (l.models.clone(), l.textures.clone())
        };
        let vfs = e.vfs().cloned();
        let Some(tree) = document(e).map(|d| d.tree()) else { return };
        let Some(Node::Mesh(mesh)) = tree.node(path) else { return };
        let elem = mesh.element;
        let slot = match path.split_last().and_then(|(_, up)| tree.node(up)) {
            Some(Node::Slot(s)) => Some(s),
            _ => None,
        };
        gui::separator_text(if path.is_empty() { "The file's root" } else { "Mesh" });
        self.path_field(e, elem, "model", "The part's model (RMV2).", &listings.0, thumbs::Browse::Model, slot);
        self.path_field(
            e,
            elem,
            "decal_diffuse",
            "A decal's colours: a texture, or a .texture_array of them (one picked per soldier).",
            &listings.1,
            thumbs::Browse::Texture,
            slot,
        );
        self.path_field(
            e,
            elem,
            "decal_normal",
            "The decal's normals: a texture, or a .texture_array.",
            &listings.1,
            thumbs::Browse::Texture,
            slot,
        );
        self.path_field(e, elem, "imposter_model", "The model drawn far away.", &listings.0, thumbs::Browse::Model, slot);
        let Some(d) = document(e) else { return };

        gui::separator_text("Colour mask");
        for k in 0..3 {
            let name = format!("mask{k}");
            let value = d.xml().get(elem, &name);
            let mut current = match value.as_deref().map(str::trim) {
                Some("0") => 1,
                Some("1") => 2,
                Some("2") => 3,
                _ => 0,
            };
            let labels = ["channel's own", "primary (0)", "secondary (1)", "uniform (2)"];
            gui::set_next_item_width(-60.0);
            if gui::combo(&name, &mut current, &labels) {
                if current == 0 {
                    d.remove_value(elem, &name);
                } else {
                    d.set_value(elem, &name, &(current - 1).to_string(), 0);
                }
            }

            if gui::is_item_hovered() {
                gui::set_tooltip(&format!(
                    "{name}: the faction colour the colour mask's channel {k} takes (the editor's reading of it: not traced in the game)."
                ));
            }
        }

        gui::separator_text("Tints");
        let mut remove = None;
        for (t, tint) in mesh.tints.iter().enumerate() {
            gui::push_id_int(t as i32);
            let mut rgb = [tint.red, tint.green, tint.blue];
            if gui::color_edit3(&format!("Tint {}", t + 1), &mut rgb, 0) {
                for (c, name) in ["red", "green", "blue"].iter().enumerate() {
                    d.set_value(tint.element, name, &number(rgb[c]), gesture(tint.element, "rgb"));
                }
            }

            if gui::is_item_deactivated_after_edit() {
                d.break_merge();
            }

            let mut range = tint.random_range;
            gui::set_next_item_width(90.0);
            if gui::drag_float("random range", &mut range, 0.005, 0.0, 1.0, "%.3f") {
                d.set_value(tint.element, "random_range", &number(range), gesture(tint.element, "random_range"));
            }

            if gui::is_item_deactivated_after_edit() {
                d.break_merge();
            }

            gui::same_line();
            let shown = self.pins.tints.get(path) == Some(&t);
            if gui::small_button(if shown { "Shown" } else { "Show" }) {
                if shown {
                    self.pins.tints.remove(path);
                } else {
                    self.pins.tints.insert(path.clone(), t);
                }
            }

            if gui::is_item_hovered() {
                gui::set_tooltip("The soldiers wear this tint, whatever the seed picks.");
            }

            gui::same_line();
            if gui::small_button("Remove") {
                remove = Some(tint.element);
            }

            gui::pop_id();
        }

        if let Some(t) = remove {
            let _ = d.structural(&Structural::Remove, t);
            self.pins.tints.remove(path);
        }

        if gui::small_button("Add a tint") {
            let _ = d.structural(&Structural::AddTint, elem);
        }

        if !mesh.meta_data.is_empty() {
            gui::separator_text("Meta data");
            for m in &mesh.meta_data {
                gui::text(m);
            }
        }

        other_attributes(d, elem, &["model", "decal_diffuse", "decal_normal", "imposter_model", "mask0", "mask1", "mask2"]);

        // The decals of a texture array: one shown, whatever the seed picks.
        if let (Some(vfs), Some(name)) = (vfs, mesh.decal_diffuse.as_deref().filter(|n| !n.trim().is_empty())) {
            let read = |p: &str| vfs.read(p).ok();
            let all = decals(&read, name);
            if all.len() > 1 {
                gui::separator_text("Decal shown");
                let pinned = self.pins.decals.get(path).copied();
                if gui::radio_button("The seed's", pinned.is_none()) {
                    self.pins.decals.remove(path);
                }

                let cells: Vec<(thumbs::Thing, String, bool)> = all
                    .iter()
                    .enumerate()
                    .map(|(k, t)| (thumbs::Thing::Texture(t.clone()), file_name(t).to_owned(), pinned == Some(k)))
                    .collect();
                if let Some(k) = self.grid(e, &cells, "decal") {
                    if pinned == Some(k) {
                        self.pins.decals.remove(path);
                    } else {
                        self.pins.decals.insert(path.clone(), k);
                    }
                }
            }
        }
    }

    fn inspect_slot(&mut self, e: &mut Editor, path: &NodePath) {
        let vfs = e.vfs().cloned();
        let Some(d) = document(e) else { return };
        let tree = d.tree();
        let Some(Node::Slot(slot)) = tree.node(path) else { return };
        let elem = slot.element;
        gui::separator_text("Slot");
        let mut name = slot.name.clone();
        gui::text_colored(theme::dim(), "name");
        gui::set_next_item_width(-gui::button_width("...") - 8.0);
        if gui::input_text(&format!("##name{elem}"), None, &mut name, 128, 0) {
            d.set_value(elem, "name", &name, gesture(elem, "name"));
        }

        if gui::is_item_deactivated_after_edit() {
            d.break_merge();
        }

        if gui::is_item_hovered() {
            gui::set_tooltip(
                "Any name: the game reads it as a label (docs/re/variant_mesh_slots.md), but for the ones it looks up: a soldier's \
                 shield and weapon_1 to weapon_4, a ship piece's _dm<state>.",
            );
        }

        // The names the game's slots have, most used first.
        gui::same_line();
        let names: Vec<String> =
            self.kinds.as_ref().map(|k| k.names().into_iter().map(|(n, c)| format!("{n}  ({c})")).collect()).unwrap_or_default();
        let filter = self.combo_filters.entry("slot_name#pick".to_owned()).or_default();
        if let Some(p) =
            pick_button(&format!("names{elem}"), "The names the game's slots have (how many), most used first.", filter, &slot.name, &names)
        {
            let picked = p.rsplit_once("  (").map_or(p.as_str(), |(n, _)| n).to_owned();
            d.set_value(elem, "name", &picked, 0);
        }

        gui::push_text_wrap_pos(0.0);
        if let Some(m) = crate::slot_kinds::meaning(&slot.name) {
            gui::text_colored(theme::palette().highlight, &format!("{} {m}", wce_editor::icons::INFO));
        } else if let Some(like) = crate::slot_kinds::near_miss(&slot.name) {
            gui::text_colored(
                theme::warning(),
                &format!(
                    "{} Nearly '{like}', which the game looks up by name; this name it won't find. (A slot's name and its attach point's \
                     bone are numbered apart: weapon_2 hangs from weapon_02.)",
                    wce_editor::icons::TRIANGLE_ALERT
                ),
            );
        }

        gui::pop_text_wrap_pos();

        // The points: the weapon bones, and the parent model's own.
        let parent_model = parent_model(&tree, path);
        let mut points: Vec<String> = WEAPON_POINTS.iter().map(|p| (*p).to_owned()).collect();
        if let (Some(vfs), Some(model)) = (&vfs, &parent_model) {
            let read = |p: &str| vfs.read(p).ok();
            points.extend(attachment_points(&read, model));
        }

        let mut point = slot.attach_point.clone().unwrap_or_default();
        gui::text_colored(theme::dim(), "attach_point");
        gui::set_next_item_width(-gui::button_width("...") - 8.0);
        if gui::input_text(&format!("##attach_point{elem}"), None, &mut point, 128, 0) {
            if point.is_empty() {
                d.remove_value(elem, "attach_point");
            } else {
                d.set_value(elem, "attach_point", &point, gesture(elem, "attach_point"));
            }
        }

        if gui::is_item_deactivated_after_edit() {
            d.break_merge();
        }

        if gui::is_item_hovered() {
            gui::set_tooltip(&format!(
                "What the slot's part hangs from: a weapon bone (weapon_01 to 05, moved by the animations), or an attachment point of the \
                 model above it{}. Empty: skinned to the body.",
                parent_model.as_deref().map(|m| format!(" ({})", file_name(m))).unwrap_or_default()
            ));
        }

        gui::same_line();
        let filter = self.combo_filters.entry("attach_point#pick".to_owned()).or_default();
        if let Some(p) =
            pick_button(&format!("points{elem}"), "The weapon bones and the points of the model above.", filter, &point, &points)
        {
            d.set_value(elem, "attach_point", &p, 0);
        }

        let mut always = slot.probability.is_none() && d.xml().get(elem, "probability").is_none();
        if gui::checkbox("Always filled", &mut always) {
            if always {
                d.remove_value(elem, "probability");
            } else {
                d.set_value(elem, "probability", "0.5", 0);
            }
        }

        if !always {
            let mut p = slot.probability.unwrap_or(0.0);
            gui::same_line();
            gui::set_next_item_width(-1.0);
            if gui::slider_float("##probability", &mut p, 0.0, 1.0, "probability %.2f") {
                d.set_value(elem, "probability", &number(p), gesture(elem, "probability"));
            }

            if gui::is_item_deactivated_after_edit() {
                d.break_merge();
            }

            if gui::is_item_hovered() {
                gui::set_tooltip("How often the slot is filled at all (the editor's reading of it: not traced in the game).");
            }
        }

        other_attributes(d, elem, &["name", "attach_point", "probability"]);

        // The alternatives, pictured: a click shows one whatever the seed picks, a second click lets the seed pick again.
        gui::separator_text("Alternatives");
        let pin = self.pins.slots.get(path).copied();
        if gui::radio_button("The seed's", pin.is_none()) {
            self.pins.slots.remove(path);
        }

        gui::same_line();
        if gui::radio_button("Nothing", pin == Some(SlotPin::Off)) {
            self.pins.slots.insert(path.clone(), SlotPin::Off);
        }

        let cells: Vec<(thumbs::Thing, String, bool)> = slot
            .alternatives
            .iter()
            .enumerate()
            .map(|(k, a)| {
                let mut at = path.clone();
                at.push(k);
                (thumbs::Thing::Alternative(at), alternative_label(a), pin == Some(SlotPin::Alternative(k)))
            })
            .collect();
        if let Some(k) = self.grid(e, &cells, "alternative") {
            if pin == Some(SlotPin::Alternative(k)) {
                self.pins.slots.remove(path);
            } else {
                self.pins.slots.insert(path.clone(), SlotPin::Alternative(k));
            }
        }
    }

    fn inspect_reference(&mut self, e: &mut Editor, path: &NodePath) -> Option<String> {
        let definitions = self.listings(e).definitions.clone();
        let tree = document(e)?.tree();
        let Some(Node::Reference { element, definition }) = tree.node(path) else { return None };
        let slot = match path.split_last().and_then(|(_, up)| tree.node(up)) {
            Some(Node::Slot(s)) => Some(s),
            _ => None,
        };
        gui::separator_text("Reference");
        gui::text_wrapped("Another definition file, used whole as this alternative.");
        self.path_field(e, element, "definition", "The definition file.", &definitions, thumbs::Browse::Definition, slot);
        let file = normalize(definition);
        (!file.is_empty() && gui::button("Open it")).then_some(file)
    }
}

fn mesh_label(mesh: &VariantMesh, root: bool) -> String {
    let model = mesh.model.as_deref().map(str::trim).filter(|m| !m.is_empty());
    let mut label = match (model, root) {
        (Some(m), _) => file_name(m).to_owned(),
        (None, true) => "root".to_owned(),
        (None, false) => "(group)".to_owned(),
    };

    if !mesh.tints.is_empty() {
        label.push_str(&format!("  [{} tints]", mesh.tints.len()));
    }

    label
}

fn alternative_label(a: &Alternative) -> String {
    match a {
        Alternative::Mesh(m) => mesh_label(m, false),
        Alternative::Reference { definition, .. } => format!("-> {}", file_name(definition)),
    }
}

/// The model of the nearest mesh above a slot.
fn parent_model(tree: &VariantMeshFile, slot: &NodePath) -> Option<String> {
    (0..slot.len()).rev().step_by(2).find_map(|k| match tree.node(&slot[..k]) {
        Some(Node::Mesh(m)) => m.model.clone().filter(|m| !m.trim().is_empty()),
        _ => None,
    })
}

/// The attributes the panel doesn't know, as text.
fn other_attributes(d: &mut VmdDocument, elem: usize, known: &[&str]) {
    let names: Vec<String> =
        d.xml().elements()[elem].attrs.iter().map(|a| a.name.clone()).filter(|n| !known.contains(&n.as_str())).collect();
    if names.is_empty() {
        return;
    }

    gui::separator_text("Other attributes");
    for name in names {
        let mut value = d.xml().get(elem, &name).unwrap_or_default();
        if gui::input_text(&format!("{name}##other{elem}"), None, &mut value, 256, 0) {
            d.set_value(elem, &name, &value, gesture(elem, &name));
        }

        if gui::is_item_deactivated_after_edit() {
            d.break_merge();
        }
    }
}

impl Tool for UnitTool {
    fn id(&self) -> &str {
        UNIT_TOOL_ID
    }

    fn title(&self) -> &str {
        "Unit"
    }

    fn icon(&self) -> &str {
        wce_editor::icons::USER
    }

    fn opens(&self) -> Vec<String> {
        vec![".variantmeshdefinition".to_owned()]
    }

    fn project_folder(&self) -> &'static str {
        "loose/variantmeshes/variantmeshdefinitions"
    }

    fn default_layout(&self, dockspace: gui::ImGuiID) {
        let (left, center) = gui::dock_builder_split(dockspace, gui::DIR_LEFT, 0.24);
        let (left_bottom, left_top) = gui::dock_builder_split(left, gui::DIR_DOWN, 0.45);
        let (right, center) = gui::dock_builder_split(center, gui::DIR_RIGHT, 0.3);
        let (right_bottom, right_top) = gui::dock_builder_split(right, gui::DIR_DOWN, 0.4);
        let (bottom, center) = gui::dock_builder_split(center, gui::DIR_DOWN, 0.22);
        gui::dock_builder_dock_window("Viewport", center);
        gui::dock_builder_dock_window("Structure", left_top);
        gui::dock_builder_dock_window("Unit files", left_bottom);
        gui::dock_builder_dock_window("Units", left_bottom);
        gui::dock_builder_dock_window("Inspector", right_top);
        gui::dock_builder_dock_window("Preview", right_bottom);
        gui::dock_builder_dock_window("Problems", bottom);
        gui::dock_builder_dock_window("Assets", bottom);
        gui::dock_builder_dock_window("Log", bottom);
        gui::dock_builder_dock_window("History", bottom);
    }

    fn lights_scene(&self) -> bool {
        true
    }

    /// A click picks the part under the cursor (its node selected in the tree and outlined), or nothing.
    fn viewport_input(&mut self, e: &mut Editor, event: &wce_editor::tool::ViewportEvent) -> bool {
        if event.kind != wce_editor::tool::ViewportEventKind::Click || event.button != 0 || document(e).is_none() {
            return false;
        }

        if self.ship.on {
            let picked = self.ship_pick(e, event.x, event.y, event.hit_source);
            if let Some(p) = &picked {
                e.status(&self.ship_part_name(p), false);
            }

            self.selected = picked;
            return true;
        }

        let picked = self.pick_part(e, event.x, event.y);
        if let Some((path, model)) = &picked {
            let through = match document(e)
                .map(|d| d.tree())
                .and_then(|t| t.node(path).map(|n| matches!(n, wce_formats::variant_mesh::Node::Reference { .. })))
            {
                Some(true) => ", through its reference",
                _ => "",
            };
            e.status(&format!("{}{through}", file_name(model)), false);
        }

        self.selected = picked.map(|(p, _)| p);
        true
    }

    fn register_commands(&self, commands: &mut CommandRegistry<Editor>) {
        register(commands);
    }

    fn activated(&mut self, e: &mut Editor) {
        self.settings = e.tool_settings(UNIT_TOOL_ID);
        self.hide_others(e);
        self.load_catalog(e);
    }

    fn deactivated(&mut self, e: &mut Editor) {
        self.show_others(e);
    }

    fn game_changed(&mut self, e: &mut Editor) {
        self.clear(e);
        self.clear_ship(e);
        *self = Self::default();
    }

    fn update(&mut self, e: &mut Editor) {
        self.hide_others(e);
        self.load_catalog(e);
        self.load_kinds(e);
        self.poll();
        self.light(e);
        let Some(id) = e.docs.of_tool(UNIT_TOOL_ID).filter(|&id| e.docs.find_as::<VmdDocument>(id).is_some()) else {
            self.clear(e);
            self.clear_ship(e);
            self.doc = 0;
            return;
        };

        self.update_ground(e, id);

        // The catalog arrived after the document: its unit now.
        if id != self.doc {
            self.document_opened(e, id);
            let path = self.preview_path.clone();
            self.ship_opened(e, &path);
        } else if self.default_pending && self.catalog.is_some() {
            let path = self.preview_path.clone();
            self.preview_default(e, &path);
        }

        // A ship is told by the unit tables, which may come after the file.
        if !self.ship.checked && self.catalog.is_some() {
            let path = self.preview_path.clone();
            self.ship_opened(e, &path);
        }

        self.update_problems(e);
        let wanted = self.wanted();
        if self.catalog.is_some()
            && !wanted.unit.is_empty()
            && self.models_for.as_ref() != Some(&wanted)
            && self.loading.is_none()
            && !self.failed.contains(&wanted)
        {
            self.load_models(e, wanted);
        }

        if self.ship.on {
            // The soldiers are loaded all the same (their pose and colours make the pictures), not shown.
            self.clear(e);
            self.update_ship(e);
        } else {
            self.dress(e);
            self.pose(e);
        }

        self.update_thumbs(e);
    }

    fn draw_panels(&mut self, e: &mut Editor) {
        let mut actions = self.draw_structure(e);
        actions.extend(self.draw_files(e));
        actions.extend(self.draw_units(e));
        actions.extend(self.draw_preview(e));
        actions.extend(self.draw_problems());
        self.apply(e, actions);
    }

    fn inspect(&mut self, e: &mut Editor, _: &mut dyn Inspector) -> bool {
        let Some(path) = self.selected.clone() else {
            if document(e).is_some() {
                gui::text_colored(theme::dim(), "Select a node in the Structure panel.");
                return true;
            }

            return false;
        };

        let Some(d) = document(e) else { return false };
        let node = d.tree().node(&path).map(|n| match n {
            Node::Mesh(_) => 0,
            Node::Slot(_) => 1,
            Node::Reference { .. } => 2,
        });
        match node {
            Some(0) => self.inspect_mesh(e, &path),
            Some(1) => self.inspect_slot(e, &path),
            Some(2) => {
                if let Some(file) = self.inspect_reference(e, &path)
                    && let Err(error) = e.open_path_from(self, &file, true, true)
                {
                    e.status(&error, true);
                }
            }

            _ => self.selected = None,
        }

        true
    }

    fn viewport_overlay(&mut self, e: &mut Editor, origin: gui::Vec2, size: gui::Vec2) {
        if document(e).is_some() {
            self.draw_weather_chips(e, origin, size);
        }
    }

    fn inspector_subject(&mut self, _: &Editor) -> Option<u64> {
        self.selected.as_ref().map(|p| hash_of(p) | 1)
    }

    fn export_menu(&mut self, e: &mut Editor) -> bool {
        if gui::menu_item("To mod pack...", None, false, true) {
            let files = project_files(&e.loose_dir());
            if files.is_empty() {
                e.status("the project has no definitions or DB rows of the Unit tool to export", true);
            } else if let Some(path) = e.pick_export_file(
                "unit_pack",
                "wce_units.pack",
                &[("Mod pack".to_owned(), "*.pack".to_owned()), ("All files".to_owned(), "*.*".to_owned())],
            ) {
                match export_pack(e, &path) {
                    Ok(n) => e.status(&format!("wrote {n} files into {}", path.display()), false),
                    Err(error) => e.status(&error, true),
                }
            }
        }

        if gui::is_item_hovered() {
            gui::set_tooltip(
                "Into a mod pack (a new one, or added to one): the project's definitions (its loose variantmeshes folder) and the \
                 DB rows of the Unit tool.",
            );
        }

        true
    }

    fn open(&mut self, e: &mut Editor, path: &str, from_packs: bool) -> Result<Box<dyn Document>, String> {
        let (source, bytes) = if from_packs {
            // The project's loose copy (where Save writes it) wins over the packs: it is mounted before them.
            let vfs = e.vfs().ok_or("the game packs are not mounted")?;
            let p = normalize(path);
            let owner = vfs.owner(&p).ok_or_else(|| format!("{p} is not in the packs"))?;
            let bytes = vfs.read(&p).map_err(|e| e.to_string())?;
            let pack = &vfs.packs()[owner];
            let label = if e.loose_copy(&p).is_some() { "project".to_owned() } else { pack.name.clone() };
            (Source { vfs_path: p, disk: pack.path.clone(), label }, bytes)
        } else {
            let bytes = std::fs::read(path).map_err(|e| format!("cannot read {path}: {e}"))?;
            (Source { vfs_path: String::new(), disk: PathBuf::from(path), label: "loose".to_owned() }, bytes)
        };

        Ok(Box::new(VmdDocument::new(source, bytes, e.loose_dir())?))
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}

/// File > Export > To mod pack: the project's files of the tool into the pack (made, or added to). Returns how many.
fn export_pack(e: &Editor, path: &std::path::Path) -> Result<usize, String> {
    let files = project_files(&e.loose_dir());
    if files.is_empty() {
        return Err("the project has no definitions or DB rows of the Unit tool to export".to_owned());
    }

    wce_vfs::mod_pack::write_files(e.game(), path, &[], &files).map_err(|error| format!("{}: {error}", path.display()))?;
    Ok(files.len())
}

/// What File > Export > To mod pack takes from the project's loose folder: its definitions (`variantmeshes/**.variantmeshdefinition`)
/// and the tool's DB files, as (path in the pack, bytes).
fn project_files(loose: &std::path::Path) -> Vec<(String, Vec<u8>)> {
    let mut out = vec![];
    let mut pending = vec![loose.join("variantmeshes")];
    while let Some(dir) = pending.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else { continue };
        for entry in entries.flatten() {
            let p = entry.path();
            if p.is_dir() {
                pending.push(p);
            } else if p.extension().is_some_and(|x| x.eq_ignore_ascii_case("variantmeshdefinition"))
                && let (Ok(rel), Ok(bytes)) = (p.strip_prefix(loose), std::fs::read(&p))
            {
                out.push((rel.to_string_lossy().replace('\\', "/"), bytes));
            }
        }
    }

    for table in crate::db::TABLES {
        let path = project_path(table);
        if let Ok(bytes) = std::fs::read(loose.join(&path)) {
            out.push((path, bytes));
        }
    }

    out.sort();
    out
}

// ---- script commands

fn ok(m: impl Into<String>) -> CommandResult {
    CommandResult::success(m)
}

fn fail(m: impl Into<String>) -> CommandResult {
    CommandResult::fail(m)
}

fn with_unit(e: &mut Editor, f: impl FnOnce(&mut UnitTool, &mut Editor) -> CommandResult) -> CommandResult {
    e.with_tool::<UnitTool, _>(f).unwrap_or_else(|| fail("the unit tool is not available"))
}

/// `0/1/2` as a path; `root` or `-` for the root.
fn parse_path(text: &str) -> Option<NodePath> {
    if matches!(text, "root" | "-") {
        return Some(vec![]);
    }

    text.split('/').map(|k| k.parse().ok()).collect()
}

fn register(c: &mut CommandRegistry<Editor>) {
    c.add("unit_preview", "unit_preview <unit> [faction]", "the unit the open definition is shown on", |e, a| {
        let Some(unit) = a.first().cloned() else { return fail("usage: unit_preview <unit> [faction]") };
        let faction = a.get(1).cloned().unwrap_or_default();
        with_unit(e, |t, e| {
            let path = document(e).map(|d| d.source().vfs_path.clone()).unwrap_or_default();
            t.set_preview(e, &path, unit, faction);
            ok("")
        })
    });

    c.add(
        "unit_weather",
        "unit_weather [<category> <value|any> | file <path>]",
        "the weather chips: set a filter (climate, season, precipitation, time_of_day, climate_change_index, map, snow_attrition) or \
         the file; prints the filters, the file and the ground",
        |e, a| {
            with_unit(e, |t, e| {
                match (a.first().map(String::as_str), a.get(1)) {
                    (Some("file"), Some(f)) => t.set_weather_file(e, f),
                    (Some(category), Some(v)) => {
                        let v = if v == "any" { "" } else { v.as_str() };
                        if let Err(m) = t.set_weather_filter(e, category, v) {
                            return fail(&m);
                        }
                    }

                    (None, _) => {}
                    _ => return fail("usage: unit_weather [<category> <value|any> | file <path>]"),
                }

                ok(format!("filters {:?} file '{}' ground {}", t.settings.weather_query, t.settings.weather, t.ground_texture(e)))
            })
        },
    );
    c.add("unit_seed", "unit_seed <n> [side by side]", "the first soldier's seed, and how many stand side by side", |e, a| {
        let Some(seed) = a.first().and_then(|s| s.parse().ok()) else { return fail("usage: unit_seed <n> [count]") };
        let grid = a.get(1).and_then(|s| s.parse().ok());
        with_unit(e, |t, _| {
            t.seed = seed;
            if let Some(g) = grid {
                t.grid = g;
            }

            ok("")
        })
    });

    c.add("unit_select", "unit_select <path|none>", "select a node by its path (0/1/0; root)", |e, a| {
        let path = match a.first().map(String::as_str) {
            Some("none") | None => None,
            Some(p) => match parse_path(p) {
                Some(p) => Some(p),
                None => return fail("a path is numbers separated by /"),
            },
        };
        with_unit(e, |t, e| {
            if let Some(p) = &path
                && document(e).is_none_or(|d| d.tree().node(p).is_none())
            {
                return fail("no such node");
            }

            t.selected = path;
            ok("")
        })
    });

    c.add("unit_pin", "unit_pin <slot path> <random|off|N>", "what a slot shows in the preview (N counts from 1)", |e, a| {
        let (Some(path), Some(what)) = (a.first().and_then(|p| parse_path(p)), a.get(1).cloned()) else {
            return fail("usage: unit_pin <slot path> <random|off|N>");
        };
        with_unit(e, |t, _| {
            match what.as_str() {
                "random" => {
                    t.pins.slots.remove(&path);
                }

                "off" => {
                    t.pins.slots.insert(path, SlotPin::Off);
                }

                n => match n.parse::<usize>() {
                    Ok(n) if n > 0 => {
                        t.pins.slots.insert(path, SlotPin::Alternative(n - 1));
                    }

                    _ => return fail("random, off or a number from 1"),
                },
            }

            ok("")
        })
    });

    c.add("unit_set", "unit_set <attribute> <value>", "set an attribute of the selected node (one undo entry)", |e, a| {
        let (Some(name), Some(value)) = (a.first().cloned(), a.get(1).cloned()) else { return fail("usage: unit_set <attribute> <value>") };
        with_unit(e, |t, e| {
            let Some(path) = t.selected.clone() else { return fail("nothing selected") };
            let Some(d) = document(e) else { return fail("no definition open") };
            let Some(elem) = d.tree().node(&path).map(|n| n.element()) else { return fail("no such node") };
            d.set_value(elem, &name, &value, 0);
            ok("")
        })
    });

    c.add(
        "unit_edit",
        "unit_edit <add_slot NAME|add_mesh|add_reference|add_tint|remove|duplicate|up|down>",
        "a structural edit of the selected node",
        |e, a| {
            let op = match (a.first().map(String::as_str), a.get(1)) {
                (Some("add_slot"), name) => Structural::AddSlot(name.cloned().unwrap_or_else(|| "new_slot".to_owned())),
                (Some("add_mesh"), model) => Structural::AddMesh(model.cloned().unwrap_or_default()),
                (Some("add_reference"), file) => Structural::AddReference(file.cloned().unwrap_or_default()),
                (Some("add_tint"), _) => Structural::AddTint,
                (Some("remove"), _) => Structural::Remove,
                (Some("duplicate"), _) => Structural::Duplicate,
                (Some("up"), _) => Structural::MoveUp,
                (Some("down"), _) => Structural::MoveDown,
                _ => return fail("unknown edit"),
            };
            with_unit(e, |t, e| {
                let Some(path) = t.selected.clone() else { return fail("nothing selected") };
                let Some(elem) = document(e).and_then(|d| d.tree().node(&path).map(|n| n.element())) else { return fail("no such node") };
                t.apply(e, vec![Action::Edit(op, elem)]);
                ok(format!("selected {:?}", t.selected))
            })
        },
    );

    c.add(
        "unit_reorder",
        "unit_reorder <path> <place>",
        "a node to another place among its siblings (what a drag in Structure does)",
        |e, a| {
            let (Some(from), Some(to)) = (a.first().and_then(|p| parse_path(p)), a.get(1).and_then(|t| t.parse().ok())) else {
                return fail("usage: unit_reorder <path> <place>");
            };
            with_unit(e, |t, e| match t.reorder(e, &from, to) {
                Ok(()) => ok(format!("selected {:?}", t.selected)),
                Err(error) => fail(error),
            })
        },
    );

    c.add("unit_play", "unit_play <slot|-> [time]", "the animation slot looped (- for the default), held at a time if given", |e, a| {
        let slot = a.first().filter(|s| *s != "-").cloned().unwrap_or_default();
        let time = a.get(1).and_then(|t| t.parse::<f32>().ok());
        with_unit(e, |t, _| {
            t.slot = slot;
            if let Some(time) = time {
                t.time = time;
                t.playing = false;
            }

            ok("")
        })
    });

    c.add(
        "unit_click_part",
        "unit_click_part <path>",
        "click with the scripted mouse where the first soldier shows a node's parts (the middle of their triangles that face the camera)",
        |e, a| {
            let Some(path) = a.first().and_then(|p| parse_path(p)) else { return fail("usage: unit_click_part <path>") };
            let at = e.with_tool::<UnitTool, _>(|t, e| {
                let posed = std::mem::take(&mut t.posed);
                let mut sum = (Float3::new(0.0, 0.0, 0.0), 0.0f32);
                for p in posed.iter().filter(|p| p.soldier == 0) {
                    for (k, part) in p.instance.parts.iter().enumerate() {
                        if t.node_of(p, k).as_ref() == Some(&path)
                            && let Some(g) = t.geometry_of(e, &part.model)
                        {
                            for tri in g.triangles(part, &p.world, &p.instance.palette, &p.instance.bones) {
                                sum = (sum.0 + (tri[0] + tri[1] + tri[2]) * (1.0 / 3.0), sum.1 + 1.0);
                            }
                        }
                    }
                }

                t.posed = posed;
                (sum.1 > 0.0).then(|| sum.0 * (1.0 / sum.1))
            });
            let Some(Some(p)) = at else { return fail("the first soldier shows no part of that node") };
            let Some((x, y)) = e.viewport.project(p) else { return fail("not in view") };
            let r = CommandRegistry::run_args(e, "mouse", &["click".to_owned(), format!("{x:.1}"), format!("{y:.1}")]);
            if r.ok { CommandResult { message: format!("clicked {x:.0} {y:.0}"), ..r } } else { r }
        },
    );

    c.add("unit_grid", "unit_grid <files|units> <on|off>", "the Unit files or Units panel as pictures or a list", |e, a| {
        let on = a.get(1).is_some_and(|v| v == "on");
        let which = a.first().cloned().unwrap_or_default();
        with_unit(e, |t, _| match which.as_str() {
            "files" => {
                t.pics.files_grid = on;
                ok("")
            }

            "units" => {
                t.pics.units_grid = on;
                ok("")
            }

            _ => fail("files or units"),
        })
    });

    c.add(
        "unit_takes",
        "unit_takes <slot path>",
        "what a slot's pickers list: its models' and definitions' folders, its decals'",
        |e, a| {
            let Some(path) = a.first().and_then(|p| parse_path(p)) else { return fail("usage: unit_takes <slot path>") };
            with_unit(e, |t, e| {
                let Some(kinds) = t.kinds.clone() else { return CommandResult { repeat: true, ..ok("") } };
                let Some(tree) = document(e).map(|d| d.tree()) else { return fail("no definition open") };
                let Some(Node::Slot(slot)) = tree.node(&path) else { return fail("not a slot") };
                ok(format!("{}: parts {:?}; decals {:?}", slot.name, kinds.folders(slot, Field::Part), kinds.folders(slot, Field::Decal)))
            })
        },
    );

    c.add("unit_ships", "unit_ships <text>", "the unit_variants_ships rows and naval units whose names have a text", |e, a| {
        let text = a.first().cloned().unwrap_or_default().to_lowercase();
        with_unit(e, |t, e| {
            let Some(c) = t.catalog.clone() else { return CommandResult { repeat: true, ..ok("") } };
            let rows: Vec<String> = c
                .ships
                .iter()
                .filter(|s| s.variant.to_lowercase().contains(&text) || s.unit.to_lowercase().contains(&text))
                .map(|s| format!("{} {} -> {}", s.unit, s.faction, s.variant))
                .take(12)
                .collect();
            let navals: Vec<String> = c
                .naval_units
                .iter()
                .filter(|(k, n)| k.to_lowercase().contains(&text) || n.naval_unit.to_lowercase().contains(&text))
                .map(|(k, n)| format!("{k} (naval_unit {})", n.naval_unit))
                .take(12)
                .collect();
            let state = t.ship_state(e);
            ok(format!("ships: {rows:?}; naval units: {navals:?}; ship unit {:?}; {state}", t.ship.unit))
        })
    });

    c.add("unit_isolate", "unit_isolate <0|1>", "only the selected node's parts", |e, a| {
        let on = a.first().is_some_and(|v| v == "1");
        with_unit(e, |t, _| {
            t.isolate = on;
            ok("")
        })
    });

    c.add("unit_ready", "unit_ready", "test: waits until the soldiers are loaded and wear the document as it is", |e, _| {
        with_unit(e, |t, e| {
            let version = document(e).map(|d| d.version());
            if t.loading.is_none()
                && t.catalog_rx.is_none()
                && t.wearer_rx.is_none()
                && !t.pics_busy()
                && !t.models.is_empty()
                && t.worn == version
                && t.models_for == Some(t.wanted())
            {
                ok("")
            } else if t.catalog_failed || t.failed.contains(&t.wanted()) {
                fail(format!("did not load: {}", t.status))
            } else {
                CommandResult { repeat: true, ..ok("") }
            }
        })
    });

    c.add(
        "unit_db",
        "unit_db <variant NAME | height SCALE VARIATION | colours RRGGBB RRGGBB RRGGBB | colours off | revert>",
        "change the previewed unit's DB rows for its faction (saved into the project at once)",
        |e, a| {
            let edit = match a.iter().map(String::as_str).collect::<Vec<_>>().as_slice() {
                ["variant", v] => LooksEdit::Set(vec![("variant", (*v).to_owned())]),
                ["height", s, v] => LooksEdit::Set(vec![("height_scale", (*s).to_owned()), ("height_variation", (*v).to_owned())]),
                ["colours", "off"] => LooksEdit::Colours(None),
                ["colours", p, q, r] => LooksEdit::Colours(Some([hex_to_rgb(p), hex_to_rgb(q), hex_to_rgb(r)])),
                ["revert"] => LooksEdit::Revert,
                _ => return fail("usage: unit_db <variant NAME | height SCALE VARIATION | colours HEX HEX HEX | colours off | revert>"),
            };
            with_unit(e, |t, e| match t.edit_looks(e, vec![edit]) {
                Ok(()) => ok(""),
                Err(error) => fail(error),
            })
        },
    );

    c.add("unit_export", "unit_export <pack>", "File > Export > To mod pack, into this pack", |e, a| {
        let Some(path) = a.first() else { return fail("usage: unit_export <pack>") };
        match export_pack(e, std::path::Path::new(path)) {
            Ok(n) => ok(format!("{n} files")),
            Err(error) => fail(error),
        }
    });

    c.add("unit_summary", "unit_summary", "what the preview shows: unit, soldiers, parts, problems", |e, _| {
        with_unit(e, |t, _| {
            let parts: Vec<String> = t
                .models
                .iter()
                .map(|m| {
                    m.man
                        .parts
                        .parts
                        .iter()
                        .map(|p| {
                            let at = p.node.as_ref().map(|n| n.iter().map(ToString::to_string).collect::<Vec<_>>().join("/"));
                            format!("{}@{}", file_name(&p.model), at.unwrap_or_default())
                        })
                        .collect::<Vec<_>>()
                        .join(",")
                })
                .collect();
            let problems: Vec<String> = t.problems.iter().map(|p| format!("{:?} {}", p.path, p.text)).collect();
            let colours = t.models.first().map(|m| m.faction_colours[..3].iter().map(|c| format!("{:.3},{:.3},{:.3}", c[0], c[1], c[2])).collect::<Vec<_>>().join(" "));
            let scale = t.models.first().map_or(0.0, |m| m.scale);
            let rows: Vec<String> = t
                .looks
                .as_ref()
                .map(|l| l.tables.iter().flat_map(|(n, tb)| tb.project_rows().into_iter().map(move |r| format!("{n}: {r:?}"))).collect())
                .unwrap_or_default();
            let entry = t.catalog.as_ref().and_then(|c| c.entry(&t.unit, &t.faction)).map(|u| format!("{u:?}"));
            ok(format!(
                "unit {} faction {} entry {entry:?} project rows {rows:?} seed {} soldiers {} scale {scale:.3} colours {colours:?} worn '{}' selected {:?} pins {} parts [{}] missing {:?} problems [{}] status '{}'",
                t.unit,
                t.faction,
                t.seed,
                t.models.len(),
                t.worn_how,
                t.selected,
                t.pins.slots.len(),
                parts.join(" | "),
                t.missing,
                problems.join("; "),
                t.status
            ))
        })
    });
}
