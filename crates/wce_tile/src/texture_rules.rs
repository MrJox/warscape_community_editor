//! Texture rules (docs/plans/auto_detail.md, A): where the ground's textures go, worked out from the terrain itself. A rule names a texture
//! (a texture group of the tile and a variation of it) and the terrain it goes on (ranges of height, slope and curvature, each with soft
//! edges); the rules of a tile are applied in order, a later one over an earlier one, as blend layers go over each other. What they make is
//! one blend layer's pixels: the rules' channels, covering what is below by their summed weight, and nothing where no rule holds.
//!
//! The rules are the editor's: TEd and the game never see them, only the blend map they paint. Their text, in a preset's file or in the
//! tile:
//!
//! ```text
//! <TEXTURE_RULES wce_version="1" preset="Default">
//!     <rule name="Rock on slopes" group="rock" variation="0" slope="35 -" slope_fade="10" breakup="25 0.5" seed="1"/>
//!     <rule name="Mud in hollows" group="mud" variation="1.5" hollow="0.4 -" hollow_radius="12" hollow_fade="0.3"/>
//! </TEXTURE_RULES>
//! ```
//!
//! A range is two numbers, `-` for an open end; its fade is how far its edges blend, centred on them. `enabled="0"` turns a rule off.
//! `preset` is the preset the rules were last loaded from or saved as (a name only: the rules are the tile's own copy).
//!
//! A tile keeps its rules in [`FILE`] beside its `tile.wce`, as its terrain layers: the editor's alone, never in the kit or the packs.

use wce_formats::xml_doc::XmlDoc;

use crate::height_field::{HeightField, fractal_noise};

/// The owner of the rules' blend layer (`terrain_layers`): it goes under the user's layers, so what they paint goes over it.
pub const OWNER: &str = "texture_rules";

/// The root element of a rule set's text.
pub const ROOT: &str = "TEXTURE_RULES";

/// The tile's rules, beside its `tile.wce`.
pub const FILE: &str = "texture_rules.wce";

/// Rules and the preset they came from.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct RuleSet {
    pub preset: String,
    pub rules: Vec<Rule>,
}

/// A range of a condition, either end open, its edges blended over `fade` (centred on them).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Range {
    pub lo: Option<f32>,
    pub hi: Option<f32>,
    pub fade: f32,
}

impl Range {
    /// How much a value is in the range, 0 .. 1.
    #[must_use]
    pub fn weight(&self, v: f64) -> f64 {
        let fade = f64::from(self.fade.max(0.0));
        let edge = |d: f64| if fade <= 0.0 { f64::from(u8::from(d >= 0.0)) } else { smooth(d / fade + 0.5) };
        let lo = self.lo.map_or(1.0, |lo| edge(v - f64::from(lo)));
        let hi = self.hi.map_or(1.0, |hi| edge(f64::from(hi) - v));
        lo * hi
    }

    fn text(&self) -> String {
        let end = |e: Option<f32>| e.map_or_else(|| "-".to_owned(), |v| crate::ted_scene::format_number(f64::from(v)));
        format!("{} {}", end(self.lo), end(self.hi))
    }

    fn parse(s: &str, fade: Option<f32>) -> Option<Self> {
        let mut parts = s.split_whitespace();
        let end = |p: Option<&str>| -> Option<Option<f32>> {
            match p? {
                "-" => Some(None),
                v => v.parse().ok().map(Some),
            }
        };
        let (lo, hi) = (end(parts.next())?, end(parts.next())?);
        Some(Self { lo, hi, fade: fade.unwrap_or(0.0) })
    }
}

