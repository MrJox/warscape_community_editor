//! Shores (docs/plans/rivers.md, step 10): reeds, water lilies and stones along a river's or a lake's water, laid along the water the
//! editor knows rather than found in the heights.
//!
//! - **Reed beds**: clumps where a smooth noise along the shore is high (dense inside, thinning at their edge, gaps between), each bed
//!   mostly one kind of reed. They stand in the shallows (up to [`ShoreSettings::shallows`] deep) and on the wet bank up to
//!   [`ShoreSettings::wet_bank`] metres from the water.
//! - **Water lilies**: a few patches on the water a little further out, up to [`LILY_DEPTH`] deep.
//! - **Stones**: small ones strung along the waterline (where the ground is within [`WATERLINE_HEIGHT`] of the water, near its edge), in
//!   runs where another noise is high, and now and then a bigger one on the bank.
//!
//! Each place of a half-metre grid draws its own numbers from the seed and its cell, and the noises are of the place too, so a shore keeps
//! what it has where nothing changed when its water is edited elsewhere. Everything is in the editor's metres.

/// What shores are made of (the app's `tables/<game>/shores.toml` and the mods' and the user's `shores_*.toml` over it, as the road
/// fences: `user_files`): each list's keys, a place picking one.
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ShoreKeys {
    pub reeds: Vec<String>,
    pub lilies: Vec<String>,
    pub stones: Vec<String>,
    pub ford_stones: Vec<String>,
    pub bank_stones: Vec<String>,
    pub bank_rocks: Vec<String>,
    pub bank_mounds: Vec<String>,
    pub shore_decals: Vec<String>,
    pub bed_decals: Vec<String>,
    pub splash: String,
}

impl Default for ShoreKeys {
    /// The built-in table.
    fn default() -> Self {
        KEYS.shipped().clone()
    }
}

/// The shipped file's name, in the app's tables folder.
pub const FILE_NAME: &str = "shores.toml";

/// The shipped files as built into the editor, by game.
pub const BUILT_IN: crate::user_files::BuiltIn = &[
    ("attila", include_str!("../../../tables/attila/shores.toml")),
    ("rome_2", include_str!("../../../tables/rome_2/shores.toml")),
    ("thrones_of_britannia", include_str!("../../../tables/thrones_of_britannia/shores.toml")),
];

/// The table, from the files ([`load`]) or as built in.
pub static KEYS: crate::user_files::Registry<ShoreKeys> = crate::user_files::Registry::new(FILE_NAME, BUILT_IN, parse_with);

/// A file over the vanilla one: each list it has in place of the vanilla one's.
#[derive(Debug, Default, serde::Deserialize)]
struct Custom {
    reeds: Option<Vec<String>>,
    lilies: Option<Vec<String>>,
    stones: Option<Vec<String>>,
    ford_stones: Option<Vec<String>>,
    bank_stones: Option<Vec<String>>,
    bank_rocks: Option<Vec<String>>,
    bank_mounds: Option<Vec<String>>,
    shore_decals: Option<Vec<String>>,
    bed_decals: Option<Vec<String>>,
    splash: Option<String>,
}

/// Reads the vanilla shores file and the files over it, in order.
///
/// # Errors
///
/// One is not TOML of its shape, or a list is empty, or a key is.
pub fn parse_with(vanilla: &str, extras: &[crate::user_files::Extra]) -> Result<ShoreKeys, String> {
    let mut k: ShoreKeys = toml::from_str(vanilla).map_err(|e| format!("{FILE_NAME}: {e}"))?;
    for c in crate::user_files::read_extras::<Custom>(extras)? {
        for (list, over) in [
            (&mut k.reeds, c.reeds),
            (&mut k.lilies, c.lilies),
            (&mut k.stones, c.stones),
            (&mut k.ford_stones, c.ford_stones),
            (&mut k.bank_stones, c.bank_stones),
            (&mut k.bank_rocks, c.bank_rocks),
            (&mut k.bank_mounds, c.bank_mounds),
            (&mut k.shore_decals, c.shore_decals),
            (&mut k.bed_decals, c.bed_decals),
        ] {
            if let Some(over) = over {
                *list = over;
            }
        }

        if let Some(splash) = c.splash {
            k.splash = splash;
        }
    }

    for (name, list) in [
        ("reeds", &k.reeds),
        ("lilies", &k.lilies),
        ("stones", &k.stones),
        ("ford_stones", &k.ford_stones),
        ("bank_stones", &k.bank_stones),
        ("shore_decals", &k.shore_decals),
        ("bed_decals", &k.bed_decals),
    ] {
        if list.is_empty() {
            return Err(format!("{FILE_NAME}: {name} needs one key at least"));
        }
    }

    // The bank's rocks and mounds may be none (a game without such pieces); no list has an empty key.
    for (name, list) in [
        ("reeds", &k.reeds),
        ("lilies", &k.lilies),
        ("stones", &k.stones),
        ("ford_stones", &k.ford_stones),
        ("bank_stones", &k.bank_stones),
        ("bank_rocks", &k.bank_rocks),
        ("bank_mounds", &k.bank_mounds),
        ("shore_decals", &k.shore_decals),
        ("bed_decals", &k.bed_decals),
    ] {
        if list.iter().any(|v| v.trim().is_empty()) {
            return Err(format!("{FILE_NAME}: {name} has an empty key"));
        }
    }

    if k.splash.trim().is_empty() {
        return Err(format!("{FILE_NAME}: splash is empty"));
    }

    Ok(k)
}

