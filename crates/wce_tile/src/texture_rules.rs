//! Texture rules (docs/plans/auto_detail.md, A): where the ground's textures go, worked out from the terrain itself. A rule names a texture
//! (a texture group of the tile and a variation of it) and the terrain it goes on (ranges of height, slope and curvature, each with soft
//! edges); the rules of a tile are applied in order, a later one over an earlier one, as blend layers go over each other. What they make is
//! one blend layer's pixels: the rules' channels, covering what is below by their summed weight, and nothing where no rule holds.
//!
//! The rules are the editor's: TEd and the game never see them, only the blend map they paint. Their text, in a preset's file or in the
//! tile:
//!
//! ```text
//! <TEXTURE_RULES wce_version="1">
//!     <rule name="Rock on slopes" group="rock" variation="0" slope="35 -" slope_fade="10" breakup="25 0.5" seed="1"/>
//!     <rule name="Mud in hollows" group="mud" variation="1.5" hollow="0.4 -" hollow_radius="12" hollow_fade="0.3"/>
//! </TEXTURE_RULES>
//! ```
//!
//! A range is two numbers, `-` for an open end; its fade is how far its edges blend, centred on them. `enabled="0"` turns a rule off.

use wce_formats::xml_doc::XmlDoc;

use crate::height_field::{HeightField, fractal_noise};

/// The root element of a rule set's text.
pub const ROOT: &str = "TEXTURE_RULES";

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

    /// The texture group, by name (`rock`, `mud`, ...: the climates' `texture_set`s, as the tile's TEXTURE_GROUP names them).
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
            breakup: None,
            seed: 0,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Breakup {
    pub scale: f32,
    pub amount: f32,
}

/// The terrain the rules look at: the heights in metres (the editor's, the sea at 0), `w` x `h` samples `mpp` metres apart across and
/// down, from the map's corner (border included, as the blend map's).
pub struct Fields {
    w: u32,
    h: u32,
    mpp: (f64, f64),
    heights: Vec<f32>,

    /// The heights summed from the corner (a summed-area table, `w + 1` x `h + 1`): a box's mean in four reads, for the hollows.
    sums: Vec<f64>,
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

        Self { w, h, mpp, heights, sums }
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

/// How far past a changed place the rules' result can change (metres): the hollows' reach, a sample's for the slope.
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

fn slot_of(groups: &[String], name: &str) -> Option<usize> {
    groups.iter().take(4).position(|g| g.eq_ignore_ascii_case(name))
}

impl Rule {
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
pub fn text(rules: &[Rule]) -> Vec<u8> {
    let num = |v: f32| crate::ted_scene::format_number(f64::from(v));
    let mut out = format!("<{ROOT} {}=\"{}\">\n", crate::project_files::WCE_VERSION_ATTR, crate::project_files::WCE_VERSION);
    for r in rules {
        out += &format!("    <rule name=\"{}\" group=\"{}\" variation=\"{}\"", escape(&r.name), escape(&r.group), num(r.variation));
        if !r.enabled {
            out += " enabled=\"0\"";
        }

        if r.strength != 1.0 {
            out += &format!(" strength=\"{}\"", num(r.strength));
        }

        for (name, range) in [("height", &r.height), ("slope", &r.slope), ("hollow", &r.hollow)] {
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
pub fn parse(bytes: &[u8]) -> Result<Vec<Rule>, String> {
    let doc = XmlDoc::parse(bytes.to_vec()).map_err(|e| e.to_string())?;
    let els = doc.elements();
    if els.first().is_none_or(|e| e.name != ROOT) {
        return Err("not a texture rule set".to_owned());
    }

    let num = |e: usize, n: &str| doc.get(e, n).and_then(|v| v.trim().parse::<f32>().ok());
    let range = |e: usize, n: &str| doc.get(e, n).and_then(|v| Range::parse(&v, num(e, &format!("{n}_fade"))));
    Ok((0..els.len())
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
                hollow_radius: num(e, "hollow_radius").unwrap_or(d.hollow_radius),
                breakup: doc.get(e, "breakup").and_then(|v| {
                    let mut p = v.split_whitespace().map(str::parse::<f32>);
                    Some(Breakup { scale: p.next()?.ok()?, amount: p.next()?.ok()? })
                }),
                seed: doc.get(e, "seed").and_then(|v| v.trim().parse().ok()).unwrap_or(0),
            }
        })
        .collect())
}

/// The built-in set `Default`: rock on steep ground, mud in hollows. The groups are the kit's usual names; a tile without one lays nothing
/// of that rule.
#[must_use]
pub fn default_rules() -> Vec<Rule> {
    vec![
        Rule {
            name: "Mud in hollows".to_owned(),
            group: "mud".to_owned(),
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
            group: "rock".to_owned(),
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
        assert_eq!(missing_groups(&[faint, snow], &groups()).len(), 1);
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
    fn text_round_trip() {
        let mut rules = default_rules();
        rules[0].enabled = false;
        rules[1].height = Some(Range { lo: None, hi: Some(120.5), fade: 0.0 });
        rules[1].name = "Rock \"high\" & dry".to_owned();
        let bytes = text(&rules);
        assert_eq!(parse(&bytes).unwrap(), rules, "{}", String::from_utf8_lossy(&bytes));
        assert!(parse(b"<VEGETATION_SET/>").is_err());
    }

    #[test]
    fn reach_of_the_hollows() {
        assert_eq!(reach(&[rock()], (1.0, 1.0)), 2.0);
        assert_eq!(reach(&default_rules(), (0.5, 0.5)), 12.5);
    }
}
