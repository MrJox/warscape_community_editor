//! Biome areas in a tile (docs/plans/auto_detail.md, C): an area is a vegetation exclusion polygon with `wce_biome` (the biome's name) and
//! `wce_biome_seed`; the tile keeps the biomes its areas use (`TileProject::biomes`, their own file). Taking a biome in is an undo entry
//! ([`BiomesCommand`]).

use std::any::Any;

use wce_editor_core::command::Command;
use wce_foundation::math::Double3;
use wce_scene::render_scene::{EntityId, MeshInstance, Payload, RenderScene};
use wce_tile::biomes::{AREA_TYPE, BIOME_ATTR, Biome, SEED_ATTR};
use wce_tile::ted_scene::TedScene;
use wce_tile::vegetation_sets::VegetationSet;

use super::draft::{turned, with_root_attr};
use super::edits::point_instance;
use super::{TERRAIN, Tile, TileDocument};

/// The layer an export puts the areas' contents in.
pub const EXPORT_LAYER: &str = "WCE biomes";

/// Metres the contents keep from a road's texture and from a real building's footprint.
const KEEP_OFF_ROAD: f64 = 1.5;
const KEEP_OFF_BUILDING: f32 = 1.0;

/// A tree, a plant of the undergrowth or a rock of an area: its element's text (its world position in it, scene units) and that position.
#[derive(Clone, Debug)]
pub(crate) struct BiomeItem {
    area: usize,
    text: Vec<u8>,
    position: [f64; 3],
}

/// The areas' contents being made off the editor's thread, and the signature of what they are made from.
pub(crate) struct BiomeJob {
    job: super::jobs::Job<Vec<BiomeItem>>,
    signature: u64,
}

/// What the contents keep off (world metres): the real buildings (x, z, radius) and the roads' pixels on the blend map (from its corner;
/// `ppm` pixels a metre, `border` metres from its corner to the tile's origin).
#[derive(Clone, Default)]
struct KeepOff {
    buildings: Vec<[f64; 3]>,
    roads: Option<(wce_tile::texture_rules::Distances, (f64, f64), f64)>,
}

impl KeepOff {
    fn blocked(&self, x: f32, z: f32) -> bool {
        let near_building =
            self.buildings.iter().any(|&[bx, bz, r]| (f64::from(x) - bx).hypot(f64::from(z) - bz) < r + f64::from(KEEP_OFF_BUILDING));
        let on_road = self.roads.as_ref().is_some_and(|(d, _, border)| d.at(f64::from(x) + border, f64::from(z) + border) < KEEP_OFF_ROAD);
        near_building || on_road
    }
}

/// An area: its element, its biome's name, its seed and its polygon (world metres, x and z).
#[derive(Clone, Debug, PartialEq)]
pub struct BiomeArea {
    pub elem: usize,
    pub biome: String,
    pub seed: u64,
    pub points: Vec<(f32, f32)>,
}

impl Tile {
    /// The tile's biome areas, in the scene's order (hidden ones too: their contents go with their visibility, the caller's to say).
    #[must_use]
    pub fn biome_areas(&self) -> Vec<BiomeArea> {
        let sc = self.tscene();
        sc.find_all(AREA_TYPE)
            .into_iter()
            .filter_map(|e| {
                let biome = sc.get(e, BIOME_ATTR).filter(|b| !b.is_empty())?;
                let seed = sc.get(e, SEED_ATTR).and_then(|s| s.trim().parse().ok()).unwrap_or(e as u64);
                let points = self.polygon_world_points(e).into_iter().map(|p| (p.x, p.z)).collect();
                Some(BiomeArea { elem: e, biome, seed, points })
            })
            .collect()
    }

    /// A biome by name: the tile's copy, else the built-in one of that name.
    #[must_use]
    pub fn biome_named(&self, name: &str) -> Option<Biome> {
        self.project
            .biomes
            .iter()
            .find(|b| b.name == name)
            .cloned()
            .or_else(|| wce_tile::biomes::builtin().into_iter().find(|b| b.name == name))
    }
}

