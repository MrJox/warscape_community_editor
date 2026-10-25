//! The 3D viewport panel: renders the scene through the renderer, drives the camera controller, picks, draws the gizmo.
//!
//! The viewport's state lives in [`Viewport`]; drawing it needs the whole editor (the renderer, the scene, the selection, the active tool), so
//! the panel is a function over the editor, and a tool called from it still reaches `editor.viewport` (rays, projection).

use std::path::Path;

use dear_imguizmo_sys as gz;
use wce_editor_core::camera_controller::{CameraController, CameraInput, FlyCamera, OrbitCamera};
use wce_foundation::log;
use wce_foundation::math::{Float3, Float4x4, cross, dot, inverse, length, look_at_lh, normalize, perspective_lh};
use wce_foundation::{clock::Clock, wce_prof};
use wce_imgui::renderer::D3D11Renderer;
use wce_render::render_target::RenderTarget;
use wce_render::renderer::ViewParams;
use wce_scene::camera::Camera;
use wce_scene::environment::{EnvironmentParams, read_graphics_options};
use wce_scene::render_scene::{EntityId, Payload, RenderScene, SourceRef};

use crate::editor::Editor;
use crate::gui;
use crate::tool::{ViewportEvent, ViewportEventKind};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum GizmoMode {
    Off,
    #[default]
    Translate,
    Rotate,
    Scale,
}

impl GizmoMode {
    /// The toolbar's order: None, Move, Rotate, Scale.
    pub fn from_index(i: usize) -> Self {
        match i {
            1 => Self::Translate,
            2 => Self::Rotate,
            3 => Self::Scale,
            _ => Self::Off,
        }
    }
}

pub struct Viewport {
    orbit: OrbitCamera,
    fly: FlyCamera,
    fly_active: bool,
    target: RenderTarget,
    texture: Option<dear_imgui_rs::TextureId>,
    pub gizmo: GizmoMode,
    pub gizmo_local: bool,
    pub snap: bool,
    pub snap_move: f32,
    pub snap_angle: f32,
    pub snap_scale: f32,
    pub show_grid: bool,
    pub hovered: bool,
    pub frames_rendered: u64,

    /// The compass at the top of the viewport, and where north is in the world (x, z): +z unless a tool says otherwise (the map context
    /// gives the map's north in the open tile's frame).
    pub show_compass: bool,
    pub north: [f32; 2],

    /// The coordinates readout (crate::coords), the map a tool places the scene in (cells, height above the sea), the ground point under
    /// the cursor (while it is over the image).
    pub show_coords: bool,
    pub map_frame: Option<crate::coords::MapFrame>,
    pub cursor_ground: Option<Float3>,
    probe: crate::coords::GroundProbe,
    trace: String,
    trace_w: u32,
    trace_h: u32,
    trace_n: u32,
    gizmo_was_using: bool,
    wheel_owned: bool,
    gesture: u64,
    press: (f32, f32),
    pressed: bool,

    /// Where the right button went down over the image (a release there, without moving, is a context click).
    right_press: Option<(f32, f32)>,

    /// The active tool owns the current left-button gesture.
    tool_drag: bool,

    /// The current left-button gesture started off the gizmo and the tool: it turns the camera even when it crosses a gizmo handle.
    orbit_drag: bool,
    image_origin: gui::Vec2,
}

impl Default for Viewport {
    fn default() -> Self {
        Self {
            orbit: OrbitCamera::default(), fly: FlyCamera::default(), fly_active: false, target: RenderTarget::default(), texture: None, gizmo: GizmoMode::Translate,
            gizmo_local: false, snap: false, snap_move: 1.0, snap_angle: 15.0, snap_scale: 0.1, show_grid: false, hovered: false, frames_rendered: 0, show_compass: true, show_coords: true, map_frame: None, cursor_ground: None, probe: Default::default(),
            north: [0.0, 1.0], trace: String::new(),
            trace_w: 0, trace_h: 0, trace_n: 0, gizmo_was_using: false, wheel_owned: false, gesture: 0, press: (0.0, 0.0), pressed: false, right_press: None, tool_drag: false, orbit_drag: false, image_origin: gui::v2(0.0, 0.0),
        }
    }
}

