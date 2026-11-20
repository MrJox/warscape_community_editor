//! The decal rules in a tile (docs/plans/auto_detail_2.md, E): the tile's rules (`TileProject::decal_rules`, their own file beside
//! tile.wce) and the decals they lay, the editor's: made again **live** off the editor's thread when what they are made from changes (the
//! rules, the heights, the scene: its water, buildings and trees, the roads' layers, the erosion's runoff), drawn locked, and added at
//! export in a layer of their own ([`EXPORT_LAYER`]), as the biome areas' contents are. An edit of the rules is an undo entry
//! ([`DecalRulesCommand`]).

use std::any::Any;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use wce_editor_core::command::Command;
use wce_foundation::math::{Double3, Float4x4};
use wce_scene::render_scene::{Decal as DecalPayload, EntityId, Payload, RenderScene};
use wce_tile::decal_rules::{self, Decal, DecalRuleSet};
use wce_tile::ted_scene::TedScene;
use wce_tile::texture_rules::{self, Distances, Feature, Fields};
use wce_tile::world_transform::{WorldTransform, euler_matrix};

use super::draft::{turned, with_root_attr};
use super::{TERRAIN, Tile, TileDocument};

/// The layer an export puts the decals in.
pub const EXPORT_LAYER: &str = "WCE decals";

/// How deep a decal's box reaches (metres; the kit's are 10 of its units).
const DEPTH: f64 = 5.0;

/// A tree's trunk, as the distance to trees measures (metres).
const TRUNK: f64 = 0.5;

/// A decal laid: its element's text (as an export writes it: on the landscape) and how the viewport draws it (world metres, on the
/// ground, before its refit).
#[derive(Clone, Debug)]
pub(crate) struct DecalItem {
    rule: usize,
    pub(super) text: Vec<u8>,
    model: String,
    matrix: Float4x4,
}

/// The decals being made off the editor's thread, the signature of what they are made from, and the job's stop flag.
pub(crate) struct DecalJob {
    job: super::jobs::Job<Option<Vec<Decal>>>,
    signature: u64,
    stop: Arc<AtomicBool>,
}

impl Drop for DecalJob {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}

fn fnv(h: &mut u64, bytes: &[u8]) {
    for &b in bytes {
        *h = (*h ^ u64::from(b)).wrapping_mul(0x0100_0000_01b3);
    }
}

/// What the decals' job reads: the rules, the heights with what the rules measure to (taken on the editor's thread, owned), the map's
/// frame.
struct Input {
    rules: Vec<decal_rules::DecalRule>,
    fields: Fields,
    water: Option<wce_tile::roads::WaterMap>,
    buildings: Option<Vec<[f64; 3]>>,
    roads: Option<(Vec<bool>, u32, u32, (f64, f64))>,
    trees: Option<Vec<[f64; 3]>>,
    runoff: Option<Option<texture_rules::Runoff>>,
    grid: (u32, u32, (f64, f64)),
    border: f64,
    extent: (f64, f64),
}

impl Tile {
    /// The trees the rules measure to (x, z, the trunk's radius; world metres): the tile's own, the biome areas', the list shown.
    fn decal_trees(&self) -> Vec<[f64; 3]> {
        let sc = self.tscene();
        let mut out = self
            .instances()
            .iter()
            .filter(|&&e| sc.type_of(e) == "VEGETATION_INSTANCE")
            .map(|&e| {
                let p = self.world_position(e);
                [f64::from(p.x), f64::from(p.z), TRUNK]
            })
            .collect::<Vec<_>>();
        let unit = self.unit();
        out.extend(self.biome_tree_positions().into_iter().map(|[x, z]| [x / unit, z / unit, TRUNK]));
        if !self.trees.name.is_empty() {
            // The lists' z runs the other way.
            let extent = self.project.extent_z();
            out.extend(self.trees.trees.iter().map(|t| [f64::from(t.x), extent - f64::from(t.z), TRUNK]));
        }

        out
    }

