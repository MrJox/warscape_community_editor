//! The texture rules in a tile (docs/plans/auto_detail.md, A): the tile's rules (`TileProject::texture_rules`, their own file beside
//! tile.wce) and the blend layer they make, owned by them (`texture_rules::OWNER`), under the user's layers so that hand paint goes over
//! it.
//!
//! The layer is made again **live**, as the roads' layers are (`roads.rs`): when what it is made from (the rules, the tile's texture
//! groups, the heights as they show) is not what its owner's signature says, and that stayed so for a look (a stroke or a drag waits for
//! its end), a job off the editor's thread makes it over a copy of the blend stack; the result goes in when the stack is still as it was
//! taken, as no undo entry of its own: an undo puts the heights or the rules back, and the layer is made again from them. An edit of the
//! rules is an undo entry ([`TextureRulesCommand`]).

use std::any::Any;
use std::sync::Arc;

use wce_editor_core::command::Command;
use wce_tile::roads::WaterMap;
use wce_tile::terrain_layers::{Blend, Ground as TerrainGround, Stack, TerrainLayers};
use wce_tile::texture_rules::{self, Distances, Feature, Fields, OWNER, RuleSet};

use super::{TERRAIN, Tile, TileDocument};

/// A job making the rules' layer, and what it was started from.
pub(crate) struct TextureRulesJob {
    job: super::jobs::Job<Made>,
    signature: u64,
    key: u64,
    before: Stack<Blend>,
    before_ground: Option<Stack<TerrainGround>>,
}

/// What a job made: the stacks with the rules' layers, the pixels of each to compose again (None: wherever the rules' layers were or
/// are; a remake of a part: that part, or nothing) and the fields it was made from.
struct Made {
    blend: Stack<Blend>,
    ground: Option<Stack<TerrainGround>>,
    rect: Option<Option<[u32; 4]>>,
    ground_rect: Option<Option<[u32; 4]>>,
    fields: Arc<Fields>,
}

/// What the rules' layer in the stack was made from: the rules' own key ([`rules_key`]: their text, the texture groups, the grids), the
/// layer's owner and the fields. A remake under the same key remakes only where the fields changed (`Fields::changed_area`), widened by
/// the rules' reach: an edit of the heights, a building moved, an area drawn remake that much of the map, not all of it.
pub(crate) struct RulesBasis {
    key: u64,
    owner: String,
    fields: Arc<Fields>,

    /// What the remake that made it covered: None, the whole map; a part, its blend map pixels (none: nothing changed).
    part: Option<Option<[u32; 4]>>,
}

/// What a remake of the rules' layer covered.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Remade {
    Whole,

    /// Nothing the rules read changed where they lay.
    Nothing,

    /// The blend map's pixels `[x0, y0, x1, y1]` (end exclusive).
    Part([u32; 4]),
}

/// The rules' own key: their text (the areas' bindings with it), the texture groups, the blend map's and the ground map's sizes.
fn rules_key(rules: &[texture_rules::Rule], groups: &[String], size: (u32, u32), ground: Option<(u32, u32)>) -> u64 {
    let mut h = 0xcbf2_9ce4_8422_2325u64;
    let text = texture_rules::text(&RuleSet { preset: String::new(), rules: rules.to_vec() });
    let area = rules.iter().flat_map(|r| r.area.map_or(u64::MAX, |a| a as u64).to_le_bytes());
    let sizes = [size.0, size.1, ground.map_or(0, |g| g.0), ground.map_or(0, |g| g.1)].into_iter().flat_map(u32::to_le_bytes);
    for b in text.iter().copied().chain(groups.iter().flat_map(|g| g.bytes().chain(*b"/"))).chain(area).chain(sizes) {
        h = (h ^ u64::from(b)).wrapping_mul(0x0100_0000_01b3);
    }

    h
}

/// Metres from the map's corner as a grid's pixels (`ppm` a metre, `w` x `h`), the pixels they touch.
fn pixel_rect(m: [f64; 4], ppm: (f64, f64), w: u32, h: u32) -> [u32; 4] {
    let px = |v: f64, s: f64, n: u32| ((v * s).max(0.0) as u32).min(n);
    [px(m[0], ppm.0, w), px(m[1], ppm.1, h), px(m[2] * ppm.0 + 0.999, 1.0, w), px(m[3] * ppm.1 + 0.999, 1.0, h)]
}

