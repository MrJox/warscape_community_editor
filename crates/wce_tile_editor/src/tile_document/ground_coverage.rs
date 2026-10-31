//! The automatic ground coverage of a tile (experimental, `wce_tile::ground_coverage`): what the generator needs of the tile (its terrain,
//! the buildings that block a differential, the rocks' sizes) and its result put into the layer `(AUTOGEN) Ground Coverage`, as one undo
//! entry. The layer has a group per kind of terrain (`Cliffs`, `Coast`, `Riverbanks`); a run of a kind replaces what its group held but
//! its `Brushed` group (the brush's strokes). Each differential gets an instance group in it (`Slope 12`, `Shore 3`, `Bank 5`) with its
//! rocks (buildings, freeform at their sunk height), its plants (trees on the landscape) and the vegetation exclusion polygons around them.
//! The parameters, the seed and the sets' names are kept on the kind's group (`wce_autogen_*`, dropped for TEd), so a later run starts from
//! them.
//!
//! The first version's layer held its `Slope N` groups and its settings itself: they are the cliffs', moved into `Cliffs` by the next run
//! (its settings stay on the layer, unused once `Cliffs` has its own: an attribute can't be taken away).

use std::collections::BTreeMap;
use std::io::Cursor;

use rpfm_lib::files::rigidmodel::RigidModel;
use rpfm_lib::files::{Decodeable, DecodeableExtraData};
use wce_foundation::math::{Double3, Float3};
use wce_tile::new_instances::NewInstance;
use wce_tile::ground_coverage::{AVOIDED_CATEGORIES, Blocker, Differential, Grid, Kind, Params, RockSize};
use wce_tile::world_transform::{set_world_position, world_transform};

use super::draft::{turned, with_root_attr};
use super::edits::{point_instance, scene_point};
use super::index::{destruct_number, static_piece};
use super::TileDocument;

/// The layer the coverage goes into.
pub const COVERAGE_LAYER: &str = "(AUTOGEN) Ground Coverage";

/// The group of the brush's strokes in a kind's group: a run of the kind keeps it.
pub const BRUSHED_GROUP: &str = "Brushed";

/// A kind's group in the layer.
pub fn kind_group_name(kind: Kind) -> &'static str {
    match kind {
        Kind::Cliffs => "Cliffs",
        Kind::Coast => "Coast",
        Kind::Riverbank => "Riverbanks",
    }
}

/// What a differential of the kind is called (its group: `Slope 12`).
pub fn region_name(kind: Kind) -> &'static str {
    match kind {
        Kind::Cliffs => "Slope",
        Kind::Coast => "Shore",
        Kind::Riverbank => "Bank",
    }
}

/// A kind group's attributes (the first version's layer's, but the preset): the parameters (`Params::to_text`), the seed, the rock set's
/// and the plant set's names, and the preset's (`ground_coverage_presets`) the values came from.
pub const PARAMS_ATTR: &str = "wce_autogen_params";
pub const SEED_ATTR: &str = "wce_autogen_seed";
pub const ROCKS_ATTR: &str = "wce_autogen_rocks";
pub const PLANTS_ATTR: &str = "wce_autogen_plants";
pub const PRESET_ATTR: &str = "wce_autogen_preset";

/// What a run used, as kept on the layer.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct CoverageSettings {
    pub params: Params,
    pub seed: u64,
    pub rocks: String,
    pub plants: String,

    /// The preset the values came from (they may have been changed since); empty: none known (the first version's).
    pub preset: String,
}

/// What a run put in.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct CoverageCounts {
    pub slopes: usize,
    pub rocks: usize,
    pub plants: usize,
    pub exclusions: usize,
}

/// Metres between the terrain samples of the generator.
pub const GRID_CELL: f32 = 1.0;

impl TileDocument {
    /// The coverage layer, when the tile has one (found by its name).
    pub fn coverage_layer(&self) -> Option<usize> {
        let sc = self.tile().tscene();
        sc.find_all("LayerInstance").into_iter().find(|&l| sc.name(l) == COVERAGE_LAYER)
    }

    /// A kind's group in the coverage layer.
    pub fn coverage_group(&self, kind: Kind) -> Option<usize> {
        let sc = self.tile().tscene();
        sc.children(self.coverage_layer()?).into_iter().find(|&g| sc.type_of(g) == "UserInstanceGroup" && sc.name(g) == kind_group_name(kind))
    }