// ImGuizmo wants row-vector matrices (v * M, row-major in memory); ours are column-vector (M * v), so it gets the transpose.
fn to_gizmo(m: &Float4x4) -> [f32; 16] {
    let mut out = [0.0; 16];
    for r in 0..4 {
        for c in 0..4 {
            out[r * 4 + c] = m.m[c][r];
        }
    }

    out
}

fn from_gizmo(v: &[f32; 16]) -> Float4x4 {
    let mut m = Float4x4::IDENTITY;
    for r in 0..4 {
        for c in 0..4 {
            m.m[c][r] = v[r * 4 + c];
        }
    }

    m
}

impl Viewport {
    pub fn controller(&mut self) -> &mut dyn CameraController {
        if self.fly_active { &mut self.fly } else { &mut self.orbit }
    }

    /// The orbit controller (the `cam` command sets it directly).
    pub fn orbit_mut(&mut self) -> &mut OrbitCamera {
        &mut self.orbit
    }

    fn controller_ref(&self) -> &dyn CameraController {
        if self.fly_active { &self.fly } else { &self.orbit }
    }

    pub fn use_orbit(&mut self) {
        if !self.fly_active {
            return;
        }

        self.orbit.set_camera(&self.fly.camera());
        self.fly_active = false;
    }

    pub fn use_fly(&mut self) {
        if self.fly_active {
            return;
        }

        self.fly.set_camera(&self.orbit.camera());
        self.fly_active = true;
    }

    pub fn is_fly(&self) -> bool {
        self.fly_active
    }

    pub fn camera(&self) -> Camera {
        self.controller_ref().camera()
    }

    pub fn target(&self) -> &RenderTarget {
        &self.target
    }

    pub fn width(&self) -> u32 {
        self.target.width()
    }

    pub fn height(&self) -> u32 {
        self.target.height()
    }

    /// Screen position of the viewport image's top-left corner (last frame).
    pub fn image_origin(&self) -> gui::Vec2 {
        self.image_origin
    }

    /// Run-length list of the sizes rendered so far ("694x824 x1, 680x824 x19"): a size change mid-run changes what exposure adaptation sees,
    /// so two runs that differ in it are not expected to match pixel for pixel.
    pub fn size_trace(&self) -> &str {
        &self.trace
    }

    /// The camera ray through a pixel of the last rendered image (world space, direction normalized). None before the first frame.
    /// The ground point under a pixel of the image (any drawable terrain of the scene).
    pub fn ground_under(&mut self, scene: &RenderScene, x: i32, y: i32) -> Option<Float3> {
        let far = self.camera().zfar;
        let (o, d) = self.ray(x, y)?;
        self.probe.hit(scene, o, d, far)
    }

    /// The coordinates readout's lines, with the cursor's last ground point.
    pub fn coords_lines(&self, scene: &RenderScene) -> Vec<String> {
        crate::coords::lines(&self.camera(), !self.is_fly(), self.cursor_ground, scene.game_units, self.map_frame.as_ref())
    }

    pub fn ray(&self, x: i32, y: i32) -> Option<(Float3, Float3)> {
        if !self.target.valid() {
            return None;
        }

        let cam = self.camera();
        let fwd = normalize(cam.target - cam.eye);
        let right = normalize(cross(Float3::new(0.0, 1.0, 0.0), fwd));
        let up = cross(fwd, right);
        let (w, h) = (self.target.width() as f32, self.target.height() as f32);
        let th = (cam.fov_y * 0.5).tan();
        let aspect = w / h;
        let nx = 2.0 * (x as f32 + 0.5) / w - 1.0;
        let ny = 1.0 - 2.0 * (y as f32 + 0.5) / h;
        Some((cam.eye, normalize(fwd + right * (nx * th * aspect) + up * (ny * th))))
    }

    /// Where a world point is in the last rendered image (pixels). None when it is behind the camera or there was no frame yet.
    pub fn project(&self, world: Float3) -> Option<(f32, f32)> {
        if !self.target.valid() {
            return None;
        }

        let cam = self.camera();
        let fwd = normalize(cam.target - cam.eye);
        let right = normalize(cross(Float3::new(0.0, 1.0, 0.0), fwd));
        let up = cross(fwd, right);
        let rel = world - cam.eye;
        let z = dot(rel, fwd);
        if z <= cam.znear {
            return None;
        }

        let (w, h) = (self.target.width() as f32, self.target.height() as f32);
        let th = (cam.fov_y * 0.5).tan();
        let aspect = w / h;
        Some(((dot(rel, right) / (z * th * aspect) * 0.5 + 0.5) * w - 0.5, (0.5 - dot(rel, up) / (z * th) * 0.5) * h - 0.5))
    }
}