impl Tile {
    /// What the areas' contents are made from, hashed: the areas (their points, biome and seed), the biomes, the heights, the real
    /// buildings, the roads' layers, the sets given (`biome_sets`).
    fn biome_signature(&mut self, areas: &[BiomeArea]) -> u64 {
        let mut h = 0xcbf2_9ce4_8422_2325u64;
        let mut eat = |bytes: &[u8]| {
            for &b in bytes {
                h = (h ^ u64::from(b)).wrapping_mul(0x0100_0000_01b3);
            }
        };
        for a in areas {
            eat(a.biome.as_bytes());
            eat(&a.seed.to_le_bytes());
            for &(x, z) in &a.points {
                eat(&x.to_le_bytes());
                eat(&z.to_le_bytes());
            }

            if let Some(b) = self.biome_named(&a.biome) {
                eat(b.element_text("").as_bytes());
            }
        }

        if let Some(hf) = self.project.height.as_ref() {
            let version = hf.version();
            if self.texture_rules_heights.0 != version {
                self.texture_rules_heights = (version, hf.region_hash(0.0, 0.0, f64::from(hf.width()), f64::from(hf.height())));
            }

            eat(&self.texture_rules_heights.1.to_le_bytes());
        }

        for v in [self.history_version, self.structure_version, self.models.len() as u64] {
            eat(&v.to_le_bytes());
        }

        for s in self.biome_sets.iter() {
            eat(s.name.as_bytes());
            for e in &s.entries {
                eat(e.key.as_bytes());
            }
        }

        h
    }

    /// Makes the areas' contents again when what they are made from changed and stayed so for a look; `wait`: now, waiting (an export).
    pub(super) fn refresh_biomes(&mut self, wait: bool) {
        if self.stroke_open {
            return;
        }

        self.take_biomes(wait);
        let areas = self.biome_areas();
        if areas.is_empty() {
            if !self.biome_items.is_empty() {
                self.biome_items.clear();
                self.biome_items_shown = false;
            }

            self.biome_made = 0;
            return;
        }

        let signature = self.biome_signature(&areas);
        if signature == self.biome_made {
            return;
        }

        if signature != self.biome_seen && !wait {
            self.biome_seen = signature;
            return;
        }

        if self.biome_job.as_ref().is_some_and(|j| j.signature == signature) {
            if wait {
                self.take_biomes(true);
            }

            return;
        }

        self.biome_seen = signature;
        self.start_biomes(areas, signature);
        if wait {
            self.take_biomes(true);
        }
    }

    fn start_biomes(&mut self, areas: Vec<BiomeArea>, signature: u64) {
        let unit = self.unit();
        let sets = self.biome_sets.clone();
        let biomes = areas.iter().map(|a| self.biome_named(&a.biome)).collect::<Vec<_>>();
        let p = &mut self.project;
        let (mx, mz) = (p.mpp_x(), p.mpp_z());
        let fields = p.height.as_ref().map(|hf| wce_tile::texture_rules::Fields::from_height_field(hf, (mx, mz), unit));
        let border = p.border_m();
        let roads = p.blend.as_ref().zip(p.layers.as_ref().and_then(|l| l.blend.as_ref())).and_then(|(bf, stack)| {
            let (bw, bh) = (bf.width(), bf.height());
            let ppm = (f64::from(bw) / (p.extent_x() + 2.0 * border), f64::from(bh) / (p.extent_z() + 2.0 * border));
            let empty = |px: &[u16]| px.iter().all(|&v| v == 0);
            let mask =
                (0..bh).flat_map(|y| (0..bw).map(move |x| (x, y))).map(|(x, y)| stack.owned_at("road:", x, y, empty)).collect::<Vec<_>>();
            mask.iter().any(|&m| m).then(|| (wce_tile::texture_rules::Distances::from_mask(bw, bh, ppm, &mask), ppm, border))
        });
        let keep_off = KeepOff { buildings: self.texture_rules_buildings(), roads };
        let job = super::jobs::Job::spawn("biomes", move || {
            wce_foundation::wce_prof!("tile.biomes");
            let height = |x: f32, z: f32| fields.as_ref().map_or(0.0, |f| f.height(f64::from(x) + border, f64::from(z) + border) as f32);
            let mut out = vec![];
            for (a, biome) in areas.iter().zip(&biomes) {
                let Some(biome) = biome else { continue };
                let made =
                    wce_tile::biomes::generate(biome, &biome.resolve(&sets), &a.points, &height, &|x, z| keep_off.blocked(x, z), a.seed);
                for (kind, list) in
                    [("VEGETATION_INSTANCE", &made.trees), ("VEGETATION_INSTANCE", &made.undergrowth), ("BUILDING_INSTANCE", &made.rocks)]
                {
                    for p in list {
                        let at = Double3::new(f64::from(p.x) * unit, f64::from(p.y) * unit, f64::from(p.z) * unit);
                        let Some(text) = item_text(kind, &p.key, at, p.scale, p.yaw, unit) else { continue };
                        out.push(BiomeItem { area: a.elem, text, position: [at.x, at.y, at.z] });
                    }
                }
            }

            out
        });
        self.biome_job = Some(BiomeJob { job, signature });
    }

