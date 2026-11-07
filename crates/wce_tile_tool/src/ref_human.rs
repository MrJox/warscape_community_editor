//! `ref_human`: a real unit standing in the tile, to measure the scale against (docs/plans/units.md). Editor only: it lives in a scene layer of
//! its own and the settings, never in the tile's files or an export. Which unit, faction and soldier it is can be switched, to compare sizes
//! (infantry, cavalry, a mod's trolls); the table and the models load on a thread of their own. It can't be picked: the Place button moves it.

use std::sync::Arc;
use std::sync::mpsc::{Receiver, TryRecvError, channel};

use rpfm_lib::schema::Schema;
use serde::{Deserialize, Serialize};
use wce_editor::editor::Editor;
use wce_editor::gui;
use wce_foundation::math::{Float3, Float4x4, rotation_y, translation};
use wce_foundation::settings::rpfm_schema_path;
use wce_render::unit_animation::UnitModel;
use wce_render::unit_catalog::UnitCatalog;
use wce_scene::render_scene::{DocumentId, EntityId, LayerId, Payload, SourceRef};
use wce_tile_editor::tile_document::TileDocument;

use crate::settings::{SETTINGS_ID, TileSettings};

/// Each game's default: the unit and faction of a soldier of ordinary size. A game without one takes the first unit of its tables.
fn default_unit(game: &str) -> (&'static str, &'static str) {
    match game {
        "attila" => ("att_rom_legio_comitatenses", "att_fact_western_roman_empire"),
        _ => ("", ""),
    }
}

/// What the user picks: the `[tools.tile.ref_human]` table.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct RefHumanSettings {
    /// A `unit_variants` unit; empty: the game's default.
    pub unit: String,
    pub faction: String,
    pub seed: u64,

    /// Replaces the faction's three colours.
    pub colours: Option<[[f32; 3]; 3]>,

    /// The animation slot it plays; empty: it stands.
    pub slot: String,
}

/// A soldier wanted: what the model on hand was loaded for: unit, faction, soldier, slot.
type Wanted = (String, String, u64, String);

#[derive(Default)]
pub struct RefHuman {
    pub show: bool,

    /// The next click on the terrain places it.
    pub placing: bool,

    /// (x, y, z, turn in degrees): scene units, where it stands in the open tile.
    at: Option<[f32; 4]>,
    at_tile: DocumentId,
    catalog: Option<Arc<UnitCatalog>>,
    catalog_rx: Option<Receiver<Result<UnitCatalog, String>>>,
    model: Option<(Wanted, Arc<UnitModel>)>,
    model_rx: Option<Receiver<(Wanted, Result<UnitModel, String>)>>,
    /// A soldier that didn't load, not to try again.
    failed: Option<Wanted>,
    entities: Vec<EntityId>,
    layer: Option<(DocumentId, LayerId)>,
    status: String,
    unit_filter: String,
    faction_filter: String,
    slot_filter: String,
}

/// The key of a tile in [`TileSettings::ref_human_at`].
fn tile_key(e: &Editor, id: DocumentId) -> Option<String> {
    Some(e.docs.find_as::<TileDocument>(id)?.project().dir.display().to_string().replace('\\', "/").to_lowercase())
}

impl RefHuman {
    /// The game changed: its tables and models go.
    pub fn game_changed(&mut self, e: &mut Editor) {
        self.catalog = None;
        self.catalog_rx = None;
        self.model = None;
        self.model_rx = None;
        self.clear(e);
    }

    fn clear(&mut self, e: &mut Editor) {
        for id in self.entities.drain(..) {
            e.scene.remove(id);
        }
    }

    /// Where it stands in tile `id`: where it was placed, else the tile's middle.
    fn position(&mut self, e: &Editor, id: DocumentId) -> Option<[f32; 4]> {
        if self.at_tile != id || self.at.is_none() {
            let d = e.docs.find_as::<TileDocument>(id)?;
            let saved = tile_key(e, id).and_then(|k| e.tool_settings::<TileSettings>(SETTINGS_ID).ref_human_at.get(&k).copied());
            let (x, z) = ((d.tile().extent_x() * 0.5) as f32, (d.tile().extent_z() * 0.5) as f32);
            self.at = Some(saved.unwrap_or([x, d.tile().terrain_height(x, z), z, 0.0]));
            self.at_tile = id;
        }

        self.at
    }

    /// Stands it at a point of tile `id`, turned `yaw` degrees (`None`: as it was).
    pub fn place(&mut self, e: &mut Editor, id: DocumentId, at: Float3, yaw: Option<f32>) {
        let yaw = yaw.or_else(|| self.position(e, id).map(|p| p[3])).unwrap_or(0.0);
        self.at = Some([at.x, at.y, at.z, yaw]);
        self.at_tile = id;
        self.placing = false;
        if let Some(key) = tile_key(e, id) {
            e.update_tool_settings::<TileSettings>(SETTINGS_ID, |s| {
                s.ref_human_at.insert(key.clone(), [at.x, at.y, at.z, yaw]);
            });
        }
    }