fn smooth(t: f64) -> f64 {
    let t = t.clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// A rule: a texture, and the terrain it goes on.
#[derive(Clone, Debug, PartialEq)]
pub struct Rule {
    pub name: String,
    pub enabled: bool,

    /// The texture group, by name (`rock`, `mud`, ...: the climates' `texture_set`s, as the tile's TEXTURE_GROUP names them); several
    /// separated by `|`, the first the tile has (the kit names the same ground `rock`, `rock_b` or `mountain` in different tiles).
    pub group: String,

    /// The group's variation, 0 .. 3, continuous: 1.3 is 70 % of variation 1 and 30 % of 2 (as TEd's Normal (Grouped)).
    pub variation: f32,

    /// The rule's weight where every condition holds, 0 .. 1.
    pub strength: f32,

    /// Metres, as the editor shows them (the sea at 0).
    pub height: Option<Range>,

    /// Degrees.
    pub slope: Option<Range>,

    /// How far a place lies under the mean ground around it (metres; a hollow above 0, a ridge below), over a square `hollow_radius`
    /// metres from it each way: where water gathers, where the wind dries.
    pub hollow: Option<Range>,
    pub hollow_radius: f32,

    /// Metres to the nearest water (rivers, lakes, the sea over the ground), road (the procedural roads' texture) and real building (a
    /// footprint's edge). Where the tile has none of one, the distance to it is endless: a range with an upper end does not hold.
    pub water: Option<Range>,
    pub road: Option<Range>,
    pub building: Option<Range>,

    /// A ground type (`ground_types::GROUND_TYPES`' index) laid where the rule's weight is at least `threshold` (sounds, unit speed:
    /// gameplay, so off unless asked). The latest rule that lays one at a place wins there.
    pub ground: Option<GroundRule>,

    /// Noise over the conditions (features `scale` metres across): each condition's value moved by up to `amount` times its fade, so
    /// that the rule's edges are not contour lines. Inside a range, past its fades, nothing changes.
    pub breakup: Option<Breakup>,
    pub seed: u32,
}

impl Default for Rule {
    fn default() -> Self {
        Self {
            name: String::new(),
            enabled: true,
            group: String::new(),
            variation: 0.0,
            strength: 1.0,
            height: None,
            slope: None,
            hollow: None,
            hollow_radius: 10.0,
            water: None,
            road: None,
            building: None,
            ground: None,
            breakup: None,
            seed: 0,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GroundRule {
    pub kind: u8,
    pub threshold: f32,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Breakup {
    pub scale: f32,
    pub amount: f32,
}

/// The terrain the rules look at: the heights in metres (the editor's, the sea at 0), `w` x `h` samples `mpp` metres apart across and
/// down, from the map's corner (border included, as the blend map's).
#[derive(Clone)]
pub struct Fields {
    w: u32,
    h: u32,
    mpp: (f64, f64),
    heights: Vec<f32>,

    /// The heights summed from the corner (a summed-area table, `w + 1` x `h + 1`): a box's mean in four reads, for the hollows.
    sums: Vec<f64>,

    /// How far each place is from water, roads and buildings ([`Feature`]), when worked out.
    distances: [Option<Distances>; 3],
}

/// What a distance condition measures to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Feature {
    Water = 0,
    Road = 1,
    Building = 2,
}

impl Feature {
    pub const ALL: [Self; 3] = [Self::Water, Self::Road, Self::Building];

    /// Whether any rule that lays something measures to it: what is not, need not be worked out.
    #[must_use]
    pub fn used(self, rules: &[Rule]) -> bool {
        rules.iter().filter(|r| r.enabled && r.strength > 0.0).any(|r| r.distance(self).is_some())
    }
}

/// The distance in metres from every pixel of a grid to the nearest marked one (exact Euclidean, by the pixels' centres), `ppm` pixels a
/// metre across and down, from the map's corner. Endless without any marked pixel.
#[derive(Clone)]
pub struct Distances {
    w: u32,
    h: u32,
    ppm: (f64, f64),
    d: Vec<f32>,
}

impl Distances {
    /// # Panics
    ///
    /// `mask` is not `w` x `h`.
    #[must_use]
    pub fn from_mask(w: u32, h: u32, ppm: (f64, f64), mask: &[bool]) -> Self {
        assert_eq!(mask.len(), w as usize * h as usize);
        let (wu, hu) = (w as usize, h as usize);
        let (sx, sz) = (1.0 / ppm.0, 1.0 / ppm.1);
        // Felzenszwalb and Huttenlocher's squared distance transform, down the columns and then along the rows, in metres.
        let inf = f64::INFINITY;
        let mut g = mask.iter().map(|&m| if m { 0.0 } else { inf }).collect::<Vec<f64>>();
        let mut col = vec![0.0; hu];
        for x in 0..wu {
            for y in 0..hu {
                col[y] = g[y * wu + x];
            }

            let out = transform_1d(&col, sz);
            for y in 0..hu {
                g[y * wu + x] = out[y];
            }
        }

        let mut d = vec![0.0f32; wu * hu];
        for y in 0..hu {
            let out = transform_1d(&g[y * wu..(y + 1) * wu], sx);
            for x in 0..wu {
                d[y * wu + x] = out[x].sqrt() as f32;
            }
        }

        Self { w, h, ppm, d }
    }

    /// The distance at a point (the pixel it is in), metres from the map's corner.
    #[must_use]
    pub fn at(&self, wx: f64, wz: f64) -> f64 {
        let x = ((wx * self.ppm.0).floor().max(0.0) as u32).min(self.w - 1);
        let y = ((wz * self.ppm.1).floor().max(0.0) as u32).min(self.h - 1);
        f64::from(self.d[y as usize * self.w as usize + x as usize])
    }
}

/// The pixels of a grid (`w` x `h`, `ppm` pixels a metre, from the map's corner, which lies `border` metres before the tile's world
/// origin) under water: a river's ribbon or a lake's plane whose surface is over the ground there, or the sea (at 0) over it. The heights
/// are `fields`'.
#[must_use]
pub fn water_mask(water: &crate::roads::WaterMap, fields: &Fields, w: u32, h: u32, ppm: (f64, f64), border: f64) -> Vec<bool> {
    let mut mask = vec![false; w as usize * h as usize];
    let centre = |x: u32, y: u32| ((f64::from(x) + 0.5) / ppm.0, (f64::from(y) + 0.5) / ppm.1);
    let ground = |x: u32, y: u32| {
        let (mx, mz) = centre(x, y);
        fields.height(mx, mz)
    };
    // The pixels of a world-space box, clamped to the grid.
    let span = |lo: f64, hi: f64, p: f64, n: u32| {
        let a = (((lo + border) * p).floor().max(0.0) as u32).min(n);
        let b = (((hi + border) * p).ceil().max(0.0) as u32).min(n);
        a..b
    };

    if water.sea {
        for y in 0..h {
            for x in 0..w {
                mask[(y * w + x) as usize] = ground(x, y) < 0.0;
            }
        }
    }

    for river in &water.rivers {
        for seg in river.windows(2) {
            let (a, b) = (seg[0], seg[1]);
            let r = a[3].max(b[3]);
            let d = [b[0] - a[0], b[1] - a[1]];
            let l2 = (d[0] * d[0] + d[1] * d[1]).max(1e-12);
            for y in span(a[1].min(b[1]) - r, a[1].max(b[1]) + r, ppm.1, h) {
                for x in span(a[0].min(b[0]) - r, a[0].max(b[0]) + r, ppm.0, w) {
                    let (mx, mz) = centre(x, y);
                    let (px, pz) = (mx - border, mz - border);
                    let t = (((px - a[0]) * d[0] + (pz - a[1]) * d[1]) / l2).clamp(0.0, 1.0);
                    let (qx, qz) = (a[0] + d[0] * t, a[1] + d[1] * t);
                    if (px - qx).hypot(pz - qz) <= a[3] + (b[3] - a[3]) * t && a[2] + (b[2] - a[2]) * t > ground(x, y) {
                        mask[(y * w + x) as usize] = true;
                    }
                }
            }
        }
    }

    for (corners, level) in &water.planes {
        let (xs, zs) = (corners.map(|c| c[0]), corners.map(|c| c[1]));
        let (x0, x1) = (xs.iter().copied().fold(f64::INFINITY, f64::min), xs.iter().copied().fold(f64::NEG_INFINITY, f64::max));
        let (z0, z1) = (zs.iter().copied().fold(f64::INFINITY, f64::min), zs.iter().copied().fold(f64::NEG_INFINITY, f64::max));
        for y in span(z0, z1, ppm.1, h) {
            for x in span(x0, x1, ppm.0, w) {
                let (mx, mz) = centre(x, y);
                let (px, pz) = (mx - border, mz - border);
                let side = |i: usize| {
                    let (a, b) = (corners[i], corners[(i + 1) % 4]);
                    (b[0] - a[0]) * (pz - a[1]) - (b[1] - a[1]) * (px - a[0])
                };
                let s = [side(0), side(1), side(2), side(3)];
                if (s.iter().all(|&v| v >= 0.0) || s.iter().all(|&v| v <= 0.0)) && *level > ground(x, y) {
                    mask[(y * w + x) as usize] = true;
                }
            }
        }
    }

    mask
}

/// The pixels of a grid (as [`water_mask`]'s) inside discs (x, z in world metres, radius), each disc's centre pixel at least.
#[must_use]
pub fn disc_mask(discs: &[[f64; 3]], w: u32, h: u32, ppm: (f64, f64), border: f64) -> Vec<bool> {
    let mut mask = vec![false; w as usize * h as usize];
    for &[cx, cz, r] in discs {
        let (px, py) = ((cx + border) * ppm.0, (cz + border) * ppm.1);
        let (ix, iy) = (px.floor(), py.floor());
        if ix >= 0.0 && iy >= 0.0 && ix < f64::from(w) && iy < f64::from(h) {
            mask[iy as usize * w as usize + ix as usize] = true;
        }

        let (y0, y1) = (((py - r * ppm.1).floor().max(0.0) as u32).min(h), ((py + r * ppm.1).ceil().max(0.0) as u32).min(h));
        let (x0, x1) = (((px - r * ppm.0).floor().max(0.0) as u32).min(w), ((px + r * ppm.0).ceil().max(0.0) as u32).min(w));
        for y in y0..y1 {
            for x in x0..x1 {
                let (dx, dz) = ((f64::from(x) + 0.5) / ppm.0 - border - cx, (f64::from(y) + 0.5) / ppm.1 - border - cz);
                if dx.hypot(dz) <= r {
                    mask[(y * w + x) as usize] = true;
                }
            }
        }
    }

    mask
}

/// One line of the squared distance transform: `f` the squared distances so far (0 at a feature, endless away), samples `step` metres
/// apart; the squared distance of each sample to the nearest one's parabola.
fn transform_1d(f: &[f64], step: f64) -> Vec<f64> {
    let n = f.len();
    let mut out = vec![f64::INFINITY; n];
    let mut v = vec![0usize; n];
    let mut z = vec![0.0f64; n + 1];
    let mut k = 0usize;
    let first = f.iter().position(|v| v.is_finite());
    let Some(first) = first else { return out };
    v[0] = first;
    z[0] = f64::NEG_INFINITY;
    z[1] = f64::INFINITY;
    let pos = |i: usize| i as f64 * step;
    for q in first + 1..n {
        if !f[q].is_finite() {
            continue;
        }

        loop {
            let p = v[k];
            let s = ((f[q] + pos(q) * pos(q)) - (f[p] + pos(p) * pos(p))) / (2.0 * (pos(q) - pos(p)));
            if s <= z[k] && k > 0 {
                k -= 1;
                continue;
            }

            k += 1;
            v[k] = q;
            z[k] = s;
            z[k + 1] = f64::INFINITY;
            break;
        }
    }

    k = 0;
    for (q, o) in out.iter_mut().enumerate() {
        while z[k + 1] < pos(q) {
            k += 1;
        }

        let d = pos(q) - pos(v[k]);
        *o = d * d + f[v[k]];
    }

    out
}

impl Fields {
    /// # Panics
    ///
    /// `heights` is not `w` x `h`, or the map is empty.
    #[must_use]
    pub fn new(w: u32, h: u32, mpp: (f64, f64), heights: Vec<f32>) -> Self {
        assert!(w > 0 && h > 0 && heights.len() == w as usize * h as usize, "heights of {} for {w} x {h}", heights.len());
        let (sw, sh) = (w as usize + 1, h as usize + 1);
        let mut sums = vec![0.0; sw * sh];
        for y in 0..h as usize {
            let mut row = 0.0;
            for x in 0..w as usize {
                row += f64::from(heights[y * w as usize + x]);
                sums[(y + 1) * sw + x + 1] = sums[y * sw + x + 1] + row;
            }
        }

        Self { w, h, mpp, heights, sums, distances: [None, None, None] }
    }

    /// From a tile's height field, its samples `mpp` metres apart; `unit`, the tile's scene units a metre (the editor's metres are the
    /// field's divided by it, as the paint brush's filters read them).
    #[must_use]
    pub fn from_height_field(field: &HeightField, mpp: (f64, f64), unit: f64) -> Self {
        let (w, h) = (field.width(), field.height());
        let heights = (0..h).flat_map(|y| (0..w).map(move |x| (y, x))).map(|(y, x)| (field.metres(x, y) / unit) as f32).collect();
        Self::new(w, h, mpp, heights)
    }

    fn at(&self, x: i64, y: i64) -> f64 {
        let (x, y) = (x.clamp(0, i64::from(self.w) - 1) as usize, y.clamp(0, i64::from(self.h) - 1) as usize);
        f64::from(self.heights[y * self.w as usize + x])
    }

    /// The height (bilinear) at a point, metres from the map's corner.
    #[must_use]
    pub fn height(&self, wx: f64, wz: f64) -> f64 {
        let (x, y) = ((wx / self.mpp.0).clamp(0.0, f64::from(self.w - 1)), (wz / self.mpp.1).clamp(0.0, f64::from(self.h - 1)));
        let (x0, y0) = (x.floor() as i64, y.floor() as i64);
        let (fx, fy) = (x - x.floor(), y - y.floor());
        let a = self.at(x0, y0) * (1.0 - fx) + self.at(x0 + 1, y0) * fx;
        let b = self.at(x0, y0 + 1) * (1.0 - fx) + self.at(x0 + 1, y0 + 1) * fx;
        a * (1.0 - fy) + b * fy
    }

    /// The slope in degrees at a point (central differences a sample apart, as the paint brush's filters).
    #[must_use]
    pub fn slope(&self, wx: f64, wz: f64) -> f64 {
        let (mx, mz) = self.mpp;
        let gx = (self.height(wx + mx, wz) - self.height(wx - mx, wz)) / (2.0 * mx);
        let gz = (self.height(wx, wz + mz) - self.height(wx, wz - mz)) / (2.0 * mz);
        gx.hypot(gz).atan().to_degrees()
    }

    /// Gives the distances to a feature.
    #[must_use]
    pub fn with_distances(mut self, feature: Feature, d: Distances) -> Self {
        self.distances[feature as usize] = Some(d);
        self
    }

    /// Metres from a point to the nearest of a feature: endless when not worked out, or there is none.
    #[must_use]
    pub fn distance(&self, feature: Feature, wx: f64, wz: f64) -> f64 {
        self.distances[feature as usize].as_ref().map_or(f64::INFINITY, |d| d.at(wx, wz))
    }

    /// How far a point lies under the mean ground of the square `radius` metres around it each way (a hollow above 0).
    #[must_use]
    pub fn hollow(&self, wx: f64, wz: f64, radius: f64) -> f64 {
        let (cx, cy) = ((wx / self.mpp.0).round() as i64, (wz / self.mpp.1).round() as i64);
        let (rx, ry) = ((radius / self.mpp.0).round().max(1.0) as i64, (radius / self.mpp.1).round().max(1.0) as i64);
        let (x0, x1) = ((cx - rx).max(0) as usize, ((cx + rx + 1).min(i64::from(self.w))).max(0) as usize);
        let (y0, y1) = ((cy - ry).max(0) as usize, ((cy + ry + 1).min(i64::from(self.h))).max(0) as usize);
        if x1 <= x0 || y1 <= y0 {
            return 0.0;
        }

        let sw = self.w as usize + 1;
        let s = |x: usize, y: usize| self.sums[y * sw + x];
        let sum = s(x1, y1) - s(x0, y1) - s(x1, y0) + s(x0, y0);
        sum / ((x1 - x0) * (y1 - y0)) as f64 - self.height(wx, wz)
    }
}

/// How far past a changed place of the heights the rules' result can change (metres): the hollows' reach, a sample's for the slope.
/// (The distances reach as far as their features move: those changes remake the whole map.)
#[must_use]
pub fn reach(rules: &[Rule], mpp: (f64, f64)) -> f64 {
    let sample = mpp.0.max(mpp.1);
    rules.iter().filter(|r| r.enabled && r.hollow.is_some()).map(|r| f64::from(r.hollow_radius)).fold(sample, f64::max) + sample
}

/// The rules whose texture group the tile has not (`groups`: the names of its TEXTURE_GROUP's four slots): they lay nothing.
#[must_use]
pub fn missing_groups<'a>(rules: &'a [Rule], groups: &[String]) -> Vec<&'a Rule> {
    rules.iter().filter(|r| r.enabled && slot_of(groups, &r.group).is_none()).collect()
}

fn slot_of(groups: &[String], names: &str) -> Option<usize> {
    names.split('|').map(str::trim).filter(|n| !n.is_empty()).find_map(|n| groups.iter().take(4).position(|g| g.eq_ignore_ascii_case(n)))
}

impl Rule {
    /// The rule's distance condition to a feature.
    #[must_use]
    pub fn distance(&self, feature: Feature) -> Option<&Range> {
        match feature {
            Feature::Water => self.water.as_ref(),
            Feature::Road => self.road.as_ref(),
            Feature::Building => self.building.as_ref(),
        }
    }

    /// The rule's weight at a point (metres from the map's corner), 0 .. 1.
    #[must_use]
    pub fn weight(&self, fields: &Fields, wx: f64, wz: f64) -> f64 {
        // One noise value moves every condition (each by its own fade): edges wobble together, as one patch of ground.
        let n = self.breakup.filter(|b| b.amount > 0.0 && b.scale > 0.0).map_or(0.0, |b| {
            let (ox, oz) = seed_offset(self.seed);
            f64::from(b.amount) * fractal_noise(wx / f64::from(b.scale) + ox, wz / f64::from(b.scale) + oz)
        });

        let cond = |r: &Option<Range>, v: &dyn Fn() -> f64| r.map_or(1.0, |r| r.weight(v() + n * f64::from(r.fade)));
        let mut w = f64::from(self.strength.clamp(0.0, 1.0));
        for (range, value) in [
            (&self.height, &(|| fields.height(wx, wz)) as &dyn Fn() -> f64),
            (&self.slope, &|| fields.slope(wx, wz)),
            (&self.hollow, &|| fields.hollow(wx, wz, f64::from(self.hollow_radius))),
            (&self.water, &|| fields.distance(Feature::Water, wx, wz)),
            (&self.road, &|| fields.distance(Feature::Road, wx, wz)),
            (&self.building, &|| fields.distance(Feature::Building, wx, wz)),
        ] {
            if w <= 0.0 {
                break;
            }

            w *= cond(range, value);
        }

        w
    }
}

/// Where a seed puts its noise: far apart, so that two rules' noises do not match.
fn seed_offset(seed: u32) -> (f64, f64) {
    let h = u64::from(seed).wrapping_mul(0x9E37_79B9_7F4A_7C15);
    (((h >> 40) & 0xFFFF) as f64 * 7.13, ((h >> 16) & 0xFFFF) as f64 * 5.71)
}

/// The rules' layer over a rectangle of a blend map `size` pixels big that covers the same ground as `fields` (`ppm`: its pixels a metre
/// across and down): `put` gets each pixel the rules hold at (its 16 channels, 4 x slot + variation; summing to the cover, 65535 being
/// all). `groups`: the tile's TEXTURE_GROUP slot names.
pub fn generate(
    rules: &[Rule],
    groups: &[String],
    fields: &Fields,
    ppm: (f64, f64),
    rect: [u32; 4],
    put: &mut dyn FnMut(u32, u32, [u16; 16]),
) {
    let live = rules
        .iter()
        .filter(|r| r.enabled && r.strength > 0.0)
        .filter_map(|r| slot_of(groups, &r.group).map(|slot| (r, slot)))
        .collect::<Vec<_>>();
    if live.is_empty() {
        return;
    }

    let [x0, y0, x1, y1] = rect;
    for y in y0..y1 {
        for x in x0..x1 {
            let (wx, wz) = ((f64::from(x) + 0.5) / ppm.0, (f64::from(y) + 0.5) / ppm.1);
            let mut acc = [0.0f64; 16];
            for &(rule, slot) in &live {
                let a = rule.weight(fields, wx, wz);
                if a <= 0.0 {
                    continue;
                }

                for c in &mut acc {
                    *c *= 1.0 - a;
                }

                let v = f64::from(rule.variation).clamp(0.0, 3.0);
                let (lo, frac) = (v.floor() as usize, v - v.floor());
                acc[slot * 4 + lo] += a * (1.0 - frac);
                if frac > 0.0 {
                    acc[slot * 4 + (lo + 1).min(3)] += a * frac;
                }
            }

            if let Some(px) = pixel(&acc) {
                put(x, y, px);
            }
        }
    }
}

/// Whether any rule that lays something lays a ground type.
#[must_use]
pub fn lays_ground(rules: &[Rule]) -> bool {
    rules.iter().any(|r| r.enabled && r.strength > 0.0 && r.ground.is_some())
}

/// The rules' ground type layer over a rectangle of a ground type map (as [`generate`] the blend map's: `ppm` its pixels a metre): `put`
/// gets each pixel a rule lays a ground type at (its colour, alpha 255), the latest such rule's. A rule whose texture group the tile has
/// not lays no ground type either.
pub fn generate_ground(
    rules: &[Rule],
    groups: &[String],
    fields: &Fields,
    ppm: (f64, f64),
    rect: [u32; 4],
    put: &mut dyn FnMut(u32, u32, [u8; 4]),
) {
    let live = rules
        .iter()
        .filter(|r| r.enabled && r.strength > 0.0 && slot_of(groups, &r.group).is_some())
        .filter_map(|r| Some((r, r.ground?, crate::ground_types::GROUND_COLOURS.get(usize::from(r.ground?.kind))?)))
        .collect::<Vec<_>>();
    if live.is_empty() {
        return;
    }

    let [x0, y0, x1, y1] = rect;
    for y in y0..y1 {
        for x in x0..x1 {
            let (wx, wz) = ((f64::from(x) + 0.5) / ppm.0, (f64::from(y) + 0.5) / ppm.1);
            if let Some(&(_, _, c)) = live.iter().rev().find(|(r, g, _)| r.weight(fields, wx, wz) >= f64::from(g.threshold.max(1e-6))) {
                put(x, y, [c[0], c[1], c[2], 255]);
            }
        }
    }
}

/// A layer's pixel from weights summing to at most 1: `None` when it holds nothing.
fn pixel(acc: &[f64; 16]) -> Option<[u16; 16]> {
    let mut out = [0u16; 16];
    for (o, &a) in out.iter_mut().zip(acc) {
        *o = (a.clamp(0.0, 1.0) * 65535.0).floor() as u16;
    }

    // Floored, so the sum stays within 65535; a full cover is made whole on its strongest channel.
    let sum = out.iter().map(|&v| u32::from(v)).sum::<u32>();
    if sum == 0 {
        return None;
    }

    if acc.iter().sum::<f64>() >= 1.0 - 1e-9
        && let Some(top) = out.iter_mut().max()
    {
        *top += (65535 - sum) as u16;
    }

    Some(out)
}

/// A rule set's text.
#[must_use]
pub fn text(set: &RuleSet) -> Vec<u8> {
    let num = |v: f32| crate::ted_scene::format_number(f64::from(v));
    let mut out = format!("<{ROOT} {}=\"{}\"", crate::project_files::WCE_VERSION_ATTR, crate::project_files::WCE_VERSION);
    if !set.preset.is_empty() {
        out += &format!(" preset=\"{}\"", escape(&set.preset));
    }

    out += ">\n";
    for r in &set.rules {
        out += &format!("    <rule name=\"{}\" group=\"{}\" variation=\"{}\"", escape(&r.name), escape(&r.group), num(r.variation));
        if !r.enabled {
            out += " enabled=\"0\"";
        }

        if r.strength != 1.0 {
            out += &format!(" strength=\"{}\"", num(r.strength));
        }

        for (name, range) in [
            ("height", &r.height),
            ("slope", &r.slope),
            ("hollow", &r.hollow),
            ("water", &r.water),
            ("road", &r.road),
            ("building", &r.building),
        ] {
            if let Some(range) = range {
                out += &format!(" {name}=\"{}\"", range.text());
                if range.fade > 0.0 {
                    out += &format!(" {name}_fade=\"{}\"", num(range.fade));
                }
            }
        }

        if r.hollow.is_some() {
            out += &format!(" hollow_radius=\"{}\"", num(r.hollow_radius));
        }

        if let Some(g) = r.ground
            && let Some((name, _)) = crate::ground_types::GROUND_TYPES.get(usize::from(g.kind))
        {
            out += &format!(" ground=\"{name}\" ground_threshold=\"{}\"", num(g.threshold));
        }

        if let Some(b) = r.breakup {
            out += &format!(" breakup=\"{} {}\"", num(b.scale), num(b.amount));
        }

        if r.seed != 0 {
            out += &format!(" seed=\"{}\"", r.seed);
        }

        out += "/>\n";
    }

    out += &format!("</{ROOT}>\n");
    out.into_bytes()
}

fn escape(s: &str) -> String {
    s.replace('&', "&amp;").replace('"', "&quot;").replace('<', "&lt;").replace('>', "&gt;")
}

/// A rule set from its text.
///
/// # Errors
///
/// It is not XML, or not a rule set.
pub fn parse(bytes: &[u8]) -> Result<RuleSet, String> {
    let doc = XmlDoc::parse(bytes.to_vec()).map_err(|e| e.to_string())?;
    let els = doc.elements();
    if els.first().is_none_or(|e| e.name != ROOT) {
        return Err("not a texture rule set".to_owned());
    }

    let num = |e: usize, n: &str| doc.get(e, n).and_then(|v| v.trim().parse::<f32>().ok());
    let range = |e: usize, n: &str| doc.get(e, n).and_then(|v| Range::parse(&v, num(e, &format!("{n}_fade"))));
    let rules = (0..els.len())
        .filter(|&e| els[e].name == "rule")
        .map(|e| {
            let d = Rule::default();
            Rule {
                name: doc.get(e, "name").unwrap_or_default(),
                enabled: doc.get(e, "enabled").is_none_or(|v| v.trim() != "0"),
                group: doc.get(e, "group").unwrap_or_default(),
                variation: num(e, "variation").unwrap_or(d.variation),
                strength: num(e, "strength").unwrap_or(d.strength),
                height: range(e, "height"),
                slope: range(e, "slope"),
                hollow: range(e, "hollow"),
                water: range(e, "water"),
                road: range(e, "road"),
                building: range(e, "building"),
                ground: doc.get(e, "ground").and_then(|name| {
                    let kind = crate::ground_types::GROUND_TYPES.iter().position(|(n, _)| n.eq_ignore_ascii_case(name.trim()))?;
                    Some(GroundRule { kind: kind as u8, threshold: num(e, "ground_threshold").unwrap_or(0.5) })
                }),
                hollow_radius: num(e, "hollow_radius").unwrap_or(d.hollow_radius),
                breakup: doc.get(e, "breakup").and_then(|v| {
                    let mut p = v.split_whitespace().map(str::parse::<f32>);
                    Some(Breakup { scale: p.next()?.ok()?, amount: p.next()?.ok()? })
                }),
                seed: doc.get(e, "seed").and_then(|v| v.trim().parse().ok()).unwrap_or(0),
            }
        })
        .collect();
    Ok(RuleSet { preset: doc.get(0, "preset").unwrap_or_default(), rules })
}

/// The built-in preset's name.
pub const BUILTIN: &str = "Default";

/// A preset of rules: its name (the file's), the folder below `texture_rule_sets` it is in, its file (none: built in) and its rules.
/// The editor's files, like the vegetation sets: `<name>.wce` in the projects' `raw_data/art/texture_rule_sets` (folders below it are
/// kept), the text of a rule set ([`text`]).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Preset {
    pub name: String,
    pub folder: String,
    pub path: Option<std::path::PathBuf>,
    pub rules: Vec<Rule>,
}

/// The folder of the presets in a raw_data folder (the projects').
#[must_use]
pub fn presets_root(raw_data: &std::path::Path) -> std::path::PathBuf {
    raw_data.join("art").join("texture_rule_sets")
}

impl Preset {
    /// The built-in preset.
    #[must_use]
    pub fn builtin() -> Self {
        Self { name: BUILTIN.to_owned(), rules: default_rules(), ..Self::default() }
    }

    #[must_use]
    pub fn is_builtin(&self) -> bool {
        self.path.is_none()
    }

    /// Reads a preset file.
    ///
    /// # Errors
    ///
    /// The file can't be read or is not a rule set.
    pub fn load(path: &std::path::Path, root: &std::path::Path) -> Result<Self, String> {
        let set =
            parse(&std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?).map_err(|e| format!("{}: {e}", path.display()))?;
        let name = path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
        let folder =
            path.parent().and_then(|p| p.strip_prefix(root).ok()).map(|p| p.to_string_lossy().replace('\\', "/")).unwrap_or_default();
        Ok(Self { name, folder, path: Some(path.to_path_buf()), rules: set.rules })
    }

    /// Writes the preset to its file (its folders too).
    ///
    /// # Errors
    ///
    /// A built-in preset (no file), or the file can't be written.
    pub fn save(&self) -> Result<(), String> {
        let path = self.path.as_ref().ok_or("a built-in preset has no file")?;
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
        }

        wce_foundation::files::write_replacing(path, text(&RuleSet { preset: String::new(), rules: self.rules.clone() }))
            .map_err(|e| format!("{}: {e}", path.display()))
    }
}

