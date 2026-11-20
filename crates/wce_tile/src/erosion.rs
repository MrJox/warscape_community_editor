//! Erosion and fine detail over a tile's heights: rain drops that wear channels down the slopes and drop their load where they slow
//! (hydraulic), loose ground sliding down where it is steeper than it can stand (thermal), and fine noise, stronger on steep ground.
//! What comes out is a height change per sample plus where the water ran and where it dropped what it carried, for the texture rules.
//!
//! The heights are a grid of `w` × `h` samples in metres, `mpp` metres apart; a mask (0 .. 1 a sample) scales everything, so the ground
//! under buildings and roads stays as it is. The same settings and seed give the same result.

use std::fmt::Write as _;

/// The owner of the erosion's height layer (`erosion#<signature>`), over the user's layers (`terrain_layers::OVER_USER_OWNERS`).
pub const OWNER: &str = "erosion";

/// The tile's erosion settings, beside tile.wce.
pub const FILE: &str = "erosion.wce";

/// How the ground is worn. Lengths are metres; the rates are fractions a step.
#[derive(Debug, Clone, PartialEq)]
pub struct Settings {
    /// Rain drops per 100 m² of ground.
    pub rain: f32,
    /// The most steps a drop runs (a step is one sample).
    pub steps: u32,
    /// How much a drop keeps its direction (0: straight down the slope, 1: never turns).
    pub inertia: f32,
    /// How much a drop carries (metres of ground, spread over its radius) on a slope of 45°, at speed 1 and full water; more on steeper
    /// ground and faster, less as it dries.
    pub capacity: f32,
    /// The share of what a drop could still carry that it takes from the ground a step.
    pub erode: f32,
    /// The share of what a drop carries past its capacity that it lays a step.
    pub deposit: f32,
    /// The share of a drop's water lost a step.
    pub evaporate: f32,
    /// How far round its path a drop wears the ground (metres): wider is smoother channels.
    pub radius: f32,
    /// The steepest loose ground stands (degrees); steeper slides down.
    pub talus: f32,
    /// How many times loose ground slides.
    pub thermal_passes: u32,
    /// Fine noise's height (metres) on steep ground; a third of it on flat ground.
    pub detail: f32,
    /// Fine noise's size (metres from bump to bump).
    pub detail_scale: f32,
    /// How much of the change is kept (0 .. 1).
    pub strength: f32,
    pub seed: u64,
}

impl Default for Settings {
    fn default() -> Self {
        PRESETS[1].1.clone()
    }
}

/// The built-in settings by name.
pub static PRESETS: std::sync::LazyLock<Vec<(&'static str, Settings)>> = std::sync::LazyLock::new(|| {
    let base = Settings {
        rain: 4.0,
        steps: 48,
        inertia: 0.1,
        capacity: 0.06,
        erode: 0.3,
        deposit: 0.3,
        evaporate: 0.02,
        radius: 2.5,
        talus: 40.0,
        thermal_passes: 4,
        detail: 0.25,
        detail_scale: 6.0,
        strength: 1.0,
        seed: 1,
    };
    vec![
        ("Light", Settings { rain: 2.0, capacity: 0.03, erode: 0.2, thermal_passes: 2, detail: 0.15, ..base.clone() }),
        ("Weathered", base.clone()),
        (
            "Rugged",
            Settings { rain: 8.0, steps: 64, capacity: 0.12, erode: 0.4, radius: 2.0, talus: 35.0, thermal_passes: 8, detail: 0.4, ..base },
        ),
    ]
});

/// What the erosion did.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Eroded {
    /// The height change a sample, metres (the mask and strength applied).
    pub delta: Vec<f32>,
    /// Where the water ran: 0 (none) .. 1 (a stream); a sample with as much water as the average gets about a quarter.
    pub flow: Vec<f32>,
    /// What the water laid there, metres (0 where it took ground away).
    pub deposit: Vec<f32>,
}

