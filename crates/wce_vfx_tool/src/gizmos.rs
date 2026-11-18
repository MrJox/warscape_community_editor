//! The focused part in the viewport: an emitter's spawn ring (SpawnAreaCurve), box (SpawnAreaBox), position and directions, or a
//! reference's position, drawn where the particles are born. The ring's and the box's dots drag their size (the curves scaled, written
//! when the drag ends); the move gizmo moves the spot where the effect plays, or the focused part's position.
//!
//! The shapes are drawn at the start of the emitter's life (the curves' first keys) and the middle of their min / max blends; the
//! particle runtime's frame: the ring and box are turned by EmitterRotation, then moved by SpawnPosition (docs/re/particles.md).

use std::cell::Cell;

use wce_editor::editor::Editor;
use wce_editor::gui;
use wce_editor::tool::{ViewportEvent, ViewportEventKind};
use wce_editor_core::document::Document;
use wce_formats::xml_doc::{decode_entities, encode_entities};
use wce_foundation::math::{Float3, Float4x4, dot, length, normalize, rotation_x, rotation_y, rotation_z, translation};
use wce_weather::xml_file_document::XmlFileDocument;

use crate::curves::Curves;
use crate::inspect::{get, number_like, put_many};
use crate::model::{Emitter, pick};
use crate::tool::{GizmoTarget, Part, PendingText, VfxTool, document, document_mut, units};

const GRAB_PX: f32 = 9.0;

/// Where the dot of a shape of size 0 is drawn (metres): out of the way of the move gizmo's centre.
const EMPTY_SHAPE_DOT: f32 = 1.0;

/// A draggable dot.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Handle {
    /// The ring's outer radius.
    Outer,

    /// The ring's inner radius.
    Inner,

    /// The box's size along its x and z.
    BoxX,
    BoxZ,

    /// The box's height (y).
    BoxY,
}

#[derive(Clone, Debug)]
pub struct HandleDrag {
    handle: Handle,

    /// The ranged_property dragged, and its curves' text when the drag began.
    elem: usize,
    start: String,

    /// The value the handle showed when the drag began.
    start_value: f32,
}

thread_local! {
    /// The move gizmo was being dragged last frame (its edits merge into one undo entry until it stops).
    static GIZMO_DRAGGING: Cell<bool> = const { Cell::new(false) };
}

/// The focused emitter's spawn shape, in the effect's frame (metres).
struct Shape {
    centre: Float3,

    /// EmitterRotation.
    rot: Float4x4,

    /// Outer and inner ring radius (0: none), the arc (degrees, from and to).
    outer: f32,
    inner: f32,
    arc: [f32; 2],

    /// Box size per axis.
    size: [f32; 3],
    ring_elem: Option<usize>,
    box_elem: Option<usize>,

    /// SpreadMethod; SpawnSpread elevation range (degrees); SpawnSpeed range.
    cone: bool,
    spread: [f32; 2],
    speed: [f32; 2],
}

fn curve_text(t: &VfxTool, d: &XmlFileDocument, elem: usize) -> String {
    if let Some(p) = t.pending.as_ref().filter(|p| p.elem == elem) {
        return p.text.clone();
    }

    d.xml().inner_text(elem).map(|raw| String::from_utf8_lossy(&decode_entities(&raw)).into_owned()).unwrap_or_default()
}

/// A curve's value at the start of the emitter's life.
fn first(c: &Curves, name: &str) -> f32 {
    c.find(name).and_then(|c| c.keys.first()).map_or(0.0, |k| k.v)
}

fn num(s: Option<String>, default: f32) -> f32 {
    s.and_then(|t| wce_formats::c_num::strtod(&t)).map_or(default, |v| v as f32)
}

