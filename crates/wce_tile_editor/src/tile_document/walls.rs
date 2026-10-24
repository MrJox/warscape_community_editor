//! Procedural walls in the document (docs/plans/procedural_walls.md, phase 1). A `WALL_SPLINE` is stored like the polygons (a `<points>`
//! list, `polygon_closed="0"`) with its own attributes (`wall_base`, `wall_tolerance`, `wall_flip`, its points' `sharp`), so the polygon
//! tools edit it; here its layout comes from the solver (`wce_walls::solver`), its ends attach to the fort buildings' ends, and the view
//! gets the lines to draw.
//!
//! Units: the solver works in scene units (the models' units); the document's world is scene / unit on every axis, and the height map's
//! metres are the scene's y.

use wce_foundation::math::Float3;
use wce_scene::render_scene::{Overlay, overlay_colour};
use wce_tile::properties::parse_bool;
use wce_tile::world_transform::world_transform;
use wce_walls::profile::{Profile, profile};
use wce_walls::solver::{Plan, WallInput, solve};

use super::Tile;

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

/// A fort building's end a wall can attach to: the building element and the end, in scene units (its y is the level the building
/// stands on).
#[derive(Clone, Debug, PartialEq)]
pub struct Anchor {
    pub elem: usize,
    pub key: String,
    pub at: [f64; 3],
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

    /// The ends of the fort buildings (`*_fort_*` keys, not generated ones): the model's x extent at z = 0, through the instance's
    /// transform. The models' bounds stand for the pieces' frames until the generator reads their logic files.
    pub fn wall_anchors(&self) -> Vec<Anchor> {
        let sc = self.tscene();
        let mut out = vec![];
        for e in sc.find_all("BUILDING_INSTANCE") {
            let key = sc.doc().get(e, "key").unwrap_or_default();
            let lower = key.to_lowercase();
            if !lower.contains("_fort_") || lower.starts_with("wce_") {
                continue;
            }

            let Some(m) = self.building_model(&key).filter(|m| m.ok && m.bmax.x > m.bmin.x) else { continue };
            let w = world_transform(sc, e);
            for x in [m.bmin.x, m.bmax.x] {
                let p = w.position + w.rotation * wce_foundation::math::Double3::new(f64::from(x), 0.0, 0.0);
                out.push(Anchor { elem: e, key: key.clone(), at: [p.x, w.position.y, p.z] });
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
        let input = WallInput { points, sharp, flip, tolerance, ends: [ends[0].as_ref().map(|a| a.at[1]), ends[1].as_ref().map(|a| a.at[1])] };
        let u = self.unit();
        let ground = |x: f64, z: f64| self.project.height_at(x / u, z / u);
        Some(WallLayout { plan: solve(&input, profile, &ground), profile, ends })
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
