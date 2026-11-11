//! Tape measures in the document (`wce_tile::measure`): a `WceMeasureLine`'s points on the ground, and its lengths.

use wce_foundation::math::Float3;
use wce_tile::measure::{Measure, measure};

use super::{MEASURE_LINE, Tile};

impl Tile {
    /// Whether `elem` is a tape measure.
    pub fn is_measure(&self, elem: usize) -> bool {
        elem < self.tscene().doc().elements().len() && self.tscene().type_of(elem) == MEASURE_LINE
    }

    /// A tape measure's points (world metres), on the ground: it is drawn there, and its points keep no height of their own (they are
    /// landscape, as the polygons are).
    pub fn measure_points(&self, elem: usize) -> Vec<Float3> {
        self.polygon_world_points(elem).into_iter().map(|p| Float3::new(p.x, self.terrain_height(p.x, p.z), p.z)).collect()
    }

    /// A tape measure's lengths; `None` for anything else.
    pub fn measure_line(&self, elem: usize) -> Option<Measure> {
        if !self.is_measure(elem) {
            return None;
        }

        let points = self.measure_points(elem).iter().map(|p| [f64::from(p.x), f64::from(p.y), f64::from(p.z)]).collect::<Vec<_>>();
        Some(measure(&points, &|x, z| f64::from(self.terrain_height(x as f32, z as f32))))
    }

    /// The tape measures shown (their elements).
    pub fn shown_measures(&self) -> Vec<usize> {
        self.tscene().find_all(MEASURE_LINE).into_iter().filter(|&e| self.shown(e)).collect()
    }
}
