//! The pieces procedural roads are laid with (docs/plans/procedural_roads.md, "Road pieces"): prefabs of the projects' `roads_proc` folder,
//! one folder per road type, each with the same pieces. A piece says where its road is with **guides** (`WceRoadGuide`, open polylines of
//! the editor's own): its centre line, its edges, its verges and its ports, where the next piece joins. The base pieces are those guides
//! alone, at the standard measures, for whoever draws a road type to put decals and buildings against; [`read`] finds a piece's arms from
//! its guides and where each of its parts is on them, which is what a road needs to bend the piece to its line.
//!
//! Metres here; a prefab is in the file's units ([`UNIT`] a metre in a battle tile).

use std::path::{Path, PathBuf};

use wce_foundation::math::{Double3, Double3x3};

use crate::new_instances::{NewInstance, NewPoint, ROAD_GUIDE, element_text};
use crate::ted_scene::TedScene;
use crate::world_transform::world_transform;

/// The folder of the road pieces in a prefab folder, and the folder of the base pieces in it.
pub const FOLDER: &str = "roads_proc";
pub const BASE: &str = "base";

/// A guide's role, and a part's own anchor where it is not where it stands (`surface`, `edge`, `verge`, `fixed`).
pub const ROLE_ATTR: &str = "guide_role";
pub const ANCHOR_ATTR: &str = "road_anchor";

/// The file's units a metre, in a battle tile.
pub const UNIT: f64 = 2.0;

/// The standard measures (metres): the road's width, a straight's length, a junction's size, an end's length, how far a verge is past its
/// edge, the curve's radius and turn.
pub const WIDTH: f64 = 6.0;
pub const STRAIGHT: f64 = 8.0;
pub const JUNCTION: f64 = 12.0;
pub const END: f64 = 4.0;
pub const VERGE: f64 = 3.0;
pub const CURVE_RADIUS: f64 = 15.0;
pub const CURVE_TURN: f64 = std::f64::consts::FRAC_PI_4;

/// A fence piece's length along its line and the width of that line (metres).
pub const FENCE: f64 = 5.0;
pub const FENCE_WIDTH: f64 = 1.0;

/// How near an edge a part is an edge's (metres).
const EDGE_BAND: f64 = 1.0;

/// How near a port a centre guide's end is that port's (metres).
const PORT_SNAP: f64 = 0.5;

/// The pieces every road type has.
/// `fence` is not laid on the road but beside it, along a line just past its edge (`road_layout::fence_runs`): a road that asks for fences.
pub const PIECES: [&str; 8] = ["straight", "curve", "corner", "t_junction", "x_junction", "end", "ford", "fence"];

/// Variants a road type may have of a piece besides the piece itself (variant, piece): each laid in the piece's place, one or another by
/// the road's seed (user 2026-11-08: the same straight every 8 m was monotonous). A variant is read as the piece it varies (its guides);
/// the base has none, and a type without one lays the piece alone.
pub const VARIANTS: [(&str, &str); 4] =
    [("straight_2", "straight"), ("straight_3", "straight"), ("straight_4", "straight"), ("straight_5", "straight")];

/// The piece a variant varies.
#[must_use]
pub fn varies(name: &str) -> Option<&'static str> {
    VARIANTS.iter().find(|v| v.0 == name).map(|v| v.1)
}

/// A prefab's key for a road type's piece: keys are one list across every folder, so the type is in it.
#[must_use]
pub fn key(road_type: &str, piece: &str) -> String {
    format!("road_{road_type}_{piece}")
}

/// What a guide marks.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Role {
    Centre,
    Edge,
    Verge,
    Port,
}

impl Role {
    #[must_use]
    pub fn name(self) -> &'static str {
        match self {
            Self::Centre => "centre",
            Self::Edge => "edge",
            Self::Verge => "verge",
            Self::Port => "port",
        }
    }

    #[must_use]
    pub fn parse(s: &str) -> Option<Self> {
        [Self::Centre, Self::Edge, Self::Verge, Self::Port].into_iter().find(|r| r.name() == s)
    }
}

/// How a part follows the road when its piece is bent and widened.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Anchor {
    /// On the road: across scaled by the road's width.
    Surface,

    /// By an edge: keeps its distance from it.
    Edge,

    /// Past an edge, on the verge: keeps its distance from the verge.
    Verge,

    /// Where it is in the piece's frame.
    Fixed,
}

impl Anchor {
    #[must_use]
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "surface" => Some(Self::Surface),
            "edge" => Some(Self::Edge),
            "verge" => Some(Self::Verge),
            "fixed" => Some(Self::Fixed),
            _ => None,
        }
    }
}

