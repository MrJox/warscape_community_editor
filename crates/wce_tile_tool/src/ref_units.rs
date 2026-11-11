//! The references, to measure the scale against (docs/plans/units.md): `ref_human`, a soldier, and `ref_unit`, a whole unit of the size the game is set
//! to. They are tile elements like any other, placed from the Create panel, as many as wanted: selected, moved, turned, grouped, undone and saved in
//! tile.wce as the rest, and not exported (`project_files::to_ted`). What is the tool's: the game's soldiers standing on them, which only exist in the
//! scene. The tables and the models load on a thread of their own. A click finds them by a sphere per soldier (`Tile::set_ref_spots`), not by the
//! picture: the renderer's id pass doesn't draw skinned meshes.
//!
//! `ref_ship`: a naval unit's ship (`wce_render::ship_model`) afloat on the tile's water (`Tile::water_level`), its men aboard. The ship's parts are
//! rigid meshes in the scene, picked by the picture like any model.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::sync::Arc;
use std::sync::mpsc::{Receiver, TryRecvError, channel};

use rpfm_lib::schema::Schema;
use wce_editor::editor::Editor;
use wce_editor::gui;
use wce_foundation::math::{Float3, Float4x4, rotation_y, translation};
use wce_foundation::settings::rpfm_schema_path;
use wce_render::ship_model::ShipModel;
use wce_render::unit_animation::{UnitInstance, UnitModel};
use wce_render::unit_catalog::{UnitCatalog, formation, men_at_size};
use wce_scene::environment::read_game_graphics_values;
use wce_scene::render_scene::{DocumentId, EntityId, LayerId, MeshInstance, Payload, SourceRef};
use wce_tile_editor::tile_document::{REF_HUMAN, REF_SHIP, REF_UNIT, TileDocument, item_of};

use crate::tool::selected;

/// The different soldiers of a `ref_unit`: every man of it is one of these, in his pose. Loading and posing each of a unit's men costs far more than
/// a reference is worth.
const UNIT_VARIATIONS: u64 = 16;

/// Each game's default: the unit and faction of a soldier of ordinary size. A game without one takes the first unit of its tables.
fn default_unit(game: &str) -> (&'static str, &'static str) {
    match game {
        "attila" => ("att_rom_legio_comitatenses", "att_fact_western_roman_empire"),
        _ => ("", ""),
    }
}

/// Each game's default naval unit (a `main_units` key) and its faction. A game without one takes the first naval unit of its tables.
fn default_naval(game: &str) -> (&'static str, &'static str) {
    match game {
        "attila" => ("att_shp_rom_heavy_marines_lib_heavy", "att_fact_western_roman_empire"),
        _ => ("", ""),
    }
}

/// The three references.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Kind {
    Human,
    Unit,
    Ship,
}

impl Kind {
    fn of(tag: &str) -> Option<Self> {
        match tag {
            REF_HUMAN => Some(Self::Human),
            REF_UNIT => Some(Self::Unit),
            REF_SHIP => Some(Self::Ship),
            _ => None,
        }
    }
}

/// The ship a `ref_ship` needs: naval unit (`main_units`), faction, seed.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
struct ShipWanted {
    unit: String,
    faction: String,
    seed: u64,
}

/// The soldiers a reference needs: unit, faction, first seed, slot, how many different ones.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
struct Wanted {
    unit: String,
    faction: String,
    seed: u64,
    slot: String,
    variations: u64,
}

/// A reference, as the tile has it.
struct Reference {
    elem: usize,
    kind: Kind,

    /// Its soldiers (a ship's: the men aboard).
    wanted: Wanted,
    ship: Option<ShipWanted>,
    colours: Option<[[f32; 3]; 3]>,

    /// On the ground under each soldier, or at its own height (freeform: a wall's top).
    on_ground: bool,
}

/// The turn about y of a matrix: how far it takes +z, where a soldier faces. A reference stands upright whatever else its matrix holds.
fn yaw_of(m: &Float4x4) -> f32 {
    m.m[0][2].atan2(m.m[2][2])
}

