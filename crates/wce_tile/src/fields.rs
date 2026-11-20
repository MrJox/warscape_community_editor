//! Fields and farmland (docs/plans/auto_detail_2.md, G): an area (a vegetation exclusion polygon, as a biome's: the game's own trees keep
//! off it) cut into strips, each a crop (the kit's field decals laid end to end along it, as its farm tiles lay them) or an orchard (rows
//! of trees), with haystacks and carts here and there, a fence round it and the ground under it worn to a texture. The same area, settings
//! and seed lay the same farmland.
//!
//! Everything here is in metres on the ground's plane (x, z); the editor turns it into decals and instances.

use std::fmt::Write as _;

/// The area's settings ([`FieldSettings::to_text`]) and its seed, on the polygon.
pub const FIELDS_ATTR: &str = "wce_fields";
pub const SEED_ATTR: &str = "wce_fields_seed";

/// What the farmland is made of.
#[derive(Clone, Debug, PartialEq)]
pub struct FieldSettings {
    /// How wide a strip is, and the verge between two (metres).
    pub strip_width: f32,
    pub verge: f32,

    /// Which way the strips run (degrees from +x towards +z); none: along the area's longest side.
    pub angle: Option<f32>,

    /// The crops' decals (a name under `rigidmodels\decals\battle`, as the decal rules'), one picked a strip, and how long a decal is.
    pub crops: Vec<String>,
    pub crop_length: f32,

    /// The share of the strips that are orchards (0 .. 1), their trees, how many rows a strip and how far apart along a row.
    pub orchard: f32,
    pub orchard_trees: Vec<String>,
    pub rows: u32,
    pub tree_spacing: f32,

    /// The fence round the area (building keys, laid end to end as a building spline's), none: no fence.
    pub fence: Vec<String>,

    /// Props here and there on the crops (haystacks, carts), how many a hectare.
    pub props: Vec<String>,
    pub props_per_ha: f32,

    /// The ground under the area: a texture group (several separated by `|`, the first the tile has), its variation and strength; an
    /// empty group: none.
    pub ground: String,
    pub ground_variation: f32,
    pub ground_strength: f32,
}

impl Default for FieldSettings {
    fn default() -> Self {
        let keys = |s: &str| s.split_whitespace().map(str::to_owned).collect::<Vec<_>>();
        Self {
            strip_width: 12.0,
            verge: 1.5,
            angle: None,
            crops: keys("global_fields_1 global_fields_2 global_fields_3 global_fields_4 global_fields_5 global_fields_6"),
            crop_length: 25.0,
            orchard: 0.2,
            orchard_trees: keys("olive.olive_a olive.olive_b olive.olive_c olive.olive_d"),
            rows: 2,
            tree_spacing: 6.0,
            fence: keys("lowfence_1 lowfence_2 lowfence_3"),
            props: keys("hay01 cart_1_hay"),
            props_per_ha: 4.0,
            ground: "mud|mud_a|mud_b|settlement_2".to_owned(),
            ground_variation: 1.0,
            ground_strength: 0.5,
        }
    }
}

impl FieldSettings {
    /// As one attribute's text: `key=value` pairs separated by `;`, lists by `,`.
    #[must_use]
    pub fn to_text(&self) -> String {
        let num = |v: f32| crate::ted_scene::format_number(f64::from(v));
        let mut t = String::new();
        let _ = write!(t, "strip={};verge={}", num(self.strip_width), num(self.verge));
        if let Some(a) = self.angle {
            let _ = write!(t, ";angle={}", num(a));
        }

        let _ = write!(
            t,
            ";crops={};crop_length={};orchard={};orchard_trees={};rows={};tree_spacing={};fence={};props={};props_ha={};ground={};ground_variation={};ground_strength={}",
            self.crops.join(","),
            num(self.crop_length),
            num(self.orchard),
            self.orchard_trees.join(","),
            self.rows,
            num(self.tree_spacing),
            self.fence.join(","),
            self.props.join(","),
            num(self.props_per_ha),
            self.ground,
            num(self.ground_variation),
            num(self.ground_strength),
        );
        t
    }

