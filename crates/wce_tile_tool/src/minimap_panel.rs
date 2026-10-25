//! The Minimap panel (docs/plans/editor_todo.md): the open tile from above, north (+z) up and +x right, as a base picture (the ground's
//! colour, its heights, or nothing) under layers that are toggled on and off: the siege AI (what `wce_siege::svg` draws), the deployment
//! zones and the camera. The left button moves the camera to where it is pressed (held: it follows), the wheel zooms around the cursor, the
//! middle button pans.

use std::sync::mpsc;

use wce_editor::editor::Editor;
use wce_editor::gui::{self, Vec2, col32, v2};
use wce_foundation::log;
use wce_scene::render_scene::DocumentId;
use wce_siege::{EdgeKind, NodeKind, P, ZoneKind};
use wce_tile_editor::terrain_bake::{BakedColour, TerrainView};
use wce_tile_editor::tile_document::TileDocument;
use wce_tile_editor::tile_document::minimap::MinimapSiege;

use crate::tool::document_id;

/// The side of the pictures made for the minimap, at most.
const HEIGHT_DIM: u32 = 512;
const COLOUR_DIM: u32 = 1024;

/// Seconds between two pictures of the heights while they change (a brush stroke), and two reads of the siege AI while the tile is edited
/// (it waits for the buttons to be released too).
const HEIGHT_EVERY: f64 = 0.3;
const SIEGE_EVERY: f64 = 0.5;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum Base {
    #[default]
    Colour,
    Height,
    None,
}

/// A picture of the minimap: its texture's key in `Editor::images`, and the ground it covers (x0, z0, w, h metres; rows along +z).
#[derive(Default)]
struct Picture {
    key: String,
    area: [f64; 4],
}

impl Picture {
    fn set(&mut self, e: &mut Editor, serial: &mut u64, w: u32, h: u32, rgba: Vec<u8>, area: [f64; 4]) {
        self.clear(e);
        *serial += 1;
        self.key = format!("tile_minimap/{serial}");
        self.area = area;
        e.images.submit(&self.key, w, h, rgba);
    }

    fn clear(&mut self, e: &mut Editor) {
        if !self.key.is_empty() {
            e.images.forget(&self.key);
            self.key.clear();
        }
    }
}

/// What the colour picture shows: the game-look colour of a climate and season (baked here), or the Ground section's debug view (its
/// picture, by address).
#[derive(Clone, Debug, PartialEq, Eq)]
enum ColourKey {
    Baked(DocumentId, String, String),
    Debug(DocumentId, TerrainView, usize),
}

pub struct MinimapUi {
    base: Base,
    show_siege: bool,
    show_zones: bool,
    show_camera: bool,

    /// 1 shows the whole area; the world point in the middle (`None`: the area's centre).
    zoom: f64,
    centre: Option<[f64; 2]>,

    doc: DocumentId,
    serial: u64,
    picture: Picture,
    picture_of: Option<Base>,
    note: String,

    colour_for: Option<ColourKey>,
    colour_job: Option<(ColourKey, mpsc::Receiver<Result<BakedColour, String>>)>,

    /// The last game-look colour baked, and whether its picture has the sea over it (the Ground section's "sea at 0 m").
    colour_baked: Option<(ColourKey, BakedColour)>,
    colour_sea: bool,

    /// The terrain version the heights picture is of, and when it was made.
    height_for: u64,
    height_time: f64,
    height_range: (f64, f64),

    siege: Option<MinimapSiege>,
    siege_for: u64,
    siege_time: f64,

    /// The middle button is panning from this cursor position.
    pan_from: Option<Vec2>,
    wheel_owned: bool,
}

impl Default for MinimapUi {
    fn default() -> Self {
        Self {
            base: Base::Colour, show_siege: true, show_zones: true, show_camera: true, zoom: 1.0, centre: None, doc: 0, serial: 0, picture: Picture::default(),
            picture_of: None, note: String::new(), colour_for: None, colour_job: None, colour_baked: None, colour_sea: false, height_for: 0, height_time: -1e9, height_range: (0.0, 0.0), siege: None,
            siege_for: 0, siege_time: -1e9, pan_from: None, wheel_owned: false,
        }
    }
}

