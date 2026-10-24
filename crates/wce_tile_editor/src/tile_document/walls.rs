//! Procedural walls in the document (docs/plans/procedural_walls.md, phase 1). A `WALL_SPLINE` is stored like the polygons (a `<points>`
//! list, `polygon_closed="0"`) with its own attributes (`wall_base`, `wall_tolerance`, `wall_flip`, its points' `sharp`), so the polygon
//! tools edit it; here its layout comes from the solver (`wce_walls::solver`), its ends attach to the fort buildings' ends, and the view
//! gets the lines to draw.
//!
//! Units: the solver works in scene units (the models' units); the document's world is scene / unit on every axis, and the height map's
//! metres are the scene's y.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use wce_foundation::math::{Double3, Float3};
use wce_scene::render_scene::{EntityId, MeshInstance, Overlay, Payload, RenderScene, overlay_colour};
use wce_tile::properties::parse_bool;
use wce_tile::world_transform::{WorldTransform, euler_matrix, world_transform};
use wce_walls::building::{BuildingFiles, folder};
use wce_walls::pieces::{PieceWarp, make_piece, pieces};
use wce_walls::profile::{Profile, profile};
use wce_walls::solver::{Plan, WallInput, solve};

use super::Tile;
use super::index::matrix_of;

/// The element name of a procedural wall.
pub const WALL_SPLINE: &str = "WALL_SPLINE";

/// The base a wall without (or with an unknown) `wall_base` uses.
pub const DEFAULT_BASE: &str = "barbarian_fort_straight";

/// A new wall's attributes (`TedScene::add_polygon` gives the same): an open polyline of the polygons' defaults, with the default base.
pub const WALL_DEFAULTS: [(&str, &str); 13] = [
    ("flags", " OIF_VISIBLE|OIF_ACTIVE|OIF_DRAGGABLE"), ("recenter_polygon", "0"), ("lf_offset", "0"), ("line_colour", "argb(1.0,0.8,0.0,1.0)"),
    ("polygon_closed", "0"), ("selected_line_colour", "argb(1.0,0.0,0.0,1.0)"), ("name", "Wall"), ("polygon_shape", "PS_POLYGON"),
    ("surface_mode", "MSM_LANDSCAPE"), ("orientation", "euler(0,0,0)"), ("wall_base", DEFAULT_BASE), ("wall_tolerance", "0.5"), ("wall_flip", "false"),
];

/// [`WALL_DEFAULTS`] as attributes.
pub fn wall_defaults() -> std::collections::BTreeMap<String, String> {
    WALL_DEFAULTS.iter().map(|(k, v)| ((*k).to_owned(), (*v).to_owned())).collect()
}

/// An end within this of a building's end is attached to it (scene units, horizontally).
pub const ATTACH: f64 = 0.5;

/// An end placed within this of a building's end is put on it (scene units).
pub const SNAP: f64 = 8.0;

/// How long a wall is left unchanged before its preview pieces are made again (dragging a point would otherwise remake them every frame).
pub const PREVIEW_DELAY: Duration = Duration::from_millis(300);

/// The preview pieces: each wall's entities and the files made for them (in `wce_vfs::generated`, under keys that change with every
/// remake, so the model cache never shows an old one), the bases read once, the walls waiting to be remade.
#[derive(Default)]
pub(crate) struct WallPreview {
    bases: HashMap<String, Option<Arc<BuildingFiles>>>,

    /// The buildings' frame extents (`Tile::frame_extent`), read once.
    extents: std::cell::RefCell<HashMap<String, Option<(f64, f64)>>>,
    entities: HashMap<usize, Vec<EntityId>>,
    keys: HashMap<usize, Vec<String>>,
    pending: HashMap<usize, Instant>,
    revision: u64,
}

/// A fort building's end a wall can attach to: the building element and the end, in scene units (its y is the level the building
/// stands on); the building's x axis (x, z: the normal of its end faces) and its outer side (its -z axis).
#[derive(Clone, Debug, PartialEq)]
pub struct Anchor {
    pub elem: usize,
    pub key: String,
    pub at: [f64; 3],
    pub axis: [f64; 2],
    pub outer: [f64; 2],
}

/// A wall's layout and what it was made from.
#[derive(Clone, Debug)]
pub struct WallLayout {
    pub plan: Plan,
    pub profile: &'static Profile,

    /// The building end each end of the wall is attached to.
    pub ends: [Option<Anchor>; 2],
}

impl Tile {
    pub fn is_wall(&self, elem: usize) -> bool {
        elem < self.tscene().doc().elements().len() && self.tscene().type_of(elem) == WALL_SPLINE
    }