/// Renders the scene into the viewport's surface.
fn render(e: &mut Editor, w: u32, h: u32) {
    wce_prof!("viewport.render");
    let outline_style = e.selection_outline_style();
    let Some(r) = e.renderer.as_mut() else { return };
    if e.scene.environment.active.is_none() {
        // Nothing lights the scene yet: the default environment with the player's output settings.
        let go = read_graphics_options();
        e.scene.environment.active = Some(EnvironmentParams { output_gamma: go.gamma, output_brightness: go.brightness, ..Default::default() });
    }

    let vp = &mut e.viewport;
    r.settings.show_grid = vp.show_grid;
    r.settings.selection = e.selection.entities(&e.scene);
    (r.settings.selection_colour, r.settings.selection_outline_px) = outline_style;
    e.clock.tick();
    r.settings.deterministic = e.clock.fixed_step();
    if !vp.target.resize(&e.dev, w, h) {
        log::error("cannot create the viewport surface");
        return;
    }

    let params = ViewParams { camera: vp.controller_ref().camera(), time_s: e.clock.time() as f32 };
    r.render(&e.scene, &params, &mut vp.target);
    vp.frames_rendered += 1;
    if w != vp.trace_w || h != vp.trace_h {
        if vp.trace_n != 0 {
            vp.trace += &format!(" x{},", vp.trace_n);
        }

        if vp.trace.len() < 600 {
            vp.trace += &format!(" {w}x{h}");
        }

        (vp.trace_w, vp.trace_h, vp.trace_n) = (w, h, 0);
    }

    vp.trace_n += 1;
}

/// What is under a pixel of the last rendered image.
pub fn pick(e: &mut Editor, x: i32, y: i32) -> EntityId {
    wce_prof!("viewport.pick");
    let Some(r) = e.renderer.as_mut() else { return 0 };
    if !e.viewport.target.valid() {
        return 0;
    }

    let params = ViewParams { camera: e.viewport.camera(), time_s: e.clock.time() as f32 };
    r.pick(&e.scene, &params, &e.viewport.target, x, y)
}

/// The surface under a pixel of the viewport image: the nearest mesh or terrain (overlays, locked layers and what `skip` says left out), its
/// world point and normal (`Renderer::pick_surface`). For placing things on other things.
pub fn surface_under(e: &mut Editor, x: i32, y: i32, skip: &dyn Fn(&wce_scene::render_scene::Entity) -> bool) -> Option<wce_render::renderer::SurfaceHit> {
    wce_prof!("viewport.surface_under");
    let r = e.renderer.as_mut()?;
    if !e.viewport.target.valid() {
        return None;
    }

    let params = ViewParams { camera: e.viewport.camera(), time_s: e.clock.time() as f32 };
    let scene = &e.scene;
    r.pick_surface(scene, &params, &e.viewport.target, x, y, &|id| scene.find(id).is_some_and(skip))
}

/// Offscreen: `frames` fixed-step frames of the scene at w x h, the last one written as PNG. No window is involved.
pub fn capture(e: &mut Editor, w: u32, h: u32, frames: u32, path: &Path) -> bool {
    let outline_style = e.selection_outline_style();
    let Some(r) = e.renderer.as_mut() else { return false };
    if w < 16 || h < 16 {
        return false;
    }

    if e.scene.environment.active.is_none() {
        e.scene.environment.active = Some(EnvironmentParams::default());
    }

    let mut out = RenderTarget::default();
    if !out.create(&e.dev, w, h) {
        return false;
    }

    let mut clock = Clock::fixed(1.0 / 60.0);
    r.settings.deterministic = true;

    // A capture is a function of the scene, not of the live frames before it.
    r.restart_temporal();
    r.settings.show_grid = e.viewport.show_grid;
    r.settings.selection = e.selection.entities(&e.scene);
    (r.settings.selection_colour, r.settings.selection_outline_px) = outline_style;
    for _ in 0..frames {
        clock.tick();
        let params = ViewParams { camera: e.viewport.camera(), time_s: clock.time() as f32 };
        r.render(&e.scene, &params, &mut out);
    }

    out.save_png(&e.dev, &e.ctx, path)
}

