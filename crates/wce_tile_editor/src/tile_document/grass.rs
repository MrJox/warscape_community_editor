//! The tile's grass: BOB's `default.grass_list.bin` (of the packed tile, or in the tile's folder), drawn by the renderer's grass pass near the
//! camera. A raw tile without one (or when the display asks) has it generated as BOB would (`wce_tile::vegetation::grass`) from the tile as
//! it is: its `<Grass generation_seed>`, the saved blend map and the kit's `max_grass.xml`. Each key (`grass/2`, `forest/0`...) is a model of the climate, `terrain/vegetation/battle/grass/<climate>/<key>.rigid_model_v2`
//! (a key the climate has no model for is not drawn, as in the game).
//!
//! How the game places a patch (empire.retail.dll "Grass Display" FUN_105103e0 and its job FUN_10515960, docs/re/vegetation.md): at the stored
//! x and z (the stored height is not used) on the terrain, tilted to the terrain normal measured half the patch model's size to each side,
//! not scaled. Patches on slopes steeper than `GRASS_ANGLE_CULL_AMOUNT` (0.7 rad) are dropped; the others are turned by a random yaw from the
//! MSVC rand LCG seeded with 0x49710ae3 at the start of every key's patches (of every worker's share of them: this is the one-worker
//! sequence), which steps only for the patches kept.

use std::io::Cursor;
use std::sync::Arc;

use rpfm_lib::files::Decodeable;
use rpfm_lib::files::fastbin_lists::{GRASS_LIST_EXTENSION, GrassListFile};
use wce_foundation::log;
use wce_foundation::math::{Float3, rotation_y, scaling, translation};
use wce_scene::render_scene::{EntityId, Grass, LayerId, Payload, RenderScene};
use wce_tile::vegetation::{self, grass::{GrassParams, generate_grass}};

use super::vegetation::tilt;
use super::{TERRAIN, Tile};

const SEED: u32 = 0x4971_0ae3;
const YAW_STEP: f32 = f32::from_bits(0x38c9_10a4);
const LIST: &str = "default";

/// The `GRASS_ANGLE_CULL_AMOUNT` tweaker's default: a patch stays where the terrain normal's y is above its cosine.
const ANGLE_CULL: f32 = 0.7;

/// FUN_10515960: the heights are measured max(the model's x / z extent, 0.4) * 0.5 metres to each side.
const MIN_SPAN: f32 = 0.4;

/// The decoded or generated list (per key: its patches' x and z) and the entities drawing it.
#[derive(Clone, Debug, Default)]
pub(crate) struct GrassState {
    /// Whether it was generated, and the list.
    pub list: Option<(bool, Arc<Vec<(String, Vec<(f32, f32)>)>>)>,

    /// What the entities show (whether grass is shown, and generated) and their ids.
    pub shown: Option<(bool, bool)>,
    pub entities: std::ops::Range<EntityId>,
}

impl Tile {

    fn read_grass_list(&self) -> Option<Vec<(String, Vec<(f32, f32)>)>> {
        let file = format!("{LIST}{GRASS_LIST_EXTENSION}");
        let bytes = std::fs::read(self.project.dir.join(&file))
            .ok()
            .or_else(|| self.packed_folder().and_then(|folder| self.files.as_deref()?.read(&format!("{folder}{file}")).ok()))?;
        match GrassListFile::decode(&mut Cursor::new(bytes), &None) {
            Ok(f) => Some(f.grass_list().grass_list().iter().map(|v| (v.key().replace('\\', "/").to_lowercase(), v.value().iter().map(|i| (*i.x(), *i.z())).collect())).collect()),
            Err(error) => {
                log::warn(format!("{file}: {error}"));
                Some(vec![])
            }
        }
    }

    /// BOB's grass from the tile as it is, grouped by key in the generator's order.
    fn generate_grass_list(&self) -> Result<Vec<(String, Vec<(f32, f32)>)>, String> {
        let path = vegetation::grass_params_path(&self.project).ok_or("no Assembly Kit max_grass.xml")?;
        let params = GrassParams::load(&path)?;
        let seed = vegetation::grass_seed(self.tscene()).ok_or("tile.ted has no <Grass> seed")?;
        let inputs = vegetation::tile_inputs(&self.project)?;
        let start = std::time::Instant::now();
        let items = generate_grass(&inputs, &params, seed);
        log::info(format!("grass: {} patches generated in {:.2} s", items.len(), start.elapsed().as_secs_f32()));
        let mut keys: Vec<(String, Vec<(f32, f32)>)> = vec![];
        for g in items {
            match keys.iter_mut().find(|(k, _)| *k == g.key) {
                Some((_, v)) => v.push((g.x, g.z)),
                None => keys.push((g.key, vec![(g.x, g.z)])),
            }
        }

        Ok(keys)
    }

    /// The list the display asks for: the packed one (unless generating is asked), else generated for a raw tile.
    fn grass_list(&mut self) -> Arc<Vec<(String, Vec<(f32, f32)>)>> {
        let generate = self.display.generate_grass;
        if let Some((g, list)) = &self.grass.list
            && *g == generate
        {
            return list.clone();
        }

        let packed = if generate { None } else { self.read_grass_list() };
        let list = packed.unwrap_or_else(|| {
            if self.project.raw_root.is_none() {
                return vec![];
            }

            self.generate_grass_list().unwrap_or_else(|error| {
                log::warn(format!("generating the grass: {error}"));
                vec![]
            })
        });
        let list = Arc::new(list);
        self.grass.list = Some((generate, list.clone()));
        list
    }

