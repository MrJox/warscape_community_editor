//! A river's shore by its points: a point's own reeds, stones, shore decals, bed decals (the river's attributes' names, on the point:
//! an amount's name; missing: as the river) and cliff banks (`river_cliffs.rs`) over the river's, from halfway to the point before to
//! halfway to the one after (`wce_tile::shores::ShoreStretch`); and the shore variation, which gives the points such values by a seed:
//! runs of points with a cliff on the outer bank of their bend, and amounts a step more or less than the river's. A point set by hand
//! keeps its values; the variation's are marked `wce_shore_random` and drawn again (or taken away) whole.

use wce_tile::shores::{AMOUNTS, ShoreSettings, ShoreStretch, amount};

use super::commands::{ABSENT, TedPointAttrsCommand};
use super::river_cliffs::{CLIFF_ATTR, OUTER_BEND_DEG, cliff_mark};
use super::river_links::RIVER_ID_ATTR;
use super::shores::{BED_DECALS_ATTR, DECALS_ATTR, REEDS_ATTR, STONES_ATTR};
use super::view::RiverSample;
use super::{Tile, TileDocument};

/// The amounts a point can have of its own, the Inspector's order.
pub const POINT_AMOUNT_ATTRS: [&str; 4] = [REEDS_ATTR, STONES_ATTR, DECALS_ATTR, BED_DECALS_ATTR];

/// Every attribute of a point's own shore.
pub const POINT_SHORE_ATTRS: [&str; 5] = [REEDS_ATTR, STONES_ATTR, DECALS_ATTR, BED_DECALS_ATTR, CLIFF_ATTR];

/// A point whose shore values are the variation's (`1`), not set by hand.
pub const RANDOM_ATTR: &str = "wce_shore_random";

/// The river's shore variation: how likely a point starts a run of cliff points, and each of its amounts changes (percent), and the
/// seed; missing: [`DEFAULT_VARIATION`], the seed the river's id.
pub const VAR_CLIFFS_ATTR: &str = "wce_shore_var_cliffs";
pub const VAR_AMOUNTS_ATTR: &str = "wce_shore_var_amounts";
pub const VAR_SEED_ATTR: &str = "wce_shore_var_seed";
pub const DEFAULT_VARIATION: (f64, f64) = (15.0, 30.0);

/// A river's shore variation.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ShoreVariation {
    pub cliffs: f64,
    pub amounts: f64,
    pub seed: u64,
}

/// A number from 0 to 1 of a seed and a draw (splitmix64).
fn unit(seed: u64, n: u64) -> f64 {
    let mut z = seed ^ n.wrapping_add(1).wrapping_mul(0x9e37_79b9_7f4a_7c15);
    z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
    z ^= z >> 31;
    (z >> 11) as f64 / (1u64 << 53) as f64
}

