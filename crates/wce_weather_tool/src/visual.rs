//! The Weather tool's visual editing of an .environment / .lighting file (docs/editor.md, "Weather tool"):
//! - the sun moved in the viewport: its disc dragged in the sky, or Ctrl+L held and the mouse moved anywhere (Unreal's "move the sun");
//! - the depth of field's focus picked with a click on what should be sharp;
//! - the saved file shown in place of the edits (compare: the check box, or C held over the viewport);
//! - the tone curve the file gives and the brightness histogram of the frame.
//!
//! The edits go through the document as the Inspector's do (`XmlFileDocument::edit_setting`): undo, the Changes tab and Save see them.

use wce_editor::editor::Editor;
use wce_editor::shortcuts::{Action, Scope};
use wce_editor::tool::{ViewportEvent, ViewportEventKind};
use wce_editor::{gui, shortcuts, theme};
use wce_editor_core::document::Document;
use wce_formats::xml_doc::XmlDoc;
use wce_foundation::math::Float3;
use wce_scene::environment::{EnvironmentParams, read_environment};
use wce_scene::render_scene::DocumentId;
use wce_weather::xml_file_document::XmlFileDocument;

const SUN_DRAG: &str = "weather.sun_drag";
const COMPARE: &str = "weather.compare";

/// The Weather tool's keys (Settings > Shortcuts).
pub(crate) const SHORTCUTS: &[Action] = &[
    Action {
        id: SUN_DRAG,
        scope: Scope::Tool("weather"),
        group: "Weather",
        label: "Move the sun (hold and move the mouse over the viewport)",
        defaults: &["Ctrl+L"],
        held: true,
    },
    Action {
        id: COMPARE,
        scope: Scope::Tool("weather"),
        group: "Weather",
        label: "Show the saved file (hold over the viewport)",
        defaults: &["C"],
        held: true,
    },
];

/// Degrees the sun turns per point of mouse motion while Ctrl+L is held.
const SUN_DEG_PER_POINT: f32 = 0.25;

/// The sun's disc in the viewport: its radius, and how near a press must be to grab it (points).
const SUN_RADIUS: f32 = 9.0;
const SUN_GRAB: f32 = 16.0;

/// The histogram's bins and how often the frame is read back for it (seconds).
const BINS: usize = 64;
const HISTOGRAM_EVERY: f64 = 0.5;

/// The tone curve's and the histogram's size, side by side (points).
const PLOT_W: f32 = 240.0;
const PLOT_H: f32 = 110.0;

#[derive(Default)]
pub(crate) struct Visual {
    /// A sun drag going on: the history position it started at (its edits are one entry, made again each frame).
    sun_from: Option<usize>,

    /// Ctrl+L's drag: the azimuth and elevation it has reached (degrees, the file's).
    sun_key: Option<(f32, f32)>,

    /// Where the sun's disc was drawn last frame (screen points).
    sun_screen: Option<gui::Vec2>,

    /// The mouse was over the viewport last frame.
    hovered: bool,

    /// The next click in the viewport sets the depth of field's focus.
    pub(crate) focus_armed: bool,

    /// The saved file lights the scene instead of the edits (the check box); C held over the viewport does it too.
    pub(crate) compare: bool,
    compare_key: bool,

    /// The saved file's parameters while comparing (the document they are of).
    baseline: Option<(DocumentId, EnvironmentParams)>,

    /// Opens or closes the Tone curve and histogram section on the next frame (`weather_tone_panel`).
    pub(crate) open_tone: Option<bool>,

    /// The frame's brightness histogram: the bins, the pixels counted, those clipped white and crushed black, when it was read.
    hist: Vec<u32>,
    hist_total: u32,
    hist_clipped: u32,
    hist_crushed: u32,
    hist_at: f64,
}

