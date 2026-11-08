//! Generated bridges (docs/plans/procedural_roads.md, "Bridges"): a kit bridge building bent along a road's deck, on the walls' warp. A
//! bridge over a road's gap is cut into pieces about as long as its base, each one a building of its own made from the base's files:
//! its meshes, its logic file (platforms, collision hull: the walls' welding, merging and winding come with [`crate::logic::warp_logic`])
//! and its zones, bent along the road's curve, sloped with it, widened to it, and its posts stretched down to the ground under them.
//!
//! The kit's bridges run along their z; the walls' cuts and warps run along x. A base is turned a quarter first ([`QuarterTurn`]), then
//! bent ([`BridgeWarp`]). Scene units throughout (the models' and the tile's file units: two a metre in a battle tile).

use std::cell::RefCell;
use std::io::Cursor;

use rpfm_lib::files::{Decodeable, Encodeable, cs2_parsed::Cs2Parsed};
use serde_json::Value;

use crate::building::{BuildingFiles, is_piece_mesh};
use crate::warp::{Cuts, Warp};

/// The element a road's bridge goes into an export as, to be made here (`export::lower_walls`): its deck's centre line as points
/// (scene units, absolute: x, the deck's surface, z), its base (`bridge_base`) and its width (`bridge_width`, scene units).
pub const BRIDGE_SPAN: &str = "WceBridgeSpan";
pub const BASE_ATTR: &str = "bridge_base";
pub const WIDTH_ATTR: &str = "bridge_width";

/// A kit building a bridge is made from: its name in a road's settings and its key.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BridgeBase {
    pub name: &'static str,
    pub key: &'static str,

    /// The kit's end piece of the base, laid at each end of the deck (a ramp down from it: `bridge_wood_2_end`'s platform drops 1 unit
    /// over 2), if it has one.
    pub end: Option<&'static str>,
}

/// The bases (Attila's kit). `bridge_wood_2`: a 20 x 10 deck on posts, one platform at 1. The wooden one alone for now (user,
/// 2026-11-08): `bridge_stone_1` is a humped bridge of its own, and cut into pieces it repeated its hump and its arch (its deck in steps);
/// other bases wait for a standard shape a base is made to. A road asking for one it does not have gets the first.
pub const BASES: [BridgeBase; 1] = [BridgeBase { name: "wood", key: "bridge_wood_2", end: Some("bridge_wood_2_end") }];

/// A base by its key or its end piece's key.
#[must_use]
pub fn base_of_key(key: &str) -> Option<BridgeBase> {
    BASES.iter().copied().find(|b| b.key == key || b.end == Some(key))
}

/// What a piece of a bridge gets at its ends besides its base's platforms.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Ends {
    /// A base without an end piece: at the deck's ends a ramp platform and a ground platform past it, the footprint over the ramp.
    Ramps,

    /// With end pieces: nothing, the pieces as the kit has them. A split bridge laid by hand from the kit's pieces (`bridge_wood_2` and
    /// two `bridge_wood_2_end`, the deck 1 unit over the ground and each end's ramp from it down to the ground) is crossed; with a ground
    /// platform added past each end piece, the hulls lowered and the outlines put on the terrain, ours was not (user 2026-11-08).
    Plain,

    /// An end piece with end pieces: as `Plain`, its footprint (collision outlines, `orange_thingies`) made again from its platform's
    /// corners (`footprint_from_platform`).
    EndPiece,
}

/// A piece of a bridge: an end piece or not, the line it is laid along (the deck's, or its own from past the deck's end to it), its
/// stretch of that line and where it stands.
#[derive(Clone, Debug)]
pub struct Part {
    pub end: bool,
    pub deck: Deck,
    pub s: (f64, f64),
    pub origin: [f64; 3],
}

impl Part {
    #[must_use]
    pub fn ends(&self, with_end_pieces: bool) -> Ends {
        match (with_end_pieces, self.end) {
            (false, _) => Ends::Ramps,
            (true, true) => Ends::EndPiece,
            (true, false) => Ends::Plain,
        }
    }
}

/// A bridge's pieces: its deck's ([`piece_stretches`]), then, with an end piece, one past each end of the deck (on a line of its own
/// from as far out as the end piece is long to the deck's end, level with it: the end piece's own slope takes it down).
#[must_use]
pub fn parts(deck: &Deck, base: &Prepared, end: Option<&Prepared>) -> Vec<Part> {
    let mut out = piece_stretches(deck, base).into_iter().map(|(s, origin)| Part { end: false, deck: deck.clone(), s, origin }).collect::<Vec<_>>();
    if let Some(e) = end {
        let l = (e.x.1 - e.x.0).max(1e-6);
        for (at, away) in [(0.0, -1.0), (deck.length(), 1.0)] {
            let (c, t) = deck.at(at);
            let outer = [c[0] + t[0] * away * l, c[1], c[2] + t[1] * away * l];
            let d = Deck::new(vec![outer, c]);
            let len = d.length();
            out.push(Part { end: true, origin: d.at(len * 0.5).0, s: (0.0, len), deck: d });
        }
    }

    out
}