impl Tile {
    /// A river point's own amount of one of [`POINT_AMOUNT_ATTRS`] (its name); none: as the river.
    #[must_use]
    pub fn river_point_amount(&self, river: usize, point: usize, attr: &str) -> Option<&'static str> {
        let sc = self.tscene();
        let &p = sc.polygon_points(river).get(point)?;
        let v = sc.get(p, attr)?;
        AMOUNTS.iter().map(|a| a.0).find(|a| *a == v.trim())
    }

    /// Whether a river point's shore values are the variation's.
    #[must_use]
    pub fn river_point_random(&self, river: usize, point: usize) -> bool {
        let sc = self.tscene();
        sc.polygon_points(river).get(point).is_some_and(|&p| sc.get(p, RANDOM_ATTR).as_deref().map(str::trim) == Some("1"))
    }

    /// A river's stretches whose shore is its points' own (`base`: the river's settings, the point's amounts over them).
    pub(super) fn river_shore_stretches(&self, river: usize, samples: &[RiverSample], base: &ShoreSettings) -> Vec<ShoreStretch> {
        let spans = self.river_point_spans(river, samples);
        let mut out = vec![];
        for (i, &(from, to)) in spans.iter().enumerate() {
            let own = POINT_AMOUNT_ATTRS.map(|a| self.river_point_amount(river, i, a).and_then(amount));
            if own.iter().all(Option::is_none) {
                continue;
            }

            let [reeds, stones, decals, bed_decals] = own;
            let settings = ShoreSettings {
                reeds: reeds.unwrap_or(base.reeds),
                stones: stones.unwrap_or(base.stones),
                decals: decals.unwrap_or(base.decals),
                bed_decals: bed_decals.unwrap_or(base.bed_decals),
                ..*base
            };
            out.push(ShoreStretch { from: from as f32, to: to as f32, settings });
        }

        out
    }

    /// A river's shore variation (its own, else the defaults and its id for a seed).
    #[must_use]
    pub fn river_shore_variation(&self, river: usize) -> ShoreVariation {
        let sc = self.tscene();
        let num = |a: &str| sc.get(river, a).and_then(|v| v.trim().parse::<f64>().ok());
        let seed = sc
            .get(river, VAR_SEED_ATTR)
            .or_else(|| sc.get(river, RIVER_ID_ATTR))
            .and_then(|v| v.trim().parse::<u64>().ok())
            .unwrap_or(river as u64);
        ShoreVariation {
            cliffs: num(VAR_CLIFFS_ATTR).unwrap_or(DEFAULT_VARIATION.0),
            amounts: num(VAR_AMOUNTS_ATTR).unwrap_or(DEFAULT_VARIATION.1),
            seed,
        }
    }

    /// The shore values the variation gives a river's points (each point's attribute and value; none: the river's), by its seed:
    /// runs of two or three points (not its ends) started at a point by its cliff chance, a cliff on the outer bank of the run's middle
    /// point's bend (a side by the seed where it runs straight), and each amount a step more or less than the river's by its amount
    /// chance.
    fn shore_variation_values(&self, river: usize, v: &ShoreVariation, river_amounts: &[&str; 4]) -> Vec<Vec<(&'static str, String)>> {
        let n = self.tscene().polygon_points(river).len();
        let mut out = vec![vec![]; n];
        let mut i = 1;
        while i + 1 < n {
            if unit(v.seed, (i * 8) as u64) * 100.0 < v.cliffs {
                let run = if unit(v.seed, (i * 8 + 1) as u64) < 0.5 { 2 } else { 3 };
                let last = (i + run - 1).min(n - 2);
                let mid = (i + last) / 2;
                let side = if self.river_bend_deg(river, mid) >= OUTER_BEND_DEG * 0.5 {
                    self.river_outer_side(river, mid)
                } else if unit(v.seed, (i * 8 + 2) as u64) < 0.5 {
                    1.0
                } else {
                    -1.0
                };
                for values in &mut out[i..=last] {
                    values.push((CLIFF_ATTR, cliff_mark(side).to_owned()));
                }

                // A gap of a point at least before the next run.
                i = last + 2;
            } else {
                i += 1;
            }
        }

        for (i, values) in out.iter_mut().enumerate() {
            for (k, attr) in POINT_AMOUNT_ATTRS.iter().enumerate() {
                let draw = (i * 8 + 3 + k) as u64;
                if unit(v.seed, draw) * 100.0 >= v.amounts {
                    continue;
                }

                let at = AMOUNTS.iter().position(|a| a.0 == river_amounts[k]).unwrap_or(0) as i64;
                let step = if unit(v.seed ^ 0x5eed, draw) < 0.5 { -1 } else { 1 };
                // At an end of the amounts, the step goes the other way.
                let to = if at + step < 0 || at + step >= AMOUNTS.len() as i64 { at - step } else { at + step };
                values.push((attr, AMOUNTS[to.clamp(0, AMOUNTS.len() as i64 - 1) as usize].0.to_owned()));
            }
        }

        out
    }
}

impl TileDocument {
    /// Sets one of river points' own shore values (`attr`: one of [`POINT_SHORE_ATTRS`]) or takes it away (none: as the river); set by
    /// hand, the points' values are no longer the variation's. Its terrain patch turned on when it has a cliff. One undo entry named
    /// `name`.
    pub fn set_river_point_shore(&mut self, river: usize, points: &[usize], attr: &str, value: Option<&str>, name: &str) -> bool {
        use wce_editor_core::document::Document;
        let sc = self.tile().tscene();
        if sc.type_of(river) != "SplineRiverInstance" || !POINT_SHORE_ATTRS.contains(&attr) {
            return false;
        }

        let pts = sc.polygon_points(river);
        let mut steps = vec![];
        for &i in points {
            let Some(&p) = pts.get(i) else { return false };
            let old = sc.get(p, attr).unwrap_or_else(|| ABSENT.to_owned());
            let new = value.map_or_else(|| ABSENT.to_owned(), str::to_owned);
            if old != new {
                steps.push((i, attr.to_owned(), old, new));
            }

            if let Some(r) = sc.get(p, RANDOM_ATTR) {
                steps.push((i, RANDOM_ATTR.to_owned(), r, ABSENT.to_owned()));
            }
        }

        if steps.is_empty() {
            return true;
        }

        self.end_stroke();
        let before = self.history().position();
        let g = self.next_gesture();
        self.history.push(Box::new(TedPointAttrsCommand { name: name.to_owned(), elem: river, steps, gesture: g }));
        self.patch_for_cliffs(river);
        let n = self.history().position() - before;
        if n > 1 {
            self.join_last(n, name);
        }

        true
    }