/// Where a piece joins the next: its middle on the centre line, the way out of the piece, and the road's width there.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Port {
    pub at: [f64; 2],
    pub out: [f64; 2],
    pub width: f64,
}

/// A stretch of the piece's road: its centre guide, ending on a port (a junction's, a corner's or an end's arm runs from the middle out)
/// or on a port at each end (a straight's, a curve's, a ford's).
#[derive(Clone, Debug, PartialEq)]
pub struct Arm {
    pub centre: Vec<[f64; 2]>,
    pub length: f64,

    /// The ports at the centre line's start and end.
    pub start: Option<Port>,
    pub end: Option<Port>,
}

/// A decal, building or mesh of a piece, and where it is on the piece's road.
#[derive(Clone, Debug, PartialEq)]
pub struct Part {
    /// Its path in the prefab's contents (`prefab_refs::path_in`): what its overrides name.
    pub path: Vec<usize>,
    pub kind: String,
    pub key: String,

    pub arm: usize,

    /// Along its arm's centre line, a fraction of the arm (0 at its start).
    pub along: f64,

    /// From the centre line, + to the right of the way the arm runs (metres).
    pub across: f64,

    /// Over the ground.
    pub up: f64,
    pub anchor: Anchor,

    /// It stands (a building or a mesh), so it never goes on the road.
    pub solid: bool,

    /// A decal's size on the ground along the road and across it, where it is (metres); 0 for the rest.
    pub size: [f64; 2],

    /// Its turn from the road's way where it is (radians, about up): where its x points, from the arm's way towards its right.
    pub turn: f64,

    /// Where it is in the piece's frame (x, z, metres), and its rotation there.
    pub at: [f64; 2],
    pub rotation: Double3x3,

    /// Its parent's place and rotation in the piece's frame (metres), when its parent is not the piece's group: what its own values are
    /// relative to.
    pub parent: Option<(Double3, Double3x3)>,

    /// Its own scale, when it has one (the file's units).
    pub scale: Option<Double3>,
}

/// A piece read.
#[derive(Clone, Debug, PartialEq)]
pub struct Piece {
    pub arms: Vec<Arm>,
    pub ports: Vec<Port>,

    /// The road's width, as its ports have it.
    pub width: f64,

    /// How far its verges are past its edges, when it has any.
    pub verge: Option<f64>,
    pub parts: Vec<Part>,

    /// What did not read as it should: a building on the road, a part far from every arm.
    pub notes: Vec<String>,
}

/// The instances of a piece that are laid with it.
fn part_kind(kind: &str) -> bool {
    matches!(kind, "ProjectedDecalInstance" | "BUILDING_INSTANCE" | "SIMPLE_MESH_INSTANCE" | "PROP_INSTANCE")
}

fn sub(a: [f64; 2], b: [f64; 2]) -> [f64; 2] {
    [a[0] - b[0], a[1] - b[1]]
}

fn len(a: [f64; 2]) -> f64 {
    a[0].hypot(a[1])
}

fn polyline_length(line: &[[f64; 2]]) -> f64 {
    line.windows(2).map(|w| len(sub(w[1], w[0]))).sum()
}

/// The way a line runs at a distance along it (a unit vector).
fn tangent_at(line: &[[f64; 2]], at: f64) -> [f64; 2] {
    let mut run = 0.0;
    let last = line.len().saturating_sub(2);
    for (i, w) in line.windows(2).enumerate() {
        let d = sub(w[1], w[0]);
        let l = len(d);
        if run + l >= at || i == last {
            let l = l.max(1e-12);
            return [d[0] / l, d[1] / l];
        }

        run += l;
    }

    [0.0, 1.0]
}

/// Where a point is on a line: (distance along it, signed distance from it, + to the right of the way it runs, distance to it).
fn station(line: &[[f64; 2]], p: [f64; 2]) -> (f64, f64, f64) {
    let (mut best, mut at, mut side) = (f64::MAX, 0.0, 0.0);
    let mut run = 0.0;
    for w in line.windows(2) {
        let (a, b) = (w[0], w[1]);
        let d = sub(b, a);
        let l = len(d).max(1e-12);
        let t = (((p[0] - a[0]) * d[0] + (p[1] - a[1]) * d[1]) / (l * l)).clamp(0.0, 1.0);
        let q = [a[0] + d[0] * t, a[1] + d[1] * t];
        let dist = len(sub(p, q));
        if dist < best {
            best = dist;
            at = run + t * l;
            // Right of the way it runs, seen from above with +z ahead: +x.
            side = (p[0] - a[0]) * d[1] - (p[1] - a[1]) * d[0];
        }

        run += l;
    }

    (at, side.signum() * best, best)
}