/// A base by its name or its key.
#[must_use]
pub fn base(name: &str) -> Option<BridgeBase> {
    BASES.iter().copied().find(|b| b.name == name || b.key == name)
}

/// How far apart the warp cuts a base's meshes and outlines along it (scene units): the curve and the slope are followed that closely.
const STEP: f32 = 2.0;

/// How far a post goes into the ground (scene units).
const BURY: f64 = 0.5;

/// How long a bridge's ground platform is past each end of its deck (scene units: 3 m), as the kit's `bridge_wood_complete` has them (6
/// units from its ramps down to the terrain, `flag_2` set: the editor draws them as ground). Without them units did not leave a generated
/// bridge (user 2026-11-08: the far end red-crossed, units went round its sides).
pub const GROUND_PLATFORM: f64 = 6.0;

/// A bridge's ends as the kit's `bridge_wood_complete`'s (scene units): a plain ramp from the deck `END_RAMP` long down to `RAMP_FOOT`
/// over the ground, then the ground platform ([`GROUND_PLATFORM`] long, `flag_2`) down to `GROUND_PLATFORM_BURY` under it (the kit's:
/// a 2-unit ramp from 1.2 to 0.2, its ground platforms 6 units to 3.4 and 3.7 under). Ours rises further (the deck 1 m over the road:
/// `DECK_LIFT` in the editor's roads), so its ramp is longer.
pub const END_RAMP: f64 = 4.0;
pub const RAMP_FOOT: f64 = 0.2;
pub const GROUND_PLATFORM_BURY: f64 = 3.5;

/// The base turned a quarter about y: its +z onto +x, its +x onto -z (a turn, not a mirror).
pub struct QuarterTurn;

impl Warp for QuarterTurn {
    fn point(&self, [x, y, z]: [f32; 3]) -> [f32; 3] {
        [z, y, -x]
    }

    fn yaw(&self, _: [f32; 3]) -> f32 {
        -std::f32::consts::FRAC_PI_2
    }
}

/// A warp that only notes the extent of what it moves (and moves nothing).
struct Extent(RefCell<([f32; 3], [f32; 3])>);

impl Warp for Extent {
    fn point(&self, p: [f32; 3]) -> [f32; 3] {
        let mut e = self.0.borrow_mut();
        for (k, v) in p.iter().enumerate() {
            e.0[k] = e.0[k].min(*v);
            e.1[k] = e.1[k].max(*v);
        }

        p
    }

    fn yaw(&self, _: [f32; 3]) -> f32 {
        0.0
    }
}

/// A base made ready to bend: turned to run along x, and measured.
#[derive(Clone, Debug)]
pub struct Prepared {
    pub files: BuildingFiles,

    /// Its intact meshes' extent along x and across (z), and their lowest point.
    pub x: (f64, f64),
    pub z: (f64, f64),
    pub bottom: f64,

    /// The height of what is walked on: its platforms', else its deck's top.
    pub deck: f64,

    /// Whether its platform was made here (the base had none).
    pub made_platform: bool,
}

fn decode(bytes: &[u8]) -> Result<Value, String> {
    let parsed = Cs2Parsed::decode(&mut Cursor::new(bytes), &None).map_err(|e| format!("cs2.parsed: {e}"))?;
    serde_json::to_value(&parsed).map_err(|e| e.to_string())
}

fn encode(v: Value) -> Result<Vec<u8>, String> {
    let mut parsed: Cs2Parsed = serde_json::from_value(v).map_err(|e| e.to_string())?;
    let mut out = vec![];
    parsed.encode(&mut out, &None).map_err(|e| format!("cs2.parsed: {e}"))?;
    Ok(out)
}

/// Every platform of a logic file's intact levels (each destruct 0's), as their outlines' points.
fn intact_platform_points(v: &Value) -> Vec<[f64; 3]> {
    let mut out = vec![];
    for piece in v["pieces"].as_array().into_iter().flatten() {
        let Some(d) = piece["destructs"].as_array().and_then(|d| d.first()) else { continue };
        for p in d["platforms"].as_array().into_iter().flatten() {
            for q in p["vertices"]["outline"].as_array().into_iter().flatten() {
                let f = |k: &str| q[k].as_f64().unwrap_or(0.0);
                out.push([f("x"), f("y"), f("z")]);
            }
        }
    }

    out
}

