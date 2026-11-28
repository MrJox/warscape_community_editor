//! A river's cliff banks (docs/plans/river_bridges.md, step 6.5): the river's `wce_shore_cliffs` (none, the outer banks of its bends,
//! left, right or both) and a point's own `wce_cliff` over it (none, left, right or both, as the river runs; missing: as the river) make
//! the bank on that side a cliff from halfway to the point before it to halfway to the one after (the river's ends at its first and last
//! points); neighbouring points with a cliff on one side make one stretch. The river's bed raises the ground behind it
//! (`wce_tile::river_bed::CliffBank`), its shore lays the cliff pieces along it (`wce_tile::shores::cliff_pieces`).

use wce_tile::river_bed::CliffBank;

use super::river_beds::{PATCH_ATTR, river_patch_on};
use super::river_links::RIVER_ID_ATTR;
use super::view::RiverSample;
use super::{Tile, TileDocument};

/// A river point's cliff banks: `none`, `left`, `right` or `both` (as the river runs); missing or empty: as the river.
pub const CLIFF_ATTR: &str = "wce_cliff";

/// A river's cliff banks where its points do not say: `none` (missing), `outer` (the outer bank of its bends), `left`, `right`, `both`.
pub const CLIFFS_ATTR: &str = "wce_shore_cliffs";

/// A point's cliff marks, by name, the Inspector's order.
pub const CLIFF_MARKS: [&str; 4] = ["none", "left", "right", "both"];

/// A river's cliff settings, by name, the Inspector's order.
pub const RIVER_CLIFFS: [&str; 5] = ["none", "outer", "left", "right", "both"];

/// How far onto either bank past the water a bridge's footprint parts a cliff bank (its ramps land there), and how far past it the cliff
/// stays away (metres).
pub const BRIDGE_BANK_M: f64 = 15.0;
pub const BRIDGE_CLEAR_M: f64 = 8.0;

/// How sharply a river turns at a point for its outer bank to be a cliff (`outer`; degrees between the line before it and after it).
pub const OUTER_BEND_DEG: f64 = 20.0;

/// The sides a mark names (1: the left as the river runs, -1: the right); none for an unknown one (as the river).
#[must_use]
pub fn cliff_sides(mark: &str) -> Option<Vec<f64>> {
    match mark.trim() {
        "none" => Some(vec![]),
        "left" => Some(vec![1.0]),
        "right" => Some(vec![-1.0]),
        "both" => Some(vec![1.0, -1.0]),
        _ => None,
    }
}

/// The mark of a side (1: left, else right).
#[must_use]
pub fn cliff_mark(side: f64) -> &'static str {
    if side > 0.0 { "left" } else { "right" }
}