    /// Drops the list, so the next update reads or generates it again (after editing the seed or the painting).
    pub fn reload_grass(&mut self) {
        self.grass.list = None;
        self.grass.shown = None;
    }

    /// The model of a grass key in the tile's climate (`default` when the climate is not known). There is no fallback to the `default`
    /// folder: the game builds `terrain/vegetation/battle/grass/<climate>/<key>` (fn 0x1111f8a0, fn 0x1111e430) and skips a key without a
    /// model. No real climate has `rock/*`, `mud/*` or `sand/*` models (only `default` has), so BOB's patches on rock and mud are not drawn.
    fn grass_model(&self, key: &str) -> Option<String> {
        let files = self.files.as_deref()?;
        let climate = self.climates().find(&self.look.climate).map_or_else(|| "default".to_owned(), |c| c.name.to_lowercase());
        let path = format!("terrain/vegetation/battle/grass/{climate}/{key}.rigid_model_v2");
        files.exists(&path).then_some(path)
    }

    /// The larger of a grass model's x and z extents (LOD 0), in metres.
    fn grass_model_size(&self, path: &str) -> Option<f32> {
        use rpfm_lib::files::rigidmodel::RigidModel;
        let bytes = self.files.as_deref()?.read(path).ok()?;
        let model = RigidModel::decode(&mut Cursor::new(bytes), &Some(Default::default())).ok()?;
        let (mut lo, mut hi) = ([f32::MAX; 2], [f32::MIN; 2]);
        for block in model.lods().first()?.mesh_blocks() {
            for v in block.mesh().vertices() {
                let p = v.position();
                lo = [lo[0].min(p.x), lo[1].min(p.z)];
                hi = [hi[0].max(p.x), hi[1].max(p.z)];
            }
        }

        (lo[0] <= hi[0]).then(|| (hi[0] - lo[0]).max(hi[1] - lo[1]))
    }

    /// The grass entities follow the display (after a publish, which dropped them, or when grass is shown or hidden).
    pub(crate) fn update_grass(&mut self, scene: &mut RenderScene, layer: LayerId, published: bool) {
        // The season of the trees' and grass' textures follows the ground's.
        if scene.vegetation_season != self.look.season {
            scene.vegetation_season.clone_from(&self.look.season);
        }

        let want = (self.display.show_grass, self.display.generate_grass);
        if !published && self.grass.shown.as_ref() == Some(&want) {
            return;
        }

        if !published {
            scene.remove_range(self.grass.entities.clone());
        }

        let first = scene.next_id();
        let added = if want.0 { self.add_grass(scene, layer) } else { 0 };
        self.grass.entities = first..first + added as EntityId;
        self.grass.shown = Some(want);
    }

    fn add_grass(&mut self, scene: &mut RenderScene, layer: LayerId) -> usize {
        let list = self.grass_list();
        if list.is_empty() || !self.project.has_height() {
            return 0;
        }

        // As the trees: the list's z runs the other way (z = extent - world z); the models in the tile's proportions.
        let unit = self.unit() as f32;
        let extent = self.project.extent_z() as f32;
        let mut added = 0;
        for (key, patches) in list.iter() {
            // The game skips a key whose model it can't find too (settlement_2 has none in any climate).
            let Some(model) = self.grass_model(key) else {
                log::info(format!("grass {key}: no model, not drawn"));
                continue;
            };

            let half = self.grass_model_size(&model).unwrap_or(0.0).max(MIN_SPAN) * 0.5;
            let height = |x: f32, z: f32| self.project.height_at(f64::from(x), f64::from(z)) as f32;
            let min_ny = ANGLE_CULL.cos();
            let mut rand = SEED;
            let mut instances = Vec::with_capacity(patches.len());
            for &(x, lz) in patches {
                // The slope over the patch (height drop per metre along x and z).
                let z = extent - lz;
                let sx = (height(x - half, z) - height(x + half, z)) / (2.0 * half);
                let sz = (height(x, z - half) - height(x, z + half)) / (2.0 * half);
                if 1.0 / (sx * sx + 1.0 + sz * sz).sqrt() <= min_ny {
                    continue;
                }

                rand = rand.wrapping_mul(0x343fd).wrapping_add(0x269ec3);
                let yaw = (rand >> 16) as f32 * YAW_STEP;
                let y = height(x, z) / unit;
                let w = translation(Float3::new(x, y, z)) * tilt(sx / unit, sz / unit) * rotation_y(-yaw) * scaling(1.0 / unit);
                let r = |i: usize| [w.m[i][0], w.m[i][1], w.m[i][2], w.m[i][3]];
                let (a, b, c) = (r(0), r(1), r(2));
                instances.push([a[0], a[1], a[2], a[3], b[0], b[1], b[2], b[3], c[0], c[1], c[2], c[3]]);
            }

            let e = scene.add(layer, wce_foundation::math::Float4x4::IDENTITY, Payload::Grass(Grass { model, instances: Arc::new(instances) }), self.source(TERRAIN));
            if let Some(en) = scene.find_mut(e) {
                en.locked = true;
            }

            added += 1;
        }

        added
    }
}