/// What the distance conditions measure to, taken from the scene (`Tile::texture_rules_inputs`), what the rules use of it only: the water
/// and the real buildings (x, z, radius in world metres), and their hash. The roads are their own layers' (taken in the job).
#[derive(Clone, Default)]
pub(crate) struct Inputs {
    water: Option<WaterMap>,
    buildings: Option<Vec<[f64; 3]>>,
    hash: u64,
}

/// The categories of the real buildings the rules measure to (the ground coverage's: not rocks, props or ground pieces).
const BUILDING_CATEGORIES: [&str; 6] = wce_tile::ground_coverage::AVOIDED_CATEGORIES;

fn fnv(h: &mut u64, bytes: &[u8]) {
    for &b in bytes {
        *h = (*h ^ u64::from(b)).wrapping_mul(0x0100_0000_01b3);
    }
}

/// The owner of the layer made from what `signature` says.
fn owner_of(signature: u64) -> String {
    format!("{OWNER}#{signature:016x}")
}

fn signature_of(owner: &str) -> Option<u64> {
    u64::from_str_radix(owner.strip_prefix(OWNER)?.strip_prefix('#')?, 16).ok()
}

impl Tile {
    /// The tile's texture group slot names (red, green, blue, alpha), empty without an entry.
    fn texture_group_slots(&self) -> Vec<String> {
        self.project.entry.as_ref().map(|e| e.texture_group()).map(|g| vec![g.red, g.green, g.blue, g.alpha]).unwrap_or_default()
    }

    /// What the distance conditions need of the scene, worked out again only when the scene or its models changed.
    /// Every rule the layer lays: the tile's, then each biome area's biome's bound to it (docs/plans/auto_detail.md, C), with the areas'
    /// masks (metres from the map's corner).
    pub fn all_texture_rules(&self) -> (Vec<texture_rules::Rule>, Vec<texture_rules::AreaMask>) {
        let mut rules = self.project.texture_rules.as_ref().map(|r| r.rules.clone()).unwrap_or_default();
        let border = self.project.border_m() as f32;
        let mut masks = vec![];
        for a in self.biome_areas() {
            let Some(b) = self.biome_named(&a.biome).filter(|b| !b.rules.is_empty()) else { continue };
            let i = masks.len();
            masks.push(texture_rules::AreaMask { points: a.points.iter().map(|&(x, z)| (x + border, z + border)).collect(), edge: b.edge });
            rules.extend(b.rules.into_iter().map(|r| texture_rules::Rule { area: Some(i), seed: r.seed ^ (a.seed as u32), ..r }));
        }

        // The field areas' ground.
        let (field_rules, field_masks) = self.field_texture_rules(masks.len());
        rules.extend(field_rules);
        masks.extend(field_masks);
        (rules, masks)
    }

    fn texture_rules_inputs(&mut self) -> Inputs {
        let rules = self.all_texture_rules().0;
        let key = {
            let mut h = 0xcbf2_9ce4_8422_2325u64;
            for v in [self.history_version, self.structure_version, self.models.len() as u64, self.unresolved.len() as u64] {
                fnv(&mut h, &v.to_le_bytes());
            }

            for f in Feature::ALL {
                fnv(&mut h, &[u8::from(f.used(&rules))]);
            }

            h
        };
        if let Some((k, inputs)) = &self.texture_rules_inputs
            && *k == key
        {
            return inputs.clone();
        }

        let mut inputs = Inputs { hash: 0xcbf2_9ce4_8422_2325, ..Inputs::default() };
        if Feature::Water.used(&rules) {
            let w = self.water_map();
            fnv(&mut inputs.hash, &[u8::from(w.sea)]);
            for v in
                w.rivers.iter().flatten().flatten().chain(w.planes.iter().flat_map(|(c, y)| c.iter().flatten().chain(std::iter::once(y))))
            {
                fnv(&mut inputs.hash, &v.to_le_bytes());
            }

            inputs.water = Some(w);
        }

        if Feature::Building.used(&rules) {
            let b = self.rule_buildings();
            for v in b.iter().flatten() {
                fnv(&mut inputs.hash, &v.to_le_bytes());
            }

            inputs.buildings = Some(b);
        }

        self.texture_rules_inputs = Some((key, inputs.clone()));
        inputs
    }

    /// The real buildings (`BUILDING_CATEGORIES`, by the kits' tables) outside the ground coverage's layer: x, z and the radius of their
    /// model across (0 until it loads), world metres.
    fn rule_buildings(&self) -> Vec<[f64; 3]> {
        self.texture_rules_buildings()
    }

