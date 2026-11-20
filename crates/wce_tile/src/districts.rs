//! District filler (docs/plans/auto_detail_2.md, I): a district (a polygon) filled with buildings, first along the roads through it (each
//! set back from the road and turned to face it), then, when asked, inside (each facing the nearest road, else the district's longest
//! side). Every building's footprint stays inside the district, off the roads, off the other buildings (and its spacing from them) and
//! off ground steeper than the most slope. The same district, settings and seed give the same buildings.
//!
//! Everything here is in metres on the ground's plane (x, z). A building's front is its local +z (turned by `facing` when a kit's
//! buildings face another way).

use crate::building_spline::Footprint;

/// The district's settings and its seed, on the polygon.
pub const DISTRICT_ATTR: &str = "wce_district";
pub const SEED_ATTR: &str = "wce_district_seed";

/// How a district is filled.
#[derive(Clone, Debug, PartialEq)]
pub struct DistrictSettings {
    /// The buildings it is filled with (keys; each as likely).
    pub keys: Vec<String>,

    /// Metres from a road's edge to a building's front, and the least metres between two buildings.
    pub setback: f32,
    pub spacing: f32,

    /// Degrees: no building stands on steeper ground (across its footprint).
    pub max_slope: f32,

    /// Whether the inside is filled too (else only along the roads).
    pub inside: bool,

    /// Degrees a building is turned past facing the road (its front is its local +z).
    pub facing: f32,

    /// How far a building may be turned off facing its road, each way (degrees).
    pub jitter: f32,
}

impl Default for DistrictSettings {
    fn default() -> Self {
        Self { keys: vec![], setback: 1.0, spacing: 2.0, max_slope: 12.0, inside: true, facing: 0.0, jitter: 4.0 }
    }
}

impl DistrictSettings {
    /// As one attribute's text: `key=value` pairs separated by `;`, the keys by `,`.
    #[must_use]
    pub fn to_text(&self) -> String {
        let num = |v: f32| crate::ted_scene::format_number(f64::from(v));
        format!(
            "keys={};setback={};spacing={};max_slope={};inside={};facing={};jitter={}",
            self.keys.join(","),
            num(self.setback),
            num(self.spacing),
            num(self.max_slope),
            u8::from(self.inside),
            num(self.facing),
            num(self.jitter)
        )
    }

    /// From its text ([`Self::to_text`]): what is missing or wrong is the default's.
    #[must_use]
    pub fn from_text(text: &str) -> Self {
        let mut s = Self::default();
        for pair in text.split(';') {
            let Some((k, v)) = pair.split_once('=') else { continue };
            let v = v.trim();
            let f = v.parse::<f32>().ok().filter(|f| f.is_finite());
            match (k.trim(), f) {
                ("keys", _) => s.keys = v.split(',').map(str::trim).filter(|k| !k.is_empty()).map(str::to_owned).collect(),
                ("setback", Some(f)) => s.setback = f.clamp(0.0, 50.0),
                ("spacing", Some(f)) => s.spacing = f.clamp(0.0, 50.0),
                ("max_slope", Some(f)) => s.max_slope = f.clamp(0.0, 60.0),
                ("inside", _) => s.inside = v != "0",
                ("facing", Some(f)) => s.facing = f,
                ("jitter", Some(f)) => s.jitter = f.clamp(0.0, 45.0),
                _ => {}
            }
        }

        s
    }
}

/// A road through the district: its middle line (x, z) and half its width.
pub struct RoadLine {
    pub line: Vec<[f64; 2]>,
    pub half_width: f64,
}

/// A building placed: its key, its middle (its pivot), its turn (radians: its local +z along (sin, cos) in x and z, as a decal's) and
/// whether it faces a road.
#[derive(Clone, Debug, PartialEq)]
pub struct Lot {
    pub key: String,
    pub x: f64,
    pub z: f64,
    pub yaw: f64,
    pub on_road: bool,
}

/// The most buildings a district gets.
pub const MOST: usize = 600;

/// A footprint placed: its corners (x, z), counter-clockwise or not.
type Quad = [[f64; 2]; 4];

fn corners(f: &Footprint, x: f64, z: f64, yaw: f64) -> Quad {
    let (s, c) = yaw.sin_cos();
    let at = |lx: f64, lz: f64| [x + c * lx + s * lz, z - s * lx + c * lz];
    [at(f.x.0, f.z.0), at(f.x.1, f.z.0), at(f.x.1, f.z.1), at(f.x.0, f.z.1)]
}

