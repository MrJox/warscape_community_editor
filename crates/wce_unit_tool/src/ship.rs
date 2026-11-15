//! A ship's definition, drawn as the ship it is: not worn by a soldier (its pieces are static meshes on bones of its own, which a soldier's
//! skeleton scattered) but assembled as the game assembles a ship (`assemble_ship_with`: its pieces intact, a part on every point of a
//! list), with what its naval unit's logic adds (oars, weapons: `ShipModel::with_parts`), as rigid meshes, its sails at rest, its ropes as
//! the file has them. It stands on the stage on its keel; the camera frames it once its models are in.
//!
//! The parts are meshes the renderer picks by itself: a click finds a part's entity, whose item names the part, and the selection outlines
//! the selected node's parts as a building's.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use wce_editor::editor::Editor;
use wce_formats::variant_mesh::{Alternative, NodePath, VariantMesh, VariantMeshFile};
use wce_foundation::math::{Float3, scaling, translation};
use wce_render::model_cache::State;
use wce_render::ship_model::{ShipModel, ShipPart};
use wce_render::unit_assembly::{Pins, assemble_ship_with};
use wce_render::unit_catalog::variant_path;
use wce_scene::render_scene::{
    DocumentId, EntityId, MODEL_ROPES_SUFFIX, MODEL_SKINNED_SUFFIX, MeshInstance, Payload, SKINNED_NO_OFFSET, SkinnedInstance, SkinnedPart,
    SourceRef,
};
use wce_vfs::normalize;

use super::{UnitTool, document, hash_of};

/// A ship part's entities have item `SHIP_ITEM + its index` (the ground and soldiers are 0, the outline's mask 1).
const SHIP_ITEM: u64 = 1000;

/// The sails' palette at rest: every cloth bone where the file has it (as the tile's references draw a sail without its cloth).
const SAIL_BONES: usize = 4096;

/// What the tool knows of the open definition as a ship.
#[derive(Default)]
pub(super) struct ShipState {
    /// The open definition is a ship's.
    pub on: bool,

    /// Its naval unit (a `main_units` key), for its logic: oars, weapons. None: the pieces alone.
    pub unit: Option<String>,

    /// No row names the file: the unit is the one whose ship's definition is named most like it.
    pub guessed: bool,

    /// The open file is a part of a ship (its sails): the ship's definition that references it, shown with it where it does.
    pub host: Option<String>,
    model: Option<ShipModel>,

    /// What the model was made from.
    key: Option<u64>,
    entities: Vec<EntityId>,

    /// How far the ship is lifted to stand on its keel (scene units), found once its models are in.
    lift: Option<f32>,
    framed: bool,
    rest: Option<Arc<Vec<[f32; 12]>>>,

    /// The outline's selection as last set.
    outlined: Option<Option<NodePath>>,

    /// Looked at with the unit tables read (a ship is told by them).
    pub checked: bool,
}

/// Whether a definition is a ship's: a `unit_variants_ships` row names it, or its slots are pieces with damage states (`_dm1`).
pub(super) fn is_ship(tool: &UnitTool, path: &str, tree: &VariantMeshFile) -> bool {
    tool.catalog.as_ref().is_some_and(|c| c.ships.iter().any(|s| variant_path(&s.variant) == path))
        || tree.root.slots.iter().any(|s| s.name.to_lowercase().ends_with("_dm1"))
}

/// Whether a file is under the ships' models (`variantmeshes/_variantmodels/ships/`): a ship's part, never a soldier's.
fn in_ships_folder(path: &str) -> bool {
    path.contains("/_variantmodels/ships/")
}

/// The ship rows' definitions that reference `path` at any depth (a ship's sails, its shields), sorted.
fn ships_referencing(read: &dyn Fn(&str) -> Option<Vec<u8>>, ships: &[String], path: &str) -> Vec<String> {
    let mut refs: HashMap<String, Vec<String>> = HashMap::new();
    let mut leads = |file: &str| -> bool {
        let mut seen = HashSet::new();
        let mut todo = vec![(file.to_owned(), 0)];
        while let Some((f, depth)) = todo.pop() {
            if f == path {
                return true;
            }

            if depth > 8 || !seen.insert(f.clone()) {
                continue;
            }

            let names = refs
                .entry(f.clone())
                .or_insert_with(|| read(&f).and_then(|b| VariantMeshFile::parse(b).ok()).map(|t| references(&t.root)).unwrap_or_default());
            todo.extend(names.iter().map(|n| (n.clone(), depth + 1)));
        }

        false
    };

    let mut out: Vec<String> = ships.iter().filter(|s| s.as_str() != path && leads(s)).cloned().collect();
    out.sort();
    out.dedup();
    out
}