/// Reads a piece: its group's text (a prefab's `group_text`). Its guides give its arms and ports; every other instance in it is a part.
///
/// # Errors
///
/// Not XML, no group, or not a piece: no centre guide, no port, or a centre guide with no port at either end.
pub fn read(group_text: &[u8]) -> Result<Piece, String> {
    let scene = TedScene::parse_any(group_text.to_vec()).map_err(|e| e.to_string())?;
    let els = scene.doc().elements();
    let root = (0..els.len()).find(|&e| els[e].parent.is_none()).ok_or("no group")?;
    let origin = world_transform(&scene, root);
    let back = origin.rotation.transpose();
    let local = |p: Double3| back * (p - origin.position);

    // The guides, in the piece's frame (metres).
    let mut guides: Vec<(Role, Vec<[f64; 2]>)> = vec![];
    for e in scene.find_all(ROAD_GUIDE) {
        let Some(role) = scene.doc().get(e, ROLE_ATTR).and_then(|r| Role::parse(&r)) else { continue };
        let w = world_transform(&scene, e);
        let line = scene
            .polygon_points(e)
            .iter()
            .filter_map(|&p| scene.point_position(p))
            .map(|p| {
                let q = local(w.position + w.rotation * p);
                [q.x / UNIT, q.z / UNIT]
            })
            .collect::<Vec<_>>();
        if line.len() >= 2 {
            guides.push((role, line));
        }
    }

    let of = |role: Role| guides.iter().filter(move |g| g.0 == role).map(|g| &g.1);
    let centres = of(Role::Centre).cloned().collect::<Vec<_>>();
    if centres.is_empty() {
        return Err("no centre guide".to_owned());
    }

    // A port: the middle of its line, across the road; its way out is found from the centre guide that ends on it.
    let port_lines = of(Role::Port).map(|l| (l[0], *l.last().unwrap_or(&l[0]))).collect::<Vec<_>>();
    if port_lines.is_empty() {
        return Err("no port guide".to_owned());
    }

    let mut ports = vec![];
    let mut arms = vec![];
    for centre in centres {
        let port_at = |end: [f64; 2], inward: [f64; 2]| {
            port_lines.iter().find(|(a, b)| len(sub([(a[0] + b[0]) * 0.5, (a[1] + b[1]) * 0.5], end)) < PORT_SNAP).map(|&(a, b)| {
                // Square to the port's line (the centre line's last stretch is a chord of a curve), away from the centre line's inside.
                let across = sub(b, a);
                let width = len(across).max(1e-12);
                let mut out = [across[1] / width, -across[0] / width];
                let way = sub(end, inward);
                if out[0] * way[0] + out[1] * way[1] < 0.0 {
                    out = [-out[0], -out[1]];
                }

                Port { at: [(a[0] + b[0]) * 0.5, (a[1] + b[1]) * 0.5], out, width }
            })
        };

        let n = centre.len();
        let start = port_at(centre[0], centre[1]);
        let end = port_at(centre[n - 1], centre[n - 2]);
        if start.is_none() && end.is_none() {
            return Err(format!("a centre guide from {:?} to {:?} ends on no port", centre[0], centre[n - 1]));
        }

        ports.extend(start.into_iter().chain(end));
        arms.push(Arm { length: polyline_length(&centre), centre, start, end });
    }

    let width = ports.iter().map(|p| p.width).sum::<f64>() / ports.len() as f64;
    let edges = of(Role::Edge).collect::<Vec<_>>();
    let verge = {
        // The nearest a verge comes to an edge: its corners are further, round the edge's.
        let v = of(Role::Verge)
            .flat_map(|l| l.iter())
            .map(|&p| edges.iter().map(|e| station(e, p).2).fold(f64::MAX, f64::min))
            .fold(f64::MAX, f64::min);
        (v < f64::MAX && !edges.is_empty()).then_some(v)
    };

    // The parts: every instance to lay with the piece, its guides aside.
    let mut parts = vec![];
    let mut notes = vec![];
    for (e, el) in els.iter().enumerate() {
        if !part_kind(&el.name) {
            continue;
        }

        let Some(path) = crate::prefab_refs::path_in(scene.doc(), root, e) else { continue };
        let w = world_transform(&scene, e);
        let p = local(w.position);
        let at = [p.x / UNIT, p.z / UNIT];
        let key = scene.doc().get(e, "key").unwrap_or_default();
        let (arm, (along, across, dist)) = arms
            .iter()
            .enumerate()
            .map(|(i, a)| (i, station(&a.centre, at)))
            .min_by(|a, b| a.1.2.total_cmp(&b.1.2))
            .unwrap_or((0, (0.0, 0.0, 0.0)));
        let solid = el.name != "ProjectedDecalInstance";
        let to_edge = edges.iter().map(|l| station(l, at).2).fold(f64::MAX, f64::min);
        let half = width * 0.5;
        let on_road = arms.iter().any(|a| station(&a.centre, at).2 < half);
        let stood = if to_edge <= EDGE_BAND {
            Anchor::Edge
        } else if on_road {
            Anchor::Surface
        } else {
            Anchor::Verge
        };

        let own = scene.doc().get(e, ANCHOR_ATTR).and_then(|a| Anchor::parse(&a));
        let mut anchor = own.unwrap_or(stood);
        if solid && anchor == Anchor::Surface {
            notes.push(format!("{key} stands on the road: laid on the verge"));
            anchor = Anchor::Verge;
        }

        if dist > half + verge.unwrap_or(VERGE) * 2.0 {
            notes.push(format!("{key} is {dist:.1} m from the road"));
        }

        // Its axes in the piece's frame against the road's way there (+x of the frame is right of +z).
        let t = tangent_at(&arms[arm].centre, along);
        let n = [t[1], -t[0]];
        let rot = back * w.rotation;
        let parent = el.parent.and_then(|c| els[c].parent).filter(|&g| g != root).map(|g| {
            let pw = world_transform(&scene, g);
            (local(pw.position) * (1.0 / UNIT), back * pw.rotation)
        });
        let (xa, za) = (rot * Double3::new(1.0, 0.0, 0.0), rot * Double3::new(0.0, 0.0, 1.0));
        let dot = |a: Double3, b: [f64; 2]| (a.x * b[0] + a.z * b[1]).abs();
        let turn = (xa.x * n[0] + xa.z * n[1]).atan2(xa.x * t[0] + xa.z * t[1]);
        let size = if solid {
            [0.0, 0.0]
        } else {
            let s = scene.scale(e).unwrap_or(Double3::new(1.0, 1.0, 1.0));
            let (sx, sz) = (s.x.abs() / UNIT, s.z.abs() / UNIT);
            [sx * dot(xa, t) + sz * dot(za, t), sx * dot(xa, n) + sz * dot(za, n)]
        };

        let length = arms[arm].length.max(1e-9);
        parts.push(Part {
            path,
            kind: el.name.clone(),
            key,
            arm,
            along: along / length,
            across,
            up: p.y / UNIT,
            anchor,
            solid,
            size,
            turn,
            at,
            rotation: rot,
            parent,
            scale: scene.scale(e),
        });
    }

    Ok(Piece { arms, ports, width, verge, parts, notes })
}

