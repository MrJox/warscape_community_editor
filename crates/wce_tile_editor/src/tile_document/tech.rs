//! Building technology overlays (the chip at the viewport's top right, `tech_chip.rs` of the tool): what a building's logic file
//! (`<key>_tech.cs2.parsed`) and its pieces' `.meta` files hold, drawn over the buildings as the game's tools show it (TEd's Building
//! Settings): the collision hulls, the outlines that block units, the platforms and the pipes between them, the formation lines, the siege
//! docking lines, the emitters, the attached buildings, the bounding boxes, and the buildings whose footprints overlap each other.
//!
//! The file is read once per building key into shapes in the building's frame (`TechModel`); each shape belongs to a destruction state
//! of a piece, and the overlay draws the state the instance shows (or, with `TechSettings::highest`, the most destroyed one). The overlay
//! is one entity of lines and translucent triangles, rebuilt when a building, the selection, the settings or (for the buildings within
//! the range) the camera changed; the settings are a view setting of the tile, not kept in the file nor in the settings (`TileDisplay`).
//!
//! The fields come from rpfm's `Cs2Parsed` (its serde form: the fields are private); where the spec in the file-formats notes and rpfm
//! differ (rpfm reads the windows, doors, cannons and docking points of v13 as counts only), rpfm is what round-trips the shipped files.
//! ASSUMPTION: a soft collision's `point_1` is its (radius, height), as the spec lists them; a docking line lies on the piece's ground
//! plane (its points are 2D) a little above it.

use std::collections::{BTreeSet, HashMap, HashSet};
use std::io::Cursor;
use std::sync::Arc;

use rpfm_lib::files::cs2_parsed::Cs2Parsed;
use rpfm_lib::files::{Decodeable, DecodeableExtraData};
use serde_json::Value;
use wce_formats::anim_meta::{MetaEffect, meta_effects};
use wce_foundation::math::{Float3, Float4x4, dot, translation};
use wce_foundation::{log, wce_prof};
use wce_scene::render_scene::{EntityId, LayerId, Overlay, Payload, RenderScene, overlay_colour};

use super::columns::{piece_name, quat_matrix, shown_effect, transform_of};
use super::view::piece_local;
use super::{BuildingModel, Tile};

/// The most lines one overlay holds (about 19 MB): the buildings nearest the camera come first.
const MAX_LINES: usize = 600_000;

/// Two footprints that overlap by less than this (metres of the world, as drawn) do not collide: walls that abut.
const TOUCH: f32 = 0.2;

/// What a building's technology can show, in the order of the chip's popup.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TechKind {
    Collision,
    Gates,
    Outlines,
    SoftCollisions,
    Footprints,
    Platforms,
    Pipes,
    EfLines,
    DockingLines,
    ArrowEmitters,
    ActionVfx,
    FileRefs,
    Emitters,
    Bounds,
    Flag,
}

pub const KINDS: [TechKind; 15] = [
    TechKind::Collision, TechKind::Gates, TechKind::Outlines, TechKind::SoftCollisions, TechKind::Footprints, TechKind::Platforms, TechKind::Pipes,
    TechKind::EfLines, TechKind::DockingLines, TechKind::ArrowEmitters, TechKind::ActionVfx, TechKind::FileRefs, TechKind::Emitters, TechKind::Bounds,
    TechKind::Flag,
];

/// The names of the groups the popup lists the kinds under.
pub const GROUPS: [&str; 4] = ["Collision", "Walking and fighting", "Siege and effects", "Frames"];

/// How a kind is listed: its name, its group (an index of [`GROUPS`]), what it is, its colour.
#[derive(Clone, Copy, Debug)]
pub struct TechInfo {
    pub name: &'static str,
    pub group: usize,
    pub tip: &'static str,
    pub rgb: [f32; 3],
}

impl TechKind {
    pub fn info(self) -> TechInfo {
        let (name, group, tip, rgb) = match self {
            Self::Collision => ("Collision mesh", 0, "The piece's collision hull (collision_3d) as the edges of its triangles: what projectiles and units are tested against.", [0.25, 0.8, 1.0]),
            Self::Gates => ("Gate collision", 0, "The collision meshes of a gate (two per gate).", [0.9, 0.4, 1.0]),
            Self::Outlines => ("Hard outlines", 0, "The collision outlines: the hard footprint that blocks units, the outline the siege AI treats as the building.", [1.0, 0.35, 0.2]),
            Self::SoftCollisions => ("Soft collisions", 0, "Soft collision cylinders (radius and height): volumes units steer around.", [0.6, 0.95, 0.3]),
            Self::Footprints => ("Footprints", 1, "The footprint outlines (the orange lines of the building tools): what the building stands on and units may walk on, a plaza's floor or a road.", [1.0, 0.6, 0.1]),
            Self::Platforms => ("Platforms", 1, "Walkable platforms (floors, walkways, tower tops), filled. Green: treated as ground; blue: raised; orange: a siege tower's ramp.", [0.3, 0.55, 1.0]),
            Self::Pipes => ("Pipes", 1, "The paths units take between platforms, by type: stairs green, ladders and ropes teal, doors and teleports purple, jumps yellow.", [0.4, 0.95, 0.3]),
            Self::EfLines => ("Formation lines", 1, "Where units stand to fight (entity formation lines): wall tops gold, windows light blue, crenels red, entrances orange, engines purple; the tick is the facing.", [1.0, 0.8, 0.2]),
            Self::DockingLines => ("Docking lines", 2, "Where siege equipment (ladders, ramps) attaches to the building; the tick is the direction.", [0.2, 0.95, 0.85]),
            Self::ArrowEmitters => ("Arrow emitters", 2, "The positions the building's arrows are fired from.", [1.0, 0.3, 0.3]),
            Self::ActionVfx => ("Action effects", 2, "The nodes where the building's action effects (and their attachment points) play.", [0.75, 0.45, 1.0]),
            Self::FileRefs => ("Attached buildings", 2, "Buildings the piece's state attaches (torches, the bridging columns of walls) at their places.", [1.0, 0.6, 0.8]),
            Self::Emitters => ("Particle emitters", 2, "The effects the pieces' .meta files play, at their places. Bright: plays while the building stands; dim: on an event (fire, destruction, capture); a box: a volumetric effect.", [0.4, 1.0, 0.5]),
            Self::Bounds => ("Bounding boxes", 3, "The building's bounding box (white) and the box of each piece's state (grey).", [0.9, 0.9, 0.95]),
            Self::Flag => ("Minimap flag", 3, "The node of the flag the building has on the tactical map.", [0.4, 1.0, 0.4]),
        };

        TechInfo { name, group, tip, rgb }
    }