/// The file's parameters as saved: the project's loose copy or the packs' (a file of the game), else the file on disk.
fn saved_params(e: &Editor, x: &XmlFileDocument) -> Option<EnvironmentParams> {
    let src = x.source();
    let bytes = if src.vfs_path.is_empty() { std::fs::read(&src.disk).ok()? } else { e.vfs()?.read(&src.vfs_path).ok()? };
    XmlDoc::parse(bytes).ok().map(|d| read_environment(&d))
}

/// The sun's (azimuth, elevation) in degrees for a direction pointing at it (the inverse of `EnvironmentParams::to_sun`).
fn sun_angles(d: Float3) -> (f32, f32) {
    let az = d.x.atan2(d.z).to_degrees();
    let el = d.y.clamp(-1.0, 1.0).asin().to_degrees();
    (az, el)
}

/// An angle in [0, 360), to a tenth of a degree.
fn wrap_deg(a: f32) -> f32 {
    ((a.rem_euclid(360.0) * 10.0).round() / 10.0) % 360.0
}

fn tenth(a: f32) -> f32 {
    (a * 10.0).round() / 10.0
}

impl Visual {
    /// Where the sun's disc was drawn last frame, in pixels of the viewport image (`weather_sun_disc`, for scripted drags).
    pub(crate) fn sun_disc_px(&self, e: &Editor) -> Option<(i32, i32)> {
        let s = self.sun_screen?;
        let density = gui::io().DisplayFramebufferScale.x.max(1e-3);
        let origin = e.viewport.image_origin();
        Some((((s.x - origin.x) * density) as i32, ((s.y - origin.y) * density) as i32))
    }

    pub(crate) fn comparing(&self) -> bool {
        self.compare || self.compare_key
    }

    /// While comparing, the saved file's parameters (read once per compare), to light the scene instead of the edits.
    pub(crate) fn compare_params(&mut self, e: &Editor, doc: DocumentId) -> Option<EnvironmentParams> {
        self.compare_key = self.hovered && !gui::io().WantTextInput && shortcuts::down(COMPARE);
        if !self.comparing() || doc == 0 {
            self.baseline = None;
            return None;
        }

        if self.baseline.as_ref().is_none_or(|(d, _)| *d != doc) {
            let x = e.docs.find_as::<XmlFileDocument>(doc)?;
            self.baseline = saved_params(e, x).map(|p| (doc, p));
        }

        self.baseline.as_ref().map(|(_, p)| p.clone())
    }

    /// The file's sun as written (azimuth, elevation), and how far the scene turns it (the shown azimuth minus the file's: a tile's battle
    /// frame).
    fn file_sun(e: &Editor, doc: DocumentId, shown: &EnvironmentParams) -> Option<(f32, f32, f32)> {
        let x = e.docs.find_as::<XmlFileDocument>(doc)?;
        let p = read_environment(x.xml());
        Some((p.sun_azimuth, p.sun_elevation, shown.sun_azimuth - p.sun_azimuth))
    }

    /// Writes the sun (the file's degrees) as one undo entry from `from`: the entry of the frame before is taken back first.
    fn set_sun(e: &mut Editor, doc: DocumentId, from: usize, az: f32, el: f32) {
        let Some(x) = e.docs.find_as_mut::<XmlFileDocument>(doc) else { return };
        if x.history().position() != from {
            x.history_mut().jump_to(from);
            x.history_mut().drop_redo();
        }

        let (az, el) = (wrap_deg(az), tenth(el.clamp(0.0, 90.0)));
        let r = x
            .edit_setting("LIGHTING", "sun_direction_azimuth", 0, |f| f[0] = az)
            .and_then(|()| x.edit_setting("LIGHTING", "sun_direction_elevation", 0, |f| f[0] = el));
        if let Err(m) = r {
            e.status(&m, true);
            return;
        }

        let x = e.docs.find_as_mut::<XmlFileDocument>(doc).expect("the document");
        let n = x.history().position().saturating_sub(from);
        if n > 1 {
            x.join_last(n, "Move the sun");
        }
    }