/// Erodes `heights` (`w` × `h` samples `mpp` metres apart), scaled by `mask` (one a sample, or empty for everywhere). `cancel` is
/// polled between drops; when it says stop, `None`.
#[must_use]
pub fn erode(heights: &[f32], w: usize, h: usize, mpp: f32, mask: &[f32], s: &Settings, cancel: &dyn Fn() -> bool) -> Option<Eroded> {
    let n = w * h;
    if w < 3 || h < 3 || heights.len() != n || !(mpp > 0.0) {
        return Some(Eroded { delta: vec![0.0; heights.len()], flow: vec![0.0; heights.len()], deposit: vec![0.0; heights.len()] });
    }
    let mask_at = |i: usize| if mask.len() == n { mask[i].clamp(0.0, 1.0) } else { 1.0 };
    let mut g = heights.to_vec();
    let mut visits = vec![0.0f32; n];
    let mut laid = vec![0.0f32; n];

    // Hydraulic: the drops.
    let brush = Brush::new((s.radius / mpp).max(0.5));
    let area_m2 = (w - 1) as f32 * (h - 1) as f32 * mpp * mpp;
    let drops = ((area_m2 / 100.0 * s.rain.max(0.0)) as usize).min(20_000_000);
    let mut rng = Random::new(s.seed);
    for d in 0..drops {
        if d % 4096 == 0 && cancel() {
            return None;
        }
        let mut x = rng.unit() * (w - 2) as f32;
        let mut y = rng.unit() * (h - 2) as f32;
        let (mut dx, mut dy) = (0.0f32, 0.0f32);
        let (mut speed, mut water, mut sediment) = (1.0f32, 1.0f32, 0.0f32);
        for _ in 0..s.steps {
            let (cx, cy) = (x as usize, y as usize);
            let (fx, fy) = (x - cx as f32, y - cy as f32);
            let (old, gx, gy) = height_and_gradient(&g, w, x, y);
            visits[cy * w + cx] += water;
            dx = dx * s.inertia - gx * (1.0 - s.inertia);
            dy = dy * s.inertia - gy * (1.0 - s.inertia);
            let len = (dx * dx + dy * dy).sqrt();
            if len < 1e-6 {
                break;
            }
            dx /= len;
            dy /= len;
            x += dx;
            y += dy;
            if !(x >= 0.0 && y >= 0.0 && x < (w - 2) as f32 && y < (h - 2) as f32) {
                break;
            }
            let dh = height_and_gradient(&g, w, x, y).0 - old;
            // By the slope it runs down, so that the spacing of the samples does not change how much a drop takes.
            let slope = (-dh / mpp).max(0.01);
            let capacity = slope * speed * water * s.capacity;
            let m = mask_at(cy * w + cx);
            if sediment > capacity || dh > 0.0 {
                // Laid at the old spot, over its four samples: uphill, at most what fills the step.
                let amount = if dh > 0.0 { dh.min(sediment) } else { (sediment - capacity) * s.deposit };
                sediment -= amount;
                let a = amount * m;
                for (i, wgt) in [(0, (1.0 - fx) * (1.0 - fy)), (1, fx * (1.0 - fy)), (w, (1.0 - fx) * fy), (w + 1, fx * fy)] {
                    let k = cy * w + cx + i;
                    g[k] += a * wgt;
                    laid[k] += a * wgt;
                }
            } else {
                // Taken round the old spot, never more than the step down (so it never digs a pit).
                let amount = ((capacity - sediment) * s.erode).min(-dh);
                let mut took = 0.0;
                for &(ox, oy, wgt) in &brush.cells {
                    let (bx, by) = (cx as i32 + ox, cy as i32 + oy);
                    if bx < 0 || by < 0 || bx >= w as i32 || by >= h as i32 {
                        continue;
                    }
                    let k = by as usize * w + bx as usize;
                    let t = amount * wgt * mask_at(k);
                    g[k] -= t;
                    laid[k] -= t;
                    took += t;
                }
                sediment += took;
            }
            speed = (speed * speed - dh / mpp * 2.0).clamp(0.0, MAX_SPEED * MAX_SPEED).sqrt();
            water *= 1.0 - s.evaporate;
            if water < 0.01 {
                break;
            }
        }
    }
    if cancel() {
        return None;
    }

    // Thermal: loose ground slides to its lower neighbours where it is steeper than the talus.
    let tan = s.talus.clamp(1.0, 89.0).to_radians().tan();
    let mut moved = vec![0.0f32; n];
    for _ in 0..s.thermal_passes {
        moved.fill(0.0);
        for y in 1..h - 1 {
            for x in 1..w - 1 {
                let i = y * w + x;
                let m = mask_at(i);
                if m <= 0.0 {
                    continue;
                }
                for (j, dist) in [(i + 1, mpp), (i - 1, mpp), (i + w, mpp), (i - w, mpp)] {
                    let over = g[i] - g[j] - tan * dist;
                    if over > 0.0 {
                        // A quarter of the excess a side, so the four sides never take more than there is.
                        let t = over * 0.125 * m * mask_at(j).max(0.25);
                        moved[i] -= t;
                        moved[j] += t;
                    }
                }
            }
        }
        for (a, b) in g.iter_mut().zip(&moved) {
            *a += b;
        }
    }

    // Detail: noise, more of it where the ground is steep.
    if s.detail > 0.0 && s.detail_scale > 0.0 {
        let (ox, oy) = ((s.seed % 9973) as f64 * 13.7, (s.seed / 9973 % 9973) as f64 * 7.3);
        for y in 1..h - 1 {
            for x in 1..w - 1 {
                let i = y * w + x;
                let m = mask_at(i);
                if m <= 0.0 {
                    continue;
                }
                let sx = (g[i + 1] - g[i - 1]) / (2.0 * mpp);
                let sy = (g[i + w] - g[i - w]) / (2.0 * mpp);
                let steep = (sx * sx + sy * sy).sqrt().min(1.0);
                let nx = f64::from(x as f32 * mpp / s.detail_scale) + ox;
                let ny = f64::from(y as f32 * mpp / s.detail_scale) + oy;
                let noise = crate::height_field::fractal_noise(nx, ny) as f32;
                g[i] += noise * s.detail * (0.33 + 0.67 * steep) * m;
            }
        }
    }

    let k = s.strength.clamp(0.0, 1.0);
    let delta: Vec<f32> = g.iter().zip(heights).map(|(a, b)| (a - b) * k).collect();
    let mean = visits.iter().sum::<f32>() / n as f32;
    let flow = visits.iter().map(|v| if mean > 0.0 { ((v / mean).ln_1p() / std::f32::consts::LN_2 / 4.0).min(1.0) } else { 0.0 }).collect();
    let deposit = laid.iter().map(|v| (v * k).max(0.0)).collect();
    Some(Eroded { delta, flow, deposit })
}