fn shape(t: &VfxTool, d: &XmlFileDocument, em: &Emitter) -> Shape {
    let q = t.quality;
    let vec3 = |name: &str| {
        pick(&em.props, name, q).map_or(Float3::default(), |p| {
            Float3::new(num(get(d, p.elem, "x"), 0.0), num(get(d, p.elem, "y"), 0.0), num(get(d, p.elem, "z"), 0.0))
        })
    };
    let range = |name: &str, lo: f32, hi: f32| {
        pick(&em.props, name, q).map_or([lo, hi], |p| [num(get(d, p.elem, "min"), lo), num(get(d, p.elem, "max"), hi)])
    };

    let er = vec3("EmitterRotation");
    let rot = rotation_z(er.z.to_radians()) * rotation_y(er.y.to_radians()) * rotation_x(er.x.to_radians());
    let ring_elem = pick(&em.props, "SpawnAreaCurve", q).map(|p| p.elem);
    let ring = ring_elem.map(|e| Curves::parse(&curve_text(t, d, e))).unwrap_or_default();
    let box_elem = pick(&em.props, "SpawnAreaBox", q).map(|p| p.elem);
    let bx = box_elem.map(|e| Curves::parse(&curve_text(t, d, e))).unwrap_or_default();
    let mid = |c: &Curves, a: &str, b: &str| 0.5 * (first(c, a) + first(c, b));
    let arc = pick(&em.props, "SpawnArc", q).map_or([0.0, 360.0], |p| [num(get(d, p.elem, "x"), 0.0), num(get(d, p.elem, "y"), 360.0)]);
    let cone = pick(&em.props, "SpreadMethod", q).and_then(|p| get(d, p.elem, "value")).is_none_or(|v| v == "true" || v == "1");
    Shape {
        centre: vec3("SpawnPosition"),
        rot,
        outer: mid(&ring, "outer_radius_min", "outer_radius_max"),
        inner: mid(&ring, "inner_radius_min", "inner_radius_max"),
        arc,
        size: [mid(&bx, "x_min", "x_max"), mid(&bx, "y_min", "y_max"), mid(&bx, "z_min", "z_max")],
        ring_elem,
        box_elem,
        cone,
        spread: range("SpawnSpread", 90.0, 90.0),
        speed: range("SpawnSpeed", 0.0, 0.0),
    }
}

/// The rotation part of a matrix applied to a direction.
fn turn(m: &Float4x4, v: Float3) -> Float3 {
    let m = &m.m;
    Float3::new(
        m[0][0] * v.x + m[0][1] * v.y + m[0][2] * v.z,
        m[1][0] * v.x + m[1][1] * v.y + m[1][2] * v.z,
        m[2][0] * v.x + m[2][1] * v.y + m[2][2] * v.z,
    )
}

impl Shape {
    /// A point of the shape's local frame (ring / box space) in the effect's frame.
    fn local(&self, p: Float3) -> Float3 {
        turn(&self.rot, p) + self.centre
    }

    fn ring_point(&self, r: f32, deg: f32) -> Float3 {
        let a = deg.to_radians();
        self.local(Float3::new(a.cos() * r, 0.0, a.sin() * r))
    }

    /// A handle's value: a radius or a box size (metres).
    fn value(&self, h: Handle) -> f32 {
        match h {
            Handle::Outer => self.outer,
            Handle::Inner => self.inner,
            Handle::BoxX => self.size[0],
            Handle::BoxZ => self.size[2],
            Handle::BoxY => self.size[1],
        }
    }

    /// Where a handle is (effect frame). A shape of size 0 shows its dot a little way out (hollow), to be dragged out from there.
    fn handle(&self, h: Handle) -> Float3 {
        let mid_arc = 0.5 * (self.arc[0] + self.arc[1]);
        let v = if self.value(h) > 1e-4 { self.value(h) } else { EMPTY_SHAPE_DOT };
        match h {
            Handle::Outer | Handle::Inner => self.ring_point(v, mid_arc),
            Handle::BoxX => self.local(Float3::new(v * 0.5, 0.0, 0.0)),
            Handle::BoxZ => self.local(Float3::new(0.0, 0.0, v * 0.5)),
            Handle::BoxY => self.local(Float3::new(0.0, v * 0.5, 0.0)),
        }
    }

    fn handles(&self) -> Vec<Handle> {
        let mut out = vec![];
        if self.ring_elem.is_some() {
            out.push(Handle::Outer);
            if self.inner > 0.0 {
                out.push(Handle::Inner);
            }
        }

        if self.box_elem.is_some() {
            out.extend([Handle::BoxX, Handle::BoxZ, Handle::BoxY]);
        }

        out
    }
}

/// The focused emitter of the open file (a library's emitter: its shapes are in this file).
fn focused_emitter(t: &VfxTool) -> Option<&Emitter> {
    match &t.focus {
        Some(Part::Emitter(id)) => t.ix.emitter(id),
        _ => None,
    }
}

/// Effect frame (metres) -> scene.
fn to_scene(t: &VfxTool, e: &Editor, p: Float3) -> Float3 {
    t.spot.unwrap_or_default() + p * (1.0 / units(e))
}

