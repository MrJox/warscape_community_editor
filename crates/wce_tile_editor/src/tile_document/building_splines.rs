//! Building splines in the document (`wce_tile::building_spline`): a `WceBuildingSpline` is stored like a road (a `<points>` list whose points
//! may carry handles), so the polygon tools edit it; here its buildings are laid along its curve and drawn. They are derived data: laid
//! again whenever the document, the ground or the buildings' models change, never saved in tile.wce, written as plain buildings in the layer
//! [`KIT_LAYER`] by an export. A click on one picks its spline.

use std::collections::HashMap;

use wce_foundation::math::Float3;
use wce_scene::render_scene::{EntityId, RenderScene};
use wce_tile::building_spline::{Footprint, KEYS, Settings, instance_text, lay, parse_keys};
use wce_tile::roads::centre_line_handled;
use wce_tile::ted_scene::TedScene;

use super::Tile;
pub use wce_tile::building_spline::BUILDING_SPLINE;

/// The layer of an export that holds the building splines' buildings.
pub const KIT_LAYER: &str = "WCE building splines";

/// A building laid along a spline: the spline, its text (scene units) and where it stands (scene units).
pub(crate) struct SplineItem {
    spline: usize,
    text: Vec<u8>,
    position: [f64; 3],
}

/// The building splines' state: their buildings, the entities drawing them, and what they were laid for.
#[derive(Default)]
pub(crate) struct BuildingSplines {
    items: Vec<SplineItem>,
    entities: Vec<EntityId>,
    shown: bool,

    /// What the buildings were laid for: the document's version, the models known, the ground.
    stamp: Option<u64>,

    /// Each building's footprint (metres, from its pivot), when found from its files (its logic file or its model).
    extents: std::cell::RefCell<HashMap<String, Option<Footprint>>>,
}

impl Tile {
    pub fn is_building_spline(&self, elem: usize) -> bool {
        elem < self.tscene().doc().elements().len() && self.tscene().type_of(elem) == BUILDING_SPLINE
    }

    fn building_spline_elems(&self) -> Vec<usize> {
        (0..self.tscene().doc().elements().len()).filter(|&e| self.is_building_spline(e)).collect()
    }

    /// Whether a building spline closes back to its first point.
    pub(crate) fn building_spline_closed(&self, elem: usize) -> bool {
        self.tscene().doc().get(elem, "bspline_closed").is_some_and(|v| wce_tile::properties::parse_bool(&v))
    }

    /// A building spline's settings, from its attributes.
    fn building_spline_settings(&self, elem: usize) -> Settings {
        let doc = self.tscene().doc();
        let number = |name: &str, or: f64| doc.get(elem, name).and_then(|v| wce_formats::c_num::strtod(&v)).unwrap_or(or);
        let flag = |name: &str, or: bool| doc.get(elem, name).map_or(or, |v| wce_tile::properties::parse_bool(&v));
        Settings {
            keys: parse_keys(&doc.get(elem, KEYS).unwrap_or_default()),
            joiners: parse_keys(&doc.get(elem, wce_tile::building_spline::JOINERS).unwrap_or_default()),
            joiner_ends: flag("bspline_joiner_ends", false),
            random: flag("bspline_random", false),
            seed: number("bspline_seed", 1.0).max(0.0) as u32,
            gap: number("bspline_gap", 0.0),
            align: flag("bspline_align", true),
            tilt_across: flag("bspline_tilt_across", false),
            closed: self.building_spline_closed(elem),
            offset: number("bspline_offset", 0.0),
            damage: number("bspline_damage", 0.0).clamp(0.0, 1.0),
            flip: number("bspline_flip", 0.0).clamp(0.0, 1.0),
            yaw_jitter: number("bspline_yaw_jitter", 0.0).clamp(0.0, 180.0),
            side_jitter: number("bspline_side_jitter", 0.0).max(0.0),
        }
    }

    /// A building spline's curve on the ground (world metres, x and z), densely sampled; a closed one ends on its first point.
    fn building_spline_line(&self, elem: usize) -> Vec<[f64; 2]> {
        let mut points = self.polygon_world_points(elem).iter().map(|p| [f64::from(p.x), f64::from(p.z)]).collect::<Vec<_>>();
        let u = self.unit();
        let mut handles = wce_walls::scene::wall_handles(self.tscene(), elem)
            .into_iter()
            .map(|h| h.map(|v| v.map(|p| [p[0] / u, p[1] / u])))
            .collect::<Vec<_>>();
        handles.resize(points.len(), [None, None]);
        if self.building_spline_closed(elem) && points.len() > 2 {
            points.push(points[0]);
            handles.push(handles[0]);
        }

        let sharp = vec![false; points.len()];
        centre_line_handled(&points, &handles, &sharp).0
    }