/// Where water would run on bare heights, without eroding them: each sample's water flows to its lowest neighbour (D8), summed down
/// the slopes; scaled as [`Eroded::flow`].
#[must_use]
pub fn flow_of(heights: &[f32], w: usize, h: usize) -> Vec<f32> {
    let n = w * h;
    if heights.len() != n || n == 0 {
        return vec![0.0; heights.len()];
    }
    let mut order: Vec<usize> = (0..n).collect();
    order.sort_by(|&a, &b| heights[b].total_cmp(&heights[a]));
    let mut acc = vec![1.0f32; n];
    for &i in &order {
        let (x, y) = (i % w, i / w);
        let mut best = (0.0f32, usize::MAX);
        for (ddx, ddy) in [(-1i32, -1i32), (0, -1), (1, -1), (-1, 0), (1, 0), (-1, 1), (0, 1), (1, 1)] {
            let (nx, ny) = (x as i32 + ddx, y as i32 + ddy);
            if nx < 0 || ny < 0 || nx >= w as i32 || ny >= h as i32 {
                continue;
            }
            let j = ny as usize * w + nx as usize;
            let drop = (heights[i] - heights[j]) / if ddx != 0 && ddy != 0 { std::f32::consts::SQRT_2 } else { 1.0 };
            if drop > best.0 {
                best = (drop, j);
            }
        }
        if best.1 != usize::MAX {
            let a = acc[i];
            acc[best.1] += a;
        }
    }
    // A drop's flow is the water that passed through, as the drops' visits; the average sample is upstream of a few.
    let mean = acc.iter().sum::<f32>() / n as f32;
    acc.iter().map(|v| ((v / mean).ln_1p() / std::f32::consts::LN_2 / 4.0).min(1.0)).collect()
}