/// Whether two convex quads overlap, `gap` apart at least (separating axes).
fn overlap(a: &Quad, b: &Quad, gap: f64) -> bool {
    for q in [a, b] {
        for i in 0..4 {
            let (p, r) = (q[i], q[(i + 1) % 4]);
            let (nx, nz) = (r[1] - p[1], p[0] - r[0]);
            let len = nx.hypot(nz);
            if len < 1e-9 {
                continue;
            }

            let (nx, nz) = (nx / len, nz / len);
            let span = |q: &Quad| q.iter().map(|v| v[0] * nx + v[1] * nz).fold((f64::MAX, f64::MIN), |(lo, hi), d| (lo.min(d), hi.max(d)));
            let ((a0, a1), (b0, b1)) = (span(a), span(b));
            if a1 + gap <= b0 || b1 + gap <= a0 {
                return false;
            }
        }
    }

    true
}

fn inside(poly: &[[f64; 2]], p: [f64; 2]) -> bool {
    let mut odd = false;
    let n = poly.len();
    for i in 0..n {
        let (a, b) = (poly[i], poly[(i + n - 1) % n]);
        if (a[1] > p[1]) != (b[1] > p[1]) && p[0] < (b[0] - a[0]) * (p[1] - a[1]) / (b[1] - a[1]) + a[0] {
            odd = !odd;
        }
    }

    odd
}

fn segment_distance(p: [f64; 2], a: [f64; 2], b: [f64; 2]) -> f64 {
    let (dx, dz) = (b[0] - a[0], b[1] - a[1]);
    let len2 = dx * dx + dz * dz;
    let t = if len2 > 0.0 { (((p[0] - a[0]) * dx + (p[1] - a[1]) * dz) / len2).clamp(0.0, 1.0) } else { 0.0 };
    (a[0] + t * dx - p[0]).hypot(a[1] + t * dz - p[1])
}

/// Points along a quad's sides, a metre apart at most, and its corners.
fn outline_points(q: &Quad) -> Vec<[f64; 2]> {
    let mut out = vec![];
    for i in 0..4 {
        let (a, b) = (q[i], q[(i + 1) % 4]);
        let n = ((b[0] - a[0]).hypot(b[1] - a[1])).ceil().max(1.0) as usize;
        for k in 0..n {
            let t = k as f64 / n as f64;
            out.push([a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t]);
        }
    }

    out
}

/// What a district is filled against.
pub struct Ground<'a> {
    pub roads: &'a [RoadLine],

    /// Buildings already there (x, z, radius).
    pub blockers: &'a [[f64; 3]],
    pub height: &'a dyn Fn(f64, f64) -> f64,

    /// A key's footprint about its pivot (metres; x across its front, z front to back).
    pub footprint: &'a dyn Fn(&str) -> Footprint,
}