    fn take_biomes(&mut self, wait: bool) {
        let Some(j) = &self.biome_job else { return };
        let made = match if wait { j.job.wait() } else { j.job.poll() } {
            super::jobs::Poll::Running => return,
            super::jobs::Poll::Done(m) => Some(m),
            super::jobs::Poll::Failed => None,
        };

        let j = self.biome_job.take().expect("a job");
        let Some(items) = made else {
            wce_foundation::log::warn("biomes: the areas' contents could not be made");
            return;
        };

        self.biome_items = items;
        self.biome_made = j.signature;
        self.biome_items_shown = false;
    }

    /// The areas' contents in the viewport, again when they were made again (locked: they are the areas', not tile.ted's).
    pub(super) fn show_biome_items(&mut self, scene: &mut RenderScene) {
        if self.biome_items_shown {
            return;
        }

        self.biome_items_shown = true;
        for id in std::mem::take(&mut self.biome_entities) {
            scene.remove(id);
        }

        let unit = self.unit();
        let mut ids: Vec<EntityId> = vec![];
        for item in &self.biome_items {
            let source = self.source(super::item_of(item.area));
            let [x, y, z] = item.position;
            for (local, payload) in self.preview_payloads(&item.text) {
                let mut world = local;
                world.m[0][3] += (x / unit) as f32;
                world.m[1][3] += (y / unit) as f32;
                world.m[2][3] += (z / unit) as f32;
                let tree = matches!(payload, Payload::MeshInstance(_)) && item.text.starts_with(b"<VEGETATION_INSTANCE");
                // A tree as the game draws it (as the tree list's): tilted about its foot to the ground, its untilted matrix in
                // POSITION5..7 so that the shaders keep the trunk upright and only the roots follow the slope.
                let (world, payload) = match payload {
                    Payload::MeshInstance(mi) if tree => {
                        let foot = wce_foundation::math::Float3::new(world.m[0][3], world.m[1][3], world.m[2][3]);
                        let tilt = self.terrain_tilt((x / unit) as f32, (z / unit) as f32);
                        let [p5, p6, p7] = super::vegetation::untilted_rows(&world);
                        let tilted =
                            wce_foundation::math::translation(foot) * tilt * wce_foundation::math::translation(foot * -1.0) * world;
                        (tilted, Payload::MeshInstance(MeshInstance { p5, p6, p7, ..mi }))
                    }
                    payload => (world, payload),
                };
                let id = scene.add(self.layer, world, payload, source);
                if let Some(en) = scene.find_mut(id) {
                    en.locked = true;
                }

                ids.push(id);
            }
        }

        self.biome_entities = ids;
    }

    /// The layer of the areas' contents an export adds, as TEd and the game read it.
    pub(super) fn with_biome_items(&self, text: Vec<u8>) -> Result<Vec<u8>, String> {
        if self.biome_items.is_empty() {
            return Ok(text);
        }

        let mut scene = TedScene::parse_any(text)?;
        let layer = scene.add_layer(scene.scene_graph(), EXPORT_LAYER)?;
        for item in &self.biome_items {
            scene.insert_element_text(layer, &item.text)?;
        }

        Ok(scene.save())
    }

    /// How many trees, plants and rocks the areas hold now.
    #[must_use]
    pub fn biome_item_counts(&self) -> (usize, usize) {
        let trees = self.biome_items.iter().filter(|i| i.text.starts_with(b"<VEGETATION_INSTANCE")).count();
        (trees, self.biome_items.len() - trees)
    }
}

