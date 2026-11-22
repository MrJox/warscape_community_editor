//! Shores (docs/plans/rivers.md, step 10): reeds and stones along a river's or a lake's water, as the shelved ground coverage's Shores had
//! them, laid along the water the editor knows rather than found in the heights. Reeds stand in the shallows (the water up to
//! [`ShoreSettings::shallows`] deep) and thinner on the bank at its edge; stones lie along the bank (within [`ShoreSettings::bank`] metres
//! of the water and less than [`ShoreSettings::rise`] over it) and a few in the shallows.
//!
//! Each place of a 1 m grid draws its own numbers from the seed and its cell, so a shore keeps what it has where nothing changed when its
//! water is edited elsewhere. Everything is in the editor's metres.

/// The built-in stones and reeds (Attila's: the built-in biomes' rocks, the Reeds and marsh biome's undergrowth).
pub const STONES: [&str; 2] = ["gaul_rock_01", "gaul_rock_02"];
pub const REEDS: [&str; 2] = ["shrub_calamus.shrub_calamus_a", "shrubs_med.water_a"];

/// How thick a shore is: reeds and stones per square metre of the shallows and the bank, how deep the shallows go, how wide the bank is
/// and how far over the water it may stand.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ShoreSettings {
    pub reeds: f32,
    pub stones: f32,
    pub shallows: f32,
    pub bank: f32,
    pub rise: f32,
}

impl Default for ShoreSettings {
    fn default() -> Self {
        Self { reeds: 1.0 / 6.0, stones: 1.0 / 25.0, shallows: 0.5, bank: 5.0, rise: 1.2 }
    }
}

/// The amounts a shore's reeds and stones can be set to, by name: per square metre.
pub const AMOUNTS: [(&str, f32); 4] = [("none", 0.0), ("few", 1.0 / 12.0), ("some", 1.0 / 6.0), ("thick", 1.0 / 3.0)];

/// Stones are rarer than reeds by this, at the same amount.
const STONE_SHARE: f32 = 0.25;

/// An amount's density (per square metre) by its name; none for an unknown one.
pub fn amount(name: &str) -> Option<f32> {
    AMOUNTS.iter().find(|a| a.0 == name.trim()).map(|a| a.1)
}

impl ShoreSettings {
    /// The settings of named amounts of reeds and stones ([`AMOUNTS`]).
    pub fn of(reeds: f32, stones: f32) -> Self {
        Self { reeds, stones: stones * STONE_SHARE, ..Self::default() }
    }
}

/// A water a shore runs along.
#[derive(Clone, Debug, PartialEq)]
pub enum Water {
    /// A river: its middle line's samples (x, z), the water's height there and its half width.
    River { line: Vec<[f32; 2]>, water: Vec<f32>, half: Vec<f32> },

    /// A lake: its outline (x, z) and its water's height.
    Lake { outline: Vec<[f32; 2]>, water: f32 },
}

impl Water {
    /// The water's edge from a place: how far outside it (negative: inside), and the water's height at the nearest place.
    fn edge(&self, x: f32, z: f32) -> (f32, f32) {
        match self {
            Water::River { line, water, half } => {
                let mut best = (f32::INFINITY, 0.0);
                for i in 0..line.len().saturating_sub(1) {
                    let (a, b) = (line[i], line[i + 1]);
                    let (dx, dz) = (b[0] - a[0], b[1] - a[1]);
                    let l2 = dx * dx + dz * dz;
                    let t = if l2 > 1e-9 { (((x - a[0]) * dx + (z - a[1]) * dz) / l2).clamp(0.0, 1.0) } else { 0.0 };
                    let h = half[i] + (half[i + 1] - half[i]) * t;
                    let d = (x - a[0] - dx * t).hypot(z - a[1] - dz * t) - h;
                    if d < best.0 {
                        best = (d, water[i] + (water[i + 1] - water[i]) * t);
                    }
                }

                best
            }

            Water::Lake { outline, water } => {
                let n = outline.len();
                let (mut inside, mut near) = (false, f32::INFINITY);
                for i in 0..n {
                    let (a, b) = (outline[i], outline[(i + n - 1) % n]);
                    if (a[1] > z) != (b[1] > z) && x < (b[0] - a[0]) * (z - a[1]) / (b[1] - a[1]) + a[0] {
                        inside = !inside;
                    }

                    let (dx, dz) = (b[0] - a[0], b[1] - a[1]);
                    let l2 = dx * dx + dz * dz;
                    let t = if l2 > 1e-9 { (((x - a[0]) * dx + (z - a[1]) * dz) / l2).clamp(0.0, 1.0) } else { 0.0 };
                    near = near.min((x - a[0] - dx * t).hypot(z - a[1] - dz * t));
                }

                (if inside { -near } else { near }, *water)
            }
        }
    }