    /// What a run of the kind replaces: its group's children but its brushed group, and for the cliffs the first version's groups.
    fn coverage_replaced(&self, kind: Kind) -> Vec<usize> {
        let sc = self.tile().tscene();
        let mut out: Vec<usize> = self.coverage_group(kind).map(|g| sc.children(g).into_iter().filter(|&c| sc.name(c) != BRUSHED_GROUP).collect()).unwrap_or_default();
        if kind == Kind::Cliffs {
            out.extend(self.coverage_old());
        }

        out
    }

    /// The first version's groups: the layer's `Slope N` groups (what is put in the layer by hand is left alone).
    fn coverage_old(&self) -> Vec<usize> {
        let sc = self.tile().tscene();
        let Some(l) = self.coverage_layer() else { return vec![] };
        let old = |c: usize| sc.type_of(c) == "UserInstanceGroup" && sc.name(c).strip_prefix("Slope ").is_some_and(|n| n.parse::<usize>().is_ok());
        sc.children(l).into_iter().filter(|&c| old(c)).collect()
    }

    /// How many instances a run of the kind replaces.
    pub fn coverage_items(&self, kind: Kind) -> usize {
        let replaced = self.coverage_replaced(kind);
        if replaced.is_empty() { 0 } else { self.tile().instances_below(&replaced).len() }
    }

    /// What the last run of the kind used, from its group's attributes (the cliffs': else the first version's layer's).
    pub fn coverage_settings(&self, kind: Kind) -> Option<CoverageSettings> {
        let l = match self.coverage_group(kind).filter(|&g| self.tile().tscene().doc().get(g, PARAMS_ATTR).is_some()) {
            Some(g) => g,
            None if kind == Kind::Cliffs => self.coverage_layer()?,
            None => return None,
        };

        let doc = self.tile().tscene().doc();
        let params = doc.get(l, PARAMS_ATTR)?;
        Some(CoverageSettings {
            params: Params::from_text_for(kind, &params),
            seed: doc.get(l, SEED_ATTR).and_then(|s| s.parse().ok()).unwrap_or(0),
            rocks: doc.get(l, ROCKS_ATTR).unwrap_or_default(),
            plants: doc.get(l, PLANTS_ATTR).unwrap_or_default(),
            preset: doc.get(l, PRESET_ATTR).unwrap_or_default(),
        })
    }

    /// The terrain's heights over the whole tile, [`GRID_CELL`] metres apart.
    pub fn coverage_grid(&self) -> Grid {
        let t = self.tile();
        Grid::sample(0.0, 0.0, t.extent_x() as f32, t.extent_z() as f32, GRID_CELL, |x, z| t.terrain_height(x, z))
    }

    /// The buildings the coverage goes around: the ones outside the coverage layer (its rocks are replaced) whose category (`category`:
    /// the packs' tables, else the kit's) is one of [`AVOIDED_CATEGORIES`]. A footprint whose model has not loaded yet takes its radius
    /// from the model's files (read once per key: a settlement has thousands of instances of a few).
    pub fn coverage_blockers(&self, category: &dyn Fn(&str) -> Option<String>) -> Vec<Blocker> {
        let sc = self.tile().tscene();
        let layer = self.coverage_layer();
        let kit = self.tile().building_db.get_or_init(|| wce_tile::building_db::BuildingDb::load_near(&self.project().dir));
        let mut sizes = std::collections::HashMap::<String, Option<RockSize>>::new();
        self.footprints().into_iter()
            .filter(|f| sc.type_of(f.elem) == "BUILDING_INSTANCE" && layer.is_none_or(|l| !sc.is_descendant(f.elem, l)))
            .filter_map(|f| {
                let key = sc.doc().get(f.elem, "key").unwrap_or_default();
                let cat = category(&key).or_else(|| kit.category(&key).map(str::to_owned))?;
                if !AVOIDED_CATEGORIES.contains(&cat.as_str()) {
                    return None;
                }

                let radius = if f.radius > 0.0 {
                    f.radius
                } else {
                    let s = sc.scale(f.elem).map_or(1.0, |s| s.x.abs().max(s.z.abs()) as f32);
                    let size = *sizes.entry(key.clone()).or_insert_with(|| self.rock_size(&key));
                    size.map_or(0.0, |z| z.radius * s)
                };

                Some(Blocker { x: f.x, z: f.z, radius })
            })
            .collect()
    }