    /// The viewport's events: a press on the sun's disc drags it, a click with the focus armed sets the focus. True when taken.
    pub(crate) fn viewport_input(&mut self, e: &mut Editor, doc: DocumentId, shown: &EnvironmentParams, ev: &ViewportEvent) -> bool {
        if ev.kind == ViewportEventKind::Hover {
            self.hovered = true;
            self.sun_key_drag(e, doc, shown);
            return false;
        }

        if doc == 0 || ev.button != 0 {
            return false;
        }

        let density = gui::io().DisplayFramebufferScale.x.max(1e-3);
        let origin = e.viewport.image_origin();
        let at = gui::v2(origin.x + ev.x as f32 / density, origin.y + ev.y as f32 / density);
        match ev.kind {
            ViewportEventKind::Press if self.focus_armed => {
                self.focus_armed = false;
                self.pick_focus(e, doc, ev.x, ev.y);
                true
            }

            ViewportEventKind::Press => {
                let near = self.sun_screen.is_some_and(|s| (s.x - at.x).hypot(s.y - at.y) <= SUN_GRAB);
                if near && !self.comparing() {
                    self.sun_from = e.docs.find(doc).map(|d| d.history().position());
                }

                self.sun_from.is_some()
            }

            ViewportEventKind::Drag => {
                let Some(from) = self.sun_from else { return false };
                let Some((_, d)) = e.viewport.ray(ev.x, ev.y) else { return true };
                let Some((_, _, yaw)) = Self::file_sun(e, doc, shown) else { return true };
                let (az, el) = sun_angles(d);
                Self::set_sun(e, doc, from, az - yaw, el);
                true
            }

            ViewportEventKind::Release => {
                if self.sun_from.take().is_some()
                    && let Some(d) = e.docs.find_mut(doc)
                {
                    d.history_mut().break_merge();
                }

                false
            }

            _ => false,
        }
    }

    /// Ctrl+L held over the viewport: the mouse's motion turns the sun (sideways: azimuth, up and down: elevation), one undo entry per hold.
    fn sun_key_drag(&mut self, e: &mut Editor, doc: DocumentId, shown: &EnvironmentParams) {
        let held = doc != 0 && !self.comparing() && !gui::io().WantTextInput && shortcuts::down(SUN_DRAG);
        if !held {
            if self.sun_key.take().is_some() {
                self.sun_from = None;
                if let Some(d) = e.docs.find_mut(doc) {
                    d.history_mut().break_merge();
                }
            }

            return;
        }

        if self.sun_key.is_none() {
            let Some((az, el, _)) = Self::file_sun(e, doc, shown) else { return };
            self.sun_key = Some((az, el));
            self.sun_from = e.docs.find(doc).map(|d| d.history().position());
        }

        let delta = gui::io().MouseDelta;
        if delta.x == 0.0 && delta.y == 0.0 {
            return;
        }

        let (Some((az, el)), Some(from)) = (self.sun_key, self.sun_from) else { return };
        let (az, el) = (az + delta.x * SUN_DEG_PER_POINT, (el - delta.y * SUN_DEG_PER_POINT).clamp(0.0, 90.0));
        self.sun_key = Some((az, el));
        Self::set_sun(e, doc, from, az, el);
    }

