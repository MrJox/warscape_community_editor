//! Tape measures and angle finders in the document (`wce_tile::measure`): a `WceMeasureLine`'s or `WceAngleFinder`'s points on the ground,
//! and what they measure.

use wce_foundation::math::Float3;
use wce_tile::measure::{Angle, Measure, angle, measure};

use super::{ANGLE_FINDER, MEASURE_LINE, Tile};

impl Tile {
    fn is_type(&self, elem: usize, kind: &str) -> bool {
        elem < self.tscene().doc().elements().len() && self.tscene().type_of(elem) == kind
    }

    /// Whether `elem` is a tape measure.
    pub fn is_measure(&self, elem: usize) -> bool {
        self.is_type(elem, MEASURE_LINE)
    }

    /// Whether `elem` is an angle finder.
    pub fn is_angle_finder(&self, elem: usize) -> bool {
        self.is_type(elem, ANGLE_FINDER)
    }

    /// A tape measure's or an angle finder's points (world metres), on the ground: it is drawn there, and its points keep no height of
    /// their own (they are landscape, as the polygons are).
    pub fn measure_points(&self, elem: usize) -> Vec<Float3> {
        self.polygon_world_points(elem).into_iter().map(|p| Float3::new(p.x, self.terrain_height(p.x, p.z), p.z)).collect()
    }

    fn measure_input(&self, elem: usize) -> Vec<[f64; 3]> {
        self.measure_points(elem).iter().map(|p| [f64::from(p.x), f64::from(p.y), f64::from(p.z)]).collect()
    }

    fn ground(&self) -> impl Fn(f64, f64) -> f64 + '_ {
        |x, z| f64::from(self.terrain_height(x as f32, z as f32))
    }

    /// A tape measure's lengths; `None` for anything else.
    pub fn measure_line(&self, elem: usize) -> Option<Measure> {
        self.is_measure(elem).then(|| measure(&self.measure_input(elem), &self.ground()))
    }

    /// An angle finder's angle; `None` for anything else (or one without three points).
    pub fn angle_finder(&self, elem: usize) -> Option<Angle> {
        if !self.is_angle_finder(elem) {
            return None;
        }

        angle(&self.measure_input(elem), &self.ground())
    }

    /// The tape measures and angle finders shown (their elements).
    pub fn shown_measures(&self) -> Vec<usize> {
        let sc = self.tscene();
        let mut all = sc.find_all(MEASURE_LINE);
        all.extend(sc.find_all(ANGLE_FINDER));
        all.into_iter().filter(|&e| self.shown(e)).collect()
    }
}