/// A uniform scale.
fn scaling(k: f32) -> Float4x4 {
    let mut m = Float4x4::IDENTITY;
    for i in 0..3 {
        m.m[i][i] = k;
    }

    m
}

/// `colours`: nine numbers.
fn parse_colours(text: &str) -> Option<[[f32; 3]; 3]> {
    let v: Vec<f32> = text.split(',').filter_map(|n| n.trim().parse().ok()).collect();
    (v.len() == 9).then(|| [[v[0], v[1], v[2]], [v[3], v[4], v[5]], [v[6], v[7], v[8]]])
}

fn format_colours(c: &[[f32; 3]; 3]) -> String {
    c.iter().flatten().map(ToString::to_string).collect::<Vec<_>>().join(",")
}

/// The game's `gfx_unit_size`, as shown.
fn size_name(size: u32) -> &'static str {
    ["small", "medium", "large", "ultra"][size.min(3) as usize]
}

type Loaded = (Wanted, Result<Vec<UnitModel>, String>);
type ShipLoaded = (ShipWanted, Result<ShipModel, String>);

/// How long a ship's water level is trusted: the tile's rivers and lakes may be edited under it.
const WATER_SECONDS: f64 = 2.0;

#[derive(Default)]
pub struct RefUnits {
    catalog: Option<Arc<UnitCatalog>>,
    catalog_rx: Option<Receiver<Result<UnitCatalog, String>>>,

    /// The tables didn't read, not to try again until the game changes.
    catalog_failed: bool,
    models: HashMap<Wanted, Arc<Vec<UnitModel>>>,
    loading: Option<Receiver<Loaded>>,

    /// What didn't load, not to try again.
    failed: HashSet<Wanted>,
    entities: Vec<EntityId>,
    layer: Option<(DocumentId, LayerId)>,

    /// `gfx_unit_size` (`None`: not read yet), and when it was: the player may change it with the editor open.
    size: Option<u32>,
    size_read: f64,

    /// What the last frame's update took.
    update_ms: f32,
    status: String,
    unit_filter: String,
    faction_filter: String,
    slot_filter: String,

    ships: HashMap<ShipWanted, Arc<ShipModel>>,
    ship_loading: Option<Receiver<ShipLoaded>>,
    ship_failed: HashSet<ShipWanted>,

    /// The ships' parts in the scene, and what each shows: model, element, world.
    ship_entities: Vec<EntityId>,
    ship_shown: Vec<(String, usize, Float4x4)>,

    /// Per ship element, the water under it: where it was asked, when, the level.
    water: HashMap<usize, ([u32; 2], f64, Option<f32>)>,
}

impl RefUnits {
    /// What is loaded and drawn, for the scripts.
    pub fn summary(&self) -> String {
        let units = self.models.keys().filter(|w| w.variations > 1).filter_map(|w| Some((w, self.catalog.as_ref()?.land_units.get(&w.unit)?))).map(|(w, l)| {
            format!("{}: {} men, {} ranks, spacing {:?}", w.unit, men_at_size(l, self.size.unwrap_or(3)), l.rank_depth, l.spacing)
        }).collect::<Vec<_>>().join("; ");
        let ships = self.ships.values().map(|s| format!("{}: {} ({} parts{})", s.unit, s.variant, s.parts.len(), if s.missing.is_empty() { String::new() } else { format!(", missing {:?}", s.missing) })).collect::<Vec<_>>().join("; ");
        let mut water: Vec<String> = self.water.iter().map(|(elem, (_, _, level))| format!("#{elem} {level:?}")).collect();
        water.sort_unstable();
        let ships = format!("{ships}; water under them: {}", water.join(", "));
        format!(
            "tables {}, {} loaded, loading {}, {} failed, {} entities, unit size {:?}, update {:.2} ms [{units}]; ships {} loaded, loading {}, {} failed, {} entities [{ships}]: {}",
            self.catalog.is_some(), self.models.len(), self.loading.is_some(), self.failed.len(), self.entities.len(), self.size, self.update_ms,
            self.ships.len(), self.ship_loading.is_some(), self.ship_failed.len(), self.ship_entities.len(), self.status
        )
    }

