//! Tape measures (`WceMeasureLine`, TEd's Tape Measure): the selected one's lengths in the Inspector, and labels in the viewport on every
//! one shown and on the one being drawn. The lines themselves are polygons of the tile (drawn, edited, saved in tile.wce like the others);
//! nothing of them reaches the kit or the game.

use wce_editor::editor::Editor;
use wce_editor::gui;
use wce_scene::render_scene::DocumentId;
use wce_foundation::math::Float3;
use wce_tile::measure::{Lengths, Measure, format_length, measure};
use wce_tile_editor::tile_document::{MEASURE_LINE, Tile, TileDocument};

use crate::tool::{TileTool, document_id, primary};

/// The labels' text and background.
const LABEL_TEXT: u32 = gui::col32(255, 244, 120, 255);
const LABEL_FAINT: u32 = gui::col32(230, 230, 230, 255);
const LABEL_BG: u32 = gui::col32(16, 16, 18, 190);

/// A grade for people: percent and degrees.
fn grade(l: &Lengths) -> String {
    format!("{:+.1} % ({:+.1} deg)", l.grade_percent(), l.grade_degrees())
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

impl TileTool {
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
                    let cells = [format!("{i}-{}", i + 1), format_length(s.ground), format_length(s.straight), format_length(s.horizontal), format!("{:+.1} %", s.grade_percent())];
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
