//! Flip: items mirrored across a vertical plane, moved or as a copy.
//!
//! A model cannot be mirrored (the game ignores a building's scale, docs/re/buildings.md), so an item keeps its own handedness: it goes to
//! its mirrored place with its heading reflected, as a rotation. What holds other items is not turned, each item inside goes to its own
//! mirrored place: turned as one, a group's layout would come out rotated, not mirrored. Polygons and splines have their points mirrored,
//! and a procedural wall its side too: left and right of its line change places, and its outer face has to stay the outer one. A river of
//! TEd's goes where the game has it and takes its real heights: its file's factor would pull it down on raised ground (`river_files`).

use wce_foundation::math::{Double3, Double3x3, Float3};
use wce_tile::new_instances::WALL_SPLINE;
use wce_tile::properties::parse_bool;
use wce_tile::river_files::RIVER_HEIGHT_ATTR;
use wce_tile::splines;
use wce_tile::ted_scene::TedScene;
use wce_tile::world_transform::{set_world_position, set_world_rotation, world_transform};

use super::edits::parent_transform;
use super::{Placement, TileDocument, item_of};

/// A vertical plane: a point of it on the ground (metres) and its angle: at 0 its normal is +x, turning towards +z.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FlipPlane {
    pub x: f32,
    pub z: f32,
    pub angle: f32,
}

impl FlipPlane {
    fn normal(self) -> Double3 {
        let (s, c) = f64::from(self.angle).to_radians().sin_cos();
        Double3::new(c, 0.0, s)
    }

    /// Along the plane, on the ground.
    pub fn along(self) -> Float3 {
        let n = self.normal();
        Float3::new(-n.z as f32, 0.0, n.x as f32)
    }

    /// A point in metres mirrored across the plane.
    pub fn mirror_metres(self, p: Float3) -> Float3 {
        let n = self.normal();
        let d = 2.0 * ((f64::from(p.x) - f64::from(self.x)) * n.x + (f64::from(p.z) - f64::from(self.z)) * n.z);
        Float3::new((f64::from(p.x) - d * n.x) as f32, p.y, (f64::from(p.z) - d * n.z) as f32)
    }

    /// A point of the file (`unit` of them a metre) mirrored across the plane.
    fn mirror(self, p: Double3, unit: f64) -> Double3 {
        let n = self.normal();
        let d = 2.0 * ((p.x - f64::from(self.x) * unit) * n.x + (p.z - f64::from(self.z) * unit) * n.z);
        Double3::new(p.x - d * n.x, p.y, p.z - d * n.z)
    }

    /// A rotation with its forward (z) and up (y) axes mirrored; x goes the other way, which keeps it a rotation.
    fn mirror_rotation(self, r: &Double3x3) -> Double3x3 {
        let n = self.normal();
        let flip = |v: Double3| {
            let d = 2.0 * (v.x * n.x + v.z * n.z);
            Double3::new(v.x - d * n.x, v.y, v.z - d * n.z)
        };

        let x = flip(r.col(0));
        Double3x3::from_cols(Double3::new(-x.x, -x.y, -x.z), flip(r.col(1)), flip(r.col(2)))
    }
}

/// Where an element goes.
struct Target {
    elem: usize,
    position: Double3,
    rotation: Double3x3,

    /// A polygon's points, a spline's points with their handles (none: the end's unused handle, written 0,0,0), a shelter's target, in the
    /// world.
    points: Vec<Double3>,
    spline: Vec<[Option<Double3>; 3]>,
    target: Option<Double3>,

    /// A procedural wall's `wall_flip` to write.
    wall_flip: Option<bool>,

    /// A river that has its real heights from now on.
    keeps_heights: bool,
}

/// An element and all that it holds, parents first.
fn with_contents(sc: &TedScene, e: usize, out: &mut Vec<usize>) {
    out.push(e);
    for c in sc.children(e) {
        with_contents(sc, c, out);
    }
}

impl TileDocument {
    /// What the rivers among `elems` and all they hold divide their heights by in the game, in document order.
    fn river_factors(&self, elems: &[usize]) -> Vec<f64> {
        let t = self.tile();
        let sc = t.tscene();
        let mut all = vec![];
        for &e in elems {
            with_contents(sc, e, &mut all);
        }

        all.into_iter().filter(|&e| sc.type_of(e) == "SplineRiverInstance").map(|e| t.river_height_factor(e)).collect()
    }