/// From the minimap's world (metres, x right, z up) to the screen and back.
#[derive(Clone, Copy)]
struct Frame {
    mid: Vec2,
    centre: [f64; 2],
    scale: f64,
}

impl Frame {
    fn screen(&self, x: f64, z: f64) -> Vec2 {
        v2((f64::from(self.mid.x) + (x - self.centre[0]) * self.scale) as f32, (f64::from(self.mid.y) - (z - self.centre[1]) * self.scale) as f32)
    }

    fn world(&self, p: Vec2) -> [f64; 2] {
        [self.centre[0] + f64::from(p.x - self.mid.x) / self.scale, self.centre[1] - f64::from(p.y - self.mid.y) / self.scale]
    }
}

impl MinimapUi {
    /// The "Minimap" window; the first time, docked in `dock` (the Tile panel's dock: a layout saved before the panel existed has no
    /// place for it).
    pub fn draw_panel(&mut self, e: &mut Editor, dock: Option<gui::ImGuiID>) {
        gui::set_next_window_size(gui::v2(380.0, 420.0), gui::COND_FIRST_USE_EVER);
        if let Some(dock) = dock.filter(|&d| d != 0) {
            gui::set_next_window_dock_id(dock, gui::COND_FIRST_USE_EVER);
        }

        let open = gui::begin("Minimap", None, 0);
        let hovered = open && self.draw(e);
        gui::end();

        // The wheel over the map is the zoom's; given back when the cursor leaves it.
        if hovered {
            gui::own_mouse_wheel(Some(gui::get_id("minimap wheel")));
            self.wheel_owned = true;
        } else if self.wheel_owned {
            gui::own_mouse_wheel(None);
            self.wheel_owned = false;
        }
    }

    /// True when the cursor is over the map.
    fn draw(&mut self, e: &mut Editor) -> bool {
        let Some(id) = document_id(e) else {
            if self.doc != 0 {
                self.forget(e);
            }

            gui::text_disabled("Open a tile first.");
            return false;
        };

        if id != self.doc {
            self.forget(e);
            self.doc = id;
        }

        self.draw_bar();
        self.refresh(e, id);
        if !self.note.is_empty() {
            gui::text_disabled(&self.note);
        }

        let avail = gui::content_region_avail();
        if avail.x < 32.0 || avail.y < 32.0 {
            return false;
        }

        let origin = gui::cursor_screen_pos();
        gui::invisible_button("##minimap", avail, gui::BUTTON_LEFT | gui::BUTTON_MIDDLE);
        let hovered = gui::is_item_hovered();
        let active = gui::is_item_active();
        let (lo, hi) = (origin, v2(origin.x + avail.x, origin.y + avail.y));

        let d = e.docs.find_as::<TileDocument>(id).expect("a tile");
        let area = d.tile().minimap_area();
        let fit = (f64::from(avail.x) / area[2]).min(f64::from(avail.y) / area[3]);
        let mut frame = Frame {
            mid: v2((lo.x + hi.x) * 0.5, (lo.y + hi.y) * 0.5), centre: self.centre.unwrap_or([area[0] + area[2] * 0.5, area[1] + area[3] * 0.5]), scale: fit * self.zoom,
        };

        // Zoom around the cursor, pan with the middle button.
        let mouse = gui::mouse_pos();
        if hovered {
            let wheel = f64::from(gui::io().MouseWheel);
            if wheel != 0.0 {
                let at = frame.world(mouse);
                self.zoom = (self.zoom * 1.25f64.powf(wheel)).clamp(1.0, 64.0);
                frame.scale = fit * self.zoom;
                let moved = frame.world(mouse);
                frame.centre = [frame.centre[0] + at[0] - moved[0], frame.centre[1] + at[1] - moved[1]];
                self.centre = Some(frame.centre);
            }
        }

        if active && gui::is_mouse_down(2) {
            if let Some(from) = self.pan_from {
                let (dx, dy) = (f64::from(mouse.x - from.x), f64::from(mouse.y - from.y));
                frame.centre = [frame.centre[0] - dx / frame.scale, frame.centre[1] + dy / frame.scale];
                self.centre = Some(frame.centre);
            }

            self.pan_from = Some(mouse);
        } else {
            self.pan_from = None;
        }

        let dl = gui::window_draw_list();
        dl.push_clip_rect(lo, hi);
        dl.rect_filled(lo, hi, col32(24, 25, 28, 255), 0.0);
        if self.picture_of == Some(self.base)
            && let Some(img) = e.images.get(&self.picture.key)
        {
            let [x0, z0, w, h] = self.picture.area;
            dl.image(img.id, frame.screen(x0, z0 + h), frame.screen(x0 + w, z0), v2(0.0, 1.0), v2(1.0, 0.0), col32(255, 255, 255, 255));
        }

        // The tile's edge (the area may hold the height map's border around it).
        let d = e.docs.find_as::<TileDocument>(id).expect("a tile");
        let (ex, ez) = (d.tile().extent_x(), d.tile().extent_z());
        dl.rect(frame.screen(0.0, ez), frame.screen(ex, 0.0), col32(255, 255, 255, 120), 0.0, 1.0);

        if let Some(s) = &self.siege {
            if self.show_zones {
                draw_zones(&dl, &frame, s);
            }

            if self.show_siege {
                draw_siege(&dl, &frame, s);
            }
        }

        if self.show_camera {
            draw_camera(&dl, &frame, e);
        }

        draw_north(&dl, hi, lo, e.viewport.north);
        dl.pop_clip_rect();

        // The left button: the camera goes there.
        if active && gui::is_mouse_down(0) {
            let at = frame.world(mouse);
            move_camera(e, id, at);
        }

        if hovered {
            let at = frame.world(mouse);
            let d = e.docs.find_as::<TileDocument>(id).expect("a tile");
            let h = d.project().height_at(at[0], at[1]);
            gui::set_tooltip(&format!("{:.0}, {:.0} m, ground {h:.1} m\nleft: camera here, wheel: zoom, middle: pan", at[0], at[1]));
        }

        hovered
    }