    /// A building's radius as the rules' distances take it (its model's reach across, scaled; metres), 0 until its model loads.
    pub(super) fn texture_rules_buildings_radius(&self, e: usize) -> f64 {
        let sc = self.tscene();
        self.model_of(e).filter(|m| m.ok && m.resolved).map_or(0.0, |m| {
            let s = sc.scale(e).map_or(1.0, |s| s.x.abs().max(s.z.abs()));
            let across = [m.bmin.x.abs(), m.bmax.x.abs(), m.bmin.z.abs(), m.bmax.z.abs()].into_iter().fold(0.0f32, f32::max);
            f64::from(across) * s / self.unit()
        })
    }

    /// The real buildings (as the rules' distances take them): x, z, radius, world metres.
    pub(super) fn texture_rules_buildings(&self) -> Vec<[f64; 3]> {
        let sc = self.tscene();
        let coverage = sc.find_all("LayerInstance").into_iter().find(|&l| sc.name(l) == super::ground_coverage::COVERAGE_LAYER);
        let kit = self.building_db.get_or_init(|| wce_tile::building_db::BuildingDb::load_near(&self.project.dir));
        self.instances()
            .iter()
            .copied()
            .filter(|&e| sc.type_of(e) == "BUILDING_INSTANCE" && coverage.is_none_or(|l| !sc.is_descendant(e, l)))
            .filter(|&e| sc.doc().get(e, "key").is_some_and(|k| kit.category(&k).is_some_and(|c| BUILDING_CATEGORIES.contains(&c))))
            .map(|e| {
                let p = self.world_position(e);
                let radius = self.model_of(e).filter(|m| m.ok && m.resolved).map_or(0.0, |m| {
                    let s = sc.scale(e).map_or(1.0, |s| s.x.abs().max(s.z.abs()));
                    let across = [m.bmin.x.abs(), m.bmax.x.abs(), m.bmin.z.abs(), m.bmax.z.abs()].into_iter().fold(0.0f32, f32::max);
                    f64::from(across) * s / self.unit()
                });
                [f64::from(p.x), f64::from(p.z), radius]
            })
            .collect()
    }

    /// What the rules' layer is made from, hashed: the rules' text, the texture groups, the heights as they show, the blend map's base
    /// (its main ground), what the distances measure to (the water, the buildings, the roads' layers).
    fn texture_rules_signature(&mut self) -> Option<u64> {
        let inputs = self.texture_rules_inputs();
        let (rules, masks) = self.all_texture_rules();
        if rules.is_empty() {
            return None;
        }

        let hf = self.project.height.as_ref()?;
        let version = hf.version();
        if self.texture_rules_heights.0 != version {
            self.texture_rules_heights = (version, hf.region_hash(0.0, 0.0, f64::from(hf.width()), f64::from(hf.height())));
        }

        let mut h = 0xcbf2_9ce4_8422_2325u64;
        let mut eat = |bytes: &[u8]| {
            for &b in bytes {
                h = (h ^ u64::from(b)).wrapping_mul(0x0100_0000_01b3);
            }
        };
        eat(&texture_rules::text(&RuleSet { preset: String::new(), rules: rules.clone() }));
        for m in &masks {
            eat(&m.edge.to_le_bytes());
            for &(x, z) in &m.points {
                eat(&x.to_le_bytes());
                eat(&z.to_le_bytes());
            }
        }

        for r in &rules {
            eat(&r.area.map_or(u64::MAX, |a| a as u64).to_le_bytes());
        }
        for g in self.texture_group_slots() {
            eat(g.as_bytes());
            eat(b"/");
        }

        eat(&self.texture_rules_heights.1.to_le_bytes());
        eat(&self.project.unit_scale.to_le_bytes());
        eat(&inputs.hash.to_le_bytes());
        if let Some(s) = self.project.layers.as_ref().and_then(|l| l.blend.as_ref()) {
            // The rules lay their textures over the base's main ground only.
            eat(&s.base_identity().to_le_bytes());
        }

        if texture_rules::uses_runoff(&rules) {
            // Where water runs: the erosion's when it made it (it comes after its layer, on a tile opened again), else the heights'.
            eat(self.erosion_made.as_ref().map_or(&b"heights"[..], |m| m.owner.as_bytes()));
        }

        if Feature::Road.used(&rules)
            && let Some(s) = self.project.layers.as_ref().and_then(|l| l.blend.as_ref())
        {
            // A road's layer is named by what it was made from (`roads.rs`): its owner says when it changed.
            for l in s.layers.iter().filter(|l| l.owner.starts_with("road:")) {
                eat(l.owner.as_bytes());
                eat(&[u8::from(l.visible)]);
            }
        }

        Some(h)
    }

