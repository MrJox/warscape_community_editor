//! Laying a road's pieces (docs/plans/procedural_roads.md, "Laying a road out" and "Fitting a piece to the road"): where each piece of a
//! road type goes on the road's centre line ([`layout`]), and the piece bent to it there by overrides of its parts ([`fit`]).
//!
//! World units (metres) along the road; a piece is in its own metres (`road_pieces`), a placed piece's text in the tile's file units.

use wce_foundation::math::{Double3, Double3x3};

use crate::prefab_refs::{Override, apply};
use crate::new_instances::ROAD_GUIDE;
use crate::road_pieces::{Anchor, Part, Piece};
use crate::ted_scene::{TedScene, format_number};
use crate::world_transform::{euler_from_matrix, euler_matrix};

/// How far a straight bends at most over its length before a curve takes its place (radians), and the most a curve bends.
pub const STRAIGHT_BEND: f64 = 15.0 * std::f64::consts::PI / 180.0;
pub const CURVE_BEND: f64 = std::f64::consts::FRAC_PI_2;

/// A stretch shorter than this between two features is left bare (metres).
const MIN_STRETCH: f64 = 0.5;

/// A line's distances, for finding places on it.
pub struct Stations<'a> {
    line: &'a [[f64; 2]],
    along: Vec<f64>,
}

impl<'a> Stations<'a> {
    #[must_use]
    pub fn new(line: &'a [[f64; 2]]) -> Self {
        let mut along = Vec::with_capacity(line.len());
        let mut d = 0.0;
        for (i, p) in line.iter().enumerate() {
            if i > 0 {
                d += (p[0] - line[i - 1][0]).hypot(p[1] - line[i - 1][1]);
            }

            along.push(d);
        }

        Self { line, along }
    }

    #[must_use]
    pub fn length(&self) -> f64 {
        self.along.last().copied().unwrap_or(0.0)
    }

    /// How far along the line a sample is.
    #[must_use]
    pub fn of_sample(&self, i: usize) -> f64 {
        self.along.get(i).copied().unwrap_or_else(|| self.length())
    }

    /// The point at a distance along the line and where the line goes there (a unit vector).
    #[must_use]
    pub fn at(&self, s: f64) -> ([f64; 2], [f64; 2]) {
        let i = self.along.partition_point(|&d| d <= s).clamp(1, self.line.len().max(2) - 1);
        let (a, b) = (self.line[i - 1], self.line[i]);
        let span = (self.along[i] - self.along[i - 1]).max(1e-9);
        let f = ((s - self.along[i - 1]) / span).clamp(0.0, 1.0);
        let len = (b[0] - a[0]).hypot(b[1] - a[1]).max(1e-9);
        ([a[0] + (b[0] - a[0]) * f, a[1] + (b[1] - a[1]) * f], [(b[0] - a[0]) / len, (b[1] - a[1]) / len])
    }
}

/// How a road's end is laid.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum End {
    /// It stops: an end piece.
    Free,

    /// On the tile's edge: it runs on into the next tile, nothing there.
    Edge,

    /// On another road: that road's junction takes it.
    Joined,
}

/// A stretch of a road's line something else takes (distances along it): a piece of its own (a junction), or nothing (a bridge, another
/// road's junction).
#[derive(Clone, Debug, PartialEq)]
pub struct Hold {
    pub from: f64,
    pub to: f64,
    pub laid: Option<Laid>,
}

/// Where a laid piece is.
#[derive(Clone, Debug, PartialEq)]
pub enum Frame {
    /// Its arm along the road, from a distance along the centre line to another (the other way when `to` is less).
    Along { from: f64, to: f64 },

    /// Its arms out of a point, each along a way (unit vectors), in the order of the piece's arms.
    Star { at: [f64; 2], ways: Vec<[f64; 2]> },
}

/// A piece of a road type, laid.
#[derive(Clone, Debug, PartialEq)]
pub struct Laid {
    pub piece: &'static str,
    pub frame: Frame,
}

/// What [`layout`] needs of a road type's pieces: their lengths (metres, the pieces' own).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Measures {
    pub straight: f64,
    pub end: f64,

    /// From a corner's middle to its ports.
    pub corner_arm: f64,
}

impl Default for Measures {
    fn default() -> Self {
        Self { straight: crate::road_pieces::STRAIGHT, end: crate::road_pieces::END, corner_arm: crate::road_pieces::JUNCTION * 0.5 }
    }
}

/// A road's pieces.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Layout {
    pub laid: Vec<Laid>,

    /// The stretches of the line something other than a straight or a curve took (bridges, junctions, corners, fords, ends), as distances
    /// along it, in order: where a road's fences stop.
    pub held: Vec<(f64, f64)>,

    /// What could not be laid as asked.
    pub notes: Vec<String>,
}

fn unit(v: [f64; 2]) -> [f64; 2] {
    let l = v[0].hypot(v[1]).max(1e-12);
    [v[0] / l, v[1] / l]
}

fn angle_between(a: [f64; 2], b: [f64; 2]) -> f64 {
    (a[0] * b[1] - a[1] * b[0]).atan2(a[0] * b[0] + a[1] * b[1])
}

fn pick(seed: u32, n: u32) -> u32 {
    let mut h = u64::from(seed).wrapping_mul(0x9E37_79B9_7F4A_7C15) ^ u64::from(n).wrapping_mul(0xC2B2_AE3D_27D4_EB4F);
    h ^= h >> 31;
    (h.wrapping_mul(0xBF58_476D_1CE4_E5B9) >> 40) as u32
}

/// Where the line is `reach` from a point on it, in a straight line, going from `c` (a distance along it) one way (`reach` < 0: back).
fn chord(st: &Stations, p: [f64; 2], c: f64, reach: f64, total: f64) -> Option<f64> {
    let away = |s: f64| { let q = st.at(s).0; (q[0] - p[0]).hypot(q[1] - p[1]) };
    let far = (c + reach * 3.0).clamp(0.0, total);
    if away(far) < reach.abs() {
        return None;
    }

    let (mut lo, mut hi) = (c, far);
    for _ in 0..60 {
        let m = (lo + hi) * 0.5;
        if away(m) < reach.abs() { lo = m } else { hi = m }
    }

    Some((lo + hi) * 0.5)
}