/// The files a mesh's references name, normalized, at any depth of its own meshes.
fn references(mesh: &VariantMesh) -> Vec<String> {
    let mut out = vec![];
    for slot in &mesh.slots {
        for a in &slot.alternatives {
            match a {
                Alternative::Mesh(m) => out.extend(references(m)),
                Alternative::Reference { definition, .. } => out.push(normalize(definition)),
            }
        }
    }

    out
}

impl UnitTool {
    /// A document opened: whether it is a ship's, and the naval unit whose ship it is.
    pub(super) fn ship_opened(&mut self, e: &mut Editor, path: &str) {
        self.clear_ship(e);
        let Some(tree) = document(e).map(|d| d.tree()) else { return };
        // A part of a ship (its sails): shown on a ship whose definition references it.
        let host = (!is_ship(self, path, &tree))
            .then(|| {
                let (vfs, c) = (e.vfs()?, self.catalog.as_ref()?);
                let read = |p: &str| vfs.read(p).ok();
                let ships: Vec<String> = c.ships.iter().map(|s| variant_path(&s.variant)).collect();
                ships_referencing(&read, &ships, path).into_iter().next()
            })
            .flatten();
        let on = is_ship(self, path, &tree) || host.is_some() || in_ships_folder(path);
        // The naval unit whose ship it is; else the one whose ship's definition is named most like it (a culture's copy of a base file:
        // `att_shp_dromon_melee_heavy_er` for `att_shp_dromon_melee_heavy`).
        let name = path.rsplit('/').next().unwrap_or_default().trim_end_matches(".variantmeshdefinition").to_lowercase();
        let found = self.catalog.as_ref().and_then(|c| {
            let common = |v: &str| v.to_lowercase().bytes().zip(name.bytes()).take_while(|(a, b)| a == b).count();
            let shown = host.as_deref().unwrap_or(path);
            let (entry, guessed) = match c.ships.iter().find(|s| variant_path(&s.variant) == shown) {
                Some(entry) => (entry, false),
                None => (
                    c.ships
                        .iter()
                        .filter(|s| common(&s.variant) >= 8)
                        .max_by_key(|s| (common(&s.variant), std::cmp::Reverse(s.variant.len())))?,
                    true,
                ),
            };
            let mut units: Vec<&String> = c.naval_units.iter().filter(|(_, n)| n.naval_unit == entry.unit).map(|(k, _)| k).collect();
            units.sort();
            units.first().map(|u| ((*u).clone(), guessed))
        });
        let checked = self.catalog.is_some();
        let (unit, guessed) = found.map_or((None, false), |(u, g)| (Some(u), g));
        self.ship = ShipState { on, unit, guessed, host, checked, ..ShipState::default() };
    }

    pub(super) fn clear_ship(&mut self, e: &mut Editor) {
        e.scene.remove_ids(&self.ship.entities);
        self.ship.entities.clear();
        self.ship.model = None;
        self.ship.key = None;
        self.ship.outlined = None;
    }

    /// The ship's node of a part entity's item.
    pub(super) fn ship_node_of(&self, item: u64) -> Option<NodePath> {
        let k = usize::try_from(item.checked_sub(SHIP_ITEM)?).ok()?;
        self.ship.model.as_ref()?.parts.get(k)?.node.clone()
    }