    /// Whether the tile has a wall (what decides that terrain and building edits redraw the walls).
    pub fn has_walls(&self) -> bool {
        self.tscene().doc().elements().iter().any(|e| e.name == WALL_SPLINE)
    }

    /// The x extent of a building's frame: its logic file's box (`<key>_tech.cs2.parsed`, the frame the vanilla walls join on), else its
    /// model's bounds. Read once per key.
    fn frame_extent(&self, key: &str) -> Option<(f64, f64)> {
        if let Some(v) = self.wall_preview.extents.borrow().get(key) {
            return *v;
        }

        let logic = self.files.as_deref().and_then(|f| f.read(&format!("{}{key}_tech.cs2.parsed", folder(key))).ok());
        let from_logic = logic.and_then(|b| wce_walls::logic::extent_x(&b).ok()).map(|(a, b)| (f64::from(a), f64::from(b))).filter(|(a, b)| b > a);
        let from_model = || self.building_model(key).filter(|m| m.ok && m.bmax.x > m.bmin.x).map(|m| (f64::from(m.bmin.x), f64::from(m.bmax.x)));
        let v = from_logic.or_else(from_model);

        // A model still loading is asked again later; a logic file's answer is kept.
        if from_logic.is_some() || self.building_model(key).is_some_and(|m| m.resolved) {
            self.wall_preview.extents.borrow_mut().insert(key.to_owned(), v);
        }

        v
    }

    /// The ends of the fort buildings (`*_fort_*` keys, not generated ones): their frame's x extent at z = 0 ([`Tile::frame_extent`]),
    /// through the instance's transform.
    pub fn wall_anchors(&self) -> Vec<Anchor> {
        let sc = self.tscene();
        let mut out = vec![];
        for e in sc.find_all("BUILDING_INSTANCE") {
            let key = sc.doc().get(e, "key").unwrap_or_default();
            let lower = key.to_lowercase();
            if !lower.contains("_fort_") || lower.starts_with("wce_") {
                continue;
            }

            let Some((lo, hi)) = self.frame_extent(&key) else { continue };
            let w = world_transform(sc, e);
            let x_axis = w.rotation * Double3::new(1.0, 0.0, 0.0);
            let outer = w.rotation * Double3::new(0.0, 0.0, -1.0);
            for x in [lo, hi] {
                let p = w.position + w.rotation * Double3::new(x, 0.0, 0.0);
                out.push(Anchor { elem: e, key: key.clone(), at: [p.x, w.position.y, p.z], axis: [x_axis.x, x_axis.z], outer: [outer.x, outer.z] });
            }
        }

        out
    }

    /// The wall's points in scene units (x, z) and their `sharp` marks.
    fn wall_points(&self, elem: usize) -> (Vec<[f64; 2]>, Vec<bool>) {
        let u = self.unit();
        let sc = self.tscene();
        let pts = self.polygon_world_points(elem).into_iter().map(|p| [f64::from(p.x) * u, f64::from(p.z) * u]).collect();
        let sharp = sc.polygon_points(elem).into_iter().map(|p| sc.doc().get(p, "sharp").is_some_and(|v| parse_bool(&v))).collect();
        (pts, sharp)
    }

    /// The wall's layout, `None` for an element that is not a wall or has fewer than 2 points.
    pub fn wall_layout(&self, elem: usize) -> Option<WallLayout> {
        if !self.is_wall(elem) {
            return None;
        }

        let sc = self.tscene();
        let base = sc.doc().get(elem, "wall_base").unwrap_or_default();
        let profile = profile(&base).or_else(|| profile(DEFAULT_BASE))?;
        let tolerance = sc.doc().get(elem, "wall_tolerance").and_then(|t| t.trim().parse::<f64>().ok()).unwrap_or(0.5).max(0.0);
        let flip = sc.doc().get(elem, "wall_flip").is_some_and(|v| parse_bool(&v));
        let (points, sharp) = self.wall_points(elem);
        if points.len() < 2 {
            return None;
        }

        let anchors = self.wall_anchors();
        let attached = |p: [f64; 2]| anchors.iter().filter(|a| (a.at[0] - p[0]).hypot(a.at[2] - p[1]) <= ATTACH).min_by(|a, b| {
            let d = |x: &Anchor| (x.at[0] - p[0]).hypot(x.at[2] - p[1]);
            d(a).total_cmp(&d(b))
        }).cloned();
        let ends = [attached(points[0]), attached(points[points.len() - 1])];
        let input = WallInput {
            points,
            sharp,
            flip,
            tolerance,
            ends: [ends[0].as_ref().map(|a| a.at[1]), ends[1].as_ref().map(|a| a.at[1])],
            end_planes: [ends[0].as_ref().map(|a| a.axis), ends[1].as_ref().map(|a| a.axis)],
        };

        let u = self.unit();
        let ground = |x: f64, z: f64| self.project.height_at(x / u, z / u);
        let mut plan = solve(&input, profile, &ground);

        // An attached end whose outer side is the other way from the building's: the wall's outer face would meet its inner one.
        if let (Some(first), Some(last)) = (plan.samples.first().copied(), plan.samples.last().copied()) {
            // The plan runs from the last point when flipped.
            let (at_first, at_last) = if flip { (&ends[1], &ends[0]) } else { (&ends[0], &ends[1]) };
            for (sm, a) in [(first, at_first), (last, at_last)] {
                let Some(a) = a else { continue };
                let wall_outer = [sm.dir[1], -sm.dir[0]];
                if wall_outer[0] * a.outer[0] + wall_outer[1] * a.outer[1] < 0.0 {
                    plan.warnings.push(format!("the end on {} has its outer side the other way from the building's: flip the wall", a.key));
                }
            }
        }

        Some(WallLayout { plan, profile, ends })
    }