    /// What the decals are made from, hashed: the rules, the heights, the scene (its water, buildings, trees), the roads' layers, the
    /// trees' list, the erosion's runoff.
    fn decal_signature(&mut self) -> Option<u64> {
        let set = self.project.decal_rules.clone()?;
        let mut h = 0xcbf2_9ce4_8422_2325u64;
        fnv(&mut h, &decal_rules::text(&set));
        let hf = self.project.height.as_ref()?;
        let version = hf.version();
        if self.texture_rules_heights.0 != version {
            self.texture_rules_heights = (version, hf.region_hash(0.0, 0.0, f64::from(hf.width()), f64::from(hf.height())));
        }

        fnv(&mut h, &self.texture_rules_heights.1.to_le_bytes());
        for v in [self.history_version, self.structure_version, self.models.len() as u64, self.biome_made] {
            fnv(&mut h, &v.to_le_bytes());
        }

        if decal_rules::uses_trees(&set.rules) {
            fnv(&mut h, self.trees.name.as_bytes());
            fnv(&mut h, &(Arc::as_ptr(&self.trees.trees) as usize as u64).to_le_bytes());
        }

        if decal_rules::uses_runoff(&set.rules) {
            fnv(&mut h, self.erosion_made.as_ref().map_or(&b"heights"[..], |m| m.owner.as_bytes()));
        }

        if decal_rules::uses(&set.rules, Feature::Road)
            && let Some(s) = self.project.layers.as_ref().and_then(|l| l.blend.as_ref())
        {
            for l in s.layers.iter().filter(|l| l.owner.starts_with("road:")) {
                fnv(&mut h, l.owner.as_bytes());
                fnv(&mut h, &[u8::from(l.visible)]);
            }
        }

        Some(h)
    }

    /// Lays the decals again when what they are made from changed and stayed so for a look; `wait`: now, waiting (an export).
    pub(super) fn refresh_decals(&mut self, wait: bool) {
        if self.stroke_open {
            return;
        }

        self.take_decals(wait);
        let Some(signature) = self.decal_signature() else {
            self.decal_job = None;
            if !self.decal_items.is_empty() {
                self.decal_items.clear();
                self.decal_items_shown = false;
            }

            self.decal_made = 0;
            return;
        };

        if signature == self.decal_made {
            return;
        }

        if signature != self.decal_seen && !wait {
            self.decal_seen = signature;
            return;
        }

        if self.decal_job.as_ref().is_some_and(|j| j.signature == signature) {
            if wait {
                self.take_decals(true);
            }

            return;
        }

        self.decal_seen = signature;
        if let Err(why) = self.start_decals(signature) {
            wce_foundation::log::warn(format!("decal rules: {why}"));
            self.decal_made = signature;
            return;
        }

        if wait {
            self.take_decals(true);
        }
    }

    /// What the job reads, taken now.
    fn decal_input(&mut self) -> Result<Input, String> {
        let set = self.project.decal_rules.clone().ok_or("no decal rules")?;
        let rules = set.rules;
        let unit = self.unit();
        let water = decal_rules::uses(&rules, Feature::Water).then(|| self.water_map());
        let buildings = decal_rules::uses(&rules, Feature::Building).then(|| self.texture_rules_buildings());
        let trees = decal_rules::uses_trees(&rules).then(|| self.decal_trees());
        let p = &mut self.project;
        let (border, ex, ez, mx, mz) = (p.border_m(), p.extent_x(), p.extent_z(), p.mpp_x(), p.mpp_z());
        let hf = p.height.as_ref().ok_or("the tile has no height map")?;
        let fields = Fields::from_height_field(hf, (mx, mz), unit);
        let grid = (hf.width(), hf.height(), (1.0 / mx, 1.0 / mz));
        let hf_size = (hf.width(), hf.height());
        let runoff = decal_rules::uses_runoff(&rules).then(|| {
            self.erosion_made
                .as_ref()
                .filter(|m| (m.w, m.h) == hf_size)
                .map(|m| texture_rules::Runoff { flow: m.flow.clone(), deposit: Some(m.deposit.clone()) })
        });

        // The roads' pixels of the blend map, when a rule measures to them and the tile has a blend map.
        let p = &mut self.project;
        let roads = if decal_rules::uses(&rules, Feature::Road) && p.load_blend().is_ok() {
            p.blend.as_ref().zip(p.layers.as_ref().and_then(|l| l.blend.as_ref())).map(|(bf, stack)| {
                let (bw, bh) = (bf.width(), bf.height());
                let ppm = (f64::from(bw) / (ex + 2.0 * border), f64::from(bh) / (ez + 2.0 * border));
                let empty = |px: &[u16]| px.iter().all(|&v| v == 0);
                let mask = (0..bh)
                    .flat_map(|y| (0..bw).map(move |x| (x, y)))
                    .map(|(x, y)| stack.owned_at("road:", x, y, empty))
                    .collect::<Vec<_>>();
                (mask, bw, bh, ppm)
            })
        } else {
            None
        };

        Ok(Input { rules, fields, water, buildings, roads, trees, runoff, grid, border, extent: (ex, ez) })
    }