    /// `factors`: the rivers' ([`Self::river_factors`], of the elements a copy was made from).
    fn flip_targets(&self, roots: &[usize], plane: FlipPlane, factors: &[f64]) -> Vec<Target> {
        let mut factors = factors.iter().copied();
        let t = self.tile();
        let sc = t.tscene();
        let u = t.unit();
        let mut all = vec![];
        for &e in roots {
            with_contents(sc, e, &mut all);
        }

        all.into_iter().filter(|&e| !sc.is_layer(e)).map(|e| {
            let w = world_transform(sc, e);
            let at = |p: Double3| w.position + w.rotation * p;
            let mirrored = |p: Double3| plane.mirror(p, u);

            // What follows the landscape stays as high above the ground: the other side of the plane can be higher (a river left under it).
            let landscape = t.placement_of_attr(e) == Placement::Landscape;
            let ground = |p: Double3| f64::from(t.terrain_height((p.x / u) as f32, (p.z / u) as f32)) * u;
            let rise = |p: Double3| if landscape { ground(plane.mirror(p, u)) - ground(p) } else { 0.0 };
            let river = sc.type_of(e) == "SplineRiverInstance";
            let factor = if river { factors.next().unwrap_or(1.0) } else { 1.0 };
            let lifted = |p: Double3, by: f64| {
                let q = plane.mirror(p, u);
                Double3::new(q.x, q.y / factor + by, q.z)
            };

            let spline = if wce_tile::splines::is_spline(sc.type_of(e)) {
                splines::read(sc, e).iter().map(|p| {
                    let by = rise(at(p.position));
                    let handle = |h: Double3| (h != Double3::ZERO).then(|| lifted(at(h), by));
                    [Some(lifted(at(p.position), by)), handle(p.control_in), handle(p.control_out)]
                }).collect()
            } else {
                vec![]
            };

            let wall_flip = (sc.type_of(e) == WALL_SPLINE).then(|| !sc.doc().get(e, "wall_flip").is_some_and(|v| parse_bool(&v)));

            let points = if spline.is_empty() { sc.polygon_points(e).into_iter().filter_map(|p| sc.point_position(p)).map(|p| lifted(at(p), rise(at(p)))).collect() } else { vec![] };

            // A shelter's target is in the frame of its parent, as its position.
            let pw = parent_transform(sc, e);
            let target = sc.doc().get(e, "target").and_then(|v| wce_tile::ted_scene::parse_vec3(&v, "vector")).map(|v| mirrored(pw.position + pw.rotation * v));

            // Lines and what holds other items are not turned: their points and contents go where they go.
            let holds = !sc.children(e).is_empty() || !sc.polygon_points(e).is_empty();
            let rotation = if holds { w.rotation } else { plane.mirror_rotation(&w.rotation) };
            let mut position = lifted(w.position, rise(w.position));
            if !holds && landscape {
                position.y = ground(position);
            }

            let keeps_heights = river && t.rivers_keep_heights() && sc.doc().get(e, RIVER_HEIGHT_ATTR).is_none();
            Target { elem: e, position, rotation, points, spline, target, wall_flip, keeps_heights }
        }).collect()
    }

    fn apply_flip(&mut self, targets: &[Target]) -> Result<(), String> {
        for t in targets {
            let sc = self.tscene_mut();
            set_world_position(sc, t.elem, t.position);
            set_world_rotation(sc, t.elem, &t.rotation);
            let w = world_transform(sc, t.elem);
            let back = w.rotation.transpose();
            let local = |p: Double3| back * (p - w.position);
            for (i, p) in t.points.iter().enumerate() {
                sc.set_polygon_point(t.elem, i, local(*p))?;
            }

            for (&elem, p) in sc.polygon_points(t.elem).iter().zip(&t.spline) {
                for (name, v) in ["position", "controlIn", "controlOut"].into_iter().zip(p) {
                    if let Some(v) = v.filter(|_| sc.doc().get(elem, name).is_some()) {
                        sc.set_attr(elem, name, &TedScene::point_text(local(v)));
                    }
                }
            }

            if let Some(v) = t.target {
                let pw = parent_transform(sc, t.elem);
                let local = pw.rotation.transpose() * (v - pw.position);
                sc.set_attr(t.elem, "target", &format!("vector({})", TedScene::point_text(local)));
            }

            if let Some(flip) = t.wall_flip {
                sc.set_attr(t.elem, "wall_flip", if flip { "true" } else { "false" });
            }

            if t.keeps_heights {
                sc.set_attr(t.elem, RIVER_HEIGHT_ATTR, "game");
            }
        }

        Ok(())
    }

    /// Mirrors the elements across a plane, one undo entry: moved, or copied into `copy_into` with the elements left as they are. Returns
    /// the elements that went.
    ///
    /// # Errors
    ///
    /// Nothing to flip, or a text does not read.
    pub fn flip(&mut self, elems: &[usize], plane: FlipPlane, copy_into: Option<usize>) -> Result<Vec<usize>, String> {
        let sc = self.tile().tscene();
        let roots = elems.iter().copied().filter(|&e| !elems.iter().any(|&a| a != e && sc.is_descendant(e, a))).collect::<Vec<_>>();
        if roots.is_empty() {
            return Err("nothing to flip".to_owned());
        }

        let items = copy_into.map(|_| self.copy_items(&roots));
        let factors = self.river_factors(&roots);
        if let Some(into) = copy_into {
            self.check_structure(&[], Some(into))?;
        }

        let mark = self.begin()?;
        let result = (|| -> Result<(Vec<usize>, Vec<usize>), String> {
            let roots = match (&items, copy_into) {
                (Some(items), Some(into)) => self.paste_elements(items, into, Float3::default())?,
                _ => roots,
            };

            let targets = self.flip_targets(&roots, plane, &factors);
            self.apply_flip(&targets)?;
            Ok((roots, targets.iter().map(|t| t.elem).collect()))
        })();

        match result {
            Ok((went, touched)) => {
                let name = format!("{} {} items", if copy_into.is_some() { "Flip a copy of" } else { "Flip" }, went.len());
                let items = went.iter().map(|&e| item_of(e)).collect::<Vec<_>>();
                if copy_into.is_some() {
                    self.push_splice_added(name, mark, vec![], items, &went);
                } else {
                    self.push_splice_touching(name, mark, items.clone(), items, touched);
                }

                Ok(went)
            }

            Err(e) => {
                let record = self.tscene_mut().end_edit(mark).unwrap_or_default();
                let _ = self.tscene_mut().unapply(&record);
                Err(e)
            }
        }
    }
}
