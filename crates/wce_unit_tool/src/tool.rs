//! The Unit tool: a variant mesh definition (VMD) open as the document, worn by a unit of the DB standing on a plain ground, looping one of
//! its animations. What the unit is ("Preview as") gives the skeleton, the animations, the mount, the scale and the colours; the edited file
//! takes the place of the unit's own (or its mount's, when it is the mount's). The soldiers are picked by a seed as the game picks them (the
//! editor's generator, not the game's), and pins force what a slot shows, so the node being edited is always in view.
//!
//! The scene is shared by every tool: while this one is active, the other tools' layers are hidden, and shown again when another tool is.

use std::any::Any;
use std::collections::{BTreeMap, HashSet};
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::mpsc::{Receiver, TryRecvError, channel};

use rpfm_lib::schema::Schema;
use serde::{Deserialize, Serialize};
use wce_editor::editor::Editor;
use wce_editor::gui;
use wce_editor::pickers::search_combo;
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
use wce_render::unit_animation::UnitModel;
use wce_render::unit_assembly::{Pins, SlotPin, assemble_with, attachment_points, decals};
use wce_render::unit_catalog::{UnitCatalog, variant_path};
use wce_scene::environment::{EnvironmentParams, read_environment};
use wce_scene::render_scene::{DocumentId, EntityId, LayerId, Payload, SourceRef};
use wce_vfs::normalize;

use crate::document::{Source, Structural, VmdDocument};
use crate::problems::{Problem, WEAPON_POINTS, check};

/// The Unit tool's id: the owner of its documents, its settings' table.
pub const UNIT_TOOL_ID: &str = "unit";

/// Where the definitions are.
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

/// What the panels asked for this frame, done after them.
enum Action {
    Select(Option<NodePath>),
    Edit(Structural, usize),
    Open(String),
    OpenUnit(String, String),
}

pub struct UnitTool {
    settings: UnitSettings,
    catalog: Option<Arc<UnitCatalog>>,
    catalog_rx: Option<Receiver<Result<UnitCatalog, String>>>,
    catalog_failed: bool,
    status: String,

    /// The document shown, and the version of it the soldiers wear.
    doc: DocumentId,
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

    entities: Vec<EntityId>,
    layer: Option<(DocumentId, LayerId)>,

    /// Other tools' layers this tool hid, and its own layers while another tool is active.
    hidden: Vec<LayerId>,
    own_hidden: Vec<LayerId>,

    environment: Option<(String, EnvironmentParams)>,
    listings: Option<Listings>,
    problems: Vec<Problem>,
    problems_for: Option<(DocumentId, u64)>,

    /// The camera was aimed at this document.
    aimed: DocumentId,