    /// Where a wall's end goes when it is put at `world` (the document's world): on the nearest fort building end within [`SNAP`], else
    /// where it is.
    pub fn snap_wall_end(&self, world: Float3) -> Float3 {
        let u = self.unit();
        let (x, z) = (f64::from(world.x) * u, f64::from(world.z) * u);
        let best = self.wall_anchors().into_iter().map(|a| ((a.at[0] - x).hypot(a.at[2] - z), a)).filter(|(d, _)| *d <= SNAP).min_by(|a, b| a.0.total_cmp(&b.0));
        match best {
            Some((_, a)) => Float3::new((a.at[0] / u) as f32, world.y, (a.at[2] / u) as f32),
            None => world,
        }
    }

    /// The lines of a wall's layout (the document's world): the outer foot on the ground (orange), the walkway (yellow), the step sections
    /// (red uprights at their ends), the corners (magenta uprights), the attached ends (green crosses).
    pub(crate) fn wall_overlay(&self, elem: usize, o: &mut Overlay) {
        let Some(l) = self.wall_layout(elem) else { return };
        let u = self.unit();
        let w = |x: f64, y: f64, z: f64| Float3::new((x / u) as f32, (y / u) as f32, (z / u) as f32);
        let (foot, walk, step, corner, end) = (
            overlay_colour(1.0, 0.55, 0.1, 1.0),
            overlay_colour(1.0, 0.9, 0.2, 1.0),
            overlay_colour(1.0, 0.2, 0.2, 1.0),
            overlay_colour(1.0, 0.3, 1.0, 1.0),
            overlay_colour(0.3, 1.0, 0.3, 1.0),
        );

        let s = &l.plan.samples;
        for p in s.windows(2) {
            let (a, b) = (p[0], p[1]);
            o.line(w(a.foot[0], a.ground + 0.2, a.foot[1]), w(b.foot[0], b.ground + 0.2, b.foot[1]), foot);
            o.line(w(a.centre[0], a.level + l.profile.walkway, a.centre[1]), w(b.centre[0], b.level + l.profile.walkway, b.centre[1]), walk);
        }

        let at = |d: f64| s.iter().min_by(|a, b| (a.s - d).abs().total_cmp(&(b.s - d).abs())).copied();
        let upright = |o: &mut Overlay, d: f64, rgba: u32| {
            if let Some(sm) = at(d) {
                o.line(w(sm.centre[0], sm.level, sm.centre[1]), w(sm.centre[0], sm.level + l.profile.walkway + 2.0, sm.centre[1]), rgba);
            }
        };

        for st in &l.plan.steps {
            upright(o, st.s.0, step);
            upright(o, st.s.1, step);
        }

        for &(d, _) in &l.plan.corners {
            upright(o, d, corner);
        }

        for a in l.ends.iter().flatten() {
            let r = 2.0;
            o.line(w(a.at[0] - r, a.at[1] + 0.3, a.at[2]), w(a.at[0] + r, a.at[1] + 0.3, a.at[2]), end);
            o.line(w(a.at[0], a.at[1] + 0.3, a.at[2] - r), w(a.at[0], a.at[1] + 0.3, a.at[2] + r), end);
            o.line(w(a.at[0], a.at[1], a.at[2]), w(a.at[0], a.at[1] + l.profile.walkway + 2.0, a.at[2]), end);
        }
    }
}

