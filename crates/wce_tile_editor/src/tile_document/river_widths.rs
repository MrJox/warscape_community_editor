//! A river's width point by point: the viewport's width handles (a marker on either edge of the water at each point, dragged across the
//! river) and the widths randomized by a seed within a deviation, as one undo entry.
//!
//! A point's `width` is TEd's; the water is drawn at it times [`Tile::river_width_scale`] (a battle river's file factor, 0.5 for today's
//! TEd). A randomized point keeps the width it had (`wce_width_base`), so that randomizing again, with another seed or deviation, starts
//! from it and does not pile up; a width set by hand (a handle, the point's row) clears it, the width set becoming the one randomized.

use wce_editor_core::document::Document;
use wce_foundation::math::Float3;
use wce_tile::ted_scene::format_number;

use super::commands::TedPointAttrsCommand;
use super::river_links::{LINK_ATTR, RIVER_ID_ATTR};
use super::{Tile, TileDocument};

/// The width a point had before it was randomized (TEd's units, as `width`).
pub const WIDTH_BASE_ATTR: &str = "wce_width_base";

/// The deviation a randomizing takes each point's width by, from and to (percent of the width; the river's, missing: the defaults).
pub const WIDTH_DEV_MIN_ATTR: &str = "wce_width_dev_min";
pub const WIDTH_DEV_MAX_ATTR: &str = "wce_width_dev_max";

/// The seed a randomizing draws by (the river's; missing: its `wce_river_id`, else its element).
pub const WIDTH_SEED_ATTR: &str = "wce_width_seed";

/// The river's attributes the Inspector's Width variation shows (not among its other attributes).
pub(crate) const WIDTH_ROW_ATTRS: [&str; 3] = [WIDTH_DEV_MIN_ATTR, WIDTH_DEV_MAX_ATTR, WIDTH_SEED_ATTR];

/// The deviation's defaults (percent).
pub const DEFAULT_WIDTH_DEV: (f64, f64) = (-25.0, 25.0);

/// The narrowest water a handle or a randomizing leaves (metres, as drawn).
pub const MIN_WIDTH_M: f64 = 2.0;

/// A river's width settings: the deviation (percent, from and to) and the seed.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct WidthSettings {
    pub min: f64,
    pub max: f64,
    pub seed: u64,
}

/// A number from 0 to 1 of a seed and a point (splitmix64).
fn unit(seed: u64, point: usize) -> f64 {
    let mut z = seed ^ (point as u64).wrapping_add(1).wrapping_mul(0x9e37_79b9_7f4a_7c15);
    z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
    z ^= z >> 31;
    (z >> 11) as f64 / (1u64 << 53) as f64
}

/// A point's width randomized: its base, times one plus the deviation drawn for it (percent, between `min` and `max`).
#[must_use]
pub fn randomized(base: f64, min: f64, max: f64, seed: u64, point: usize) -> f64 {
    let (lo, hi) = (min.min(max), min.max(max));
    base * (1.0 + (lo + (hi - lo) * unit(seed, point)) / 100.0)
}

impl Tile {
    /// What a river's point `width` is multiplied by to give the water as drawn (metres): a battle river's file factor, a campaign
    /// river's unit.
    pub(crate) fn river_width_scale(&self, elem: usize) -> f64 {
        if wce_tile::tile_project::is_campaign_tile(&self.project.dir) {
            1.0 / wce_tile::tile_project::height_unit(&self.project.dir)
        } else {
            self.river_width_factor(elem)
        }
    }