    /// From its text ([`Self::to_text`]): what is missing or wrong is the default's.
    #[must_use]
    pub fn from_text(text: &str) -> Self {
        let mut s = Self::default();
        let list = |v: &str| v.split(',').map(str::trim).filter(|k| !k.is_empty()).map(str::to_owned).collect::<Vec<_>>();
        for pair in text.split(';') {
            let Some((k, v)) = pair.split_once('=') else { continue };
            let v = v.trim();
            let f = v.parse::<f32>().ok().filter(|f| f.is_finite());
            match (k.trim(), f) {
                ("strip", Some(f)) => s.strip_width = f.clamp(1.0, 200.0),
                ("verge", Some(f)) => s.verge = f.clamp(0.0, 50.0),
                ("angle", Some(f)) => s.angle = Some(f),
                ("crops", _) => s.crops = list(v),
                ("crop_length", Some(f)) => s.crop_length = f.clamp(1.0, 200.0),
                ("orchard", Some(f)) => s.orchard = f.clamp(0.0, 1.0),
                ("orchard_trees", _) => s.orchard_trees = list(v),
                ("rows", Some(f)) => s.rows = (f as u32).clamp(1, 20),
                ("tree_spacing", Some(f)) => s.tree_spacing = f.clamp(1.0, 100.0),
                ("fence", _) => s.fence = list(v),
                ("props", _) => s.props = list(v),
                ("props_ha", Some(f)) => s.props_per_ha = f.clamp(0.0, 1000.0),
                ("ground", _) => s.ground = v.to_owned(),
                ("ground_variation", Some(f)) => s.ground_variation = f.clamp(0.0, 3.0),
                ("ground_strength", Some(f)) => s.ground_strength = f.clamp(0.0, 1.0),
                _ => {}
            }
        }

        s
    }
}

/// A crop's decal: its name (as the settings name it), where its middle is, which way its length runs (radians: its length along
/// (sin, cos) in x and z, as a decal rule's) and its size.
#[derive(Clone, Debug, PartialEq)]
pub struct CropDecal {
    pub key: String,
    pub x: f32,
    pub z: f32,
    pub yaw: f32,
    pub across: f32,
    pub length: f32,
}

/// A tree or a prop: its key, where, its turn (degrees) and its scale.
#[derive(Clone, Debug, PartialEq)]
pub struct Item {
    pub key: String,
    pub x: f32,
    pub z: f32,
    pub yaw: f32,
    pub scale: f32,
}

/// A strip: its middle line's ends (its pieces inside the area) and what it holds.
#[derive(Clone, Debug, PartialEq)]
pub struct Strip {
    pub pieces: Vec<[(f32, f32); 2]>,
    pub orchard: bool,
    pub crop: Option<String>,
}

/// What an area lays.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Farmland {
    pub strips: Vec<Strip>,
    pub decals: Vec<CropDecal>,
    pub trees: Vec<Item>,
    pub props: Vec<Item>,
}

/// The way the strips run for an area: the settings' angle, else its longest side's (radians from +x towards +z).
#[must_use]
pub fn strip_angle(poly: &[(f32, f32)], s: &FieldSettings) -> f32 {
    if let Some(a) = s.angle {
        return a.to_radians();
    }

    let n = poly.len();
    (0..n)
        .map(|i| (poly[i], poly[(i + 1) % n]))
        .max_by(|a, b| {
            let len = |(p, q): &((f32, f32), (f32, f32))| (q.0 - p.0).hypot(q.1 - p.1);
            len(a).total_cmp(&len(b))
        })
        .map_or(0.0, |(p, q)| (q.1 - p.1).atan2(q.0 - p.0))
}