impl Tile {
    /// A river point's own cliff mark (`CLIFF_MARKS`); none: as the river.
    #[must_use]
    pub fn river_point_cliff(&self, river: usize, point: usize) -> Option<&'static str> {
        let sc = self.tscene();
        let &p = sc.polygon_points(river).get(point)?;
        let mark = sc.get(p, CLIFF_ATTR)?;
        CLIFF_MARKS.into_iter().find(|m| *m == mark.trim())
    }

    /// A river's own cliff setting (`RIVER_CLIFFS`; `none` when it has none).
    #[must_use]
    pub fn river_cliffs_mode(&self, river: usize) -> &'static str {
        let mark = self.tscene().get(river, CLIFFS_ATTR).unwrap_or_default();
        RIVER_CLIFFS.into_iter().find(|m| *m == mark.trim()).unwrap_or("none")
    }

    /// How far onto either bank past the water a bridge's footprint parts a cliff bank (its ramps land there), and how far past it the cliff
    /// stays away (metres).
    pub const BRIDGE_BANK_M: f64 = 15.0;
    pub const BRIDGE_CLEAR_M: f64 = 4.0;

    /// How sharply a river turns at a point (degrees; 0 at its ends).
    #[must_use]
    pub fn river_bend_deg(&self, river: usize, point: usize) -> f64 {
        let w = self.polygon_world_points(river);
        if point == 0 || point + 1 >= w.len() {
            return 0.0;
        }

        let (a, b, c) = (w[point - 1], w[point], w[point + 1]);
        let (u, v) = ([f64::from(b.x - a.x), f64::from(b.z - a.z)], [f64::from(c.x - b.x), f64::from(c.z - b.z)]);
        let (lu, lv) = (u[0].hypot(u[1]), v[0].hypot(v[1]));
        if lu < 1e-9 || lv < 1e-9 {
            return 0.0;
        }

        ((u[0] * v[0] + u[1] * v[1]) / (lu * lv)).clamp(-1.0, 1.0).acos().to_degrees()
    }

    /// The sides a river point's banks are cliffs on: its own mark, else the river's setting (`outer`: the outer bank where it turns
    /// [`OUTER_BEND_DEG`] or more).
    #[must_use]
    pub fn river_point_cliff_sides(&self, river: usize, point: usize) -> Vec<f64> {
        if let Some(own) = self.river_point_cliff(river, point).and_then(cliff_sides) {
            return own;
        }

        match self.river_cliffs_mode(river) {
            "outer" if self.river_bend_deg(river, point) >= OUTER_BEND_DEG => vec![self.river_outer_side(river, point)],
            "outer" | "none" => vec![],
            mark => cliff_sides(mark).unwrap_or_default(),
        }
    }

    /// Each of a river's points' stretch along its ribbon's samples (metres, as its bed and shore measure along them): from halfway to
    /// the point before to halfway to the one after (its ends: the ribbon's), each point at its nearest place of the ribbon.
    pub(super) fn river_point_spans(&self, river: usize, samples: &[RiverSample]) -> Vec<(f64, f64)> {
        if samples.len() < 2 {
            return vec![];
        }

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
        let at = self.polygon_world_points(river).iter().map(|p| along(f64::from(p.x), f64::from(p.z))).collect::<Vec<_>>();
        let n = at.len();
        (0..n)
            .map(|i| {
                let from = if i == 0 { 0.0 } else { (at[i - 1] + at[i]) * 0.5 };
                let to = if i + 1 >= n { length } else { (at[i] + at[i + 1]) * 0.5 };
                (from.min(to), from.max(to))
            })
            .collect()
    }

    /// A river's cliff banks along its ribbon's samples: each point's stretch on each side it has a cliff on, neighbouring ones on one
    /// side joined.
    pub(super) fn river_cliffs_on(&self, river: usize, samples: &[RiverSample]) -> Vec<CliffBank> {
        let spans = self.river_point_spans(river, samples);
        let sides = (0..spans.len()).map(|i| self.river_point_cliff_sides(river, i)).collect::<Vec<_>>();
        let mut out: Vec<CliffBank> = vec![];
        for side in [1.0, -1.0] {
            let mut open: Option<CliffBank> = None;
            for (i, &(from, to)) in spans.iter().enumerate() {
                if sides[i].contains(&side) {
                    match &mut open {
                        Some(c) => c.to = to.max(c.to),
                        None => open = Some(CliffBank { from, to, side }),
                    }
                } else if let Some(c) = open.take() {
                    out.push(c);
                }
            }

            out.extend(open);
        }

        self.clear_of_bridges(out, samples)
    }

    /// Cliff banks parted round the bridges over their river (their stretch under a bridge's footprint or a road bridge's deck, across
    /// the water and [`BRIDGE_BANK_M`] onto either bank, and [`BRIDGE_CLEAR_M`] past it): no pieces under a deck, and the ground behind
    /// not raised where its ends land. Each part's face falls to the water at the bridge over its ramp.
    fn clear_of_bridges(&self, cliffs: Vec<CliffBank>, samples: &[RiverSample]) -> Vec<CliffBank> {
        let covers = self.bridge_covers();
        if cliffs.is_empty() || covers.is_empty() || samples.len() < 2 {
            return cliffs;
        }

        // The stretches under a bridge (metres along the samples).
        let mut walked = 0.0;
        let mut under: Vec<(f64, f64)> = vec![];
        for i in 0..samples.len() {
            if i > 0 {
                walked += (samples[i].x - samples[i - 1].x).hypot(samples[i].z - samples[i - 1].z);
            }

            let (a, b) = (&samples[i.saturating_sub(1)], &samples[(i + 1).min(samples.len() - 1)]);
            let l = (b.x - a.x).hypot(b.z - a.z).max(1e-9);
            let (nx, nz) = (-(b.z - a.z) / l, (b.x - a.x) / l);
            let s = &samples[i];
            let reach = s.width * 0.5 + BRIDGE_BANK_M;
            let steps = (reach / 3.0).ceil() as i32;
            let covered = (-steps..=steps).any(|k| {
                let off = reach * f64::from(k) / f64::from(steps.max(1));
                let (x, z) = ((s.x + nx * off) as f32, (s.z + nz * off) as f32);
                covers.iter().any(|c| c.covers(x, z, wce_tile::shores::BRIDGE_MARGIN))
            });
            if covered {
                match under.last_mut() {
                    Some(u) if walked - u.1 <= 6.0 => u.1 = walked,
                    _ => under.push((walked, walked)),
                }
            }
        }

        let mut out = vec![];
        for c in cliffs {
            let mut parts = vec![c];
            for &(a, b) in &under {
                let (a, b) = (a - BRIDGE_CLEAR_M, b + BRIDGE_CLEAR_M);
                parts = parts
                    .into_iter()
                    .flat_map(|p| {
                        if b <= p.from || a >= p.to {
                            return vec![p];
                        }

                        [CliffBank { to: a, ..p }, CliffBank { from: b, ..p }].into_iter().filter(|q| q.to - q.from > 1.0).collect()
                    })
                    .collect();
            }

            out.extend(parts);
        }

        out
    }

    /// A river's cliff banks (its ribbon as drawn); none when it is not drawn.
    #[must_use]
    pub fn river_cliffs(&self, river: usize) -> Vec<CliffBank> {
        self.river_ribbons_by_elem().remove(&river).map_or_else(Vec::new, |(_, samples)| self.river_cliffs_on(river, &samples))
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

    /// Whether a river has a cliff anywhere (its own setting or a point's).
    #[must_use]
    pub fn river_has_cliffs(&self, river: usize) -> bool {
        (0..self.tscene().polygon_points(river).len()).any(|i| !self.river_point_cliff_sides(river, i).is_empty())
    }
}