/// Scene -> viewport overlay coordinates.
fn to_screen(e: &Editor, origin: gui::Vec2, p: Float3) -> Option<gui::Vec2> {
    let density = gui::io().DisplayFramebufferScale.x.max(1e-3);
    let (x, y) = e.viewport.project(p)?;
    Some(gui::v2(origin.x + x / density, origin.y + y / density))
}

/// The handles' image pixels (hit tests use the viewport event's pixels).
fn handle_px(t: &VfxTool, e: &Editor, s: &Shape, h: Handle) -> Option<(f32, f32)> {
    e.viewport.project(to_scene(t, e, s.handle(h)))
}

pub fn draw(t: &mut VfxTool, e: &mut Editor, origin: gui::Vec2, size: gui::Vec2) {
    if !t.show_handles || e.viewport.game_view {
        t.hot_handle = None;
        return;
    }

    // The handle under the mouse, found here (screen points) rather than from the viewport's hover: a handle over the move gizmo's arrows
    // hides the gizmo (`gizmo_frame`), so the press is the handle's.
    if t.drag.is_none() {
        let m = gui::mouse_pos();
        let inside = m.x >= origin.x && m.y >= origin.y && m.x < origin.x + size.x && m.y < origin.y + size.y;
        let density = gui::io().DisplayFramebufferScale.x.max(1e-3);
        t.hot_handle = if inside { hit(t, e, (m.x - origin.x) * density, (m.y - origin.y) * density).map(|h| h.0) } else { None };
    }

    let Some(d) = document(e) else { return };
    let dl = gui::window_draw_list();
    let line = |a: Float3, b: Float3, col: u32, w: f32| {
        if let (Some(a), Some(b)) = (to_screen(e, origin, to_scene(t, e, a)), to_screen(e, origin, to_scene(t, e, b))) {
            dl.line(a, b, gui::col32(0, 0, 0, 150), w + 2.0);
            dl.line(a, b, col, w);
        }
    };

    // Where the effect plays: a small cross on the ground.
    let s = 0.4;
    let white = gui::col32(235, 235, 235, 200);
    line(Float3::new(-s, 0.0, 0.0), Float3::new(s, 0.0, 0.0), white, 1.0);
    line(Float3::new(0.0, 0.0, -s), Float3::new(0.0, 0.0, s), white, 1.0);

    let accent = gui::col32(255, 205, 70, 255);
    let soft = gui::col32(255, 205, 70, 150);
    let blue = gui::col32(110, 200, 255, 220);
    if let Some(em) = focused_emitter(t) {
        let sh = shape(t, d, em);

        // The spawn position.
        let c = sh.centre;
        for axis in [Float3::new(0.25, 0.0, 0.0), Float3::new(0.0, 0.25, 0.0), Float3::new(0.0, 0.0, 0.25)] {
            line(c - axis, c + axis, accent, 1.5);
        }

        // The ring and the arc.
        if sh.ring_elem.is_some() {
            for (r, col) in [(sh.outer, accent), (sh.inner, soft)] {
                if r <= 0.0 {
                    continue;
                }

                let n = 64;
                let (a0, a1) = (sh.arc[0], sh.arc[1]);
                for k in 0..n {
                    let (u, v) = (a0 + (a1 - a0) * k as f32 / n as f32, a0 + (a1 - a0) * (k + 1) as f32 / n as f32);
                    line(sh.ring_point(r, u), sh.ring_point(r, v), col, 1.5);
                }
            }
        }

        // The box.
        if sh.box_elem.is_some() && sh.size.iter().any(|&v| v > 0.0) {
            let h = sh.size.map(|v| v * 0.5);
            let corner = |i: u32| {
                sh.local(Float3::new(
                    if i & 1 == 0 { -h[0] } else { h[0] },
                    if i & 2 == 0 { -h[1] } else { h[1] },
                    if i & 4 == 0 { -h[2] } else { h[2] },
                ))
            };
            for (a, b) in [(0, 1), (2, 3), (4, 5), (6, 7), (0, 2), (1, 3), (4, 6), (5, 7), (0, 4), (1, 5), (2, 6), (3, 7)] {
                line(corner(a), corner(b), accent, 1.5);
            }
        }

        // The directions particles leave in: the cone's edges at the spread's min and max, a second of the speed's max long (0.5 to 3 m).
        let len = sh.speed[0].abs().max(sh.speed[1].abs()).clamp(0.5, 3.0);
        if sh.cone {
            for el in sh.spread {
                for k in 0..8 {
                    let az = k as f32 * std::f32::consts::TAU / 8.0;
                    let er = el.to_radians();
                    let dir = Float3::new(az.cos() * er.cos(), er.sin(), -(az.sin() * er.cos()));
                    line(c, c + turn(&sh.rot, dir) * len, blue, 1.0);
                }
            }
        } else {
            line(c, c + turn(&sh.rot, Float3::new(0.0, 1.0, 0.0)) * len, blue, 1.0);
        }

        // The dots.
        let mouse = gui::mouse_pos();
        for h in sh.handles() {
            let Some(p) = to_screen(e, origin, to_scene(t, e, sh.handle(h))) else { continue };
            let hot = t.drag.as_ref().is_some_and(|dr| dr.handle == h) || t.hot_handle == Some(h);
            dl.circle_filled(p, if hot { 7.0 } else { 5.0 }, gui::col32(0, 0, 0, 180));
            let fill = if hot { gui::col32(255, 255, 255, 255) } else { accent };
            if sh.value(h) > 1e-4 {
                dl.circle_filled(p, if hot { 5.5 } else { 4.0 }, fill);
            } else {
                dl.ellipse(p, gui::v2(4.0, 4.0), fill, 16, 1.5);
            }
            if hot && t.drag.is_none() && (mouse.x - p.x).hypot(mouse.y - p.y) <= GRAB_PX + 2.0 {
                gui::set_tooltip(match h {
                    Handle::Outer => "Spawn ring, outer radius (SpawnAreaCurve outer_radius_min / max): drag to scale it.",
                    Handle::Inner => "Spawn ring, inner radius (SpawnAreaCurve inner_radius_min / max): drag to scale it.",
                    Handle::BoxX => "Spawn box, x size (SpawnAreaBox x_min / x_max): drag to scale it.",
                    Handle::BoxZ => "Spawn box, z size (SpawnAreaBox z_min / z_max): drag to scale it.",
                    Handle::BoxY => "Spawn box, height (SpawnAreaBox y_min / y_max): drag to scale it.",
                });
            }
        }

        if let Some(dr) = &t.drag {
            gui::set_tooltip(&format!("{:.3} m (Ctrl: tenths)", sh.value(dr.handle)));
        }
    }

    // References: where each plays, the focused one brighter.
    for r in &t.ix.refs {
        let pos = Float3::new(num(get(d, r.elem, "pos_x"), 0.0), num(get(d, r.elem, "pos_y"), 0.0), num(get(d, r.elem, "pos_z"), 0.0));
        let focused = t.focus.as_ref() == Some(&Part::Reference(r.inst_name.clone()));
        let col = if focused { accent } else { soft };
        let a = 0.3;
        line(pos - Float3::new(a, 0.0, 0.0), pos + Float3::new(a, 0.0, 0.0), col, 1.5);
        line(pos - Float3::new(0.0, 0.0, a), pos + Float3::new(0.0, 0.0, a), col, 1.5);
        line(pos, pos + Float3::new(0.0, a * 2.0, 0.0), col, 1.5);
        if focused && let Some(p) = to_screen(e, origin, to_scene(t, e, pos + Float3::new(0.0, a * 2.0, 0.0))) {
            let scale = format!(
                "{}  (scale {}..{})",
                r.inst_name,
                get(d, r.elem, "scale_min").unwrap_or_default(),
                get(d, r.elem, "scale_max").unwrap_or_default()
            );
            dl.text(gui::v2(p.x + 6.0, p.y - 8.0), col, &scale);
        }
    }
}