/// The built-in preset, then every preset below `root` (`.wce` files) by folder and name; ones that don't read are left out.
#[must_use]
pub fn scan_presets(root: &std::path::Path) -> Vec<Preset> {
    let mut out = vec![];
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let Ok(read) = std::fs::read_dir(&dir) else { continue };
        for path in read.flatten().map(|e| e.path()) {
            if path.is_dir() {
                stack.push(path);
            } else if path.extension().is_some_and(|x| x == "wce")
                && let Ok(p) = Preset::load(&path, root)
            {
                out.push(p);
            }
        }
    }

    out.sort_by_cached_key(|p| (p.folder.to_lowercase(), p.name.to_lowercase()));
    out.insert(0, Preset::builtin());
    out
}

/// The kit's names of mud and of rock, the most usual first (its TILES entries).
const MUD: &str = "mud|mud_a|mud_b";
const ROCK: &str = "rock|rock_b|mountain";

/// The built-in set `Default`: mud by the water and in hollows, worn ground by roads and buildings, rock on steep ground. The groups are the kit's usual names; a tile without one lays nothing
/// of that rule.
#[must_use]
pub fn default_rules() -> Vec<Rule> {
    vec![
        Rule {
            name: "Mud by the water".to_owned(),
            group: MUD.to_owned(),
            variation: 0.0,
            strength: 0.9,
            slope: Some(Range { lo: None, hi: Some(25.0), fade: 8.0 }),
            water: Some(Range { lo: None, hi: Some(4.0), fade: 4.0 }),
            breakup: Some(Breakup { scale: 8.0, amount: 0.7 }),
            seed: 3,
            ..Rule::default()
        },
        Rule {
            name: "Worn ground by roads".to_owned(),
            group: MUD.to_owned(),
            variation: 2.0,
            strength: 0.6,
            road: Some(Range { lo: None, hi: Some(2.0), fade: 3.0 }),
            breakup: Some(Breakup { scale: 6.0, amount: 0.8 }),
            seed: 4,
            ..Rule::default()
        },
        Rule {
            name: "Worn ground round buildings".to_owned(),
            group: MUD.to_owned(),
            variation: 2.0,
            strength: 0.5,
            slope: Some(Range { lo: None, hi: Some(25.0), fade: 8.0 }),
            building: Some(Range { lo: None, hi: Some(3.0), fade: 3.0 }),
            breakup: Some(Breakup { scale: 6.0, amount: 0.8 }),
            seed: 5,
            ..Rule::default()
        },
        Rule {
            name: "Mud in hollows".to_owned(),
            group: MUD.to_owned(),
            variation: 1.0,
            strength: 0.8,
            slope: Some(Range { lo: None, hi: Some(20.0), fade: 8.0 }),
            hollow: Some(Range { lo: Some(0.4), hi: None, fade: 0.4 }),
            hollow_radius: 12.0,
            breakup: Some(Breakup { scale: 15.0, amount: 0.6 }),
            seed: 2,
            ..Rule::default()
        },
        Rule {
            name: "Rock on slopes".to_owned(),
            group: ROCK.to_owned(),
            variation: 0.0,
            slope: Some(Range { lo: Some(35.0), hi: None, fade: 12.0 }),
            breakup: Some(Breakup { scale: 25.0, amount: 0.5 }),
            seed: 1,
            ..Rule::default()
        },
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A map of `n` x `n` samples a metre apart from a height function of (x, z) metres.
    fn field(n: u32, f: impl Fn(f64, f64) -> f64) -> Fields {
        let heights = (0..n).flat_map(|y| (0..n).map(move |x| (x, y))).map(|(x, y)| f(f64::from(x), f64::from(y)) as f32).collect();
        Fields::new(n, n, (1.0, 1.0), heights)
    }

    fn groups() -> Vec<String> {
        ["grass", "mud", "forest", "rock"].map(str::to_owned).to_vec()
    }

    fn layer(rules: &[Rule], f: &Fields, n: u32) -> Vec<Option<[u16; 16]>> {
        let mut out = vec![None; (n * n) as usize];
        generate(rules, &groups(), f, (1.0, 1.0), [0, 0, n, n], &mut |x, y, p| out[(y * n + x) as usize] = Some(p));
        out
    }

    fn rock() -> Rule {
        Rule {
            name: "rock".to_owned(),
            group: "rock".to_owned(),
            slope: Some(Range { lo: Some(30.0), hi: None, fade: 0.0 }),
            ..Rule::default()
        }
    }

    #[test]
    fn range_weights_and_fades() {
        let r = Range { lo: Some(10.0), hi: Some(20.0), fade: 4.0 };
        assert_eq!(r.weight(0.0), 0.0);
        assert!((r.weight(10.0) - 0.5).abs() < 1e-9);
        assert_eq!(r.weight(15.0), 1.0);
        assert!((r.weight(20.0) - 0.5).abs() < 1e-9);
        assert_eq!(r.weight(30.0), 0.0);
        let hard = Range { lo: Some(10.0), hi: None, fade: 0.0 };
        assert_eq!(hard.weight(9.99), 0.0);
        assert_eq!(hard.weight(10.0), 1.0);
        assert_eq!(hard.weight(1e9), 1.0);
    }

    #[test]
    fn a_ramp_gets_rock_where_it_is_steep() {
        // Flat for x < 32, then rising 1 m a metre (45 degrees).
        let f = field(64, |x, _| (x - 32.0).max(0.0));
        let l = layer(&[rock()], &f, 64);
        assert!(l[(32 * 64 + 10) as usize].is_none(), "flat ground has no rock");
        let steep = l[(32 * 64 + 50) as usize].expect("steep ground has rock");
        assert_eq!(steep[12], 65535, "all of the rock group's variation 0: {steep:?}");
    }

    #[test]
    fn a_bowl_gets_mud_at_its_bottom() {
        let f = field(64, |x, z| ((x - 32.0).powi(2) + (z - 32.0).powi(2)) / 100.0);
        let mud = Rule {
            group: "mud".to_owned(),
            variation: 1.5,
            hollow: Some(Range { lo: Some(0.5), hi: None, fade: 0.0 }),
            hollow_radius: 10.0,
            ..Rule::default()
        };
        let l = layer(&[mud], &f, 64);
        let bottom = l[(32 * 64 + 32) as usize].expect("the bottom is a hollow");
        assert_eq!(u32::from(bottom[5]) + u32::from(bottom[6]), 65535);
        assert!(bottom[5].abs_diff(bottom[6]) <= 1, "half of variations 1 and 2: {bottom:?}");
        assert!(l[(32 * 64 + 60) as usize].is_none(), "the bowl's side is no hollow");
    }

    #[test]
    fn a_later_rule_goes_over_an_earlier_one() {
        let f = field(32, |x, _| x * 2.0);
        let half_mud = Rule { group: "mud".to_owned(), strength: 1.0, ..Rule::default() };
        let half_rock = Rule { strength: 0.5, ..rock() };
        let p = layer(&[half_mud, half_rock], &f, 32)[(16 * 32 + 16) as usize].unwrap();
        assert_eq!(u32::from(p[4]) + u32::from(p[12]), 65535);
        assert!(p[4].abs_diff(32767) <= 1 && p[12].abs_diff(32767) <= 1, "{p:?}");
    }

    #[test]
    fn partial_cover_and_unknown_groups() {
        let f = field(16, |_, _| 0.0);
        let faint = Rule { group: "grass".to_owned(), variation: 2.0, strength: 0.25, ..Rule::default() };
        let p = layer(std::slice::from_ref(&faint), &f, 16)[0].unwrap();
        assert_eq!(p[2], (0.25f64 * 65535.0).floor() as u16);
        assert_eq!(p.iter().map(|&v| u32::from(v)).sum::<u32>(), u32::from(p[2]));

        let snow = Rule { group: "snow".to_owned(), ..Rule::default() };
        assert!(layer(std::slice::from_ref(&snow), &f, 16).iter().all(Option::is_none));
        assert_eq!(missing_groups(&[faint, snow.clone()], &groups()).len(), 1);

        // Alternatives: the first the tile has.
        let either = Rule { group: "snow | forest|rock".to_owned(), ..Rule::default() };
        assert_eq!(layer(std::slice::from_ref(&either), &f, 16)[0].unwrap()[8], 65535, "forest, slot 2");
    }

    #[test]
    fn breakup_moves_edges_only() {
        // A slope rising across the map: 0 degrees at x = 0 to about 63 at x = 63.
        let f = field(64, |x, _| x * x / 64.0);
        let mut r = Rule { slope: Some(Range { lo: Some(40.0), hi: None, fade: 10.0 }), ..rock() };
        let plain = layer(std::slice::from_ref(&r), &f, 64);
        r.breakup = Some(Breakup { scale: 8.0, amount: 1.0 });
        let noisy = layer(std::slice::from_ref(&r), &f, 64);
        // Far from the edge (40 degrees at x of about 27; the fade with the noise reaches 30 .. 50) the two agree.
        for y in 0..64 {
            assert_eq!(plain[(y * 64 + 2) as usize], noisy[(y * 64 + 2) as usize]);
            assert_eq!(plain[(y * 64 + 63) as usize], noisy[(y * 64 + 63) as usize]);
        }

        assert_ne!(plain, noisy, "the edge moved");
    }

    #[test]
    fn distances_are_euclidean_in_metres() {
        // 20 x 10 pixels, two a metre across, one down; one marked pixel at (4, 5).
        let (w, h) = (20u32, 10u32);
        let mut mask = vec![false; (w * h) as usize];
        mask[(5 * w + 4) as usize] = true;
        let d = Distances::from_mask(w, h, (2.0, 1.0), &mask);
        let at = |x: u32, y: u32| d.at((f64::from(x) + 0.5) / 2.0, f64::from(y) + 0.5);
        assert_eq!(at(4, 5), 0.0);
        assert!((at(10, 5) - 3.0).abs() < 1e-6, "6 pixels across: 3 m");
        assert!((at(4, 9) - 4.0).abs() < 1e-6, "4 pixels down: 4 m");
        assert!((at(10, 9) - 5.0).abs() < 1e-6, "3-4-5");
        let none = Distances::from_mask(w, h, (1.0, 1.0), &vec![false; (w * h) as usize]);
        assert!(none.at(3.0, 3.0).is_infinite());
    }

    #[test]
    fn mud_by_the_water_and_nothing_without_water() {
        let f = field(32, |_, _| 0.0);
        let mut mask = vec![false; 32 * 32];
        for y in 0..32 {
            mask[y * 32] = true;
        }

        let by_water = Rule { group: "mud".to_owned(), water: Some(Range { lo: None, hi: Some(5.0), fade: 0.0 }), ..Rule::default() };
        assert!(layer(std::slice::from_ref(&by_water), &f, 32).iter().all(Option::is_none), "no water known: endless");
        let f = f.with_distances(Feature::Water, Distances::from_mask(32, 32, (1.0, 1.0), &mask));
        let l = layer(std::slice::from_ref(&by_water), &f, 32);
        assert!(l[(10 * 32 + 3) as usize].is_some(), "3 m from the water");
        assert!(l[(10 * 32 + 8) as usize].is_none(), "8 m from it");
        assert!(Feature::Water.used(&[by_water]) && !Feature::Road.used(&[rock()]));
    }

    #[test]
    fn masks_of_water_and_buildings() {
        // A 40 x 40 m map, a pixel a metre, the tile's origin 4 m in (its border), the ground at 1 m.
        let f = field(40, |_, _| 1.0);
        let river =
            crate::roads::WaterMap { rivers: vec![vec![[0.0, 10.0, 2.0, 2.0], [30.0, 10.0, 2.0, 2.0]]], planes: vec![], sea: false };
        let m = water_mask(&river, &f, 40, 40, (1.0, 1.0), 4.0);
        assert!(m[(14 * 40 + 10) as usize], "on the river's line");
        assert!(m[(15 * 40 + 10) as usize] && !m[(17 * 40 + 10) as usize], "2 m wide each side");
        let dry = crate::roads::WaterMap { rivers: vec![vec![[0.0, 10.0, 0.5, 2.0], [30.0, 10.0, 0.5, 2.0]]], planes: vec![], sea: false };
        assert!(water_mask(&dry, &f, 40, 40, (1.0, 1.0), 4.0).iter().all(|&v| !v), "under the ground");
        let sea = crate::roads::WaterMap { rivers: vec![], planes: vec![], sea: true };
        let low = field(40, |x, _| x - 20.0);
        let m = water_mask(&sea, &low, 40, 40, (1.0, 1.0), 4.0);
        assert!(m[(5 * 40 + 10) as usize] && !m[(5 * 40 + 30) as usize]);

        let m = disc_mask(&[[10.0, 10.0, 3.0], [30.0, 30.0, 0.0]], 40, 40, (1.0, 1.0), 4.0);
        assert!(m[(14 * 40 + 14) as usize] && m[(14 * 40 + 16) as usize] && !m[(14 * 40 + 19) as usize]);
        assert!(m[(34 * 40 + 34) as usize], "a point marks its pixel");
    }

    #[test]
    fn ground_types_where_the_weight_is_enough() {
        // A ramp from flat to 45 degrees: rock past 30 at full weight, the fade 20 wide.
        let f = field(64, |x, _| (x - 32.0).max(0.0));
        let rock = Rule {
            slope: Some(Range { lo: Some(30.0), hi: None, fade: 0.0 }),
            ground: Some(GroundRule { kind: 5, threshold: 0.5 }),
            ..rock()
        };
        let mut out = vec![None; 64 * 64];
        generate_ground(std::slice::from_ref(&rock), &groups(), &f, (1.0, 1.0), [0, 0, 64, 64], &mut |x, y, p| {
            out[(y * 64 + x) as usize] = Some(p)
        });
        assert_eq!(out[32 * 64 + 50], Some([170, 170, 170, 255]), "rock's colour on the steep part");
        assert_eq!(out[32 * 64 + 10], None, "nothing on the flat");
        assert!(lays_ground(&[rock]) && !lays_ground(&default_rules()), "Default lays no ground type");
    }

    #[test]
    fn text_round_trip() {
        let mut set = RuleSet { preset: "Default".to_owned(), rules: default_rules() };
        set.rules[0].enabled = false;
        set.rules[1].height = Some(Range { lo: None, hi: Some(120.5), fade: 0.0 });
        set.rules[1].name = "Rock \"high\" & dry".to_owned();
        set.rules[1].water = Some(Range { lo: Some(2.0), hi: Some(30.0), fade: 4.0 });
        set.rules[1].road = Some(Range { lo: None, hi: Some(3.0), fade: 0.0 });
        set.rules[1].building = Some(Range { lo: Some(1.0), hi: None, fade: 2.0 });
        set.rules[1].ground = Some(GroundRule { kind: 5, threshold: 0.75 });
        let bytes = text(&set);
        assert_eq!(parse(&bytes).unwrap(), set, "{}", String::from_utf8_lossy(&bytes));
        let unnamed = RuleSet { preset: String::new(), rules: vec![] };
        assert_eq!(parse(&text(&unnamed)).unwrap(), unnamed);
        assert!(parse(b"<VEGETATION_SET/>").is_err());
    }

    #[test]
    fn presets_are_saved_read_and_listed() {
        let dir = tempfile::tempdir().unwrap();
        let root = presets_root(dir.path());
        let mut p = Preset::builtin();
        p.name = "dry".to_owned();
        p.path = Some(root.join("south").join("dry.wce"));
        p.rules.truncate(1);
        p.save().unwrap();
        let all = scan_presets(&root);
        assert_eq!(all.len(), 2);
        assert!(all[0].is_builtin());
        assert_eq!(all[1], Preset { folder: "south".to_owned(), ..p });
        assert!(Preset::builtin().save().is_err());
    }

    #[test]
    fn reach_of_the_hollows() {
        assert_eq!(reach(&[rock()], (1.0, 1.0)), 2.0);
        assert_eq!(reach(&default_rules(), (0.5, 0.5)), 12.5);
        assert!(
            default_rules()
                .iter()
                .all(|r| slot_of(&["grass_a".into(), "grass_b".into(), "mud_a".into(), "rock_b".into()], &r.group).is_some())
        );
    }
}