/// Frames the selected items (the bounds of their models and overlays).
pub fn focus_selection(e: &mut Editor) {
    let Some(r) = e.renderer.as_mut() else { return };
    if e.selection.is_empty() {
        return;
    }

    let mut lo = Float3::new(1e30, 1e30, 1e30);
    let mut hi = Float3::new(-1e30, -1e30, -1e30);
    let mut grow = |p: Float3| {
        lo = Float3::new(lo.x.min(p.x), lo.y.min(p.y), lo.z.min(p.z));
        hi = Float3::new(hi.x.max(p.x), hi.y.max(p.y), hi.z.max(p.z));
    };

    for id in e.selection.entities(&e.scene) {
        let Some(ent) = e.scene.find(id) else { continue };
        match &ent.payload {
            Payload::MeshInstance(mi) => {
                if let Some(m) = r.models().model(&mi.model) {
                    for k in 0..8 {
                        let corner = Float3::new(
                            if k & 1 != 0 { m.bmax.x } else { m.bmin.x },
                            if k & 2 != 0 { m.bmax.y } else { m.bmin.y },
                            if k & 4 != 0 { m.bmax.z } else { m.bmin.z },
                        );
                        grow(ent.world.transform_point(corner));
                    }
                }
            }

            Payload::Overlay(o) => {
                for v in o.lines.iter().chain(o.triangles.iter()) {
                    grow(ent.world.transform_point(v.p));
                }
            }

            _ => {}
        }
    }

    if lo.x > hi.x {
        return;
    }

    e.viewport.controller().focus((lo + hi) * 0.5, (length(hi - lo) * 0.5).max(2.0));
}

/// The camera's heading in degrees from north (clockwise seen from above: east is 90), `None` looking straight down or up.
pub fn heading(cam: &Camera, north: [f32; 2]) -> Option<f32> {
    let (fx, fz) = (cam.target.x - cam.eye.x, cam.target.z - cam.eye.z);
    let nl = (north[0] * north[0] + north[1] * north[1]).sqrt();
    if fx * fx + fz * fz < 1e-12 || nl < 1e-12 {
        return None;
    }

    // Left-handed, y up: east is north turned a quarter clockwise seen from above.
    let (nx, nz) = (north[0] / nl, north[1] / nl);
    let (ex, ez) = (nz, -nx);
    Some((fx * ex + fz * ez).atan2(fx * nx + fz * nz).to_degrees().rem_euclid(360.0))
}

/// A compass strip at the top of the viewport image: the directions around the camera's heading, which is marked in the middle.
fn draw_compass(origin: gui::Vec2, size: gui::Vec2, cam: &Camera, north: [f32; 2]) {
    const SPAN: f32 = 75.0;
    let Some(h) = heading(cam, north) else { return };
    let width = (size.x - 16.0).min(360.0);
    if width < 120.0 || size.y < 60.0 {
        return;
    }

    let dl = gui::window_draw_list();
    let (cx, top, bar) = (origin.x + size.x * 0.5, origin.y + 6.0, 24.0);
    let (x0, x1) = (cx - width * 0.5, cx + width * 0.5);
    let per_degree = width * 0.5 / SPAN;
    dl.push_clip_rect(gui::v2(x0, top), gui::v2(x1, top + bar + 18.0));
    dl.rect_filled(gui::v2(x0, top), gui::v2(x1, top + bar), gui::col32(0, 0, 0, 140), 4.0);
    let white = gui::col32(235, 235, 235, 255);
    let first = ((h - SPAN) / 5.0).floor() as i32 * 5;
    for d in (first..=(h + SPAN) as i32 + 5).step_by(5) {
        let x = cx + (d as f32 - h) * per_degree;
        let deg = d.rem_euclid(360);
        let (len, label) = match deg {
            0 => (10.0, "N"),
            45 => (8.0, "NE"),
            90 => (10.0, "E"),
            135 => (8.0, "SE"),
            180 => (10.0, "S"),
            225 => (8.0, "SW"),
            270 => (10.0, "W"),
            315 => (8.0, "NW"),
            _ if deg % 15 == 0 => (6.0, ""),
            _ => (3.0, ""),
        };

        dl.line(gui::v2(x, top + bar - len), gui::v2(x, top + bar), white, 1.0);
        if !label.is_empty() {
            let colour = if deg == 0 { gui::col32(255, 90, 70, 255) } else { white };
            dl.text(gui::v2(x - gui::calc_text_width(label) * 0.5, top + 1.0), colour, label);
        }
    }

    // The heading: a mark in the middle and its value under the strip.
    let mark = gui::col32(255, 210, 60, 255);
    dl.triangle_filled(gui::v2(cx - 5.0, top + bar + 6.0), gui::v2(cx + 5.0, top + bar + 6.0), gui::v2(cx, top + bar - 1.0), mark);
    let value = format!("{:.0}", h.round().rem_euclid(360.0));
    dl.text(gui::v2(cx + 8.0, top + bar + 1.0), mark, &value);
    dl.pop_clip_rect();
}