/// One platform (a quad over x and z at height y, its corners round as the game's: clockwise from above).
fn platform(x: (f64, f64), z: (f64, f64), y: f64) -> Value {
    serde_json::json!({ "flag_1": false, "flag_2": false, "flag_3": false, "normal": { "x": 0.0, "y": 1.0, "z": 0.0 }, "vertices": { "outline": [
        { "x": x.0, "y": y, "z": z.1 }, { "x": x.1, "y": y, "z": z.1 }, { "x": x.1, "y": y, "z": z.0 }, { "x": x.0, "y": y, "z": z.0 } ] } })
}

/// Turns a base to run along x and measures it; a base without platforms gets one over its deck (the widest upward face along its
/// middle, `mesh::treads`), across 0.8 of its width.
///
/// # Errors
///
/// A file of the base doesn't read or write back, or it has no intact mesh.
pub fn prepare(base: &BuildingFiles) -> Result<Prepared, String> {
    let mut files = base.clone().without_collapses();
    let cuts = Cuts::every(1.0e6);
    let logic_name = files.logic_name();
    let zones_name = files.zones_name();
    for (name, bytes) in &mut files.files {
        if is_piece_mesh(name) {
            *bytes = crate::mesh::warp_mesh(bytes, &QuarterTurn, &cuts)?;
        } else if *name == logic_name {
            *bytes = crate::logic::warp_logic(bytes, &QuarterTurn, &cuts)?.0;
        } else if *name == zones_name {
            *bytes = crate::zones::warp_zones(&String::from_utf8_lossy(bytes), &QuarterTurn, 1.0e6)?.into_bytes();
        }
    }

    // The intact meshes' extent.
    let intact = files.files.iter().filter(|(n, _)| is_piece_mesh(n) && crate::pieces::level_of(n) <= 1).map(|(_, b)| b.clone()).collect::<Vec<_>>();
    if intact.is_empty() {
        return Err(format!("{}: no intact mesh", base.key));
    }

    let extent = Extent(RefCell::new(([f32::MAX; 3], [f32::MIN; 3])));
    for m in &intact {
        crate::mesh::warp_mesh(m, &extent, &cuts)?;
    }

    let (lo, hi) = extent.0.into_inner();
    let (x, z, bottom) = ((f64::from(lo[0]), f64::from(hi[0])), (f64::from(lo[2]), f64::from(hi[2])), f64::from(lo[1]));

    // What is walked on: the platforms, else the deck's top, and a platform made there.
    let mut made_platform = false;
    let mut deck = f64::from(hi[1]);
    if let Some(bytes) = files.files.get_mut(&logic_name) {
        let mut v = decode(bytes)?;
        let points = intact_platform_points(&v);
        if points.is_empty() {
            let meshes = intact.iter().map(Vec::as_slice).collect::<Vec<_>>();
            let middle = |a: f64, b: f64, part: f64| ((a + (b - a) * (0.5 - part)) as f32, (a + (b - a) * (0.5 + part)) as f32);
            let treads = crate::mesh::treads(&meshes, middle(x.0, x.1, 0.25), middle(z.0, z.1, 0.2));
            if let Some(&(y, _, _)) = treads.iter().max_by(|a, b| (a.2 - a.1).total_cmp(&(b.2 - b.1))) {
                deck = f64::from(y);
            }

            let half = (z.1 - z.0) * 0.4;
            let mid = (z.0 + z.1) * 0.5;
            // On the first piece's intact level.
            let first = v["pieces"].as_array_mut().and_then(|p| p.first_mut()).and_then(|piece| piece["destructs"].as_array_mut()).and_then(|d| d.first_mut());
            if let Some(d) = first {
                if !d["platforms"].is_array() {
                    d["platforms"] = Value::Array(vec![]);
                }

                if let Some(ps) = d["platforms"].as_array_mut() {
                    ps.push(platform(x, (mid - half, mid + half), deck));
                    made_platform = true;
                }
            }

            *bytes = encode(v)?;
        } else {
            deck = points.iter().map(|p| p[1]).fold(f64::MIN, f64::max);
        }
    }

    // Its length is where its walkway ends (as a wall piece's joint: `logic::joint_extent_x`), so that two pieces' platforms meet: its
    // meshes reach a little past them (bridge_wood_2's -10.02..10.01 for its platform's -10..10).
    let x = match files.files.get(&logic_name).map(|l| crate::logic::joint_extent_x(l)) {
        Some(Ok((a, b))) if b > a => (f64::from(a), f64::from(b)),
        _ => x,
    };

    Ok(Prepared { files, x, z, bottom, deck, made_platform })
}