/// An item's element text at a point (scene units), scaled (a tree; a building stays at 1: the game ignores its scale) and turned.
impl Tile {
    /// What a biome's thumbnail shows: a patch of its inside (`side` metres square on flat ground, no edge band, a seed of its own) as
    /// models around the patch's centre, as a single item's thumbnail has them.
    #[must_use]
    pub fn biome_preview_parts(&self, biome: &Biome, sets: &[VegetationSet], side: f32) -> Vec<(wce_foundation::math::Float4x4, Payload)> {
        let h = side * 0.5;
        let poly = [(-h, -h), (h, -h), (h, h), (-h, h)];
        // Its inside: no edge band (a patch this small would be all edge).
        let inside = Biome { edge: 0.0, ..biome.clone() };
        let made = wce_tile::biomes::generate(&inside, &inside.resolve(sets), &poly, &|_, _| 0.0, &|_, _| false, 7);
        let unit = self.unit();
        let mut out = vec![];
        for (kind, list) in
            [("VEGETATION_INSTANCE", &made.trees), ("VEGETATION_INSTANCE", &made.undergrowth), ("BUILDING_INSTANCE", &made.rocks)]
        {
            for p in list {
                let at = Double3::new(f64::from(p.x) * unit, 0.0, f64::from(p.z) * unit);
                let Some(text) = item_text(kind, &p.key, at, p.scale, p.yaw, unit) else { continue };
                for (local, payload) in self.item_parts(&text) {
                    let mut world = local;
                    world.m[0][3] += p.x;
                    world.m[2][3] += p.z;
                    out.push((world, payload));
                }
            }
        }

        out
    }
}

fn item_text(kind: &str, key: &str, at: Double3, scale: f32, yaw: f32, unit: f64) -> Option<Vec<u8>> {
    let n = if kind == "BUILDING_INSTANCE" {
        wce_tile::new_instances::NewInstance::new(kind).position(at).attr("key", key).attr("name", key)
    } else {
        point_instance(kind, at, key, unit).ok()?
    };
    let mut text = wce_tile::new_instances::element_text(&n).ok()?;
    if kind == "BUILDING_INSTANCE" {
        text = with_root_attr(&text, "surface_mode", "MSM_FREEFORM");
    } else {
        text = with_root_attr(&text, "scale", &scale.to_string());
    }

    Some(turned(&text, yaw))
}

impl TileDocument {
    /// The vegetation and building sets the biomes' named props are looked for in (the tool's: the projects' sets).
    pub fn set_biome_sets(&mut self, sets: std::sync::Arc<Vec<VegetationSet>>) {
        let t = self.tile_mut();
        if !std::sync::Arc::ptr_eq(&t.biome_sets, &sets) {
            t.biome_sets = sets;
        }
    }

    /// A brush stroke on the biome areas (circles of `radius` at `dabs`, world metres): `area`'s outline grown by them (or cut, `erase`),
    /// or (no area) a new area of `biome` in the layer `into`. What is left in more than one piece becomes as many areas (the same
    /// biome, new seeds); an area cut away entirely goes. One undo entry; the areas after it.
    ///
    /// # Errors
    ///
    /// No such area, nothing to make (a stroke that makes no area), or the scene's edit failed.
    pub fn biome_stroke(
        &mut self,
        area: Option<usize>,
        into: usize,
        biome: &str,
        dabs: &[(f32, f32)],
        radius: f32,
        erase: bool,
    ) -> Result<Vec<usize>, String> {
        let t = self.tile();
        let cell = (radius / 4.0).clamp(0.5, 4.0);
        let (existing, attrs, layer) = match area {
            Some(e) => {
                let a = t.biome_areas().into_iter().find(|a| a.elem == e).ok_or("not a biome area")?;
                let sc = t.tscene();
                let attrs = [BIOME_ATTR, SEED_ATTR, "name"]
                    .into_iter()
                    .filter_map(|k| sc.get(e, k).map(|v| (k.to_owned(), v)))
                    .collect::<std::collections::BTreeMap<_, _>>();
                // Its container: the layer or group whose `children` it is in.
                let els = sc.doc().elements();
                let holder = els[e].parent.and_then(|c| if els[c].name == "children" { els[c].parent } else { Some(c) }).unwrap_or(into);
                (a.points, attrs, holder)
            }

            None if erase => return Err("nothing to take away from".to_owned()),
            None => {
                let attrs = [(BIOME_ATTR, biome), (SEED_ATTR, &dabs.len().to_string()), ("name", biome)]
                    .into_iter()
                    .map(|(k, v)| (k.to_owned(), v.to_owned()))
                    .collect::<std::collections::BTreeMap<_, _>>();
                (vec![], attrs, into)
            }
        };

        let outlines = wce_tile::biomes::stroke_outline(&existing, dabs, radius, erase, cell);
        if outlines.is_empty() && area.is_none() {
            return Err("the stroke makes no area".to_owned());
        }

        // Each outline's points on the ground, world (metres).
        let worlds = outlines
            .iter()
            .map(|o| o.iter().map(|&(x, z)| wce_foundation::math::Float3::new(x, self.tile().terrain_height(x, z), z)).collect::<Vec<_>>())
            .collect::<Vec<_>>();
        let mark = self.begin()?;
        let mut out = vec![];
        let result = (|| -> Result<(), String> {
            let mut pieces = worlds.iter();
            if let Some(e) = area {
                match pieces.next() {
                    // The area takes the largest piece: its points moved, the missing ones added, the extra ones taken away.
                    Some(world) => {
                        let local = world.iter().map(|&p| self.tile().to_local(e, p)).collect::<Vec<_>>();
                        let sc = self.tscene_mut();
                        let had = sc.polygon_points(e).len();
                        for (i, &p) in local.iter().enumerate().take(had) {
                            sc.set_polygon_point(e, i, p)?;
                        }

                        for (i, &p) in local.iter().enumerate().skip(had) {
                            sc.insert_polygon_point(e, i - 1, p)?;
                        }

                        while sc.polygon_points(e).len() > local.len() {
                            let last = sc.polygon_points(e).len() - 1;
                            if !sc.remove_polygon_point(e, last)? {
                                break;
                            }
                        }

                        out.push(e);
                    }

                    None => {
                        self.tscene_mut().remove(e)?;
                        return Ok(());
                    }
                }
            }

            // The other pieces: new areas of the same biome, each its own seed.
            for (k, world) in pieces.enumerate() {
                let local = world.iter().map(|&p| self.tile().to_local(layer, p)).collect::<Vec<_>>();
                let c = local.iter().fold(Double3::default(), |a, &p| a + p) * (1.0 / local.len() as f64);
                let mut n = super::edits::new_polygon(AREA_TYPE, local.into_iter().map(|p| p - c).collect(), true).position(c);
                for (key, v) in &attrs {
                    let v = if key == SEED_ATTR { format!("{}{k}", v) } else { v.clone() };
                    n.attrs.insert(key.clone(), v);
                }

                let text = wce_tile::new_instances::element_text(&n)?;
                out.push(self.tscene_mut().insert_element_text(layer, &text)?);
            }

            Ok(())
        })();

        if let Err(err) = result {
            return self.rolled_back(mark, err);
        }

        let sel = out.iter().map(|&e| super::item_of(e)).collect::<Vec<_>>();
        self.push_splice(if erase { "Biome brush: take away" } else { "Biome brush" }.to_owned(), mark, vec![], sel);
        Ok(out)
    }