/// A guide's text, its points in the piece's frame (metres).
fn guide(role: Role, name: &str, line: &[[f64; 2]]) -> Vec<u8> {
    let mut n = NewInstance::new(ROAD_GUIDE).position(Double3::ZERO).attr("name", name).attr(ROLE_ATTR, role.name());
    n.points = line.iter().map(|p| NewPoint::at(Double3::new(p[0] * UNIT, 0.0, p[1] * UNIT))).collect();
    element_text(&n).unwrap_or_default()
}

/// An arc about `c` of radius `r`, from angle `a0` to `a1` (radians; 0 towards -x, turning towards +z), every 2.5 degrees.
fn arc(c: [f64; 2], r: f64, a0: f64, a1: f64) -> Vec<[f64; 2]> {
    let n = (((a1 - a0).abs() / 2.5f64.to_radians()).ceil() as usize).max(1);
    (0..=n)
        .map(|i| {
            let a = a0 + (a1 - a0) * i as f64 / n as f64;
            [c[0] - r * a.cos(), c[1] + r * a.sin()]
        })
        .collect()
}

/// The guides of a base piece, at the standard measures.
#[must_use]
pub fn base_guides(piece: &str) -> Vec<Vec<u8>> {
    if let Some(p) = varies(piece) {
        return base_guides(p);
    }

    let (h, v, j) = (WIDTH * 0.5, WIDTH * 0.5 + VERGE, JUNCTION * 0.5);
    let straight = |l: f64| {
        let z = l * 0.5;
        vec![
            guide(Role::Centre, "centre", &[[0.0, -z], [0.0, z]]),
            guide(Role::Edge, "edge left", &[[-h, -z], [-h, z]]),
            guide(Role::Edge, "edge right", &[[h, -z], [h, z]]),
            guide(Role::Verge, "verge left", &[[-v, -z], [-v, z]]),
            guide(Role::Verge, "verge right", &[[v, -z], [v, z]]),
            guide(Role::Port, "port back", &[[-h, -z], [h, -z]]),
            guide(Role::Port, "port front", &[[-h, z], [h, z]]),
        ]
    };

    match piece {
        "straight" | "ford" => straight(STRAIGHT),
        "fence" => {
            // A fence's line, no verge: its centre and its two joins.
            let (z, w) = (FENCE * 0.5, FENCE_WIDTH * 0.5);
            vec![
                guide(Role::Centre, "centre", &[[0.0, -z], [0.0, z]]),
                guide(Role::Edge, "edge left", &[[-w, -z], [-w, z]]),
                guide(Role::Edge, "edge right", &[[w, -z], [w, z]]),
                guide(Role::Port, "port back", &[[-w, -z], [w, -z]]),
                guide(Role::Port, "port front", &[[-w, z], [w, z]]),
            ]
        }
        "end" => {
            // The road stops at z = -2: a port at the front only, the edges closed across the back.
            let z = END * 0.5;
            vec![
                guide(Role::Centre, "centre", &[[0.0, -z], [0.0, z]]),
                guide(Role::Edge, "edge", &[[-h, z], [-h, -z], [h, -z], [h, z]]),
                guide(Role::Verge, "verge", &[[-v, z], [-v, -z - VERGE], [v, -z - VERGE], [v, z]]),
                guide(Role::Port, "port front", &[[-h, z], [h, z]]),
            ]
        }

        "curve" => {
            // Turning right (towards +x) about (R, 0), the piece's middle on its origin heading +z.
            let (c, r, a) = ([CURVE_RADIUS, 0.0], CURVE_RADIUS, CURVE_TURN * 0.5);
            let at = |rr: f64, t: f64| [c[0] - rr * t.cos(), c[1] + rr * t.sin()];
            vec![
                guide(Role::Centre, "centre", &arc(c, r, -a, a)),
                guide(Role::Edge, "edge left", &arc(c, r + h, -a, a)),
                guide(Role::Edge, "edge right", &arc(c, r - h, -a, a)),
                guide(Role::Verge, "verge left", &arc(c, r + v, -a, a)),
                guide(Role::Verge, "verge right", &arc(c, r - v, -a, a)),
                guide(Role::Port, "port back", &[at(r + h, -a), at(r - h, -a)]),
                guide(Role::Port, "port front", &[at(r + h, a), at(r - h, a)]),
            ]
        }

        "corner" => vec![
            // From the back (-z) turning right (+x) at the origin.
            guide(Role::Centre, "centre back", &[[0.0, 0.0], [0.0, -j]]),
            guide(Role::Centre, "centre right", &[[0.0, 0.0], [j, 0.0]]),
            guide(Role::Edge, "edge outer", &[[-h, -j], [-h, h], [j, h]]),
            guide(Role::Edge, "edge inner", &[[h, -j], [h, -h], [j, -h]]),
            guide(Role::Verge, "verge outer", &[[-v, -j], [-v, v], [j, v]]),
            guide(Role::Port, "port back", &[[-h, -j], [h, -j]]),
            guide(Role::Port, "port right", &[[j, h], [j, -h]]),
        ],

        "t_junction" => vec![
            // The through road along z, the branch to the right (+x).
            guide(Role::Centre, "centre back", &[[0.0, 0.0], [0.0, -j]]),
            guide(Role::Centre, "centre front", &[[0.0, 0.0], [0.0, j]]),
            guide(Role::Centre, "centre right", &[[0.0, 0.0], [j, 0.0]]),
            guide(Role::Edge, "edge left", &[[-h, -j], [-h, j]]),
            guide(Role::Edge, "edge back right", &[[h, -j], [h, -h], [j, -h]]),
            guide(Role::Edge, "edge front right", &[[j, h], [h, h], [h, j]]),
            guide(Role::Verge, "verge left", &[[-v, -j], [-v, j]]),
            guide(Role::Port, "port back", &[[-h, -j], [h, -j]]),
            guide(Role::Port, "port front", &[[-h, j], [h, j]]),
            guide(Role::Port, "port right", &[[j, h], [j, -h]]),
        ],

        "x_junction" => vec![
            guide(Role::Centre, "centre back", &[[0.0, 0.0], [0.0, -j]]),
            guide(Role::Centre, "centre front", &[[0.0, 0.0], [0.0, j]]),
            guide(Role::Centre, "centre left", &[[0.0, 0.0], [-j, 0.0]]),
            guide(Role::Centre, "centre right", &[[0.0, 0.0], [j, 0.0]]),
            guide(Role::Edge, "edge back right", &[[h, -j], [h, -h], [j, -h]]),
            guide(Role::Edge, "edge front right", &[[j, h], [h, h], [h, j]]),
            guide(Role::Edge, "edge front left", &[[-h, j], [-h, h], [-j, h]]),
            guide(Role::Edge, "edge back left", &[[-j, -h], [-h, -h], [-h, -j]]),
            guide(Role::Port, "port back", &[[-h, -j], [h, -j]]),
            guide(Role::Port, "port front", &[[-h, j], [h, j]]),
            guide(Role::Port, "port left", &[[-j, -h], [-j, h]]),
            guide(Role::Port, "port right", &[[j, h], [j, -h]]),
        ],

        _ => vec![],
    }
}