    /// The word the `tile_tech` command knows it by.
    pub fn key(self) -> &'static str {
        match self {
            Self::Collision => "collision",
            Self::Gates => "gates",
            Self::Outlines => "outlines",
            Self::SoftCollisions => "soft",
            Self::Footprints => "footprints",
            Self::Platforms => "platforms",
            Self::Pipes => "pipes",
            Self::EfLines => "formation",
            Self::DockingLines => "docking",
            Self::ArrowEmitters => "arrows",
            Self::ActionVfx => "actionfx",
            Self::FileRefs => "attached",
            Self::Emitters => "emitters",
            Self::Bounds => "bounds",
            Self::Flag => "flag",
        }
    }

    pub fn from_key(key: &str) -> Option<Self> {
        KINDS.iter().copied().find(|k| k.key() == key)
    }

    fn bit(self) -> u32 {
        1 << self as u32
    }
}

/// Which buildings the overlay is drawn for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TechScope {
    /// Every building within `TechSettings::distance` of the camera.
    All,

    /// The selected buildings (a selected layer or group: the buildings in it).
    Selected,
}

/// The view settings of the building technology overlays (the chip's popup).
#[derive(Clone, Debug, PartialEq)]
pub struct TechSettings {
    /// A bit per [`TechKind`].
    pub kinds: u32,
    pub scope: TechScope,

    /// The range of `TechScope::All`, metres from the camera.
    pub distance: f32,

    /// Drawn over the buildings (else hidden behind what is in front).
    pub xray: bool,

    /// The platforms' fill opacity.
    pub fill: f32,

    /// The most destroyed state of every piece instead of the one the building shows (TEd's `force_show_highest_destruct_level`).
    pub highest: bool,

    /// Mark the buildings whose footprints overlap (TEd's `detect_building_collisions`).
    pub detect: bool,
}

impl Default for TechSettings {
    fn default() -> Self {
        Self { kinds: 0, scope: TechScope::All, distance: 150.0, xray: true, fill: 0.25, highest: false, detect: false }
    }
}

impl TechSettings {
    pub fn on(&self, kind: TechKind) -> bool {
        self.kinds & kind.bit() != 0
    }

    pub fn set(&mut self, kind: TechKind, on: bool) {
        if on {
            self.kinds |= kind.bit();
        } else {
            self.kinds &= !kind.bit();
        }
    }

    /// Every kind on or off.
    pub fn set_all(&mut self, on: bool) {
        self.kinds = if on { KINDS.iter().fold(0, |bits, k| bits | k.bit()) } else { 0 };
    }

    /// Something is to be drawn.
    pub fn any(&self) -> bool {
        self.kinds != 0 || self.detect
    }
}

/// What the last rebuild drew.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct TechStats {
    pub buildings: usize,
    pub lines: usize,

    /// Pairs of buildings whose footprints overlap (with `TechSettings::detect`).
    pub collisions: usize,

    /// The line limit was reached: the buildings farthest from the camera are left out.
    pub capped: bool,
}

// ---- the file

/// One thing to draw, in the frame of the piece (or building) it is in.
#[derive(Clone, Debug)]
enum Shape {
    Lines { pts: Vec<Float3>, closed: bool },

    /// Pairs of points.
    Segments(Vec<Float3>),

    /// A triangle fan from the first point.
    Fill(Vec<Float3>),

    /// A marker with the axes of a frame, this long (a diamond in the kind's colour too from 1.5).
    Axes(Float4x4, f32),
    Box(Float3, Float3),
    BoxAt(Float4x4, Float3),
    Circle { at: Float4x4, radius: f32, height: f32 },
}

#[derive(Clone, Debug)]
struct Item {
    kind: TechKind,
    rgb: [f32; 3],
    shape: Shape,
}

/// A piece's destruction state.
#[derive(Clone, Debug, Default)]
struct TechLevel {
    index: u32,
    items: Vec<Item>,

    /// The collision outlines (else the state's box as a rectangle): what the collision detection compares.
    rings: Vec<Vec<Float3>>,
}

#[derive(Clone, Debug)]
struct TechPiece {
    /// Lower case (`piece01`).
    name: String,
    local: Float4x4,
    levels: Vec<TechLevel>,
}

/// A building's logic file as shapes.
#[derive(Clone, Debug, Default)]
pub(crate) struct TechModel {
    pieces: Vec<TechPiece>,

    /// The building's own: its box and its flag.
    building: Vec<Item>,
}

fn num(v: &Value) -> f32 {
    v.as_f64().unwrap_or(0.0) as f32
}

fn p3(v: &Value) -> Float3 {
    Float3::new(num(&v["x"]), num(&v["y"]), num(&v["z"]))
}

