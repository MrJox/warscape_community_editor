//! The tile's procedural trees: generated as BOB would (`wce_tile::vegetation`) for the terrain look's climate, from the tile's seeds,
//! height map, saved blend map, exclusion polygons and static trees and the Assembly Kit's vegetation parameters, and drawn with the static
//! VEGETATION_INSTANCEs. The packed tile's lists (`<list>.tree_list.bin`, BOB's from before) are not drawn: their generated trees came from
//! seeds a decompiled tile does not have. A generated list holds the static trees too (those inside an exclusion polygon, hidden ones
//! included), so a list tree standing where a static instance stands is skipped.
//!
//! How the game places a list tree (empire.retail.dll FUN_10429b80, docs/re/vegetation.md): at the stored x and z, with the stored height
//! when the tree is freeform and on the terrain otherwise, scaled by the scale byte / 127, turned by a random yaw. The yaw comes from the
//! MSVC rand LCG seeded with 0x49710ae3 at the start of every list (of every worker's share of it, so the game's yaws depend on its thread
//! count; this is the one-worker sequence). Every tree is tilted to the terrain: its up axis goes along the terrain normal measured 0.5 m around it.
//! The static VEGETATION_INSTANCEs are list trees in the game, so they are drawn tilted too, and with the model the list's climate
//! converts their key to (`per_climate_tree_conversions.xml`, as BOB writes them into the list).
//!
//! The packed lists are still read to convert a decompiled project's whole-tile exclusion polygon (`static_cover`).

use std::collections::{BTreeMap, HashSet};
use std::io::Cursor;
use std::sync::Arc;

use rpfm_lib::files::Decodeable;
use rpfm_lib::files::fastbin_lists::{TREE_LIST_EXTENSION, TreeListFile};
use wce_formats::old_lists::{OldList, OldListKind};
use wce_foundation::log;
use wce_foundation::math::{Float3, Float4x4, rotation_y, scaling, translation};
use wce_scene::render_scene::{EntityId, LayerId, MeshInstance, Payload, RenderScene};

use wce_foundation::math::Double3;
use wce_tile::ted_scene::TedScene;
use wce_tile::tile_project::TileProject;
use wce_tile::vegetation::{self, GenerationParams};
use wce_tile::world_transform::world_transform;

use super::{TERRAIN, Tile, TileDocument};

/// The root attribute saying the packed lists are mirrored (`Tile::list_flip`).
pub const LIST_FLIP_ATTR: &str = "wce_list_flip";

/// The category generated for a climate the kit has none for: BOB's list for no particular climate.
pub const DEFAULT_TREE_LIST: &str = "default";

const SEED: u32 = 0x4971_0ae3;

/// The older tree lists' extension (Rome 2's tiles have no `.tree_list.bin`).
const OLD_TREE_LIST_EXTENSION: &str = ".tree_list";