/// Lays a road's pieces on its centre line (`roads::centre_line_sharp`): a corner on each of `corners` (the samples of its sharp
/// points), an end piece on each free end, and straights and curves between, `round(length / straight)` to a stretch, each bent less
/// than [`STRAIGHT_BEND`] a straight. `holds`: what takes stretches of the line first (bridges, junctions: [`junctions`]). `scale`: the
/// road's width over the pieces': a corner grows with it. The seed turns straights and curves end for end.
#[must_use]
pub fn layout(line: &[[f64; 2]], corners: &[usize], ends: [End; 2], holds: &[Hold], measures: &Measures, scale: f64, seed: u32) -> Layout {
    let mut out = Layout::default();
    let st = Stations::new(line);
    let total = st.length();
    if line.len() < 2 || total <= 0.0 {
        return out;
    }

    // What takes the line first: bridges and junctions, corners, ends; each as the stretch it holds.
    let mut held: Vec<(f64, f64, Option<Laid>)> = holds.iter().map(|h| (h.from.max(0.0), h.to.min(total), h.laid.clone())).collect();
    let overlaps = |held: &[(f64, f64, Option<Laid>)], a: f64, b: f64| held.iter().any(|h| a < h.1 && b > h.0);
    let half = measures.corner_arm * scale;
    for &i in corners {
        let c = st.of_sample(i);
        let at = line[i.min(line.len() - 1)];
        // Its ports, as far from its middle as its arms are long, in a straight line: the line may bend a little near the corner.
        let (Some(a), Some(b)) = (chord(&st, at, c, -half, total), chord(&st, at, c, half, total)) else {
            out.notes.push(format!("a corner at {:.0}, {:.0} has no room for its piece ({:.0} m each way): laid as a bend", at[0], at[1], half));
            continue;
        };

        if overlaps(&held, a, b) {
            out.notes.push(format!("a corner at {:.0}, {:.0} has no room for its piece ({:.0} m each way): laid as a bend", at[0], at[1], half));
            continue;
        }

        let ways = vec![unit([st.at(a).0[0] - at[0], st.at(a).0[1] - at[1]]), unit([st.at(b).0[0] - at[0], st.at(b).0[1] - at[1]])];
        held.push((a, b, Some(Laid { piece: "corner", frame: Frame::Star { at, ways } })));
    }

    let end = measures.end;
    // A joined end is its junction's (a hold); an end on the tile's edge has nothing.
    if ends[0] == End::Free && !overlaps(&held, 0.0, end) && end < total {
        held.push((0.0, end, Some(Laid { piece: "end", frame: Frame::Along { from: 0.0, to: end } })));
    }

    if ends[1] == End::Free && !overlaps(&held, total - end, total) && end < total {
        held.push((total - end, total, Some(Laid { piece: "end", frame: Frame::Along { from: total, to: total - end } })));
    }

    held.sort_by(|a, b| a.0.total_cmp(&b.0));
    out.held = held.iter().map(|h| (h.0, h.1)).collect();

    // The stretches between: straights, or curves where a straight would bend too much.
    let mut stretches = vec![];
    let mut from = 0.0;
    for h in &held {
        if h.0 > from {
            stretches.push((from, h.0));
        }

        from = from.max(h.1);
    }

    if from < total {
        stretches.push((from, total));
    }

    let mut n_laid = 0u32;
    let mut laid = held.into_iter().filter_map(|h| h.2.map(|l| (h.0, l))).collect::<Vec<_>>();
    for (a, b) in stretches {
        let length = b - a;
        if length < MIN_STRETCH {
            continue;
        }

        let n = ((length / measures.straight).round() as usize).max(1);
        for k in 0..n {
            let (s0, s1) = (a + length * k as f64 / n as f64, a + length * (k + 1) as f64 / n as f64);
            let bend = angle_between(st.at(s0 + 1e-6).1, st.at(s1 - 1e-6).1).abs();
            let piece = if bend > STRAIGHT_BEND { "curve" } else { "straight" };
            if bend > CURVE_BEND {
                let p = st.at((s0 + s1) * 0.5).0;
                out.notes.push(format!("the road turns {:.0} degrees in {:.1} m at {:.0}, {:.0}: tighter than a curve piece takes", bend.to_degrees(), s1 - s0, p[0], p[1]));
            }

            let frame = if pick(seed, n_laid) & 1 == 1 { Frame::Along { from: s1, to: s0 } } else { Frame::Along { from: s0, to: s1 } };
            n_laid += 1;
            laid.push((s0, Laid { piece, frame }));
        }
    }

    laid.sort_by(|a, b| a.0.total_cmp(&b.0));
    out.laid = laid.into_iter().map(|l| l.1).collect();
    out
}

/// A piece fitted to the road: its group's text, and where its group is (the file's units).
#[derive(Clone, Debug, PartialEq)]
pub struct Fitted {
    pub text: Vec<u8>,
    pub position: [f64; 3],

    /// The overrides that bent it, as a reference to its prefab would keep them.
    pub overrides: Vec<Override>,
}

/// The road a piece is fitted to: its centre line, its width (metres), the tile's file units a metre, the terrain's height (metres).
pub struct Site<'a> {
    pub line: &'a [[f64; 2]],
    pub width: f64,
    pub unit: f64,
    pub ground: &'a dyn Fn(f64, f64) -> f64,
}

/// Where a part goes and which way its x points (world metres, x and z).
fn place(piece: &Piece, part: &Part, frame: &Frame, st: &Stations, width: f64) -> ([f64; 2], [f64; 2]) {
    let pw = piece.width.max(1e-6);
    let across = match part.anchor {
        Anchor::Surface | Anchor::Fixed => part.across * width / pw,
        Anchor::Edge | Anchor::Verge => part.across + part.across.signum() * (width - pw) * 0.5,
    };

    let dir = |t: [f64; 2], n: [f64; 2]| [t[0] * part.turn.cos() + n[0] * part.turn.sin(), t[1] * part.turn.cos() + n[1] * part.turn.sin()];
    match frame {
        Frame::Along { from, to } => {
            let s = from + part.along * (to - from);
            let (p, t) = st.at(s);
            let t = if to >= from { t } else { [-t[0], -t[1]] };
            let n = [t[1], -t[0]];
            ([p[0] + n[0] * across, p[1] + n[1] * across], dir(t, n))
        }

        Frame::Star { at, ways } => {
            let scale = width / pw;
            let arm = &piece.arms[part.arm];
            if part.anchor == Anchor::Fixed {
                // Turned with the first arm, as the piece is.
                let a0 = arm_way(&piece.arms[0]);
                let r = angle_between(a0, ways[0]);
                let (c, s) = (r.cos(), r.sin());
                let rot = |v: [f64; 2]| [v[0] * c - v[1] * s, v[0] * s + v[1] * c];
                let q = rot([part.at[0] * scale, part.at[1] * scale]);
                let x = part.rotation * Double3::new(1.0, 0.0, 0.0);
                return ([at[0] + q[0], at[1] + q[1]], rot(unit([x.x, x.z])));
            }

            let t = ways[part.arm.min(ways.len() - 1)];
            let n = [t[1], -t[0]];
            let along = part.along * arm.length * scale;
            ([at[0] + t[0] * along + n[0] * across, at[1] + t[1] * along + n[1] * across], dir(t, n))
        }
    }
}

/// The way an arm runs: from its centre line's start to its end.
fn arm_way(arm: &crate::road_pieces::Arm) -> [f64; 2] {
    let (a, b) = (arm.centre[0], arm.centre[arm.centre.len() - 1]);
    unit([b[0] - a[0], b[1] - a[1]])
}

/// Every order of `n` things.
fn orders(n: usize) -> Vec<Vec<usize>> {
    if n == 0 {
        return vec![vec![]];
    }

    let mut out = vec![];
    for rest in orders(n - 1) {
        for at in 0..=rest.len() {
            let mut o = rest.clone();
            o.insert(at, n - 1);
            out.push(o);
        }
    }

    out
}

/// A star's ways put in the order of the piece's arms: the order whose angles between ways are nearest the piece's between arms, turning
/// the same way. So a right-turning corner laid on a left turn takes its arms the other way round, and a T junction drawn with its branch
/// on the right, laid where the branch is on the left, takes its through arms swapped: its outer side stays outside.
fn arranged(piece: &Piece, frame: &Frame) -> Frame {
    let Frame::Star { at, ways } = frame else { return frame.clone() };
    let n = piece.arms.len();
    if ways.len() != n || n > 5 {
        return frame.clone();
    }

    let arms = piece.arms.iter().map(arm_way).collect::<Vec<_>>();
    let wrap = |a: f64| (a + std::f64::consts::PI).rem_euclid(std::f64::consts::TAU) - std::f64::consts::PI;
    let cost = |o: &[usize]| {
        let mut c = 0.0;
        for i in 0..n {
            for j in i + 1..n {
                c += wrap(angle_between(arms[i], arms[j]) - angle_between(ways[o[i]], ways[o[j]])).abs();
            }
        }

        c
    };

    let best = orders(n).into_iter().min_by(|a, b| cost(a).total_cmp(&cost(b))).unwrap_or_else(|| (0..n).collect());
    Frame::Star { at: *at, ways: best.iter().map(|&i| ways[i]).collect() }
}

/// The heading of a direction on the ground as the files turn it (`euler_matrix`: Ry(a) takes +x to (cos a, 0, -sin a)).
fn heading(v: [f64; 2]) -> f64 {
    (-v[1]).atan2(v[0])
}