fn list(v: &Value) -> &[Value] {
    v.as_array().map_or(&[][..], Vec::as_slice)
}

fn matrix(v: &Value) -> Float4x4 {
    transform_of(v).unwrap_or(Float4x4::IDENTITY)
}

fn cube(v: &Value) -> Option<(Float3, Float3)> {
    let (lo, hi) = (Float3::new(num(&v["min_x"]), num(&v["min_y"]), num(&v["min_z"])), Float3::new(num(&v["max_x"]), num(&v["max_y"]), num(&v["max_z"])));
    (hi.x >= lo.x && hi.y >= lo.y && hi.z >= lo.z && (hi.x > lo.x || hi.y > lo.y || hi.z > lo.z)).then_some((lo, hi))
}

/// The edges of a collision mesh's triangles, each once.
fn mesh_edges(c: &Value) -> Vec<Float3> {
    let verts: Vec<Float3> = list(&c["vertices"]).iter().map(p3).collect();
    let (mut seen, mut out) = (HashSet::new(), vec![]);
    for t in list(&c["triangles"]) {
        let idx = ["vertex_1", "vertex_2", "vertex_3"].map(|k| t[k].as_i64().filter(|&i| i >= 0 && (i as usize) < verts.len()));
        for (a, b) in [(0, 1), (1, 2), (2, 0)] {
            let (Some(i), Some(j)) = (idx[a], idx[b]) else { continue };
            if seen.insert((i.min(j), i.max(j))) {
                out.push(verts[i as usize]);
                out.push(verts[j as usize]);
            }
        }
    }

    out
}

fn pipe_rgb(kind: &str) -> [f32; 3] {
    if kind.contains("Ladder") || kind.contains("Rope") {
        [0.2, 0.9, 0.85]
    } else if kind.contains("Stair") {
        [0.4, 0.95, 0.3]
    } else if kind.contains("Door") || kind.contains("Teleport") {
        [0.8, 0.45, 1.0]
    } else if kind.contains("Jump") {
        [1.0, 0.95, 0.3]
    } else {
        [0.85, 0.85, 0.9]
    }
}

fn ef_rgb(action: &str) -> [f32; 3] {
    if action.starts_with("Crenel") {
        [1.0, 0.35, 0.3]
    } else if action == "LowWall" || action == "HighWall" {
        [1.0, 0.8, 0.2]
    } else if action == "Window" {
        [0.5, 0.8, 1.0]
    } else if action == "EntranceDefense" {
        [1.0, 0.55, 0.15]
    } else if action.contains("Engine") {
        [0.8, 0.5, 1.0]
    } else {
        [0.65, 0.65, 0.7]
    }
}

fn parse_level(d: &Value) -> TechLevel {
    let mut level = TechLevel { index: d["index"].as_u64().unwrap_or(0) as u32, ..Default::default() };
    let mut push = |kind: TechKind, rgb: Option<[f32; 3]>, shape: Shape| level.items.push(Item { kind, rgb: rgb.unwrap_or_else(|| kind.info().rgb), shape });

    let hull = mesh_edges(&d["collision_3d"]);
    if !hull.is_empty() {
        push(TechKind::Collision, None, Shape::Segments(hull));
    }

    for g in list(&d["gates"]) {
        for k in ["collision_1", "collision_2"] {
            let edges = mesh_edges(&g[k]);
            if !edges.is_empty() {
                push(TechKind::Gates, None, Shape::Segments(edges));
            }
        }
    }

    let mut rings = vec![];
    for o in list(&d["collision_outlines"]) {
        let pts: Vec<Float3> = list(&o["vertices"]["outline"]).iter().map(p3).collect();
        if pts.len() >= 2 {
            if pts.len() >= 3 {
                rings.push(pts.clone());
            }

            push(TechKind::Outlines, None, Shape::Lines { pts, closed: true });
        }
    }

    for ring in list(&d["orange_thingies"]) {
        let pts: Vec<Float3> = list(ring).iter().map(|q| Float3::new(num(&q["vertex"]["x"]), 0.05, num(&q["vertex"]["y"]))).collect();
        if pts.len() >= 2 {
            push(TechKind::Footprints, None, Shape::Lines { pts, closed: true });
        }
    }

    for s in list(&d["soft_collisions"]) {
        let (radius, height) = (num(&s["point_1"]["x"]), num(&s["point_1"]["y"]));
        if radius > 0.0 {
            push(TechKind::SoftCollisions, None, Shape::Circle { at: matrix(&s["transform"]), radius, height });
        }
    }

    for p in list(&d["platforms"]) {
        let pts: Vec<Float3> = list(&p["vertices"]["outline"]).iter().map(p3).collect();
        if pts.len() < 3 {
            continue;
        }

        let rgb = if p["flag_3"].as_bool() == Some(true) {
            [1.0, 0.6, 0.2]
        } else if p["flag_2"].as_bool() == Some(true) {
            [0.3, 0.9, 0.4]
        } else {
            [0.3, 0.55, 1.0]
        };

        push(TechKind::Platforms, Some(rgb), Shape::Fill(pts.clone()));
        push(TechKind::Platforms, Some(rgb), Shape::Lines { pts, closed: true });
    }

    for p in list(&d["pipes"]) {
        let pts: Vec<Float3> = list(&p["line"]["outline"]).iter().map(p3).collect();
        if pts.len() >= 2 {
            push(TechKind::Pipes, Some(pipe_rgb(p["line_type"].as_str().unwrap_or_default())), Shape::Lines { pts, closed: false });
        }
    }

    for l in list(&d["ef_lines"]) {
        let (a, b, dir) = (p3(&l["start"]), p3(&l["end"]), p3(&l["direction"]));
        let mid = (a + b) * 0.5;
        push(TechKind::EfLines, Some(ef_rgb(l["action"].as_str().unwrap_or_default())), Shape::Segments(vec![a, b, mid, mid + dir * 1.5]));
    }

    for l in list(&d["docking_lines"]) {
        let pt = |v: &Value| Float3::new(num(&v["x"]), 0.2, num(&v["y"]));
        let (a, b) = (pt(&l["start"]), pt(&l["end"]));
        let (mid, dir) = ((a + b) * 0.5, Float3::new(num(&l["direction"]["x"]), 0.0, num(&l["direction"]["y"])));
        push(TechKind::DockingLines, None, Shape::Segments(vec![a, b, mid, mid + dir * 1.5]));
    }

    for e in list(&d["arrow_emitters"]) {
        push(TechKind::ArrowEmitters, None, Shape::Axes(matrix(&e["transform"]), 1.5));
    }

    for v in list(&d["action_vfx"]).iter().chain(list(&d["action_vfx_attachments"])) {
        push(TechKind::ActionVfx, None, Shape::Axes(matrix(&v["matrix_1"]), 1.2));
    }

    for f in list(&d["file_refs"]) {
        push(TechKind::FileRefs, None, Shape::Axes(matrix(&f["transform"]), 2.0));
    }

    if let Some((lo, hi)) = cube(&d["bounding_box"]) {
        push(TechKind::Bounds, Some([0.55, 0.55, 0.6]), Shape::Box(lo, hi));
        if rings.is_empty() {
            rings.push(vec![Float3::new(lo.x, lo.y, lo.z), Float3::new(hi.x, lo.y, lo.z), Float3::new(hi.x, lo.y, hi.z), Float3::new(lo.x, lo.y, hi.z)]);
        }
    }

    level.rings = rings;
    level
}

