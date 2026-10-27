//! The tile's procedural vegetation: the trees BOB generated from the tile's seeds (`<list>.tree_list.bin` of the packed tile, one list per
//! climate key plus `default`), drawn with the static VEGETATION_INSTANCEs. The lists hold the static trees too, those of the layers hidden in
//! TEd included (the game draws them), so a list tree standing where a shown static instance stands is skipped.
//!
//! How the game places a list tree (empire.retail.dll FUN_10429b80, docs/re/vegetation.md): at the stored x and z, with the stored height
//! when the tree is freeform and on the terrain otherwise, scaled by the scale byte / 127, turned by a random yaw. The yaw comes from the
//! MSVC rand LCG seeded with 0x49710ae3 at the start of every list (of every worker's share of it, so the game's yaws depend on its thread
//! count; this is the one-worker sequence). Every tree is tilted to the terrain: its up axis goes along the terrain normal measured 0.5 m around it.
//! The static VEGETATION_INSTANCEs are list trees in the game, so they are drawn tilted too, and with the model the shown list's climate
//! converts their key to (`per_climate_tree_conversions.xml`, as BOB writes them into the list).
//!
//! A list the tile has no packed file for (a raw tile BOB hasn't processed, or `gen:<list>` asked for) is generated as BOB would
//! (`wce_tile::vegetation`), from the tile's seeds, height map, saved blend map, exclusion polygons and static trees and the Assembly
//! Kit's vegetation parameters.

use std::collections::HashSet;
use std::io::Cursor;
use std::sync::Arc;

use rpfm_lib::files::Decodeable;
use rpfm_lib::files::fastbin_lists::{TREE_LIST_EXTENSION, TreeListFile};
use wce_foundation::log;
use wce_foundation::math::{Float3, Float4x4, rotation_y, scaling, translation};
use wce_scene::render_scene::{EntityId, LayerId, MeshInstance, Payload, RenderScene};

use wce_tile::vegetation::{self, GenerationParams};

use super::{TERRAIN, Tile};

/// The list drawn unless the display says otherwise: BOB's list for no particular climate.
pub const DEFAULT_TREE_LIST: &str = "default";

const SEED: u32 = 0x4971_0ae3;

/// The prefix of a list generated from the seeds even when the tile has it packed.
pub const GENERATED_PREFIX: &str = "gen:";

/// FUN_10429b80's yaw step (2 pi / 65536, as the game rounds it).
const YAW_STEP: f32 = f32::from_bits(0x38c9_10a4);

/// How close a list tree must be to a static instance (x and z, world units) to be that instance.
const SAME_TREE: f32 = 0.02;

/// One tree of a list, in the list's own frame.
#[derive(Clone, Debug)]
pub(crate) struct ListTree {
    pub model: String,
    pub x: f32,
    pub y: f32,
    pub z: f32,
    pub scale: f32,
    pub freeform: bool,
    pub yaw: f32,
}

/// A decoded list, kept while the display shows it, and the entities drawing it.
#[derive(Clone, Debug, Default)]
pub(crate) struct TreeList {
    pub name: String,
    pub trees: Arc<Vec<ListTree>>,

    /// What the entities show (the list's name, empty for none, and whether vegetation is hidden), and their ids.
    pub shown: Option<(String, bool)>,
    pub entities: std::ops::Range<EntityId>,

    /// The tile's lists, looked up once.
    pub names: std::cell::OnceCell<Vec<String>>,

    /// The Assembly Kit's vegetation parameters, read once (None without a kit or when they don't read).
    pub params: std::cell::OnceCell<Option<Arc<GenerationParams>>>,
}

impl Tile {

    /// The matrix an instance's model is drawn with: [`Tile::world_matrix`], tilted to the terrain for a tree.
    pub fn drawn_matrix(&self, elem: usize) -> Float4x4 {
        let w = self.world_matrix(elem);
        if self.tscene().type_of(elem) != "VEGETATION_INSTANCE" {
            return w;
        }

        let p = Float3::new(w.m[0][3], w.m[1][3], w.m[2][3]);
        translation(p) * self.terrain_tilt(p.x, p.z) * translation(Float3::new(-p.x, -p.y, -p.z)) * w
    }

    /// FUN_10429b80's tilt at a world point: the rotation that takes the up axis to the terrain normal, the normal from heights 0.5 m to each side.
    pub(crate) fn terrain_tilt(&self, x: f32, z: f32) -> Float4x4 {
        if !self.project.has_height() {
            return Float4x4::IDENTITY;
        }

        let h = |x: f32, z: f32| (self.project.height_at(f64::from(x), f64::from(z)) / self.unit()) as f32;
        tilt(h(x - 0.5, z) - h(x + 0.5, z), h(x, z - 0.5) - h(x, z + 0.5))
    }