/// A piece's group: `children` its contents' texts (at column 0).
#[must_use]
pub fn group_text(key: &str, children: &[Vec<u8>]) -> Vec<u8> {
    let mut out = format!("<UserInstanceGroup flags=\" OIF_VISIBLE|OIF_ACTIVE|OIF_DRAGGABLE\" position=\"vector(0,0,0)\" name=\"{key}\" orientation=\"euler(0,0,0)\">\n    <children>\n").into_bytes();
    for child in children {
        for line in child.split_inclusive(|&c| c == b'\n') {
            out.extend_from_slice(b"        ");
            out.extend_from_slice(line);
        }
    }

    out.extend_from_slice(b"    </children>\n</UserInstanceGroup>\n");
    out
}

/// A generated piece's group keeps what its contents hashed to when they were written: while they still do, nobody edited the piece,
/// and a newer generation may take its place.
pub const GENERATED_ATTR: &str = "wce_generated";

fn hash(bytes: &[u8]) -> String {
    format!("{:016x}", bytes.iter().fold(0xCBF2_9CE4_8422_2325u64, |h, &b| (h ^ u64::from(b)).wrapping_mul(0x0100_0000_01B3)))
}

/// A group's contents, as a prefab file holding it reads them back.
fn contents_of(group_text: &[u8]) -> Option<Vec<u8>> {
    let scene = TedScene::parse_any(crate::prefabs::file_text(group_text)).ok()?;
    let group = *scene.find_all("UserInstanceGroup").first()?;
    Some(scene.children_text(group))
}