    fn load_catalog(&mut self, e: &Editor) {
        if self.catalog.is_some() || self.catalog_rx.is_some() {
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

    /// The unit and faction to show: the settings', else the game's default, else the first unit.
    fn wanted(&self, e: &Editor) -> Option<Wanted> {
        let s = e.tool_settings::<TileSettings>(SETTINGS_ID).ref_human;
        let catalog = self.catalog.as_ref()?;
        let (unit, faction) = default_unit(e.game().key);
        let unit = [s.unit.as_str(), unit].into_iter().find(|u| !u.is_empty() && catalog.entry(u, "").is_some()).or_else(|| catalog.units().first().copied())?;
        let faction = if s.unit.is_empty() { faction.to_owned() } else { s.faction };
        Some((unit.to_owned(), faction, s.seed, s.slot))
    }

    fn load_model(&mut self, e: &Editor, wanted: Wanted) {
        let (Some(vfs), Some(catalog)) = (e.vfs().cloned(), self.catalog.clone()) else { return };
        let (tx, rx) = channel();
        self.model_rx = Some(rx);
        format!("loading {}...", wanted.0).clone_into(&mut self.status);
        std::thread::spawn(move || {
            let read = |p: &str| vfs.read(p).ok();
            let model = UnitModel::load(&read, &catalog, &wanted.0, &wanted.1, wanted.2).map(|mut m| {
                if !wanted.3.is_empty() {
                    m.play(&read, &wanted.3);
                }

                m
            });
            let _ = tx.send((wanted, model));
        });
    }

    /// Per frame, with tile `id` open: what is finished loading, and the soldier in the scene, in his pose at this moment.
    pub fn update(&mut self, e: &mut Editor, id: DocumentId) {
        if !self.show {
            self.clear(e);
            return;
        }

        self.load_catalog(e);
        if let Some(rx) = &self.catalog_rx {
            match rx.try_recv() {
                Ok(Ok(catalog)) => {
                    self.catalog = Some(Arc::new(catalog));
                    self.catalog_rx = None;
                    self.status.clear();
                }

                Ok(Err(error)) => {
                    self.catalog_rx = None;
                    self.show = false;
                    self.status = error;
                }

                Err(TryRecvError::Disconnected) => self.catalog_rx = None,
                Err(TryRecvError::Empty) => {}
            }
        }

        if let Some(rx) = &self.model_rx {
            match rx.try_recv() {
                Ok((wanted, Ok(model))) => {
                    self.status.clear();
                    self.model = Some((wanted, Arc::new(model)));
                    self.model_rx = None;
                }

                Ok((wanted, Err(error))) => {
                    self.status = error;
                    self.model = None;
                    self.model_rx = None;
                    self.failed = Some(wanted);
                }

                Err(TryRecvError::Disconnected) => self.model_rx = None,
                Err(TryRecvError::Empty) => {}
            }
        }

        let Some(wanted) = self.wanted(e) else { return };
        if self.model_rx.is_none() && self.model.as_ref().is_none_or(|(w, _)| *w != wanted) && self.failed.as_ref() != Some(&wanted) {
            self.load_model(e, wanted.clone());
        }

        let Some((_, model)) = self.model.clone().filter(|(w, _)| *w == wanted) else {
            self.clear(e);
            return;
        };

        // The tile's scene layer; a new tile has a new one.
        let layer = match self.layer {
            Some((doc, layer)) if doc == id => layer,
            _ => {
                self.clear(e);
                let layer = e.scene.add_layer(id, "ref_human");
                self.layer = Some((id, layer));
                layer
            }
        };

        let Some(mut at) = self.position(e, id) else { return };

        // It stands on the ground as the ground is now: the terrain may have been edited under it.
        if let Some(d) = e.docs.find_as::<TileDocument>(id) {
            at[1] = d.tile().terrain_height(at[0], at[2]);
        }

        let colours = e.tool_settings::<TileSettings>(SETTINGS_ID).ref_human.colours;
        let mut figures = model.instances(e.clock.time() as f32, true);
        if self.entities.len() != figures.len() || self.entities.iter().any(|&id| e.scene.find(id).is_none()) {
            self.clear(e);
            self.entities = figures.iter().map(|_| e.scene.add(layer, Float4x4::IDENTITY, Payload::CameraRig(Default::default()), SourceRef { doc: id, item: 0 })).collect();
        }

        // Models are in game units; the tile's scene isn't.
        let units = e.scene.game_units.max(1e-3);
        let place = translation(Float3::new(at[0], at[1], at[2])) * rotation_y(at[3].to_radians());
        for (&entity, figure) in self.entities.iter().zip(&mut figures) {
            if let Some(c) = colours {
                figure.instance.faction_colours = [c[0], c[1], c[2], figure.instance.faction_colours[3]];
            }

            let mut world = place;
            let k = figure.scale / units;
            for r in 0..3 {
                for c in 0..3 {
                    world.m[r][c] *= k;
                }
            }

            e.scene.update_world(entity, world * figure.local);
            e.scene.update_payload(entity, Payload::SkinnedInstance(figure.instance.clone()));
        }
    }

    /// A list with a filter, for a combo: true when `value` was picked.
    fn combo(label: &str, filter: &mut String, value: &mut String, items: &[&str]) -> bool {
        let mut picked = false;
        if gui::begin_combo(label, value) {
            gui::input_text(&format!("##{label}_filter"), Some("filter"), filter, 64, 0);
            let words = filter.to_lowercase();
            for item in items.iter().filter(|i| i.contains(&words)).take(300) {
                if gui::selectable(item, *item == value.as_str(), 0) {
                    (*item).clone_into(value);
                    picked = true;
                }
            }

            gui::end_combo();
        }

        picked
    }

    /// The Tile panel's section.
    pub fn draw(&mut self, e: &mut Editor, id: DocumentId) {
        if !gui::collapsing_header("Reference unit (ref_human)", 0) {
            return;
        }

        gui::checkbox("Show##ref_human", &mut self.show);
        if gui::is_item_hovered() {
            gui::set_tooltip("A unit standing in the tile, to measure the scale against. It is not part of the tile: it is not saved in it or exported.");
        }

        if !self.status.is_empty() {
            gui::text_disabled(&self.status);
        }

        if !self.show {
            return;
        }

        if gui::button(if self.placing { "Click on the terrain...##ref_human" } else { "Place##ref_human" }) {
            self.placing = !self.placing;
        }

        let Some(at) = self.position(e, id) else { return };
        let mut turn = at[3];
        gui::same_line();
        gui::set_next_item_width(160.0);
        if gui::slider_float("Turn##ref_human", &mut turn, -180.0, 180.0, "%.0f deg") {
            self.place(e, id, Float3::new(at[0], at[1], at[2]), Some(turn));
        }

        let Some(catalog) = self.catalog.clone() else { return };
        let Some((mut unit, mut faction, seed, mut slot)) = self.wanted(e) else { return };
        let units = catalog.units();
        let mut factions: Vec<&str> = catalog.colours.keys().map(String::as_str).collect();
        factions.sort_unstable();
        let picked_unit = Self::combo("Unit##ref_human", &mut self.unit_filter, &mut unit, &units);
        let picked_faction = Self::combo("Faction##ref_human", &mut self.faction_filter, &mut faction, &factions);
        let slots = self.model.as_ref().map(|(_, m)| m.slots()).unwrap_or_default();
        let picked_slot = Self::combo("Plays##ref_human", &mut self.slot_filter, &mut slot, &slots);
        let mut seed_box = seed as i32;
        gui::set_next_item_width(120.0);
        let picked_seed = gui::input_int("Soldier##ref_human", &mut seed_box);
        gui::same_line();
        let next = gui::small_button("Next##ref_human");
        if picked_unit || picked_faction || picked_seed || next || picked_slot {
            let seed = if next { seed.wrapping_add(1) } else { u64::from(seed_box.unsigned_abs()) };
            e.update_tool_settings::<TileSettings>(SETTINGS_ID, |s| {
                s.ref_human.unit.clone_from(&unit);
                s.ref_human.faction.clone_from(&faction);
                s.ref_human.seed = seed;
                s.ref_human.slot.clone_from(&slot);
                if picked_faction {
                    s.ref_human.colours = None;
                }
            });
        }

        // The faction's colours, or the user's.
        let s = e.tool_settings::<TileSettings>(SETTINGS_ID).ref_human;
        let white = [[1.0; 3], [1.0; 3], [1.0; 3], [0.0; 3]];
        let of_faction = catalog.colours.get(&faction).copied().unwrap_or(white);
        let mut colours = s.colours.unwrap_or([of_faction[0], of_faction[1], of_faction[2]]);
        let mut changed = false;
        for (k, name) in ["Primary", "Secondary", "Uniform"].iter().enumerate() {
            changed |= gui::color_edit3(&format!("{name}##ref_human"), &mut colours[k], 0);
        }

        if changed {
            e.update_tool_settings::<TileSettings>(SETTINGS_ID, |t| t.ref_human.colours = Some(colours));
        }

        if s.colours.is_some() && gui::small_button("Faction's colours##ref_human") {
            e.update_tool_settings::<TileSettings>(SETTINGS_ID, |t| t.ref_human.colours = None);
        }

        if let Some((_, model)) = &self.model {
            gui::text_disabled(&format!("{}, scale {:.3}{}", model.man.rig.skeleton.skeleton, model.scale, model.mount.as_ref().map_or(String::new(), |m| format!(", on {}", m.rig.skeleton.skeleton))));
        }
    }
}