/// A bridge's deck: its centre line (scene units: x, the deck's surface, z) and its distances along it.
#[derive(Clone, Debug, PartialEq)]
pub struct Deck {
    pub path: Vec<[f64; 3]>,
    along: Vec<f64>,
}

impl Deck {
    #[must_use]
    pub fn new(path: Vec<[f64; 3]>) -> Self {
        let mut along = vec![0.0; path.len()];
        for i in 1..path.len() {
            along[i] = along[i - 1] + (path[i][0] - path[i - 1][0]).hypot(path[i][2] - path[i - 1][2]);
        }

        Self { path, along }
    }

    #[must_use]
    pub fn length(&self) -> f64 {
        self.along.last().copied().unwrap_or(0.0)
    }

    /// The deck's middle at a distance along it (x, the surface's height, z), and its way there (x, z, unit).
    #[must_use]
    pub fn at(&self, s: f64) -> ([f64; 3], [f64; 2]) {
        let n = self.path.len();
        if n < 2 {
            return (self.path.first().copied().unwrap_or([0.0; 3]), [1.0, 0.0]);
        }

        let i = self.along.partition_point(|&d| d <= s).clamp(1, n - 1);
        let (a, b) = (self.path[i - 1], self.path[i]);
        let span = (self.along[i] - self.along[i - 1]).max(1e-9);
        let f = (s - self.along[i - 1]) / span;
        let d = [b[0] - a[0], b[2] - a[2]];
        let l = d[0].hypot(d[1]).max(1e-9);
        ([a[0] + (b[0] - a[0]) * f, a[1] + (b[1] - a[1]) * f, a[2] + (b[2] - a[2]) * f], [d[0] / l, d[1] / l])
    }
}

/// A prepared base onto one piece of a bridge: its x onto the deck's stretch `s`, its z across, widened by `across`, its walked-on height
/// onto the deck's surface; what is under its `foot` (its posts) stretched down to the ground there. Points come out relative to the
/// piece's `origin` (where its building stands, turned by nothing).
pub struct BridgeWarp<'a> {
    pub deck: &'a Deck,
    pub base: &'a Prepared,
    pub s: (f64, f64),
    pub across: f64,
    pub origin: [f64; 3],
    pub ground: &'a dyn Fn(f64, f64) -> f64,
}

impl BridgeWarp<'_> {
    /// Below this the base is its posts: they reach down to the ground (a metre under its walked-on height).
    fn foot(&self) -> f64 {
        (self.base.deck - 2.0).max(self.base.bottom)
    }

    fn s_of(&self, x: f64) -> f64 {
        let (x0, x1) = self.base.x;
        self.s.0 + (x - x0) / (x1 - x0).max(1e-9) * (self.s.1 - self.s.0)
    }
}

impl Warp for BridgeWarp<'_> {
    fn point(&self, p: [f32; 3]) -> [f32; 3] {
        let [x, y, z] = p.map(f64::from);
        let (c, t) = self.deck.at(self.s_of(x));
        let n = [-t[1], t[0]];
        let (qx, qz) = (c[0] + n[0] * z * self.across, c[2] + n[1] * z * self.across);
        let (deck, foot, bottom) = (self.base.deck, self.foot(), self.base.bottom);
        let top = c[1] + (foot - deck);
        let qy = if y >= foot || foot - bottom < 1e-6 {
            c[1] + (y - deck)
        } else {
            // The posts: from their foot down to the ground, wherever it is.
            let g = (self.ground)(qx, qz) - BURY;
            if g >= top { c[1] + (y - deck) } else { g + (y - bottom) / (foot - bottom) * (top - g) }
        };

        [(qx - self.origin[0]) as f32, (qy - self.origin[1]) as f32, (qz - self.origin[2]) as f32]
    }

    fn yaw(&self, p: [f32; 3]) -> f32 {
        let (_, t) = self.deck.at(self.s_of(f64::from(p[0])));
        t[1].atan2(t[0]) as f32
    }
}

/// A bridge's pieces: its deck cut into about as many as its base's length goes into it, each with its stretch and where it stands (its
/// stretch's middle on the deck).
#[must_use]
pub fn piece_stretches(deck: &Deck, base: &Prepared) -> Vec<((f64, f64), [f64; 3])> {
    let len = deck.length();
    let own = (base.x.1 - base.x.0).max(1e-6);
    let n = ((len / own).round() as usize).max(1);
    (0..n).map(|k| {
        let s = (len * k as f64 / n as f64, len * (k + 1) as f64 / n as f64);
        (s, deck.at((s.0 + s.1) * 0.5).0)
    }).collect()
}