fn parse(bytes: &[u8]) -> Result<TechModel, String> {
    let parsed = Cs2Parsed::decode(&mut Cursor::new(bytes), &Some(DecodeableExtraData::default())).map_err(|e| e.to_string())?;
    let v = serde_json::to_value(&parsed).map_err(|e| e.to_string())?;
    let mut model = TechModel::default();
    if let Some((lo, hi)) = cube(&v["bounding_box"]) {
        model.building.push(Item { kind: TechKind::Bounds, rgb: [1.0, 1.0, 1.0], shape: Shape::Box(lo, hi) });
    }

    let flag = &v["ui_flag"];
    if flag["transform"].is_object() {
        let at = matrix(&flag["transform"]);
        let rgb = TechKind::Flag.info().rgb;
        model.building.push(Item { kind: TechKind::Flag, rgb, shape: Shape::Axes(at, 1.5) });
        let pole = [Float3::new(0.0, 0.0, 0.0), Float3::new(0.0, 6.0, 0.0)];
        model.building.push(Item { kind: TechKind::Flag, rgb, shape: Shape::Segments(pole.iter().map(|&p| at.transform_point(p)).collect()) });
        let cloth = [Float3::new(0.0, 6.0, 0.0), Float3::new(0.0, 4.5, 0.0), Float3::new(2.5, 5.25, 0.0)].map(|p| at.transform_point(p));
        model.building.push(Item { kind: TechKind::Flag, rgb, shape: Shape::Lines { pts: cloth.to_vec(), closed: true } });
    }

    for p in list(&v["pieces"]) {
        let mut levels: Vec<TechLevel> = list(&p["destructs"]).iter().map(parse_level).collect();
        levels.sort_by_key(|l| l.index);
        model.pieces.push(TechPiece { name: p["name"].as_str().unwrap_or_default().to_lowercase(), local: matrix(&p["node_transform"]), levels });
    }

    Ok(model)
}

/// How many shapes of each kind (in the order of [`KINDS`]) a logic file holds, over every piece and every state, and how many points they
/// hold in all: what the gate `tech_check` reports.
pub fn tech_counts(bytes: &[u8]) -> Result<([usize; 15], usize), String> {
    let model = parse(bytes)?;
    let (mut counts, mut points) = ([0; 15], 0);
    for item in model.building.iter().chain(model.pieces.iter().flat_map(|p| p.levels.iter().flat_map(|l| l.items.iter()))) {
        counts[item.kind as usize] += 1;
        points += match &item.shape {
            Shape::Lines { pts, .. } | Shape::Fill(pts) | Shape::Segments(pts) => pts.len(),
            _ => 1,
        };
    }

    Ok((counts, points))
}

/// The state of a piece to draw: the one numbered `want`, else the highest below it, else the first.
fn pick(levels: &[TechLevel], want: i64) -> Option<&TechLevel> {
    levels.iter().rev().find(|l| i64::from(l.index) <= want).or_else(|| levels.first())
}

// ---- drawing