    /// Its bounds (x0, z0, x1, z1), grown by `m` metres.
    fn bounds(&self, m: f32) -> Option<[f32; 4]> {
        let pts: Box<dyn Iterator<Item = ([f32; 2], f32)>> = match self {
            Water::River { line, half, .. } => Box::new(line.iter().copied().zip(half.iter().copied())),
            Water::Lake { outline, .. } => Box::new(outline.iter().map(|&p| (p, 0.0))),
        };
        pts.map(|(p, h)| [p[0] - h - m, p[1] - h - m, p[0] + h + m, p[1] + h + m])
            .reduce(|a, b| [a[0].min(b[0]), a[1].min(b[1]), a[2].max(b[2]), a[3].max(b[3])])
    }
}

/// A reed or a stone laid: its key, its place (x, y on the ground, z), its scale and its turn (degrees).
#[derive(Clone, Debug, PartialEq)]
pub struct Placed {
    pub key: String,
    pub x: f32,
    pub y: f32,
    pub z: f32,
    pub scale: f32,
    pub yaw: f32,
}

/// A shore's reeds and stones.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Shore {
    pub reeds: Vec<Placed>,
    pub stones: Vec<Placed>,
}

/// Four numbers in [0, 1) of a grid cell and a seed (a hash: the same cell gives the same numbers).
fn cell_numbers(seed: u64, i: i64, j: i64) -> [f32; 4] {
    let mut h = seed ^ (i as u64).wrapping_mul(0x9e37_79b9_7f4a_7c15) ^ (j as u64).wrapping_mul(0xc2b2_ae3d_27d4_eb4f);
    let mut out = [0.0; 4];
    for v in &mut out {
        h ^= h >> 33;
        h = h.wrapping_mul(0xff51_afd7_ed55_8ccd);
        h ^= h >> 33;
        h = h.wrapping_mul(0xc4ce_b9fe_1a85_ec53);
        h ^= h >> 33;
        *v = (h >> 40) as f32 / (1u64 << 24) as f32;
    }

    out
}

/// The grid's cell, metres: one draw a cell (so a density over 1 a square metre is 1).
const CELL: f32 = 1.0;