/// A piece made from a prepared base under a new key, bent by its warp: its meshes (every level), its logic file and its zones.
///
/// # Errors
///
/// A file doesn't read or write back.
pub fn make_piece(base: &Prepared, key: &str, warp: &BridgeWarp, ends: Ends) -> Result<BuildingFiles, String> {
    let mut b = base.files.clone().renamed(key);
    let (logic_name, zones_name) = (b.logic_name(), b.zones_name());
    let cuts = Cuts::every(STEP);
    let mut drop = vec![];
    for (name, bytes) in &mut b.files {
        if is_piece_mesh(name) {
            *bytes = crate::mesh::warp_mesh(bytes, warp, &cuts)?;
        } else if *name == logic_name {
            let warped = crate::logic::warp_logic(bytes, warp, &cuts)?.0;
            let made = without_inline_corners(&with_ground_platforms(&warped, base, warp, ends)?)?;
            *bytes = if ends == Ends::EndPiece { footprint_from_platform(&made)? } else { made };
        } else if *name == zones_name {
            *bytes = crate::zones::warp_zones(&String::from_utf8_lossy(bytes), warp, STEP)?.into_bytes();
        } else if !name.ends_with(".meta") {
            drop.push(name.clone());
        }
    }

    for n in drop {
        b.files.remove(&n);
    }

    Ok(b)
}