    /// The size of a building's model (metres, at scale 1): its loaded bounds, else its pieces' file bounds read now; `None` when it has no
    /// model.
    pub fn rock_size(&self, key: &str) -> Option<RockSize> {
        let t = self.tile();
        let (lo, hi) = match t.building_model(key).filter(|m| m.ok && m.resolved) {
            Some(m) => (m.bmin, m.bmax),
            None => self.model_file_bounds(key)?,
        };

        let unit = t.unit() as f32;
        let across = [lo.x.abs(), hi.x.abs(), lo.z.abs(), hi.z.abs()].into_iter().fold(0.0f32, f32::max);
        Some(RockSize { radius: across / unit, height: (hi.y - lo.y).max(0.0) / unit })
    }

    /// The union of the bounds of a building's standing pieces (their files' mesh boxes, LOD 0).
    fn model_file_bounds(&self, key: &str) -> Option<(Float3, Float3)> {
        let files = self.tile().files.as_deref()?;
        let mut lo = Float3::new(f32::MAX, f32::MAX, f32::MAX);
        let mut hi = Float3::new(f32::MIN, f32::MIN, f32::MIN);
        let mut any = false;
        for path in files.list(&format!("rigidmodels/buildings/{key}/"), ".rigid_model_v2") {
            let file = &path[path.rfind('/').map_or(0, |s| s + 1)..];
            if !static_piece(file) || (file.contains("destruct") && destruct_number(file) != 1) {
                continue;
            }

            let Ok(model) = files.read(&path).and_then(|b| RigidModel::decode(&mut Cursor::new(&b), &Some(DecodeableExtraData::default())).map_err(|e| e.to_string())) else {
                continue;
            };

            for block in model.lods().first().map(|l| l.mesh_blocks().as_slice()).unwrap_or_default() {
                let (a, b) = (block.mesh().min_bb(), block.mesh().max_bb());
                lo = Float3::new(lo.x.min(a.x), lo.y.min(a.y), lo.z.min(a.z));
                hi = Float3::new(hi.x.max(b.x), hi.y.max(b.y), hi.z.max(b.z));
                any = true;
            }
        }

        any.then_some((lo, hi))
    }

    /// Puts a run's result into the kind's group of the coverage layer (each made when missing: the layer at the end of the scene graph,
    /// the group at the end of the layer), replacing what the group held but its brushed group, and keeps the settings on the group. The
    /// first version's groups in the layer are moved into `Cliffs` first (a run of the cliffs replaces them). One undo entry; nothing is
    /// selected afterwards (the layer's outline would hide what is in it).
    ///
    /// # Errors
    ///
    /// The tile has no scene graph, or a text does not read (nothing changes).
    pub fn apply_coverage(&mut self, kind: Kind, result: &[Differential], settings: &CoverageSettings) -> Result<CoverageCounts, String> {
        let root = self.tile().tscene().scene_graph().ok_or("this tile has no scene graph")?;
        let texts = self.coverage_texts(result)?;
        let mark = self.begin()?;
        let outcome = (|| -> Result<CoverageCounts, String> {
            if self.coverage_layer().is_none() {
                self.tscene_mut().add_layer(Some(root), COVERAGE_LAYER)?;
            }

            // The first version's groups: the cliffs'.
            let old = self.coverage_old();
            if !old.is_empty() {
                let cliffs = self.kind_group(Kind::Cliffs)?;
                self.tscene_mut().move_elements(old, cliffs, None)?;
            }

            // The group with its brushed strokes only (the indices move as the scene changes: found again each time).
            let group = self.kind_group(kind)?;
            let sc = self.tscene_mut();
            sc.set_attr(group, PARAMS_ATTR, &settings.params.to_text());
            sc.set_attr(group, SEED_ATTR, &settings.seed.to_string());
            sc.set_attr(group, ROCKS_ATTR, &settings.rocks);
            sc.set_attr(group, PLANTS_ATTR, &settings.plants);
            sc.set_attr(group, PRESET_ATTR, &settings.preset);
            let brushed = sc.children(group).into_iter().find(|&c| sc.name(c) == BRUSHED_GROUP).map(|b| sc.element_text(b));
            sc.set_children_text(group, b"")?;
            if let Some(text) = brushed {
                sc.insert_element_text(group, &text)?;
            }

            let mut counts = CoverageCounts::default();
            for (d, (items, polygons)) in result.iter().filter(|d| d.kept()).zip(texts) {
                if items.is_empty() && polygons.is_empty() {
                    continue;
                }

                // The group at the middle of what it holds (its pivot), every item at its world point.
                let unit = self.tile().unit();
                let n = items.len().max(1) as f32;
                let mid = items.iter().fold(Float3::new(0.0, 0.0, 0.0), |a, (_, p)| Float3::new(a.x + p.x / n, a.y + p.y / n, a.z + p.z / n));
                let parent = self.coverage_group(kind).ok_or("the kind's group went")?;
                let lw = world_transform(self.tile().tscene(), parent);
                let local = lw.rotation.transpose() * (scene_point(mid, unit) - lw.position);
                let sc = self.tscene_mut();
                let group = sc.add_group(Some(parent), &format!("{} {}", region_name(kind), d.number), local)?;
                for (text, at) in &items {
                    let e = sc.insert_element_text(group, text)?;
                    set_world_position(sc, e, scene_point(*at, unit));
                }

                let gw = world_transform(sc, group);
                for pts in &polygons {
                    let local = pts.iter().map(|&p| gw.rotation.transpose() * (scene_point(p, unit) - gw.position)).collect::<Vec<Double3>>();
                    sc.add_polygon(Some(group), "VegetationExclusionPolygonInstance", "Polygon Instance", &local, &BTreeMap::new())?;
                }

                counts.slopes += 1;
                counts.rocks += d.rocks.len();
                counts.plants += d.plants.len();
                counts.exclusions += polygons.len();
            }

            Ok(counts)
        })();

        match outcome {
            Ok(counts) => {
                self.push_splice("Generate ground coverage".to_owned(), mark, vec![], vec![]);
                Ok(counts)
            }

            Err(err) => {
                let record = self.tscene_mut().end_edit(mark).unwrap_or_default();
                let _ = self.tscene_mut().unapply(&record);
                Err(err)
            }
        }
    }