/// The "Viewport" window.
pub fn draw(e: &mut Editor, textures: &mut D3D11Renderer) {
    e.viewport.hovered = false;
    if e.viewport.wheel_owned {
        gui::own_mouse_wheel(None);
        e.viewport.wheel_owned = false;
    }

    gui::push_style_var_v(gui::VAR_WINDOW_PADDING, gui::v2(0.0, 0.0));
    let open = gui::begin("Viewport", Some(&mut e.panels.viewport), gui::WINDOW_NO_SCROLLBAR | gui::WINDOW_NO_SCROLL_WITH_MOUSE);
    gui::pop_style_var(1);
    if !open {
        gui::end();
        return;
    }

    // SAFETY: inside a Dear ImGui frame.
    unsafe { gz::ImGuizmo_BeginFrame() };
    let size = gui::content_region_avail();
    let (w, h) = (size.x as i32, size.y as i32);

    // The image is rendered at the window's pixel density (a scaled HiDPI output) and shown at its logical size; positions given to the
    // tools and to picking are pixels of the image.
    let density = gui::io().DisplayFramebufferScale.x.max(1e-3);
    let (pw, ph) = ((size.x * density) as u32, (size.y * density) as u32);
    if e.renderer.is_none() {
        gui::text_disabled("The renderer is not available (the game packs are not mounted).");
        gui::end();
        return;
    }

    if w < 16 || h < 16 {
        gui::end();
        return;
    }

    render(e, pw, ph);
    let Some(srv) = e.viewport.target.srv().cloned() else {
        gui::end();
        return;
    };

    let tex = match e.viewport.texture {
        Some(id) => {
            textures.replace(id, &srv);
            id
        }

        None => *e.viewport.texture.insert(textures.register(&srv)),
    };

    let origin = gui::cursor_screen_pos();
    e.viewport.image_origin = origin;
    gui::image(tex.id(), size);
    if e.viewport.show_compass {
        draw_compass(origin, size, &e.viewport.camera(), e.viewport.north);
    }

    gui::set_cursor_screen_pos(origin);
    gui::invisible_button("##viewport", size, gui::BUTTON_LEFT | gui::BUTTON_RIGHT | gui::BUTTON_MIDDLE);
    let hovered = gui::is_item_hovered();
    e.viewport.hovered = hovered;

    // The wheel over the image is the camera's: no window behind it scrolls (Shift+wheel scrolled the host window sideways).
    if hovered {
        gui::own_mouse_wheel(Some(gui::get_id("viewport wheel")));
        e.viewport.wheel_owned = true;
    }
    let active = gui::is_item_active();
    let button_id = gui::item_id();
    let io = gui::io();
    let (mouse_x, mouse_y) = (io.MousePos.x, io.MousePos.y);

    // The coordinates: the ground under the cursor, then the readout.
    if e.viewport.show_coords {
        if hovered {
            let (px, py) = (((mouse_x - origin.x) * density) as i32, ((mouse_y - origin.y) * density) as i32);
            e.viewport.cursor_ground = e.viewport.ground_under(&e.scene, px, py);
        }

        crate::coords::draw(origin, size, &e.viewport.coords_lines(&e.scene));
    }

    // The tool's widgets over the image (the tile's weather chips).
    e.with_active_tool(|t, e| t.viewport_overlay(e, origin, size));

    let (ctrl, shift, alt) = (io.KeyCtrl, io.KeyShift, io.KeyAlt);
    let fly = e.viewport.fly_active;

    // ---- gizmo (not while the tool owns the left button: a grab or a brush must not hand the click to the gizmo under the cursor)
    let claims = e.viewport_and_tool_claims();
    let mut gizmo_target: Option<(SourceRef, Float4x4)> = None;
    if e.viewport.gizmo != GizmoMode::Off && !fly && !claims && !e.selection.is_empty() {
        let primary = e.selection.items()[0];
        gizmo_target = e.docs.find(primary.doc).and_then(|d| d.item_transform(primary)).map(|world| (primary, world));
    }

    let mut using_gizmo = false;
    let mut over_gizmo_handle = false;
    if let Some((primary, world)) = gizmo_target {
        let vp = &e.viewport;
        let cam = vp.camera();
        let v = to_gizmo(&look_at_lh(cam.eye, cam.target, Float3::new(0.0, 1.0, 0.0)));
        let p = to_gizmo(&perspective_lh(cam.fov_y, w as f32 / h as f32, cam.znear, cam.zfar));
        let mut m = to_gizmo(&world);
        let op = match vp.gizmo {
            GizmoMode::Translate => gz::TRANSLATE,
            GizmoMode::Rotate => gz::ROTATE,
            _ => gz::SCALE,
        };

        let mode = if vp.gizmo_local && vp.gizmo != GizmoMode::Translate { gz::LOCAL } else { gz::WORLD };
        let mut snap_values = [vp.snap_move; 3];
        if vp.gizmo == GizmoMode::Rotate {
            snap_values[0] = vp.snap_angle;
        }

        if vp.gizmo == GizmoMode::Scale {
            snap_values[0] = vp.snap_scale;
        }

        // The C++ editor's ImGuizmo has SetAllowedItem: the gizmo may start a drag while the viewport's button is the hovered item (ImGuizmo
        // refuses otherwise). Upstream ImGuizmo does not, so the button stops being "an item" while Manipulate runs.
        let ids = gui::context_ids();
        let clear = |id: gui::ImGuiID| if id == button_id { 0 } else { id };
        gui::set_context_ids((clear(ids.0), clear(ids.1), clear(ids.2)));

        // SAFETY: inside a Dear ImGui frame, in the viewport window; the arrays outlive the calls.
        unsafe {
            gz::ImGuizmo_SetOrthographic(false);

            // Larger than ImGuizmo's default 0.1: the handles of a building in a 100 m view were 35 px.
            gz::ImGuizmo_SetGizmoSizeClipSpace(0.16);
            gz::ImGuizmo_SetDrawlist(dear_imgui_sys::igGetWindowDrawList() as *mut _);
            gz::ImGuizmo_SetRect(origin.x, origin.y, size.x, size.y);
            gz::ImGuizmo_Manipulate(
                v.as_ptr(), p.as_ptr(), op, mode, m.as_mut_ptr(), std::ptr::null_mut(),
                if vp.snap { snap_values.as_ptr() } else { std::ptr::null() }, std::ptr::null(), std::ptr::null(),
            );
            using_gizmo = gz::ImGuizmo_IsUsing();
            over_gizmo_handle = gz::ImGuizmo_IsOver_Nil();
        }

        // ImGuizmo does not change these ids itself: back to what Dear ImGui had.
        gui::set_context_ids(ids);
        if using_gizmo {
            if !e.viewport.gizmo_was_using {
                e.viewport.gesture += 1;
            }

            let delta = from_gizmo(&m) * inverse(&world);
            let items = e.selection.items().to_vec();
            let gesture = e.viewport.gesture;
            if let Some(d) = e.docs.find_mut(primary.doc) {
                d.transform_items(&items, &delta, gesture);
            }
        }
    }

    if !using_gizmo && e.viewport.gizmo_was_using {
        let doc = e.selection.items().first().map_or(0, |s| s.doc);
        if let Some(d) = e.docs.find_mut(doc) {
            d.history_mut().break_merge();
        }
    }

    e.viewport.gizmo_was_using = using_gizmo;
    let over_gizmo = using_gizmo || (gizmo_target.is_some() && over_gizmo_handle);

    // ---- the active tool's mouse events (hover every frame, the left button's press / drag / release)
    let event = |kind| ViewportEvent {
        kind, x: ((mouse_x - origin.x) * density) as i32, y: ((mouse_y - origin.y) * density) as i32, ctrl, shift, alt, ..Default::default()
    };
    let has_tool = e.has_active_tool();
    if has_tool && hovered && !over_gizmo {
        e.tool_viewport_input(&event(ViewportEventKind::Hover));
        if gui::is_mouse_clicked(0) && !fly {
            let taken = e.tool_viewport_input(&event(ViewportEventKind::Press));
            if taken || claims {
                e.viewport.tool_drag = true;
            }
        }
    }

    if has_tool && e.viewport.tool_drag {
        if gui::is_mouse_down(0) {
            e.tool_viewport_input(&event(ViewportEventKind::Drag));
        } else {
            e.tool_viewport_input(&event(ViewportEventKind::Release));
            e.viewport.tool_drag = false;
            e.viewport.pressed = false;
        }
    }

    // ---- camera
    let io = gui::io();
    let mut input = CameraInput { dt: io.DeltaTime, ..Default::default() };
    let tool_drag = e.viewport.tool_drag;
    // A left drag that starts off the gizmo and the tool turns the orbit camera, gizmo shown or not; one that starts on a handle is the gizmo's.
    if hovered && gui::is_mouse_clicked(0) {
        e.viewport.orbit_drag = !over_gizmo && !tool_drag && !claims;
    }

    if !gui::is_mouse_down(0) {
        e.viewport.orbit_drag = false;
    }

    if hovered || active {
        input.dx = io.MouseDelta.x;
        input.dy = io.MouseDelta.y;
        input.wheel = if hovered { io.MouseWheel } else { 0.0 };
        let left = e.viewport.orbit_drag;
        if !over_gizmo || left {
            input.rotate = if fly { gui::is_mouse_down(1) && active } else { (left || gui::is_mouse_down(1)) && active };
            input.pan = gui::is_mouse_down(2) && active;
        }

        // WASD (Q / E down / up, Shift faster) while the right button is held, in both controllers; without it W / E / R / Q pick the gizmo.
        if gui::is_mouse_down(1) && active && !io.WantTextInput {
            input.forward = gui::is_key_down(gui::KEY_W);
            input.back = gui::is_key_down(gui::KEY_S);
            input.left = gui::is_key_down(gui::KEY_A);
            input.right = gui::is_key_down(gui::KEY_D);
            input.up = gui::is_key_down(gui::KEY_E);
            input.down = gui::is_key_down(gui::KEY_Q);
            input.fast = shift;

            // Flying is not a click.
            if input.forward || input.back || input.left || input.right || input.up || input.down {
                e.viewport.right_press = None;
            }
        }
    }

    e.viewport.controller().update(&input);

    // ---- a right click (a short press and release without moving or flying; a right drag turns the camera, WASD flies): the tool's context
    // menu
    if hovered && gui::is_mouse_clicked(1) {
        e.viewport.right_press = Some((mouse_x, mouse_y));
    }

    if gui::is_mouse_released(1)
        && let Some((px, py)) = e.viewport.right_press.take()
        && has_tool
        && hovered
        && (mouse_x - px).hypot(mouse_y - py) < 4.0
        && io.MouseDownDurationPrev[1] < 0.5
    {
        let mut ev = event(ViewportEventKind::ContextMenu);
        ev.hit = pick(e, ev.x, ev.y);
        if let Some(ent) = e.scene.find(ev.hit) {
            ev.hit_source = ent.source;
        }

        e.tool_viewport_input(&ev);
    }

    // ---- click: a press and release without moving selects what is under the cursor
    if hovered && gui::is_mouse_clicked(0) && !over_gizmo && !tool_drag && !claims {
        e.viewport.press = (mouse_x, mouse_y);
        e.viewport.pressed = true;
    }

    if e.viewport.pressed && gui::is_mouse_released(0) {
        e.viewport.pressed = false;
        let moved = (mouse_x - e.viewport.press.0).hypot(mouse_y - e.viewport.press.1);
        if hovered && moved < 4.0 && !over_gizmo {
            let mut ev = event(ViewportEventKind::Click);
            ev.hit = pick(e, ev.x, ev.y);
            if let Some(ent) = e.scene.find(ev.hit) {
                ev.hit_source = ent.source;
            }

            if !(has_tool && e.tool_viewport_input(&ev)) {
                if ev.hit_source.doc == 0 {
                    if !ev.ctrl {
                        e.selection.clear();
                    }
                } else if ev.ctrl {
                    e.selection.toggle(ev.hit_source);
                } else if ev.shift {
                    e.selection.add(ev.hit_source);
                } else {
                    e.selection.set(ev.hit_source);
                }

                if ev.hit_source.doc != 0 {
                    e.docs.set_active(ev.hit_source.doc);
                }
            }
        }
    }

    gui::end();
}