/// Fits a piece to the road where it is laid: each part moved and turned to its place, stretched along with the piece and, on the road,
/// across with the width, at the ground's height there, as overrides of its prefab's contents (`children`, the prefab's `children_text`).
/// The guides are left out of the text.
///
/// # Errors
///
/// The prefab's contents do not read.
pub fn fit(piece: &Piece, children: &[u8], key: &str, laid: &Laid, site: &Site) -> Result<Fitted, String> {
    fit_dressed(piece, children, key, laid, site, &Dress::default())
}

/// What a piece's parts get besides their place: buildings tilted to the ground (`tilt`: a fence follows the slope it stands on), and
/// attributes of their own (`attrs`, by the part's index in the piece: a fence's `destruction_level`).
#[derive(Default)]
pub struct Dress<'a> {
    pub tilt: bool,
    pub attrs: Option<&'a dyn Fn(usize, &Part) -> Vec<(String, String)>>,
}

/// How far along and across a building the ground is read to tilt it (metres): a fence's ends, its width.
const TILT_ALONG: f64 = 2.0;
const TILT_ACROSS: f64 = 0.5;

/// The turn that takes up (0, 1, 0) to `n` (a unit vector, its y over 0) by the shortest way.
fn up_to(n: Double3) -> Double3x3 {
    let (x, z) = (n.z, -n.x);
    let s = x.hypot(z);
    if s < 1e-9 {
        return Double3x3::default();
    }

    let (kx, kz) = (x / s, z / s);
    let (c, sn) = (n.y.clamp(-1.0, 1.0), s);
    let t = 1.0 - c;
    Double3x3 { m: [[c + t * kx * kx, -sn * kz, t * kx * kz], [sn * kz, c, -sn * kx], [t * kx * kz, sn * kx, c + t * kz * kz]] }
}

/// [`fit`], its parts dressed as `dress` says.
///
/// # Errors
///
/// The prefab's contents do not read.
pub fn fit_dressed(piece: &Piece, children: &[u8], key: &str, laid: &Laid, site: &Site, dress: &Dress) -> Result<Fitted, String> {
    let Site { line, width, unit, ground } = *site;
    let st = Stations::new(line);
    let frame = arranged(piece, &laid.frame);

    let origin = match &frame {
        Frame::Along { from, to } => st.at((from + to) * 0.5).0,
        Frame::Star { at, .. } => *at,
    };

    // How much longer the piece is laid than drawn.
    let stretch = match &frame {
        Frame::Along { from, to } => (to - from).abs() / piece.arms.first().map_or(1.0, |a| a.length).max(1e-6),
        Frame::Star { .. } => width / piece.width.max(1e-6),
    };

    let n = format_number;
    let mut ovs = vec![];
    for (index, part) in piece.parts.iter().enumerate() {
        let (at, x) = place(piece, part, &frame, &st, width);
        let height = ground(at[0], at[1]);
        let own_x = part.rotation * Double3::new(1.0, 0.0, 0.0);
        let turn = heading(x) - heading([own_x.x, own_x.z]);
        let mut world_rot = euler_matrix(Double3::new(turn, 0.0, 0.0)) * part.rotation;

        // A building tilted to the ground under it: its slope along it (end to end) and across.
        if dress.tilt && part.solid {
            let c = [-x[1], x[0]];
            let h = |d: [f64; 2], k: f64| ground(at[0] + d[0] * k, at[1] + d[1] * k);
            let a = Double3::new(x[0] * 2.0 * TILT_ALONG, h(x, TILT_ALONG) - h(x, -TILT_ALONG), x[1] * 2.0 * TILT_ALONG);
            let b = Double3::new(c[0] * 2.0 * TILT_ACROSS, h(c, TILT_ACROSS) - h(c, -TILT_ACROSS), c[1] * 2.0 * TILT_ACROSS);
            let cross = Double3::new(a.y * b.z - a.z * b.y, a.z * b.x - a.x * b.z, a.x * b.y - a.y * b.x);
            let len = (cross.x * cross.x + cross.y * cross.y + cross.z * cross.z).sqrt().max(1e-12);
            let up = if cross.y < 0.0 { cross * (-1.0 / len) } else { cross * (1.0 / len) };
            world_rot = up_to(up) * world_rot;
        }

        let world_pos = Double3::new((at[0] - origin[0]) * unit, height * unit, (at[1] - origin[1]) * unit);
        let (pos, rot) = match part.parent {
            Some((pp, pr)) => (pr.transpose() * (world_pos - pp * unit), pr.transpose() * world_rot),
            None => (world_pos, world_rot),
        };

        let e = euler_from_matrix(&rot);
        let mut set = |attr: &str, value: String| ovs.push(Override { path: part.path.clone(), element: part.kind.clone(), attr: attr.to_owned(), value });
        set("position", format!("vector({},{},{})", n(pos.x), n(pos.y), n(pos.z)));
        set("orientation", format!("euler({},{},{})", n(e.x), n(e.y), n(e.z)));
        set("surface_mode", "MSM_FREEFORM".to_owned());
        set("lf_offset", n(height * unit));
        if let Some(attrs) = dress.attrs {
            for (a, v) in attrs(index, part) {
                set(&a, v);
            }
        }

        if let Some(s) = part.scale.filter(|_| part.kind == "ProjectedDecalInstance") {
            // Its x runs along the road, or its z does.
            let x_along = part.turn.cos().abs() >= part.turn.sin().abs();
            let across = if part.anchor == Anchor::Surface { width / piece.width.max(1e-6) } else { 1.0 };
            let (sx, sz) = if x_along { (s.x * stretch, s.z * across) } else { (s.x * across, s.z * stretch) };
            set("scale", format!("vector({},{},{})", n(sx), n(s.y), n(sz)));
        }
    }

    let (filled, _) = apply(children, &ovs)?;
    let position = [origin[0] * unit, 0.0, origin[1] * unit];
    let mut text = format!(
        "<UserInstanceGroup flags=\" OIF_VISIBLE|OIF_ACTIVE\" lf_offset=\"0\" position=\"vector({},0,{})\" name=\"{key}\" surface_mode=\"MSM_FREEFORM\" orientation=\"euler(0,0,0)\">\n    <children>\n",
        n(position[0]), n(position[2])
    ).into_bytes();
    for l in filled.split_inclusive(|&c| c == b'\n') {
        text.extend_from_slice(b"        ");
        text.extend_from_slice(l.trim_ascii_start());
    }

    if !text.ends_with(b"\n") {
        text.push(b'\n');
    }

    text.extend_from_slice(b"    </children>\n</UserInstanceGroup>\n");

    // Without its guides: they are the piece's drawing, not the road.
    let mut scene = TedScene::parse_any(text).map_err(|e| e.to_string())?;
    for g in scene.find_all(ROAD_GUIDE).into_iter().rev() {
        scene.remove(g).map_err(|e| e.to_string())?;
    }

    Ok(Fitted { text: scene.save(), position, overrides: ovs })
}

/// An instance's element where it goes: its position and rotation set, freeform at that height (as the kit's tiles keep one, the height
/// again in `lf_offset`, over the tile's LF of 0). `None`: the text does not read.
#[must_use]
pub fn placed_text(text: &[u8], position: [f64; 3], rotation: &Double3x3) -> Option<Vec<u8>> {
    let mut mini = TedScene::parse_any(text.to_vec()).ok()?;
    let els = mini.doc().elements();
    let root = (0..els.len()).find(|&e| els[e].parent.is_none())?;
    mini.set_position(root, Double3::new(position[0], position[1], position[2]));
    mini.set_euler(root, euler_from_matrix(rotation));
    mini.set_attr(root, "lf_offset", &format_number(position[1]));
    Some(mini.save())
}