    fn draw_bar(&mut self) {
        let bases = [Base::Colour, Base::Height, Base::None];
        let mut bi = bases.iter().position(|&b| b == self.base).unwrap_or(0) as i32;
        gui::set_next_item_width(90.0);
        if gui::combo("##minimap_base", &mut bi, &["colour", "height", "none"]) {
            self.base = bases[bi as usize];
        }

        gui::same_line();
        gui::checkbox("siege AI", &mut self.show_siege);
        gui::same_line();
        gui::checkbox("deployment", &mut self.show_zones);
        gui::same_line();
        gui::checkbox("camera", &mut self.show_camera);
        gui::same_line();
        if gui::small_button("fit") {
            self.zoom = 1.0;
            self.centre = None;
        }
    }

    /// Makes what is out of date: the base picture, the siege AI.
    fn refresh(&mut self, e: &mut Editor, id: DocumentId) {
        let now = gui::time();
        self.note.clear();
        match self.base {
            Base::None => {}
            Base::Height => self.refresh_heights(e, id, now),
            Base::Colour => self.refresh_colour(e, id),
        }

        if self.show_siege || self.show_zones {
            let d = e.docs.find_as::<TileDocument>(id).expect("a tile");
            let version = d.edit_version();
            // Not while a button is held (a drag, a stroke): reading it takes tens of milliseconds on a big tile.
            let idle = !(0..3).any(gui::is_mouse_down);
            if self.siege.is_none() || (version != self.siege_for && idle && now - self.siege_time >= SIEGE_EVERY) {
                wce_foundation::wce_prof!("tile.minimap.siege");
                self.siege = Some(d.tile().minimap_siege());
                self.siege_for = version;
                self.siege_time = now;
            }
        }
    }