    /// A river's width handles: at each point, the point (on the ground as drawn) and its water's two edges, across the river's way
    /// there (world metres). None for anything but a river.
    #[must_use]
    pub fn river_width_handles(&self, river: usize) -> Vec<(Float3, Float3, Float3)> {
        let sc = self.tscene();
        if river >= sc.doc().elements().len() || sc.type_of(river) != "SplineRiverInstance" {
            return vec![];
        }

        let scale = self.river_width_scale(river);
        let elems = sc.polygon_points(river);
        let pts = self.polygon_display_points(river);
        let handles = self.spline_world_handles(river);
        let n = pts.len();
        (0..n)
            .map(|i| {
                let p = pts[i];
                // The way the river runs there: its handles, else its neighbours.
                let (hin, hout) = handles.get(i).copied().unwrap_or((None, None));
                let mut t = match (hin, hout) {
                    (Some(a), Some(b)) => (b.x - a.x, b.z - a.z),
                    (None, Some(b)) => (b.x - p.x, b.z - p.z),
                    (Some(a), None) => (p.x - a.x, p.z - a.z),
                    (None, None) => (0.0, 0.0),
                };
                if t.0.hypot(t.1) < 1e-4 {
                    let (a, b) = (pts[i.saturating_sub(1)], pts[(i + 1).min(n - 1)]);
                    t = (b.x - a.x, b.z - a.z);
                }

                let len = t.0.hypot(t.1).max(1e-6);
                let normal = (-t.1 / len, t.0 / len);
                let width = elems.get(i).and_then(|&e| sc.get(e, "width")).and_then(|v| v.trim().parse::<f64>().ok()).unwrap_or(1.0);
                let half = (width * scale * 0.5) as f32;
                let edge = |s: f32| Float3::new(p.x + normal.0 * half * s, p.y, p.z + normal.1 * half * s);
                (p, edge(1.0), edge(-1.0))
            })
            .collect()
    }

    /// A river's width settings, its own or the defaults.
    #[must_use]
    pub fn river_width_settings(&self, river: usize) -> WidthSettings {
        let sc = self.tscene();
        let num = |a: &str| sc.get(river, a).and_then(|v| v.trim().parse::<f64>().ok());
        let seed = sc
            .get(river, WIDTH_SEED_ATTR)
            .or_else(|| sc.get(river, RIVER_ID_ATTR))
            .and_then(|v| v.trim().parse::<u64>().ok())
            .unwrap_or(river as u64);
        WidthSettings {
            min: num(WIDTH_DEV_MIN_ATTR).unwrap_or(DEFAULT_WIDTH_DEV.0),
            max: num(WIDTH_DEV_MAX_ATTR).unwrap_or(DEFAULT_WIDTH_DEV.1),
            seed,
        }
    }
}

impl TileDocument {
    /// Sets a river point's width to `metres` of water as drawn (no narrower than [`MIN_WIDTH_M`]): a width handle's drag, whose steps
    /// (one `gesture`) merge into one undo entry. The point's randomizing base goes: this width is the one randomized next.
    pub fn set_river_point_width(&mut self, river: usize, point: usize, metres: f64, gesture: u64) -> bool {
        let t = self.tile();
        let sc = t.tscene();
        if river >= sc.doc().elements().len() || sc.type_of(river) != "SplineRiverInstance" {
            return false;
        }

        let Some(&p) = sc.polygon_points(river).get(point) else { return false };
        let width = format_number(metres.max(MIN_WIDTH_M) / t.river_width_scale(river));
        let mut steps = vec![(point, "width".to_owned(), sc.get(p, "width").unwrap_or_else(|| super::commands::ABSENT.to_owned()), width)];
        if let Some(base) = sc.get(p, WIDTH_BASE_ATTR) {
            steps.push((point, WIDTH_BASE_ATTR.to_owned(), base, String::new()));
        }

        self.history.push(Box::new(TedPointAttrsCommand { name: "River width".to_owned(), elem: river, steps, gesture }));
        true
    }

    /// Randomizes a river's widths (one undo entry): each point's, from its base (the width it had before it was first randomized),
    /// times one plus a deviation between `min` and `max` percent drawn from `seed` and the point; the points joined to a river link
    /// keep theirs (the link's 45 m). The settings are kept on the river. Returns how many points changed.
    ///
    /// # Errors
    ///
    /// Not a river.
    pub fn randomize_river_widths(&mut self, river: usize, min: f64, max: f64, seed: u64) -> Result<usize, String> {
        self.set_river_widths(river, Some((min, max, seed)))
    }