    /// Gives a river's points the shore variation's values by its settings (kept on the river), in place of those it gave them before;
    /// the points set by hand keep theirs. One undo entry. Returns how many points have values of the variation.
    ///
    /// # Errors
    ///
    /// Not a river.
    pub fn randomize_river_shore(&mut self, river: usize, v: ShoreVariation) -> Result<usize, String> {
        self.set_river_shore_variation(river, Some(v))
    }

    /// Takes the shore variation's values off a river's points (those set by hand stay). One undo entry. Returns how many points had
    /// them.
    ///
    /// # Errors
    ///
    /// Not a river.
    pub fn reset_river_shore(&mut self, river: usize) -> Result<usize, String> {
        self.set_river_shore_variation(river, None)
    }

    fn set_river_shore_variation(&mut self, river: usize, v: Option<ShoreVariation>) -> Result<usize, String> {
        use wce_editor_core::document::Document;
        let t = self.tile();
        let sc = t.tscene();
        if river >= sc.doc().elements().len() || sc.type_of(river) != "SplineRiverInstance" {
            return Err("select a river".to_owned());
        }

        let river_amounts = POINT_AMOUNT_ATTRS.map(|a| {
            let current = sc.get(river, a).unwrap_or_default();
            AMOUNTS.iter().map(|x| x.0).find(|x| *x == current.trim()).unwrap_or(super::shores::DEFAULT_AMOUNT)
        });
        let values = v.map(|v| t.shore_variation_values(river, &v, &river_amounts));
        let mut steps = vec![];
        let mut varied = 0;
        for (i, &p) in sc.polygon_points(river).iter().enumerate() {
            let random = t.river_point_random(river, i);
            let by_hand = !random && POINT_SHORE_ATTRS.iter().any(|a| sc.get(p, a).is_some_and(|v| !v.trim().is_empty()));
            if by_hand {
                continue;
            }

            let new = values.as_ref().map(|v| v[i].clone()).unwrap_or_default();
            for attr in POINT_SHORE_ATTRS {
                let old = sc.get(p, attr).unwrap_or_else(|| ABSENT.to_owned());
                let value = new.iter().find(|(a, _)| *a == attr).map_or_else(|| ABSENT.to_owned(), |(_, v)| v.clone());
                if old != value {
                    steps.push((i, attr.to_owned(), old, value));
                }
            }

            let mark = if new.is_empty() { ABSENT.to_owned() } else { "1".to_owned() };
            let old = sc.get(p, RANDOM_ATTR).unwrap_or_else(|| ABSENT.to_owned());
            if old != mark {
                steps.push((i, RANDOM_ATTR.to_owned(), old, mark));
            }

            varied += usize::from(!new.is_empty() || (v.is_none() && random));
        }

        self.end_stroke();
        let before = self.history().position();
        if let Some(v) = v {
            for (attr, value) in [
                (VAR_CLIFFS_ATTR, wce_tile::ted_scene::format_number(v.cliffs)),
                (VAR_AMOUNTS_ATTR, wce_tile::ted_scene::format_number(v.amounts)),
                (VAR_SEED_ATTR, v.seed.to_string()),
            ] {
                if self.tile().tscene().get(river, attr).as_deref() != Some(value.as_str()) {
                    self.set_attribute(river, attr, &value);
                }
            }
        }

        let name = if v.is_some() { "Randomize river shore" } else { "Reset river shore" };
        if !steps.is_empty() {
            let g = self.next_gesture();
            self.history.push(Box::new(TedPointAttrsCommand { name: name.to_owned(), elem: river, steps, gesture: g }));
        }

        self.patch_for_cliffs(river);
        let n = self.history().position() - before;
        if n > 1 {
            self.join_last(n, name);
        }

        Ok(varied)
    }
}