impl Tile {
    /// The wall changed (or its ground, or what it is attached to): its preview pieces are made again once it has been left alone for
    /// [`PREVIEW_DELAY`].
    pub(crate) fn wall_changed(&mut self, elem: usize) {
        self.wall_preview.pending.insert(elem, Instant::now());
    }

    /// Every wall's preview is made again (the layer was cleared, or the ground changed).
    pub(crate) fn walls_changed(&mut self) {
        let walls = (0..self.tscene().doc().elements().len()).filter(|&e| self.is_wall(e)).collect::<Vec<_>>();
        let now = Instant::now();
        for e in walls {
            self.wall_preview.pending.insert(e, now);
        }
    }

    /// The preview entities are gone with the layer (and the walls' indices may have moved): every wall's preview is made again.
    pub(crate) fn wall_preview_cleared(&mut self) {
        self.wall_preview.entities.clear();
        for key in self.wall_preview.keys.drain().flat_map(|(_, k)| k) {
            wce_vfs::generated::remove_prefix(&folder(&key));
        }

        self.wall_preview.pending.clear();
        self.walls_changed();
    }

    /// Per frame: makes again the previews of the walls left alone long enough.
    pub(crate) fn update_wall_previews(&mut self, scene: &mut RenderScene) {
        if self.wall_preview.pending.is_empty() || self.stroke_open {
            return;
        }

        let now = Instant::now();
        let due = self.wall_preview.pending.iter().filter(|(_, t)| now.duration_since(**t) >= PREVIEW_DELAY).map(|(e, _)| *e).collect::<Vec<_>>();
        for e in due {
            self.wall_preview.pending.remove(&e);
            self.make_wall_preview(scene, e);
        }
    }

    /// The base wall's files, read once.
    fn wall_base(&mut self, key: &str) -> Option<Arc<BuildingFiles>> {
        if let Some(b) = self.wall_preview.bases.get(key) {
            return b.clone();
        }

        let read = self.files.as_deref().and_then(|f| match BuildingFiles::read_files(f, key) {
            Ok(b) => Some(Arc::new(b)),
            Err(err) => {
                wce_foundation::log::warn(format!("wall preview: base {key}: {err}"));
                None
            }
        });
        self.wall_preview.bases.insert(key.to_owned(), read.clone());
        read
    }

    /// The wall's preview pieces: its old ones removed, a piece per stretch of the layout made from the base (its intact meshes) and
    /// drawn where it goes. A hidden wall (or one that is no longer a wall) has none.
    fn make_wall_preview(&mut self, scene: &mut RenderScene, elem: usize) {
        for id in self.wall_preview.entities.remove(&elem).unwrap_or_default() {
            scene.remove(id);
        }

        for key in self.wall_preview.keys.remove(&elem).unwrap_or_default() {
            wce_vfs::generated::remove_prefix(&folder(&key));
        }

        if !self.is_wall(elem) || !self.shown(elem) {
            return;
        }

        let Some(layout) = self.wall_layout(elem) else { return };
        let Some(base) = self.wall_base(layout.profile.key) else { return };
        let u = self.unit();
        let ground = |x: f64, z: f64| self.project.height_at(x / u, z / u);
        self.wall_preview.revision += 1;
        let revision = self.wall_preview.revision;
        let mut made = vec![];
        for (i, piece) in pieces(&layout.plan, layout.profile).into_iter().enumerate() {
            let key = format!("wce_preview_{elem}_{revision}_{i}");
            let warp = PieceWarp::new(&layout.plan, layout.profile, piece, &ground);
            match make_piece(&base, &key, &warp, false, Some(1)) {
                Ok(files) => made.push((key, piece, files)),
                Err(err) => wce_foundation::log::warn(format!("wall preview: piece {i}: {err}")),
            }
        }

        let source = self.source(super::item_of(elem));
        let mut ids = vec![];
        let mut keys = vec![];
        for (key, piece, files) in made {
            let wt = WorldTransform { position: Double3::new(piece.origin[0], piece.origin[1], piece.origin[2]), rotation: euler_matrix(Double3::new(-piece.yaw, 0.0, 0.0)) };
            let world = matrix_of(&wt, u, Some(Double3::new(1.0 / u, 1.0 / u, 1.0 / u)));
            for (path, bytes) in files.into_pack_files() {
                let model = path.ends_with(".rigid_model_v2").then(|| path.clone());
                wce_vfs::generated::put(&path, bytes);
                if let Some(model) = model {
                    ids.push(scene.add(self.layer, world, Payload::MeshInstance(MeshInstance { model, lod: true, ..Default::default() }), source));
                }
            }

            keys.push(key);
        }

        self.wall_preview.entities.insert(elem, ids);
        self.wall_preview.keys.insert(elem, keys);
    }
}