    /// The kind's group in the coverage layer (which must be there), made at the end of the layer when missing.
    fn kind_group(&mut self, kind: Kind) -> Result<usize, String> {
        if let Some(g) = self.coverage_group(kind) {
            return Ok(g);
        }

        let layer = self.coverage_layer().ok_or("no coverage layer")?;
        self.tscene_mut().add_group(Some(layer), kind_group_name(kind), Double3::ZERO)?;
        self.coverage_group(kind).ok_or_else(|| "the kind's group was not made".to_owned())
    }

    /// The texts of the kept differentials, each: its rocks and plants (text at the origin, world point) and its polygons (world points).
    #[allow(clippy::type_complexity)]
    fn coverage_texts(&self, result: &[Differential]) -> Result<Vec<(Vec<(Vec<u8>, Float3)>, Vec<Vec<Float3>>)>, String> {
        let t = self.tile();
        let unit = t.unit();
        let text = |kind: &str, key: &str| -> Result<Vec<u8>, String> {
            let n = match kind {
                // The kit's defaults, named after its key as TEd names a placed building.
                "BUILDING_INSTANCE" => NewInstance::new(kind).position(Double3::ZERO).attr("key", key).attr("name", key),
                _ => point_instance(kind, Double3::ZERO, key, unit)?,
            };

            wce_tile::new_instances::element_text(&n)
        };

        // f32's shortest text (1.3, not 1.2999999523162842).
        let num = |v: f32| v.to_string();
        let mut out = vec![];
        for d in result.iter().filter(|d| d.kept()) {
            let mut items = vec![];
            for r in &d.rocks {
                let mut s = with_root_attr(&text("BUILDING_INSTANCE", &r.key)?, "surface_mode", "MSM_FREEFORM");
                if r.scale != 1.0 {
                    s = with_root_attr(&s, wce_tile::project_files::SCALE_ATTR, &num(r.scale));
                }

                items.push((turned(&s, r.yaw), Float3::new(r.x, r.y, r.z)));
            }

            for p in &d.plants {
                let s = with_root_attr(&text("VEGETATION_INSTANCE", &p.key)?, "scale", &num(p.scale));
                items.push((turned(&s, p.yaw), Float3::new(p.x, p.y, p.z)));
            }

            let polygons = d.exclusions.iter().map(|poly| poly.iter().map(|&(x, z)| Float3::new(x, t.terrain_height(x, z), z)).collect()).collect();
            out.push((items, polygons));
        }

        Ok(out)
    }
}