/// The ports of a fitted piece, where they are on the road (world metres): what the next piece must meet.
#[must_use]
pub fn ports_at(piece: &Piece, laid: &Laid, line: &[[f64; 2]], width: f64) -> Vec<[f64; 2]> {
    let st = Stations::new(line);
    let mut out = vec![];
    for (i, arm) in piece.arms.iter().enumerate() {
        for (port, f) in [(arm.start, 0.0), (arm.end, 1.0)] {
            if port.is_none() {
                continue;
            }

            let probe = Part {
                path: vec![], kind: String::new(), key: String::new(), arm: i, along: f, across: 0.0, up: 0.0, anchor: Anchor::Surface, solid: false,
                size: [0.0; 2], turn: 0.0, at: [0.0; 2], rotation: Double3x3::default(), parent: None, scale: None,
            };
            out.push(place(piece, &probe, &arranged(piece, &laid.frame), &st, width).0);
        }
    }

    out
}

/// How far a fence stands past a road's edge (metres), and how far its runs stop short of what the road holds (a corner, a junction, a
/// bridge, a ford, an end).
pub const FENCE_OUT: f64 = 1.0;
pub const FENCE_GAP: f64 = 0.0;

/// A road's fences along one side (`side`: +1 its right, -1 its left, as it runs): the line they stand on (`offset` from the centre line,
/// metres) and the fence pieces on it, `length` each, end to end in the stretches the road holds nothing on (`held`: `Layout::held`),
/// each run centred in its stretch. A fence is never stretched: a run is as many as fit.
#[must_use]
pub fn fence_runs(line: &[[f64; 2]], held: &[(f64, f64)], side: f64, offset: f64, length: f64) -> (Vec<[f64; 2]>, Vec<Laid>) {
    let n = line.len();
    if n < 2 || length <= 0.0 {
        return (vec![], vec![]);
    }

    let fence_line = (0..n).map(|i| {
        let t = unit([line[(i + 1).min(n - 1)][0] - line[i.saturating_sub(1)][0], line[(i + 1).min(n - 1)][1] - line[i.saturating_sub(1)][1]]);
        [line[i][0] + t[1] * side * offset, line[i][1] - t[0] * side * offset]
    }).collect::<Vec<_>>();
    let (centre, beside) = (Stations::new(line), Stations::new(&fence_line));

    // A distance along the centre line as one along the fence's line, by the samples it falls between.
    let onto = |s: f64| {
        let i = (1..n).find(|&i| centre.of_sample(i) >= s).unwrap_or(n - 1);
        let (a, b) = (centre.of_sample(i - 1), centre.of_sample(i));
        let f = ((s - a) / (b - a).max(1e-9)).clamp(0.0, 1.0);
        beside.of_sample(i - 1) + f * (beside.of_sample(i) - beside.of_sample(i - 1))
    };

    // The stretches nothing holds.
    let total = centre.length();
    let mut free = vec![];
    let mut from = 0.0;
    for &(a, b) in held {
        if a - FENCE_GAP > from + FENCE_GAP {
            free.push((from + if from > 0.0 { FENCE_GAP } else { 0.0 }, a - FENCE_GAP));
        }

        from = from.max(b);
    }

    if from < total {
        free.push((from + if from > 0.0 { FENCE_GAP } else { 0.0 }, total));
    }

    let laid = free.into_iter().flat_map(|(a, b)| fences_between(onto(a), onto(b), length)).collect();
    (fence_line, laid)
}

/// Fences from one distance along a line to another, `length` each, as many as cover it: the first starting at `a`, the last ending at
/// `b`, those between spread evenly (they overlap a little rather than leave a gap). Shorter than one fence: one in its middle, past its
/// ends by as much; shorter than half a fence: none.
#[must_use]
pub fn fences_between(a: f64, b: f64, length: f64) -> Vec<Laid> {
    let span = b - a;
    if span < length * 0.5 || length <= 0.0 {
        return vec![];
    }

    let count = (span / length).ceil().max(1.0) as usize;
    let first = a + length * 0.5;
    let last = (b - length * 0.5).max(first);
    (0..count).map(|k| {
        let c = if count == 1 { (a + b) * 0.5 } else { first + (last - first) * k as f64 / (count - 1) as f64 };
        Laid { piece: "fence", frame: Frame::Along { from: c - length * 0.5, to: c + length * 0.5 } }
    }).collect()
}

/// The fences' lines round a corner or a junction (`at`, its arms' `ways`, each `reach` long to its port): between each two arms next to
/// each other, from one arm's fence line (`offset` from its centre line, on the side facing the other) to the other's, on the arc that
/// meets both of them square at the ports (a straight line where they are in line: a junction's through side). `fenced`: whether the
/// sector between two arms, by the way through its middle, has a fence. Each as a line (metres), about every half metre.
#[must_use]
pub fn star_fences(at: [f64; 2], ways: &[[f64; 2]], reach: f64, offset: f64, fenced: &dyn Fn([f64; 2]) -> bool) -> Vec<Vec<[f64; 2]>> {
    let tau = std::f64::consts::TAU;
    let angle = |w: [f64; 2]| w[1].atan2(w[0]).rem_euclid(tau);
    let mut sorted = ways.to_vec();
    sorted.sort_by(|a, b| angle(*a).total_cmp(&angle(*b)));
    let turn = |w: [f64; 2], a: f64| [w[0] * a.cos() - w[1] * a.sin(), w[0] * a.sin() + w[1] * a.cos()];
    let mut out = vec![];
    for i in 0..sorted.len() {
        let (wi, wj) = (sorted[i], sorted[(i + 1) % sorted.len()]);
        let sector = if sorted.len() == 1 { tau } else { (angle(wj) - angle(wi)).rem_euclid(tau) };
        if sector < 1e-6 || !fenced(turn(wi, sector * 0.5)) {
            continue;
        }

        // Each arm's fence line on the side facing the sector, at its port.
        let (ni, nj) = (turn(wi, std::f64::consts::FRAC_PI_2), turn(wj, -std::f64::consts::FRAC_PI_2));
        let si = [at[0] + wi[0] * reach + ni[0] * offset, at[1] + wi[1] * reach + ni[1] * offset];
        let sj = [at[0] + wj[0] * reach + nj[0] * offset, at[1] + wj[1] * reach + nj[1] * offset];
        let line = |a: [f64; 2], b: [f64; 2]| {
            let n = ((a[0] - b[0]).hypot(a[1] - b[1]) / 0.5).ceil().max(1.0) as usize;
            (0..=n).map(|k| { let t = k as f64 / n as f64; [a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t] }).collect::<Vec<_>>()
        };

        if (sector - std::f64::consts::PI).abs() < 1e-3 {
            out.push(line(si, sj));
            continue;
        }

        // The arc square to both: its middle where the lines square to them at the ports cross.
        let den = ni[0] * nj[1] - ni[1] * nj[0];
        if den.abs() < 1e-9 {
            continue;
        }

        let d = [sj[0] - si[0], sj[1] - si[1]];
        let t = (d[0] * nj[1] - d[1] * nj[0]) / den;
        let c = [si[0] + ni[0] * t, si[1] + ni[1] * t];

        // An inside arc the fence line passes beyond (a sharp turn): a straight fence across it.
        if sector < std::f64::consts::PI && t <= 0.0 {
            out.push(line(si, sj));
            continue;
        }

        let r = (si[0] - c[0]).hypot(si[1] - c[1]);
        let (a0, a1) = ((si[1] - c[1]).atan2(si[0] - c[0]), (sj[1] - c[1]).atan2(sj[0] - c[0]));
        // The way round: as the fence goes on from arm i (back along it, into the star) and on out along arm j.
        let into = [-wi[0], -wi[1]];
        let ccw = (si[0] - c[0]) * into[1] - (si[1] - c[1]) * into[0] > 0.0;
        let sweep = if ccw { (a1 - a0).rem_euclid(tau) } else { -(a0 - a1).rem_euclid(tau) };
        let n = ((r * sweep.abs()) / 0.5).ceil().max(1.0) as usize;
        out.push((0..=n).map(|k| { let a = a0 + sweep * k as f64 / n as f64; [c[0] + r * a.cos(), c[1] + r * a.sin()] }).collect());
    }

    out
}

/// How near another road a road's end is to be taken onto it (metres), and how near it is once taken.
pub const SNAP: f64 = 8.0;
const ON: f64 = 1.0;