    /// Makes the rules' layer again when it is stale, or takes it away with the rules. `sync_terrain_layers` has run.
    pub(super) fn refresh_texture_rules(&mut self) {
        self.refresh_texture_rules_with(false);
    }

    /// [`Self::refresh_texture_rules`]; `wait`: what is being made is waited for, and what is to be made is made now (an export).
    pub(super) fn refresh_texture_rules_with(&mut self, wait: bool) {
        if self.stroke_open {
            return;
        }

        self.take_texture_rules(wait);
        let made = self
            .project
            .layers
            .as_ref()
            .and_then(|l| l.blend.as_ref())
            .and_then(|s| s.layers.iter().find(|l| l.owner.starts_with(OWNER)))
            .map(|l| signature_of(&l.owner));
        if self.project.texture_rules.is_none() && self.all_texture_rules().0.is_empty() {
            self.texture_rules_job = None;
            if made.is_some() {
                self.remove_texture_rules_layer();
            }

            return;
        }

        let Some(signature) = self.texture_rules_signature() else { return };
        if made == Some(Some(signature)) {
            self.texture_rules_seen = signature;
            return;
        }

        // Still changing (a drag, an undo after another): the next look.
        if signature != self.texture_rules_seen && !wait {
            self.texture_rules_seen = signature;
            return;
        }

        if self.texture_rules_job.as_ref().is_some_and(|j| j.signature == signature) {
            if wait {
                self.take_texture_rules(true);
            }

            return;
        }

        self.texture_rules_seen = signature;
        if let Err(why) = self.start_texture_rules(signature) {
            wce_foundation::log::warn(format!("texture rules: {why}"));
            return;
        }

        if wait {
            self.take_texture_rules(true);
        }
    }

    /// Starts making the rules' layer off the editor's thread, on a copy of the blend stack (its samples shared, cheap) and of the heights.
    /// What working out the rules needs, taken now (owned copies): see [`Prepared`].
    fn prepare_texture_rules(&mut self, rules: Vec<texture_rules::Rule>) -> Result<Prepared, String> {
        let groups = self.texture_group_slots();
        let inputs = self.texture_rules_inputs();
        let p = &mut self.project;
        p.load_blend()?;
        let (border, ex, ez, mx, mz, unit) = (p.border_m(), p.extent_x(), p.extent_z(), p.mpp_x(), p.mpp_z(), p.unit_scale);
        let (Some(hf), Some(bf)) = (p.height.as_ref(), p.blend.as_ref()) else { return Err("the tile has no height map".to_owned()) };
        let fields = Fields::from_height_field(hf, (mx, mz), unit);
        let hf_size = (hf.width(), hf.height());
        let (bw, bh) = (bf.width(), bf.height());
        let ppm = (f64::from(bw) / (ex + 2.0 * border), f64::from(bh) / (ez + 2.0 * border));
        let layers = p.layers.get_or_insert_with(TerrainLayers::default);
        let blend = layers.blend.get_or_insert_with(|| Stack::new(bf)).clone();

        // The ground type map when a rule lays a ground type, or the rules' ground layer is to go.
        let had_ground = layers.ground.as_ref().is_some_and(|s| s.layers.iter().any(|l| l.owner.starts_with(OWNER)));
        // A tile without a ground type map still gets its textures.
        let ground = if (texture_rules::lays_ground(&rules) || had_ground)
            && let Ok(gf) = p.load_ground()
        {
            let (gw, gh) = (gf.width(), gf.height());
            let gppm = (f64::from(gw) / (ex + 2.0 * border), f64::from(gh) / (ez + 2.0 * border));
            let layers = p.layers.get_or_insert_with(TerrainLayers::default);
            let stack = match layers.ground.as_ref() {
                Some(s) => s.clone(),
                None => Stack::new(p.ground.as_ref().expect("loaded above")),
            };
            Some((stack, (gw, gh), gppm))
        } else {
            None
        };

        let runoff = texture_rules::uses_runoff(&rules).then(|| {
            self.erosion_made
                .as_ref()
                .filter(|m| (m.w, m.h) == hf_size)
                .map(|m| texture_rules::Runoff { flow: m.flow.clone(), deposit: Some(m.deposit.clone()) })
        });
        Ok(Prepared { rules, groups, inputs, fields, blend, ground, size: (bw, bh), ppm, border, mpp: (mx, mz), runoff })
    }