    fn refresh_heights(&mut self, e: &mut Editor, id: DocumentId, now: f64) {
        let d = e.docs.find_as::<TileDocument>(id).expect("a tile");
        let version = d.tile().terrain_version();
        let stale = self.picture_of != Some(Base::Height) || version != self.height_for;
        if stale && (self.picture_of != Some(Base::Height) || now - self.height_time >= HEIGHT_EVERY) {
            wce_foundation::wce_prof!("tile.minimap.heights");
            match d.tile().minimap_heights(HEIGHT_DIM) {
                Some(img) => {
                    self.height_range = img.range;
                    self.picture.set(e, &mut self.serial, img.w, img.h, img.rgba, [img.x0, img.z0, img.w_m, img.h_m]);
                }

                None => self.picture.clear(e),
            }

            self.picture_of = Some(Base::Height);
            self.colour_for = None;
            self.height_for = version;
            self.height_time = now;
        }

        if self.picture.key.is_empty() {
            self.note = "no height map".to_owned();
        } else {
            self.note = format!("ground from {:.1} to {:.1} m", self.height_range.0, self.height_range.1);
        }
    }

    /// The colour: the Ground section's debug view when one is shown (its picture), else the game-look colour baked on a thread.
    fn refresh_colour(&mut self, e: &mut Editor, id: DocumentId) {
        // A bake that finished.
        if let Some((key, rx)) = &self.colour_job {
            match rx.try_recv() {
                Ok(Ok(b)) => {
                    let key = key.clone();
                    self.colour_job = None;
                    self.colour_baked = Some((key.clone(), b));
                    self.show_baked(e, id);
                    self.colour_for = Some(key);
                }

                Ok(Err(why)) => {
                    let key = key.clone();
                    self.colour_job = None;
                    log::warn(format!("minimap: no ground colour: {why}"));
                    self.picture.clear(e);
                    self.picture_of = Some(Base::Colour);
                    self.colour_for = Some(key);
                }

                Err(mpsc::TryRecvError::Empty) => {}
                Err(mpsc::TryRecvError::Disconnected) => self.colour_job = None,
            }
        }

        let d = e.docs.find_as::<TileDocument>(id).expect("a tile");
        let look = d.tile().terrain_look().clone();
        let key = if look.view == TerrainView::Colour {
            ColourKey::Baked(id, look.climate.clone(), look.season.clone())
        } else {
            ColourKey::Debug(id, look.view, d.tile().baked_colour().map_or(0, |b| std::ptr::from_ref(b) as usize))
        };

        let current = self.picture_of == Some(Base::Colour) && self.colour_for.as_ref() == Some(&key);

        // The sea switched on or off: the same bake, the sea over it or not.
        if current && matches!(key, ColourKey::Baked(..)) && self.colour_baked.is_some() && d.tile().display().show_sea != self.colour_sea {
            self.show_baked(e, id);
        }

        let d = e.docs.find_as::<TileDocument>(id).expect("a tile");
        let baking = self.colour_job.as_ref().is_some_and(|(k, _)| *k == key);
        if !current && !baking {
            match &key {
                ColourKey::Debug(..) => {
                    match d.tile().baked_colour() {
                        Some(b) => {
                            let (w, h, rgba, area) = (b.w, b.h, b.rgba.clone(), [b.x0, b.z0, b.w_m, b.h_m]);
                            self.picture.set(e, &mut self.serial, w, h, rgba, area);
                        }

                        None => self.picture.clear(e),
                    }

                    self.picture_of = Some(Base::Colour);
                    self.colour_for = Some(key.clone());
                }

                // Back from a debug view: the last bake is still good.
                ColourKey::Baked(..) if self.colour_baked.as_ref().is_some_and(|(k, _)| *k == key) => {
                    self.show_baked(e, id);
                    self.colour_for = Some(key.clone());
                }

                ColourKey::Baked(..) => match d.tile().minimap_colour_bake(COLOUR_DIM) {
                    Ok(bake) => {
                        let (tx, rx) = mpsc::channel();
                        std::thread::spawn(move || {
                            let _ = tx.send(bake.run());
                        });
                        self.colour_job = Some((key.clone(), rx));
                    }

                    Err(why) => {
                        log::info(format!("minimap: no ground colour: {why}"));
                        self.picture.clear(e);
                        self.picture_of = Some(Base::Colour);
                        self.colour_for = Some(key.clone());
                    }
                },
            }
        }

        if self.colour_job.is_some() {
            self.note = "baking the ground's colour...".to_owned();
        } else if self.picture.key.is_empty() || self.picture_of != Some(Base::Colour) {
            self.note = "no ground colour (see the log); try the height base".to_owned();
        } else if let ColourKey::Baked(_, climate, season) = &key {
            self.note = format!("{climate}, {season} (the blend's CPU stand-in)");
        }
    }