/// The height (bilinear) and its gradient at a point in samples.
fn height_and_gradient(g: &[f32], w: usize, x: f32, y: f32) -> (f32, f32, f32) {
    let (cx, cy) = (x as usize, y as usize);
    let (fx, fy) = (x - cx as f32, y - cy as f32);
    let i = cy * w + cx;
    let (nw, ne, sw, se) = (g[i], g[i + 1], g[i + w], g[i + w + 1]);
    let gx = (ne - nw) * (1.0 - fy) + (se - sw) * fy;
    let gy = (sw - nw) * (1.0 - fx) + (se - ne) * fx;
    let height = nw * (1.0 - fx) * (1.0 - fy) + ne * fx * (1.0 - fy) + sw * (1.0 - fx) * fy + se * fx * fy;
    (height, gx, gy)
}

/// The samples round a point a drop wears, weighted to sum to 1.
struct Brush {
    cells: Vec<(i32, i32, f32)>,
}

impl Brush {
    fn new(radius: f32) -> Self {
        let r = radius.ceil() as i32;
        let mut cells = Vec::new();
        for oy in -r..=r {
            for ox in -r..=r {
                let d = ((ox * ox + oy * oy) as f32).sqrt();
                if d <= radius {
                    cells.push((ox, oy, 1.0 - d / (radius + 1.0)));
                }
            }
        }
        let sum: f32 = cells.iter().map(|c| c.2).sum();
        for c in &mut cells {
            c.2 /= sum;
        }
        Self { cells }
    }
}

/// A small random sequence (xorshift, seeded through splitmix), as the ground coverage's.
struct Random(u64);

impl Random {
    fn new(seed: u64) -> Self {
        let mut z = seed.wrapping_add(0x9e37_79b9_7f4a_7c15);
        z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
        Self((z ^ (z >> 31)) | 1)
    }

    fn unit(&mut self) -> f32 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        (self.0 >> 40) as f32 / (1u64 << 24) as f32
    }
}

/// The fastest a drop runs (its speed starts at 1 and grows with the slopes it runs down): on a long steep slope it would carry ever more.
const MAX_SPEED: f32 = 4.0;

/// The settings as the lines of an `erosion.wce` file (`key value`), with `on` first when the erosion is on.
#[must_use]
pub fn settings_text(on: bool, s: &Settings) -> String {
    let mut t = String::from("# Warscape Community Editor: the tile's erosion\n");
    let _ = writeln!(t, "on {}", u8::from(on));
    for (k, v) in [
        ("rain", s.rain),
        ("inertia", s.inertia),
        ("capacity", s.capacity),
        ("erode", s.erode),
        ("deposit", s.deposit),
        ("evaporate", s.evaporate),
        ("radius", s.radius),
        ("talus", s.talus),
        ("detail", s.detail),
        ("detail_scale", s.detail_scale),
        ("strength", s.strength),
    ] {
        let _ = writeln!(t, "{k} {v}");
    }
    let _ = writeln!(t, "steps {}", s.steps);
    let _ = writeln!(t, "thermal_passes {}", s.thermal_passes);
    let _ = writeln!(t, "seed {}", s.seed);
    t
}