/// The focused emitter's handles and where they are in the viewport image (pixels), with their values: for scripts (`vfx_handles`).
pub fn handle_list(t: &VfxTool, e: &Editor) -> Vec<(Handle, Option<(f32, f32)>, f32)> {
    let (Some(d), Some(em)) = (document(e), focused_emitter(t)) else { return vec![] };
    let sh = shape(t, d, em);
    sh.handles().into_iter().map(|h| (h, handle_px(t, e, &sh, h), sh.value(h))).collect()
}

/// The handle under the cursor (image pixels), if any.
fn hit(t: &VfxTool, e: &Editor, x: f32, y: f32) -> Option<(Handle, usize, f32)> {
    let d = document(e)?;
    let em = focused_emitter(t)?;
    let sh = shape(t, d, em);
    let density = gui::io().DisplayFramebufferScale.x.max(1e-3);
    sh.handles().into_iter().find_map(|h| {
        let (px, py) = handle_px(t, e, &sh, h)?;
        if (px - x).hypot(py - y) > GRAB_PX * density {
            return None;
        }

        let elem = match h {
            Handle::Outer | Handle::Inner => sh.ring_elem?,
            Handle::BoxX | Handle::BoxZ | Handle::BoxY => sh.box_elem?,
        };
        Some((h, elem, sh.value(h)))
    })
}