    /// The `minimap` command: its base, its layers, its zoom; the camera moved as by a click.
    ///
    /// # Errors
    ///
    /// Wrong arguments, or no tile for `goto`.
    pub fn command(&mut self, e: &mut Editor, a: &[String]) -> Result<String, String> {
        let on = |w: Option<&String>| match w.map(String::as_str) {
            Some("on") => Ok(true),
            Some("off") => Ok(false),
            _ => Err("on or off".to_owned()),
        };

        let num = |w: Option<&String>| w.and_then(|v| v.parse::<f64>().ok()).ok_or("a number");
        match a.first().map(String::as_str) {
            None => {}
            Some("colour") => self.base = Base::Colour,
            Some("height") => self.base = Base::Height,
            Some("none") => self.base = Base::None,
            Some("siege") => self.show_siege = on(a.get(1))?,
            Some("deployment") => self.show_zones = on(a.get(1))?,
            Some("camera") => self.show_camera = on(a.get(1))?,
            Some("zoom") => {
                self.zoom = num(a.get(1))?.clamp(1.0, 64.0);
                self.centre = if a.len() >= 4 { Some([num(a.get(2))?, num(a.get(3))?]) } else { None };
            }

            Some("goto") => {
                let id = document_id(e).ok_or("no tile")?;
                move_camera(e, id, [num(a.get(1))?, num(a.get(2))?]);
            }

            Some(w) => return Err(format!("unknown {w}")),
        }

        let base = match self.base {
            Base::Colour => "colour",
            Base::Height => "height",
            Base::None => "none",
        };

        let siege = self.siege.as_ref().map_or_else(String::new, |s| {
            format!(", {} nodes, {} edges, {} deployment zone groups", s.layout.nodes.len(), s.layout.edges.len(), s.zones.len())
        });

        Ok(format!("minimap: {base}, siege {}, deployment {}, camera {}, zoom {:.2}{siege}", self.show_siege, self.show_zones, self.show_camera, self.zoom))
    }

    /// The baked colour as the picture, with the sea over it when the Ground section shows the sea.
    fn show_baked(&mut self, e: &mut Editor, id: DocumentId) {
        let Some((_, b)) = &self.colour_baked else { return };
        let d = e.docs.find_as::<TileDocument>(id).expect("a tile");
        self.colour_sea = d.tile().display().show_sea;
        let rgba = if self.colour_sea { d.tile().minimap_sea_over(b) } else { b.rgba.clone() };
        let (w, h, area) = (b.w, b.h, [b.x0, b.z0, b.w_m, b.h_m]);
        self.picture.set(e, &mut self.serial, w, h, rgba, area);
        self.picture_of = Some(Base::Colour);
    }

    /// Another tile: nothing of the last one is kept.
    fn forget(&mut self, e: &mut Editor) {
        self.picture.clear(e);
        *self = Self { base: self.base, show_siege: self.show_siege, show_zones: self.show_zones, show_camera: self.show_camera, serial: self.serial, wheel_owned: self.wheel_owned, ..Self::default() };
    }
}