    /// Starts laying the decals off the editor's thread.
    fn start_decals(&mut self, signature: u64) -> Result<(), String> {
        let input = self.decal_input()?;
        let stop = Arc::new(AtomicBool::new(false));
        let stopped = stop.clone();
        let job = super::jobs::Job::spawn("decal rules", move || {
            wce_foundation::wce_prof!("tile.decal_rules");
            let Input { rules, mut fields, water, buildings, roads, trees, runoff, grid: (w, h, ppm), border, extent } = input;
            if let Some(water) = &water {
                let mask = texture_rules::water_mask(water, &fields, w, h, ppm, border);
                fields = fields.with_distances(Feature::Water, Distances::from_mask(w, h, ppm, &mask));
            }

            if let Some(b) = &buildings {
                let mask = texture_rules::disc_mask(b, w, h, ppm, border);
                fields = fields.with_distances(Feature::Building, Distances::from_mask(w, h, ppm, &mask));
            }

            if let Some((mask, bw, bh, bppm)) = &roads {
                fields = fields.with_distances(Feature::Road, Distances::from_mask(*bw, *bh, *bppm, mask));
            }

            match runoff {
                Some(Some(r)) => fields = fields.with_runoff(r),
                Some(None) => fields = fields.with_runoff_of_heights(),
                None => {}
            }

            let trees = trees.map(|t| Distances::from_mask(w, h, ppm, &texture_rules::disc_mask(&t, w, h, ppm, border)));
            let rect = [border, border, border + extent.0, border + extent.1];
            let laid = decal_rules::lay(&rules, &fields, trees.as_ref(), rect, 0, &|| stopped.load(Ordering::Relaxed))?;
            // From the map's corner to the world's frame.
            Some(laid.into_iter().map(|d| Decal { x: d.x - border as f32, z: d.z - border as f32, ..d }).collect())
        });
        self.decal_job = Some(DecalJob { job, signature, stop });
        Ok(())
    }

    fn take_decals(&mut self, wait: bool) {
        let Some(j) = &self.decal_job else { return };
        let made = match if wait { j.job.wait() } else { j.job.poll() } {
            super::jobs::Poll::Running => return,
            super::jobs::Poll::Done(m) => m,
            super::jobs::Poll::Failed => None,
        };

        let j = self.decal_job.take().expect("a job");
        let Some(decals) = made else {
            if !j.stop.load(Ordering::Relaxed) {
                wce_foundation::log::warn("decal rules: the decals could not be laid");
            }
            return;
        };

        self.decal_items = decals.iter().filter_map(|d| self.decal_item(d)).collect();
        self.decal_made = j.signature;
        self.decal_items_shown = false;
    }

    /// A decal as the viewport draws it and an export writes it (world metres; refitted to the ground).
    pub(super) fn decal_item(&self, d: &Decal) -> Option<DecalItem> {
        let unit = self.unit();
        let (x, z) = (f64::from(d.x), f64::from(d.z));
        let y = self.project.height_at(x, z) / unit;
        let text = decal_text(d, unit)?;
        let world = WorldTransform {
            position: Double3::new(x * unit, y * unit, z * unit),
            rotation: euler_matrix(Double3::new(f64::from(d.yaw), 0.0, 0.0)),
        };
        let scale = Double3::new(f64::from(d.across), DEPTH, -f64::from(d.length));
        let matrix = self.refit_decal(super::index::matrix_of(&world, unit, Some(scale)));
        Some(DecalItem { rule: d.rule, text, model: d.key.replace('\\', "/"), matrix })
    }

    /// Decals in the viewport, locked: their entities.
    pub(super) fn show_decals(&self, scene: &mut RenderScene, items: &[DecalItem]) -> Vec<EntityId> {
        let source = self.source(TERRAIN);
        let mut ids: Vec<EntityId> = Vec::with_capacity(items.len());
        for item in items {
            let payload = Payload::Decal(DecalPayload { model: item.model.clone(), tiled: false, opacity: 1.0, parallax: 0.1 });
            let id = scene.add(self.layer, item.matrix, payload, source);
            if let Some(en) = scene.find_mut(id) {
                en.locked = true;
            }

            ids.push(id);
        }

        ids
    }

    /// The decals in the viewport, again when they were laid again (locked: they are the rules', not tile.ted's).
    pub(super) fn show_decal_items(&mut self, scene: &mut RenderScene) {
        if self.decal_items_shown {
            return;
        }

        self.decal_items_shown = true;
        for id in std::mem::take(&mut self.decal_entities) {
            scene.remove(id);
        }

        self.decal_entities = self.show_decals(scene, &self.decal_items);
    }

