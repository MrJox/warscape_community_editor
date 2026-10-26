//! The Brush Falloff window (TEd's Brush Falloff Editor): the height and paint brushes' weight by the distance from their centre, as a curve
//! through points (`wce_tile::falloff`). Drag a point; double-click adds one; right-click takes one away (not the ends); the presets
//! replace them. Kept in the settings (`[tools.tile] brush_curve`).

use wce_editor::editor::Editor;
use wce_editor::{gui, theme};
use wce_tile::falloff::{FalloffCurve, PRESETS};

use crate::settings::{BrushCurve, SETTINGS_ID, TileSettings};
use crate::tool::TileTool;

/// The canvas, pixels.
const W: f32 = 280.0;
const H: f32 = 160.0;

/// How near (pixels) a click must be to take a point.
const PICK: f32 = 8.0;

impl TileTool {
    /// The brush's curve setting, read from the settings the first time.
    pub(crate) fn brush_curve(&mut self, e: &Editor) -> BrushCurve {
        self.curve.get_or_insert_with(|| e.tool_settings::<TileSettings>(SETTINGS_ID).brush_curve).clone()
    }

    /// A new curve setting: into the brush (its table), and into the settings when `keep` (not at every step of a drag).
    pub(crate) fn set_brush_curve(&mut self, e: &mut Editor, c: BrushCurve, keep: bool) {
        self.brush.curve = c.on.then(|| FalloffCurve::from_points(&c.points).lut());
        self.curve = Some(c.clone());
        self.falloff_unsaved = !keep;
        if keep {
            e.update_tool_settings(SETTINGS_ID, |s: &mut TileSettings| s.brush_curve = c);
        }
    }

    /// Puts the saved curve into the brush once.
    pub(crate) fn sync_brush_curve(&mut self, e: &Editor) {
        if self.curve.is_none() {
            let c = self.brush_curve(e);
            self.brush.curve = c.on.then(|| FalloffCurve::from_points(&c.points).lut());
        }
    }

    pub(crate) fn draw_falloff_window(&mut self, e: &mut Editor) {
        if !self.falloff_open {
            return;
        }

        let mut open = true;
        gui::set_next_window_size(gui::v2(W + 30.0, H + 120.0), gui::COND_FIRST_USE_EVER);
        if !gui::begin("Brush Falloff", Some(&mut open), gui::WINDOW_NO_DOCKING | gui::WINDOW_ALWAYS_AUTO_RESIZE) {
            gui::end();
            self.falloff_open = open;
            return;
        }

        let mut c = self.brush_curve(e);
        let before = c.clone();
        gui::checkbox("use the curve", &mut c.on);
        if gui::is_item_hovered() {
            gui::set_tooltip("Off: the brushes' falloff slider");
        }

        // The canvas: distance from the centre left to right, weight bottom to top.
        let origin = gui::cursor_screen_pos();
        gui::invisible_button("##falloff_canvas", gui::v2(W, H), 0);
        let (hovered, active) = (gui::is_item_hovered(), gui::is_item_active());
        let to_screen = |p: [f32; 2]| gui::v2(origin.x + p[0] * W, origin.y + (1.0 - p[1]) * H);
        let m = gui::mouse_pos();
        let at_mouse = [((m.x - origin.x) / W).clamp(0.0, 1.0), (1.0 - (m.y - origin.y) / H).clamp(0.0, 1.0)];
        let near = c.points.iter().position(|&p| {
            let s = to_screen(p);
            (s.x - m.x).hypot(s.y - m.y) <= PICK
        });

        if hovered && gui::is_mouse_clicked(0) {
            self.falloff_drag = near;
        }

        if !active {
            self.falloff_drag = None;
        }

        if let Some(i) = self.falloff_drag.filter(|&i| i < c.points.len()) {
            // The ends stay at the centre and the edge; a point stays between its neighbours.
            let last = c.points.len() - 1;
            let x = if i == 0 { 0.0 } else if i == last { 1.0 } else { at_mouse[0].clamp(c.points[i - 1][0] + 0.01, c.points[i + 1][0] - 0.01) };
            c.points[i] = [x, at_mouse[1]];
        } else if hovered && gui::is_mouse_double_clicked(0) && near.is_none() {
            c.points.push(at_mouse);
            c.points = FalloffCurve::from_points(&c.points).points;
        } else if hovered && gui::is_mouse_clicked(1)
            && let Some(i) = near.filter(|&i| i != 0 && i + 1 != c.points.len())
        {
            c.points.remove(i);
        }

        let dl = gui::window_draw_list();
        let (dim, grid, line, dot) = (theme::u32(theme::palette_background()), gui::col32(90, 90, 90, 120), theme::u32(theme::text()), gui::col32(250, 190, 60, 255));
        dl.rect_filled(origin, gui::v2(origin.x + W, origin.y + H), dim, 0.0);
        for k in 1..4 {
            let f = k as f32 / 4.0;
            dl.line(gui::v2(origin.x + f * W, origin.y), gui::v2(origin.x + f * W, origin.y + H), grid, 1.0);
            dl.line(gui::v2(origin.x, origin.y + f * H), gui::v2(origin.x + W, origin.y + H * f), grid, 1.0);
        }

        let curve = FalloffCurve::from_points(&c.points);
        let col = if c.on { line } else { grid };
        let mut prev = to_screen([0.0, curve.eval(0.0)]);
        for s in 1..=64 {
            let x = s as f32 / 64.0;
            let next = to_screen([x, curve.eval(x)]);
            dl.line(prev, next, col, 2.0);
            prev = next;
        }

        for (i, &p) in c.points.iter().enumerate() {
            dl.circle_filled(to_screen(p), if Some(i) == near || Some(i) == self.falloff_drag { 6.0 } else { 4.5 }, dot);
        }

        dl.rect(origin, gui::v2(origin.x + W, origin.y + H), grid, 0.0, 1.0);

        // The axis: the brush's centre on the left, its edge on the right.
        let below = gui::cursor_screen_pos();
        let label = theme::u32(theme::dim());
        dl.text(below, label, "centre");
        dl.text(gui::v2(origin.x + W - 30.0, below.y), label, "edge");
        gui::dummy(gui::v2(W, gui::text_line_height()));
        for (k, (name, points)) in PRESETS.iter().enumerate() {
            if k != 0 {
                gui::same_line();
            }

            if gui::small_button(name) {
                c.points = points.to_vec();
                c.on = true;
            }
        }

        gui::text_disabled("Drag a point; double-click adds one; right-click takes one away.");
        gui::end();
        self.falloff_open = open;
        if c != before || (self.falloff_unsaved && !active) {
            self.set_brush_curve(e, c, !active);
        }
    }
}