    /// The packed tile's folder in the game files (`terrain/tiles/battle/<set>/.../<variation>/`), for a tile under a kit's raw_data.
    pub(crate) fn packed_folder(&self) -> Option<String> {
        let dir = self.project.dir.to_string_lossy().replace('\\', "/").to_lowercase();
        let at = dir.find("/terrain/tiles/")?;
        Some(format!("{}/", dir[at + 1..].trim_end_matches('/')))
    }

    /// The tree lists there are for the tile (their names: `default`, `rome_med`, ...), in the game files or in the tile's folder, then
    /// the generated ones (`gen:<list>`) when the kit's parameters are there.
    pub fn tree_lists(&self) -> &[String] {
        self.trees.names.get_or_init(|| self.find_tree_lists())
    }

    fn find_tree_lists(&self) -> Vec<String> {
        let mut names = Vec::new();
        if let (Some(files), Some(folder)) = (self.files.as_deref(), self.packed_folder()) {
            for path in files.list(&folder, TREE_LIST_EXTENSION) {
                let file = &path[folder.len()..];
                if !file.contains('/') {
                    names.push(file.trim_end_matches(TREE_LIST_EXTENSION).to_owned());
                }
            }
        }

        if let Ok(dir) = std::fs::read_dir(&self.project.dir) {
            for entry in dir.flatten() {
                let file = entry.file_name().to_string_lossy().to_lowercase();
                if let Some(name) = file.strip_suffix(TREE_LIST_EXTENSION) {
                    names.push(name.to_owned());
                }
            }
        }

        names.sort();
        names.dedup();
        if let Some(params) = self.generation_params() {
            names.extend(params.categories.iter().map(|c| format!("{GENERATED_PREFIX}{}", c.to_lowercase())));
        }

        names
    }

    /// The climate whose conversions the static trees are drawn with: the shown list's (none for `default` or no list).
    pub(crate) fn vegetation_climate(&self) -> String {
        self.display.tree_list.trim_start_matches(GENERATED_PREFIX).to_lowercase()
    }

    /// A vegetation key as the shown list's climate converts it (BOB's static trees in that list), or the key itself.
    pub(crate) fn converted_vegetation_key(&self, key: &str) -> String {
        let climate = self.vegetation_climate();
        if climate.is_empty() {
            return key.to_owned();
        }

        self.generation_params()
            .and_then(|p| p.conversions.get(&climate).and_then(|t| t.get(&key.to_lowercase())).cloned())
            .unwrap_or_else(|| key.to_owned())
    }

    fn generation_params(&self) -> Option<Arc<GenerationParams>> {
        self.trees
            .params
            .get_or_init(|| {
                let dir = vegetation::params_dir(&self.project)?;
                match GenerationParams::load(&dir) {
                    Ok(p) => Some(Arc::new(p)),
                    Err(error) => {
                        log::warn(format!("vegetation parameters: {error}"));
                        None
                    }
                }
            })
            .clone()
    }

    /// Drops the decoded or generated list, so the next update reads or generates it again (after editing the seeds, the exclusions...).
    pub fn reload_tree_list(&mut self) {
        self.trees.name.clear();
        self.trees.shown = None;
    }

    /// BOB's trees for a list (a category) from the tile as it is now.
    fn generate_tree_list(&self, name: &str) -> Result<Vec<ListTree>, String> {
        let params = self.generation_params().ok_or("no Assembly Kit vegetation parameters")?;
        let category = params.category(name).ok_or_else(|| format!("no vegetation category {name}"))?;
        let seeds = vegetation::seeds(self.tscene()).ok_or("tile.ted has no <Vegetation> seeds")?;
        let inputs = vegetation::tile_inputs(&self.project)?;
        let files = self.files.clone();
        let exists = |p: &str| files.as_deref().is_none_or(|f| f.exists(&p.replace('\\', "/").to_lowercase()));
        let start = std::time::Instant::now();
        let statics = vegetation::static_trees(self.tscene(), &inputs);
        let generated = vegetation::tree_list(&inputs, &params, category, seeds, &statics, &exists);
        log::info(format!("{name}: {} trees generated in {:.2} s", generated.len(), start.elapsed().as_secs_f32()));

        // Grouped by path as the list stores them (paths differing only in case are separate lists), in the list's order: the yaws restart
        // per path.
        let mut models: Vec<(&str, Vec<&vegetation::ListTree>)> = vec![];
        for t in &generated {
            match models.iter_mut().find(|(p, _)| *p == t.path) {
                Some((_, v)) => v.push(t),
                None => models.push((&t.path, vec![t])),
            }
        }

        let mut trees = vec![];
        for (path, items) in models {
            let model = path.replace('\\', "/").to_lowercase();
            let mut rand = SEED;
            for t in items {
                rand = rand.wrapping_mul(0x343fd).wrapping_add(0x269ec3);
                let scale = f32::from(t.scale_byte) / 127.0;
                trees.push(ListTree { model: model.clone(), x: t.x, y: t.y, z: t.z, scale, freeform: false, yaw: (rand >> 16) as f32 * YAW_STEP });
            }
        }

        Ok(trees)
    }