/// Fills a district (`poly`: x and z, metres).
#[must_use]
pub fn fill(poly: &[[f64; 2]], g: &Ground, s: &DistrictSettings, seed: u32) -> Vec<Lot> {
    let mut lots: Vec<Lot> = vec![];
    if poly.len() < 3 || s.keys.is_empty() {
        return lots;
    }

    let mut rnd = Random::new(u64::from(seed));
    let mut placed: Vec<Quad> = vec![];
    let tan = f64::from(s.max_slope.clamp(0.0, 89.0)).to_radians().tan();
    let spacing = f64::from(s.spacing.max(0.0));
    let fits = |q: &Quad, placed: &[Quad]| {
        let pts = outline_points(q);
        if !pts.iter().all(|&p| inside(poly, p)) {
            return false;
        }

        // Off the roads (their half width, and a little).
        if g.roads.iter().any(|r| r.line.windows(2).any(|w| pts.iter().any(|&p| segment_distance(p, w[0], w[1]) < r.half_width + 0.5))) {
            return false;
        }

        // Off the buildings there and those placed.
        let mid = [(q[0][0] + q[2][0]) * 0.5, (q[0][1] + q[2][1]) * 0.5];
        let reach = (q[0][0] - q[2][0]).hypot(q[0][1] - q[2][1]) * 0.5;
        if g.blockers.iter().any(|&[bx, bz, r]| {
            (bx - mid[0]).hypot(bz - mid[1]) < reach + r + spacing && pts.iter().any(|p| (p[0] - bx).hypot(p[1] - bz) < r + spacing)
        }) {
            return false;
        }

        if placed.iter().any(|o| overlap(q, o, spacing)) {
            return false;
        }

        // Ground flat enough across it.
        let hs = q.iter().map(|c| (g.height)(c[0], c[1])).collect::<Vec<_>>();
        let (lo, hi) = hs.iter().fold((f64::MAX, f64::MIN), |(lo, hi), &h| (lo.min(h), hi.max(h)));
        let across = (q[0][0] - q[2][0]).hypot(q[0][1] - q[2][1]).max(1.0);
        hi - lo <= tan * across
    };
    let pick = |rnd: &mut Random| s.keys[((rnd.unit() * s.keys.len() as f64) as usize).min(s.keys.len() - 1)].clone();
    let facing = f64::from(s.facing).to_radians();
    let jitter = |rnd: &mut Random| (rnd.unit() - 0.5) * 2.0 * f64::from(s.jitter).to_radians();

    // 1. Along the roads: each side of each road, walked from one end; a building's front at the road's edge plus the setback.
    for road in g.roads {
        let line = &road.line;
        if line.len() < 2 {
            continue;
        }

        let mut at = vec![0.0];
        for w in line.windows(2) {
            let l = at.last().copied().unwrap_or(0.0) + (w[1][0] - w[0][0]).hypot(w[1][1] - w[0][1]);
            at.push(l);
        }

        let total = *at.last().unwrap_or(&0.0);
        let point = |d: f64| {
            let k = at.partition_point(|&v| v <= d).clamp(1, line.len() - 1);
            let (a, b) = (line[k - 1], line[k]);
            let seg = (at[k] - at[k - 1]).max(1e-9);
            let t = ((d - at[k - 1]) / seg).clamp(0.0, 1.0);
            let (tx, tz) = ((b[0] - a[0]) / seg, (b[1] - a[1]) / seg);
            ([a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t], [tx, tz])
        };

        for side in [1.0f64, -1.0] {
            let mut d = 0.0;
            while d < total && lots.len() < MOST {
                let key = pick(&mut rnd);
                let f = (g.footprint)(&key);
                let width = (f.x.1 - f.x.0).max(1.0);
                let (p, t) = point((d + width * 0.5).min(total));
                // Out of the road, on this side; the building's +z (its front) back towards it.
                let n = [-t[1] * side, t[0] * side];
                let edge = road.half_width + f64::from(s.setback);
                let c = [p[0] + n[0] * (edge + f.z.1), p[1] + n[1] * (edge + f.z.1)];
                let yaw = (-n[0]).atan2(-n[1]) + facing + jitter(&mut rnd);
                // Its pivot is not its footprint's middle across: moved so that the footprint is centred on the point.
                let cx = c[0] - (f.x.0 + f.x.1) * 0.5 * yaw.cos();
                let cz = c[1] + (f.x.0 + f.x.1) * 0.5 * yaw.sin();
                let q = corners(&f, cx, cz, yaw);
                if fits(&q, &placed) {
                    placed.push(q);
                    lots.push(Lot { key, x: cx, z: cz, yaw, on_road: true });
                    d += width + spacing;
                } else {
                    d += 2.0;
                }
            }
        }
    }

    // 2. Inside: tried at random, each facing the nearest road near enough, else along the district's longest side.
    if s.inside {
        let n = poly.len();
        let longest = (0..n)
            .map(|i| (poly[i], poly[(i + 1) % n]))
            .max_by(|a, b| {
                let len = |(p, q): &([f64; 2], [f64; 2])| (q[0] - p[0]).hypot(q[1] - p[1]);
                len(a).total_cmp(&len(b))
            })
            .map_or(0.0, |(p, q)| (q[0] - p[0]).atan2(q[1] - p[1]));
        let (x0, x1) = poly.iter().fold((f64::MAX, f64::MIN), |(lo, hi), p| (lo.min(p[0]), hi.max(p[0])));
        let (z0, z1) = poly.iter().fold((f64::MAX, f64::MIN), |(lo, hi), p| (lo.min(p[1]), hi.max(p[1])));
        let tries = (((x1 - x0) * (z1 - z0) / 10.0) as usize).clamp(50, 20_000);
        for _ in 0..tries {
            if lots.len() >= MOST {
                break;
            }

            let p = [x0 + rnd.unit() * (x1 - x0), z0 + rnd.unit() * (z1 - z0)];
            let key = pick(&mut rnd);
            if !inside(poly, p) {
                continue;
            }

            let near = g
                .roads
                .iter()
                .flat_map(|r| r.line.windows(2))
                .map(|w| {
                    let (dx, dz) = (w[1][0] - w[0][0], w[1][1] - w[0][1]);
                    let len2 = dx * dx + dz * dz;
                    let t = if len2 > 0.0 { (((p[0] - w[0][0]) * dx + (p[1] - w[0][1]) * dz) / len2).clamp(0.0, 1.0) } else { 0.0 };
                    [w[0][0] + t * dx, w[0][1] + t * dz]
                })
                .min_by(|a, b| (a[0] - p[0]).hypot(a[1] - p[1]).total_cmp(&(b[0] - p[0]).hypot(b[1] - p[1])))
                .filter(|q| (q[0] - p[0]).hypot(q[1] - p[1]) < 80.0);
            let yaw = match near {
                Some(q) => (q[0] - p[0]).atan2(q[1] - p[1]),
                None => longest + if rnd.unit() < 0.5 { 0.0 } else { std::f64::consts::PI },
            } + facing
                + jitter(&mut rnd);
            let f = (g.footprint)(&key);
            let q = corners(&f, p[0], p[1], yaw);
            if fits(&q, &placed) {
                placed.push(q);
                lots.push(Lot { key, x: p[0], z: p[1], yaw, on_road: false });
            }
        }
    }

    lots
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

    fn unit(&mut self) -> f64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        (self.0 >> 11) as f64 / (1u64 << 53) as f64
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn flat(_: f64, _: f64) -> f64 {
        0.0
    }

    /// 10 m across its front, 8 m deep, its pivot in its middle.
    fn house(_: &str) -> Footprint {
        Footprint { x: (-5.0, 5.0), z: (-4.0, 4.0) }
    }

    fn square() -> Vec<[f64; 2]> {
        vec![[0.0, 0.0], [100.0, 0.0], [100.0, 100.0], [0.0, 100.0]]
    }

    fn settings() -> DistrictSettings {
        DistrictSettings { keys: vec!["a".to_owned(), "b".to_owned()], jitter: 0.0, ..DistrictSettings::default() }
    }

    #[test]
    fn houses_line_a_road_facing_it_and_fill_the_inside() {
        // A road along z = 50, 6 m wide.
        let roads = [RoadLine { line: vec![[-10.0, 50.0], [110.0, 50.0]], half_width: 3.0 }];
        let g = Ground { roads: &roads, blockers: &[], height: &flat, footprint: &house };
        let lots = fill(&square(), &g, &settings(), 1);
        let on_road = lots.iter().filter(|l| l.on_road).collect::<Vec<_>>();
        // Each side about 100 / (10 + 2) houses.
        assert!(on_road.len() >= 12, "{}", on_road.len());
        for l in &on_road {
            // Its front 4 m from its middle at the road's edge plus the setback: its middle 3 + 1 + 4 = 8 m off the road's middle.
            assert!(((l.z - 50.0).abs() - 8.0).abs() < 0.01, "{l:?}");
            // Facing the road: its +z towards z = 50.
            let towards = if l.z > 50.0 { -1.0 } else { 1.0 };
            assert!((l.yaw.cos() - towards).abs() < 1e-6, "{l:?}");
        }

        assert!(lots.iter().any(|l| !l.on_road), "the inside too");
        // None overlapping, none on the road, all inside.
        let quads = lots.iter().map(|l| corners(&house(""), l.x, l.z, l.yaw)).collect::<Vec<_>>();
        for (i, a) in quads.iter().enumerate() {
            assert!(a.iter().all(|&c| inside(&square(), c)));
            assert!(a.iter().all(|c| (c[1] - 50.0).abs() >= 3.0));
            for b in &quads[i + 1..] {
                assert!(!overlap(a, b, 1.9), "spaced");
            }
        }

        assert_eq!(fill(&square(), &g, &settings(), 1), lots, "the same seed, the same district");
    }

    #[test]
    fn steep_ground_and_buildings_there_are_kept_off() {
        let steep = |x: f64, _: f64| if x > 50.0 { x * 0.5 } else { 0.0 };
        let blockers = [[25.0, 25.0, 10.0]];
        let g = Ground { roads: &[], blockers: &blockers, height: &steep, footprint: &house };
        let lots = fill(&square(), &g, &settings(), 2);
        assert!(!lots.is_empty());
        for l in &lots {
            assert!(l.x < 56.0, "not on the slope: {l:?}");
            assert!((l.x - 25.0).hypot(l.z - 25.0) > 10.0 + 2.0, "off the building there: {l:?}");
        }
    }

    #[test]
    fn settings_through_their_text() {
        let s = DistrictSettings { keys: vec!["x_house".to_owned()], inside: false, facing: 180.0, ..DistrictSettings::default() };
        assert_eq!(DistrictSettings::from_text(&s.to_text()), s);
    }
}
