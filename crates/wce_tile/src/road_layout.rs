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
/// than [`STRAIGHT_BEND`] a straight. `skip`: stretches of the line that take no piece (a bridge's), as distances along it. `scale`: the
/// road's width over the pieces': a corner grows with it. The seed turns straights and curves end for end.
#[must_use]
pub fn layout(line: &[[f64; 2]], corners: &[usize], ends: [End; 2], skip: &[(f64, f64)], measures: &Measures, scale: f64, seed: u32) -> Layout {
    let mut out = Layout::default();
    let st = Stations::new(line);
    let total = st.length();
    if line.len() < 2 || total <= 0.0 {
        return out;
    }

    // What takes the line first: bridges, corners, ends; each as the stretch it holds.
    let mut held: Vec<(f64, f64, Option<Laid>)> = skip.iter().map(|&(a, b)| (a.max(0.0), b.min(total), None)).collect();
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
    if ends[0] == End::Free && !overlaps(&held, 0.0, end) && end < total {
        held.push((0.0, end, Some(Laid { piece: "end", frame: Frame::Along { from: 0.0, to: end } })));
    }

    if ends[1] == End::Free && !overlaps(&held, total - end, total) && end < total {
        held.push((total - end, total, Some(Laid { piece: "end", frame: Frame::Along { from: total, to: total - end } })));
    }

    held.sort_by(|a, b| a.0.total_cmp(&b.0));

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
    let Site { line, width, unit, ground } = *site;
    let st = Stations::new(line);
    let frame = match &laid.frame {
        // The corner's ways in the order its arms turn: a turn the other way takes them the other way round.
        Frame::Star { at, ways } if ways.len() == 2 && piece.arms.len() == 2 => {
            let own = angle_between(arm_way(&piece.arms[0]), arm_way(&piece.arms[1]));
            let road = angle_between(ways[0], ways[1]);
            if own.signum() == road.signum() { laid.frame.clone() } else { Frame::Star { at: *at, ways: vec![ways[1], ways[0]] } }
        }

        f => f.clone(),
    };

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
    for part in &piece.parts {
        let (at, x) = place(piece, part, &frame, &st, width);
        let height = ground(at[0], at[1]);
        let own_x = part.rotation * Double3::new(1.0, 0.0, 0.0);
        let turn = heading(x) - heading([own_x.x, own_x.z]);
        let world_rot = euler_matrix(Double3::new(turn, 0.0, 0.0)) * part.rotation;
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
            out.push(place(piece, &probe, &laid.frame, &st, width).0);
        }
    }

    out
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
            assert!((s.x - 17.0 * 1.25).abs() < 1e-6 && (s.z.abs() - 5.0 * 1.5).abs() < 1e-6, "{s:?}");
            let x = scene.position(d).unwrap().x / 2.0;
            assert!((x.abs() - 1.3 * 1.5).abs() < 1e-6, "{x}");
        }

        // An edge's border keeps its width and moves out with the edge.
        let border = scene.find_all("ProjectedDecalInstance").into_iter().find(|&d| scene.doc().get(d, "key").unwrap().contains("border_1")).unwrap();
        assert!((scene.position(border).unwrap().x.abs() / 2.0 - 4.5).abs() < 1e-6);
        assert!((scene.scale(border).unwrap().z.abs() - 3.0).abs() < 1e-6);
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

        // The outer edge's corner border (at -3, 3 in the piece) is outside the turn: +x, +z of the corner.
        let fixed_border = scene.find_all("ProjectedDecalInstance").into_iter().find(|&d| scene.doc().get(d, "key").unwrap().contains("border_2") && scene.doc().get(d, "road_anchor").is_some()).unwrap();
        let p = scene.position(fixed_border).unwrap();
        assert!(p.x > 0.0 && p.z > 0.0, "{p:?}");
    }
}
