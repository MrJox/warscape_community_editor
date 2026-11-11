//! Tape measures (`WceMeasureLine`, TEd's Tape Measure) and angle finders (`WceAngleFinder`, TEd's `ANGLE_FINDER_INSTANCE`): the selected
//! one's lengths or angle in the Inspector, and labels in the viewport on every one shown and on the one being drawn (an angle finder's
//! corner gets an arc too). They are polygons of the tile (drawn, edited, saved in tile.wce like the others); nothing of them reaches the
//! kit or the game.

use wce_editor::editor::Editor;
use wce_editor::gui;
use wce_scene::render_scene::DocumentId;
use wce_foundation::math::Float3;
use wce_scene::render_scene::{Overlay, overlay_colour};
use wce_tile::measure::{Angle, Lengths, Measure, angle, format_length, measure};
use wce_tile_editor::tile_document::{ANGLE_FINDER, MEASURE_LINE, Tile, TileDocument};

use crate::tool::{TileTool, document_id, primary};

/// The labels' text and background.
const LABEL_TEXT: u32 = gui::col32(255, 244, 120, 255);
const LABEL_FAINT: u32 = gui::col32(230, 230, 230, 255);
const LABEL_BG: u32 = gui::col32(16, 16, 18, 190);

/// A value to one decimal with its sign, without a "-0.0" for what rounds to nothing.
fn signed1(v: f64) -> String {
    if v.abs() < 0.05 { "0.0".to_owned() } else { format!("{v:+.1}") }
}

/// A grade for people: percent and degrees.
fn grade(l: &Lengths) -> String {
    format!("{} % ({} deg)", signed1(l.grade_percent()), signed1(l.grade_degrees()))
}

/// What the Inspector and `tile_measure` say of a line: its total lengths, one per line.
pub fn summary(m: &Measure) -> Vec<(&'static str, String)> {
    let t = &m.total;
    vec![
        ("Along the ground", format_length(t.ground)),
        ("Straight (3D)", format_length(t.straight)),
        ("On the map", format_length(t.horizontal)),
        ("Rise, end to end", format!("{:+.2} m", t.rise)),
        ("Average grade", grade(&Lengths { horizontal: t.horizontal, rise: t.rise, ..Default::default() })),
    ]
}

/// What the Inspector and `tile_angle` say of an angle finder.
pub fn angle_summary(a: &Angle) -> Vec<(&'static str, String)> {
    let arm = |l: &Lengths| format!("{} ({} on the map), {} %", format_length(l.ground), format_length(l.horizontal), signed1(l.grade_percent()));
    vec![
        ("Angle on the map", format!("{:.1} deg", a.on_map)),
        ("Angle in space", format!("{:.1} deg", a.in_space)),
        ("Arm to point 0", arm(&a.arms[0])),
        ("Arm to point 2", arm(&a.arms[1])),
    ]
}

/// An angle finder's label: the angle on the map, and in space when that differs by a tenth of a degree or more.
fn angle_text(a: &Angle) -> String {
    if (a.on_map - a.in_space).abs() >= 0.05 { format!("{:.1} deg (in space {:.1})", a.on_map, a.in_space) } else { format!("{:.1} deg", a.on_map) }
}

/// The angle at the middle of three points (world metres) on the tile's ground.
fn angle_world(t: &Tile, pts: &[Float3]) -> Option<Angle> {
    let points = pts.iter().map(|p| [f64::from(p.x), f64::from(t.terrain_height(p.x, p.z)), f64::from(p.z)]).collect::<Vec<_>>();
    angle(&points, &|x, z| f64::from(t.terrain_height(x as f32, z as f32)))
}