    /// Starts making the rules' layer off the editor's thread, on a copy of the blend stack (its samples shared, cheap) and of the heights.
    fn start_texture_rules(&mut self, signature: u64) -> Result<(), String> {
        let (rules, masks) = self.all_texture_rules();
        let mut prepared = self.prepare_texture_rules(rules)?;
        prepared.fields = prepared.fields.with_areas(masks);
        let key = rules_key(&prepared.rules, &prepared.groups, prepared.size, prepared.ground.as_ref().map(|g| g.1));

        // The fields the layer in the stack was made from, when it is still that layer under the same rules: only what changed is made.
        let blend_owner = prepared.blend.layers.iter().find(|l| l.owner.starts_with(OWNER)).map(|l| l.owner.clone());
        let ground_owner =
            prepared.ground.as_ref().and_then(|g| g.0.layers.iter().find(|l| l.owner.starts_with(OWNER)).map(|l| l.owner.clone()));
        let lays_ground = texture_rules::lays_ground(&prepared.rules);
        let old = self
            .texture_rules_basis
            .as_ref()
            .filter(|b| b.key == key && blend_owner.as_ref() == Some(&b.owner))
            .filter(|b| if lays_ground { ground_owner.as_ref() == Some(&b.owner) } else { ground_owner.is_none() })
            .map(|b| b.fields.clone());

        let id = self.project.layers.get_or_insert_with(TerrainLayers::default).new_id();
        let ground_id = self.project.layers.get_or_insert_with(TerrainLayers::default).new_id();
        let before = prepared.blend.clone();
        let before_ground = prepared.ground.as_ref().map(|g| g.0.clone());
        let job = super::jobs::Job::spawn("texture rules", move || {
            wce_foundation::wce_prof!("tile.texture_rules");
            let (fields, mut blend) = prepared.fields();
            let Prepared { rules, groups, ground, size: (bw, bh), ppm, mpp, .. } = prepared;
            let owner = owner_of(signature);

            // A part: where the fields changed, the rules' reach around a changed height.
            let part = old.map(|o| fields.changed_area(&o, texture_rules::reach(&rules, mpp)));
            let (rect, ground) = match part {
                Some(changed) => {
                    let rect = changed.map(|m| pixel_rect(m, ppm, bw, bh));
                    if let Some(l) = blend.layers.iter_mut().find(|l| l.owner.starts_with(OWNER)) {
                        l.owner.clone_from(&owner);
                        let id = l.id;
                        if let Some(r) = rect {
                            for y in r[1]..r[3] {
                                for x in r[0]..r[2] {
                                    blend.put(id, x, y, &[0; 16]);
                                }
                            }

                            texture_rules::generate(&rules, &groups, &fields, ppm, r, &mut |x, y, px| blend.put(id, x, y, &px));
                        }
                    }

                    let ground = ground.map(|(mut stack, (gw, gh), gppm)| {
                        let rect = changed.map(|m| pixel_rect(m, gppm, gw, gh));
                        if let Some(l) = stack.layers.iter_mut().find(|l| l.owner.starts_with(OWNER)) {
                            l.owner.clone_from(&owner);
                            let id = l.id;
                            if let Some(r) = rect {
                                for y in r[1]..r[3] {
                                    for x in r[0]..r[2] {
                                        stack.put(id, x, y, &[0; 4]);
                                    }
                                }

                                texture_rules::generate_ground(&rules, &groups, &fields, gppm, r, &mut |x, y, px| stack.put(id, x, y, &px));
                            }
                        }

                        (stack, Some(rect))
                    });
                    (Some(rect), ground)
                }

                None => {
                    blend.layers.retain(|l| !l.owner.starts_with(OWNER));
                    let id = blend.add(id, "Texture rules", &owner);
                    texture_rules::generate(&rules, &groups, &fields, ppm, [0, 0, bw, bh], &mut |x, y, px| blend.put(id, x, y, &px));
                    let ground = ground.map(|(mut stack, (gw, gh), gppm)| {
                        stack.layers.retain(|l| !l.owner.starts_with(OWNER));
                        if texture_rules::lays_ground(&rules) {
                            let id = stack.add(ground_id, "Texture rules", &owner);
                            texture_rules::generate_ground(&rules, &groups, &fields, gppm, [0, 0, gw, gh], &mut |x, y, px| {
                                stack.put(id, x, y, &px)
                            });
                        }

                        (stack, None)
                    });
                    (None, ground)
                }
            };

            let (ground, ground_rect) = match ground {
                Some((s, r)) => (Some(s), r),
                None => (None, None),
            };
            Made { blend, ground, rect, ground_rect, fields: Arc::new(fields) }
        });
        self.texture_rules_job = Some(TextureRulesJob { job, signature, key, before, before_ground });
        Ok(())
    }