/// A piece's logic file with a ground platform past each end of the deck it has (the first piece's start, the last one's end): from the
/// deck's end edge (its corners the deck platform's there) [`GROUND_PLATFORM`] on along the road, down through the ground to
/// [`GROUND_PLATFORM_BURY`] under it, `flag_2` set.
fn with_ground_platforms(logic: &[u8], base: &Prepared, warp: &BridgeWarp, mode: Ends) -> Result<Vec<u8>, String> {
    let len = warp.deck.length();
    let ends = match mode {
        Ends::Ramps => [(warp.s.0 <= 1e-6).then_some((0.0, -1.0)), (warp.s.1 >= len - 1e-6).then_some((len, 1.0))],
        Ends::Plain | Ends::EndPiece => [None, None],
    };
    if ends.iter().all(Option::is_none) {
        return Ok(logic.to_vec());
    }


    // Across: the base's own platforms', else its meshes'.
    let base_logic = base.files.files.get(&base.files.logic_name()).map(|b| decode(b)).transpose()?;
    let zs = base_logic.as_ref().map(intact_platform_points).unwrap_or_default().iter().map(|p| p[2]).collect::<Vec<_>>();
    let (z0, z1) = if zs.is_empty() { base.z } else { (zs.iter().copied().fold(f64::MAX, f64::min), zs.iter().copied().fold(f64::MIN, f64::max)) };

    let mut v = decode(logic)?;
    let Some(d) = v["pieces"].as_array_mut().and_then(|p| p.first_mut()).and_then(|piece| piece["destructs"].as_array_mut()).and_then(|d| d.first_mut()) else {
        return Ok(logic.to_vec());
    };

    if !d["platforms"].is_array() {
        d["platforms"] = Value::Array(vec![]);
    }

    for (s, out) in ends.into_iter().flatten() {
        let (c, t) = warp.deck.at(s);
        let n = [-t[1], t[0]];
        // As the kit's bridge_wood_complete (Attila's and Rome II's alike): a plain ramp from the deck down to just over the ground, then
        // the ground platform (`flag_2`) from there into it. A ground platform joined to the deck itself was walked onto and no further
        // (user 2026-11-08: units stopped at the deck's edge).
        let ramps = mode == Ends::Ramps;
        let y_at = |x: f64, zz: f64, along: f64| {
            if along <= 0.0 {
                c[1]
            } else if ramps && along <= END_RAMP + 1e-9 {
                (warp.ground)(x, zz) + RAMP_FOOT
            } else {
                (warp.ground)(x, zz) - GROUND_PLATFORM_BURY
            }
        };

        let corner = |z: f64, along: f64| {
            let (x, zz) = (c[0] + n[0] * z * warp.across + t[0] * out * along, c[2] + n[1] * z * warp.across + t[1] * out * along);
            [x - warp.origin[0], y_at(x, zz, along) - warp.origin[1], zz - warp.origin[2]]
        };

        // The building's footprint over the ramp too (an end piece has its own), as the kit's runs over its ramps and stops at its ground
        // platforms (bridge_wood_complete's to z +-52: its deck's 50 and its ramps' 2). Its corners at this end of the deck go out by the
        // ramp's length.
        for o in d["collision_outlines"].as_array_mut().into_iter().flatten().filter(|_| ramps) {
            for q in o["vertices"]["outline"].as_array_mut().into_iter().flatten() {
                let (x, z) = (q["x"].as_f64().unwrap_or(0.0) + warp.origin[0], q["z"].as_f64().unwrap_or(0.0) + warp.origin[2]);
                let along = ((x - c[0]) * t[0] + (z - c[2]) * t[1]) * out;
                if along > -0.5 {
                    q["x"] = serde_json::json!((x + t[0] * out * END_RAMP - warp.origin[0]) as f32);
                    q["z"] = serde_json::json!((z + t[1] * out * END_RAMP - warp.origin[2]) as f32);
                }
            }
        }

        // And the footprint the game also keeps beside it (`orange_thingies`: the same ring, x and z as `vertex.x` and `vertex.y`; the
        // kit's has the outline's corners): left as the deck's, the two disagreed and units kept off the ramps' ground too (user
        // 2026-11-08).
        for ring in d["orange_thingies"].as_array_mut().into_iter().flatten().filter(|_| ramps) {
            for q in ring.as_array_mut().into_iter().flatten() {
                let v = &mut q["vertex"];
                let (x, z) = (v["x"].as_f64().unwrap_or(0.0) + warp.origin[0], v["y"].as_f64().unwrap_or(0.0) + warp.origin[2]);
                let along = ((x - c[0]) * t[0] + (z - c[2]) * t[1]) * out;
                if along > -0.5 {
                    v["x"] = serde_json::json!((x + t[0] * out * END_RAMP - warp.origin[0]) as f32);
                    v["y"] = serde_json::json!((z + t[1] * out * END_RAMP - warp.origin[2]) as f32);
                }
            }
        }

        let segments: &[((f64, f64), bool)] = if ramps { &[((0.0, END_RAMP), false), ((END_RAMP, END_RAMP + GROUND_PLATFORM), true)] } else { &[((0.0, GROUND_PLATFORM), true)] };
        for &((from, to), ground) in segments {
            let pts = [corner(z0, from), corner(z1, from), corner(z1, to), corner(z0, to)];
            let (a, b) = ([pts[1][0] - pts[0][0], pts[1][1] - pts[0][1], pts[1][2] - pts[0][2]], [pts[3][0] - pts[0][0], pts[3][1] - pts[0][1], pts[3][2] - pts[0][2]]);
            let mut nrm = [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]];
            let l = (nrm[0] * nrm[0] + nrm[1] * nrm[1] + nrm[2] * nrm[2]).sqrt().max(1e-12);
            let up = if nrm[1] < 0.0 { -1.0 } else { 1.0 };
            nrm = [nrm[0] * up / l, nrm[1] * up / l, nrm[2] * up / l];
            let point = |p: [f64; 3]| serde_json::json!({ "x": p[0] as f32, "y": p[1] as f32, "z": p[2] as f32 });
            let mut platform = serde_json::json!({ "flag_1": false, "flag_2": ground, "flag_3": false,
                "normal": { "x": nrm[0] as f32, "y": nrm[1] as f32, "z": nrm[2] as f32 },
                "vertices": { "outline": pts.iter().map(|&p| point(p)).collect::<Vec<_>>() } });
            crate::logic::wind_as_the_game(&mut platform);
            if let Some(ps) = d["platforms"].as_array_mut() {
                ps.push(platform);
            }
        }
    }

    encode(v)
}

/// How far a platform corner may be off the line through its neighbours and still be dropped (scene units).
const INLINE: f64 = 0.02;