    /// The focus distance of the depth of field: how far in front of the camera the surface under the pixel is.
    fn pick_focus(&mut self, e: &mut Editor, doc: DocumentId, x: i32, y: i32) {
        let point = wce_editor::viewport::surface_under(e, x, y, &|_| false).map(|h| h.point);
        let point = point.or_else(|| {
            let scene = &e.scene;
            e.viewport.ground_under(scene, x, y)
        });
        let Some(p) = point else {
            e.status("focus: nothing under the cursor (the sky)", true);
            return;
        };

        // ASSUMPTION: the shaders compare the focus with the view depth (along the camera's axis), not the distance to the eye.
        let cam = e.viewport.camera();
        let axis = cam.target - cam.eye;
        let len = (axis.x * axis.x + axis.y * axis.y + axis.z * axis.z).sqrt().max(1e-6);
        let depth = ((p.x - cam.eye.x) * axis.x + (p.y - cam.eye.y) * axis.y + (p.z - cam.eye.z) * axis.z) / len;
        let depth = tenth(depth.max(0.1));
        let Some(x) = e.docs.find_as_mut::<XmlFileDocument>(doc) else { return };
        match x.edit_setting("DEPTH_OF_FIELD", "DOF_focus", 0, |f| f[0] = depth) {
            Ok(()) => {
                let on = read_environment(x.xml()).use_dof;
                let note = if on { "" } else { " (use_DOF is off: turn it on in the Inspector's Depth of field to see it)" };
                e.status(&format!("focus: {depth} m{note}"), false);
            }

            Err(m) => e.status(&m, true),
        }
    }

    /// True while the left button is the tool's: the focus pick armed, or the sun being dragged.
    pub(crate) fn claims_left_button(&self) -> bool {
        self.focus_armed || self.sun_from.is_some()
    }

    /// Over the viewport: the sun's disc in the sky (grab it to move the sun), the focus pick's hint, the compare label.
    pub(crate) fn overlay(&mut self, e: &mut Editor, doc: DocumentId, shown: &EnvironmentParams, origin: gui::Vec2, size: gui::Vec2) {
        self.hovered = false;
        self.sun_screen = None;
        let dl = gui::window_draw_list();
        let density = gui::io().DisplayFramebufferScale.x.max(1e-3);
        if doc != 0 && !self.comparing() {
            let cam = e.viewport.camera();
            let far = cam.zfar * 0.5;
            let s = shown.to_sun();
            let sun = Float3::new(cam.eye.x + s.x * far, cam.eye.y + s.y * far, cam.eye.z + s.z * far);
            if let Some((px, py)) = e.viewport.project(sun) {
                let at = gui::v2(origin.x + px / density, origin.y + py / density);
                let inside = at.x >= origin.x && at.y >= origin.y && at.x <= origin.x + size.x && at.y <= origin.y + size.y;
                if inside {
                    let mouse = gui::io().MousePos;
                    let hot = self.sun_from.is_some() || (mouse.x - at.x).hypot(mouse.y - at.y) <= SUN_GRAB;
                    let fill = if hot { gui::col32(255, 220, 90, 255) } else { gui::col32(255, 210, 80, 170) };
                    dl.circle_filled(at, SUN_RADIUS, fill);
                    dl.ellipse(at, gui::v2(SUN_RADIUS + 3.0, SUN_RADIUS + 3.0), gui::col32(20, 20, 20, 200), 24, 1.5);
                    self.sun_screen = Some(at);
                    if hot && self.sun_from.is_none() {
                        gui::set_tooltip(&format!(
                            "The sun: azimuth {:.1}\u{b0}, elevation {:.1}\u{b0}\nDrag it across the sky to move it, or hold {} and move the mouse anywhere.",
                            shown.sun_azimuth.rem_euclid(360.0),
                            shown.sun_elevation,
                            shortcuts::label(SUN_DRAG).unwrap_or_default()
                        ));
                    }
                }
            }
        }

        let top = gui::v2(origin.x + size.x * 0.5, origin.y + 40.0);
        let banner = if self.comparing() {
            Some(("THE SAVED FILE (comparing: uncheck Compare, or let go of C, for the edits)".to_owned(), theme::warning()))
        } else if self.focus_armed {
            Some(("Click what should be in focus (Esc: cancel)".to_owned(), theme::modified()))
        } else if self.sun_key.is_some() || self.sun_from.is_some() {
            Some((
                format!("sun: azimuth {:.1}\u{b0}, elevation {:.1}\u{b0}", shown.sun_azimuth.rem_euclid(360.0), shown.sun_elevation),
                theme::dim(),
            ))
        } else {
            None
        };

        if let Some((text, colour)) = banner {
            let w = gui::calc_text_width(&text);
            let lo = gui::v2(top.x - w * 0.5 - 8.0, top.y - 4.0);
            dl.rect_filled(lo, gui::v2(top.x + w * 0.5 + 8.0, top.y + 18.0), gui::col32(10, 10, 12, 200), 4.0);
            dl.text(gui::v2(top.x - w * 0.5, top.y), theme::u32(colour), &text);
        }

        if self.focus_armed && gui::is_key_pressed(gui::KEY_ESCAPE, false) {
            self.focus_armed = false;
        }
    }

