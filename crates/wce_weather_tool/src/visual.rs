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

/// The preview switches: what each takes out of the light shown (not out of the file), and what it does.
pub(crate) const SWITCHES: [(&str, &str); 6] = [
    ("fog", "The battle fog (distance and height fog) taken out of the preview."),
    ("clouds", "The cloud layers taken out of the preview (the sky and the cloud shade on the ground)."),
    ("precipitation", "The weather definition's effects (rain, snow, dust) taken out of the preview."),
    ("tone curve", "The Exposure section's S-curve made straight in the preview (low and high tones bias 1/3 and 2/3)."),
    ("colour grading", "The Colour grading section made neutral in the preview (overlay, brightness, contrast, saturation 1, hue 0)."),
    ("depth of field", "The depth of field turned off in the preview."),
];
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

    /// The disc is being dragged (`sun_from` is its start); else `sun_from` is Ctrl+L's.
    sun_disc: bool,

    /// Ctrl+L's drag: the azimuth and elevation it has reached (degrees, the file's).
    sun_key: Option<(f32, f32)>,

    /// Where the sun's disc was drawn last frame (screen points).
    sun_screen: Option<gui::Vec2>,

    /// The wind arrow: a drag of its head going on (the history position it started at), where its head was drawn last frame (screen
    /// points), and the ground point it starts from with the height of the plane the drag runs on.
    wind_from: Option<usize>,
    wind_tip: Option<gui::Vec2>,
    wind_base: Option<Float3>,

    /// The Mix section shows a cross-fade: the handles are off (what is shown is not the file).
    pub(crate) mixing: bool,

    /// The mouse was over the viewport last frame.
    hovered: bool,

    /// The preview switches that are on (`SWITCHES`): those parts of the file are left out of what lights the viewport.
    pub(crate) without: [bool; 6],

    /// Where the fog over the ground reaches 25, 50 and 75 %, as rings around the camera (the Fog distances check box, `weather_fog_rings`).
    pub(crate) fog_rings: bool,

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
        Self::image_px(e, self.sun_screen?)
    }

    /// Where the wind arrow's head was drawn last frame, in pixels of the viewport image (`weather_wind_tip`).
    pub(crate) fn wind_tip_px(&self, e: &Editor) -> Option<(i32, i32)> {
        Self::image_px(e, self.wind_tip?)
    }

    fn image_px(e: &Editor, s: gui::Vec2) -> Option<(i32, i32)> {
        let density = gui::io().DisplayFramebufferScale.x.max(1e-3);
        let origin = e.viewport.image_origin();
        Some((((s.x - origin.x) * density) as i32, ((s.y - origin.y) * density) as i32))
    }

    /// The sun and wind handles work: the file is what the viewport shows (not compared, not mixed).
    fn handles(&self) -> bool {
        !self.comparing() && !self.mixing
    }

    /// Takes out of `p` what the preview switches leave out (the viewport's light only; the file is not touched).
    pub(crate) fn apply_switches(&self, p: &mut EnvironmentParams) {
        let neutral = EnvironmentParams::default();
        if self.without[0] {
            p.fog_distance_scale = 0.0;
            p.fog_height_strength = 0.0;
        }

        if self.without[1] {
            p.cloud_planes.clear();
        }

        if self.without[2] {
            p.weather_definition.clear();
            p.snowing_intensity = 0.0;
        }

        if self.without[3] {
            p.low_tones_bias = [1.0 / 3.0; 3];
            p.high_tones_bias = [2.0 / 3.0; 3];
            p.tones_bias_single = p.tones_bias_single.map(|_| [1.0 / 3.0, 2.0 / 3.0]);
        }

        if self.without[4] {
            (p.overlay, p.brightness, p.contrast, p.saturation, p.hue) =
                (neutral.overlay, neutral.brightness, neutral.contrast, neutral.saturation, neutral.hue);
        }

        if self.without[5] {
            p.use_dof = false;
        }
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
                if !self.handles() {
                    return false;
                }

                // Ctrl+L held owns the sun: the click is swallowed (no second drag of it, no camera turn).
                if self.sun_key.is_some() {
                    return true;
                }

                let at_wind = self.wind_tip.is_some_and(|s| (s.x - at.x).hypot(s.y - at.y) <= SUN_GRAB);
                let at_sun = self.sun_screen.is_some_and(|s| (s.x - at.x).hypot(s.y - at.y) <= SUN_GRAB);
                let here = e.docs.find(doc).map(|d| d.history().position());
                if at_wind {
                    self.wind_from = here;
                } else if at_sun {
                    self.sun_from = here;
                    self.sun_disc = self.sun_from.is_some();
                }

                self.sun_disc || self.wind_from.is_some()
            }

            ViewportEventKind::Drag if self.wind_from.is_some() => {
                self.wind_drag(e, doc, shown, ev.x, ev.y);
                true
            }

            ViewportEventKind::Drag => {
                let Some(from) = self.sun_from.filter(|_| self.sun_disc) else { return self.sun_key.is_some() };
                let Some((_, d)) = e.viewport.ray(ev.x, ev.y) else { return true };
                let Some((_, _, yaw)) = Self::file_sun(e, doc, shown) else { return true };
                let (az, el) = sun_angles(d);
                Self::set_sun(e, doc, from, az - yaw, el);
                true
            }

            // The disc's or the arrow's drag ends; a hold of Ctrl+L goes on (its own end is the key's release).
            ViewportEventKind::Release => {
                let disc = std::mem::take(&mut self.sun_disc);
                if disc {
                    self.sun_from = None;
                }

                if (disc || self.wind_from.take().is_some())
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
        // A drag of the disc owns the sun until its release: Ctrl+L pressed meanwhile waits.
        let held = doc != 0 && !self.sun_disc && self.handles() && !gui::io().WantTextInput && shortcuts::down(SUN_DRAG);
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
                let p = read_environment(x.xml());
                let note = if !p.use_dof {
                    " (use_DOF is off: turn it on in the Inspector's Depth of field to see it)".to_owned()
                } else if p.dof_blur_size > 1.0 {
                    format!(" (DOF_blur_size {} is above 1: the game's shader overshoots, with halos out of focus)", p.dof_blur_size)
                } else {
                    String::new()
                };
                e.status(&format!("focus: {depth} m{note}"), false);
            }

            Err(m) => e.status(&m, true),
        }
    }

    /// A drag of the wind arrow's head: the angle on the ground from the arrow's start to the cursor becomes wind_dir_degrees (the
    /// file's, the scene's turn taken off), to a degree; one undo entry per drag.
    fn wind_drag(&mut self, e: &mut Editor, doc: DocumentId, shown: &EnvironmentParams, x: i32, y: i32) {
        let (Some(from), Some(base)) = (self.wind_from, self.wind_base) else { return };
        let Some((o, d)) = e.viewport.ray(x, y) else { return };
        let s = if d.y.abs() > 1e-4 { (base.y - o.y) / d.y } else { -1.0 };
        if s <= 0.0 {
            return;
        }

        let (vx, vz) = (o.x + d.x * s - base.x, o.z + d.z * s - base.z);
        let Some((_, _, yaw)) = Self::file_sun(e, doc, shown) else { return };
        if vx.hypot(vz) < 1e-3 {
            return;
        }

        // The wind's angle runs from +x towards +z, and the scene's turn takes it the other way round (`Renderer::wind_direction_degrees`).
        let deg = (vz.atan2(vx).to_degrees() + yaw).rem_euclid(360.0).round() % 360.0;
        let Some(x) = e.docs.find_as_mut::<XmlFileDocument>(doc) else { return };
        if x.history().position() != from {
            x.history_mut().jump_to(from);
            x.history_mut().drop_redo();
        }

        if let Err(m) = x.edit_setting("WEATHER", "wind_dir_degrees", 0, |f| f[0] = deg) {
            e.status(&m, true);
        }
    }

    /// The wind arrow on the ground at the camera's target: the way the wind blows (grey when the battle picks a random direction);
    /// its head is the handle.
    fn draw_wind(&mut self, e: &Editor, doc: DocumentId, shown: &EnvironmentParams, origin: gui::Vec2, dl: &gui::DrawList) {
        let Some((_, _, yaw)) = Self::file_sun(e, doc, shown) else { return };
        let density = gui::io().DisplayFramebufferScale.x.max(1e-3);
        let cam = e.viewport.camera();
        let base = match self.wind_base.filter(|_| self.wind_from.is_some()) {
            Some(b) => b,
            None => cam.target,
        };
        self.wind_base = Some(base);
        let (dx, dy, dz) = (cam.eye.x - cam.target.x, cam.eye.y - cam.target.y, cam.eye.z - cam.target.z);
        let len = ((dx * dx + dy * dy + dz * dz).sqrt() * 0.2).clamp(3.0, 400.0);
        let a = (shown.wind_dir_degrees - yaw).to_radians();
        let tip = Float3::new(base.x + a.cos() * len, base.y, base.z + a.sin() * len);
        let (Some((bx, by)), Some((tx, ty))) = (e.viewport.project(base), e.viewport.project(tip)) else { return };
        let (b, t) = (gui::v2(origin.x + bx / density, origin.y + by / density), gui::v2(origin.x + tx / density, origin.y + ty / density));
        let (ux, uy) = (t.x - b.x, t.y - b.y);
        let n = ux.hypot(uy);
        if n < 4.0 {
            return;
        }

        let (ux, uy) = (ux / n, uy / n);
        let mouse = gui::io().MousePos;
        let hot = self.wind_from.is_some() || (mouse.x - t.x).hypot(mouse.y - t.y) <= SUN_GRAB;
        let colour = match (shown.wind_dir_random, hot) {
            (true, _) => gui::col32(160, 160, 160, 200),
            (false, true) => gui::col32(120, 230, 255, 255),
            (false, false) => gui::col32(90, 200, 240, 210),
        };
        let back = gui::v2(t.x - ux * 14.0, t.y - uy * 14.0);
        dl.line(b, back, gui::col32(10, 10, 12, 180), 5.0);
        dl.line(b, back, colour, 3.0);
        dl.triangle_filled(t, gui::v2(back.x - uy * 7.0, back.y + ux * 7.0), gui::v2(back.x + uy * 7.0, back.y - ux * 7.0), colour);
        self.wind_tip = Some(t);
        if hot && self.wind_from.is_none() {
            gui::set_tooltip(&format!(
                "The wind blows this way: wind_dir_degrees {:.0}\u{b0}{}\nDrag the arrowhead to turn it.",
                shown.wind_dir_degrees.rem_euclid(360.0),
                if shown.wind_dir_random { " (wind_dir_random is on: each battle picks its own direction instead)" } else { "" }
            ));
        }
    }

    /// True while the left button is the tool's: the focus pick armed, or the sun being dragged.
    pub(crate) fn claims_left_button(&self) -> bool {
        self.focus_armed || self.sun_from.is_some() || self.wind_from.is_some()
    }

    /// Over the viewport: the sun's disc in the sky (grab it to move the sun), the focus pick's hint, the compare label.
    pub(crate) fn overlay(&mut self, e: &mut Editor, doc: DocumentId, shown: &EnvironmentParams, origin: gui::Vec2, size: gui::Vec2) {
        self.hovered = false;
        self.sun_screen = None;
        self.wind_tip = None;
        let dl = gui::window_draw_list();
        let density = gui::io().DisplayFramebufferScale.x.max(1e-3);
        if doc != 0 && self.handles() {
            self.draw_wind(e, doc, shown, origin, &dl);
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

        if doc != 0 && self.fog_rings {
            draw_fog_rings(e, shown, origin, &dl);
        }

        let top = gui::v2(origin.x + size.x * 0.5, origin.y + 40.0);
        let off = SWITCHES.iter().zip(self.without).filter(|(_, on)| *on).map(|((name, _), _)| *name).collect::<Vec<_>>();
        let banner = if self.comparing() {
            Some(("THE SAVED FILE (comparing: uncheck Compare, or let go of C, for the edits)".to_owned(), theme::warning()))
        } else if !off.is_empty() {
            Some((format!("preview without {} (the file keeps them)", off.join(", ")), theme::warning()))
        } else if self.focus_armed {
            Some(("Click what should be in focus (Esc: cancel)".to_owned(), theme::modified()))
        } else if self.wind_from.is_some() {
            Some((format!("wind: towards {:.0}\u{b0}", shown.wind_dir_degrees.rem_euclid(360.0)), theme::dim()))
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

        gui::same_line();
        gui::checkbox("Fog distances", &mut self.fog_rings);
        if gui::is_item_hovered_allow_disabled() {
            gui::set_tooltip(
                "Rings on the ground around the camera where the fog over it reaches 25, 50 and 75 %, as the game's fog shader works it out from the Fog section (the distance fog, the height fog over the ground, the clear distance's fade-in). They follow the camera, as the fog does.",
            );
        }

        gui::end_disabled();
        if doc != 0 {
            gui::same_line();
            gui::text_disabled(&format!("sun: drag its disc in the sky, or hold {}", shortcuts::label(SUN_DRAG).unwrap_or_default()));
        }

        if doc != 0 {
            gui::text_disabled("preview without:");
            for (i, (name, tip)) in SWITCHES.iter().enumerate() {
                gui::same_line();
                gui::checkbox(&format!("{name}##without"), &mut self.without[i]);
                if gui::is_item_hovered() {
                    gui::set_tooltip(&format!("{tip} Only the viewport's preview: the file keeps it."));
                }
            }
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

/// The fog's opacity steps the rings show.
pub(crate) const FOG_STEPS: [f32; 3] = [0.25, 0.5, 0.75];

/// The distances along the ground from the point below the camera where the fog's opacity first reaches each of `FOG_STEPS` (`None`: not
/// within 30 km), and the strongest it gets there.
pub(crate) fn fog_rings(p: &EnvironmentParams, h: f32, y: f32) -> ([Option<f32>; 3], f32) {
    let mut out = [None; 3];
    let mut strongest = 0.0f32;
    let mut r = 0.0f32;
    while r <= 30_000.0 {
        let a = wce_scene::environment::fog_opacity(p, r, h, y);
        strongest = strongest.max(a);
        for (slot, step) in out.iter_mut().zip(FOG_STEPS) {
            if slot.is_none() && a >= step {
                *slot = Some(r);
            }
        }

        // Finer near the camera, where the ring is read most closely.
        r += (r * 0.01).max(0.5);
    }

    (out, strongest)
}

/// The fog rings on the ground (the height of the camera's target) around the point below the camera, labelled where they cross the
/// camera's forward direction.
fn draw_fog_rings(e: &Editor, p: &EnvironmentParams, origin: gui::Vec2, dl: &gui::DrawList) {
    let density = gui::io().DisplayFramebufferScale.x.max(1e-3);
    let cam = e.viewport.camera();
    let ground = cam.target.y;
    let (rings, strongest) = fog_rings(p, (cam.eye.y - ground).max(0.0), ground);
    let colours = [gui::col32(150, 230, 255, 230), gui::col32(200, 210, 255, 230), gui::col32(245, 245, 245, 230)];
    let (fx, fz) = (cam.target.x - cam.eye.x, cam.target.z - cam.eye.z);
    let fl = fx.hypot(fz).max(1e-6);
    for (i, (radius, colour)) in rings.iter().zip(colours).enumerate() {
        let Some(r) = radius.filter(|r| *r > 0.5) else { continue };

        // The ring in runs of the points in front of the camera.
        let mut run: Vec<gui::Vec2> = vec![];
        for k in 0..=97 {
            let a = k as f32 / 96.0 * std::f32::consts::TAU;
            let w = Float3::new(cam.eye.x + a.cos() * r, ground, cam.eye.z + a.sin() * r);
            match e.viewport.project(w).filter(|_| k <= 96) {
                Some((x, y)) => run.push(gui::v2(origin.x + x / density, origin.y + y / density)),
                None => {
                    if run.len() > 1 {
                        dl.polyline(&run, colour, false, 1.5);
                    }

                    run.clear();
                }
            }
        }

        let w = Float3::new(cam.eye.x + fx / fl * r, ground, cam.eye.z + fz / fl * r);
        if let Some((x, y)) = e.viewport.project(w) {
            let text = format!("fog {:.0} %  {r:.0} m", FOG_STEPS[i] * 100.0);
            dl.text(gui::v2(origin.x + x / density + 4.0, origin.y + y / density - 16.0 - 14.0 * i as f32), colour, &text);
        }
    }

    if rings.iter().all(Option::is_none) {
        let text = format!("fog over the ground: at most {:.0} %", strongest * 100.0);
        dl.text(gui::v2(origin.x + 10.0, origin.y + 64.0), gui::col32(200, 210, 255, 230), &text);
    }
}