    /// The game changed: its tables and models go.
    pub fn game_changed(&mut self, e: &mut Editor) {
        self.catalog = None;
        self.catalog_rx = None;
        self.catalog_failed = false;
        self.models.clear();
        self.loading = None;
        self.failed.clear();
        self.size = None;
        self.ships.clear();
        self.ship_loading = None;
        self.ship_failed.clear();
        self.water.clear();
        self.clear(e);
    }

    fn clear(&mut self, e: &mut Editor) {
        // Not one by one: each would go over the scene, and a removal of that kind has the renderer build all its draws again.
        e.scene.remove_ids(&self.entities);
        self.entities.clear();
        self.clear_ships(e);
    }

    fn clear_ships(&mut self, e: &mut Editor) {
        e.scene.remove_ids(&self.ship_entities);
        self.ship_entities.clear();
        self.ship_shown.clear();
    }

    fn load_catalog(&mut self, e: &Editor) {
        if self.catalog.is_some() || self.catalog_rx.is_some() || self.catalog_failed {
            return;
        }

        let Some(vfs) = e.vfs().cloned() else {
            "the game's packs are not mounted".clone_into(&mut self.status);
            return;
        };

        let (tx, rx) = channel();
        self.catalog_rx = Some(rx);
        "reading the unit tables...".clone_into(&mut self.status);
        std::thread::spawn(move || {
            let result = rpfm_schema_path(vfs.profile()).ok_or_else(|| "no rpfm schema of the game (Help > Update Manager downloads it)".to_owned())
                .and_then(|path| Schema::load(&path, None).map_err(|e| e.to_string()))
                .and_then(|schema| UnitCatalog::load(&vfs, &schema));
            let _ = tx.send(result);
        });
    }