/// The arc between an angle finder's arms at its corner, on the ground: a third of the shorter arm, 1 .. 25 m.
fn angle_arc(t: &Tile, pts: &[Float3], o: &mut Overlay) {
    let &[a, c, b] = pts else { return };
    let (ua, ub) = ((a.x - c.x).atan2(a.z - c.z), (b.x - c.x).atan2(b.z - c.z));
    let r = ((a.x - c.x).hypot(a.z - c.z).min((b.x - c.x).hypot(b.z - c.z)) / 3.0).clamp(1.0, 25.0);
    let mut sweep = ub - ua;
    while sweep > std::f32::consts::PI {
        sweep -= std::f32::consts::TAU;
    }

    while sweep < -std::f32::consts::PI {
        sweep += std::f32::consts::TAU;
    }

    let col = overlay_colour(0.45, 0.85, 1.0, 1.0);
    let at = |u: f32| {
        let (x, z) = (c.x + u.sin() * r, c.z + u.cos() * r);
        Float3::new(x, t.terrain_height(x, z) + 0.4, z)
    };

    let n = 32;
    for i in 0..n {
        let (u0, u1) = (ua + sweep * i as f32 / n as f32, ua + sweep * (i + 1) as f32 / n as f32);
        o.line(at(u0), at(u1), col);
    }
}

impl TileTool {
    /// The arcs of the angle finders shown, and of the one being drawn (the tool's overlay).
    pub(crate) fn angle_overlay(&self, e: &Editor, t: &Tile, o: &mut Overlay) {
        for elem in t.shown_measures().into_iter().filter(|&el| t.is_angle_finder(el)) {
            angle_arc(t, &t.measure_points(elem), o);
        }

        if let Some(pts) = self.drawn_angle(e) {
            angle_arc(t, &pts, o);
        }
    }

    /// The angle finder being drawn, with the mouse as its last point: its three points.
    fn drawn_angle(&self, e: &Editor) -> Option<Vec<Float3>> {
        if !self.drawing || !self.draft.as_ref().is_some_and(|d| d.kind == ANGLE_FINDER) || self.draw_pts.len() != 2 || !(self.terrain_hit && e.viewport.hovered) {
            return None;
        }

        Some(vec![self.draw_pts[0], self.draw_pts[1], self.snapped_cursor])
    }

    /// The selected angle finder in the Inspector, below the grid.
    pub(crate) fn inspect_angle(&self, e: &Editor, id: DocumentId) {
        let Some(d) = e.docs.find_as::<TileDocument>(id) else { return };
        let Some(a) = primary(e).and_then(|s| d.tile().angle_finder(s)) else { return };
        gui::separator_text("Angle finder");
        for (label, value) in angle_summary(&a) {
            gui::row(label);
            gui::text(&value);
        }

        gui::push_style_color(gui::COL_TEXT, gui::style_color(gui::COL_TEXT_DISABLED));
        gui::text_wrapped(
            "The corner is point 1. On the map: seen from above; in space: with the arms' rises. Arms: along the ground (on the map), grade from \
             the corner out. Editor only: kept in tile.wce, never written for the Assembly Kit or the game.",
        );
        gui::pop_style_color(1);
    }

    /// The Inspector below the grid: the selected tape measure's lengths, the whole line's and each stretch's.
    pub(crate) fn inspect_measure(&self, e: &Editor, id: DocumentId) {
        let Some(d) = e.docs.find_as::<TileDocument>(id) else { return };
        let Some((elem, m)) = primary(e).and_then(|s| Some((s, d.tile().measure_line(s)?))) else { return };
        gui::separator_text("Tape measure");
        for (label, value) in summary(&m) {
            gui::row(label);
            gui::text(&value);
        }

        if m.stretches.len() > 1 {
            gui::spacing();
            let flags = gui::TABLE_SIZING_STRETCH_PROP | gui::TABLE_ROW_BG | gui::TABLE_BORDERS_INNER_V;
            if gui::begin_table("measure_stretches", 5, flags, gui::v2(0.0, 0.0)) {
                for h in ["Points", "Ground", "Straight", "Map", "Grade"] {
                    gui::table_setup_column(h, 0, 1.0);
                }

                gui::table_headers_row();
                for (i, s) in m.stretches.iter().enumerate() {
                    gui::table_next_row();
                    let lit = self.poly_point.is_some_and(|p| p == i || p == i + 1);
                    let cells = [format!("{i}-{}", i + 1), format_length(s.ground), format_length(s.straight), format_length(s.horizontal), format!("{} %", signed1(s.grade_percent()))];
                    for c in cells {
                        gui::table_next_column();
                        if lit { gui::text_colored([1.0, 0.95, 0.45, 1.0], &c) } else { gui::text(&c) }
                    }
                }

                gui::end_table();
            }
        }

        gui::push_style_color(gui::COL_TEXT, gui::style_color(gui::COL_TEXT_DISABLED));
        gui::text_wrapped(&format!(
            "{} point(s). Editor only: kept in tile.wce, never written for the Assembly Kit or the game. The ground is sampled every {} m.",
            d.tile().tscene().polygon_points(elem).len(),
            wce_tile::measure::GROUND_STEP
        ));
        gui::pop_style_color(1);
    }