fn emit(o: &mut Overlay, w: &Float4x4, item: &Item, fill: f32) {
    let col = |a: f32| overlay_colour(item.rgb[0], item.rgb[1], item.rgb[2], a);
    let line = col(1.0);
    match &item.shape {
        Shape::Lines { pts, closed } => {
            let p: Vec<Float3> = pts.iter().map(|&q| w.transform_point(q)).collect();
            o.polygon_outline(&p, *closed, line);
        }

        Shape::Segments(pts) => {
            for pair in pts.as_chunks::<2>().0 {
                o.line(w.transform_point(pair[0]), w.transform_point(pair[1]), line);
            }
        }

        Shape::Fill(pts) => {
            if fill > 0.0 {
                let p: Vec<Float3> = pts.iter().map(|&q| w.transform_point(q)).collect();
                for k in 1..p.len().saturating_sub(1) {
                    o.triangle(p[0], p[k], p[k + 1], col(fill));
                }
            }
        }

        Shape::Axes(at, size) => {
            let m = *w * *at;
            let (c, r) = (m.transform_point(Float3::default()), size * 0.3);
            let tip = |x: f32, y: f32, z: f32| m.transform_point(Float3::new(x, y, z));

            // The small markers (the action effects: some buildings have hundreds) are the axes alone; the others have a diamond too.
            if *size >= 1.5 {
                let ring = [tip(r, 0.0, 0.0), tip(-r, 0.0, 0.0), tip(0.0, r, 0.0), tip(0.0, -r, 0.0), tip(0.0, 0.0, r), tip(0.0, 0.0, -r)];
                for i in 0..6 {
                    for j in i + 1..6 {
                        if i / 2 != j / 2 {
                            o.line(ring[i], ring[j], line);
                        }
                    }
                }
            }

            o.line(c, tip(*size, 0.0, 0.0), overlay_colour(1.0, 0.25, 0.25, 1.0));
            o.line(c, tip(0.0, *size, 0.0), overlay_colour(0.3, 1.0, 0.3, 1.0));
            o.line(c, tip(0.0, 0.0, *size), overlay_colour(0.35, 0.5, 1.0, 1.0));
        }

        Shape::Box(lo, hi) => o.box_wire_transformed(w, *lo, *hi, line),
        Shape::BoxAt(at, half) => o.box_wire_transformed(&(*w * *at), -*half, *half, line),
        Shape::Circle { at, radius, height } => {
            let m = *w * *at;
            let ring = |y: f32| -> Vec<Float3> { (0..16).map(|k| { let a = k as f32 * std::f32::consts::TAU / 16.0; m.transform_point(Float3::new(a.cos() * radius, y, a.sin() * radius)) }).collect() };
            let (base, top) = (ring(0.0), ring(*height));
            o.polygon_outline(&base, true, line);
            o.polygon_outline(&top, true, line);
            for k in (0..16).step_by(4) {
                o.line(base[k], top[k], line);
            }
        }
    }
}

// ---- the collision test

fn inside(p: [f32; 2], ring: &[[f32; 2]]) -> bool {
    let mut odd = false;
    let mut j = ring.len() - 1;
    for i in 0..ring.len() {
        let (a, b) = (ring[i], ring[j]);
        if (a[1] > p[1]) != (b[1] > p[1]) && p[0] < (b[0] - a[0]) * (p[1] - a[1]) / (b[1] - a[1]) + a[0] {
            odd = !odd;
        }

        j = i;
    }

    odd
}

/// The distance from a point to a segment.
fn seg_dist(p: [f32; 2], a: [f32; 2], b: [f32; 2]) -> f32 {
    let (d, e) = ([b[0] - a[0], b[1] - a[1]], [p[0] - a[0], p[1] - a[1]]);
    let len2 = d[0] * d[0] + d[1] * d[1];
    let t = if len2 > 1e-12 { ((e[0] * d[0] + e[1] * d[1]) / len2).clamp(0.0, 1.0) } else { 0.0 };
    (e[0] - d[0] * t).hypot(e[1] - d[1] * t)
}

/// Whether a point is inside a ring by more than the touching margin.
fn deep_inside(p: [f32; 2], ring: &[[f32; 2]]) -> bool {
    inside(p, ring) && (0..ring.len()).all(|i| seg_dist(p, ring[i], ring[(i + 1) % ring.len()]) > TOUCH)
}

/// Whether two segments cross, their ends clear of each other's line by more than the touching margin.
fn crosses(a: [f32; 2], b: [f32; 2], c: [f32; 2], d: [f32; 2]) -> bool {
    let side = |p: [f32; 2], q: [f32; 2], r: [f32; 2]| (q[0] - p[0]) * (r[1] - p[1]) - (q[1] - p[1]) * (r[0] - p[0]);
    let (d1, d2, d3, d4) = (side(c, d, a), side(c, d, b), side(a, b, c), side(a, b, d));
    d1 * d2 < 0.0 && d3 * d4 < 0.0 && seg_dist(a, c, d).min(seg_dist(b, c, d)).min(seg_dist(c, a, b)).min(seg_dist(d, a, b)) > TOUCH
}

fn xz(ring: &[Float3]) -> Vec<[f32; 2]> {
    ring.iter().map(|p| [p.x, p.z]).collect()
}

fn centroid(ring: &[[f32; 2]]) -> [f32; 2] {
    let n = ring.len() as f32;
    [ring.iter().map(|p| p[0]).sum::<f32>() / n, ring.iter().map(|p| p[1]).sum::<f32>() / n]
}

/// Whether two footprints overlap: an edge of one crosses an edge of the other, or a corner (or the middle) of one is well inside the other.
/// The middle is what finds two that coincide (a building put twice on the same place), where no corner is inside and no edge crosses.
fn rings_overlap(a: &[Vec<[f32; 2]>], b: &[Vec<[f32; 2]>]) -> bool {
    a.iter().any(|ra| b.iter().any(|rb| {
        (0..ra.len()).any(|i| {
            let (p, q) = (ra[i], ra[(i + 1) % ra.len()]);
            (0..rb.len()).any(|j| crosses(p, q, rb[j], rb[(j + 1) % rb.len()]))
        }) || ra.iter().any(|&p| deep_inside(p, rb)) || rb.iter().any(|&p| deep_inside(p, ra)) || deep_inside(centroid(ra), rb) || deep_inside(centroid(rb), ra)
    }))
}