    /// The rules' layer made off the editor's thread, put in place of the stack it was made from, and the map composed again over where
    /// it was and is: when the stack is still as it was taken. Edited meanwhile, the job is let go and the layer is made again.
    fn take_texture_rules(&mut self, wait: bool) {
        let Some(j) = &self.texture_rules_job else { return };
        let made = match if wait { j.job.wait() } else { j.job.poll() } {
            super::jobs::Poll::Running => return,
            super::jobs::Poll::Done(m) => Some(m),
            super::jobs::Poll::Failed => None,
        };

        let j = self.texture_rules_job.take().expect("a job");
        let Some(Made { blend, ground, rect, ground_rect, fields }) = made else {
            wce_foundation::log::warn("texture rules: their layer could not be made");
            return;
        };

        let p = &mut self.project;
        let (Some(bf), Some(layers)) = (p.blend.as_mut(), p.layers.as_mut()) else { return };
        let ground_same = match (&j.before_ground, layers.ground.as_ref()) {
            (Some(b), Some(now)) => now.unchanged_since(b),
            (Some(_), None) => true,
            (None, _) => true,
        };
        if !layers.blend.as_ref().is_some_and(|b| b.unchanged_since(&j.before)) || !ground_same {
            return;
        }

        if let (Some(ground), Some(gf)) = (ground, p.ground.as_mut()) {
            let rect = ground_rect.unwrap_or_else(|| rules_rect(j.before_ground.as_ref().expect("taken with it"), &ground));
            layers.ground = Some(ground);
            if let Some(s) = layers.ground.as_mut() {
                if let Some(r) = rect {
                    s.apply(gf, r);
                }

                s.remember();
            }

            p.ground_dirty |= rect.is_some();
        }

        let made_part = rect;
        let rect = rect.unwrap_or_else(|| rules_rect(&j.before, &blend));
        layers.blend = Some(blend);
        layers.dirty = true;
        if let Some(s) = layers.blend.as_mut() {
            if let Some(r) = rect {
                s.apply(bf, r);
            }

            s.remember();
        }

        // The game and the compile read the map, not the layers.
        p.blend_dirty |= rect.is_some();
        self.texture_rules_basis = Some(RulesBasis { key: j.key, owner: owner_of(j.signature), fields, part: made_part });
        self.layers_shown = true;
        if rect.is_some() {
            self.reload_grass();
        }
    }

    /// Takes the rules' layer away (the rules went).
    fn remove_texture_rules_layer(&mut self) {
        self.texture_rules_basis = None;
        let p = &mut self.project;
        let (Some(bf), Some(layers)) = (p.blend.as_mut(), p.layers.as_mut()) else { return };
        if let (Some(s), Some(gf)) = (layers.ground.as_mut(), p.ground.as_mut()) {
            let before = s.clone();
            s.layers.retain(|l| !l.owner.starts_with(OWNER));
            if let Some(r) = rules_rect(&before, s) {
                s.apply(gf, r);
                p.ground_dirty = true;
            }

            s.remember();
        }

        let Some(s) = layers.blend.as_mut() else { return };
        let before = s.clone();
        s.layers.retain(|l| !l.owner.starts_with(OWNER));
        if let Some(r) = rules_rect(&before, s) {
            s.apply(bf, r);
            p.blend_dirty = true;
        }

        s.remember();
        layers.dirty = true;
        self.layers_shown = true;
        self.reload_grass();
    }
}

/// What working out the rules needs of a tile, taken on the editor's thread: the rules, the texture group slots, the scene's part of the
/// distances, the heights, a copy of the blend stack (the roads' layers, and what the job makes the rules' layer in), the blend map's size,
/// its pixels a metre and the border (metres from the map's corner to the tile's origin).
struct Prepared {
    rules: Vec<texture_rules::Rule>,
    groups: Vec<String>,
    inputs: Inputs,
    fields: Fields,
    blend: Stack<Blend>,

    /// A copy of the ground type stack, its map's size and pixels a metre: when a rule lays a ground type, or the rules' ground layer
    /// is to go.
    ground: Option<(Stack<TerrainGround>, (u32, u32), (f64, f64))>,
    size: (u32, u32),
    ppm: (f64, f64),
    border: f64,