    /// The areas' contents made now, waiting for those being made (an export; scripts).
    pub fn finish_biomes(&mut self) {
        self.tile_mut().refresh_biomes(true);
    }

    /// The biomes the tile keeps.
    #[must_use]
    pub fn tile_biomes(&self) -> &[Biome] {
        &self.project().biomes
    }

    /// Takes a biome into the tile (in place of its copy of that name; a preset's file and folder are not the tile's): one undo entry, none
    /// when the tile has it so already.
    pub fn adopt_biome(&mut self, biome: &Biome) {
        let b = Biome { folder: String::new(), path: None, ..biome.clone() };
        let before = self.project().biomes.clone();
        let mut after = before.clone();
        match after.iter_mut().find(|o| o.name == b.name) {
            Some(o) if *o == b => return,
            Some(o) => *o = b,
            None => after.push(b),
        }

        self.end_stroke();
        self.history.push(Box::new(BiomesCommand { name: format!("Biome {}", biome.name), before, after }));
    }

    /// Takes the tile's biomes no area uses out (one undo entry, none when every one is used).
    pub fn drop_unused_biomes(&mut self) {
        let used = self.tile().biome_areas().into_iter().map(|a| a.biome).collect::<std::collections::HashSet<_>>();
        let before = self.project().biomes.clone();
        let after = before.iter().filter(|b| used.contains(&b.name)).cloned().collect::<Vec<_>>();
        if after.len() != before.len() {
            self.end_stroke();
            self.history.push(Box::new(BiomesCommand { name: "Unused biomes".to_owned(), before, after }));
        }
    }
}

/// A change of the tile's biomes.
pub(crate) struct BiomesCommand {
    name: String,
    before: Vec<Biome>,
    after: Vec<Biome>,
}

impl Command<Tile> for BiomesCommand {
    fn name(&self) -> String {
        self.name.clone()
    }

    fn apply(&mut self, t: &mut Tile) {
        t.project.biomes.clone_from(&self.after);
        t.project.biomes_dirty = true;
    }

    fn revert(&mut self, t: &mut Tile) {
        t.project.biomes.clone_from(&self.before);
        t.project.biomes_dirty = true;
    }

    fn dirty_items(&self) -> Vec<u64> {
        vec![TERRAIN]
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}