    /// The curve a building spline's outline draws, on the terrain.
    pub(super) fn building_spline_world_curve(&self, elem: usize) -> Vec<Float3> {
        let line = self.building_spline_line(elem);
        let every = (line.len() / 400).max(1);
        let mut curve: Vec<_> = line
            .iter()
            .step_by(every)
            .chain(line.last())
            .map(|p| Float3::new(p[0] as f32, self.terrain_height(p[0] as f32, p[1] as f32), p[1] as f32))
            .collect();
        curve.dedup();
        curve
    }

    /// A building's footprint (metres, from its pivot; it is laid along the longer of its x and z): its logic file's box, else its model's
    /// bounds (the scene's, else its piece files' vertices). A building with neither is 5 m long along x about its pivot.
    fn building_extent(&self, key: &str) -> Footprint {
        let fallback = Footprint::centred(5.0, 0.5);
        if let Some(v) = self.building_splines.extents.borrow().get(key) {
            return v.unwrap_or(fallback);
        }

        let unit = self.unit();
        let files = self.files.as_deref();
        let real = |f: &Footprint| f.x.1 > f.x.0 && f.z.1 >= f.z.0 && (f.x.1 - f.x.0).max(f.z.1 - f.z.0) > 1e-3;
        let from_logic = files
            .and_then(|f| f.read(&format!("{}{key}_tech.cs2.parsed", wce_walls::building::folder(key))).ok())
            .and_then(|b| wce_walls::logic::extent_xz(&b).ok())
            .map(|[x, z]| Footprint { x: (f64::from(x.0), f64::from(x.1)), z: (f64::from(z.0), f64::from(z.1)) })
            .filter(real);
        let from_scene = || {
            self.building_model(key)
                .filter(|m| m.ok && m.resolved)
                .map(|m| Footprint { x: (f64::from(m.bmin.x), f64::from(m.bmax.x)), z: (f64::from(m.bmin.z), f64::from(m.bmax.z)) })
                .filter(real)
        };
        let from_files = || {
            use rpfm_lib::files::Decodeable;
            use rpfm_lib::files::rigidmodel::RigidModel;
            let files = files?;
            let (mut lo, mut hi) = ([f32::MAX; 2], [f32::MIN; 2]);
            for path in self.preview_building(key, 0) {
                let Ok(bytes) = files.read(&path) else { continue };
                let Ok(model) = RigidModel::decode(&mut std::io::Cursor::new(bytes), &Some(Default::default())) else { continue };
                for block in model.lods().first().map(|l| l.mesh_blocks().as_slice()).unwrap_or_default() {
                    for v in block.mesh().vertices() {
                        let p = v.position();
                        lo = [lo[0].min(p.x), lo[1].min(p.z)];
                        hi = [hi[0].max(p.x), hi[1].max(p.z)];
                    }
                }
            }

            Some(Footprint { x: (f64::from(lo[0]), f64::from(hi[0])), z: (f64::from(lo[1]), f64::from(hi[1])) }).filter(real)
        };

        let found = from_logic
            .or_else(from_scene)
            .or_else(from_files)
            .map(|f| Footprint { x: (f.x.0 / unit, f.x.1 / unit), z: (f.z.0 / unit, f.z.1 / unit) });
        // Without the game's files, the scene's model may still be loading: asked again later.
        if files.is_some() {
            self.building_splines.extents.borrow_mut().insert(key.to_owned(), found);
        }

        found.unwrap_or(fallback)
    }