    /// A unit asked for with its file (Units panel), applied when that file is open.
    pending_preview: Option<(String, String, String)>,

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
            catalog_failed: false,
            status: String::new(),
            doc: 0,
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
            entities: vec![],
            layer: None,
            hidden: vec![],
            own_hidden: vec![],
            environment: None,
            listings: None,
            problems: vec![],
            problems_for: None,
            aimed: 0,
            pending_preview: None,
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
                .and_then(|schema| UnitCatalog::load(&vfs, &schema));
            let _ = tx.send(result);
        });
    }

    fn poll(&mut self) {
        if let Some(rx) = &self.catalog_rx {
            match rx.try_recv() {
                Ok(Ok(catalog)) => {
                    self.catalog = Some(Arc::new(catalog));
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
                definitions: owned(vfs.list(DEFINITIONS, ".variantmeshdefinition")),
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

    /// A document opened: its unit (remembered, else one that wears it, else the game's default) and the camera on it.
    fn document_opened(&mut self, e: &mut Editor, id: DocumentId) {
        self.doc = id;
        self.selected = None;
        self.pins = Pins::default();
        self.parts_key = None;
        self.problems_for = None;
        let Some(path) = e.docs.find_as::<VmdDocument>(id).map(|d| d.source().vfs_path.clone()) else { return };
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

    /// The first unit that wears the file (or rides it), else the game's default.
    fn preview_default(&mut self, e: &Editor, path: &str) {
        let (unit, faction) = default_unit(e.game().key);
        (self.unit, self.faction) = (unit.to_owned(), faction.to_owned());
        let Some(c) = &self.catalog else { return };
        if let Some(entry) = c.entries.iter().find(|u| variant_path(&u.variant) == path && c.land_units.contains_key(&u.unit)) {
            (self.unit, self.faction) = (entry.unit.clone(), entry.faction.clone());
            return;
        }

        let mounts: Vec<&String> = c.mounts.iter().filter(|(_, m)| variant_path(&m.variant) == path).map(|(k, _)| k).collect();
        if let Some((unit, _)) = c.land_units.iter().find(|(_, l)| mounts.contains(&&l.mount))
            && let Some(entry) = c.entries.iter().find(|u| u.unit == *unit)
        {
            (self.unit, self.faction) = (entry.unit.clone(), entry.faction.clone());
        }
    }

    fn set_preview(&mut self, e: &mut Editor, path: &str, unit: String, faction: String) {
        self.unit.clone_from(&unit);
        self.faction.clone_from(&faction);
        self.slot.clear();
        let path = path.to_owned();
        e.update_tool_settings::<UnitSettings>(UNIT_TOOL_ID, |s| {
            s.preview.insert(path, (unit, faction));
        });
        self.settings = e.tool_settings(UNIT_TOOL_ID);
    }

    /// Whether the file is the unit's mount's rather than its own.
    fn is_mount_file(&self, path: &str) -> bool {
        let Some(c) = &self.catalog else { return false };
        let Some(land) = c.land_units.get(&self.unit) else { return false };
        c.mounts.get(&land.mount).is_some_and(|m| variant_path(&m.variant) == path)
            && c.entry(&self.unit, &self.faction).is_none_or(|u| variant_path(&u.variant) != path)
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

    /// The soldiers wear the document as it is now.
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

        self.parts_key = Some(key);
        self.worn = Some(version);
        let read = |p: &str| vfs.read(p).ok();
        let mount = self.is_mount_file(&path);
        self.missing.clear();
        for (k, model) in self.models.iter_mut().enumerate() {
            let mut parts = assemble_with(&read, &tree, self.seed.wrapping_add(k as u64), &pins);
            if self.isolate
                && let Some(sel) = &self.selected
            {
                parts.parts.retain(|p| p.node.starts_with(sel));
            }

            self.missing.extend(parts.missing.iter().cloned());
            match (&mut model.mount, mount) {
                (Some(m), true) => m.parts = parts,
                _ => model.man.parts = parts,
            }
        }

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

                            // Pins name places, and the places moved.
                            if matches!(op, Structural::Remove | Structural::MoveUp | Structural::MoveDown) {
                                self.pins = Pins::default();
                            }
                        }

                        Err(error) => e.status(&error, true),
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

    // ---- panels

    fn draw_files(&mut self, e: &mut Editor) -> Vec<Action> {
        let mut actions = vec![];
        if !gui::begin("Unit files", None, 0) {
            gui::end();
            return actions;
        }

        gui::set_next_item_width(-1.0);
        gui::input_text("##vmd_filter", Some("filter the definitions"), &mut self.file_filter, 128, 0);
        let words: Vec<String> = self.file_filter.split_whitespace().map(str::to_lowercase).collect();
        let open = e.docs.of_tool(UNIT_TOOL_ID).and_then(|id| e.docs.find_as::<VmdDocument>(id)).map(|d| d.source().vfs_path.clone());
        let shown: Vec<String> = self.listings(e).definitions.iter().filter(|p| wce_editor::pickers::matches(p, &words)).cloned().collect();
        gui::text_colored(theme::dim(), &format!("{} definitions", shown.len()));
        if gui::begin_child("##vmd_list", gui::v2(0.0, 0.0), 0, 0) {
            gui::clipped(shown.len(), |k| {
                let p = &shown[k];
                let label = p.strip_prefix(DEFINITIONS).unwrap_or(p);
                let ours = e.loose_copy(p).is_some();
                let text = if ours { format!("{label}  (project)") } else { label.to_owned() };
                if gui::selectable(&text, open.as_deref() == Some(p.as_str()), 0) {
                    actions.push(Action::Open(p.clone()));
                }
            });
        }

        gui::end_child();
        gui::end();
        actions
    }

    fn draw_units(&mut self) -> Vec<Action> {
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

        gui::set_next_item_width(-1.0);
        gui::input_text("##unit_filter", Some("filter: unit, faction or definition"), &mut self.unit_filter, 128, 0);
        let words: Vec<String> = self.unit_filter.split_whitespace().map(str::to_lowercase).collect();
        let rows: Vec<&wce_render::unit_catalog::UnitEntry> =
            c.entries.iter().filter(|u| wce_editor::pickers::matches(&format!("{} {} {}", u.unit, u.faction, u.variant), &words)).collect();
        gui::text_colored(
            theme::dim(),
            &format!("{} unit_variants rows; a click opens the row's definition, worn by its unit", rows.len()),
        );
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

    fn draw_structure(&mut self, e: &mut Editor) -> Vec<Action> {
        let mut actions = vec![];
        if !gui::begin("Structure", None, 0) {
            gui::end();
            return actions;
        }

        let Some(d) = document(e) else {
            gui::text_wrapped("Open a variant mesh definition: from Unit files, from Units (a unit's own), or File > Open.");
            gui::end();
            return actions;
        };

        let tree = d.tree();
        let source = d.source().clone();
        let repaired = d.repaired();
        gui::text(&d.title());
        gui::same_line();
        gui::text_colored(theme::dim(), &format!("({})", source.label));
        if repaired {
            gui::text_colored(
                theme::warning(),
                "This file isn't valid XML: it is shown as the game most likely reads it, and Save writes that repaired text.",
            );
        }

        // The selected node's buttons.
        let sel = self.selected.clone().filter(|p| tree.node(p).is_some());
        let node = sel.as_ref().and_then(|p| tree.node(p));
        let elem = node.map(|n| n.element());
        let mut first = true;
        let mut add = |label: &str, enabled: bool, tip: &str| {
            if !first {
                gui::same_line_or_wrap(gui::button_width(label));
            }

            first = false;
            gui::begin_disabled(!enabled);
            let clicked = gui::small_button(label);
            gui::end_disabled();
            if gui::is_item_hovered_allow_disabled() {
                gui::set_tooltip(tip);
            }

            clicked
        };
        let is_mesh = matches!(node, Some(Node::Mesh(_))) || sel.is_none();
        let target = if sel.is_none() { Some(tree.root.element) } else { elem };
        if add("+ Slot", is_mesh, "A slot in the selected mesh (or the file's root): a place for one part, among its alternatives.")
            && let Some(t) = target
        {
            actions.push(Action::Edit(Structural::AddSlot("new_slot".to_owned()), t));
        }

        let is_slot = matches!(node, Some(Node::Slot(_)));
        if add("+ Mesh", is_slot, "An alternative of the selected slot: a model with its decals, masks and tints.")
            && let Some(t) = elem
        {
            actions.push(Action::Edit(Structural::AddMesh(String::new()), t));
        }

        if add("+ Reference", is_slot, "An alternative of the selected slot that is another definition file.")
            && let Some(t) = elem
        {
            actions.push(Action::Edit(Structural::AddReference(String::new()), t));
        }

        let any = sel.as_ref().is_some_and(|p| !p.is_empty());
        if add("Copy", any, "A copy of the selected node right after it.")
            && let Some(t) = elem
        {
            actions.push(Action::Edit(Structural::Duplicate, t));
        }

        if add("Up", any, "Before the node above it.")
            && let Some(t) = elem
        {
            actions.push(Action::Edit(Structural::MoveUp, t));
        }

        if add("Down", any, "After the node below it.")
            && let Some(t) = elem
        {
            actions.push(Action::Edit(Structural::MoveDown, t));
        }

        if add("Delete", any, "The selected node and everything in it.")
            && let Some(t) = elem
        {
            actions.push(Action::Edit(Structural::Remove, t));
        }

        gui::separator();
        if gui::begin_child("##vmd_tree", gui::v2(0.0, 0.0), 0, 0) {
            let root_flags = gui::TREE_DEFAULT_OPEN
                | gui::TREE_OPEN_ON_ARROW
                | gui::TREE_SPAN_AVAIL_WIDTH
                | if sel.as_ref().is_some_and(Vec::is_empty) { gui::TREE_SELECTED } else { 0 };
            let open = gui::tree_node_ex_id("root", root_flags, &mesh_label(&tree.root, true));
            if gui::is_item_clicked() && !gui::is_item_toggled_open() {
                actions.push(Action::Select(Some(vec![])));
            }

            if open {
                draw_mesh_children(&tree.root, &mut vec![], sel.as_ref(), &self.pins, &mut actions);
                gui::tree_pop();
            }
        }

        gui::end_child();
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

        gui::separator_text("Light");
        let weathers = self.listings(e).weathers.clone();
        let current =
            if self.settings.weather.is_empty() { e.game().default_environment.to_owned() } else { self.settings.weather.clone() };
        if let Some(w) = self.combo("Weather", "The weather file lighting the stage.", &current, &weathers) {
            e.update_tool_settings::<UnitSettings>(UNIT_TOOL_ID, |s| s.weather.clone_from(&w));
            self.settings.weather = w;
        }

        gui::separator_text("State");
        if let Some(m) = self.models.first() {
            gui::text_colored(theme::dim(), &format!("skeleton {}, scale {:.3}", m.man.rig.skeleton.skeleton, m.scale));
            if let Some(mount) = &m.mount {
                gui::text_colored(theme::dim(), &format!("mount on {}", mount.rig.skeleton.skeleton));
            }

            if self.is_mount_file(&path) {
                gui::text_colored(theme::dim(), "This file is the unit's mount's: the mount wears it.");
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

    fn path_field(&mut self, d: &mut VmdDocument, elem: usize, name: &str, tip: &str, items: &[String], strip: &str) {
        let value = d.xml().get(elem, name).unwrap_or_default();
        let mut text = value.clone();
        gui::set_next_item_width(-60.0);
        if gui::input_text(&format!("{name}##text{elem}"), None, &mut text, 512, 0) {
            d.set_value(elem, name, &text, gesture(elem, name));
        }

        if gui::is_item_deactivated_after_edit() {
            d.break_merge();
        }

        if gui::is_item_hovered() {
            gui::set_tooltip(tip);
        }

        gui::same_line();
        gui::set_next_item_width(-1.0);
        let filter = self.combo_filters.entry(format!("{name}#pick")).or_default();
        if let Some(p) = search_combo(&format!("##pick{name}{elem}"), "Pick a file of the packs.", filter, &value, items) {
            d.set_value(elem, name, p.trim_end_matches(strip), 0);
        }
    }

    fn inspect_mesh(&mut self, e: &mut Editor, path: &NodePath) {
        let listings = {
            let l = self.listings(e);
            (l.models.clone(), l.textures.clone())
        };
        let vfs = e.vfs().cloned();
        let Some(d) = document(e) else { return };
        let tree = d.tree();
        let Some(Node::Mesh(mesh)) = tree.node(path) else { return };
        let elem = mesh.element;
        gui::separator_text(if path.is_empty() { "The file's root" } else { "Mesh" });
        self.path_field(d, elem, "model", "The part's model (RMV2).", &listings.0, "");
        self.path_field(
            d,
            elem,
            "decal_diffuse",
            "A decal's colours: a texture, or a .texture_array of them (one picked per soldier).",
            &listings.1,
            "",
        );
        self.path_field(d, elem, "decal_normal", "The decal's normals: a texture, or a .texture_array.", &listings.1, "");
        self.path_field(d, elem, "imposter_model", "The model drawn far away.", &listings.0, "");

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
            gui::set_next_item_width(-1.0);
            if gui::combo(&format!("##{name}"), &mut current, &labels) {
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
            gui::set_next_item_width(120.0);
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

        // The decals of a texture array, to show one.
        if let (Some(vfs), Some(name)) = (vfs, mesh.decal_diffuse.as_deref().filter(|n| !n.trim().is_empty())) {
            let read = |p: &str| vfs.read(p).ok();
            let all = decals(&read, name);
            if all.len() > 1 {
                gui::separator_text("Decal shown");
                let mut current = self.pins.decals.get(path).map_or(0, |k| *k as i32 + 1);
                let mut labels = vec!["the seed's".to_owned()];
                labels.extend(all.iter().map(|t| file_name(t).to_owned()));
                let labels: Vec<&str> = labels.iter().map(String::as_str).collect();
                gui::set_next_item_width(-1.0);
                if gui::combo("##decal_shown", &mut current, &labels) {
                    if current == 0 {
                        self.pins.decals.remove(path);
                    } else {
                        self.pins.decals.insert(path.clone(), current as usize - 1);
                    }
                }
            }
        }

        if !mesh.meta_data.is_empty() {
            gui::separator_text("Meta data");
            for m in &mesh.meta_data {
                gui::text(m);
            }
        }

        other_attributes(d, elem, &["model", "decal_diffuse", "decal_normal", "imposter_model", "mask0", "mask1", "mask2"]);
    }

    fn inspect_slot(&mut self, e: &mut Editor, path: &NodePath) {
        let vfs = e.vfs().cloned();
        let Some(d) = document(e) else { return };
        let tree = d.tree();
        let Some(Node::Slot(slot)) = tree.node(path) else { return };
        let elem = slot.element;
        gui::separator_text("Slot");
        let mut name = slot.name.clone();
        if gui::input_text(&format!("name##{elem}"), None, &mut name, 128, 0) {
            d.set_value(elem, "name", &name, gesture(elem, "name"));
        }

        if gui::is_item_deactivated_after_edit() {
            d.break_merge();
        }

        // The points: the weapon bones, and the parent model's own.
        let parent_model = parent_model(&tree, path);
        let mut points: Vec<String> = WEAPON_POINTS.iter().map(|p| (*p).to_owned()).collect();
        if let (Some(vfs), Some(model)) = (&vfs, &parent_model) {
            let read = |p: &str| vfs.read(p).ok();
            points.extend(attachment_points(&read, model));
        }

        let mut point = slot.attach_point.clone().unwrap_or_default();
        gui::set_next_item_width(-60.0);
        if gui::input_text(&format!("attach_point##{elem}"), None, &mut point, 128, 0) {
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
        gui::set_next_item_width(-1.0);
        let filter = self.combo_filters.entry("attach_point#pick".to_owned()).or_default();
        if let Some(p) = search_combo(&format!("##points{elem}"), "", filter, &point, &points) {
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

        gui::separator_text("Shown in the preview");
        let labels: Vec<String> = ["the seed's".to_owned(), "nothing".to_owned()]
            .into_iter()
            .chain(slot.alternatives.iter().enumerate().map(|(k, a)| format!("{}: {}", k + 1, alternative_label(a))))
            .collect();
        let labels: Vec<&str> = labels.iter().map(String::as_str).collect();
        let mut current = match self.pins.slots.get(path) {
            None => 0,
            Some(SlotPin::Off) => 1,
            Some(SlotPin::Alternative(k)) => *k as i32 + 2,
        };
        gui::set_next_item_width(-1.0);
        if gui::combo("##slot_shown", &mut current, &labels) {
            match current {
                0 => {
                    self.pins.slots.remove(path);
                }

                1 => {
                    self.pins.slots.insert(path.clone(), SlotPin::Off);
                }

                k => {
                    self.pins.slots.insert(path.clone(), SlotPin::Alternative(k as usize - 2));
                }
            }
        }

        other_attributes(d, elem, &["name", "attach_point", "probability"]);
    }

    fn inspect_reference(&mut self, e: &mut Editor, path: &NodePath) -> Option<String> {
        let definitions = self.listings(e).definitions.clone();
        let d = document(e)?;
        let tree = d.tree();
        let Some(Node::Reference { element, definition }) = tree.node(path) else { return None };
        gui::separator_text("Reference");
        gui::text_wrapped("Another definition file, used whole as this alternative.");
        self.path_field(d, element, "definition", "The definition file.", &definitions, "");
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

fn draw_mesh_children(mesh: &VariantMesh, path: &mut NodePath, sel: Option<&NodePath>, pins: &Pins, actions: &mut Vec<Action>) {
    for (i, slot) in mesh.slots.iter().enumerate() {
        path.push(i);
        let mut label = if slot.name.is_empty() { "(slot)".to_owned() } else { slot.name.clone() };
        if let Some(a) = &slot.attach_point {
            label.push_str(&format!("  @{a}"));
        }

        if let Some(p) = slot.probability {
            label.push_str(&format!("  {:.0}%", p * 100.0));
        }

        match pins.slots.get(path) {
            Some(SlotPin::Off) => label.push_str("  [pinned: nothing]"),
            Some(SlotPin::Alternative(k)) => label.push_str(&format!("  [pinned: {}]", k + 1)),
            None => {}
        }

        let flags = gui::TREE_OPEN_ON_ARROW
            | gui::TREE_SPAN_AVAIL_WIDTH
            | gui::TREE_DEFAULT_OPEN
            | if slot.alternatives.is_empty() { gui::TREE_LEAF } else { 0 }
            | if sel == Some(path) { gui::TREE_SELECTED } else { 0 };
        let id = path.iter().map(ToString::to_string).collect::<Vec<_>>().join("/");
        let open = gui::tree_node_ex_id(&id, flags, &label);
        if gui::is_item_clicked() && !gui::is_item_toggled_open() {
            actions.push(Action::Select(Some(path.clone())));
        }

        if open {
            for (j, alternative) in slot.alternatives.iter().enumerate() {
                path.push(j);
                let id = path.iter().map(ToString::to_string).collect::<Vec<_>>().join("/");
                let selected = if sel == Some(path) { gui::TREE_SELECTED } else { 0 };
                match alternative {
                    Alternative::Mesh(m) => {
                        let leaf = if m.slots.is_empty() { gui::TREE_LEAF } else { 0 };
                        let open = gui::tree_node_ex_id(
                            &id,
                            gui::TREE_OPEN_ON_ARROW | gui::TREE_SPAN_AVAIL_WIDTH | gui::TREE_DEFAULT_OPEN | leaf | selected,
                            &mesh_label(m, false),
                        );
                        if gui::is_item_clicked() && !gui::is_item_toggled_open() {
                            actions.push(Action::Select(Some(path.clone())));
                        }

                        if open {
                            draw_mesh_children(m, path, sel, pins, actions);
                            gui::tree_pop();
                        }
                    }

                    Alternative::Reference { definition, .. } => {
                        let open = gui::tree_node_ex_id(
                            &id,
                            gui::TREE_LEAF | gui::TREE_SPAN_AVAIL_WIDTH | selected,
                            &format!("-> {}", file_name(definition)),
                        );
                        if gui::is_item_clicked() {
                            actions.push(Action::Select(Some(path.clone())));
                        }

                        if gui::is_item_hovered() {
                            gui::set_tooltip(&format!("{definition}\nDouble-click to open it."));
                        }

                        if gui::is_item_hovered() && gui::is_mouse_double_clicked(0) {
                            actions.push(Action::Open(normalize(definition)));
                        }

                        if open {
                            gui::tree_pop();
                        }
                    }
                }

                path.pop();
            }

            gui::tree_pop();
        }

        path.pop();
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
        *self = Self::default();
    }

    fn update(&mut self, e: &mut Editor) {
        self.hide_others(e);
        self.load_catalog(e);
        self.poll();
        self.light(e);
        let Some(id) = e.docs.of_tool(UNIT_TOOL_ID).filter(|&id| e.docs.find_as::<VmdDocument>(id).is_some()) else {
            self.clear(e);
            self.doc = 0;
            return;
        };

        // The catalog arrived after the document: its unit now.
        if id != self.doc || (self.unit.is_empty() && self.catalog.is_some()) {
            self.document_opened(e, id);
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

        self.dress(e);
        self.pose(e);
    }

    fn draw_panels(&mut self, e: &mut Editor) {
        let mut actions = self.draw_structure(e);
        actions.extend(self.draw_files(e));
        actions.extend(self.draw_units());
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

    fn inspector_subject(&mut self, _: &Editor) -> Option<u64> {
        self.selected.as_ref().map(|p| hash_of(p) | 1)
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

    c.add("unit_summary", "unit_summary", "what the preview shows: unit, soldiers, parts, problems", |e, _| {
        with_unit(e, |t, _| {
            let parts: Vec<String> = t
                .models
                .iter()
                .map(|m| m.man.parts.parts.iter().map(|p| file_name(&p.model).to_owned()).collect::<Vec<_>>().join(","))
                .collect();
            let problems: Vec<String> = t.problems.iter().map(|p| format!("{:?} {}", p.path, p.text)).collect();
            ok(format!(
                "unit {} faction {} seed {} soldiers {} selected {:?} pins {} parts [{}] missing {:?} problems [{}] status '{}'",
                t.unit,
                t.faction,
                t.seed,
                t.models.len(),
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