/// The camera over `at` (x, z metres): the orbit's target goes there (on the ground, the eye keeping its place around it), the free camera's
/// eye goes there at the same height above the ground.
fn move_camera(e: &mut Editor, id: DocumentId, at: [f64; 2]) {
    let d = e.docs.find_as::<TileDocument>(id).expect("a tile");
    let ground = |x: f32, z: f32| d.project().height_at(f64::from(x), f64::from(z)) as f32;
    let (x, z) = (at[0] as f32, at[1] as f32);
    if e.viewport.is_fly() {
        let mut cam = e.viewport.camera();
        let dy = ground(x, z) - ground(cam.eye.x, cam.eye.z);
        let shift = wce_foundation::math::Float3::new(x - cam.eye.x, dy, z - cam.eye.z);
        cam.eye = cam.eye + shift;
        cam.target = cam.target + shift;
        e.viewport.controller().set_camera(&cam);
    } else {
        let y = ground(x, z);
        let o = e.viewport.orbit_mut();
        o.target = wce_foundation::math::Float3::new(x, y, z);
    }
}

fn screen_pts(f: &Frame, unit: f64, pts: &[P]) -> Vec<Vec2> {
    pts.iter().map(|p| f.screen(p[0] / unit, p[1] / unit)).collect()
}

/// Every deployment zone group: the defender's (alliance 1) blue, the attacker's red, any other alliance's yellow.
fn draw_zones(dl: &gui::DrawList, f: &Frame, s: &MinimapSiege) {
    for (alliance, zone) in &s.zones {
        let (r, g, b) = match alliance.as_str() {
            "1" => (70, 110, 255),
            "0" | "" => (255, 70, 60),
            _ => (240, 200, 40),
        };

        for (kind, p) in &zone.polygons {
            let pts = screen_pts(f, s.unit, p);
            match kind {
                ZoneKind::Additive => {
                    dl.concave_poly_filled(&pts, col32(r, g, b, 32));
                    dl.polyline(&pts, col32(r, g, b, 230), true, 2.0);
                }

                ZoneKind::Subtractive => {
                    dl.concave_poly_filled(&pts, col32(0, 0, 0, 70));
                    dl.polyline(&pts, col32(r, g, b, 200), true, 1.0);
                }

                ZoneKind::GuerrillaAdditive | ZoneKind::GuerrillaSubtractive => dl.polyline(&pts, col32(r, g, b, 140), true, 1.0),
            }
        }
    }
}

/// What `wce_siege::svg` draws, in its colours: the no-go regions, the buildings' outlines and the fort pieces, the perimeter, the nodes, the
/// edges, the lines with a direction (approaches, separators, a port's lines), the captures.
fn draw_siege(dl: &gui::DrawList, f: &Frame, s: &MinimapSiege) {
    let u = s.unit;
    let (input, l) = (&s.input, &s.layout);
    let fill = |p: &[P], col: u32| dl.concave_poly_filled(&screen_pts(f, u, p), col);
    let ring = |p: &[P], col: u32, t: f32| dl.polyline(&screen_pts(f, u, p), col, true, t);
    let line = |p: &[P], col: u32, t: f32| dl.polyline(&screen_pts(f, u, p), col, false, t);

    for p in &input.nogo {
        fill(p, col32(244, 120, 120, 90));
        ring(p, col32(220, 110, 110, 220), 1.0);
    }

    for p in &input.obstacles {
        fill(p, col32(200, 200, 200, 110));
    }

    for w in &input.walls {
        for r in &w.footprint {
            fill(r, if w.gate { col32(170, 51, 204, 220) } else { col32(110, 110, 110, 220) });
        }
    }

    ring(&l.perimeter, col32(51, 102, 204, 255), 2.0);
    for n in &l.nodes {
        let (r, g, b) = match n.kind {
            NodeKind::Area => (34, 170, 34),
            NodeKind::Intersection => (51, 102, 255),
            NodeKind::Entry => (204, 0, 204),
        };

        fill(&n.boundary, col32(r, g, b, 45));
        ring(&n.boundary, col32(r, g, b, 200), 1.0);
        fill(&n.bbox, col32(r, g, b, 65));
    }

    for e in &l.edges {
        let (col, t) = match e.kind {
            EdgeKind::Street => (col32(20, 20, 20, 230), 1.5),
            EdgeKind::AreaConnection => (col32(0, 187, 187, 255), 2.0),
            EdgeKind::Wall => (col32(238, 0, 0, 255), 2.0),
            EdgeKind::Palisade => (col32(136, 85, 0, 255), 2.5),
        };

        line(&e.line, col, t);
        for side in &e.sides {
            line(side, col32(238, 102, 170, 200), 1.0);
        }
    }

    // Lines with a direction: a dot at their end.
    let arrow = |p: &[P], col: u32| {
        line(p, col, 2.5);
        if let Some(end) = p.last() {
            dl.circle_filled(f.screen(end[0] / u, end[1] / u), 3.0, col);
        }
    };
    for a in &l.approaches {
        arrow(a, col32(0, 136, 136, 255));
    }

    for p in &l.separators {
        arrow(p, col32(136, 136, 0, 255));
    }

    for a in &l.deployment_lines {
        arrow(a, col32(170, 0, 221, 255));
    }

    for p in &l.beaches {
        arrow(p, col32(255, 136, 0, 255));
    }

    for a in &l.beach_approaches {
        arrow(a, col32(34, 187, 34, 255));
    }

    for c in &input.captures {
        let at = f.screen(c.at[0] / u, c.at[1] / u);
        let r = if c.major { 5.0 } else { 3.5 };
        dl.circle_filled(at, r + 1.0, col32(170, 136, 0, 255));
        dl.circle_filled(at, r, col32(255, 204, 0, 255));
    }
}