    /// The heights' metres a sample.
    mpp: (f64, f64),

    /// When a rule reads where water runs: the erosion's runoff (None inside: worked out from the heights).
    runoff: Option<Option<texture_rules::Runoff>>,
}

impl Prepared {
    /// The heights with the distances the rules use worked out (on the blend map's grid), and the blend stack. Off the editor's thread.
    fn fields(&self) -> (Fields, Stack<Blend>) {
        let (bw, bh) = self.size;
        let (ppm, border) = (self.ppm, self.border);
        let mut fields = self.fields.clone();
        match &self.runoff {
            Some(Some(r)) => fields = fields.with_runoff(r.clone()),
            Some(None) => fields = fields.with_runoff_of_heights(),
            None => {}
        }

        // Only over the tile's main ground: what its own painting holds in other textures stays (a kit tile's artist's work).
        let base = &self.blend;
        let (_, share) = texture_rules::main_ground(bw, bh, |x, y| {
            let mut px = [0u16; 16];
            base.base_at(x, y, &mut px);
            px
        });
        fields = fields.with_main_ground(bw, bh, ppm, share);
        if let Some(water) = &self.inputs.water {
            let mask = texture_rules::water_mask(water, &fields, bw, bh, ppm, border);
            fields = fields.with_distances(Feature::Water, Distances::from_mask(bw, bh, ppm, &mask));
        }

        if let Some(buildings) = &self.inputs.buildings {
            let mask = texture_rules::disc_mask(buildings, bw, bh, ppm, border);
            fields = fields.with_distances(Feature::Building, Distances::from_mask(bw, bh, ppm, &mask));
        }

        if Feature::Road.used(&self.rules) {
            let empty = |p: &[u16]| p.iter().all(|&v| v == 0);
            let blend = &self.blend;
            let mask =
                (0..bh).flat_map(|y| (0..bw).map(move |x| (x, y))).map(|(x, y)| blend.owned_at("road:", x, y, empty)).collect::<Vec<_>>();
            fields = fields.with_distances(Feature::Road, Distances::from_mask(bw, bh, ppm, &mask));
        }

        (fields, self.blend.clone())
    }
}

/// Where a rule lays its texture, on a grid over the tile without its border (`RuleArea::cells`: row by row along +z).
pub struct RuleArea {
    /// The world point of the first cell's centre, the cells' size (metres) and how many there are across (x) and down (z).
    pub x0: f32,
    pub z0: f32,
    pub cell: f32,
    pub nx: usize,
    pub nz: usize,

    /// The rule's weight at each cell's centre, 0 .. 1.
    pub weights: Vec<f32>,
}

/// Where the rules' layers of two stacks hold samples.
fn rules_rect<K: wce_tile::terrain_layers::Kind>(old: &Stack<K>, new: &Stack<K>) -> Option<[u32; 4]> {
    old.layers
        .iter()
        .chain(&new.layers)
        .filter(|l| l.owner.starts_with(OWNER))
        .filter_map(|l| l.extent())
        .reduce(|a, b| [a[0].min(b[0]), a[1].min(b[1]), a[2].max(b[2]), a[3].max(b[3])])
}

impl TileDocument {
    /// What the last remake of the rules' layer covered; None before any.
    #[must_use]
    pub fn texture_rules_last_part(&self) -> Option<Remade> {
        self.tile().texture_rules_basis.as_ref().map(|b| match b.part {
            None => Remade::Whole,
            Some(None) => Remade::Nothing,
            Some(Some(r)) => Remade::Part(r),
        })
    }

    /// The tile's texture rules, if it has any.
    #[must_use]
    pub fn texture_rules(&self) -> Option<&RuleSet> {
        self.project().texture_rules.as_ref()
    }

    /// Sets the tile's texture rules (`None`: takes them and their layer away). One undo entry; the layer follows live.
    pub fn set_texture_rules(&mut self, rules: Option<RuleSet>) {
        if self.texture_rules() == rules.as_ref() {
            return;
        }

        self.end_stroke();
        let before = self.texture_rules().cloned();
        let name = match &rules {
            None => "Remove texture rules".to_owned(),
            Some(_) if before.is_none() => "Add texture rules".to_owned(),
            Some(_) => "Edit texture rules".to_owned(),
        };
        self.history.push(Box::new(TextureRulesCommand { name, before, after: rules }));
    }

    /// The rules' layer made now, waiting for one being made (an export; scripts). The edits not yet in the layers go in first: the map
    /// is composed again from the stack.
    pub fn finish_texture_rules(&mut self) {
        let t = self.tile_mut();
        t.sync_terrain_layers();
        t.refresh_texture_rules_with(true);
    }