/// A building's footprint in the world: its rings (3D, for drawing) and the box around them.
struct Foot {
    elem: usize,
    rings: Vec<Vec<Float3>>,
    lo: [f32; 2],
    hi: [f32; 2],
    centre: Float3,
}

// ---- the tile

pub(crate) struct TechState {
    /// Something they depend on changed: rebuilt at the next update.
    pub(crate) dirty: bool,
    models: HashMap<String, Option<Arc<TechModel>>>,
    metas: HashMap<String, Option<Arc<Vec<MetaEffect>>>>,
    entity: EntityId,

    /// Where the camera was at the last rebuild.
    eye: Float3,
    selection: Vec<usize>,
    stats: TechStats,
}

impl Default for TechState {
    fn default() -> Self {
        Self { dirty: true, models: HashMap::new(), metas: HashMap::new(), entity: 0, eye: Float3::new(1e30, 1e30, 1e30), selection: vec![], stats: TechStats::default() }
    }
}

/// What a building shows: its frame, its logic file, and the pieces placed.
struct Placed {
    world: Float4x4,
    model: Arc<TechModel>,

    /// (the piece's frame, its index in the logic file, the state wanted).
    pieces: Vec<(Float4x4, usize, i64)>,

    /// (the piece's frame, the model file it shows) for the emitters.
    files: Vec<(Float4x4, String)>,
}

fn is_group(kind: &str) -> bool {
    matches!(kind, "LayerInstance" | "UserInstanceGroup" | "INSTANCE_GROUP" | "LAYER_INSTANCE_GROUP")
}

impl Tile {
    pub fn tech_stats(&self) -> TechStats {
        self.tech.stats
    }

    /// The settings changed: drawn again at the next update.
    pub fn tech_changed(&mut self) {
        self.tech.dirty = true;
    }

    /// Forgets the entity (the scene it was in is gone).
    pub(crate) fn reset_tech(&mut self) {
        self.tech.entity = 0;
        self.tech.dirty = true;
    }

    /// The selection, from the highlight: the overlay of the selected buildings follows it.
    pub(crate) fn tech_selection(&mut self, selected: &[usize]) {
        if self.tech.selection != selected {
            self.tech.selection = selected.to_vec();
            self.tech.dirty |= self.display.tech.scope == TechScope::Selected;
        }
    }

    fn tech_model(&mut self, key: &str) -> Option<Arc<TechModel>> {
        if let Some(m) = self.tech.models.get(key) {
            return m.clone();
        }

        let path = format!("rigidmodels/buildings/{key}/{key}_tech.cs2.parsed");
        let found = self.files.as_deref().filter(|f| f.exists(&path)).and_then(|f| f.read(&path).ok()).and_then(|bytes| match parse(&bytes) {
            Ok(m) => Some(Arc::new(m)),
            Err(error) => {
                log::warn(format!("{path}: {error} (its technology is not drawn)"));
                None
            }
        });

        self.tech.models.insert(key.to_owned(), found.clone());
        found
    }

    fn tech_meta(&mut self, file: &str) -> Option<Arc<Vec<MetaEffect>>> {
        if let Some(m) = self.tech.metas.get(file) {
            return m.clone();
        }

        let path = format!("{}.meta", file.strip_suffix(".rigid_model_v2").unwrap_or(file));
        let found = self.files.as_deref().filter(|f| f.exists(&path)).and_then(|f| f.read(&path).ok()).and_then(|bytes| meta_effects(&bytes)).filter(|e| !e.is_empty()).map(Arc::new);
        self.tech.metas.insert(file.to_owned(), found.clone());
        found
    }

    /// The buildings the overlay is for, the nearest first.
    fn tech_candidates(&self, eye: Float3, s: &TechSettings) -> Vec<usize> {
        if self.display.hidden_types.contains("BUILDING_INSTANCE") {
            return vec![];
        }

        let building = |e: usize| self.tscene().type_of(e) == "BUILDING_INSTANCE";
        let mut found: Vec<(f32, usize)> = match s.scope {
            TechScope::All => self.instances.iter().copied().filter(|&e| building(e) && self.shown(e)).filter_map(|e| {
                let d = self.world_position(e) - eye;
                let d = dot(d, d).sqrt();
                (d <= s.distance + self.marker_radius(e)).then_some((d, e))
            }).collect(),
            TechScope::Selected => {
                let groups: Vec<usize> = self.tech.selection.iter().copied().filter(|&g| g < self.tscene().doc().elements().len() && is_group(self.tscene().type_of(g))).collect();
                let mut chosen: Vec<usize> = self.tech.selection.iter().copied().filter(|&e| e < self.tscene().doc().elements().len() && building(e)).collect();
                if !groups.is_empty() {
                    chosen.extend(self.instances.iter().copied().filter(|&b| building(b) && groups.iter().any(|&g| self.tscene().is_descendant(b, g))));
                }

                chosen.sort_unstable();
                chosen.dedup();
                chosen.into_iter().filter(|&e| self.shown(e)).map(|e| {
                    let d = self.world_position(e) - eye;
                    (dot(d, d).sqrt(), e)
                }).collect()
            }
        };

        found.sort_by(|a, b| a.0.total_cmp(&b.0));
        found.into_iter().map(|f| f.1).collect()
    }