/// The nearest place to a point on a line: (distance along it, distance to it, the place).
#[must_use]
pub fn nearest(line: &[[f64; 2]], p: [f64; 2]) -> (f64, f64, [f64; 2]) {
    let (mut best, mut at, mut place) = (f64::MAX, 0.0, line.first().copied().unwrap_or(p));
    let mut run = 0.0;
    for w in line.windows(2) {
        let (a, b) = (w[0], w[1]);
        let d = [b[0] - a[0], b[1] - a[1]];
        let l2 = (d[0] * d[0] + d[1] * d[1]).max(1e-12);
        let t = (((p[0] - a[0]) * d[0] + (p[1] - a[1]) * d[1]) / l2).clamp(0.0, 1.0);
        let q = [a[0] + d[0] * t, a[1] + d[1] * t];
        let dist = (p[0] - q[0]).hypot(p[1] - q[1]);
        if dist < best {
            (best, at, place) = (dist, run + t * l2.sqrt(), q);
        }

        run += l2.sqrt();
    }

    (at, best, place)
}

/// Takes roads' ends onto the roads they stop near (`points`: each road's points; `centres`: each one's centre line): an end within
/// [`SNAP`] of another road's line, away from that road's own ends, goes onto it, so that the road reaches the junction it makes there.
pub fn snap_ends(points: &mut [Vec<[f64; 2]>], centres: &[Vec<[f64; 2]>]) {
    for (a, own) in points.iter_mut().enumerate() {
        let n = own.len();
        if n < 2 {
            continue;
        }

        for end in [0, n - 1] {
            let p = own[end];
            let best = (0..centres.len()).filter(|&b| b != a && centres[b].len() >= 2).map(|b| {
                let (s, d, q) = nearest(&centres[b], p);
                (s, d, q, Stations::new(&centres[b]).length())
            }).filter(|&(s, d, _, l)| d <= SNAP && s > SNAP && s < l - SNAP).min_by(|x, y| x.1.total_cmp(&y.1));
            if let Some((_, _, q, _)) = best {
                own[end] = q;
            }
        }
    }
}

/// The junctions of a tile's roads.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Junctions {
    /// Each road's: what its junctions take of its line.
    pub holds: Vec<Vec<Hold>>,

    /// Each road's ends that end on another road.
    pub joined: Vec<[bool; 2]>,
    pub notes: Vec<String>,
}

/// Where two lines cross: (distance along the first, distance along the second, the place).
fn crossings(a: &[[f64; 2]], b: &[[f64; 2]]) -> Vec<(f64, f64, [f64; 2])> {
    let mut out: Vec<(f64, f64, [f64; 2])> = vec![];
    let (sa, sb) = (Stations::new(a), Stations::new(b));
    let bounds = |l: &[[f64; 2]]| l.iter().fold([f64::MAX, f64::MAX, f64::MIN, f64::MIN], |m, p| [m[0].min(p[0]), m[1].min(p[1]), m[2].max(p[0]), m[3].max(p[1])]);
    let (ba, bb) = (bounds(a), bounds(b));
    if ba[0] > bb[2] || bb[0] > ba[2] || ba[1] > bb[3] || bb[1] > ba[3] {
        return out;
    }

    for i in 0..a.len().saturating_sub(1) {
        let (p, r) = (a[i], [a[i + 1][0] - a[i][0], a[i + 1][1] - a[i][1]]);
        for j in 0..b.len().saturating_sub(1) {
            let (q, t) = (b[j], [b[j + 1][0] - b[j][0], b[j + 1][1] - b[j][1]]);
            let den = r[0] * t[1] - r[1] * t[0];
            if den.abs() < 1e-12 {
                continue;
            }

            let w = [q[0] - p[0], q[1] - p[1]];
            let (u, v) = ((w[0] * t[1] - w[1] * t[0]) / den, (w[0] * r[1] - w[1] * r[0]) / den);
            if (0.0..1.0).contains(&u) && (0.0..1.0).contains(&v) {
                let at = [p[0] + r[0] * u, p[1] + r[1] * u];
                let (da, db) = (sa.of_sample(i) + u * r[0].hypot(r[1]), sb.of_sample(j) + v * t[0].hypot(t[1]));
                if out.iter().all(|o| (o.0 - da).abs() > 1.0) {
                    out.push((da, db, at));
                }
            }
        }
    }

    out
}

/// Finds where a tile's roads meet (`lines`: each one's centre line, ends already taken onto the roads they stop on: [`snap_ends`]): a
/// road ending on another makes a T junction on that one, two roads crossing an X junction on the first of them. `halves`: how far each
/// road's junctions reach each way (their arms' length, by its width). A junction is laid by the road it is on; the others leave it
/// their stretch.
#[must_use]
pub fn junctions(lines: &[Vec<[f64; 2]>], halves: &[f64]) -> Junctions {
    let n = lines.len();
    let mut out = Junctions { holds: vec![vec![]; n], joined: vec![[false; 2]; n], notes: vec![] };
    let st = lines.iter().map(|l| Stations::new(l)).collect::<Vec<_>>();
    let way = |r: usize, s: f64, at: [f64; 2]| {
        let q = st[r].at(s).0;
        unit([q[0] - at[0], q[1] - at[1]])
    };

    // A road's end on another.
    for a in 0..n {
        if lines[a].len() < 2 {
            continue;
        }

        let la = st[a].length();
        for (e, p) in [(0, lines[a][0]), (1, lines[a][lines[a].len() - 1])] {
            let found = (0..n)
                .filter(|&b| b != a && lines[b].len() >= 2)
                .map(|b| (b, nearest(&lines[b], p)))
                .filter(|(_, (_, d, _))| *d <= ON)
                .min_by(|x, y| x.1.1.total_cmp(&y.1.1));
            let Some((b, (sb, _, q))) = found else { continue };
            let (lb, half) = (st[b].length(), halves[b]);
            let on_b = (chord(&st[b], q, sb, -half, lb), chord(&st[b], q, sb, half, lb));
            let on_a = if e == 1 { chord(&st[a], q, la, -half, la) } else { chord(&st[a], q, 0.0, half, la) };
            let (Some(b1), Some(b2), Some(sa)) = (on_b.0, on_b.1, on_a) else {
                out.notes.push(format!("a road ends on another at {:.0}, {:.0} with no room for a junction", q[0], q[1]));
                continue;
            };

            let ways = vec![way(b, b1, q), way(b, b2, q), way(a, sa, q)];
            out.holds[b].push(Hold { from: b1, to: b2, laid: Some(Laid { piece: "t_junction", frame: Frame::Star { at: q, ways } }) });
            out.holds[a].push(if e == 1 { Hold { from: sa, to: la, laid: None } } else { Hold { from: 0.0, to: sa, laid: None } });
            out.joined[a][e] = true;
        }
    }

    // Two roads crossing, away from their ends (an end on a road is a T, above).
    for a in 0..n {
        for b in a + 1..n {
            for (da, db, x) in crossings(&lines[a], &lines[b]) {
                let (la, lb, half) = (st[a].length(), st[b].length(), halves[a]);
                if da < ON || db < ON || da > la - ON || db > lb - ON {
                    continue;
                }

                let found = (chord(&st[a], x, da, -half, la), chord(&st[a], x, da, half, la), chord(&st[b], x, db, -half, lb), chord(&st[b], x, db, half, lb));
                let (Some(a1), Some(a2), Some(b1), Some(b2)) = found else {
                    out.notes.push(format!("two roads cross at {:.0}, {:.0} too near an end for a junction", x[0], x[1]));
                    continue;
                };

                let ways = vec![way(a, a1, x), way(a, a2, x), way(b, b1, x), way(b, b2, x)];
                out.holds[a].push(Hold { from: a1, to: a2, laid: Some(Laid { piece: "x_junction", frame: Frame::Star { at: x, ways } }) });
                out.holds[b].push(Hold { from: b1, to: b2, laid: None });
            }
        }
    }

    out
}