/// Reads a game's shores files in the tables folder, those of the mods loaded (`mods`) too: the table from then on.
pub fn load(tables: &std::path::Path, game: &str, mods: &[String]) {
    KEYS.load(tables, game, mods);
}

/// The table: the files' last read, else the built-in one.
#[must_use]
pub fn keys() -> &'static ShoreKeys {
    KEYS.get()
}

/// A list's key at an index (wrapped).
fn pick(list: &[String], i: usize) -> &str {
    &list[i % list.len()]
}

/// How deep lilies float at most (metres), and how far from the water's edge stones lie either side (metres).
pub const LILY_DEPTH: f32 = 1.5;
pub const WATERLINE: f32 = 1.5;

/// How far over or under the water the ground at the waterline is (metres): the stones lie there.
pub const WATERLINE_HEIGHT: f32 = 0.35;

/// How a shore is: how much of it its reed beds cover and its stones (0 .. 1, [`AMOUNTS`]), how deep its reeds stand and how far up the
/// bank they grow.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ShoreSettings {
    pub reeds: f32,
    pub stones: f32,

    /// How many decals lie along a river's shore (muddy rocks over the waterline) and on its bed (mud under the water by its edge)
    /// (0 .. 1, [`AMOUNTS`]).
    pub decals: f32,
    pub bed_decals: f32,
    pub shallows: f32,
    pub wet_bank: f32,
}

impl Default for ShoreSettings {
    fn default() -> Self {
        Self { reeds: 0.5, stones: 0.25, decals: 0.5, bed_decals: 0.5, shallows: 0.8, wet_bank: 2.5 }
    }
}

/// The amounts a shore's reeds and stones can be set to, by name: how much of the shore they take (0 .. 1).
pub const AMOUNTS: [(&str, f32); 4] = [("none", 0.0), ("few", 0.25), ("some", 0.5), ("thick", 0.85)];

/// An amount by its name; none for an unknown one.
pub fn amount(name: &str) -> Option<f32> {
    AMOUNTS.iter().find(|a| a.0 == name.trim()).map(|a| a.1)
}

impl ShoreSettings {
    /// The settings of amounts of reeds and stones ([`AMOUNTS`]).
    pub fn of(reeds: f32, stones: f32) -> Self {
        Self { reeds, stones, decals: 0.0, bed_decals: 0.0, ..Self::default() }
    }

    /// With decals along the shore and on the bed ([`AMOUNTS`]).
    pub fn with_decals(self, decals: f32, bed_decals: f32) -> Self {
        Self { decals, bed_decals, ..self }
    }
}

/// A water a shore runs along.
#[derive(Clone, Debug, PartialEq)]
pub enum Water {
    /// A river: its middle line's samples (x, z), the water's height there and its half width; its fords' held stretches (where each
    /// starts and ends along it, metres).
    River { line: Vec<[f32; 2]>, water: Vec<f32>, half: Vec<f32>, fords: Vec<crate::river_bed::Ford> },

    /// A lake: its outline (x, z) and its water's height.
    Lake { outline: Vec<[f32; 2]>, water: f32 },
}