    fn read_tree_list(&self, name: &str) -> Option<Vec<u8>> {
        let file = format!("{name}{TREE_LIST_EXTENSION}");
        if let Ok(bytes) = std::fs::read(self.project.dir.join(&file)) {
            return Some(bytes);
        }

        let path = format!("{}{file}", self.packed_folder()?);
        self.files.as_deref()?.read(&path).ok()
    }

    /// The list the display asks for, decoded once; empty when it is off or missing.
    pub(crate) fn tree_list(&mut self) -> Arc<Vec<ListTree>> {
        let name = self.display.tree_list.clone();
        if name.is_empty() {
            return Arc::default();
        }

        if self.trees.name == name {
            return self.trees.trees.clone();
        }

        self.trees.name.clone_from(&name);

        let packed = if name.starts_with(GENERATED_PREFIX) { None } else { self.read_tree_list(&name) };
        let trees = match packed.map(|bytes| TreeListFile::decode(&mut Cursor::new(bytes), &None)) {
            Some(Ok(file)) => list_trees(&file),
            Some(Err(error)) => {
                log::warn(format!("{name}{TREE_LIST_EXTENSION}: {error}"));
                vec![]
            }
            None => {
                let category = name.trim_start_matches(GENERATED_PREFIX);
                // Without a packed list, only a raw tile (with the kit around it) can generate one; quietly nothing otherwise.
                if self.project.raw_root.is_none() {
                    vec![]
                } else {
                    self.generate_tree_list(category).unwrap_or_else(|error| {
                        log::warn(format!("generating the {category} trees: {error}"));
                        vec![]
                    })
                }
            }
        };

        self.trees.trees = Arc::new(trees);
        self.trees.trees.clone()
    }

    /// The entities of the list trees follow the display (after a publish, which dropped them, or a change of list or of the vegetation's
    /// visibility).
    pub(crate) fn update_list_trees(&mut self, scene: &mut RenderScene, layer: LayerId, published: bool) {
        let want = (self.display.tree_list.clone(), self.display.hidden_types.contains("VEGETATION_INSTANCE"));
        if !published && self.trees.shown.as_ref() == Some(&want) {
            return;
        }

        if !published {
            scene.remove_range(self.trees.entities.clone());
            self.update_static_tree_models(scene);
        }

        let first = scene.next_id();
        let added = if want.1 { 0 } else { self.add_list_trees(scene, layer) };
        self.trees.entities = first..first + added as EntityId;
        self.trees.shown = Some(want);
    }

    /// The static instances' model entities with the model of their key as the shown list's climate converts it.
    fn update_static_tree_models(&mut self, scene: &mut RenderScene) {
        for &e in &self.instances.clone() {
            if self.first_entity.get(e).is_none_or(|&f| f == 0) || self.entity_count[e] != 1 || self.tscene().type_of(e) != "VEGETATION_INSTANCE" {
                continue;
            }

            let Some(model) = self.vegetation_instance_model(e) else { continue };
            if let Some(Payload::MeshInstance(mi)) = scene.find(self.first_entity[e]).map(|en| en.payload.clone())
                && mi.model != model
            {
                scene.update_payload(self.first_entity[e], Payload::MeshInstance(MeshInstance { model, ..mi }));
            }
        }
    }