/// An `erosion.wce` file's text read back: whether it's on, and its settings (unknown lines skipped, missing ones the defaults).
#[must_use]
pub fn settings_in(text: &str) -> (bool, Settings) {
    let mut s = Settings::default();
    let mut on = false;
    for line in text.lines().map(str::trim).filter(|l| !l.is_empty() && !l.starts_with('#')) {
        let Some((k, v)) = line.split_once(char::is_whitespace) else { continue };
        let v = v.trim();
        let f = v.parse::<f32>().ok().filter(|f| f.is_finite());
        match (k, f) {
            ("on", _) => on = v == "1",
            ("rain", Some(f)) => s.rain = f.clamp(0.0, 100.0),
            ("inertia", Some(f)) => s.inertia = f.clamp(0.0, 0.99),
            ("capacity", Some(f)) => s.capacity = f.clamp(0.0, 10.0),
            ("erode", Some(f)) => s.erode = f.clamp(0.0, 1.0),
            ("deposit", Some(f)) => s.deposit = f.clamp(0.0, 1.0),
            ("evaporate", Some(f)) => s.evaporate = f.clamp(0.0, 1.0),
            ("radius", Some(f)) => s.radius = f.clamp(0.5, 20.0),
            ("talus", Some(f)) => s.talus = f.clamp(1.0, 89.0),
            ("detail", Some(f)) => s.detail = f.clamp(0.0, 10.0),
            ("detail_scale", Some(f)) => s.detail_scale = f.clamp(0.5, 200.0),
            ("strength", Some(f)) => s.strength = f.clamp(0.0, 1.0),
            ("steps", Some(f)) => s.steps = (f as u32).min(1000),
            ("thermal_passes", Some(f)) => s.thermal_passes = (f as u32).min(100),
            ("seed", _) => s.seed = v.parse().unwrap_or(s.seed),
            _ => {}
        }
    }
    (on, s)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn never() -> bool {
        false
    }

    /// A cone 40 m across, 12 m high, at 1 m a sample.
    fn cone(w: usize) -> Vec<f32> {
        let c = (w as f32 - 1.0) / 2.0;
        (0..w * w)
            .map(|i| {
                let (x, y) = ((i % w) as f32 - c, (i / w) as f32 - c);
                (12.0 - (x * x + y * y).sqrt() * 0.6).max(0.0)
            })
            .collect()
    }

    fn no_detail() -> Settings {
        Settings { detail: 0.0, thermal_passes: 0, ..Settings::default() }
    }

    #[test]
    fn rain_wears_a_slope_and_lays_its_load_at_the_foot() {
        let w = 41;
        let g = cone(w);
        let e = erode(&g, w, w, 1.0, &[], &Settings { rain: 40.0, ..no_detail() }, &never).unwrap();
        let taken: f32 = e.delta.iter().filter(|d| **d < 0.0).sum();
        assert!(taken < -1.0, "the cone lost ground: {taken}");
        // Laid lower down than taken: the mean height of what was laid is under that of what was taken.
        let mean_at = |pick: &dyn Fn(f32) -> bool| {
            let (mut s, mut n) = (0.0, 0.0);
            for (d, h) in e.delta.iter().zip(&g) {
                if pick(*d) {
                    s += h * d.abs();
                    n += d.abs();
                }
            }
            s / n
        };
        assert!(mean_at(&|d| d > 0.0) < mean_at(&|d| d < 0.0));
        assert!(e.deposit.iter().any(|d| *d > 0.0));
        assert!(e.flow.iter().all(|f| (0.0..=1.0).contains(f)));
        // The water ran more on the cone's lower slopes than at its top.
        let at = |x: usize, y: usize| e.flow[y * w + x];
        assert!(at(20, 32) > at(20, 20));
    }

    #[test]
    fn the_same_seed_gives_the_same_ground_and_another_seed_other_ground() {
        let w = 33;
        let g = cone(w);
        let s = Settings { rain: 20.0, ..Settings::default() };
        let a = erode(&g, w, w, 1.0, &[], &s, &never).unwrap();
        assert_eq!(a, erode(&g, w, w, 1.0, &[], &s, &never).unwrap());
        assert_ne!(a.delta, erode(&g, w, w, 1.0, &[], &Settings { seed: 2, ..s }, &never).unwrap().delta);
    }

    #[test]
    fn masked_ground_stays_as_it_is() {
        let w = 33;
        let g = cone(w);
        // The left half masked off.
        let mask: Vec<f32> = (0..w * w).map(|i| if i % w < 16 { 0.0 } else { 1.0 }).collect();
        let e = erode(&g, w, w, 1.0, &mask, &Settings { rain: 30.0, ..Settings::default() }, &never).unwrap();
        for (i, d) in e.delta.iter().enumerate() {
            if i % w < 16 {
                assert!(d.abs() < 1e-6, "masked sample {i} changed by {d}");
            }
        }
        assert!(e.delta.iter().any(|d| d.abs() > 0.01));
    }

    #[test]
    fn loose_ground_slides_off_a_spike_down_to_its_talus() {
        let w = 21;
        let mut g = vec![0.0; w * w];
        g[10 * w + 10] = 10.0;
        let s = Settings { rain: 0.0, detail: 0.0, thermal_passes: 60, talus: 40.0, ..Settings::default() };
        let e = erode(&g, w, w, 1.0, &[], &s, &never).unwrap();
        let after: Vec<f32> = g.iter().zip(&e.delta).map(|(a, b)| a + b).collect();
        assert!(after[10 * w + 10] < 5.0, "the spike came down: {}", after[10 * w + 10]);
        assert!(after[10 * w + 11] > 0.1, "onto its neighbours");
        // Nothing made or lost.
        assert!((after.iter().sum::<f32>() - 10.0).abs() < 1e-3);
    }

    #[test]
    fn water_gathers_in_a_valley_on_bare_heights() {
        // A V valley along y: the middle column gets the flow.
        let w = 21;
        let g: Vec<f32> = (0..w * w).map(|i| ((i % w) as f32 - 10.0).abs() + (i / w) as f32 * 0.1).collect();
        let f = flow_of(&g, w, w);
        assert!(f[2 * w + 10] > f[2 * w + 3] + 0.3, "{} vs {}", f[2 * w + 10], f[2 * w + 3]);
    }

    #[test]
    fn a_steep_mountain_is_worn_not_dug_away() {
        // 300 m over 200 m, at 4 m a sample: slopes past 70°.
        let w = 101;
        let c = 50.0;
        let g: Vec<f32> = (0..w * w)
            .map(|i| {
                let (x, y) = ((i % w) as f32 - c, (i / w) as f32 - c);
                (300.0 - (x * x + y * y).sqrt() * 12.0).max(0.0)
                    + crate::height_field::fractal_noise(f64::from(x) / 6.0, f64::from(y) / 6.0) as f32 * 20.0
            })
            .collect();
        for (name, s) in PRESETS.iter() {
            let e = erode(&g, w, w, 4.0, &[], s, &never).unwrap();
            let most = e.delta.iter().fold(0.0f32, |a, d| a.max(d.abs()));
            assert!(most < 25.0, "{name}: {most} m");
            assert!(most > 0.2, "{name}: {most} m");
        }
    }

    #[test]
    fn a_cancelled_erosion_gives_nothing() {
        assert!(erode(&cone(21), 21, 21, 1.0, &[], &Settings::default(), &|| true).is_none());
    }

    #[test]
    fn settings_through_their_file() {
        let s = Settings { rain: 3.5, steps: 40, talus: 33.0, seed: 99, ..Settings::default() };
        assert_eq!(settings_in(&settings_text(true, &s)), (true, s.clone()));
        assert_eq!(settings_in(&settings_text(false, &s)).0, false);
        // Rubbish skipped, the rest kept.
        let (on, t) = settings_in("on 1\nrain banana\nwhat 3\ntalus 1000\n");
        assert!(on);
        assert_eq!((t.rain, t.talus), (Settings::default().rain, 89.0));
    }
}