    /// Per frame: the ship made again when its definition or pins changed, placed, lifted, framed and outlined.
    pub(super) fn update_ship(&mut self, e: &mut Editor) {
        let Some(id) = (self.doc != 0).then_some(self.doc) else { return };
        let (Some(vfs), Some(catalog)) = (e.vfs().cloned(), self.catalog.clone()) else { return };
        let Some(d) = document(e) else { return };
        let (tree, version, path) = (d.tree(), d.version(), d.source().vfs_path.clone());
        let edited = d.xml().save();
        let pins = self.effective_pins();
        let key = hash_of(&(
            version,
            id,
            format!("{:?}", pins.slots.iter().collect::<std::collections::BTreeMap<_, _>>()),
            &self.selected.as_ref().filter(|_| self.isolate),
            &self.ship.unit,
            &self.faction,
            self.seed,
        ));
        if self.ship.key != Some(key) {
            let read = |p: &str| if p == path { Some(edited.clone()) } else { vfs.read(p).ok() };
            // A ship's part on the ship that references it, pinned where it does; else the file itself.
            let host = self.ship.host.as_deref().and_then(|h| Some((h, VariantMeshFile::parse(vfs.read(h).ok()?).ok()?)));
            let mut parts = match &host {
                Some((_, ship)) => {
                    let focus = Pins { focus: Some(path.clone()), show_focus: true, ..pins.clone() };
                    assemble_ship_with(&read, ship, self.seed, &focus)
                }

                None => assemble_ship_with(&read, &tree, self.seed, &pins),
            };
            if self.isolate
                && let Some(sel) = &self.selected
            {
                parts.parts.retain(|p| p.node.as_ref().is_some_and(|n| n.starts_with(sel)));
            }

            self.missing.clone_from(&parts.missing);
            let shown = host.as_ref().map_or(self.preview_path.as_str(), |(h, _)| h);
            let name = shown.rsplit('/').next().unwrap_or_default().trim_end_matches(".variantmeshdefinition").to_owned();
            let model = ShipModel::with_parts(&read, &catalog, self.ship.unit.as_deref(), &self.faction, &name, parts, self.seed);
            e.scene.remove_ids(&self.ship.entities);
            self.ship.entities.clear();
            self.ship.model = Some(model);
            self.ship.key = Some(key);
            self.ship.outlined = None;
            self.worn = Some(version);
            if host.is_some() { "shown on the ship that references it" } else { "drawn as a ship" }.clone_into(&mut self.worn_how);
        }

        let Some(layer) = self.layer.filter(|(doc, l)| *doc == id && e.scene.layer(*l).is_some()).map(|(_, l)| l).or_else(|| {
            let l = e.scene.add_layer(id, "unit preview");
            self.layer = Some((id, l));
            Some(l)
        }) else {
            return;
        };

        let units = e.scene.game_units.max(1e-3);
        if self.ship.lift.is_none() {
            self.ship.lift = self.keel(e, units);
        }

        let place = translation(Float3::new(0.0, self.ship.lift.unwrap_or(0.0), 0.0)) * scaling(1.0 / units);
        if self.ship.entities.is_empty()
            && let Some(model) = &self.ship.model
        {
            let rest = self.ship.rest.get_or_insert_with(|| Arc::new(vec![SKINNED_NO_OFFSET; SAIL_BONES])).clone();
            let colours = self.colours.map_or(model.faction_colours, |c| [c[0], c[1], c[2], model.faction_colours[3]]);
            for (k, p) in model.parts.iter().enumerate() {
                let world = place * ShipModel::part_at(p, 0.0, false);
                let source = SourceRef { doc: id, item: SHIP_ITEM + k as u64 };
                for payload in payloads(p, &rest, colours) {
                    self.ship.entities.push(e.scene.add(layer, world, payload, source));
                }
            }
        }

        if !self.ship.framed
            && let Some(radius) = self.ship_radius(e, units)
        {
            self.ship.framed = true;
            let y = self.ship.lift.unwrap_or(0.0) + radius * 0.25;
            e.viewport.controller().set_orbit(std::f32::consts::PI + 0.7, 0.3, radius * 1.7, Float3::new(0.0, y, 0.0));
        }

        self.outline_ship(e, id);
    }

    /// The selection outlines the selected node's parts (the renderer outlines meshes by their entities).
    fn outline_ship(&mut self, e: &mut Editor, id: DocumentId) {
        if self.ship.outlined.as_ref() == Some(&self.selected) {
            return;
        }

        self.ship.outlined = Some(self.selected.clone());
        let Some(model) = &self.ship.model else { return };
        let mine = |r: &SourceRef| r.doc == id;
        let items: Vec<SourceRef> = self
            .selected
            .as_ref()
            .map(|sel| {
                model
                    .parts
                    .iter()
                    .enumerate()
                    .filter(|(_, p)| p.node.as_ref().is_some_and(|n| n.starts_with(sel)))
                    .map(|(k, _)| SourceRef { doc: id, item: SHIP_ITEM + k as u64 })
                    .collect()
            })
            .unwrap_or_default();
        if items.is_empty() {
            if e.selection.items().iter().any(mine) {
                e.selection.clear();
            }

            return;
        }

        e.selection.set(items[0]);
        for r in &items[1..] {
            e.selection.add(*r);
        }
    }