/// How far a crossing's AI hint reaches past the road's edge on each side, and past the crossing's ends (metres): vanilla's bridges' hints
/// are 7 scene units wider than the bridge on each side and reach its feet (`river_crossing` tiles, `AIH_BRIDGE_CROSSING`).
pub const HINT_SIDE: f64 = 3.5;
pub const HINT_ENDS: f64 = 2.0;

/// The least half width of a ford's AI hint (metres): vanilla's fords' hints are 32 to 40 m across (`river_ford` tiles).
pub const FORD_HINT_HALF: f64 = 16.0;

/// The outline of a stretch of a road (from and to: distances along its line, metres), `half` each side of its centre line, sampled about
/// every 4 m: one side forward, the other back, closed. In the file's units (`unit` a metre), on the ground (x, z).
#[must_use]
pub fn strip_outline(line: &[[f64; 2]], from: f64, to: f64, half: f64, unit: f64) -> Vec<[f64; 2]> {
    let st = Stations::new(line);
    let (from, to) = (from.max(0.0), to.min(st.length()));
    let n = (((to - from) / 4.0).ceil() as usize).max(1);
    let (mut left, mut right) = (vec![], vec![]);
    for k in 0..=n {
        let s = from + (to - from) * k as f64 / n as f64;
        let (p, t) = st.at(s);
        let nrm = [t[1], -t[0]];
        right.push([(p[0] + nrm[0] * half) * unit, (p[1] + nrm[1] * half) * unit]);
        left.push([(p[0] - nrm[0] * half) * unit, (p[1] - nrm[1] * half) * unit]);
    }

    right.extend(left.into_iter().rev());
    right
}