impl Water {
    /// The water's edge from a place: how far outside it (negative: inside), the water's height at the nearest place, how far along a
    /// river that place is and where across it (-1 at one edge of the water, 1 at the other; NaN for a lake).
    fn edge(&self, x: f32, z: f32) -> (f32, f32, f32, f32) {
        match self {
            Water::River { line, water, half, .. } => {
                let mut best = (f32::INFINITY, 0.0, f32::NAN, f32::NAN);
                let mut along = 0.0;
                for i in 0..line.len().saturating_sub(1) {
                    let (a, b) = (line[i], line[i + 1]);
                    let (dx, dz) = (b[0] - a[0], b[1] - a[1]);
                    let l2 = dx * dx + dz * dz;
                    let t = if l2 > 1e-9 { (((x - a[0]) * dx + (z - a[1]) * dz) / l2).clamp(0.0, 1.0) } else { 0.0 };
                    let h = half[i] + (half[i + 1] - half[i]) * t;
                    let r = (x - a[0] - dx * t).hypot(z - a[1] - dz * t);
                    let d = r - h;
                    if d < best.0 {
                        let side = if dx * (z - a[1]) - dz * (x - a[0]) < 0.0 { -1.0 } else { 1.0 };
                        best = (d, water[i] + (water[i + 1] - water[i]) * t, along + l2.sqrt() * t, side * r / h.max(1e-6));
                    }

                    along += l2.sqrt();
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

                (if inside { -near } else { near }, *water, f32::NAN, f32::NAN)
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

    /// The splashes over a ford's edges (the table's `splash`, at the water's height).
    pub splashes: Vec<Placed>,

    /// Along a river's banks (not at its fords): decals over the seam of water and land (their `scale`: their size, metres), and now and
    /// then a long flat rock or a low ground mound laid along the bank (buildings).
    pub decals: Vec<Placed>,
    pub bank_pieces: Vec<Placed>,
}

/// How far apart along a bank its dressing is looked for (metres).
const BANK_STEP: f32 = 4.0;

/// How far apart a ford's splashes are across the river, and how far its edge's stones reach either side of it (metres).
const SPLASH_STEP: f32 = 8.0;

/// How far in from the water's edge, and how far past a ford's ends along the river at least (half its length when more), its banks are
/// kept clear of stones (metres): units step in and out of it there, its shallow ramp too.
const FORD_BANK_MARGIN: f32 = 6.0;

/// How far across the river (of its half width) a ford's end lines reach: short of its sides, where units step on and off it.
const FORD_LINE_REACH: f32 = 0.75;
const FORD_EDGE: f32 = 1.0;

/// Steps of a range by a stride (`step_by` for floats).
trait StepByF32 {
    fn step_by_f32(self, step: f32) -> Vec<f32>;
}

impl StepByF32 for std::ops::Range<f32> {
    fn step_by_f32(self, step: f32) -> Vec<f32> {
        let mut out = vec![];
        let mut v = self.start;
        while v < self.end {
            out.push(v);
            v += step;
        }

        out
    }
}

/// A river's middle line at a distance along it: the place and the direction there.
fn at_along(line: &[[f32; 2]], half: &[f32], along: f32) -> Option<([f32; 2], [f32; 2], f32)> {
    let mut walked = 0.0;
    for i in 0..line.len().saturating_sub(1) {
        let (a, b) = (line[i], line[i + 1]);
        let l = (b[0] - a[0]).hypot(b[1] - a[1]);
        if l > 1e-6 && walked + l >= along {
            let t = ((along - walked) / l).clamp(0.0, 1.0);
            let dir = [(b[0] - a[0]) / l, (b[1] - a[1]) / l];
            return Some(([a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t], dir, half[i] + (half[i + 1] - half[i]) * t));
        }

        walked += l;
    }

    None
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

/// A smooth noise in [0, 1) over the ground, its features about `size` metres across (value noise: a lattice of the seed's numbers,
/// smoothly interpolated).
fn noise(seed: u64, x: f32, z: f32, size: f32) -> f32 {
    let (u, v) = (x / size, z / size);
    let (i, j) = (u.floor() as i64, v.floor() as i64);
    let (fu, fv) = (u - u.floor(), v - v.floor());
    let s = |t: f32| t * t * (3.0 - 2.0 * t);
    let at = |di: i64, dj: i64| cell_numbers(seed ^ 0x5bd1_e995, i + di, j + dj)[0];
    let (a, b) = (at(0, 0) + (at(1, 0) - at(0, 0)) * s(fu), at(0, 1) + (at(1, 1) - at(0, 1)) * s(fu));
    a + (b - a) * s(fv)
}

/// The grid's cell, metres: one draw a cell.
const CELL: f32 = 0.5;

/// Reeds a square metre inside a bed, at its heart.
const BED_DENSITY: f32 = 2.4;

/// How big the beds and the stones' runs are (metres across), and how much of a bed's edge thins out (of the noise's range).
const BED_SIZE: f32 = 9.0;
const STONE_RUN: f32 = 6.0;
const BED_EDGE: f32 = 0.12;

/// A clump of bed decals round a place: three to six of them, each off its middle by up to `radius` metres, its own size and turn,
/// most of one kind (the clump's) and now and then another, so that they overlap into a patch of no set shape rather than squares
/// scattered.
#[allow(clippy::too_many_arguments)]
fn clump(
    out: &mut Vec<Placed>,
    cx: f32,
    cz: f32,
    radius: f32,
    id: (i64, i64),
    keys: &ShoreKeys,
    height: &dyn Fn(f32, f32) -> f32,
    blocked: &dyn Fn(f32, f32) -> bool,
    seed: u64,
) {
    let [q, kind, ..] = cell_numbers(seed ^ 0x5851_f42d, id.0, id.1);
    let n = 3 + (q * 4.0) as i64;
    let own = (kind * keys.bed_decals.len() as f32) as usize;
    for j in 0..n {
        let [r1, r2, r3, r4] = cell_numbers(seed ^ 0x2545_f491 ^ (j as u64).wrapping_mul(0x9e37), id.0, id.1);
        let (angle, dist) = (r1 * std::f32::consts::TAU, radius * r2.sqrt());
        let (x, z) = (cx + angle.cos() * dist, cz + angle.sin() * dist);
        if blocked(x, z) {
            continue;
        }

        let key = if r4 < 0.75 { pick(&keys.bed_decals, own) } else { pick(&keys.bed_decals, (r4 * 97.0) as usize) };
        out.push(Placed { key: key.to_owned(), x, y: height(x, z), z, scale: 2.5 + 3.0 * r3, yaw: 360.0 * r4 });
    }
}

/// The shore of a water over a half-metre grid round it, each place by the ground there and its distance from the water's edge (see the
/// module's notes), off the places `blocked` says. `height`: the ground's.
pub fn place(
    water: &Water,
    s: &ShoreSettings,
    keys: &ShoreKeys,
    height: &dyn Fn(f32, f32) -> f32,
    blocked: &dyn Fn(f32, f32) -> bool,
    seed: u64,
) -> Shore {
    let mut out = Shore::default();
    let reach = s.wet_bank.max(WATERLINE).max(4.0);
    let Some([x0, z0, x1, z1]) = water.bounds(reach + CELL) else { return out };
    let (i0, i1) = ((x0 / CELL).floor() as i64, (x1 / CELL).ceil() as i64);
    let (j0, j1) = ((z0 / CELL).floor() as i64, (z1 / CELL).ceil() as i64);
    let area = CELL * CELL;
    for j in j0..j1 {
        for i in i0..i1 {
            let [a, b, c, d] = cell_numbers(seed, i, j);
            let (x, z) = ((i as f32 + b) * CELL, (j as f32 + c) * CELL);
            let (edge, level, along, u) = water.edge(x, z);
            // A ford: across its river, the stretch held shallow, and its two curved ends (where it slopes down to the deep water).
            let fords: &[crate::river_bed::Ford] = if let Water::River { fords, .. } = water { fords } else { &[] };
            let in_water = edge <= 0.5;
            let ends = |f: &crate::river_bed::Ford| {
                let (a, b) = f.ends(f64::from(u));
                (a as f32, b as f32)
            };
            let on_ford = in_water
                && fords.iter().any(|f| {
                    let (s0, s1) = ends(f);
                    along >= s0 - FORD_EDGE && along <= s1 + FORD_EDGE
                });
            let ford_edge = in_water
                && fords.iter().any(|f| {
                    let (s0, s1) = ends(f);
                    (along - s0).abs() <= FORD_EDGE || (along - s1).abs() <= FORD_EDGE
                });

            // Inside a river or a lake further than a lily floats from its edge (but on a ford), or out past the bank: nothing.
            if edge > reach || (edge < -12.0 && !on_ford) {
                continue;
            }

            let g = height(x, z);
            let depth = level - g;

            // A ford, bank to bank, and its banks past its ends: no stones but its ends' lines (units walk in and out of it from the
            // banks, and its floor is all shallow enough for the waterline's stones).
            let ford_bank = fords.iter().any(|f| {
                let (s0, s1) = ends(f);
                // Past its ends too, where its ramp is still shallow enough to wade: half its length (6 m at least).
                let margin = FORD_BANK_MARGIN.max(f.length as f32 * 0.5);
                along >= s0 - margin && along <= s1 + margin
            });

            // A ford's ends lined with small stones (gaps here and there), so that it shows where it is, whatever the shore's stones; not
            // near its sides, where units step on and off it.
            if ford_edge && u.abs() <= FORD_LINE_REACH {
                let gaps = noise(seed ^ 0x0f0e_d0c1, x, z, 3.0);
                if gaps > 0.3 && a < 2.0 * area && !blocked(x, z) {
                    let key = pick(&keys.ford_stones, (d * 61.0) as usize);
                    out.stones.push(Placed { key: key.to_owned(), x, y: g - 0.05, z, scale: 1.0, yaw: 360.0 * c });
                }

                continue;
            }

            // A reed bed: where the bed noise is over the threshold the amount sets, thinning over its edge.
            if s.reeds > 0.0 && depth <= s.shallows && edge <= s.wet_bank && g - level <= 1.0 {
                let n = noise(seed, x, z, BED_SIZE);
                // On a ford, a few clumps only: its crossing stays open.
                let threshold = if on_ford && edge < -2.0 { 1.0 - s.reeds * 0.3 } else { 1.0 - s.reeds };
                let heart = ((n - threshold) / BED_EDGE).clamp(0.0, 1.0);
                // Thinner up the bank, away from the water.
                let wet = if edge <= 0.0 { 1.0 } else { 1.0 - edge / s.wet_bank };
                if a < BED_DENSITY * area * heart * wet.max(0.0) {
                    if !blocked(x, z) {
                        // The bed's kind (by another noise, coarser), now and then another's.
                        let kind = noise(seed ^ 0x2545_f491, x, z, BED_SIZE * 1.6);
                        let k = if d < 0.85 { (kind * keys.reeds.len() as f32) as usize } else { (d * 97.0) as usize };
                        let key = pick(&keys.reeds, k);
                        out.reeds.push(Placed { key: key.to_owned(), x, y: g, z, scale: 0.85 + 0.4 * d, yaw: 360.0 * d });
                    }

                    continue;
                }
            }

            // Lilies: a few patches out on the water.
            if s.reeds > 0.0 && depth > 0.3 && depth <= LILY_DEPTH && edge < -1.0 {
                let n = noise(seed ^ 0x7f4a_7c15, x, z, BED_SIZE * 0.7);
                if n > 1.0 - s.reeds * 0.35 && a < 0.5 * area && !blocked(x, z) {
                    let key = pick(&keys.lilies, (d * keys.lilies.len() as f32) as usize);
                    out.reeds.push(Placed { key: key.to_owned(), x, y: level, z, scale: 0.8 + 0.4 * c, yaw: 360.0 * d });
                    continue;
                }
            }

            // Stones: along the waterline in runs, a bigger one now and then on the bank; none where units leave a ford.
            if s.stones > 0.0 && !ford_bank {
                let run = noise(seed ^ 0x1234_5678, x, z, STONE_RUN);
                // Where the ground meets the water (as the ground has it, not the ribbon's edge).
                let waterline = depth.abs() <= WATERLINE_HEIGHT && (-6.0..=WATERLINE).contains(&edge);
                // The noise keeps near its middle: a run takes from about a third of the waterline (few) to all of it (thick).
                let density = if waterline && run > 0.85 - s.stones * 0.7 { 2.0 } else { 0.0 };
                if a > 1.0 - density * area && !blocked(x, z) {
                    let key = pick(&keys.stones, (d * keys.stones.len() as f32) as usize);
                    // Sunk a little: stones of a waterline lie in the mud.
                    out.stones.push(Placed { key: key.to_owned(), x, y: g - 0.05, z, scale: 1.0, yaw: 360.0 * d });
                } else if edge > WATERLINE && edge <= reach && g - level <= 1.5 && a > 1.0 - 0.02 * s.stones * area && !blocked(x, z) {
                    let key = pick(&keys.bank_stones, (d * keys.bank_stones.len() as f32) as usize);
                    out.stones.push(Placed { key: key.to_owned(), x, y: g - 0.1, z, scale: 1.0, yaw: 360.0 * d });
                }
            }
        }
    }

    // A river's banks: decals over the seam, now and then a flat rock or a mound along them; not at a ford nor its clear banks.
    if let Water::River { line, half, fords, .. } = water {
        let length = line.windows(2).map(|w| (w[1][0] - w[0][0]).hypot(w[1][1] - w[0][1])).sum::<f32>();
        let mut along = BANK_STEP * 0.5;
        let mut k = 0i64;
        while along < length {
            if let Some((p, dir, h)) = at_along(line, half, along) {
                for side in [-1.0f32, 1.0] {
                    let near_ford = fords.iter().any(|f| {
                        let (s0, s1) = f.ends(f64::from(side));
                        let margin = f64::from(FORD_BANK_MARGIN).max(f.length * 0.5);
                        f64::from(along) >= s0 - margin && f64::from(along) <= s1 + margin
                    });
                    let [a, b, c, d] = cell_numbers(seed ^ 0x6c07_8965, k, if side < 0.0 { 1 } else { 2 });
                    let yaw = -dir[1].atan2(dir[0]).to_degrees();
                    // Out from the middle line: past the water's edge a little (on the bank's top).
                    let at = |off: f32| {
                        let r = (h + off) * side;
                        (p[0] - dir[1] * r, p[1] + dir[0] * r)
                    };

                    // The waterline (the seam of water and land, as the ground has it: the bank's profile meets the water short of the
                    // ribbon's edge).
                    // None where the ground is not under the water first (out from the middle) and over it then: no waterline there.
                    let waterline = || {
                        let (_, level, ..) = water.edge(p[0], p[1]);
                        let mut wet = false;
                        (0..=24).map(|i| -10.0 + 0.5 * i as f32).find(|&o| {
                            let (x, z) = at(o);
                            let dry = height(x, z) >= level;
                            let found = dry && wet;
                            wet |= !dry;
                            found
                        })
                    };

                    // The shore: muddy rocks over the waterline, a little into the water and onto the bank.
                    // Not by a ford: its banks are where units step on and off it.
                    if let (true, Some(wl)) = (s.decals > 0.0 && !near_ford && a < (s.decals * 2.0).min(1.0), waterline()) {
                        // On the land side of the waterline, over its seam: near continuous along it (the reeds and stones stand on
                        // it too; they hid sparse ones).
                        let size = 5.0 + 3.0 * d;
                        // From the waterline up the bank: the bed's meet it there.
                        let (x, z) = at(wl + size * 0.5 + 0.3 * b);
                        if !blocked(x, z) {
                            let key = pick(&keys.shore_decals, (c * keys.shore_decals.len() as f32) as usize);
                            out.decals.push(Placed {
                                key: key.to_owned(),
                                x,
                                y: height(x, z),
                                z,
                                scale: size,
                                yaw: yaw + 30.0 * (b - 0.5),
                            });
                        }
                    }

                    // The bed: clumps of mud and dirt under the water by its edge (seen through the shallows), a clump now and then.
                    let [e1, e2, ..] = cell_numbers(seed ^ 0x1b87_3593, k, if side < 0.0 { 3 } else { 4 });
                    if let (true, Some(wl)) = (s.bed_decals > 0.0 && !near_ford && e1 < s.bed_decals * 0.5, waterline()) {
                        let (x, z) = at(wl - 2.0 - 1.5 * e2);
                        clump(&mut out.decals, x, z, 2.5, (k, if side < 0.0 { 3 } else { 4 }), keys, height, blocked, seed);
                    }

                    if s.stones > 0.0 && !near_ford {
                        let n = noise(seed ^ 0x2c1b_3c6d, along, side * 1000.0, 40.0);
                        let (x, z) = at(2.0 + 2.0 * c);
                        if n > 0.62 && b < 0.035 * s.stones && !keys.bank_rocks.is_empty() && !blocked(x, z) {
                            out.bank_pieces.push(Placed {
                                key: pick(&keys.bank_rocks, (c * 31.0) as usize).to_owned(),
                                x,
                                y: height(x, z) - 0.6,
                                z,
                                scale: 1.0,
                                yaw,
                            });
                        } else if n < 0.3 && b > 1.0 - 0.02 * s.stones && !keys.bank_mounds.is_empty() && !blocked(x, z) {
                            out.bank_pieces.push(Placed {
                                key: pick(&keys.bank_mounds, (c * 31.0) as usize).to_owned(),
                                x,
                                y: height(x, z) - 1.2,
                                z,
                                scale: 1.0,
                                yaw,
                            });
                        }
                    }
                }
            }

            // A ford's floor: clumps of mud and dirt over it (its water is shallow: the bed shows there most).
            if s.bed_decals > 0.0
                && let Some((p, dir, h)) = at_along(line, half, along)
            {
                let across = (-h + 3.0..h - 3.0).step_by_f32(6.0);
                for (n, off) in across.into_iter().enumerate() {
                    let [a, b, ..] = cell_numbers(seed ^ 0x3c6e_f372, k, 10 + n as i64);
                    let off = off + 3.0 * (b - 0.5);
                    let (x, z) = (p[0] - dir[1] * off, p[1] + dir[0] * off);
                    let (_, _, al, u) = water.edge(x, z);
                    let on_floor = fords.iter().any(|f| {
                        let (s0, s1) = f.ends(f64::from(u));
                        f64::from(al) >= s0 && f64::from(al) <= s1
                    });
                    if on_floor && a < s.bed_decals * 0.9 {
                        clump(&mut out.decals, x, z, 3.5, (k, 10 + n as i64), keys, height, blocked, seed);
                    }
                }
            }

            along += BANK_STEP;
            k += 1;
        }
    }

    // The splashes: across each ford's two curved ends, every few metres, on the water.
    if let Water::River { line, half, fords, .. } = water {
        for f in fords {
            for end in [0, 1] {
                // The middle's place there, for the river's width.
                let mid = if end == 0 { f.ends(0.0).0 } else { f.ends(0.0).1 } as f32;
                let Some((_, _, h)) = at_along(line, half, mid) else { continue };
                let n = ((2.0 * (h - 2.0)) / SPLASH_STEP).floor().max(0.0) as i32;
                for k in 0..=n {
                    let off = -(n as f32) * SPLASH_STEP * 0.5 + k as f32 * SPLASH_STEP;
                    let ends = f.ends(f64::from(off / h.max(1e-6)));
                    let along = if end == 0 { ends.0 } else { ends.1 } as f32;
                    let Some((p, dir, _)) = at_along(line, half, along) else { continue };
                    // Across to the side `off` is on (the left of the river's direction positive, as `Water::edge` has it).
                    let (x, z) = (p[0] - dir[1] * off, p[1] + dir[0] * off);
                    let (_, level, ..) = water.edge(x, z);
                    if !blocked(x, z) {
                        let yaw = dir[1].atan2(dir[0]).to_degrees();
                        out.splashes.push(Placed { key: keys.splash.clone(), x, y: level, z, scale: 1.0, yaw });
                    }
                }
            }
        }
    }

    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A straight river along x at z 50, 10 m wide, its water at 0; the ground 2.4 m under it in the middle, rising 0.4 a metre to the
    /// sides (the water's edge at 6 m from the middle).
    fn river() -> (Water, impl Fn(f32, f32) -> f32) {
        let w = Water::River { line: vec![[0.0, 50.0], [200.0, 50.0]], water: vec![0.0, 0.0], half: vec![5.0, 5.0], fords: vec![] };
        (w, |_: f32, z: f32| ((z - 50.0).abs() - 6.0) * 0.4)
    }

    #[test]
    fn reeds_stand_in_the_shallows_and_the_wet_bank_and_stones_on_the_waterline() {
        let (w, g) = river();
        let shore = place(&w, &ShoreSettings::of(0.85, 0.85), &ShoreKeys::default(), &g, &|_, _| false, 7);
        let reeds = shore.reeds.iter().filter(|r| ShoreKeys::default().reeds.contains(&r.key)).collect::<Vec<_>>();
        assert!(reeds.len() > 50 && !shore.stones.is_empty(), "{} reeds, {} stones", reeds.len(), shore.stones.len());
        for r in &reeds {
            assert!(-g(r.x, r.z) <= 0.8 + 1e-4, "no reed deeper than the shallows: {} at {}", -g(r.x, r.z), r.z);
            assert!((r.z - 50.0).abs() <= 5.0 + 2.5 + 0.01, "none far up the bank: {}", r.z);
            assert!((r.y - g(r.x, r.z)).abs() < 1e-4, "on the ground");
        }

        // The small stones where the ground meets the water (6 m from the middle).
        for st in shore.stones.iter().filter(|s| ShoreKeys::default().stones.contains(&s.key)) {
            assert!(g(st.x, st.z).abs() <= WATERLINE_HEIGHT + 1e-4, "on the waterline: {} at {}", g(st.x, st.z), st.z);
        }
    }

    #[test]
    fn a_fords_edges_are_lined_with_stones_and_splashes() {
        let ford = crate::river_bed::Ford { at: 100.0, length: 20.0, depth: 0.5 };
        let w = Water::River { line: vec![[0.0, 50.0], [200.0, 50.0]], water: vec![0.0, 0.0], half: vec![10.0, 10.0], fords: vec![ford] };
        // Deep but at the ford (its held stretch, curved), a quarter metre there.
        let g = move |x: f32, z: f32| {
            let (s0, s1) = ford.ends(f64::from((z - 50.0) / 10.0));
            if f64::from(x) >= s0 && f64::from(x) <= s1 { -0.25 } else { -3.0 }
        };
        let shore = place(&w, &ShoreSettings::of(0.0, 0.0), &ShoreKeys::default(), &g, &|_, _| false, 5);
        assert!(shore.stones.len() > 20, "the edges lined whatever the shore's stones: {}", shore.stones.len());
        assert!(shore.stones.iter().all(|s| ShoreKeys::default().ford_stones.contains(&s.key)), "small ones");
        let on_end = |s: &Placed| {
            let (s0, s1) = ford.ends(f64::from((s.z - 50.0) / 10.0));
            (f64::from(s.x) - s0).abs() <= 1.01 || (f64::from(s.x) - s1).abs() <= 1.01
        };
        assert!(shore.stones.iter().all(on_end), "on its curved ends only");
        assert!(
            shore.stones.iter().all(|s| (s.z - 50.0).abs() <= 10.0 * FORD_LINE_REACH + 0.5),
            "none by its sides, where units step on and off"
        );
        let stony = place(&w, &ShoreSettings::of(0.0, 0.85), &ShoreKeys::default(), &g, &|_, _| false, 5);
        assert!(
            stony
                .stones
                .iter()
                .all(|s| ShoreKeys::default().ford_stones.contains(&s.key) || f64::from(s.x) < 80.5 || f64::from(s.x) > 119.5),
            "the shore's stones keep off the ford, bank to bank"
        );
        let (mid, side) = (ford.ends(0.0), ford.ends(1.0));
        assert!((mid.0 - side.0).abs() > 0.2 && (mid.1 - side.1).abs() > 0.2, "slightly curved, not straight across: {mid:?} {side:?}");
        assert!(shore.stones.iter().any(|s| (s.z - 50.0).abs() < 3.0), "across the river, its middle too");
        assert_eq!(
            shore.splashes.len(),
            2 * 3,
            "three across each edge: {:?}",
            shore.splashes.iter().map(|s| (s.x, s.z)).collect::<Vec<_>>()
        );
        assert!(shore.splashes.iter().all(|s| s.key == ShoreKeys::default().splash && s.y == 0.0));

        // Reeds may stand on it (its middle, shallow), never on the deep water beside it.
        let reedy = place(&w, &ShoreSettings::of(0.85, 0.0), &ShoreKeys::default(), &g, &|_, _| false, 5);
        assert!(reedy.reeds.iter().any(|r| (r.z - 50.0).abs() < 5.0), "on the ford");
        assert!(reedy.reeds.iter().all(|r| g(r.x, r.z) > -1.0), "not in the deep");
    }

    #[test]
    fn a_rivers_banks_take_decals_its_fords_bed_decals_and_now_and_then_a_rock_not_at_them() {
        let ford = crate::river_bed::Ford { at: 300.0, length: 30.0, depth: 0.5 };
        let w = Water::River { line: vec![[0.0, 50.0], [600.0, 50.0]], water: vec![0.0, 0.0], half: vec![10.0, 10.0], fords: vec![ford] };
        let g = |_: f32, z: f32| ((z - 50.0).abs() - 11.0) * 0.5;
        let shore = place(&w, &ShoreSettings::of(0.0, 0.85).with_decals(0.85, 0.85), &ShoreKeys::default(), &g, &|_, _| false, 9);
        let (shore_d, bed_d): (Vec<_>, Vec<_>) = shore.decals.iter().partition(|d| ShoreKeys::default().shore_decals.contains(&d.key));
        assert!(shore_d.len() > 100 && bed_d.len() > 100, "{} shore decals, {} bed decals", shore_d.len(), bed_d.len());
        // The ground meets the water 11 m from the middle (past the ribbon's 10): the shore's decals there, the bed's inside it.
        let off = |d: &&Placed| (d.z - 50.0).abs() - 11.0;
        assert!(shore_d.iter().all(|d| off(d) >= d.scale * 0.5 - 0.6 && off(d) <= d.scale * 0.5 + 0.9), "from the waterline up the bank");
        // The bed's in clumps by the waterline (inside it, a clump's radius either way), or over a ford's floor (its held stretch: 285 to
        // 315, a metre or two either way by its curve, and a clump's radius).
        let on_ford_floor = |d: &&Placed| d.x > 276.0 && d.x < 324.0 && (d.z - 50.0).abs() < 11.0;
        assert!(
            bed_d
                .iter()
                .all(|d| ShoreKeys::default().bed_decals.contains(&d.key) && (on_ford_floor(d) || (off(d) <= 0.6 && off(d) >= -6.6))),
            "on the bed"
        );
        assert!(bed_d.iter().filter(|d| on_ford_floor(d)).count() >= 10, "over the ford's floor");
        // In clumps: most have another within 3 m.
        let near = bed_d.iter().filter(|d| bed_d.iter().any(|e| !std::ptr::eq(**d, *e) && (d.x - e.x).hypot(d.z - e.z) < 3.0)).count();
        assert!(near * 10 >= bed_d.len() * 8, "clumped: {near} of {}", bed_d.len());
        assert!(shore_d.iter().all(|d| !(d.x > 270.0 && d.x < 330.0)), "no rocks' decals by the ford");
        assert!(!shore.bank_pieces.is_empty() && shore.bank_pieces.len() < 40, "now and then: {}", shore.bank_pieces.len());
        let none = ShoreKeys { bank_rocks: vec![], bank_mounds: vec![], ..ShoreKeys::default() };
        assert!(
            place(&w, &ShoreSettings::of(0.0, 0.85).with_decals(0.85, 0.85), &none, &g, &|_, _| false, 9).bank_pieces.is_empty(),
            "a game without them"
        );
        let at_ford = |x: f32| x > 270.0 && x < 330.0;
        assert!(shore.bank_pieces.iter().all(|p| !at_ford(p.x)), "no rocks or mounds at the ford's banks");
        assert!(place(&w, &ShoreSettings::of(0.0, 0.0), &ShoreKeys::default(), &g, &|_, _| false, 9).decals.is_empty(), "none asked");
    }

    #[test]
    fn reeds_grow_in_beds_with_gaps_between() {
        let (w, g) = river();
        let shore = place(&w, &ShoreSettings::of(0.5, 0.0), &ShoreKeys::default(), &g, &|_, _| false, 11);
        // Along the river in 4 m stretches: some full, some empty.
        let mut bins = [0usize; 50];
        for r in shore.reeds.iter().filter(|r| r.z > 50.0) {
            bins[((r.x / 4.0) as usize).min(49)] += 1;
        }

        let empty = bins.iter().filter(|&&n| n == 0).count();
        let full = bins.iter().filter(|&&n| n >= 8).count();
        assert!(empty >= 8 && full >= 8, "beds and gaps: {bins:?}");
    }

    #[test]
    fn a_shore_keeps_its_places_and_its_blocked_ground() {
        let (w, g) = river();
        let s = ShoreSettings::default();
        let a = place(&w, &s, &ShoreKeys::default(), &g, &|_, _| false, 3);
        assert_eq!(a, place(&w, &s, &ShoreKeys::default(), &g, &|_, _| false, 3), "the same each time");
        assert_ne!(a, place(&w, &s, &ShoreKeys::default(), &g, &|_, _| false, 4), "another seed, another shore");
        let b = place(&w, &s, &ShoreKeys::default(), &g, &|x, _| x < 100.0, 3);
        assert!(b.reeds.iter().chain(&b.stones).all(|p| p.x >= 100.0));
        assert!(b.reeds.iter().all(|p| a.reeds.contains(p)), "what is left is where it was");
        assert!(place(&w, &ShoreSettings::of(0.0, 0.0), &ShoreKeys::default(), &g, &|_, _| false, 3) == Shore::default(), "none");
    }

    #[test]
    fn a_lakes_shore_runs_round_its_outline() {
        let w = Water::Lake { outline: vec![[0.0, 0.0], [60.0, 0.0], [60.0, 60.0], [0.0, 60.0]], water: 0.0 };
        // A bowl: 0.3 under the water a metre in from the outline, deeper further in, rising outside.
        let g = |x: f32, z: f32| {
            let e = x.min(60.0 - x).min(z).min(60.0 - z);
            if e > 0.0 { -0.3 * e } else { -0.2 * e }
        };
        let shore = place(&w, &ShoreSettings::of(0.85, 0.5), &ShoreKeys::default(), &g, &|_, _| false, 1);
        let reeds = shore.reeds.iter().filter(|r| ShoreKeys::default().reeds.contains(&r.key)).collect::<Vec<_>>();
        assert!(!reeds.is_empty());
        assert!(reeds.iter().all(|r| g(r.x, r.z) >= -0.8 - 1e-4), "in the shallows or on the bank");
        assert!(reeds.iter().all(|r| !(r.x > 4.0 && r.x < 56.0 && r.z > 4.0 && r.z < 56.0)), "none in the deep middle");
    }

    #[test]
    fn the_shores_tables_read_and_a_file_over_them_replaces_its_lists() {
        for (game, text) in BUILT_IN {
            let k = parse_with(text, &[]).unwrap_or_else(|e| panic!("{game}: {e}"));
            assert!(!k.reeds.is_empty() && !k.bed_decals.is_empty(), "{game}");
        }

        let over = |t: &str| [("shores_mine.toml".to_owned(), t.to_owned())];
        let k = parse_with(BUILT_IN[0].1, &over("mod_key = \"\"\nreeds = [\"my_reed\"]\nsplash = \"my_splash\"\n")).unwrap();
        let shipped = ShoreKeys::default();
        assert_eq!((k.reeds.as_slice(), k.splash.as_str()), (["my_reed".to_owned()].as_slice(), "my_splash"));
        assert_eq!(k.stones, shipped.stones, "the lists it does not have stay");
        assert!(parse_with(BUILT_IN[0].1, &over("mod_key = \"\"\nstones = []\n")).is_err(), "an empty list");
        assert!(parse_with(BUILT_IN[0].1, &over("mod_key = \"\"\nreeds = [\" \"]\n")).is_err(), "an empty key");
        assert!(
            parse_with(BUILT_IN[0].1, &over("mod_key = \"\"\nbank_mounds = []\n")).is_ok_and(|k| k.bank_mounds.is_empty()),
            "no mounds"
        );
    }

    #[test]
    fn the_amounts_read_by_name() {
        assert_eq!(amount("thick"), Some(0.85));
        assert_eq!(amount(" none "), Some(0.0));
        assert_eq!(amount("lots"), None);
    }
}