    /// What finished loading.
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
                    self.models.insert(wanted, Arc::new(models));
                    self.loading = None;
                }

                Ok((wanted, Err(error))) => {
                    self.status = error;
                    self.failed.insert(wanted);
                    self.loading = None;
                }

                Err(TryRecvError::Disconnected) => self.loading = None,
                Err(TryRecvError::Empty) => {}
            }
        }

        if let Some(rx) = &self.ship_loading {
            match rx.try_recv() {
                Ok((wanted, Ok(ship))) => {
                    self.status.clear();
                    self.ships.insert(wanted, Arc::new(ship));
                    self.ship_loading = None;
                }

                Ok((wanted, Err(error))) => {
                    self.status = error;
                    self.ship_failed.insert(wanted);
                    self.ship_loading = None;
                }

                Err(TryRecvError::Disconnected) => self.ship_loading = None,
                Err(TryRecvError::Empty) => {}
            }
        }
    }

    fn load_ship(&mut self, e: &Editor, wanted: ShipWanted) {
        let (Some(vfs), Some(catalog)) = (e.vfs().cloned(), self.catalog.clone()) else { return };
        let (tx, rx) = channel();
        self.ship_loading = Some(rx);
        format!("loading the ship of {}...", wanted.unit).clone_into(&mut self.status);
        std::thread::spawn(move || {
            let read = |p: &str| vfs.read(p).ok();
            let ship = ShipModel::load(&read, &catalog, &wanted.unit, &wanted.faction, wanted.seed);
            let _ = tx.send((wanted, ship));
        });
    }

    fn load_models(&mut self, e: &Editor, wanted: Wanted) {
        let (Some(vfs), Some(catalog)) = (e.vfs().cloned(), self.catalog.clone()) else { return };
        let (tx, rx) = channel();
        self.loading = Some(rx);
        format!("loading {}...", wanted.unit).clone_into(&mut self.status);
        std::thread::spawn(move || {
            let read = |p: &str| vfs.read(p).ok();
            let models = (0..wanted.variations).map(|k| {
                let mut model = UnitModel::load(&read, &catalog, &wanted.unit, &wanted.faction, wanted.seed.wrapping_add(k))?;
                if !wanted.slot.is_empty() {
                    model.play(&read, &wanted.slot);
                }

                Ok(model)
            }).collect();
            let _ = tx.send((wanted, models));
        });
    }

    /// The tile's references: its elements of the three types that are shown, with the defaults filled in. Nothing to do on a tile without any.
    fn references(&self, e: &Editor, id: DocumentId) -> Vec<Reference> {
        let Some(d) = e.docs.find_as::<TileDocument>(id) else { return vec![] };
        let t = d.tile();
        let mut out = vec![];
        for tag in [REF_HUMAN, REF_UNIT, REF_SHIP] {
            if !t.type_counts().contains_key(tag) {
                continue;
            }

            let Some(kind) = Kind::of(tag) else { continue };
            for elem in t.tscene().find_all(tag).into_iter().filter(|&elem| t.shown(elem)) {
                let attr = |name: &str| t.tscene().doc().get(elem, name).unwrap_or_default();
                let Some((wanted, ship)) = self.wanted(e, &attr("unit"), &attr("faction"), attr("seed").trim().parse().unwrap_or(0), &attr("slot"), kind) else { continue };
                out.push(Reference { elem, kind, wanted, ship, colours: parse_colours(&attr("colours")), on_ground: attr("surface_mode") != "MSM_FREEFORM" });
            }
        }

        out
    }

    /// The unit and faction to show: the element's, else the game's default, else the first unit. A ship's: its naval unit, and the men aboard.
    fn wanted(&self, e: &Editor, unit: &str, faction: &str, seed: u64, slot: &str, kind: Kind) -> Option<(Wanted, Option<ShipWanted>)> {
        let catalog = self.catalog.as_ref()?;
        let faction_or = |default: &str| if unit.is_empty() { default.to_owned() } else { faction.to_owned() };
        if kind == Kind::Ship {
            let (default, default_faction) = default_naval(e.game().key);
            let chosen = [unit, default].into_iter().find(|u| !u.is_empty() && catalog.naval_units.contains_key(*u)).or_else(|| catalog.naval().first().copied())?;
            let faction = faction_or(default_faction);
            let men = Wanted { unit: catalog.naval_units[chosen].land_unit.clone(), faction: faction.clone(), seed, slot: slot.to_owned(), variations: UNIT_VARIATIONS };
            return Some((men, Some(ShipWanted { unit: chosen.to_owned(), faction, seed })));
        }

        let (default, default_faction) = default_unit(e.game().key);
        let chosen = [unit, default].into_iter().find(|u| !u.is_empty() && catalog.entry(u, "").is_some()).or_else(|| catalog.units().first().copied())?;
        let variations = if kind == Kind::Unit { UNIT_VARIATIONS } else { 1 };
        Some((Wanted { unit: chosen.to_owned(), faction: faction_or(default_faction), seed, slot: slot.to_owned(), variations }, None))
    }

    /// The water under a ship, asked of the tile again when it moves or now and then.
    fn water_under(&mut self, e: &Editor, id: DocumentId, elem: usize, x: f32, z: f32) -> Option<f32> {
        let now = gui::time();
        let at = [x.to_bits(), z.to_bits()];
        if let Some((was, when, level)) = self.water.get(&elem) && *was == at && now - when < WATER_SECONDS {
            return *level;
        }

        let level = e.docs.find_as::<TileDocument>(id)?.tile().water_level(x, z);
        self.water.insert(elem, (at, now, level));
        level
    }

    /// The game's unit size, read again now and then. Without a preferences file, the tables' size: the largest.
    fn unit_size(&mut self, e: &Editor) -> u32 {
        let now = gui::time();
        if self.size.is_none() || now - self.size_read > 3.0 {
            self.size_read = now;
            self.size = Some(read_game_graphics_values(e.game()).get("gfx_unit_size").and_then(|v| v.trim().parse().ok()).unwrap_or(3));
        }

        self.size.unwrap_or(3)
    }

    /// Per frame, with tile `id` open: what finished loading, and the soldiers of every reference in the scene, in their poses at this moment.
    pub fn update(&mut self, e: &mut Editor, id: DocumentId) {
        let started = std::time::Instant::now();
        self.update_scene(e, id);
        self.update_ms = started.elapsed().as_secs_f32() * 1000.0;
    }

    fn update_scene(&mut self, e: &mut Editor, id: DocumentId) {
        let any = e.docs.find_as::<TileDocument>(id).is_some_and(|d| [REF_HUMAN, REF_UNIT, REF_SHIP].iter().any(|k| d.tile().type_counts().contains_key(*k)));
        if !any || e.viewport.game_view {
            self.clear(e);
            if let Some(d) = e.docs.find_as::<TileDocument>(id) {
                d.tile().set_ref_spots(vec![]);
            }

            return;
        }

        self.load_catalog(e);
        self.poll();
        let references = self.references(e, id);
        let size = self.unit_size(e);
        let used_ships: HashSet<&ShipWanted> = references.iter().filter_map(|r| r.ship.as_ref()).collect();
        self.ships.retain(|w, _| used_ships.contains(w));
        self.water.retain(|elem, _| references.iter().any(|r| r.elem == *elem && r.kind == Kind::Ship));
        if self.ship_loading.is_none()
            && let Some(wanted) = references.iter().filter_map(|r| r.ship.as_ref()).find(|w| !self.ships.contains_key(*w) && !self.ship_failed.contains(*w)).cloned()
        {
            self.load_ship(e, wanted);
        }

        // The tile's scene layer; a new tile has a new one.
        let layer = match self.layer {
            Some((doc, layer)) if doc == id => layer,
            _ => {
                self.clear(e);
                let layer = e.scene.add_layer(id, "references");
                self.layer = Some((id, layer));
                layer
            }
        };

        // Each reference's men (where they stand: about its origin, or on a ship's deck) and, for a ship, its matrix.
        let catalog = self.catalog.clone();
        let mut ship_places: HashMap<usize, (Float4x4, Arc<ShipModel>)> = HashMap::new();
        let mut men_of: HashMap<usize, Vec<[f32; 3]>> = HashMap::new();
        let units = e.scene.game_units.max(1e-3);
        for r in &references {
            let stands: Vec<[f32; 3]> = match r.kind {
                Kind::Human => vec![[0.0; 3]],
                Kind::Unit => catalog.as_ref().and_then(|c| c.land_units.get(&r.wanted.unit)).map_or_else(|| vec![[0.0; 3]], |land| {
                    formation(men_at_size(land, size), land.rank_depth, land.spacing).into_iter().map(|[x, z]| [x, 0.0, z]).collect()
                }),
                Kind::Ship => {
                    let Some(ship) = r.ship.as_ref().and_then(|w| self.ships.get(w)).cloned() else { continue };
                    let Some(d) = e.docs.find_as::<TileDocument>(id) else { return };
                    let (at, m) = (d.tile().world_position(r.elem), d.tile().world_matrix(r.elem));
                    let ground = d.tile().terrain_height(at.x, at.z);
                    let y = if r.on_ground { self.water_under(e, id, r.elem, at.x, at.z).map_or(ground, |w| w.max(ground)) } else { at.y };
                    let place = translation(Float3::new(at.x, y, at.z)) * rotation_y(yaw_of(&m)) * scaling(1.0 / units);
                    let land = catalog.as_ref().and_then(|c| c.land_units.get(&ship.unit));
                    let spots = land.map_or_else(Vec::new, |land| ship.crew(men_at_size(land, size), land.spacing[0]));
                    ship_places.insert(r.elem, (place, ship));
                    spots
                }
            };

            men_of.insert(r.elem, stands);
        }

        self.place_ships(e, id, &ship_places);
        let used: HashSet<&Wanted> = references.iter().filter(|r| men_of.get(&r.elem).is_some_and(|m| !m.is_empty())).map(|r| &r.wanted).collect();
        self.models.retain(|w, _| used.contains(w));
        if self.loading.is_none()
            && let Some(wanted) = references.iter().map(|r| &r.wanted).find(|w| used.contains(w) && !self.models.contains_key(*w) && !self.failed.contains(*w)).cloned()
        {
            self.load_models(e, wanted);
        }

        // A soldier's poses are worked out once a frame, however many men show them.
        let time = e.clock.time() as f32;
        let mut poses: HashMap<(&Wanted, usize), Vec<UnitInstance>> = HashMap::new();
        let mut placed: Vec<(Float4x4, UnitInstance, Option<[[f32; 3]; 3]>)> = vec![];
        let mut spots = vec![];
        let Some(d) = e.docs.find_as::<TileDocument>(id) else { return };
        let t = d.tile();
        for r in &references {
            let Some(models) = self.models.get(&r.wanted) else { continue };
            let Some(stands) = men_of.get(&r.elem) else { continue };
            let at = t.world_position(r.elem);
            let m = t.world_matrix(r.elem);
            let turn = rotation_y(yaw_of(&m));
            let mounted = models[0].mount.is_some();
            let (chest, radius) = if mounted { (1.3, 1.3) } else { (0.9, 0.8) };
            for (k, stand) in stands.iter().enumerate() {
                // On a ship, its deck; else the ground under each man, or level at the element's height.
                let (x, y, z) = match ship_places.get(&r.elem) {
                    Some((place, _)) => {
                        let p = place.transform_point(Float3::new(stand[0], stand[1], stand[2]));
                        (p.x, p.y, p.z)
                    }

                    None => {
                        let local = turn.transform_point(Float3::new(stand[0] / units, 0.0, stand[2] / units));
                        let (x, z) = (at.x + local.x, at.z + local.z);
                        (x, if r.on_ground { t.terrain_height(x, z) } else { at.y }, z)
                    }
                };

                // The same soldier isn't next to himself, in file or in rank.
                let variation = (k * 7) % models.len();
                let model = &models[variation];
                let figures = poses.entry((&r.wanted, variation)).or_insert_with(|| model.instances(time, true));
                let place = translation(Float3::new(x, y, z)) * turn;
                for figure in figures.iter() {
                    // Models are in game units; the tile's scene isn't.
                    let mut world = place;
                    let k = figure.scale / units;
                    for row in 0..3 {
                        for column in 0..3 {
                            world.m[row][column] *= k;
                        }
                    }

                    placed.push((world * figure.local, figure.clone(), r.colours));
                }

                spots.push((r.elem, Float3::new(x, y + chest / units, z), radius / units));
            }
        }

        t.set_ref_spots(spots);
        if self.entities.len() != placed.len() || self.entities.iter().any(|&id| e.scene.find(id).is_none()) {
            self.clear(e);
            self.entities = placed.iter().map(|_| e.scene.add(layer, Float4x4::IDENTITY, Payload::CameraRig(Default::default()), SourceRef { doc: id, item: 0 })).collect();
        }

        for (&entity, (world, mut figure, colours)) in self.entities.iter().zip(placed) {
            if let Some(c) = colours {
                figure.instance.faction_colours = [c[0], c[1], c[2], figure.instance.faction_colours[3]];
            }

            e.scene.update_pose(entity, world, figure.instance);
        }
    }

    /// The ships' parts in the scene, each ship at its matrix: added again when what is shown changes, moved when only where does.
    fn place_ships(&mut self, e: &mut Editor, id: DocumentId, ships: &HashMap<usize, (Float4x4, Arc<ShipModel>)>) {
        let mut elems: Vec<&usize> = ships.keys().collect();
        elems.sort_unstable();
        let mut want: Vec<(String, usize, Float4x4)> = vec![];
        for &elem in elems {
            let (place, ship) = &ships[&elem];
            want.extend(ship.parts.iter().map(|p| (p.model.clone(), elem, *place * p.local)));
        }

        let Some(layer) = self.layer.filter(|(doc, _)| *doc == id).map(|(_, l)| l) else { return };
        let same = self.ship_shown.len() == want.len() && self.ship_shown.iter().zip(&want).all(|(a, b)| a.0 == b.0 && a.1 == b.1);
        if !same || self.ship_entities.iter().any(|&en| e.scene.find(en).is_none()) {
            self.clear_ships(e);
            self.ship_entities = want.iter().map(|(model, elem, world)| {
                let mesh = MeshInstance { model: model.clone(), lod: true, ..MeshInstance::default() };
                e.scene.add(layer, *world, Payload::MeshInstance(mesh), SourceRef { doc: id, item: item_of(*elem) })
            }).collect();
        } else {
            for (&entity, (shown, wanted)) in self.ship_entities.iter().zip(self.ship_shown.iter().zip(&want)) {
                if shown.2 != wanted.2 {
                    e.scene.update_world(entity, wanted.2);
                }
            }
        }

        self.ship_shown = want;
    }

    /// A list with a filter, for a combo: the item picked.
    fn combo(label: &str, filter: &mut String, value: &str, items: &[&str]) -> Option<String> {
        let mut picked = None;
        if gui::begin_combo(label, value) {
            gui::input_text(&format!("##{label}_filter"), Some("filter"), filter, 64, 0);
            let words = filter.to_lowercase();
            let shown: Vec<&&str> = items.iter().filter(|i| i.contains(&words)).collect();
            gui::clipped(shown.len(), |k| {
                if gui::selectable(shown[k], *shown[k] == value, 0) {
                    picked = Some((*shown[k]).to_owned());
                }
            });

            gui::end_combo();
        }

        picked
    }

    /// The Inspector's rows of the selected references, below their properties: an edit goes to all of them.
    pub fn draw(&mut self, e: &mut Editor, id: DocumentId) {
        let Some(d) = e.docs.find_as::<TileDocument>(id) else { return };
        let is_reference = |&elem: &usize| Kind::of(d.tile().tscene().type_of(elem)).is_some();
        let elems: Vec<usize> = selected(e).into_iter().filter(is_reference).collect();
        let Some(&first) = elems.first() else { return };
        gui::separator_text("Reference");
        if !self.status.is_empty() {
            gui::text_disabled(&self.status);
        }

        let Some(catalog) = self.catalog.clone() else { return };
        let Some(kind) = Kind::of(d.tile().tscene().type_of(first)) else { return };
        let attr = |name: &str| d.tile().tscene().doc().get(first, name).unwrap_or_default();
        let seed: u64 = attr("seed").trim().parse().unwrap_or(0);
        let Some((wanted, ship)) = self.wanted(e, &attr("unit"), &attr("faction"), seed, &attr("slot"), kind) else { return };
        let stored = parse_colours(&attr("colours"));
        let mut edits: BTreeMap<&str, String> = BTreeMap::new();

        // A ship's unit is its naval unit; its men's land unit is shown below.
        let (units, unit) = match &ship {
            Some(s) => (catalog.naval(), s.unit.clone()),
            None => (catalog.units(), wanted.unit.clone()),
        };

        let mut factions: Vec<&str> = catalog.colours.keys().map(String::as_str).collect();
        factions.sort_unstable();
        if let Some(unit) = Self::combo("Unit##reference", &mut self.unit_filter, &unit, &units) {
            edits.insert("unit", unit);
            edits.insert("faction", wanted.faction.clone());
        }

        if let Some(faction) = Self::combo("Faction##reference", &mut self.faction_filter, &wanted.faction, &factions) {
            edits.insert("unit", unit.clone());
            edits.insert("faction", faction);
            edits.insert("colours", String::new());
        }

        let slots = self.models.get(&wanted).map(|m| m[0].slots()).unwrap_or_default();
        if let Some(slot) = Self::combo("Plays##reference", &mut self.slot_filter, &wanted.slot, &slots) {
            edits.insert("slot", slot);
        }

        let mut seed_box = seed as i32;
        gui::set_next_item_width(120.0);
        if gui::input_int("Soldier##reference", &mut seed_box) {
            edits.insert("seed", seed_box.unsigned_abs().to_string());
        }

        gui::same_line();
        if gui::small_button("Next##reference") {
            edits.insert("seed", seed.wrapping_add(1).to_string());
        }

        // The faction's colours, or the user's.
        let white = [[1.0; 3], [1.0; 3], [1.0; 3], [0.0; 3]];
        let of_faction = catalog.colours.get(&wanted.faction).copied().unwrap_or(white);
        let mut colours = stored.unwrap_or([of_faction[0], of_faction[1], of_faction[2]]);
        let mut changed = false;
        for (k, name) in ["Primary", "Secondary", "Uniform"].iter().enumerate() {
            changed |= gui::color_edit3(&format!("{name}##reference"), &mut colours[k], 0);
        }

        if changed {
            edits.insert("colours", format_colours(&colours));
        }

        if stored.is_some() && gui::small_button("Faction's colours##reference") {
            edits.insert("colours", String::new());
        }

        if let Some(model) = self.models.get(&wanted).and_then(|m| m.first()) {
            gui::text_disabled(&format!("{}, scale {:.3}{}", model.man.rig.skeleton.skeleton, model.scale, model.mount.as_ref().map_or(String::new(), |m| format!(", on {}", m.rig.skeleton.skeleton))));
        }

        if let Some(s) = &ship {
            let naval = catalog.naval_units.get(&s.unit);
            let size = self.size.unwrap_or(3);
            let men = catalog.land_units.get(&s.unit).map_or(0, |land| men_at_size(land, size));
            let model = self.ships.get(s).map_or_else(|| "loading".to_owned(), |m| format!("{}, {} parts", m.variant, m.parts.len()));
            gui::text_disabled(&format!("{} ({}), {model}", naval.map_or("", |n| n.ship.as_str()), naval.map_or("", |n| n.category.as_str())));
            gui::text_disabled(&format!("{men} men of {} aboard (the game's unit size: {})", wanted.unit, size_name(size)));
        }

        if kind == Kind::Unit && let Some(land) = catalog.land_units.get(&wanted.unit) {
            let size = self.size.unwrap_or(3);
            let men = men_at_size(land, size);
            let ranks = land.rank_depth.clamp(1, men.max(1));
            let (wide, deep) = (men.div_ceil(ranks).saturating_sub(1) as f32 * land.spacing[0], (ranks - 1) as f32 * land.spacing[1]);
            gui::text_disabled(&format!("{men} men in {ranks} ranks, {wide:.1} x {deep:.1} m (the game's unit size: {})", size_name(size)));
        }

        let Some(d) = e.docs.find_as_mut::<TileDocument>(id) else { return };
        for (name, value) in edits {
            d.set_attribute_all(&elems, name, &value);
        }
    }
}

#[cfg(test)]
mod tests {
    use wce_foundation::math::rotation_y;

    use super::{format_colours, parse_colours, yaw_of};

    #[test]
    fn a_turn_comes_back_from_its_matrix() {
        for yaw in [-2.5f32, -1.0, 0.0, 0.7, 3.0] {
            assert!((yaw_of(&rotation_y(yaw)) - yaw).abs() < 1e-5, "{yaw}");
        }
    }

    #[test]
    fn colours_are_nine_numbers() {
        let c = [[0.25, 0.5, 1.0], [0.0, 0.1, 0.2], [1.0, 1.0, 1.0]];
        assert_eq!(parse_colours(&format_colours(&c)), Some(c));
        assert_eq!(parse_colours(""), None);
        assert_eq!(parse_colours("1,2,3"), None);
    }
}