/// What the generations written before the mark existed hashed to (2026-11-08, R1's base pieces and R2's dirt ones): such a file without
/// the mark is still ours, unedited.
const UNMARKED_GENERATIONS: [&str; 13] = [
    "0ea06c28e2cbf9f1",
    "a119a0807ad50a77",
    "ae8682a019e54c98",
    "b28b133b4107e64f",
    "2de3689d6c29c133",
    "efc5d95f569d98db",
    "b4d97e3924d340e1",
    "7c0571253d797e06",
    "1290d469aa26f852",
    "d782de44fe71c570",
    "a361417b204538bf",
    "01b363212a0a2570",
    "122c4720a8834886",
];

/// Whether a generated piece (`children`, its contents) is to be written at `path`: it is missing, or it is an older generation that
/// nobody edited since (its contents still hash to its mark, or, written before the mark, to [`UNMARKED_GENERATIONS`]). A piece without
/// the mark otherwise, or edited, is the user's.
fn to_write(path: &Path, key: &str, children: &[Vec<u8>]) -> Option<Vec<u8>> {
    let fresh = contents_of(&group_text(key, children))?;
    if path.exists() {
        let on_disk = crate::prefabs::load(path).ok()?;
        let scene = TedScene::parse_any(on_disk.group_text).ok()?;
        let now = hash(&on_disk.children_text);
        let unedited = match scene.doc().get(0, GENERATED_ATTR) {
            Some(mark) => mark == now,
            None => UNMARKED_GENERATIONS.contains(&now.as_str()),
        };

        if !unedited || on_disk.children_text == fresh {
            return None;
        }
    }

    let text = group_text(key, children);
    let at = text.iter().position(|&c| c == b' ')?;
    let mut marked = text[..at].to_vec();
    marked.extend_from_slice(format!(" {GENERATED_ATTR}=\"{}\"", hash(&fresh)).as_bytes());
    marked.extend_from_slice(&text[at..]);
    Some(marked)
}

/// Writes generated pieces of a road type (each piece's name and contents) into a prefab folder (the projects'), where [`to_write`]
/// says so. Returns the files written.
///
/// # Errors
///
/// A file can't be written.
pub fn write_generated(prefab_root: &Path, road_type: &str, pieces: &[(&str, Vec<Vec<u8>>)]) -> Result<Vec<PathBuf>, String> {
    let dir = prefab_root.join(FOLDER).join(road_type);
    let mut written = vec![];
    for (piece, children) in pieces {
        let key = key(road_type, piece);
        let path = dir.join(format!("{key}.wce"));
        if let Some(text) = to_write(&path, &key, children) {
            crate::prefabs::write(&path, &text, None)?;
            written.push(path);
        }
    }

    Ok(written)
}