    /// The Weather scene panel's part: Compare, Pick focus, then the tone curve and the histogram (read back only while it is open).
    pub(crate) fn draw_panel(&mut self, e: &mut Editor, doc: DocumentId, shown: &EnvironmentParams) {
        gui::begin_disabled(doc == 0);
        gui::checkbox("Compare with saved", &mut self.compare);
        if gui::is_item_hovered_allow_disabled() {
            gui::set_tooltip(&format!(
                "Lights the scene with the file as saved instead of the edits, to see what they change. Holding {} over the viewport does it too.",
                shortcuts::label(COMPARE).unwrap_or_default()
            ));
        }

        gui::same_line();
        if gui::button("Pick focus") {
            self.focus_armed = true;
        }

        if gui::is_item_hovered_allow_disabled() {
            gui::set_tooltip(
                "Then click what should be sharp in the viewport: its distance becomes the depth of field's focus (DOF_focus).",
            );
        }

        gui::end_disabled();
        if doc != 0 {
            gui::same_line();
            gui::text_disabled(&format!("sun: drag its disc in the sky, or hold {}", shortcuts::label(SUN_DRAG).unwrap_or_default()));
        }

        if let Some(open) = self.open_tone.take() {
            gui::set_next_item_open(open, 0);
        }

        if !gui::collapsing_header("Tone curve and histogram", 0) {
            return;
        }

        if gui::time() - self.hist_at >= HISTOGRAM_EVERY {
            self.read_histogram(e);
        }

        self.draw_curve(shown);
        gui::same_line();
        self.draw_histogram();
        if let Some(r) = e.renderer() {
            gui::text_disabled(&format!("exposure now: black point {:.4}, white point {:.3}", r.luma_black(), r.luma_white()));
            if gui::is_item_hovered() {
                gui::set_tooltip(
                    "The scene brightness the tone map takes as black and as white this frame: the game adapts them to the scene (auto_exposure_speed), within the HDR limits of the Exposure section.",
                );
            }
        }
    }

    /// The S-curve per channel (the HDR section's low and high tones biases): the cubic Bezier the engine bakes (`build_scurve`).
    fn draw_curve(&self, p: &EnvironmentParams) {
        let (lo, size) = (gui::cursor_screen_pos(), gui::v2(PLOT_W, PLOT_H));
        let hi = gui::v2(lo.x + size.x, lo.y + size.y);
        let dl = gui::window_draw_list();
        dl.rect_filled(lo, hi, gui::col32(18, 18, 20, 255), 2.0);
        dl.line(gui::v2(lo.x, hi.y), gui::v2(hi.x, lo.y), gui::col32(80, 80, 80, 255), 1.0);
        let colours = [gui::col32(235, 80, 80, 255), gui::col32(90, 210, 90, 255), gui::col32(90, 140, 255, 255)];
        for (ch, &colour) in colours.iter().enumerate() {
            let (b, c) = (p.low_tones_bias[ch], p.high_tones_bias[ch]);
            let pts = (0..=48)
                .map(|k| {
                    let t = k as f32 / 48.0;
                    let u = 1.0 - t;
                    let x = 3.0 * u * u * t / 3.0 + 3.0 * u * t * t * 2.0 / 3.0 + t * t * t;
                    let y = 3.0 * u * u * t * b + 3.0 * u * t * t * c + t * t * t;
                    gui::v2(lo.x + x * size.x, hi.y - y.clamp(-0.1, 1.1) * size.y)
                })
                .collect::<Vec<_>>();
            dl.polyline(&pts, colour, false, 1.5);
        }

        dl.text(gui::v2(lo.x + 4.0, lo.y + 2.0), theme::u32(theme::dim()), "tone curve");
        gui::dummy(size);
        if gui::is_item_hovered() {
            gui::set_tooltip(
                "How the scene's brightness (left to right, between the black and white points) becomes the image's (bottom to top), per \
                 channel: the Exposure section's low_tones_bias and high_tones_bias. The diagonal changes nothing.",
            );
        }
    }