/// The farmland of an area (`poly`: x and z, metres; either winding), from its settings and seed.
#[must_use]
pub fn lay(poly: &[(f32, f32)], s: &FieldSettings, seed: u32) -> Farmland {
    let mut out = Farmland::default();
    if poly.len() < 3 {
        return out;
    }

    // The area turned so that the strips run along +u, across +v.
    let a = strip_angle(poly, s);
    let (sin, cos) = a.sin_cos();
    let to_uv = |(x, z): (f32, f32)| (x * cos + z * sin, -x * sin + z * cos);
    let to_xz = |(u, v): (f32, f32)| (u * cos - v * sin, u * sin + v * cos);
    let uv = poly.iter().map(|&p| to_uv(p)).collect::<Vec<_>>();
    let (v0, v1) = uv.iter().fold((f32::MAX, f32::MIN), |(lo, hi), p| (lo.min(p.1), hi.max(p.1)));
    let width = s.strip_width.max(1.0);
    let pitch = width + s.verge.max(0.0);

    // The decals' length runs along the strip: along +u, which is (cos, sin) in x and z.
    let yaw = cos.atan2(sin);
    let mut rnd = Random::new(u64::from(seed));
    let mut v = v0 + s.verge.max(0.0) + width * 0.5;
    while v + width * 0.5 <= v1 + 1e-3 && out.strips.len() < 1000 {
        // The strip's pieces: where its middle and both its sides are all inside the area.
        let pieces = intersect(&intersect(&spans(&uv, v), &spans(&uv, v - width * 0.5 + 0.01)), &spans(&uv, v + width * 0.5 - 0.01))
            .into_iter()
            .filter(|(lo, hi)| hi - lo >= width.min(4.0))
            .collect::<Vec<_>>();
        let orchard = !s.orchard_trees.is_empty() && rnd.unit() < s.orchard;
        let crop = (!orchard && !s.crops.is_empty())
            .then(|| s.crops[((rnd.unit() * s.crops.len() as f32) as usize).min(s.crops.len() - 1)].clone());
        for &(lo, hi) in &pieces {
            if let Some(key) = &crop {
                let n = ((hi - lo) / s.crop_length.max(1.0)).ceil().max(1.0) as usize;
                let len = (hi - lo) / n as f32;
                for i in 0..n {
                    let (x, z) = to_xz((lo + (i as f32 + 0.5) * len, v));
                    out.decals.push(CropDecal { key: key.clone(), x, z, yaw, across: width, length: len });
                }

                // Props on the crops: here and there, by the area.
                let count = (hi - lo) * width / 10_000.0 * s.props_per_ha;
                let count = count.floor() as usize + usize::from(rnd.unit() < count.fract());
                for _ in 0..count {
                    if s.props.is_empty() {
                        break;
                    }

                    let (x, z) = to_xz((lo + rnd.unit() * (hi - lo), v + (rnd.unit() - 0.5) * width * 0.8));
                    let key = s.props[((rnd.unit() * s.props.len() as f32) as usize).min(s.props.len() - 1)].clone();
                    out.props.push(Item { key, x, z, yaw: rnd.unit() * 360.0, scale: 1.0 });
                }
            } else if orchard {
                let rows = s.rows.max(1);
                let spacing = s.tree_spacing.max(1.0);
                for r in 0..rows {
                    let rv = v - width * 0.5 + width * (r as f32 + 0.5) / rows as f32;
                    let mut u = lo + spacing * 0.5;
                    while u <= hi - spacing * 0.25 {
                        let jitter = (rnd.unit() - 0.5) * spacing * 0.15;
                        let (x, z) = to_xz((u + jitter, rv));
                        let key =
                            s.orchard_trees[((rnd.unit() * s.orchard_trees.len() as f32) as usize).min(s.orchard_trees.len() - 1)].clone();
                        out.trees.push(Item { key, x, z, yaw: rnd.unit() * 360.0, scale: 0.85 + rnd.unit() * 0.3 });
                        u += spacing;
                    }
                }
            }
        }

        out.strips.push(Strip { pieces: pieces.iter().map(|&(lo, hi)| [to_xz((lo, v)), to_xz((hi, v))]).collect(), orchard, crop });
        v += pitch;
    }

    out
}