/// Whether [`write_generated`] would write any of these pieces.
#[must_use]
pub fn generated_stale(prefab_root: &Path, road_type: &str, pieces: &[(&str, Vec<Vec<u8>>)]) -> bool {
    let dir = prefab_root.join(FOLDER).join(road_type);
    pieces.iter().any(|(piece, children)| {
        let key = key(road_type, piece);
        to_write(&dir.join(format!("{key}.wce")), &key, children).is_some()
    })
}

/// The base pieces' contents.
#[must_use]
pub fn base_pieces() -> Vec<(&'static str, Vec<Vec<u8>>)> {
    PIECES.iter().map(|&p| (p, base_guides(p))).collect()
}

/// Writes the base pieces into a prefab folder (the projects'): each one where it is missing, or an older generation nobody edited. What
/// the user made of one is kept. Returns the files written.
///
/// # Errors
///
/// A file can't be written.
pub fn write_base(prefab_root: &Path) -> Result<Vec<PathBuf>, String> {
    write_generated(prefab_root, BASE, &base_pieces())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn base(piece: &str) -> Piece {
        read(&group_text(&key(BASE, piece), &base_guides(piece))).unwrap()
    }

    fn near(a: f64, b: f64) -> bool {
        (a - b).abs() < 1e-6
    }

    #[test]
    fn every_base_piece_reads_back_as_its_measures() {
        let arms = |p: &Piece| p.arms.iter().map(|a| (a.start.is_some(), a.end.is_some())).collect::<Vec<_>>();
        for piece in PIECES {
            let p = base(piece);
            assert!(near(p.width, if piece == "fence" { FENCE_WIDTH } else { WIDTH }), "{piece}: {}", p.width);
            assert!(p.notes.is_empty() && p.parts.is_empty(), "{piece}: {p:?}");
            if piece != "x_junction" && piece != "fence" {
                assert!(p.verge.is_some_and(|v| (v - VERGE).abs() < 0.2), "{piece}: {:?}", p.verge);
            }

            match piece {
                "straight" | "ford" => {
                    assert_eq!(arms(&p), [(true, true)]);
                    assert!(near(p.arms[0].length, STRAIGHT));
                }

                "fence" => {
                    assert_eq!(arms(&p), [(true, true)]);
                    assert!(near(p.arms[0].length, FENCE));
                }

                "end" => {
                    assert_eq!(arms(&p), [(false, true)]);
                    assert!(near(p.arms[0].length, END));
                }

                "curve" => {
                    assert_eq!(arms(&p), [(true, true)]);
                    assert!((p.arms[0].length - CURVE_RADIUS * CURVE_TURN).abs() < 0.05, "{}", p.arms[0].length);
                    let [a, b] = [p.ports[0].out, p.ports[1].out];
                    let turn = (a[0] * b[0] + a[1] * b[1]).clamp(-1.0, 1.0).acos();
                    assert!((std::f64::consts::PI - turn - CURVE_TURN).abs() < 1e-3, "{turn}");
                }

                "corner" => assert_eq!(arms(&p), [(false, true); 2]),
                "t_junction" => assert_eq!(arms(&p), [(false, true); 3]),
                "x_junction" => assert_eq!(arms(&p), [(false, true); 4]),
                _ => unreachable!(),
            }

            for port in &p.ports {
                assert!(near(len(port.out), 1.0));
                // A port faces away from the piece's middle.
                assert!(port.at[0] * port.out[0] + port.at[1] * port.out[1] > 0.0, "{piece}: {port:?}");
            }
        }
    }

    #[test]
    fn a_corners_ports_turn_a_right_angle() {
        let p = base("corner");
        let outs = p.ports.iter().map(|q| q.out).collect::<Vec<_>>();
        assert!(outs.contains(&[0.0, -1.0]) && outs.contains(&[1.0, 0.0]), "{outs:?}");
    }

    #[test]
    fn parts_are_anchored_by_where_they_stand() {
        let decal = |x: f64, z: f64| {
            format!(
            "<ProjectedDecalInstance flags=\"\" scale=\"vector(4,10,8)\" position=\"vector({},0,{})\" key=\"d\" name=\"d\" orientation=\"euler(0,0,0)\"/>\n",
            x * UNIT, z * UNIT
        ).into_bytes()
        };
        let mut children = base_guides("straight");
        children.push(decal(0.0, 2.0));
        children.push(decal(2.6, -1.0));
        children.push(decal(-4.5, 0.0));
        children.push(
            b"<BUILDING_INSTANCE flags=\"\" position=\"vector(1,0,0)\" key=\"fence\" name=\"f\" orientation=\"euler(0,0,0)\"/>\n".to_vec(),
        );
        children.push(b"<BUILDING_INSTANCE flags=\"\" position=\"vector(0,0,0)\" key=\"post\" name=\"p\" road_anchor=\"fixed\" orientation=\"euler(0,0,0)\"/>\n".to_vec());
        let p = read(&group_text("road_test_straight", &children)).unwrap();
        let anchors = p.parts.iter().map(|q| q.anchor).collect::<Vec<_>>();
        assert_eq!(anchors, [Anchor::Surface, Anchor::Edge, Anchor::Verge, Anchor::Verge, Anchor::Fixed]);
        assert!(near(p.parts[0].along, 0.75) && near(p.parts[0].across, 0.0), "{:?}", p.parts[0]);
        assert!(near(p.parts[1].across, 2.6) && near(p.parts[2].across, -4.5), "{:?}", p.parts);
        assert_eq!(p.parts[0].size, [4.0, 2.0]);
        assert_eq!(p.notes.len(), 1, "{:?}", p.notes);

        // Each part's path finds it again in the contents.
        let scene = TedScene::parse_any(group_text("road_test_straight", &children)).unwrap();
        for part in &p.parts {
            let e = crate::prefab_refs::element_at(scene.doc(), 0, &part.path).unwrap();
            assert_eq!(scene.doc().get(e, "key").unwrap(), part.key);
        }
    }

    #[test]
    fn a_piece_without_guides_is_not_one() {
        assert!(read(&group_text("p", &[])).is_err());
        assert!(read(&group_text("p", &[guide(Role::Centre, "c", &[[0.0, 0.0], [0.0, 4.0]])])).is_err());
    }

    #[test]
    fn the_base_is_written_once_and_kept() {
        let dir = tempfile::tempdir().unwrap();
        let written = write_base(dir.path()).unwrap();
        assert_eq!(written.len(), PIECES.len());
        let straight = dir.path().join(FOLDER).join(BASE).join("road_base_straight.wce");
        std::fs::write(&straight, b"mine").unwrap();
        assert!(write_base(dir.path()).unwrap().is_empty());
        assert_eq!(std::fs::read(&straight).unwrap(), b"mine");

        // What was written reads as a prefab, and as its piece.
        let lib = crate::prefabs::PrefabLibrary::scan_folders(&[dir.path().to_path_buf()]);
        let corner = lib.load("road_base_corner").unwrap();
        assert_eq!(read(&corner.group_text).unwrap().arms.len(), 2);
        assert_eq!(lib.get("road_base_corner").unwrap().folder, "roads_proc/base");
    }

    #[test]
    fn an_older_generation_nobody_edited_is_replaced_and_an_edited_one_kept() {
        let dir = tempfile::tempdir().unwrap();
        let old = vec![(
            "straight",
            vec![guide(Role::Centre, "c", &[[0.0, -4.0], [0.0, 4.0]]), guide(Role::Port, "p", &[[-3.0, 4.0], [3.0, 4.0]])],
        )];
        assert_eq!(write_generated(dir.path(), "t", &old).unwrap().len(), 1);
        assert!(write_generated(dir.path(), "t", &old).unwrap().is_empty(), "the same generation is not written again");

        // A newer generation takes the place of the unedited one.
        let new = vec![("straight", base_guides("straight"))];
        assert!(generated_stale(dir.path(), "t", &new));
        assert_eq!(write_generated(dir.path(), "t", &new).unwrap().len(), 1);
        let path = dir.path().join(FOLDER).join("t").join("road_t_straight.wce");
        assert_eq!(read(&crate::prefabs::load(&path).unwrap().group_text).unwrap().arms.len(), 1);

        // Edited (its contents no longer what its mark says): kept.
        crate::prefabs::update_children(&path, b"<WceRoadGuide guide_role=\"centre\"/>\n").unwrap();
        assert!(!generated_stale(dir.path(), "t", &old));
        assert!(write_generated(dir.path(), "t", &old).unwrap().is_empty());

        // A file without the mark is the user's.
        let mine = dir.path().join(FOLDER).join("t");
        std::fs::write(mine.join("road_t_straight.wce"), crate::prefabs::file_text(&group_text("road_t_straight", &[]))).unwrap();
        assert!(write_generated(dir.path(), "t", &new).unwrap().is_empty());
    }

    #[test]
    fn a_kit_save_leaves_the_guides_out() {
        let (ted, report) =
            crate::project_files::to_ted(crate::prefabs::file_text(&group_text("road_base_end", &base_guides("end")))).unwrap();
        assert!(!String::from_utf8_lossy(&ted).contains(ROAD_GUIDE), "{}", String::from_utf8_lossy(&ted));
        assert!(!report.dropped.is_empty());
    }
}