    /// Reads the viewport's image back and bins its brightness.
    fn read_histogram(&mut self, e: &Editor) {
        self.hist_at = gui::time();
        let Some((w, h, px)) = e.read_viewport_rgba8() else { return };
        let mut bins = vec![0u32; BINS];
        let (mut total, mut clipped, mut crushed) = (0u32, 0u32, 0u32);
        let step = 3;
        for y in (0..h as usize).step_by(step) {
            for x in (0..w as usize).step_by(step) {
                let i = (y * w as usize + x) * 4;
                let Some(&[r, g, b, _]) = px.get(i..i + 4).and_then(|s| <&[u8; 4]>::try_from(s).ok()) else { continue };
                let luma = 0.2126 * f32::from(r) + 0.7152 * f32::from(g) + 0.0722 * f32::from(b);
                bins[((luma / 256.0) * BINS as f32) as usize % BINS] += 1;
                total += 1;
                clipped += u32::from(r == 255 || g == 255 || b == 255);
                crushed += u32::from(r <= 2 && g <= 2 && b <= 2);
            }
        }

        (self.hist, self.hist_total, self.hist_clipped, self.hist_crushed) = (bins, total, clipped, crushed);
    }

    fn draw_histogram(&self) {
        if self.hist.is_empty() || self.hist_total == 0 {
            return;
        }

        let (lo, size) = (gui::cursor_screen_pos(), gui::v2(PLOT_W, PLOT_H));
        let hi = gui::v2(lo.x + size.x, lo.y + size.y);
        let dl = gui::window_draw_list();
        dl.rect_filled(lo, hi, gui::col32(18, 18, 20, 255), 2.0);
        let top = self.hist.iter().copied().max().unwrap_or(1).max(1) as f32;
        let bw = size.x / BINS as f32;
        for (i, &n) in self.hist.iter().enumerate() {
            let hgt = (n as f32 / top).sqrt() * (size.y - 20.0);
            let x = lo.x + i as f32 * bw;
            dl.rect_filled(gui::v2(x, hi.y - hgt), gui::v2(x + bw - 1.0, hi.y), gui::col32(150, 150, 150, 255), 0.0);
        }

        let pct = |n: u32| 100.0 * n as f32 / self.hist_total as f32;
        let (c, k) = (pct(self.hist_clipped), pct(self.hist_crushed));
        let colour = if c > 2.0 || k > 2.0 { theme::warning() } else { theme::dim() };
        dl.text(gui::v2(lo.x + 4.0, lo.y + 2.0), theme::u32(colour), &format!("this frame  clipped {c:.1}%  crushed {k:.1}%"));
        gui::dummy(size);
        if gui::is_item_hovered() {
            gui::set_tooltip(
                "How bright the viewport's pixels are, dark on the left, bright on the right (read twice a second).\nClipped: pixels \
                 with a channel at full white; crushed: pixels at black. Detail is lost at either end (over 2% shows in amber).",
            );
        }
    }
}