/// The camera: its eye, what it sees (the horizontal field of view) and the orbit's target.
fn draw_camera(dl: &gui::DrawList, f: &Frame, e: &Editor) {
    let cam = e.viewport.camera();
    let eye = f.screen(f64::from(cam.eye.x), f64::from(cam.eye.z));
    let (fx, fz) = (cam.target.x - cam.eye.x, cam.target.z - cam.eye.z);
    let len = (fx * fx + fz * fz).sqrt();
    if len > 1e-4 {
        // Screen y is -z.
        let (dx, dy) = (fx / len, -fz / len);
        let aspect = e.viewport.width().max(1) as f32 / e.viewport.height().max(1) as f32;
        let half = ((cam.fov_y * 0.5).tan() * aspect).atan();
        let reach = 48.0;
        let side = |a: f32| {
            let (s, c) = a.sin_cos();
            v2(eye.x + (dx * c - dy * s) * reach, eye.y + (dx * s + dy * c) * reach)
        };
        let (a, b) = (side(-half), side(half));
        dl.triangle_filled(eye, a, b, col32(255, 255, 255, 45));
        dl.polyline(&[a, eye, b], col32(255, 255, 255, 220), false, 1.5);
    }

    if !e.viewport.is_fly() {
        let t = f.screen(f64::from(cam.target.x), f64::from(cam.target.z));
        dl.line(v2(t.x - 4.0, t.y), v2(t.x + 4.0, t.y), col32(255, 255, 255, 220), 1.5);
        dl.line(v2(t.x, t.y - 4.0), v2(t.x, t.y + 4.0), col32(255, 255, 255, 220), 1.5);
    }

    dl.circle_filled(eye, 4.0, col32(0, 0, 0, 200));
    dl.circle_filled(eye, 3.0, col32(255, 230, 80, 255));
}

/// An arrow to the map's north (the viewport's compass's), at the top right.
fn draw_north(dl: &gui::DrawList, hi: Vec2, lo: Vec2, north: [f32; 2]) {
    let l = (north[0] * north[0] + north[1] * north[1]).sqrt();
    if l < 1e-6 {
        return;
    }

    let (dx, dy) = (north[0] / l, -north[1] / l);
    let c = v2(hi.x - 18.0, lo.y + 18.0);
    let tip = v2(c.x + dx * 11.0, c.y + dy * 11.0);
    let tail = v2(c.x - dx * 8.0, c.y - dy * 8.0);
    let (px, py) = (-dy * 5.0, dx * 5.0);
    dl.circle_filled(c, 14.0, col32(0, 0, 0, 110));
    dl.triangle_filled(tip, v2(tail.x + px, tail.y + py), v2(tail.x - px, tail.y - py), col32(235, 70, 60, 255));
    dl.text(v2(tail.x - dx * 6.0 - 4.0, tail.y - dy * 6.0 - 7.0), col32(255, 255, 255, 230), "N");
}