/// The handle's value where the cursor is: the ray met with the shape's plane (the ring's, turned by EmitterRotation).
fn value_at(t: &VfxTool, e: &Editor, h: Handle, x: i32, y: i32) -> Option<f32> {
    let d = document(e)?;
    let em = focused_emitter(t)?;
    let sh = shape(t, d, em);
    let (o, dir) = e.viewport.ray(x, y)?;
    let u = units(e);
    let centre = to_scene(t, e, sh.centre);
    let up = normalize(turn(&sh.rot, Float3::new(0.0, 1.0, 0.0)));

    // The ring's plane; for the height, the plane through the box's y axis that faces the camera.
    let normal = if h == Handle::BoxY {
        let facing = dir - up * dot(dir, up);
        if length(facing) < 1e-4 { return None } else { normalize(facing) }
    } else {
        up
    };
    let denom = dot(dir, normal);
    let along = if denom.abs() > 1e-4 { dot(centre - o, normal) / denom } else { -1.0 };
    let p = if along > 0.0 {
        o + dir * along
    } else {
        // Seen edge-on: the point of the ray nearest to the centre.
        let s = dot(centre - o, dir).max(0.0);
        o + dir * s
    };

    let rel = (p - centre) * u;
    let in_plane = rel - normal * dot(rel, normal);
    Some(match h {
        Handle::Outer | Handle::Inner => length(in_plane),
        Handle::BoxX => 2.0 * dot(in_plane, normalize(turn(&sh.rot, Float3::new(1.0, 0.0, 0.0)))).abs(),
        Handle::BoxZ => 2.0 * dot(in_plane, normalize(turn(&sh.rot, Float3::new(0.0, 0.0, 1.0)))).abs(),
        Handle::BoxY => 2.0 * dot(rel, up).abs(),
    })
}

/// The curves' text with a handle's curves scaled from `from` to `to` (or set flat to `to` when they were 0).
fn scaled(text: &str, h: Handle, from: f32, to: f32) -> String {
    let mut c = Curves::parse(text);
    let names: &[&str] = match h {
        Handle::Outer => &["outer_radius_min", "outer_radius_max"],
        Handle::Inner => &["inner_radius_min", "inner_radius_max"],
        Handle::BoxX => &["x_min", "x_max"],
        Handle::BoxZ => &["z_min", "z_max"],
        Handle::BoxY => &["y_min", "y_max"],
    };
    for name in names {
        let Some(curve) = c.find_mut(name) else { continue };
        let pts = curve
            .points()
            .into_iter()
            .map(|(k, v)| (k, if from.abs() > 1e-5 { v * to / from } else { to }))
            .map(|(k, v)| (k, (v * 1e5).round() / 1e5))
            .collect::<Vec<_>>();
        curve.set_points(&pts);
    }

    c.write()
}

pub fn input(t: &mut VfxTool, e: &mut Editor, ev: &ViewportEvent) -> bool {
    if ev.kind == ViewportEventKind::Hover && t.drag.is_none() {
        t.hot_handle = if t.show_handles { hit(t, e, ev.x as f32, ev.y as f32).map(|h| h.0) } else { None };
    }

    match ev.kind {
        ViewportEventKind::Press if ev.button == 0 => {
            let Some((handle, elem, value)) = hit(t, e, ev.x as f32, ev.y as f32) else { return false };
            let Some(d) = document(e) else { return false };
            let start = curve_text(t, d, elem);
            t.drag = Some(HandleDrag { handle, elem, start, start_value: value });
            true
        }

        ViewportEventKind::Drag => {
            let Some(dr) = t.drag.clone() else { return false };
            if let Some(v) = value_at(t, e, dr.handle, ev.x, ev.y) {
                let v = if ev.ctrl { (v * 10.0).round() / 10.0 } else { v };
                t.pending = Some(PendingText { elem: dr.elem, text: scaled(&dr.start, dr.handle, dr.start_value, v.max(0.0)) });
            }

            true
        }

        ViewportEventKind::Release => {
            let Some(dr) = t.drag.take() else { return false };
            if let Some(p) = t.pending.take_if(|p| p.elem == dr.elem)
                && p.text != dr.start
                && let Some(d) = document_mut(e)
            {
                let what = match dr.handle {
                    Handle::Outer | Handle::Inner => "spawn ring",
                    Handle::BoxX | Handle::BoxZ | Handle::BoxY => "spawn box",
                };
                if let Err(m) = d.set_inner_text(dr.elem, &encode_entities(p.text.as_bytes(), b'"'), &format!("Scale the {what}")) {
                    e.status(&m, true);
                }
            }

            true
        }

        _ => false,
    }
}