    /// The bounds of the parts whose models are in (scene units, the ship unlifted); `None` until one is.
    fn ship_bounds(&self, e: &mut Editor, units: f32) -> Option<(Float3, Float3)> {
        let model = self.ship.model.as_ref()?;
        let r = e.renderer()?;
        let mut lo = Float3::new(f32::MAX, f32::MAX, f32::MAX);
        let mut hi = Float3::new(f32::MIN, f32::MIN, f32::MIN);
        let place = scaling(1.0 / units);
        for p in model.parts.iter().filter(|p| p.what.solid || p.what.skinned) {
            let key = if p.what.solid { p.model.clone() } else { format!("{}{MODEL_SKINNED_SUFFIX}", p.model) };
            match r.models().state(&key) {
                State::Ready => {}
                State::Failed => continue,
                _ => return None,
            }

            let Some(m) = r.models().model(&key) else { continue };
            let world = place * p.local;
            for k in 0..8 {
                let c = Float3::new(
                    if k & 1 != 0 { m.bmax.x } else { m.bmin.x },
                    if k & 2 != 0 { m.bmax.y } else { m.bmin.y },
                    if k & 4 != 0 { m.bmax.z } else { m.bmin.z },
                );
                let q = world.transform_point(c);
                lo = Float3::new(lo.x.min(q.x), lo.y.min(q.y), lo.z.min(q.z));
                hi = Float3::new(hi.x.max(q.x), hi.y.max(q.y), hi.z.max(q.z));
            }
        }

        (lo.x <= hi.x).then_some((lo, hi))
    }

    /// How far to lift the ship so its keel is on the stage (its origin is its waterline).
    fn keel(&self, e: &mut Editor, units: f32) -> Option<f32> {
        self.ship_bounds(e, units).map(|(lo, _)| (-lo.y).max(0.0))
    }

    fn ship_radius(&self, e: &mut Editor, units: f32) -> Option<f32> {
        self.ship_bounds(e, units).map(|(lo, hi)| {
            let d = hi - lo;
            (d.x * d.x + d.y * d.y + d.z * d.z).sqrt() * 0.5
        })
    }

    /// The naval unit whose logic adds oars and weapons: another one, or none.
    pub(super) fn set_ship_unit(&mut self, e: &mut Editor, unit: Option<String>) {
        self.clear_ship(e);
        self.ship.unit = unit;
        self.ship.guessed = false;
        self.ship.lift = None;
        self.ship.framed = false;
    }

    /// For the scripts: the lift, whether framed, the parts and those whose models aren't in.
    pub(super) fn ship_state(&self, e: &mut Editor) -> String {
        let mut waiting = vec![];
        if let (Some(r), Some(m)) = (e.renderer(), &self.ship.model) {
            for p in m.parts.iter().filter(|p| p.what.solid) {
                let state = r.models().state(&p.model);
                if state != State::Ready {
                    waiting.push(format!("{}:{state:?}", super::file_name(&p.model)));
                }
            }
        }

        format!(
            "lift {:?} framed {} parts {} not ready {waiting:?}",
            self.ship.lift,
            self.ship.framed,
            self.ship.model.as_ref().map_or(0, |m| m.parts.len())
        )
    }

    /// The node of what a click hit, when it is a part of the ship.
    pub(super) fn ship_pick(&self, hit: SourceRef) -> Option<NodePath> {
        (hit.doc == self.doc).then(|| self.ship_node_of(hit.item)).flatten()
    }

    /// The model of a ship's part of a node, for the status line.
    pub(super) fn ship_part_name(&self, path: &NodePath) -> String {
        self.ship
            .model
            .as_ref()
            .and_then(|m| m.parts.iter().find(|p| p.node.as_ref() == Some(path)))
            .map(|p| super::file_name(&p.model).to_owned())
            .unwrap_or_default()
    }
}

/// What a ship part puts in the scene: its solid meshes, its ropes as the file has them, its sails at rest.
fn payloads(p: &ShipPart, rest: &Arc<Vec<[f32; 12]>>, colours: [[f32; 3]; 4]) -> Vec<Payload> {
    let mut out = vec![];
    if p.what.solid {
        out.push(Payload::MeshInstance(MeshInstance { model: p.model.clone(), lod: true, ..MeshInstance::default() }));
    }

    // The ropes read POSITION4.w as their burn.
    if p.what.ropes {
        out.push(Payload::MeshInstance(MeshInstance {
            model: format!("{}{MODEL_ROPES_SUFFIX}", p.model),
            lod: false,
            p4: [0.0; 4],
            ..MeshInstance::default()
        }));
    }

    if p.what.skinned {
        out.push(Payload::SkinnedInstance(SkinnedInstance {
            parts: vec![ShipModel::rest_part(p)],
            palette: rest.clone(),
            bones: rest.clone(),
            casts_shadow: true,
            faction_colours: colours,
            faction_scale: 0.0,
        }));
    }

    if p.what.cloth {
        let part = SkinnedPart {
            model: p.model.clone(),
            tint: [0.0; 4],
            decal_diffuse: None,
            decal_normal: None,
            masks: p.masks,
            bone: None,
            offset: SKINNED_NO_OFFSET,
        };
        out.push(Payload::SkinnedInstance(SkinnedInstance {
            parts: vec![part],
            palette: rest.clone(),
            bones: rest.clone(),
            casts_shadow: true,
            faction_colours: colours,
            faction_scale: 0.0,
        }));
    }

    out
}