    /// Adds the list's trees that are not static instances to the layer; they can't be selected (they belong to the seeds, not to tile.ted).
    fn add_list_trees(&mut self, scene: &mut RenderScene, layer: LayerId) -> usize {
        let trees = self.tree_list();
        if trees.is_empty() {
            return 0;
        }

        let cell = |x: f32, z: f32| ((x / SAME_TREE).round() as i32, (z / SAME_TREE).round() as i32);
        let mut statics = HashSet::new();
        for &e in &self.instances {
            if self.tscene().type_of(e) == "VEGETATION_INSTANCE" && self.shown(e) {
                let p = self.world_position(e);
                statics.insert(cell(p.x, p.z));
            }
        }

        let is_static = |x: f32, z: f32| {
            let (cx, cz) = cell(x, z);
            (-1..=1).any(|dx| (-1..=1).any(|dz| statics.contains(&(cx + dx, cz + dz))))
        };

        // The lists' z runs the other way (z = extent - world z); their heights are in scene units.
        let unit = self.unit() as f32;
        let extent = self.project.extent_z() as f32;
        let mut added = 0;
        for (k, t) in trees.iter().enumerate() {
            let z = extent - t.z;
            if is_static(t.x, z) {
                continue;
            }

            let y = if t.freeform || !self.project.has_height() { t.y / unit } else { (self.project.height_at(f64::from(t.x), f64::from(z)) / self.unit()) as f32 };
            let w = translation(Float3::new(t.x, y, z)) * self.terrain_tilt(t.x, z) * rotation_y(-t.yaw) * scaling(t.scale / unit);
            let seed = ((k as u32).wrapping_mul(2_654_435_761) >> 8) as f32 / 16_777_216.0;
            let mi = MeshInstance { model: t.model.clone(), lod: true, p4: [seed, 1.0, 0.0, 1000.0], ..Default::default() };
            let e = scene.add(layer, w, Payload::MeshInstance(mi), self.source(TERRAIN));
            if let Some(en) = scene.find_mut(e) {
                en.locked = true;
            }

            added += 1;
        }

        added
    }
}

/// The rotation taking the up axis to the normal of a terrain whose height drops by `dx` along +x and `dz` along +z over 1 m.
pub(crate) fn tilt(dx: f32, dz: f32) -> Float4x4 {
    let l = (dx * dx + 1.0 + dz * dz).sqrt();
    let (nx, ny, nz) = (dx / l, 1.0 / l, dz / l);
    let r = (nx * nx + ny * ny).sqrt();
    let (c, s) = (ny / r, -nx / r);

    // Columns: (c, s, 0), the normal, and their cross product.
    let mut t = Float4x4::IDENTITY;
    t.m[0] = [c, nx, s * nz, 0.0];
    t.m[1] = [s, ny, -c * nz, 0.0];
    t.m[2] = [0.0, nz, c * ny - s * nx, 0.0];
    t
}

/// The trees of a decoded list with the game's yaws.
fn list_trees(file: &TreeListFile) -> Vec<ListTree> {
    let mut trees = Vec::new();
    for vector in file.tree_list().tree_list() {
        let model = vector.key().replace('\\', "/").to_lowercase();
        let mut rand = SEED;
        for item in vector.value() {
            rand = rand.wrapping_mul(0x343fd).wrapping_add(0x269ec3);
            trees.push(ListTree {
                model: model.clone(),
                x: *item.x(),
                y: *item.y(),
                z: *item.z(),
                scale: *item.scale(),
                freeform: *item.flags() != 0,
                yaw: (rand >> 16) as f32 * YAW_STEP,
            });
        }
    }

    trees
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tilt_is_a_rotation_to_the_normal() {
        for (dx, dz) in [(0.0, 0.0), (0.7, 0.0), (0.0, -0.4), (0.5, 0.9), (-1.3, 0.2)] {
            let t = tilt(dx, dz);
            let col = |j: usize| Float3::new(t.m[0][j], t.m[1][j], t.m[2][j]);
            let l = (dx * dx + 1.0f32 + dz * dz).sqrt();
            let up = col(1);
            assert!((up.x - dx / l).abs() < 1e-6 && (up.y - 1.0 / l).abs() < 1e-6 && (up.z - dz / l).abs() < 1e-6);
            for i in 0..3 {
                for j in 0..3 {
                    let d = wce_foundation::math::dot(col(i), col(j));
                    assert!((d - if i == j { 1.0 } else { 0.0 }).abs() < 1e-5, "{dx} {dz}: {i}.{j} = {d}");
                }
            }

            // Right-handed: no mirroring of the model.
            let (a, b, c) = (col(0), col(1), col(2));
            let det = a.x * (b.y * c.z - b.z * c.y) - a.y * (b.x * c.z - b.z * c.x) + a.z * (b.x * c.y - b.y * c.x);
            assert!((det - 1.0).abs() < 1e-5);
        }
    }

    #[test]
    fn yaws_follow_the_msvc_lcg() {
        let mut state = SEED;
        state = state.wrapping_mul(0x343fd).wrapping_add(0x269ec3);
        assert_eq!(state, 0x4971_0ae3u32.wrapping_mul(214_013).wrapping_add(2_531_011));
        assert!(((state >> 16) as f32 * YAW_STEP) < std::f32::consts::TAU + 1e-3);
    }
}