    /// Puts a river's randomized widths back to their bases (one undo entry). Returns how many points changed.
    ///
    /// # Errors
    ///
    /// Not a river.
    pub fn reset_river_widths(&mut self, river: usize) -> Result<usize, String> {
        self.set_river_widths(river, None)
    }

    fn set_river_widths(&mut self, river: usize, random: Option<(f64, f64, u64)>) -> Result<usize, String> {
        let t = self.tile();
        let sc = t.tscene();
        if river >= sc.doc().elements().len() || sc.type_of(river) != "SplineRiverInstance" {
            return Err("select a river".to_owned());
        }

        let floor = MIN_WIDTH_M / t.river_width_scale(river);
        let mut steps = vec![];
        for (i, &p) in sc.polygon_points(river).iter().enumerate() {
            if sc.get(p, LINK_ATTR).is_some_and(|v| !v.trim().is_empty()) {
                continue;
            }

            let width = sc.get(p, "width").unwrap_or_else(|| super::commands::ABSENT.to_owned());
            let base_text = sc.get(p, WIDTH_BASE_ATTR).filter(|v| !v.trim().is_empty());
            let Some(base) = base_text.as_deref().or(Some(width.as_str())).and_then(|v| v.trim().parse::<f64>().ok()) else { continue };
            let (new_width, new_base) = match random {
                Some((min, max, seed)) => (format_number(randomized(base, min, max, seed, i).max(floor)), format_number(base)),
                None if base_text.is_some() => (format_number(base), String::new()),
                None => continue,
            };
            if new_width != width {
                steps.push((i, "width".to_owned(), width, new_width));
            }

            // A point without a base: put back, it has none again (not an empty one).
            let old_base = sc.get(p, WIDTH_BASE_ATTR).unwrap_or_else(|| super::commands::ABSENT.to_owned());
            if old_base != new_base && !(old_base == super::commands::ABSENT && new_base.is_empty()) {
                steps.push((i, WIDTH_BASE_ATTR.to_owned(), old_base, new_base));
            }
        }

        let changed = steps.iter().filter(|s| s.1 == "width").count();
        self.end_stroke();
        let mark = self.history().position();
        if let Some((min, max, seed)) = random {
            for (attr, value) in
                [(WIDTH_DEV_MIN_ATTR, format_number(min)), (WIDTH_DEV_MAX_ATTR, format_number(max)), (WIDTH_SEED_ATTR, seed.to_string())]
            {
                if self.tile().tscene().get(river, attr).as_deref() != Some(value.as_str()) {
                    self.set_attribute(river, attr, &value);
                }
            }
        }

        if !steps.is_empty() {
            let name = if random.is_some() { "Randomize river widths" } else { "Reset river widths" };
            let g = self.next_gesture();
            self.history.push(Box::new(TedPointAttrsCommand { name: name.to_owned(), elem: river, steps, gesture: g }));
        }

        let n = self.history().position() - mark;
        if n > 1 {
            self.join_last(n, if random.is_some() { "Randomize river widths" } else { "Reset river widths" });
        }

        Ok(changed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_randomized_width_stays_within_its_deviation_and_follows_its_seed() {
        for i in 0..200 {
            let w = randomized(40.0, -25.0, 10.0, 7, i);
            assert!((30.0..=44.0).contains(&w), "{w}");
        }

        assert_eq!(randomized(40.0, -25.0, 10.0, 7, 3), randomized(40.0, -25.0, 10.0, 7, 3), "the same seed, the same width");
        assert_ne!(randomized(40.0, -25.0, 10.0, 7, 3), randomized(40.0, -25.0, 10.0, 8, 3), "another seed, another");
        assert_eq!(randomized(40.0, 10.0, -25.0, 7, 3), randomized(40.0, -25.0, 10.0, 7, 3), "either order");
        assert_eq!(randomized(40.0, 0.0, 0.0, 7, 3), 40.0, "no deviation, no change");
    }
}