/// A crossing's AI hint as vanilla's bridges and fords have one: a closed `AIH_BRIDGE_CROSSING` polygon on the ground over it (its outline
/// in the file's units: [`strip_outline`]). Its text at column 0.
///
/// # Errors
///
/// Fewer than three points.
pub fn crossing_hint(outline: &[[f64; 2]]) -> Result<Vec<u8>, String> {
    use crate::new_instances::{NewInstance, NewPoint, element_text, polygon_area};
    if outline.len() < 3 {
        return Err("a crossing's hint needs three points".to_owned());
    }

    let n = outline.len() as f64;
    let c = outline.iter().fold([0.0, 0.0], |a, p| [a[0] + p[0] / n, a[1] + p[1] / n]);
    let local = outline.iter().map(|p| Double3::new(p[0] - c[0], 0.0, p[1] - c[1])).collect::<Vec<_>>();
    let mut h = NewInstance::new("AI_HINT_POLYGON_INSTANCE")
        .position(Double3::new(c[0], 0.0, c[1]))
        .attr("name", "Road crossing")
        .attr("hint_type", "AIH_BRIDGE_CROSSING")
        .attr("polygon_closed", "1")
        .attr("surface_mode", "MSM_LANDSCAPE")
        .attr("area", format_number(polygon_area(&local)));
    h.points = local.into_iter().map(NewPoint::at).collect();
    element_text(&h)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::road_pieces::{PIECES, base_guides, group_text, read};
    use crate::roads::{centre_line_sharp, sharp_points};

    fn dirt(piece: &str) -> (Piece, Vec<u8>) {
        let mut children = base_guides(piece);
        children.extend(crate::road_dirt::parts(piece));
        let text = group_text(&format!("road_dirt_{piece}"), &children);
        let scene = TedScene::parse_any(text.clone()).unwrap();
        let root = scene.find_all("UserInstanceGroup")[0];
        (read(&text).unwrap(), scene.children_text(root))
    }

    /// A curved road with a sharp corner and two free ends.
    fn road() -> (Vec<[f64; 2]>, Vec<usize>, Layout) {
        let points = [[0.0, 0.0], [40.0, 15.0], [80.0, 10.0], [80.0, 60.0], [110.0, 70.0]];
        let sharp = sharp_points(&points, &[]);
        assert_eq!(sharp, [false, false, true, false, false], "the turn of 97 degrees at the third point is a corner");
        let (line, at) = centre_line_sharp(&points, &sharp);
        let corners = at.iter().zip(&sharp).filter(|(_, s)| **s).map(|(i, _)| *i).collect::<Vec<_>>();
        let l = layout(&line, &corners, [End::Free, End::Free], &[], &Measures::default(), 1.0, 7);
        (line, corners, l)
    }

    #[test]
    fn a_road_is_laid_end_to_end_with_its_corner() {
        let (line, _, l) = road();
        let names = l.laid.iter().map(|p| p.piece).collect::<Vec<_>>();
        assert_eq!(names.first(), Some(&"end"));
        assert_eq!(names.last(), Some(&"end"));
        assert_eq!(names.iter().filter(|n| **n == "corner").count(), 1, "{names:?}");
        assert!(names.contains(&"curve") && names.contains(&"straight"), "{names:?}");
        assert!(l.notes.is_empty(), "{:?}", l.notes);

        // The pieces cover the line with no gap and no overlap.
        let st = Stations::new(&line);
        let mut spans = l.laid.iter().map(|p| match &p.frame {
            Frame::Along { from, to } => (from.min(*to), from.max(*to)),
            Frame::Star { at, ways } => {
                // Its ports on the line: the distances along it nearest them.
                let near = |q: [f64; 2]| (0..=(st.length() * 100.0) as usize).map(|k| k as f64 / 100.0).min_by(|a, b| {
                    let (pa, pb) = (st.at(*a).0, st.at(*b).0);
                    (pa[0] - q[0]).hypot(pa[1] - q[1]).total_cmp(&(pb[0] - q[0]).hypot(pb[1] - q[1]))
                }).unwrap();
                let (a, b) = (near([at[0] + ways[0][0] * 6.0, at[1] + ways[0][1] * 6.0]), near([at[0] + ways[1][0] * 6.0, at[1] + ways[1][1] * 6.0]));
                (a.min(b), a.max(b))
            }
        }).collect::<Vec<_>>();
        spans.sort_by(|a, b| a.0.total_cmp(&b.0));
        assert!(spans[0].0.abs() < 1e-6 && (spans.last().unwrap().1 - st.length()).abs() < 1e-6);
        for w in spans.windows(2) {
            assert!((w[1].0 - w[0].1).abs() < 0.02, "{w:?}");
        }
    }

    #[test]
    fn the_laid_pieces_meet_port_to_port() {
        let (line, _, l) = road();
        let pieces = PIECES.iter().map(|p| (*p, dirt(p).0)).collect::<Vec<_>>();
        let piece = |name: &str| &pieces.iter().find(|p| p.0 == name).unwrap().1;
        let ports = l.laid.iter().map(|p| ports_at(piece(p.piece), p, &line, 6.0)).collect::<Vec<_>>();

        // Every port but the ends' closed sides meets one of the next piece's or the one before's.
        for (i, own) in ports.iter().enumerate() {
            for p in own {
                let near = |j: usize| ports.get(j).is_some_and(|q| q.iter().any(|q| (q[0] - p[0]).hypot(q[1] - p[1]) < 0.05));
                assert!(near(i + 1) || (i > 0 && near(i - 1)), "piece {i} ({}) port {p:?} meets nothing", l.laid[i].piece);
            }
        }
    }

    #[test]
    fn a_piece_fitted_keeps_its_parts_on_the_road() {
        let (line, _, l) = road();
        let st = Stations::new(&line);
        for laid in &l.laid {
            let (piece, children) = dirt(laid.piece);
            let f = fit(&piece, &children, "road_dirt_x", laid, &Site { line: &line, width: 6.0, unit: 2.0, ground: &|_, _| 3.0 }).unwrap();
            let text = String::from_utf8(f.text.clone()).unwrap();
            assert!(!text.contains(ROAD_GUIDE), "no guides");
            let scene = TedScene::parse_any(f.text).unwrap();
            let decals = scene.find_all("ProjectedDecalInstance");
            assert_eq!(decals.len(), piece.parts.len());
            for d in decals {
                let w = crate::world_transform::world_transform(&scene, d);
                let p = [w.position.x / 2.0, w.position.z / 2.0];
                assert!((w.position.y - 6.0).abs() < 1e-6, "at the ground's height");

                // Within the road and its verge of the centre line.
                let near = (0..=(st.length() as usize)).map(|s| { let q = st.at(s as f64).0; (q[0] - p[0]).hypot(q[1] - p[1]) }).fold(f64::MAX, f64::min);
                assert!(near < 7.5, "{}: a part {near:.1} m off the road", laid.piece);
            }
        }
    }

    #[test]
    fn a_straight_stretched_scales_its_ruts_along() {
        let line = (0..=10).map(|i| [0.0, f64::from(i)]).collect::<Vec<_>>();
        let (piece, children) = dirt("straight");
        let laid = Laid { piece: "straight", frame: Frame::Along { from: 0.0, to: 10.0 } };
        let f = fit(&piece, &children, "s", &laid, &Site { line: &line, width: 9.0, unit: 2.0, ground: &|_, _| 0.0 }).unwrap();
        let scene = TedScene::parse_any(f.text).unwrap();
        let ruts = scene.find_all("ProjectedDecalInstance").into_iter().filter(|&d| scene.doc().get(d, "key").unwrap().contains("path_1")).collect::<Vec<_>>();
        for d in ruts {
            let s = scene.scale(d).unwrap();
            assert!((s.x - 19.0 * 1.25).abs() < 1e-6 && (s.z.abs() - 5.2 * 1.5).abs() < 1e-6, "{s:?}");
            let x = scene.position(d).unwrap().x / 2.0;
            assert!((x.abs() - 1.3 * 1.5).abs() < 1e-6, "{x}");
        }

        // An edge's border keeps its width and moves out with the edge.
        let border = scene.find_all("ProjectedDecalInstance").into_iter().find(|&d| scene.doc().get(d, "key").unwrap().contains("border_1")).unwrap();
        assert!((scene.position(border).unwrap().x.abs() / 2.0 - 4.5).abs() < 1e-6);
        assert!((scene.scale(border).unwrap().z.abs() - 4.0).abs() < 1e-6);
    }

    #[test]
    fn a_left_corner_turns_its_piece_over() {
        // Up +z, then left (-x): the corner piece turns right, so its arms swap and its outer edge stays outside.
        let points = [[0.0, -30.0], [0.0, 0.0], [-30.0, 0.0]];
        let sharp = sharp_points(&points, &[]);
        let (line, at) = centre_line_sharp(&points, &sharp);
        let l = layout(&line, &[at[1]], [End::Edge, End::Edge], &[], &Measures::default(), 1.0, 1);
        let corner = l.laid.iter().find(|p| p.piece == "corner").unwrap();
        let (piece, children) = dirt("corner");
        let f = fit(&piece, &children, "c", corner, &Site { line: &line, width: 6.0, unit: 2.0, ground: &|_, _| 0.0 }).unwrap();
        let scene = TedScene::parse_any(f.text).unwrap();

        // The outer edge (the arc of border_1 chords) lies outside the inner one (border_2): towards +x, +z, the turn's outside.
        let mean = |name: &str| {
            let ps = scene.find_all("ProjectedDecalInstance").into_iter().filter(|&d| scene.doc().get(d, "key").unwrap().contains(name)).map(|d| scene.position(d).unwrap()).collect::<Vec<_>>();
            let n = ps.len() as f64;
            (ps.iter().map(|p| p.x).sum::<f64>() / n, ps.iter().map(|p| p.z).sum::<f64>() / n)
        };
        let (outer, inner) = (mean("border_1"), mean("border_2"));
        assert!(outer.0 - inner.0 > 2.0 && outer.1 - inner.1 > 2.0, "{outer:?} {inner:?}");
    }

    /// Two roads crossing (A along +x, B along +z through it at x = 50), and a third (C) stopping 3 m short of A from its left (+z) side.
    fn three_roads() -> (Vec<Vec<[f64; 2]>>, Junctions, Vec<Layout>) {
        let mut points = vec![vec![[0.0, 0.0], [100.0, 0.0]], vec![[50.0, -50.0], [50.0, 50.0]], vec![[20.0, 40.0], [30.0, 3.0]]];
        let centre = |p: &Vec<[f64; 2]>| centre_line_sharp(p, &[]).0;
        let centres = points.iter().map(centre).collect::<Vec<_>>();
        snap_ends(&mut points, &centres);
        assert!(points[2][1][1].abs() < 1e-9, "C's end is taken onto A: {:?}", points[2][1]);
        let lines = points.iter().map(centre).collect::<Vec<_>>();
        let j = junctions(&lines, &[6.0, 6.0, 6.0]);
        let laid = (0..3).map(|r| {
            let ends = [0, 1].map(|e| if j.joined[r][e] { End::Joined } else { End::Free });
            layout(&lines[r], &[], ends, &j.holds[r], &Measures::default(), 1.0, 3)
        }).collect::<Vec<_>>();
        (lines, j, laid)
    }

    #[test]
    fn crossing_roads_make_an_x_and_an_end_on_a_road_a_t() {
        let (_, j, laid) = three_roads();
        assert!(j.notes.is_empty(), "{:?}", j.notes);
        assert_eq!(j.joined, [[false, false], [false, false], [false, true]]);
        let names = |r: usize| laid[r].laid.iter().map(|l| l.piece).collect::<Vec<_>>();
        assert_eq!(names(0).iter().filter(|n| **n == "t_junction").count(), 1, "{:?}", names(0));
        assert_eq!(names(0).iter().filter(|n| **n == "x_junction").count(), 1, "the first road lays the crossing: {:?}", names(0));
        assert!(!names(1).contains(&"x_junction"), "the second leaves it its stretch: {:?}", names(1));
        assert_eq!(names(2).iter().filter(|n| **n == "end").count(), 1, "C's end on A has no end piece: {:?}", names(2));
    }

    #[test]
    fn the_pieces_of_roads_that_meet_meet_port_to_port() {
        let (lines, _, laid) = three_roads();
        let pieces = PIECES.iter().map(|p| (*p, dirt(p).0)).collect::<Vec<_>>();
        let piece = |name: &str| &pieces.iter().find(|p| p.0 == name).unwrap().1;
        let ports = laid.iter().zip(&lines).flat_map(|(l, line)| l.laid.iter().map(|p| (p.piece, ports_at(piece(p.piece), p, line, 6.0))).collect::<Vec<_>>()).collect::<Vec<_>>();
        for (i, (name, own)) in ports.iter().enumerate() {
            for p in own {
                let met = ports.iter().enumerate().any(|(k, (_, q))| k != i && q.iter().any(|q| (q[0] - p[0]).hypot(q[1] - p[1]) < 0.05));
                assert!(met, "a {name}'s port at {p:?} meets nothing");
            }
        }
    }

    #[test]
    fn a_t_with_its_branch_on_the_left_keeps_its_through_edge_opposite() {
        let (lines, _, laid) = three_roads();
        let t = laid[0].laid.iter().find(|l| l.piece == "t_junction").unwrap();
        let (piece, children) = dirt("t_junction");
        let f = fit(&piece, &children, "t", t, &Site { line: &lines[0], width: 6.0, unit: 2.0, ground: &|_, _| 0.0 }).unwrap();
        let scene = TedScene::parse_any(f.text).unwrap();

        // The through edge (border_1, at x = -3 in the piece, away from its branch) lies on A's far side from C: -z.
        let borders = scene.find_all("ProjectedDecalInstance").into_iter().filter(|&d| scene.doc().get(d, "key").unwrap().contains("border_1")).collect::<Vec<_>>();
        assert_eq!(borders.len(), 2);
        for d in borders {
            let w = crate::world_transform::world_transform(&scene, d);
            assert!(w.position.z < 0.0, "{:?}", w.position);
        }
    }

    #[test]
    fn a_crossing_hint_covers_the_crossing_and_its_sides() {
        // A road along x, bent up towards its end; a crossing from 40 to 60 m.
        let line = (0..=100).map(|i| { let x = f64::from(i); [x, if x > 50.0 { (x - 50.0) * 0.3 } else { 0.0 }] }).collect::<Vec<_>>();
        let outline = strip_outline(&line, 38.0, 62.0, 6.5, 2.0);
        assert!(outline.len() >= 8 && outline.len() % 2 == 0);
        let inside = |p: [f64; 2]| {
            let mut odd = false;
            for i in 0..outline.len() {
                let (a, b) = (outline[i], outline[(i + 1) % outline.len()]);
                if (a[1] > p[1]) != (b[1] > p[1]) && p[0] < a[0] + (p[1] - a[1]) / (b[1] - a[1]) * (b[0] - a[0]) {
                    odd = !odd;
                }
            }

            odd
        };

        // The road's centre and its edges over the crossing are inside, in the file's units; past its ends, not.
        let st = Stations::new(&line);
        for s in [40.0, 50.0, 60.0] {
            let (p, t) = st.at(s);
            let n = [t[1], -t[0]];
            for d in [0.0, 3.0, -3.0] {
                assert!(inside([(p[0] + n[0] * d) * 2.0, (p[1] + n[1] * d) * 2.0]), "{s} {d}");
            }
        }

        assert!(!inside([30.0 * 2.0, 0.0]) && !inside([50.0 * 2.0, 8.0 * 2.0]));

        let text = String::from_utf8(crossing_hint(&outline).unwrap()).unwrap();
        assert!(text.starts_with("<AI_HINT_POLYGON_INSTANCE") && text.contains("hint_type=\"AIH_BRIDGE_CROSSING\"") && text.contains("polygon_closed=\"1\""), "{text}");
        assert_eq!(text.matches("<point ").count(), outline.len());
        let scene = TedScene::parse_any(text.into_bytes()).unwrap();
        assert!(scene.polygon_area(0) > 2.0 * 20.0 * 2.0 * 13.0 * 0.9, "{}", scene.polygon_area(0));
    }

    #[test]
    fn fences_run_beside_the_road_and_stop_at_what_it_holds() {
        let line = (0..=100).map(|i| [f64::from(i), 0.0]).collect::<Vec<_>>();
        let (fence_line, laid) = fence_runs(&line, &[(40.0, 60.0)], 1.0, 4.0, 5.0);
        // On the right (+x ahead: -z), 4 m out.
        assert!(fence_line.iter().all(|p| (p[1] + 4.0).abs() < 1e-9), "{:?}", &fence_line[..3]);
        assert_eq!(laid.len(), 16, "8 a side of the held stretch, end to end: {laid:?}");
        for l in &laid {
            let Frame::Along { from, to } = l.frame else { panic!("along") };
            assert!((to - from - 5.0).abs() < 1e-9);
            assert!(to <= 40.0 + 1e-9 || from >= 60.0 - 1e-9, "{from}..{to} in the held stretch");
        }

        // The left side is the other way across.
        let (left, _) = fence_runs(&line, &[], -1.0, 4.0, 5.0);
        assert!(left.iter().all(|p| (p[1] - 4.0).abs() < 1e-9));
    }

    #[test]
    fn fences_cover_a_stretch_end_to_end() {
        let f = fences_between(0.0, 12.0, 5.0);
        assert_eq!(f.len(), 3);
        let spans = f.iter().map(|l| match l.frame { Frame::Along { from, to } => (from, to), _ => unreachable!() }).collect::<Vec<_>>();
        assert!((spans[0].0).abs() < 1e-9 && (spans[2].1 - 12.0).abs() < 1e-9, "{spans:?}");
        assert!(fences_between(0.0, 2.0, 5.0).is_empty());
        assert_eq!(fences_between(0.0, 3.0, 5.0).len(), 1);
    }

    #[test]
    fn a_corners_fences_go_round_it_outside_and_in() {
        // Back along -z, out along +x: ports 6 from the middle, fences 4 out.
        let lines = star_fences([0.0, 0.0], &[[0.0, -1.0], [1.0, 0.0]], 6.0, 4.0, &|_| true);
        assert_eq!(lines.len(), 2);
        let ends = |l: &Vec<[f64; 2]>| (l[0], l[l.len() - 1]);
        let near = |a: [f64; 2], b: [f64; 2]| (a[0] - b[0]).hypot(a[1] - b[1]) < 1e-6;
        // Each one round its centre (6, -6): radius 10 outside, 2 inside, from one arm's fence line to the other's.
        for l in &lines {
            let r = (l[0][0] - 6.0).hypot(l[0][1] + 6.0);
            assert!(l.iter().all(|p| ((p[0] - 6.0).hypot(p[1] + 6.0) - r).abs() < 1e-6), "not one arc");
            assert!((r - 10.0).abs() < 1e-6 || (r - 2.0).abs() < 1e-6, "{r}");
        }

        let (a, b) = ends(lines.iter().find(|l| (l[0][0] - 6.0).hypot(l[0][1] + 6.0) > 5.0).unwrap());
        assert!((near(a, [-4.0, -6.0]) && near(b, [6.0, 4.0])) || (near(b, [-4.0, -6.0]) && near(a, [6.0, 4.0])), "{a:?} {b:?}");
    }

    #[test]
    fn a_ts_through_side_is_straight_and_its_other_sides_bend_in() {
        let lines = star_fences([0.0, 0.0], &[[0.0, -1.0], [0.0, 1.0], [1.0, 0.0]], 6.0, 4.0, &|_| true);
        assert_eq!(lines.len(), 3);
        let straight = lines.iter().filter(|l| l.iter().all(|p| (p[0] + 4.0).abs() < 1e-6)).count();
        assert_eq!(straight, 1, "the through side, opposite the branch");
    }

    #[test]
    fn a_tilt_takes_up_onto_the_grounds_normal_and_keeps_level_ground_level() {
        for n in [Double3::new(0.3, 0.9, -0.2), Double3::new(-0.5, 0.8, 0.4), Double3::new(0.0, 1.0, 0.0)] {
            let l = (n.x * n.x + n.y * n.y + n.z * n.z).sqrt();
            let n = n * (1.0 / l);
            let up = up_to(n) * Double3::new(0.0, 1.0, 0.0);
            assert!((up.x - n.x).abs() < 1e-9 && (up.y - n.y).abs() < 1e-9 && (up.z - n.z).abs() < 1e-9, "{up:?} {n:?}");
        }

        // A fence on a slope along it: tilted; on level ground: as it was.
        let line = (0..=20).map(|i| [0.0, f64::from(i)]).collect::<Vec<_>>();
        let mut children = crate::road_pieces::base_guides("fence");
        children.extend(crate::road_dirt::parts("fence"));
        let text = crate::road_pieces::group_text("road_dirt_fence", &children);
        let piece = crate::road_pieces::read(&text).unwrap();
        let scene = TedScene::parse_any(text).unwrap();
        let contents = scene.children_text(scene.find_all("UserInstanceGroup")[0]);
        let laid = Laid { piece: "fence", frame: Frame::Along { from: 5.0, to: 10.0 } };
        let dress = Dress { tilt: true, attrs: None };
        let orientation = |ground: &dyn Fn(f64, f64) -> f64| {
            let f = fit_dressed(&piece, &contents, "f", &laid, &Site { line: &line, width: 1.0, unit: 2.0, ground }, &dress).unwrap();
            let s = TedScene::parse_any(f.text).unwrap();
            s.euler(s.find_all("BUILDING_INSTANCE")[0]).unwrap()
        };

        let flat = orientation(&|_, _| 0.0);
        assert!(flat.y.abs() < 1e-9 && flat.z.abs() < 1e-9, "{flat:?}");
        let sloped = orientation(&|_, z| z * 0.2);
        assert!(sloped.y.abs() > 0.1 || sloped.z.abs() > 0.1, "{sloped:?}");
    }
}