    /// The layer of the decals an export adds, as TEd and the game read it.
    pub(super) fn with_decal_items(&self, text: Vec<u8>) -> Result<Vec<u8>, String> {
        if self.decal_items.is_empty() {
            return Ok(text);
        }

        let mut scene = TedScene::parse_any(text)?;
        let layer = scene.add_layer(scene.scene_graph(), EXPORT_LAYER)?;
        for item in &self.decal_items {
            scene.insert_element_text(layer, &item.text)?;
        }

        Ok(scene.save())
    }
}

/// A decal's element text: on the landscape (its height the ground's), its size in the scene's units, turned to its way.
fn decal_text(d: &Decal, unit: f64) -> Option<Vec<u8>> {
    let at = Double3::new(f64::from(d.x) * unit, 0.0, f64::from(d.z) * unit);
    let n = wce_tile::new_instances::NewInstance::new("ProjectedDecalInstance").position(at).attr("key", &d.key);
    let text = wce_tile::new_instances::element_text(&n).ok()?;
    let num = |v: f64| wce_tile::ted_scene::format_number(v);
    let scale = format!("vector({},{},{})", num(f64::from(d.across) * unit), num(DEPTH * unit), num(-f64::from(d.length) * unit));
    let text = with_root_attr(&text, "scale", &scale);
    let text = with_root_attr(&text, "surface_mode", "MSM_LANDSCAPE");
    Some(turned(&text, d.yaw.to_degrees()))
}

impl TileDocument {
    /// The tile's decal rules, if it has any.
    #[must_use]
    pub fn decal_rules(&self) -> Option<&DecalRuleSet> {
        self.project().decal_rules.as_ref()
    }

    /// Sets the tile's decal rules (`None`: takes them and their decals away). One undo entry; the decals follow live.
    pub fn set_decal_rules(&mut self, rules: Option<DecalRuleSet>) {
        if self.decal_rules() == rules.as_ref() {
            return;
        }

        self.end_stroke();
        let before = self.decal_rules().cloned();
        let name = match &rules {
            None => "Remove decal rules".to_owned(),
            Some(_) if before.is_none() => "Add decal rules".to_owned(),
            Some(_) => "Edit decal rules".to_owned(),
        };
        self.history.push(Box::new(DecalRulesCommand { name, before, after: rules }));
    }

    /// The decals laid now, waiting for those being laid (an export; scripts).
    pub fn finish_decals(&mut self) {
        let t = self.tile_mut();
        t.sync_terrain_layers();
        t.refresh_decals(true);
    }

    /// Whether the decals are being laid.
    #[must_use]
    pub fn decals_busy(&self) -> bool {
        self.tile().decal_job.is_some()
    }

    /// How many decals each rule laid, by the rules' order.
    #[must_use]
    pub fn decal_counts(&self) -> Vec<usize> {
        let t = self.tile();
        let n = t.project.decal_rules.as_ref().map_or(0, |s| s.rules.len());
        let mut out = vec![0; n];
        for i in &t.decal_items {
            if let Some(c) = out.get_mut(i.rule) {
                *c += 1;
            }
        }

        out
    }

    /// The decals' models the rules name that the game's files lack, by rule (its name and the decal's name).
    #[must_use]
    pub fn decal_rules_missing(&self) -> Vec<(String, String)> {
        let Some(set) = self.decal_rules() else { return vec![] };
        let Some(files) = self.tile().files.as_deref() else { return vec![] };
        let mut out = vec![];
        for r in &set.rules {
            for (k, _) in &r.keys {
                let path = decal_rules::key_of(k).replace('\\', "/").to_lowercase();
                if !files.exists(&path) {
                    out.push((r.name.clone(), k.clone()));
                }
            }
        }

        out
    }
}

/// An edit of the tile's decal rules.
pub(crate) struct DecalRulesCommand {
    name: String,
    before: Option<DecalRuleSet>,
    after: Option<DecalRuleSet>,
}

impl Command<Tile> for DecalRulesCommand {
    fn name(&self) -> String {
        self.name.clone()
    }

    fn apply(&mut self, t: &mut Tile) {
        t.project.decal_rules.clone_from(&self.after);
        t.project.decal_rules_dirty = true;
    }

    fn revert(&mut self, t: &mut Tile) {
        t.project.decal_rules.clone_from(&self.before);
        t.project.decal_rules_dirty = true;
    }

    fn dirty_items(&self) -> Vec<u64> {
        vec![TERRAIN]
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}