    /// Where one of a set of rules (`index`) lays its texture, at its own weight (as though it were alone; not what it shows under later
    /// rules), on a grid of `cell` metres over the tile. Worked out now, on the editor's thread: the window's outline of the selected rule.
    ///
    /// # Errors
    ///
    /// No such rule, or the tile has no height or blend map.
    pub fn texture_rule_area(&mut self, rules: &[texture_rules::Rule], index: usize, cell: f32) -> Result<RuleArea, String> {
        let rule = rules.get(index).ok_or_else(|| format!("no rule {index}"))?.clone();
        let t = self.tile_mut();
        let (ex, ez) = (t.project.extent_x() as f32, t.project.extent_z() as f32);
        let prepared = t.prepare_texture_rules(vec![rule.clone()])?;
        let (fields, _) = prepared.fields();
        let border = prepared.border;
        let cell = cell.max(0.25);
        let (nx, nz) = ((ex / cell).floor() as usize + 1, (ez / cell).floor() as usize + 1);
        let weights = (0..nz)
            .flat_map(|z| (0..nx).map(move |x| (x, z)))
            .map(|(x, z)| {
                let (wx, wz) = (f64::from(x as f32 * cell) + border, f64::from(z as f32 * cell) + border);
                rule.weight(&fields, wx, wz) as f32
            })
            .collect();
        Ok(RuleArea { x0: 0.0, z0: 0.0, cell, nx, nz, weights })
    }

    /// What the rules' layer holds: the share of the blend map it covers, and of that each texture group's share (by the tile's slot names),
    /// the most first. `None` without the layer.
    #[must_use]
    pub fn texture_rules_summary(&self) -> Option<String> {
        let s = self.project().layers.as_ref()?.blend.as_ref()?;
        let id = s.layers.iter().find(|l| l.owner.starts_with(OWNER))?.id;
        let (w, h) = s.size();
        let mut px = [0u16; 16];
        let (mut cover, mut groups) = (0.0f64, [0.0f64; 4]);
        for y in 0..h {
            for x in 0..w {
                if s.get(id, x, y, &mut px) {
                    for (c, &v) in px.iter().enumerate() {
                        let v = f64::from(v) / 65535.0;
                        cover += v;
                        groups[c / 4] += v;
                    }
                }
            }
        }

        let n = f64::from(w) * f64::from(h);
        let slots = self.tile().texture_group_slots();
        let mut parts = groups.iter().copied().enumerate().filter(|&(_, g)| g > 0.0).map(|(i, g)| (g, i)).collect::<Vec<_>>();
        parts.sort_by(|a, b| b.0.total_cmp(&a.0));
        let parts = parts
            .iter()
            .map(|&(g, i)| format!("{} {:.1} %", slots.get(i).map_or("?", String::as_str), 100.0 * g / cover.max(1e-9)))
            .collect::<Vec<_>>()
            .join(", ");
        Some(format!("texture rules cover {:.1} % of the blend map ({parts})", 100.0 * cover / n.max(1.0)))
    }

    /// The tile's texture group slot names (red, green, blue, alpha), empty without an entry.
    #[must_use]
    pub fn texture_group_slots(&self) -> Vec<String> {
        self.tile().texture_group_slots()
    }

    /// The rules whose texture group the tile does not have (they lay nothing), by name.
    #[must_use]
    pub fn texture_rules_missing_groups(&self) -> Vec<String> {
        let Some(set) = self.texture_rules() else { return vec![] };
        let groups = self.tile().texture_group_slots();
        texture_rules::missing_groups(&set.rules, &groups).into_iter().map(|r| format!("{} ({})", r.name, r.group)).collect()
    }
}

/// An edit of the tile's texture rules.
pub(crate) struct TextureRulesCommand {
    name: String,
    before: Option<RuleSet>,
    after: Option<RuleSet>,
}

impl Command<Tile> for TextureRulesCommand {
    fn name(&self) -> String {
        self.name.clone()
    }

    fn apply(&mut self, t: &mut Tile) {
        t.project.texture_rules.clone_from(&self.after);
        t.project.texture_rules_dirty = true;
    }

    fn revert(&mut self, t: &mut Tile) {
        t.project.texture_rules.clone_from(&self.before);
        t.project.texture_rules_dirty = true;
    }

    fn dirty_items(&self) -> Vec<u64> {
        vec![TERRAIN]
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}