/// A piece's platforms without the corners that lie on the line through their neighbours (the warp's cuts leave some across the deck's
/// ends and the end pieces' feet). The game links one edge per pair of platforms, and one per platform and ground region
/// (docs/re/platforms_and_bridges.md): a foot or a joint split in two by such a corner linked by half its width only (user 2026-11-08:
/// units crossed by one side of each end).
fn without_inline_corners(logic: &[u8]) -> Result<Vec<u8>, String> {
    let mut v = decode(logic)?;
    for piece in v["pieces"].as_array_mut().into_iter().flatten() {
        for d in piece["destructs"].as_array_mut().into_iter().flatten() {
            for p in d["platforms"].as_array_mut().into_iter().flatten() {
                let Some(o) = p["vertices"]["outline"].as_array().cloned() else { continue };
                let pt = |q: &Value| [q["x"].as_f64().unwrap_or(0.0), q["y"].as_f64().unwrap_or(0.0), q["z"].as_f64().unwrap_or(0.0)];
                let mut kept = o.clone();
                loop {
                    let n = kept.len();
                    if n <= 3 {
                        break;
                    }

                    let drop = (0..n).find(|&i| {
                        let (a, b, c) = (pt(&kept[(i + n - 1) % n]), pt(&kept[i]), pt(&kept[(i + 1) % n]));
                        let ac = [c[0] - a[0], c[1] - a[1], c[2] - a[2]];
                        let ab = [b[0] - a[0], b[1] - a[1], b[2] - a[2]];
                        let l2 = ac[0] * ac[0] + ac[1] * ac[1] + ac[2] * ac[2];
                        if l2 < 1e-12 {
                            return true;
                        }

                        let t = ((ab[0] * ac[0] + ab[1] * ac[1] + ab[2] * ac[2]) / l2).clamp(0.0, 1.0);
                        let off = [ab[0] - ac[0] * t, ab[1] - ac[1] * t, ab[2] - ac[2] * t];
                        (off[0] * off[0] + off[1] * off[1] + off[2] * off[2]).sqrt() <= INLINE
                    });
                    match drop {
                        Some(i) => {
                            kept.remove(i);
                        }
                        None => break,
                    }
                }

                if kept.len() != o.len() {
                    p["vertices"]["outline"] = Value::Array(kept);
                }
            }
        }
    }

    encode(v)
}

/// An end piece's footprint made from its platform: each collision outline and `orange_thingies` ring becomes the platform's corners in the
/// other order (at the outline's own height), as the kit's `bridge_wood_2_end` has them. The game links a platform edge to the ground only
/// where a ground edge has the same two corners, run the other way, and the ground's edges along a building are its outline's
/// (docs/re/platforms_and_bridges.md). Bent, the end piece's ramp foot got a middle corner its outline did not have, and the foot linked to
/// no ground (user 2026-11-08: units got on only by its sides).
fn footprint_from_platform(logic: &[u8]) -> Result<Vec<u8>, String> {
    let mut v = decode(logic)?;
    for piece in v["pieces"].as_array_mut().into_iter().flatten() {
        for d in piece["destructs"].as_array_mut().into_iter().flatten() {
            let Some(corners) = d["platforms"].as_array().and_then(|p| p.iter().find(|p| p["flag_2"].as_bool() != Some(true)))
                .and_then(|p| p["vertices"]["outline"].as_array().cloned()) else { continue };
            let xz = corners.iter().rev().map(|q| (q["x"].as_f64().unwrap_or(0.0), q["z"].as_f64().unwrap_or(0.0))).collect::<Vec<_>>();
            for o in d["collision_outlines"].as_array_mut().into_iter().flatten() {
                let y = o["vertices"]["outline"].as_array().and_then(|a| a.first()).and_then(|q| q["y"].as_f64()).unwrap_or(0.0);
                o["vertices"]["outline"] = serde_json::Value::Array(xz.iter().map(|&(x, z)| serde_json::json!({ "x": x as f32, "y": y as f32, "z": z as f32 })).collect());
            }

            for ring in d["orange_thingies"].as_array_mut().into_iter().flatten() {
                let kind = ring.as_array().and_then(|a| a.first()).map_or(serde_json::json!(2), |q| q["vertex_type"].clone());
                *ring = serde_json::Value::Array(xz.iter().map(|&(x, z)| serde_json::json!({ "vertex": { "x": x as f32, "y": z as f32 }, "vertex_type": kind })).collect());
            }
        }
    }

    encode(v)
}

/// A bridge read from its export element: its base's key, its deck, its width (scene units).
#[must_use]
pub fn span_of(scene: &wce_tile::ted_scene::TedScene, elem: usize) -> Option<(String, Deck, f64)> {
    let doc = scene.doc();
    let key = base(&doc.get(elem, BASE_ATTR)?)?.key.to_owned();
    let width = doc.get(elem, WIDTH_ATTR).and_then(|w| w.parse::<f64>().ok())?;
    let w = wce_tile::world_transform::world_transform(scene, elem);
    let path = scene.polygon_points(elem).iter().filter_map(|&p| scene.point_position(p)).map(|p| {
        let q = w.position + w.rotation * p;
        [q.x, q.y, q.z]
    }).collect::<Vec<_>>();
    (path.len() >= 2).then(|| (key, Deck::new(path), width))
}