impl TileDocument {
    /// Turns a river's terrain patch on (its cliffs raise the ground through it) and gives it an id if it has none, when it has a cliff.
    pub(super) fn patch_for_cliffs(&mut self, river: usize) {
        if !self.tile().river_has_cliffs(river) {
            return;
        }

        let sc = self.tile().tscene();
        let (patch, bed) = (sc.get(river, PATCH_ATTR), sc.get(river, super::river_beds::BED_ATTR));
        let has_id = sc.get(river, RIVER_ID_ATTR).is_some_and(|v| !v.is_empty());
        if !river_patch_on(patch.as_deref(), bed.as_deref()) {
            self.set_attribute(river, PATCH_ATTR, "1");
        }

        if !has_id {
            let id = self.tile().next_river_id();
            self.set_attribute(river, RIVER_ID_ATTR, &id.to_string());
        }
    }

    /// Sets river points' own cliff marks (`CLIFF_MARKS`), or takes them away (none: as the river); set by hand, they are no longer the
    /// shore variation's. Its terrain patch turned on when it has a cliff. One undo entry.
    pub fn set_river_cliff(&mut self, river: usize, points: &[usize], mark: Option<&str>) -> bool {
        if mark.is_some_and(|m| !CLIFF_MARKS.contains(&m)) {
            return false;
        }

        self.set_river_point_shore(river, points, CLIFF_ATTR, mark, if mark.is_some() { "Cliff bank" } else { "Cliff bank as the river" })
    }

    /// Sets a river's own cliff setting (`RIVER_CLIFFS`), its terrain patch turned on when it has a cliff. One undo entry.
    pub fn set_river_cliffs_mode(&mut self, river: usize, mode: &str) -> bool {
        use wce_editor_core::document::Document;
        if !RIVER_CLIFFS.contains(&mode) {
            return false;
        }

        let before = self.history().position();
        self.set_attribute(river, CLIFFS_ATTR, mode);
        self.patch_for_cliffs(river);
        let n = self.history().position() - before;
        if n > 1 {
            self.join_last(n, "River cliffs");
        }

        true
    }
}
