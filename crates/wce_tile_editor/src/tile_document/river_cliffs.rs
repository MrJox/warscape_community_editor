//! A river's cliff banks (docs/plans/river_bridges.md, step 6.5): a river point marked `wce_cliff` (`left` or `right`, as the river runs)
//! makes the bank on that side a cliff from halfway to the point before it to halfway to the one after (the river's ends at its first and
//! last points); neighbouring marked points on one side make one stretch. The river's bed raises the ground behind it
//! (`wce_tile::river_bed::CliffBank`), its shore lays the cliff pieces along it (`wce_tile::shores::cliff_pieces`).

use wce_tile::river_bed::CliffBank;

use super::river_beds::{PATCH_ATTR, river_patch_on};
use super::river_links::RIVER_ID_ATTR;
use super::view::RiverSample;
use super::{Tile, TileDocument};

/// A river point's cliff bank: `left` or `right` (as the river runs); missing or empty: none.
pub const CLIFF_ATTR: &str = "wce_cliff";

/// The side a mark names: 1 to the left as the river runs, -1 to the right; none for anything else.
#[must_use]
pub fn cliff_side(mark: &str) -> Option<f64> {
    match mark.trim() {
        "left" => Some(1.0),
        "right" => Some(-1.0),
        _ => None,
    }
}

/// The mark of a side (1: left, else right).
#[must_use]
pub fn cliff_mark(side: f64) -> &'static str {
    if side > 0.0 { "left" } else { "right" }
}

impl Tile {
    /// A river's cliff banks along its ribbon's samples (`samples`, as its bed and shore measure along them): each marked point's stretch,
    /// neighbouring ones on one side joined.
    pub(super) fn river_cliffs_on(&self, river: usize, samples: &[RiverSample]) -> Vec<CliffBank> {
        let sc = self.tscene();
        let points = sc.polygon_points(river);
        let marks = points.iter().map(|&p| sc.get(p, CLIFF_ATTR).as_deref().and_then(cliff_side)).collect::<Vec<_>>();
        if samples.len() < 2 || marks.iter().all(Option::is_none) {
            return vec![];
        }

        // Each point's place along the ribbon: its nearest place on it.
        let mut walked = vec![0.0; samples.len()];
        for i in 1..samples.len() {
            walked[i] = walked[i - 1] + (samples[i].x - samples[i - 1].x).hypot(samples[i].z - samples[i - 1].z);
        }

        let length = walked[walked.len() - 1];
        let along = |x: f64, z: f64| {
            let mut best = (f64::INFINITY, 0.0);
            for i in 0..samples.len() - 1 {
                let (a, b) = (&samples[i], &samples[i + 1]);
                let (dx, dz) = (b.x - a.x, b.z - a.z);
                let l2 = dx * dx + dz * dz;
                let t = if l2 > 1e-12 { (((x - a.x) * dx + (z - a.z) * dz) / l2).clamp(0.0, 1.0) } else { 0.0 };
                let d = (x - a.x - dx * t).hypot(z - a.z - dz * t);
                if d < best.0 {
                    best = (d, walked[i] + (walked[i + 1] - walked[i]) * t);
                }
            }

            best.1
        };
        let world = self.polygon_world_points(river);
        let at = world.iter().map(|p| along(f64::from(p.x), f64::from(p.z))).collect::<Vec<_>>();
        let n = at.len().min(marks.len());
        let mut out: Vec<CliffBank> = vec![];
        for i in 0..n {
            let Some(side) = marks[i] else { continue };
            let from = if i == 0 { 0.0 } else { (at[i - 1] + at[i]) * 0.5 };
            let to = if i + 1 >= n { length } else { (at[i] + at[i + 1]) * 0.5 };
            match out.last_mut() {
                Some(last) if i > 0 && marks[i - 1] == Some(side) && last.side == side => last.to = to.max(last.to),
                _ => out.push(CliffBank { from: from.min(to), to: from.max(to), side }),
            }
        }

        out
    }

    /// A river's cliff banks (its ribbon as drawn); none when it is not drawn.
    #[must_use]
    pub fn river_cliffs(&self, river: usize) -> Vec<CliffBank> {
        self.river_ribbons_by_elem().remove(&river).map_or_else(Vec::new, |(_, samples)| self.river_cliffs_on(river, &samples))
    }

    /// A river point's cliff bank side, none when it has none.
    #[must_use]
    pub fn river_point_cliff(&self, river: usize, point: usize) -> Option<f64> {
        let sc = self.tscene();
        let &p = sc.polygon_points(river).get(point)?;
        sc.get(p, CLIFF_ATTR).as_deref().and_then(cliff_side)
    }

    /// The outer bank of a river's bend at a point (1: left, -1: right, as the river runs): the side it turns away from; the left where it
    /// runs straight (or at its ends).
    #[must_use]
    pub fn river_outer_side(&self, river: usize, point: usize) -> f64 {
        let w = self.polygon_world_points(river);
        if point == 0 || point + 1 >= w.len() {
            return 1.0;
        }

        let (a, b, c) = (w[point - 1], w[point], w[point + 1]);
        let (u, v) = ([b.x - a.x, b.z - a.z], [c.x - b.x, c.z - b.z]);
        // Turning left (towards (-dz, dx)) is a positive cross: its outer bank is the right.
        let cross = u[0] * v[1] - u[1] * v[0];
        if cross > 0.0 { -1.0 } else { 1.0 }
    }
}

impl TileDocument {
    /// Marks a river point's bank a cliff on a side (1: left, -1: right, as the river runs), or takes its mark away (none). Its terrain
    /// patch turned on (the ground behind is raised by it), an id given if it has none. One undo entry.
    pub fn set_river_cliff(&mut self, river: usize, point: usize, side: Option<f64>) -> bool {
        use wce_editor_core::document::Document;
        let sc = self.tile().tscene();
        if point >= sc.polygon_points(river).len() {
            return false;
        }

        let (patch, bed) = (sc.get(river, PATCH_ATTR), sc.get(river, super::river_beds::BED_ATTR));
        let has_id = sc.get(river, RIVER_ID_ATTR).is_some_and(|v| !v.is_empty());
        let before = self.history().position();
        self.set_point_attribute(river, point, CLIFF_ATTR, side.map_or("", cliff_mark));
        if side.is_some() {
            if !river_patch_on(patch.as_deref(), bed.as_deref()) {
                self.set_attribute(river, PATCH_ATTR, "1");
            }

            if !has_id {
                let id = self.tile().next_river_id();
                self.set_attribute(river, RIVER_ID_ATTR, &id.to_string());
            }
        }

        let n = self.history().position() - before;
        if n > 1 {
            self.join_last(n, if side.is_some() { "Cliff bank" } else { "No cliff bank" });
        }

        true
    }
}