/// The shore of a water: over a 1 m grid round it, each place in the shallows (under the water, up to `shallows` deep) or on the bank
/// (dry, within `bank` of the water's edge and up to `rise` over it) draws a reed and a stone by their densities there (reeds thick in the
/// shallows, half as thick on the bank's first half; stones on the bank, a quarter as many in the shallows), placed in the cell by its own
/// numbers, off the places `blocked` says. `height`: the ground's.
pub fn place(water: &Water, s: &ShoreSettings, height: &dyn Fn(f32, f32) -> f32, blocked: &dyn Fn(f32, f32) -> bool, seed: u64) -> Shore {
    let mut out = Shore::default();
    let Some([x0, z0, x1, z1]) = water.bounds(s.bank + CELL) else { return out };
    let (i0, i1) = ((x0 / CELL).floor() as i64, (x1 / CELL).ceil() as i64);
    let (j0, j1) = ((z0 / CELL).floor() as i64, (z1 / CELL).ceil() as i64);
    for j in j0..j1 {
        for i in i0..i1 {
            let [a, b, c, d] = cell_numbers(seed, i, j);
            let (x, z) = ((i as f32 + b) * CELL, (j as f32 + c) * CELL);
            let (edge, level) = water.edge(x, z);
            if edge > s.bank {
                continue;
            }

            let g = height(x, z);
            let depth = level - g;
            let (reed, stone) = if depth > 0.0 {
                // The shallows: under the water, not too deep.
                if depth > s.shallows { (0.0, 0.0) } else { (s.reeds, s.stones * 0.25) }
            } else if -depth <= s.rise && edge >= -0.5 {
                // The bank: its first half has reeds.
                (if edge <= s.bank * 0.5 { s.reeds * 0.5 } else { 0.0 }, s.stones)
            } else {
                (0.0, 0.0)
            };

            let area = CELL * CELL;
            if a < reed * area && !blocked(x, z) {
                let key = REEDS[(d * REEDS.len() as f32) as usize % REEDS.len()];
                out.reeds.push(Placed { key: key.to_owned(), x, y: g, z, scale: 0.8 + 0.5 * c, yaw: 360.0 * d });
            } else if a >= 1.0 - stone * area && !blocked(x, z) {
                let key = STONES[(d * STONES.len() as f32) as usize % STONES.len()];
                out.stones.push(Placed { key: key.to_owned(), x, y: g, z, scale: 0.5 + 0.5 * b, yaw: 360.0 * b });
            }
        }
    }

    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A straight river along x at z 50, 10 m wide, its water at 0; the ground a slope from 2 m under it in the middle up to the sides.
    fn river() -> (Water, impl Fn(f32, f32) -> f32) {
        let w = Water::River { line: vec![[0.0, 50.0], [100.0, 50.0]], water: vec![0.0, 0.0], half: vec![5.0, 5.0] };
        (w, |_: f32, z: f32| ((z - 50.0).abs() - 6.0) * 0.4)
    }

    #[test]
    fn reeds_stand_in_the_shallows_and_stones_on_the_bank() {
        let (w, g) = river();
        let shore = place(&w, &ShoreSettings::of(1.0 / 3.0, 1.0 / 3.0), &g, &|_, _| false, 7);
        assert!(!shore.reeds.is_empty() && !shore.stones.is_empty());
        for r in &shore.reeds {
            let depth = -g(r.x, r.z);
            assert!(depth <= 0.5 + 1e-4, "no reed deeper than the shallows: {depth} at {}", r.z);
            assert!((r.y - g(r.x, r.z)).abs() < 1e-4, "on the ground");
        }

        for s in &shore.stones {
            assert!(g(s.x, s.z) <= 1.2 + 1e-4 && (s.z - 50.0).abs() <= 5.0 + 5.0 + 1.0, "on the bank: {} at {}", g(s.x, s.z), s.z);
        }

        // None in the deep middle, none far up the bank.
        assert!(shore.reeds.iter().chain(&shore.stones).all(|p| (p.z - 50.0).abs() > 4.0 && (p.z - 50.0).abs() < 11.0));
    }

    #[test]
    fn a_shore_keeps_its_places_and_its_blocked_ground() {
        let (w, g) = river();
        let s = ShoreSettings::default();
        let a = place(&w, &s, &g, &|_, _| false, 3);
        assert_eq!(a, place(&w, &s, &g, &|_, _| false, 3), "the same each time");
        assert_ne!(a, place(&w, &s, &g, &|_, _| false, 4), "another seed, another shore");
        let b = place(&w, &s, &g, &|x, _| x < 50.0, 3);
        assert!(b.reeds.iter().chain(&b.stones).all(|p| p.x >= 50.0));
        assert!(b.reeds.iter().all(|p| a.reeds.contains(p)), "what is left is where it was");
        assert!(place(&w, &ShoreSettings::of(0.0, 0.0), &g, &|_, _| false, 3) == Shore::default(), "none");
    }

    #[test]
    fn a_lakes_shore_runs_round_its_outline() {
        let w = Water::Lake { outline: vec![[0.0, 0.0], [40.0, 0.0], [40.0, 40.0], [0.0, 40.0]], water: 0.0 };
        // A bowl: 0.3 under the water 1 m in from the outline, deeper further in, rising outside.
        let g = |x: f32, z: f32| {
            let e = x.min(40.0 - x).min(z).min(40.0 - z);
            if e > 0.0 { -0.3 * e } else { -0.2 * e }
        };
        let shore = place(&w, &ShoreSettings::of(1.0 / 3.0, 1.0 / 3.0), &g, &|_, _| false, 1);
        assert!(!shore.reeds.is_empty());
        assert!(shore.reeds.iter().all(|r| g(r.x, r.z) >= -0.5 - 1e-4), "in the shallows or on the bank");
        assert!(shore.reeds.iter().all(|r| !(r.x > 3.0 && r.x < 37.0 && r.z > 3.0 && r.z < 37.0)), "none in the deep middle");
    }

    #[test]
    fn the_amounts_read_by_name() {
        assert_eq!(amount("thick"), Some(1.0 / 3.0));
        assert_eq!(amount(" none "), Some(0.0));
        assert_eq!(amount("lots"), None);
    }
}