    /// The labels on the viewport: every tape measure shown has its length along the ground at its last point (the selected one each
    /// stretch's too, at its middle), and the one being drawn its length so far at the mouse.
    pub(crate) fn draw_measure_labels(&self, e: &Editor, origin: gui::Vec2) {
        let Some(d) = document_id(e).and_then(|id| e.docs.find_as::<TileDocument>(id)) else { return };
        let t = d.tile();
        let density = gui::io().DisplayFramebufferScale.x.max(1e-3);
        let screen = |p: Float3| e.viewport.project(p).map(|(x, y)| gui::v2(origin.x + x / density, origin.y + y / density));
        let dl = gui::window_draw_list();
        let label = |at: Float3, text: &str, col: u32| {
            let Some(p) = screen(Float3::new(at.x, at.y + 1.0, at.z)) else { return };
            let size = gui::calc_text_size(text);
            let lo = gui::v2(p.x - size.x * 0.5 - 4.0, p.y - size.y - 6.0);
            dl.rect_filled(lo, gui::v2(lo.x + size.x + 8.0, lo.y + size.y + 4.0), LABEL_BG, 3.0);
            dl.text(gui::v2(lo.x + 4.0, lo.y + 2.0), col, text);
        };

        let selected = primary(e);
        for elem in t.shown_measures() {
            let (Some(m), points) = (t.measure_line(elem), t.measure_points(elem)) else { continue };
            let Some(&last) = points.last() else { continue };
            if selected == Some(elem) && m.stretches.len() > 1 {
                for (s, w) in m.stretches.iter().zip(points.windows(2)) {
                    let mid = Float3::new((w[0].x + w[1].x) * 0.5, 0.0, (w[0].z + w[1].z) * 0.5);
                    label(Float3::new(mid.x, t.terrain_height(mid.x, mid.z), mid.z), &format_length(s.ground), LABEL_FAINT);
                }
            }

            label(last, &total_text(&m), LABEL_TEXT);
        }

        for elem in t.shown_measures() {
            let Some(a) = t.angle_finder(elem) else { continue };
            if let Some(&corner) = t.measure_points(elem).get(1) {
                label(corner, &angle_text(&a), LABEL_TEXT);
            }
        }

        if let Some(pts) = self.drawn_angle(e)
            && let Some(a) = angle_world(t, &pts)
        {
            label(pts[1], &angle_text(&a), LABEL_TEXT);
        }

        // The line being drawn, to the mouse.
        if self.drawing && self.draft.as_ref().is_some_and(|d| d.kind == MEASURE_LINE) && !self.draw_pts.is_empty() {
            let mut pts = self.draw_pts.clone();
            if self.terrain_hit && e.viewport.hovered {
                pts.push(self.snapped_cursor);
            }

            if pts.len() >= 2 {
                let m = measure_world(t, &pts);
                label(*pts.last().unwrap_or(&pts[0]), &total_text(&m), LABEL_TEXT);
            }
        }
    }
}

/// A line's label: along the ground, and straight when that differs by a centimetre or more.
fn total_text(m: &Measure) -> String {
    let t = &m.total;
    if (t.ground - t.straight).abs() >= 0.01 {
        format!("{} (straight {})", format_length(t.ground), format_length(t.straight))
    } else {
        format_length(t.ground)
    }
}

/// The lengths of a line through these points (world metres) on the tile's ground.
fn measure_world(t: &Tile, pts: &[Float3]) -> Measure {
    let points = pts.iter().map(|p| [f64::from(p.x), f64::from(t.terrain_height(p.x, p.z)), f64::from(p.z)]).collect::<Vec<_>>();
    measure(&points, &|x, z| f64::from(t.terrain_height(x as f32, z as f32)))
}