    /// What a building shows of its technology: the pieces it has, at the states it shows.
    fn tech_placed(&mut self, e: usize, highest: bool) -> Option<Placed> {
        let key = self.tscene().doc().get(e, "key")?.to_lowercase();
        let tech = self.tech_model(&key)?;
        let world = self.world_matrix(e);
        let model: Option<BuildingModel> = self.model_of(e).filter(|m| m.ok).cloned();
        let want = |level: i64| if highest { i64::MAX } else { level };
        let (mut pieces, mut files, mut done) = (vec![], vec![], HashSet::new());
        let mut saved = 0;
        if let Some(m) = &model {
            saved = self.saved_destruction_level(e, m.levels);
            for (k, piece) in m.pieces.iter().enumerate() {
                let props = self.piece_props(e, k).cloned();
                let name = piece_name(piece);
                if let Some(n) = &name {
                    done.insert(n.clone());
                }

                if props.as_ref().is_some_and(|p| !p.visible) {
                    continue;
                }

                let level = props.as_ref().map_or(saved, |p| if p.destruction_level < 0 { saved } else { p.destruction_level });
                let levels = m.piece_levels.get(k).copied().unwrap_or(m.levels).max(1);
                let w = props.as_ref().map_or(world, |p| world * piece_local(p));
                files.push((w, self.piece_file(piece, want(level), levels)));
                if let Some(i) = name.and_then(|n| tech.pieces.iter().position(|p| p.name == n)) {
                    pieces.push((w, i, want(level)));
                }
            }
        }

        // The pieces the logic file has and no model shows (or all of them without a model): at the building's own frame.
        for (i, p) in tech.pieces.iter().enumerate() {
            if !done.contains(&p.name) {
                pieces.push((world, i, want(saved)));
            }
        }

        Some(Placed { world, model: tech, pieces, files })
    }

    /// A building's footprint in the world.
    fn tech_foot(&mut self, e: usize, highest: bool) -> Option<Foot> {
        let placed = self.tech_placed(e, highest)?;
        let mut rings: Vec<Vec<Float3>> = vec![];
        for &(w, i, want) in &placed.pieces {
            let piece = &placed.model.pieces[i];
            let frame = w * piece.local;
            let Some(level) = pick(&piece.levels, want) else { continue };
            rings.extend(level.rings.iter().map(|r| r.iter().map(|&p| frame.transform_point(p)).collect::<Vec<_>>()));
        }

        let all: Vec<Float3> = rings.iter().flatten().copied().collect();
        if all.is_empty() {
            return None;
        }

        let (mut lo, mut hi) = ([f32::MAX; 2], [f32::MIN; 2]);
        for p in &all {
            (lo, hi) = ([lo[0].min(p.x), lo[1].min(p.z)], [hi[0].max(p.x), hi[1].max(p.z)]);
        }

        let n = all.len() as f32;
        let centre = Float3::new(all.iter().map(|p| p.x).sum::<f32>() / n, all.iter().map(|p| p.y).sum::<f32>() / n, all.iter().map(|p| p.z).sum::<f32>() / n);
        Some(Foot { elem: e, rings, lo, hi, centre })
    }

    /// Per frame: draws the technology again when it changed (a building, the selection, the settings, the camera for the buildings in range).
    pub(crate) fn update_tech(&mut self, scene: &mut RenderScene, layer: LayerId, eye: Float3) {
        wce_prof!("tile.update_tech");
        let s = self.display.tech.clone();

        // A piece the Inspector's show_collision is on for draws its collision whatever the settings.
        let piece_collision = self.piece_props.values().any(|p| p.show_collision);
        if !s.any() && !piece_collision {
            if self.tech.entity != 0 {
                scene.remove(self.tech.entity);
                scene.touch();
                self.tech.entity = 0;
            }

            self.tech.stats = TechStats::default();
            return;
        }

        let range = (s.distance * 0.15).max(8.0);
        let moved = s.scope == TechScope::All && {
            let d = eye - self.tech.eye;
            dot(d, d) > range * range
        };

        if !self.tech.dirty && !moved && self.tech.entity != 0 && scene.find(self.tech.entity).is_some() {
            return;
        }

        self.tech.dirty = false;
        self.tech.eye = eye;
        let candidates = self.tech_candidates(eye, &s);
        let mut o = Overlay { depth_test: !s.xray, pickable: false, ..Default::default() };
        let mut stats = TechStats::default();
        let collision_bits = TechKind::Collision.bit() | TechKind::Gates.bit();
        for &e in &candidates {
            let Some(placed) = self.tech_placed(e, s.highest) else { continue };
            stats.buildings += 1;
            for item in placed.model.building.iter().filter(|i| s.on(i.kind)) {
                emit(&mut o, &placed.world, item, s.fill);
            }

            for &(w, i, want) in &placed.pieces {
                let piece = &placed.model.pieces[i];
                let Some(level) = pick(&piece.levels, want) else { continue };
                let frame = w * piece.local;
                let force = self.piece_props.iter().any(|(&(el, _), p)| el == e && p.show_collision);
                for item in level.items.iter().filter(|i| s.on(i.kind) || (force && collision_bits & i.kind.bit() != 0)) {
                    emit(&mut o, &frame, item, s.fill);
                }
            }

            if s.on(TechKind::Emitters) {
                for (w, file) in &placed.files {
                    let Some(effects) = self.tech_meta(file) else { continue };
                    for fx in effects.iter() {
                        let at = *w * translation(Float3::new(fx.position[0], fx.position[1], fx.position[2])) * quat_matrix(fx.rotation);
                        let playing = shown_effect(fx, false);
                        let rgb = if playing { [0.4, 1.0, 0.5] } else { [0.75, 0.65, 0.25] };
                        let shape = match fx.volume {
                            Some(v) => Shape::BoxAt(at, Float3::new(v[0] * 0.5, v[1] * 0.5, v[2] * 0.5)),
                            None => Shape::Axes(at, 1.2),
                        };

                        emit(&mut o, &Float4x4::IDENTITY, &Item { kind: TechKind::Emitters, rgb, shape }, s.fill);
                    }
                }
            }

            if o.lines.len() / 2 > MAX_LINES {
                stats.capped = true;
                break;
            }
        }

        if s.detect {
            stats.collisions = self.tech_collisions(&mut o, &candidates, &s);
        }

        stats.lines = o.lines.len() / 2;
        self.tech.stats = stats;
        let payload = Payload::Overlay(o);
        if self.tech.entity != 0 && scene.find(self.tech.entity).is_some() {
            scene.update_payload(self.tech.entity, payload);
        } else {
            self.tech.entity = scene.add(layer, Float4x4::IDENTITY, payload, self.source(super::TERRAIN));
        }

        scene.touch();
    }