/// Where a line across the area at `v` is inside it: its spans along u, sorted.
fn spans(uv: &[(f32, f32)], v: f32) -> Vec<(f32, f32)> {
    let n = uv.len();
    let mut cuts = vec![];
    for i in 0..n {
        let (p, q) = (uv[i], uv[(i + 1) % n]);
        // Half-open, so that a vertex on the line counts once.
        if (p.1 <= v) != (q.1 <= v) {
            cuts.push(p.0 + (v - p.1) / (q.1 - p.1) * (q.0 - p.0));
        }
    }

    cuts.sort_by(f32::total_cmp);
    cuts.chunks_exact(2).map(|c| (c[0], c[1])).collect()
}

/// Where two lists of spans both are.
fn intersect(a: &[(f32, f32)], b: &[(f32, f32)]) -> Vec<(f32, f32)> {
    let mut out = vec![];
    for &(a0, a1) in a {
        for &(b0, b1) in b {
            let (lo, hi) = (a0.max(b0), a1.min(b1));
            if hi > lo {
                out.push((lo, hi));
            }
        }
    }

    out
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

#[cfg(test)]
mod tests {
    use super::*;

    /// 100 m along x, 60 m along z.
    fn rect() -> Vec<(f32, f32)> {
        vec![(0.0, 0.0), (100.0, 0.0), (100.0, 60.0), (0.0, 60.0)]
    }

    #[test]
    fn strips_run_along_the_longest_side_and_stay_inside() {
        let s = FieldSettings { orchard: 0.0, ..FieldSettings::default() };
        let f = lay(&rect(), &s, 1);
        // 60 m across: strips of 12 m with 1.5 m verges, 4 of them.
        assert_eq!(f.strips.len(), 4);
        assert!(f.trees.is_empty());
        for d in &f.decals {
            assert!((d.yaw.sin().abs() - 1.0).abs() < 1e-4, "along x (either way): {}", d.yaw.to_degrees());
            assert!((d.across - 12.0).abs() < 1e-4 && d.length <= 25.0 + 1e-3);
            // The decal's box inside the area.
            assert!(d.x - d.length * 0.5 >= -1e-3 && d.x + d.length * 0.5 <= 100.0 + 1e-3);
            assert!(d.z - 6.0 >= -1e-3 && d.z + 6.0 <= 60.0 + 1e-3);
        }

        // Each strip is covered end to end: 100 m in 4 decals of 25 m.
        assert_eq!(f.decals.len(), 16);
        assert_eq!(lay(&rect(), &s, 1), f, "the same seed, the same farmland");
    }

    #[test]
    fn an_orchard_strip_has_rows_of_trees_and_a_concave_area_is_cut_around() {
        // An L: the strips stop at its inner corner.
        let l = vec![(0.0, 0.0), (100.0, 0.0), (100.0, 30.0), (40.0, 30.0), (40.0, 80.0), (0.0, 80.0)];
        let s = FieldSettings { orchard: 1.0, angle: Some(0.0), ..FieldSettings::default() };
        let f = lay(&l, &s, 3);
        assert!(f.decals.is_empty() && !f.trees.is_empty());
        for t in &f.trees {
            let inside_l = (t.x >= 0.0 && t.x <= 100.0 && t.z >= 0.0 && t.z <= 30.0) || (t.x >= 0.0 && t.x <= 40.0 && t.z <= 80.0);
            assert!(inside_l, "{t:?}");
        }

        // Two rows a strip: the trees of a strip at two distances across it.
        let first = &f.strips[0];
        let v = first.pieces[0][0].1;
        let rows = f
            .trees
            .iter()
            .filter(|t| (t.z - v).abs() < 6.0)
            .map(|t| (t.z * 10.0).round() as i32)
            .collect::<std::collections::BTreeSet<_>>();
        assert!(rows.len() >= 2, "{rows:?}");
    }

    #[test]
    fn settings_through_their_text() {
        let s = FieldSettings { angle: Some(30.0), rows: 3, fence: vec![], props_per_ha: 2.5, ..FieldSettings::default() };
        assert_eq!(FieldSettings::from_text(&s.to_text()), s);
        assert_eq!(FieldSettings::from_text("strip=banana;rows=4"), FieldSettings { rows: 4, ..FieldSettings::default() });
    }
}
