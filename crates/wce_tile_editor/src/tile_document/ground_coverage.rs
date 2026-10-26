//! The automatic ground coverage of a tile (experimental, `wce_tile::ground_coverage`): what the generator needs of the tile (its terrain,
//! the buildings that block a differential, the rocks' sizes) and its result put into the layer `(AUTOGEN) Ground Coverage`, replacing what
//! the layer held, as one undo entry. Each differential gets an instance group (`Slope 12`) with its rocks (buildings, freeform at their
//! sunk height), its plants (trees on the landscape) and the vegetation exclusion polygons around them. The parameters, the seed and the
//! sets' names are kept on the layer (`wce_autogen_*`, dropped for TEd), so a later run starts from them.

use std::collections::BTreeMap;
use std::io::Cursor;

use rpfm_lib::files::rigidmodel::RigidModel;
use rpfm_lib::files::{Decodeable, DecodeableExtraData};
use wce_foundation::math::{Double3, Float3};
use wce_tile::new_instances::NewInstance;
use wce_tile::ground_coverage::{Blocker, Differential, Grid, Params, RockSize};
use wce_tile::world_transform::{set_world_position, world_transform};

use super::draft::{turned, with_root_attr};
use super::edits::{point_instance, scene_point};
use super::index::{destruct_number, static_piece};
use super::{TileDocument, item_of};

/// The layer the coverage goes into.
pub const COVERAGE_LAYER: &str = "(AUTOGEN) Ground Coverage";

/// The layer's attributes: the parameters (`Params::to_text`), the seed, the rock set's and the plant set's names.
pub const PARAMS_ATTR: &str = "wce_autogen_params";
pub const SEED_ATTR: &str = "wce_autogen_seed";
pub const ROCKS_ATTR: &str = "wce_autogen_rocks";
pub const PLANTS_ATTR: &str = "wce_autogen_plants";

/// What a run used, as kept on the layer.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct CoverageSettings {
    pub params: Params,
    pub seed: u64,
    pub rocks: String,
    pub plants: String,
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

    /// How many instances the coverage layer holds (what a run replaces).
    pub fn coverage_items(&self) -> usize {
        self.coverage_layer().map_or(0, |l| self.tile().instances_below(&[l]).len())
    }

    /// What the last run used, from the layer's attributes.
    pub fn coverage_settings(&self) -> Option<CoverageSettings> {
        let l = self.coverage_layer()?;
        let doc = self.tile().tscene().doc();
        let params = doc.get(l, PARAMS_ATTR)?;
        Some(CoverageSettings {
            params: Params::from_text(&params),
            seed: doc.get(l, SEED_ATTR).and_then(|s| s.parse().ok()).unwrap_or(0),
            rocks: doc.get(l, ROCKS_ATTR).unwrap_or_default(),
            plants: doc.get(l, PLANTS_ATTR).unwrap_or_default(),
        })
    }

    /// The terrain's heights over the whole tile, [`GRID_CELL`] metres apart.
    pub fn coverage_grid(&self) -> Grid {
        let t = self.tile();
        Grid::sample(0.0, 0.0, t.extent_x() as f32, t.extent_z() as f32, GRID_CELL, |x, z| t.terrain_height(x, z))
    }

    /// The buildings that leave a differential alone: every one outside the coverage layer (its rocks are replaced).
    pub fn coverage_blockers(&self) -> Vec<Blocker> {
        let sc = self.tile().tscene();
        let layer = self.coverage_layer();
        self.footprints().into_iter()
            .filter(|f| sc.type_of(f.elem) == "BUILDING_INSTANCE" && layer.is_none_or(|l| !sc.is_descendant(f.elem, l)))
            .map(|f| Blocker { x: f.x, z: f.z, radius: f.radius })
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

    /// Puts a run's result into the coverage layer (made at the end of the scene graph when the tile has none), replacing what it held, and
    /// keeps the settings on it. One undo entry; the selection afterwards is the layer.
    ///
    /// # Errors
    ///
    /// The tile has no scene graph, or a text does not read (nothing changes).
    pub fn apply_coverage(&mut self, result: &[Differential], settings: &CoverageSettings) -> Result<CoverageCounts, String> {
        let existing = self.coverage_layer();
        let root = self.tile().tscene().scene_graph().ok_or("this tile has no scene graph")?;
        let texts = self.coverage_texts(result)?;
        let mark = self.begin()?;
        let outcome = (|| -> Result<(usize, CoverageCounts), String> {
            let layer = match existing {
                Some(l) => l,
                None => self.tscene_mut().add_layer(Some(root), COVERAGE_LAYER)?,
            };

            let sc = self.tscene_mut();
            sc.set_attr(layer, PARAMS_ATTR, &settings.params.to_text());
            sc.set_attr(layer, SEED_ATTR, &settings.seed.to_string());
            sc.set_attr(layer, ROCKS_ATTR, &settings.rocks);
            sc.set_attr(layer, PLANTS_ATTR, &settings.plants);
            sc.set_children_text(layer, b"")?;

            let mut counts = CoverageCounts::default();
            for (d, (items, polygons)) in result.iter().filter(|d| d.kept()).zip(texts) {
                if items.is_empty() && polygons.is_empty() {
                    continue;
                }

                // The group at the middle of what it holds (its pivot), every item at its world point.
                let unit = self.tile().unit();
                let n = items.len().max(1) as f32;
                let mid = items.iter().fold(Float3::new(0.0, 0.0, 0.0), |a, (_, p)| Float3::new(a.x + p.x / n, a.y + p.y / n, a.z + p.z / n));
                let lw = world_transform(self.tile().tscene(), layer);
                let local = lw.rotation.transpose() * (scene_point(mid, unit) - lw.position);
                let sc = self.tscene_mut();
                let group = sc.add_group(Some(layer), &format!("Slope {}", d.number), local)?;
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

            Ok((layer, counts))
        })();

        match outcome {
            Ok((layer, counts)) => {
                self.push_splice("Generate ground coverage".to_owned(), mark, vec![], vec![item_of(layer)]);
                Ok(counts)
            }

            Err(err) => {
                let record = self.tscene_mut().end_edit(mark).unwrap_or_default();
                let _ = self.tscene_mut().unapply(&record);
                Err(err)
            }
        }
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