    /// Marks the buildings whose footprints overlap, among the ones drawn (and, for a selection, whatever stands against it); how many pairs.
    fn tech_collisions(&mut self, o: &mut Overlay, drawn: &[usize], s: &TechSettings) -> usize {
        let mut subjects = drawn.to_vec();
        if s.scope == TechScope::Selected && !drawn.is_empty() && drawn.len() * self.instances.len() <= 4_000_000 {
            let near: Vec<(Float3, f32)> = drawn.iter().map(|&e| (self.world_position(e), self.marker_radius(e))).collect();
            let rest: Vec<usize> = self.instances.iter().copied().filter(|&e| self.tscene().type_of(e) == "BUILDING_INSTANCE" && self.shown(e) && !drawn.contains(&e)).collect();
            for e in rest {
                let (p, r) = (self.world_position(e), self.marker_radius(e));
                if near.iter().any(|&(q, rq)| { let d = p - q; dot(d, d).sqrt() <= r + rq + 4.0 }) {
                    subjects.push(e);
                }
            }
        }

        let mut feet: Vec<Foot> = subjects.iter().filter_map(|&e| self.tech_foot(e, s.highest)).collect();
        feet.sort_by(|a, b| a.lo[0].total_cmp(&b.lo[0]));
        let flat: Vec<Vec<Vec<[f32; 2]>>> = feet.iter().map(|f| f.rings.iter().map(|r| xz(r)).collect()).collect();
        let (mut pairs, mut hit) = (vec![], BTreeSet::new());
        for i in 0..feet.len() {
            for j in i + 1..feet.len() {
                if feet[j].lo[0] > feet[i].hi[0] {
                    break;
                }

                // Two buildings of a selection or one of them: not two others far from it.
                if feet[i].hi[1] < feet[j].lo[1] || feet[j].hi[1] < feet[i].lo[1] {
                    continue;
                }

                if s.scope == TechScope::Selected && !drawn.contains(&feet[i].elem) && !drawn.contains(&feet[j].elem) {
                    continue;
                }

                if rings_overlap(&flat[i], &flat[j]) {
                    pairs.push((i, j));
                    hit.insert(i);
                    hit.insert(j);
                }
            }
        }

        let red = overlay_colour(1.0, 0.1, 0.1, 1.0);
        for &i in &hit {
            for ring in &feet[i].rings {
                o.polygon_outline(ring, true, red);
            }

            let c = feet[i].centre;
            o.line(c, Float3::new(c.x, c.y + 12.0, c.z), red);
        }

        for &(i, j) in &pairs {
            o.line(feet[i].centre, feet[j].centre, overlay_colour(1.0, 0.45, 0.1, 1.0));
        }

        pairs.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn square(x: f32, z: f32, s: f32) -> Vec<[f32; 2]> {
        vec![[x, z], [x + s, z], [x + s, z + s], [x, z + s]]
    }

    #[test]
    fn overlapping_squares_collide_and_touching_ones_do_not() {
        let a = vec![square(0.0, 0.0, 10.0)];
        assert!(rings_overlap(&a, &[square(5.0, 5.0, 10.0)]), "a corner inside the other");
        assert!(rings_overlap(&a, &[square(2.0, 2.0, 2.0)]), "one inside the other");
        assert!(!rings_overlap(&a, &[square(10.0, 0.0, 10.0)]), "sharing an edge");
        assert!(!rings_overlap(&a, &[square(20.0, 0.0, 10.0)]), "apart");
        assert!(!rings_overlap(&a, &[square(9.9, 0.0, 10.0)]), "overlapping by less than the margin");
        assert!(rings_overlap(&a, &[square(0.0, 0.0, 10.0)]), "the same footprint twice");
    }

    #[test]
    fn crossing_bars_collide() {
        let a = vec![vec![[0.0, 4.0], [20.0, 4.0], [20.0, 6.0], [0.0, 6.0]]];
        let b = vec![vec![[9.0, 0.0], [11.0, 0.0], [11.0, 10.0], [9.0, 10.0]]];
        assert!(rings_overlap(&a, &b));
    }

    #[test]
    fn a_state_is_picked_by_its_number() {
        let levels = [0, 1, 2].map(|index| TechLevel { index, ..Default::default() });
        assert_eq!(pick(&levels, 1).map(|l| l.index), Some(1));
        assert_eq!(pick(&levels, i64::MAX).map(|l| l.index), Some(2));
        assert_eq!(pick(&levels, -1).map(|l| l.index), Some(0));
        let gaps = [0, 2].map(|index| TechLevel { index, ..Default::default() });
        assert_eq!(pick(&gaps, 1).map(|l| l.index), Some(0));
    }

    #[test]
    fn settings_are_bits() {
        let mut s = TechSettings::default();
        assert!(!s.any());
        s.set(TechKind::Pipes, true);
        assert!(s.on(TechKind::Pipes) && !s.on(TechKind::Gates) && s.any());
        s.set(TechKind::Pipes, false);
        assert!(!s.any());
    }
}