/// The folder of a decompiled project its tile's vegetation files are carried in (`wce_tile_io::attila::stage_c::CARRIED_DIR`).
pub(crate) const CARRIED_DIR: &str = "decompiled_vegetation";

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

    /// A placed tree's piece without the tilt, as its instance's POSITION5..7 ([`untilted_rows`]).
    pub(crate) fn untilted_piece_rows(&self, elem: usize, piece: usize) -> [[f32; 4]; 3] {
        let w = self.world_matrix(elem);
        untilted_rows(&self.piece_props.get(&(elem, piece)).map_or(w, |p| w * super::view::piece_local(p)))
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

    /// The tree categories the kit's generator has (`default`, the climate keys), empty without its parameters.
    pub fn tree_categories(&self) -> Vec<String> {
        self.generation_params().map(|p| p.categories.iter().map(|c| c.to_lowercase()).collect()).unwrap_or_default()
    }

    /// The category the trees are generated for: the display's choice, else the terrain look's climate as the game picks it (its own list
    /// when the kit has one, else `default`). None without the kit's parameters.
    pub fn tree_category(&self) -> Option<String> {
        let params = self.generation_params()?;
        let want = if self.display.tree_category.is_empty() { self.look.climate.to_lowercase() } else { self.display.tree_category.to_lowercase() };
        Some(if params.category(&want).is_some() { want } else { DEFAULT_TREE_LIST.to_owned() })
    }

    /// Why the trees can't be generated; None when they can.
    pub fn trees_unavailable(&self) -> Option<&'static str> {
        if self.generation_params().is_none() {
            Some("no Assembly Kit vegetation parameters (Rome 2's kit has none)")
        } else if self.project.raw_root.is_none() {
            Some("the tile is not under an Assembly Kit's raw_data")
        } else if vegetation::seeds(self.tscene()).is_none() {
            Some("tile.ted has no <Vegetation> seeds")
        } else {
            None
        }
    }

    /// The climate whose conversions the static trees are drawn with: the trees' category (none for `default`), whether they are drawn or not.
    pub(crate) fn vegetation_climate(&self) -> String {
        self.tree_category().unwrap_or_default()
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
    /// Whether the tile's packed tree and grass lists (BOB's, made before) are mirrored along x and along z: the tile was flipped whole
    /// since (`TileDocument::flip_tile`, the root's `wce_list_flip`: "x", "z" or "xz"). Lists generated now follow the tile as it is.
    pub(crate) fn list_flip(&self) -> (bool, bool) {
        let sc = self.tscene();
        let root = sc.doc().roots()[0];
        let v = sc.doc().get(root, LIST_FLIP_ATTR).unwrap_or_default();
        (v.contains('x'), v.contains('z'))
    }

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

    /// The trees generated for the display's list (a category), once per list; empty when it is off or can't be generated.
    pub(crate) fn tree_list(&mut self) -> Arc<Vec<ListTree>> {
        let name = self.display.tree_list.clone();
        if name.is_empty() {
            return Arc::default();
        }

        if self.trees.name == name {
            return self.trees.trees.clone();
        }

        self.trees.name.clone_from(&name);
        let trees = match self.trees_unavailable() {
            Some(why) => {
                log::info(format!("procedural trees: {why}"));
                vec![]
            }
            None => self.generate_tree_list(&name).unwrap_or_else(|error| {
                log::warn(format!("generating the {name} trees: {error}"));
                vec![]
            }),
        };

        self.trees.trees = Arc::new(trees);
        self.trees.trees.clone()
    }

    /// The entities of the list trees follow the display (after a publish, which dropped them, or a change of list or of the vegetation's
    /// visibility).
    pub(crate) fn update_list_trees(&mut self, scene: &mut RenderScene, layer: LayerId, published: bool) {
        // The trees of the terrain look's climate (or the one chosen), while they are on.
        let list = if self.display.show_trees { self.tree_category().unwrap_or_default() } else { String::new() };
        if self.display.tree_list != list {
            self.display.tree_list = list;
        }

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

    /// Adds the list's trees that are not static instances (shown or hidden) to the layer; they can't be selected (they belong to the seeds,
    /// not to tile.ted).
    fn add_list_trees(&mut self, scene: &mut RenderScene, layer: LayerId) -> usize {
        let trees = self.tree_list();
        if trees.is_empty() {
            return 0;
        }

        let cell = |x: f32, z: f32| ((x / SAME_TREE).round() as i32, (z / SAME_TREE).round() as i32);
        let mut statics = HashSet::new();
        for &e in &self.instances {
            // Hidden ones too: their list trees are the same trees, hidden with them.
            if self.tscene().type_of(e) == "VEGETATION_INSTANCE" {
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

            // Not on a road: a tile's packed list was made before it, and the blend map alone need not rule its trees out (as grass).
            if self.road_at(f64::from(t.x), f64::from(z)) {
                continue;
            }

            let y = if t.freeform || !self.project.has_height() { t.y / unit } else { (self.project.height_at(f64::from(t.x), f64::from(z)) / self.unit()) as f32 };
            let at = translation(Float3::new(t.x, y, z));
            let turn = rotation_y(-t.yaw) * scaling(t.scale / unit);
            let w = at * self.terrain_tilt(t.x, z) * turn;
            let seed = ((k as u32).wrapping_mul(2_654_435_761) >> 8) as f32 / 16_777_216.0;
            let [p5, p6, p7] = untilted_rows(&(at * turn));
            let mi = MeshInstance { model: t.model.clone(), lod: true, p4: [seed, 1.0, 0.0, 1000.0], p5, p6, p7, ..Default::default() };
            let e = scene.add(layer, w, Payload::MeshInstance(mi), self.source(TERRAIN));
            if let Some(en) = scene.find_mut(e) {
                en.locked = true;
            }

            added += 1;
        }

        added
    }
}

/// A decompiled project's exclusion polygon over the whole tile (the decompiler's until 2026-11-12: BOB kept the static trees and generated
/// nothing) replaced by polygons around its static trees (`wce_tile::vegetation::static_cover`, without the squares where the carried lists
/// have generated trees), so that the trees are generated around them. Done on the project as it opens, before the document is made from
/// it (the document's tables and the drawing then start from the new scene; the document says it is modified,
/// `TileDocument::set_converted_on_open`). How many polygons it made; None when the tile is not a decompiled one or has no such polygon.
///
/// # Errors
///
/// The scene's edit failed or does not read again.
pub fn convert_whole_tile_exclusion(project: &mut TileProject) -> Result<Option<usize>, String> {
    let dir = project.dir.join(CARRIED_DIR);
    if !dir.is_dir() {
        return Ok(None);
    }

    let unit = project.unit_scale;
    let (ex, ez) = (project.extent_x(), project.extent_z());
    let scene = &project.scene;

    // The decompiler's rectangle (4 points), not an outline of the trees that happens to reach every edge.
    let whole = vegetation::exclusion_outlines(scene).into_iter().filter(|(_, pts)| pts.len() == 4).find(|(_, pts)| {
        let (mut lo, mut hi) = ((f64::MAX, f64::MAX), (f64::MIN, f64::MIN));
        for &(x, z) in pts {
            let (x, z) = (f64::from(x) / unit, f64::from(z) / unit);
            lo = (lo.0.min(x), lo.1.min(z));
            hi = (hi.0.max(x), hi.1.max(z));
        }

        lo.0 <= 0.0 && lo.1 <= 0.0 && hi.0 >= ex && hi.1 >= ez
    });
    let Some((elem, _)) = whole else { return Ok(None) };
    let doc = scene.doc();
    let layer = std::iter::successors(Some(elem), |&e| doc.elements()[e].parent)
        .find(|&e| scene.is_layer(e))
        .or_else(|| scene.scene_graph())
        .ok_or("the tile's exclusion polygon is in no layer")?;

    // The static trees (metres), and the carried lists' trees that are none of them: their generated ones.
    let statics = (0..doc.elements().len())
        .filter(|&e| doc.elements()[e].name == "VEGETATION_INSTANCE")
        .map(|e| {
            let p = world_transform(scene, e).position;
            (p.x / unit, p.z / unit)
        })
        .collect::<Vec<_>>();
    let key = |x: f64, z: f64| ((x * 20.0).round() as i64, (z * 20.0).round() as i64);
    let at = statics.iter().map(|&(x, z)| key(x, z)).collect::<HashSet<_>>();
    let is_static = |x: f64, z: f64| {
        let (kx, kz) = key(x, z);
        (-1..=1).any(|dx| (-1..=1).any(|dz| at.contains(&(kx + dx, kz + dz))))
    };

    let flip = doc.get(doc.roots()[0], LIST_FLIP_ATTR).unwrap_or_default();
    let (fx, fz) = (flip.contains('x'), flip.contains('z'));
    let mut generated = vec![];
    for entry in std::fs::read_dir(&dir).map_err(|e| e.to_string())?.flatten() {
        let name = entry.file_name().to_string_lossy().to_lowercase();
        let Ok(bytes) = std::fs::read(entry.path()) else { continue };
        let trees = if name.ends_with(TREE_LIST_EXTENSION) {
            TreeListFile::decode(&mut Cursor::new(bytes), &None).map(|f| list_trees(&f)).ok()
        } else if name.ends_with(OLD_TREE_LIST_EXTENSION) {
            OldList::parse(OldListKind::Tree, &bytes).map(|l| old_list_trees(&l)).ok()
        } else {
            None
        };

        for tree in trees.unwrap_or_default() {
            // The list's z runs from the far edge; mirrored as the tile was since (`list_flip`).
            let x = if fx { ex - f64::from(tree.x) } else { f64::from(tree.x) };
            let z = if fz { f64::from(tree.z) } else { ez - f64::from(tree.z) };
            if !is_static(x, z) {
                generated.push((x, z));
            }
        }
    }

    let cover = vegetation::static_cover::static_cover(&statics, &generated, (ex, ez));
    let w = world_transform(scene, layer);
    let to_layer = |x: f64, z: f64| w.rotation.transpose() * (Double3::new(x * unit, 0.0, z * unit) - w.position);
    let local = cover.iter().map(|outline| outline.iter().map(|&(x, z)| to_layer(x, z)).collect::<Vec<_>>()).collect::<Vec<_>>();

    // On a copy (the project is left as it was when this fails). The polygon goes first (its layer, before it, keeps its index), then the
    // new ones at the layer's end; the scene read again, as the tile opens (`TileProject::make_references`).
    let mut edited = TedScene::parse(project.scene.save())?;
    edited.remove(elem)?;
    for pts in &local {
        edited.add_polygon(Some(layer), "VegetationExclusionPolygonInstance", "Static trees", pts, &BTreeMap::new())?;
    }

    project.scene = TedScene::parse(edited.save())?;
    Ok(Some(cover.len()))
}

impl TileDocument {
    /// The project was converted as it opened (`convert_whole_tile_exclusion`): the document says it is modified until it is saved.
    pub fn set_converted_on_open(&mut self) {
        self.tile_mut().converted_on_open = true;
    }
}

/// A tree's untilted matrix as its instance's POSITION5..7 (the rows): the tree shaders blend it with the tilted world matrix per vertex
/// (the game's `P2 * yaw * scale` beside `P1 * tilt * yaw * scale`, FUN_10429b80), so the trunk stays upright on a slope and only the
/// roots follow it (docs/re/vegetation.md, "Placement").
pub(crate) fn untilted_rows(m: &Float4x4) -> [[f32; 4]; 3] {
    [m.m[0], m.m[1], m.m[2]]
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

/// The trees of an older list (Rome 2's), as [`list_trees`]: a yaw sequence per model, none freeform.
fn old_list_trees(list: &OldList) -> Vec<ListTree> {
    let mut trees = Vec::with_capacity(list.len());
    for (t, (name, first)) in list.types.iter().enumerate() {
        let model = String::from_utf8_lossy(name).replace('\\', "/").to_lowercase();
        let end = list.types.get(t + 1).map_or(list.len(), |(_, next)| *next as usize);
        let mut rand = SEED;
        for i in *first as usize..end.min(list.len()) {
            rand = rand.wrapping_mul(0x343fd).wrapping_add(0x269ec3);
            let [x, y, z] = list.position(i);
            trees.push(ListTree { model: model.clone(), x, y, z, scale: list.scale(i).unwrap_or(1.0), freeform: false, yaw: (rand >> 16) as f32 * YAW_STEP });
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