/// The move gizmo's frame: the spot, or the focused part's position.
pub fn gizmo_frame(t: &VfxTool, e: &Editor, dragging: bool) -> Option<Float4x4> {
    if GIZMO_DRAGGING.with(Cell::get) && !dragging {
        GIZMO_DRAGGING.with(|g| g.set(false));
    }

    // A handle under the mouse or being dragged: no gizmo meanwhile (the handles sit on its arrows).
    if !dragging && (t.hot_handle.is_some() || t.drag.is_some()) {
        return None;
    }

    let spot = t.spot?;
    let p = match (t.gizmo_target, &t.focus) {
        (GizmoTarget::Focused, Some(Part::Emitter(_))) => {
            let d = document(e)?;
            let sh = shape(t, d, focused_emitter(t)?);
            sh.centre
        }

        (GizmoTarget::Focused, Some(Part::Reference(n))) => {
            let d = document(e)?;
            let r = t.ix.reference(n)?;
            Float3::new(num(get(d, r.elem, "pos_x"), 0.0), num(get(d, r.elem, "pos_y"), 0.0), num(get(d, r.elem, "pos_z"), 0.0))
        }

        _ => Float3::default(),
    };

    Some(translation(spot + p * (1.0 / units(e))))
}

pub fn gizmo_moved(t: &mut VfxTool, e: &mut Editor, frame: &Float4x4) {
    let at = Float3::new(frame.m[0][3], frame.m[1][3], frame.m[2][3]);
    let first = !GIZMO_DRAGGING.with(Cell::get);
    GIZMO_DRAGGING.with(|g| g.set(true));
    let target = t.gizmo_target;
    let focus = t.focus.clone();
    if target == GizmoTarget::Spot || focus.is_none() {
        t.spot = Some(at);
        return;
    }

    let rel = (at - t.spot.unwrap_or_default()) * units(e);
    let (elem, names) = match &focus {
        Some(Part::Emitter(id)) => {
            let Some(p) = t.ix.emitter(id).and_then(|em| pick(&em.props, "SpawnPosition", t.quality)) else {
                e.status("this emitter has no SpawnPosition to move", true);
                return;
            };
            (p.elem, ["x", "y", "z"])
        }

        Some(Part::Reference(n)) => {
            let Some(r) = t.ix.reference(n) else { return };
            (r.elem, ["pos_x", "pos_y", "pos_z"])
        }

        None => return,
    };

    let Some(d) = document_mut(e) else { return };
    if first {
        d.history_mut().break_merge();
    }

    let gesture = (u64::from(d.id()) << 40) ^ ((elem as u64) << 8) ^ 0x9177;
    let mut values = vec![];
    for (name, v) in names.iter().zip([rel.x, rel.y, rel.z]) {
        let Some(old) = get(d, elem, name) else { continue };
        let v = (v * 1e4).round() / 1e4;
        if (num(Some(old.clone()), 0.0) - v).abs() > 1e-5 {
            values.push((*name, number_like(&old, v)));
        }
    }

    put_many(d, elem, &values, gesture);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_handle_drag_scales_its_curves_only() {
        let text = "inner_radius_min,255;0.0,0.5|outer_radius_min,255;0.0,1.0;1.0,2.0|outer_radius_max,65280;0.0,2.0";
        assert_eq!(
            scaled(text, Handle::Outer, 1.5, 3.0),
            "inner_radius_min,255;0.0,0.5|outer_radius_min,255;0.0,2.0;1.0,4.0|outer_radius_max,65280;0.0,4.0"
        );

        // A shape of size 0 is set flat to where it was dragged.
        assert_eq!(scaled("x_min,255;0.0,0|x_max,255;0.0,0", Handle::BoxX, 0.0, 1.25), "x_min,255;0.0,1.25|x_max,255;0.0,1.25");
    }
}