/// A bridge's export element (at column 0): its deck's points (scene units, absolute), its base's name and its width.
#[must_use]
pub fn span_text(base: &str, width: f64, path: &[[f64; 3]]) -> Vec<u8> {
    let n = wce_tile::ted_scene::format_number;
    let mut out = format!("<{BRIDGE_SPAN} flags=\" OIF_VISIBLE|OIF_ACTIVE\" position=\"vector(0,0,0)\" orientation=\"euler(0,0,0)\" surface_mode=\"MSM_FREEFORM\" name=\"Bridge\" {BASE_ATTR}=\"{base}\" {WIDTH_ATTR}=\"{}\">\n    <points>\n", n(width)).into_bytes();
    for p in path {
        out.extend_from_slice(format!("        <point position=\"{},{},{}\"/>\n", n(p[0]), n(p[1]), n(p[2])).as_bytes());
    }

    out.extend_from_slice(format!("    </points>\n</{BRIDGE_SPAN}>\n").as_bytes());
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_quarter_turn_is_a_turn() {
        let q = QuarterTurn;
        assert_eq!(q.point([0.0, 1.0, 5.0]), [5.0, 1.0, 0.0]);
        assert_eq!(q.point([2.0, 0.0, 0.0]), [0.0, 0.0, -2.0]);
        // The yaw turns directions as the points go.
        let d = crate::warp::turn([0.0, 1.0], q.yaw([0.0; 3]));
        assert!((d[0] - 1.0).abs() < 1e-6 && d[1].abs() < 1e-6, "{d:?}");
    }

    fn prepared() -> Prepared {
        Prepared { files: BuildingFiles { key: "b".into(), files: Default::default() }, x: (-10.0, 10.0), z: (-5.0, 5.0), bottom: -6.0, deck: 1.0, made_platform: false }
    }

    #[test]
    fn a_bridge_follows_its_deck_and_its_posts_reach_the_ground() {
        // A quarter circle of radius 40, rising 4 over it.
        let path = (0..=90).map(|i| {
            let a = f64::from(i).to_radians();
            [40.0 * a.sin(), 4.0 * f64::from(i) / 90.0, 40.0 - 40.0 * a.cos()]
        }).collect::<Vec<_>>();
        let deck = Deck::new(path);
        let base = prepared();
        let pieces = piece_stretches(&deck, &base);
        assert_eq!(pieces.len(), 3, "62.8 long in pieces of 20");
        let ground = |_: f64, _: f64| -20.0;
        let (s, origin) = pieces[1];
        let w = BridgeWarp { deck: &deck, base: &base, s, across: 1.5, origin, ground: &ground };

        // The walked-on height goes onto the deck's surface, its middle onto the curve.
        let mid = w.point([0.0, 1.0, 0.0]).map(f64::from);
        let (c, _) = deck.at((s.0 + s.1) * 0.5);
        assert!((mid[0] + origin[0] - c[0]).abs() < 1e-3 && (mid[1] + origin[1] - c[1]).abs() < 1e-3 && (mid[2] + origin[2] - c[2]).abs() < 1e-3, "{mid:?} {c:?}");

        // Across, widened, square to the way.
        let side = w.point([0.0, 1.0, 5.0]).map(f64::from);
        assert!(((side[0] - mid[0]).hypot(side[2] - mid[2]) - 7.5).abs() < 1e-3);

        // A post's foot on the ground, buried.
        let foot = w.point([0.0, -6.0, 0.0]).map(f64::from);
        assert!((foot[1] + origin[1] - (-20.0 - BURY)).abs() < 1e-3, "{foot:?}");

        // The pieces' ends meet: the end of one is the start of the next.
        let (s2, o2) = pieces[2];
        let next = BridgeWarp { deck: &deck, base: &base, s: s2, across: 1.5, origin: o2, ground: &ground };
        let a = w.point([10.0, 1.0, 3.0]).map(f64::from);
        let b = next.point([-10.0, 1.0, 3.0]).map(f64::from);
        let (a, b) = ([a[0] + origin[0], a[1] + origin[1], a[2] + origin[2]], [b[0] + o2[0], b[1] + o2[1], b[2] + o2[2]]);
        assert!((a[0] - b[0]).abs() < 1e-3 && (a[1] - b[1]).abs() < 1e-3 && (a[2] - b[2]).abs() < 1e-3, "{a:?} {b:?}");
    }

    #[test]
    fn a_span_reads_back_from_its_text() {
        let path = vec![[0.0, 2.0, 0.0], [10.0, 3.0, 0.0], [20.0, 2.5, 5.0]];
        let text = span_text("wood", 12.0, &path);
        let scene = wce_tile::ted_scene::TedScene::parse_any(text).unwrap();
        let (key, deck, width) = span_of(&scene, 0).unwrap();
        assert_eq!(key, "bridge_wood_2");
        assert_eq!(width, 12.0);
        assert_eq!(deck.path, path);
    }
}