    /// Per frame: the buildings laid again when anything they are laid from may have changed (the document, the ground, the models), and
    /// put on the scene when they differ from those shown.
    pub(super) fn update_building_splines(&mut self, scene: &mut RenderScene) {
        if self.stroke_open {
            return;
        }

        let any = self.idx.valid && self.idx.type_names.iter().any(|t| t == BUILDING_SPLINE);
        if !any && self.building_splines.items.is_empty() {
            if !self.building_splines.entities.is_empty() {
                for id in std::mem::take(&mut self.building_splines.entities) {
                    scene.remove(id);
                }
            }

            return;
        }

        let stamp = {
            use std::hash::{Hash, Hasher};
            let mut h = std::hash::DefaultHasher::new();
            (self.history_version, self.structure_version, self.models.len(), self.unresolved.len(), self.files.is_some()).hash(&mut h);
            h.finish()
        };

        if self.building_splines.stamp != Some(stamp) {
            self.building_splines.stamp = Some(stamp);
            let items = self.lay_building_splines(false);
            let same = items.len() == self.building_splines.items.len()
                && items.iter().zip(&self.building_splines.items).all(|(a, b)| a.spline == b.spline && a.text == b.text);
            self.building_splines.items = items;
            if !same {
                self.building_splines.shown = false;
            }
        }

        if self.building_splines.shown {
            return;
        }

        self.building_splines.shown = true;
        for id in std::mem::take(&mut self.building_splines.entities) {
            scene.remove(id);
        }

        let unit = self.unit();
        let mut ids = vec![];
        for item in &self.building_splines.items {
            let source = self.source(super::item_of(item.spline));
            let [x, y, z] = item.position;
            for (local, payload) in self.preview_payloads(&item.text) {
                let mut world = local;
                world.m[0][3] += (x / unit) as f32;
                world.m[1][3] += (y / unit) as f32;
                world.m[2][3] += (z / unit) as f32;
                ids.push(scene.add(self.layer, world, payload, source));
            }
        }

        self.building_splines.entities = ids;
    }

    /// The layer was cleared: the entities went with it.
    pub(crate) fn building_splines_cleared(&mut self) {
        self.building_splines.entities.clear();
        self.building_splines.shown = false;
    }

    /// Every shown building spline's buildings, laid now; `export`: only those of the splines an export writes (not in a layer that is
    /// not exported).
    fn lay_building_splines(&self, export: bool) -> Vec<SplineItem> {
        let unit = self.unit();
        let ground = |x: f64, z: f64| f64::from(self.terrain_height(x as f32, z as f32));
        let mut out = vec![];
        for elem in self.building_spline_elems() {
            if !self.shown(elem) || (export && !wce_walls::scene::exported(self.tscene(), elem)) {
                continue;
            }

            let settings = self.building_spline_settings(elem);
            if settings.keys.is_empty() {
                continue;
            }

            let line = self.building_spline_line(elem);
            let levels = |key: &str| self.building_levels(key);
            for laid in lay(&line, &settings, &|k| self.building_extent(k), &levels, &ground) {
                match instance_text(&laid, unit) {
                    Ok(text) => {
                        let [x, y, z] = laid.position;
                        out.push(SplineItem { spline: elem, text, position: [x * unit, y * unit, z * unit] });
                    }

                    Err(err) => wce_foundation::log::warn(format!("building spline: {err}")),
                }
            }
        }

        out
    }

    /// How many buildings the building splines lay, all of them and the given one's.
    #[must_use]
    pub fn building_spline_count(&self, elem: Option<usize>) -> usize {
        self.building_splines.items.iter().filter(|i| elem.is_none_or(|e| i.spline == e)).count()
    }

    /// A building spline's buildings, for a report: how many, and each one's key, position (metres) and destruction level.
    #[must_use]
    pub fn building_spline_report(&self, elem: usize) -> String {
        let unit = self.unit();
        let items = self.building_splines.items.iter().filter(|i| i.spline == elem).collect::<Vec<_>>();
        let list = items
            .iter()
            .map(|i| {
                let t = String::from_utf8_lossy(&i.text);
                let attr = |a: &str| t.split(&format!(" {a}=\"")).nth(1).and_then(|v| v.split('"').next()).unwrap_or("").to_owned();
                let [x, y, z] = i.position;
                format!(
                    "{} ({:.2}, {:.2}, {:.2}) {} level {}",
                    attr("key"),
                    x / unit,
                    y / unit,
                    z / unit,
                    attr("orientation"),
                    attr("destruction_level")
                )
            })
            .collect::<Vec<_>>();
        format!("{} building(s): {}", items.len(), list.join("; "))
    }

    /// The buildings of the building splines as an export reads them: laid now (what is shown may be a frame old).
    pub(super) fn with_building_spline_items(&self, text: Vec<u8>) -> Result<Vec<u8>, String> {
        let items = self.lay_building_splines(true);
        if items.is_empty() {
            return Ok(text);
        }

        let mut scene = TedScene::parse_any(text)?;
        let layer = scene.add_layer(scene.scene_graph(), KIT_LAYER)?;
        for item in &items {
            scene.insert_element_text(layer, &item.text)?;
        }

        Ok(scene.save())
    }
}
