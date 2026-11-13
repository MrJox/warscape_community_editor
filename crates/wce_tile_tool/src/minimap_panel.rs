//! The Minimap panel (docs/plans/editor_todo.md): the open tile from above, north (+z) up and +x right, as a base picture (a view of the
//! ground as the viewport's ground views show it, baked here: the colour or a debug view of the tile's maps; its heights; or nothing) under
//! layers that are toggled on and off: the siege AI (what `wce_siege::svg` draws), the deployment zones, the tile's cells (the entry's
//! mask), the automatic no-go areas and the camera. The base is picked from a chip at the map's top left (the north arrow is at its top
//! right), as the viewport's ground views.
//! The left button moves the camera to where it is pressed (held: it follows), the wheel zooms around the cursor, the middle button pans.

use std::sync::mpsc;

use wce_editor::editor::Editor;
use wce_editor_core::camera_controller::CameraMode;
use wce_editor_core::document::Document;
use wce_editor::gui::{self, Vec2, col32, v2};
use wce_editor::{icons, theme};
use wce_foundation::log;
use wce_scene::render_scene::DocumentId;
use wce_siege::{EdgeKind, NodeKind, P, ZoneKind};
use wce_tile::blend_field::BlendField;
use wce_tile::ground_type_field::GroundTypeField;
use wce_tile_editor::terrain_bake::{BakedColour, TerrainView};
use wce_tile_editor::tile_document::TileDocument;
use wce_tile_editor::tile_document::minimap::MinimapSiege;

use crate::ground_views::{AUTO_NOGO_TIP, begin_chip_popup, chip, season_name, view_items, view_name};
use crate::tool::document_id;

/// The side of the pictures made for the minimap, at most.
const HEIGHT_DIM: u32 = 512;
const GROUND_DIM: u32 = 1024;

/// Seconds between two pictures of the heights while they change (a brush stroke), and two reads of the siege AI while the tile is edited
/// (it waits for the buttons to be released too).
const HEIGHT_EVERY: f64 = 0.3;
const SIEGE_EVERY: f64 = 0.5;

/// What the minimap shows under its layers: a view of the ground (as the viewport's), the heights, or nothing.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Base {
    Ground(TerrainView),
    Height,

    /// The radar the tile would ship (`wce_tile_editor::radar`, docs/plans/radar.md).
    Radar,
    None,
}

/// The bases of the minimap only (after the ground views), with their names and the `minimap` command's words; the ground views' words.
const OWN_BASES: [(Base, &str, &str); 3] = [(Base::Height, "height", "height"), (Base::Radar, "radar", "radar"), (Base::None, "none", "none")];

/// Seconds between two radars while the tile is edited (it waits for the buttons to be released too).
const RADAR_EVERY: f64 = 1.0;

/// The radar's file in the tile's folder: the game reads a JPEG by this name (CA's: 512 x 512 baseline RGB), the export carries it.
pub const RADAR_FILE: &str = "loading_screen_radar.jpg";
/// JPEG quality of a written radar (CA's 512 x 512 radars are about 190 kB, a high quality).
const RADAR_JPEG_QUALITY: u8 = 92;
/// The JPEG comment (COM segment) marking a radar the editor wrote: one without it is hand-made and is only replaced with `force`.
const RADAR_MARK: &[u8] = b"wce radar generator";
const VIEW_WORDS: [(TerrainView, &str); 5] = [
    (TerrainView::Colour, "colour"), (TerrainView::BlendGroups, "groups"), (TerrainView::GroundTypes, "ground"), (TerrainView::Protection, "protection"),
    (TerrainView::WaterDepth, "water"),
];

fn base_name(base: Base) -> &'static str {
    match base {
        Base::Ground(v) => view_name(v),
        Base::Height => "height",
        Base::Radar => "radar",
        Base::None => "none",
    }
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

    /// The texture and the ground it covers, once it is on the GPU.
    fn image(&self, e: &Editor) -> Option<(u64, [f64; 4])> {
        e.images.get(&self.key).map(|i| (i.id, self.area))
    }
}

/// What a picture of the ground is of: the tile, the view, the look's climate and season (the views that read the climate's textures), and
/// the maps it reads as they were (the blend map's and the ground type map's edit depths, the water depth's raster by address).
#[derive(Clone, Debug, PartialEq, Eq)]
struct GroundKey {
    doc: DocumentId,
    view: TerrainView,
    climate: String,
    season: String,
    maps: (usize, usize, usize),
}

fn ground_key(d: &TileDocument, doc: DocumentId, view: TerrainView) -> GroundKey {
    let (p, t) = (d.project(), d.tile());
    let blend = p.blend.as_ref().map_or(0, BlendField::edit_depth);
    let ground = p.ground.as_ref().map_or(0, GroundTypeField::edit_depth);
    let water = t.water_depth().map_or(0, |w| std::ptr::from_ref(w) as usize);
    let maps = match view {
        TerrainView::Colour | TerrainView::BlendGroups => (blend, 0, 0),
        TerrainView::GroundTypes => (0, ground, 0),
        TerrainView::WaterDepth => (0, ground, water),
        TerrainView::Protection => (0, 0, 0),
    };

    let look = t.terrain_look();
    let (climate, season) = if matches!(view, TerrainView::Colour | TerrainView::BlendGroups) { (look.climate.clone(), look.season.clone()) } else { Default::default() };
    GroundKey { doc, view, climate, season, maps }
}

/// A view of the ground baked for the minimap on a thread (`Tile::minimap_ground_bake`): the last bake, shown while the next one is made
/// (one at a time), and its picture (the colour with the sea over it while the sea shows).
#[derive(Default)]
struct GroundPicture {
    job: Option<(GroundKey, mpsc::Receiver<Result<BakedColour, String>>)>,
    baked: Option<(GroundKey, BakedColour)>,
    failed: Option<(GroundKey, String)>,
    picture: Picture,
    picture_of: Option<(GroundKey, bool)>,
}

impl GroundPicture {
    /// Up to date with the tile's `view`: a bake started when what it shows changed.
    fn update(&mut self, e: &mut Editor, serial: &mut u64, id: DocumentId, view: TerrainView) {
        if let Some((key, rx)) = &self.job {
            match rx.try_recv() {
                Ok(Ok(b)) => {
                    self.baked = Some((key.clone(), b));
                    self.job = None;
                }

                Ok(Err(why)) => {
                    log::info(format!("minimap: no {}: {why}", view_name(key.view)));
                    self.failed = Some((key.clone(), why));
                    self.job = None;
                }

                Err(mpsc::TryRecvError::Empty) => {}
                Err(mpsc::TryRecvError::Disconnected) => self.job = None,
            }
        }

        let Some(d) = e.docs.find_as::<TileDocument>(id) else { return };
        let key = ground_key(d, id, view);
        let done = self.baked.as_ref().is_some_and(|(k, _)| *k == key) || self.failed.as_ref().is_some_and(|(k, _)| *k == key);
        if !done && self.job.is_none() {
            match d.tile().minimap_ground_bake(view, GROUND_DIM) {
                Ok(bake) => {
                    let (tx, rx) = mpsc::channel();
                    std::thread::spawn(move || {
                        let _ = tx.send(bake.run());
                    });
                    self.job = Some((key, rx));
                }

                Err(why) => {
                    log::info(format!("minimap: no {}: {why}", view_name(view)));
                    self.failed = Some((key, why));
                }
            }
        }

        // The picture of the last bake of this tile and view.
        let sea = view == TerrainView::Colour && d.tile().display().show_sea;
        let made = match &self.baked {
            Some((k, b)) if k.doc == id && k.view == view => {
                let of = (k.clone(), sea);
                (self.picture_of.as_ref() != Some(&of)).then(|| {
                    let rgba = if sea { d.tile().minimap_sea_over(b) } else { b.rgba.clone() };
                    (b.w, b.h, rgba, [b.x0, b.z0, b.w_m, b.h_m], of)
                })
            }

            _ => {
                if self.picture_of.is_some() {
                    self.picture.clear(e);
                    self.picture_of = None;
                }

                None
            }
        };

        if let Some((w, h, rgba, area, of)) = made {
            self.picture.set(e, serial, w, h, rgba, area);
            self.picture_of = Some(of);
        }
    }

    fn image(&self, e: &Editor) -> Option<(u64, [f64; 4])> {
        self.picture_of.as_ref().and_then(|_| self.picture.image(e))
    }

    /// What the picture is: being baked, why there is none, or the bake's report.
    fn note(&self) -> String {
        match (&self.job, &self.failed, &self.baked) {
            (Some((k, _)), _, _) => format!("baking the {}...", view_name(k.view)),
            (None, Some((k, why)), _) if self.baked.as_ref().is_none_or(|(b, _)| b != k) => format!("no {}: {why}", view_name(k.view)),
            (None, _, Some((k, _))) if k.view == TerrainView::Colour => format!("{}, {} (the blend's CPU stand-in)", k.climate, season_name(&k.season)),
            (None, _, Some((_, b))) => b.report.clone(),
            _ => String::new(),
        }
    }

    fn clear(&mut self, e: &mut Editor) {
        self.picture.clear(e);
        *self = Self::default();
    }
}

pub struct MinimapUi {
    base: Base,
    show_siege: bool,
    show_zones: bool,
    show_cells: bool,
    show_nogo: bool,
    show_camera: bool,

    /// 1 shows the whole area; the world point in the middle (`None`: the area's centre).
    zoom: f64,
    centre: Option<[f64; 2]>,

    /// The window was drawn last frame (open, not collapsed, not a hidden tab): the views it needs are worked out only then.
    visible: bool,

    doc: DocumentId,
    serial: u64,
    note: String,

    /// The colour (a base, and the Tile panel's valid cells over it) and the other views of the ground.
    colour: GroundPicture,
    ground: GroundPicture,

    /// The heights' picture: the terrain version it is of (`None`: not made), when it was made, the ground's range.
    heights: Picture,
    height_for: Option<u64>,
    height_time: f64,
    height_range: (f64, f64),

    /// The radar: its picture, what it was drawn from (the history's version, whether the water depth and the no-go areas were in, how many
    /// models had their bounds: the water planes' footprints), when.
    radar: Picture,
    radar_for: Option<(u64, bool, bool, usize)>,
    radar_time: f64,

    siege: Option<MinimapSiege>,
    siege_for: u64,
    siege_time: f64,

    /// The middle button is panning from this cursor position.
    pan_from: Option<Vec2>,
    wheel_owned: bool,
    /// The fit pill's size last frame (it sits in the map's bottom right corner).
    fit_size: Vec2,
}

impl Default for MinimapUi {
    fn default() -> Self {
        Self {
            base: Base::Ground(TerrainView::Colour), show_siege: true, show_zones: true, show_cells: false, show_nogo: false, show_camera: true, zoom: 1.0, centre: None,
            visible: false, doc: 0, serial: 0, note: String::new(), colour: GroundPicture::default(), ground: GroundPicture::default(), heights: Picture::default(),
            height_for: None, height_time: -1e9, height_range: (0.0, 0.0), radar: Picture::default(), radar_for: None, radar_time: -1e9, siege: None, siege_for: 0, siege_time: -1e9, pan_from: None, wheel_owned: false, fit_size: v2(0.0, 0.0),
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
        self.visible = open;
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

    /// The ground view the minimap shows while it shows (for the water depth, which the tool works out).
    pub fn view(&self) -> Option<TerrainView> {
        match self.base {
            Base::Ground(v) if self.visible => Some(v),
            _ => None,
        }
    }

    /// The automatic no-go areas show on it (the tool works them out); the radar's cliffs are drawn from them too.
    pub fn shows_auto_nogos(&self) -> bool {
        self.visible && (self.show_nogo || self.base == Base::Radar)
    }

    /// The radar shows: the tool works out the water depth for its rivers.
    pub fn wants_radar(&self) -> bool {
        self.visible && self.base == Base::Radar
    }

    /// The ground's colour picture, for the Tile panel's valid cells (its bake starts now when it is not made): the texture and the ground
    /// it covers (x0, z0, w, h metres; rows along +z).
    pub fn colour_picture(&mut self, e: &mut Editor, id: DocumentId) -> Option<(u64, [f64; 4])> {
        if id != self.doc {
            self.forget(e);
            self.doc = id;
        }

        self.colour.update(e, &mut self.serial, id, TerrainView::Colour);
        self.colour.image(e)
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
        let base = match self.base {
            Base::Ground(TerrainView::Colour) => self.colour.image(e),
            Base::Ground(_) => self.ground.image(e),
            Base::Height => self.heights.image(e),
            Base::Radar => self.radar.image(e),
            Base::None => None,
        };

        if let Some((tex, [x0, z0, w, h])) = base {
            dl.image(tex, frame.screen(x0, z0 + h), frame.screen(x0 + w, z0), v2(0.0, 1.0), v2(1.0, 0.0), col32(255, 255, 255, 255));
        }

        // The tile's edge (the area may hold the height map's border around it).
        let d = e.docs.find_as::<TileDocument>(id).expect("a tile");
        let (ex, ez) = (d.tile().extent_x(), d.tile().extent_z());
        dl.rect(frame.screen(0.0, ez), frame.screen(ex, 0.0), col32(255, 255, 255, 120), 0.0, 1.0);

        if self.show_cells {
            draw_cells(&dl, &frame, d);
        }

        if self.show_nogo
            && let Some(nogos) = d.tile().auto_nogos()
        {
            draw_nogos(&dl, &frame, nogos);
        }

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

        // The chips over the map: the base's and the overlays' at the top left, fit at the bottom right (the north marker has the top right).
        let hovered = self.draw_chip(lo) && self.draw_fit(hi) && hovered;

        // The left button: the camera goes there.
        if active && gui::is_mouse_down(0) {
            let at = frame.world(mouse);
            move_camera(e, id, at);
        }

        if hovered {
            let at = frame.world(mouse);
            let d = e.docs.find_as::<TileDocument>(id).expect("a tile");
            let h = d.project().height_at(at[0], at[1]);
            let cell = if self.show_cells { cell_at(d, at).map_or_else(String::new, |(x, y, ok)| format!(", cell ({x}, {y}) {}", if ok { "valid" } else { "not the tile's" })) } else { String::new() };
            gui::set_tooltip(&format!("{:.0}, {:.0} m, ground {h:.1} m{cell}\nleft: camera here, wheel: zoom, middle: pan", at[0], at[1]));
        }

        hovered
    }

    /// The base's chip and its popup (the ground views, then the minimap's own bases), and the overlays' chip; false while the cursor is
    /// over the pill.
    fn draw_chip(&mut self, lo: Vec2) -> bool {
        gui::set_cursor_screen_pos(v2(lo.x + 6.0, lo.y + 6.0));
        gui::push_style_color(gui::COL_CHILD_BG, gui::v4(0.08, 0.08, 0.09, 0.78));
        let mut over = false;
        if gui::begin_pill("##minimapchip") {
            let tip = format!("Minimap: {}\nClick: the ground views, the heights, the radar or nothing", base_name(self.base));
            if chip(self.base != Base::Ground(TerrainView::Colour), icons::LAYERS, "minimapbase", &tip) {
                gui::open_popup("minimapbase");
            }

            over |= gui::is_item_hovered();
            if begin_chip_popup("minimapbase") {
                gui::text_disabled("Minimap");
                let current = match self.base {
                    Base::Ground(v) => Some(v),
                    _ => None,
                };

                if let Some(v) = view_items(current, "minimapview") {
                    self.base = Base::Ground(v);
                }

                gui::separator();
                for (base, name, _) in OWN_BASES {
                    if gui::radio_button(&format!("{name}##minimapbase"), self.base == base) {
                        self.base = base;
                    }
                }

                gui::end_popup();
            }

            gui::same_line_touching();
            let shown = [(self.show_siege, "siege AI"), (self.show_zones, "deployment"), (self.show_cells, "cells"), (self.show_nogo, "no-go"), (self.show_camera, "camera")]
                .into_iter()
                .filter_map(|(on, name)| on.then_some(name))
                .collect::<Vec<_>>();
            let tip = format!("Overlays: {}\nClick: what is drawn over the map", if shown.is_empty() { "none".to_owned() } else { shown.join(", ") });
            if chip(!shown.is_empty(), icons::EYE, "minimapoverlays", &tip) {
                gui::open_popup("minimapoverlays");
            }

            over |= gui::is_item_hovered();
            if begin_chip_popup("minimapoverlays") {
                self.draw_overlays();
                gui::end_popup();
            }
        }

        gui::end_child();
        gui::pop_style_color(1);
        !over
    }

    fn draw_overlays(&mut self) {
        gui::text_disabled("Overlays");
        gui::checkbox("Siege AI##minimap", &mut self.show_siege);
        gui::checkbox("Deployment##minimap", &mut self.show_zones);
        gui::checkbox("Cells##minimap", &mut self.show_cells);
        if gui::is_item_hovered() {
            gui::set_tooltip("The tile's cells (its database entry's): the valid ones outlined, the others darkened (the Tile panel's valid cells).");
        }

        gui::checkbox("No-go##minimap", &mut self.show_nogo);
        if gui::is_item_hovered() {
            gui::set_tooltip(AUTO_NOGO_TIP);
        }

        gui::checkbox("Camera##minimap", &mut self.show_camera);
    }

    /// The fit pill at the bottom right of the map (`hi`): the whole tile again; false while the cursor is over it.
    fn draw_fit(&mut self, hi: Vec2) -> bool {
        gui::set_cursor_screen_pos(v2(hi.x - self.fit_size.x - 6.0, hi.y - self.fit_size.y - 6.0));
        gui::push_style_color(gui::COL_CHILD_BG, gui::v4(0.08, 0.08, 0.09, 0.78));
        let mut over = false;
        if gui::begin_pill("##minimapfit") {
            if chip(false, icons::FOCUS, "minimapfit", "Fit: the whole tile in the panel again (undoes the zoom and the pan)") {
                self.zoom = 1.0;
                self.centre = None;
            }

            over = gui::is_item_hovered();
        }

        gui::end_child();
        let (l, h) = gui::item_rect();
        self.fit_size = v2(h.x - l.x, h.y - l.y);
        gui::pop_style_color(1);
        !over
    }

    /// Makes what is out of date: the base picture, the siege AI.
    fn refresh(&mut self, e: &mut Editor, id: DocumentId) {
        let now = gui::time();
        self.note.clear();
        match self.base {
            Base::None => {}
            Base::Height => self.refresh_heights(e, id, now),
            Base::Radar => self.refresh_radar(e, id, now),
            Base::Ground(TerrainView::Colour) => {
                self.colour.update(e, &mut self.serial, id, TerrainView::Colour);
                self.note = self.colour.note();
            }

            Base::Ground(v) => {
                self.ground.update(e, &mut self.serial, id, v);
                self.note = self.ground.note();
            }
        }

        let d = e.docs.find_as::<TileDocument>(id).expect("a tile");
        if self.show_nogo && d.tile().auto_nogos().is_none() {
            self.note = format!("{}{}working out the no-go areas...", self.note, if self.note.is_empty() { "" } else { "; " });
        }

        if self.show_siege || self.show_zones {
            let version = d.tile().siege_version();
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

    fn refresh_radar(&mut self, e: &mut Editor, id: DocumentId, now: f64) {
        let d = e.docs.find_as::<TileDocument>(id).expect("a tile");
        let key = (d.history().version(), d.tile().water_depth().is_some(), d.tile().auto_nogos().is_some(), d.tile().radar_models_resolved());
        let idle = !(0..3).any(gui::is_mouse_down);
        if self.radar_for != Some(key) && (self.radar_for.is_none() || (idle && now - self.radar_time >= RADAR_EVERY)) {
            let started = std::time::Instant::now();
            let (size, rgba, area) = draw_radar(e, id);
            self.radar.set(e, &mut self.serial, size, size, rgba, area);
            self.radar_for = Some(key);
            self.radar_time = now;
            log::info(format!("radar drawn in {:.2} s ({} models with their bounds)", started.elapsed().as_secs_f64(), key.3));
        }

        let (water, nogo) = (key.1, key.2);
        self.note = match (water, nogo) {
            (true, true) => format!("the radar the tile would ship (`radar_save` writes {RADAR_FILE})"),
            _ => "radar: working out the rivers and the cliffs...".to_owned(),
        };
    }

    /// Writes the open tile's radar to `path` (a PNG when it ends in `.png`, else the game's JPEG; the tile's folder's `RADAR_FILE`
    /// without one). A JPEG the editor did not write (a hand-made radar, CA's in a decompiled tile) is only replaced with `force`.
    ///
    /// # Errors
    ///
    /// No tile, a hand-made radar in the way without `force`, or the file does not write.
    pub fn save_radar(&mut self, e: &mut Editor, path: Option<&str>, force: bool) -> Result<String, String> {
        let id = document_id(e).ok_or("no tile")?;
        let d = e.docs.find_as::<TileDocument>(id).expect("a tile");
        let (water, nogo) = (d.tile().water_depth().is_some(), d.tile().auto_nogos().is_some());
        let path = path.map_or_else(|| d.project().dir.join(RADAR_FILE), std::path::PathBuf::from);
        let png = path.extension().is_some_and(|x| x.eq_ignore_ascii_case("png"));
        if !force
            && !png
            && let Ok(old) = std::fs::read(&path)
            && !radar_is_generated(&old)
        {
            return Err(format!("{} is a hand-made radar: `radar_save force` replaces it", path.display()));
        }
        let (size, rgba, _) = draw_radar(e, id);
        // Back to rows from the north.
        let row = size as usize * 4;
        let pixels: Vec<u8> = rgba.chunks(row).rev().flatten().copied().collect();
        let bytes = if png {
            let png = wce_formats::png_file::PngFile { width: size, height: size, bit_depth: 8, color_type: 6, pixels, pixels_edited: true, ..Default::default() };
            png.encode().map_err(|e| e.to_string())?
        } else {
            radar_jpeg(size, &pixels)?
        };
        std::fs::write(&path, bytes).map_err(|e| format!("{}: {e}", path.display()))?;
        let missing = match (water, nogo) {
            (true, true) => String::new(),
            (false, false) => " (without rivers and cliffs: show the radar on the minimap first)".to_owned(),
            (false, true) => " (without rivers: show the radar on the minimap first)".to_owned(),
            (true, false) => " (without cliffs: show the radar on the minimap first)".to_owned(),
        };
        Ok(format!("radar written to {}{missing}", path.display()))
    }

    fn refresh_heights(&mut self, e: &mut Editor, id: DocumentId, now: f64) {
        let d = e.docs.find_as::<TileDocument>(id).expect("a tile");
        let version = d.tile().terrain_version();
        if self.height_for != Some(version) && (self.height_for.is_none() || now - self.height_time >= HEIGHT_EVERY) {
            wce_foundation::wce_prof!("tile.minimap.heights");
            match d.tile().minimap_heights(HEIGHT_DIM) {
                Some(img) => {
                    self.height_range = img.range;
                    self.heights.set(e, &mut self.serial, img.w, img.h, img.rgba, [img.x0, img.z0, img.w_m, img.h_m]);
                }

                None => self.heights.clear(e),
            }

            self.height_for = Some(version);
            self.height_time = now;
        }

        if self.heights.key.is_empty() {
            self.note = "no height map".to_owned();
        } else {
            self.note = format!("ground from {:.1} to {:.1} m", self.height_range.0, self.height_range.1);
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
        let word = a.first().map(String::as_str);
        if let Some((v, _)) = VIEW_WORDS.iter().find(|(_, w)| Some(*w) == word) {
            self.base = Base::Ground(*v);
        } else if let Some((b, _, _)) = OWN_BASES.iter().find(|(_, _, w)| Some(*w) == word) {
            self.base = *b;
        } else {
            match word {
                None => {}
                Some("siege") => self.show_siege = on(a.get(1))?,
                Some("deployment") => self.show_zones = on(a.get(1))?,
                Some("cells") => self.show_cells = on(a.get(1))?,
                Some("nogo") => self.show_nogo = on(a.get(1))?,
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
        }

        let siege = self.siege.as_ref().map_or_else(String::new, |s| {
            format!(", {} nodes, {} edges, {} deployment zone groups", s.layout.nodes.len(), s.layout.edges.len(), s.zones.len())
        });

        Ok(format!(
            "minimap: {}, siege {}, deployment {}, cells {}, no-go {}, camera {}, zoom {:.2}{siege}{}", base_name(self.base), self.show_siege, self.show_zones,
            self.show_cells, self.show_nogo, self.show_camera, self.zoom, if self.note.is_empty() { String::new() } else { format!(" ({})", self.note) },
        ))
    }

    /// Another tile: nothing of the last one is kept.
    fn forget(&mut self, e: &mut Editor) {
        self.colour.clear(e);
        self.ground.clear(e);
        self.heights.clear(e);
        self.radar.clear(e);
        *self = Self {
            base: self.base, show_siege: self.show_siege, show_zones: self.show_zones, show_cells: self.show_cells, show_nogo: self.show_nogo, show_camera: self.show_camera,
            visible: self.visible, serial: self.serial, wheel_owned: self.wheel_owned, ..Self::default()
        };
    }
}

/// The radar of a tile as it is (with the water depth and the no-go areas the tool has worked out): its side, its pixels with rows along +z
/// (as the minimap's pictures), the ground it covers.
fn draw_radar(e: &mut Editor, id: DocumentId) -> (u32, Vec<u8>, [f64; 4]) {
    wce_foundation::wce_prof!("tile.minimap.radar");
    let d = e.docs.find_as_mut::<TileDocument>(id).expect("a tile");
    let water = d.tile().water_depth().cloned();
    let input = d.tile_mut().radar_input(water.as_ref());
    let img = wce_tile_editor::radar::render(&input, &wce_tile_editor::radar::RadarStyle::default());
    let row = img.size as usize * 4;
    let rgba = img.rgba.chunks(row).rev().flatten().copied().collect();
    (img.size, rgba, input.area)
}

/// The entry's cells (x, rows from the +z edge, as the entry counts them) and whether each is valid (its mask; all without one).
fn tile_cells(d: &TileDocument) -> Option<(usize, usize, Vec<bool>)> {
    let entry = d.project().entry.as_ref()?;
    let (w, h) = (entry.width() as usize, entry.height() as usize);
    let mask = entry.tile_attr("mask").unwrap_or_default();
    let cells = if mask.len() == w * h { mask.bytes().map(|c| c == b'1').collect() } else { vec![true; w * h] };
    Some((w, h, cells))
}

/// The cell under `at` (metres) and whether it is valid.
fn cell_at(d: &TileDocument, at: [f64; 2]) -> Option<(usize, usize, bool)> {
    let (w, h, cells) = tile_cells(d)?;
    let (x, z) = ((at[0] / 128.0).floor(), (at[1] / 128.0).floor());
    if x < 0.0 || z < 0.0 || x >= w as f64 || z >= h as f64 {
        return None;
    }

    let (x, y) = (x as usize, h - 1 - z as usize);
    Some((x, y, cells[y * w + x]))
}

/// The tile's cells: the ones not the tile's darkened, the valid ones outlined in the highlight colour.
fn draw_cells(dl: &gui::DrawList, f: &Frame, d: &TileDocument) {
    let Some((w, h, cells)) = tile_cells(d) else { return };
    let rect = |x: usize, y: usize| {
        let (x0, z0, x1, z1) = d.tile().cell_rect(x as i32, y as i32);
        (f.screen(f64::from(x0), f64::from(z1)), f.screen(f64::from(x1), f64::from(z0)))
    };

    for y in 0..h {
        for x in 0..w {
            if !cells[y * w + x] {
                let (lo, hi) = rect(x, y);
                dl.rect_filled(lo, hi, col32(0, 0, 0, 150), 0.0);
                dl.rect(lo, hi, col32(140, 140, 140, 110), 0.0, 1.0);
            }
        }
    }

    let hl = theme::u32(theme::palette().highlight);
    for y in 0..h {
        for x in 0..w {
            if cells[y * w + x] {
                let (lo, hi) = rect(x, y);
                dl.rect(lo, hi, hl, 0.0, 1.5);
            }
        }
    }
}

/// The automatic no-go areas, filled red, as the NOGO polygons' colour in the viewport.
fn draw_nogos(dl: &gui::DrawList, f: &Frame, nogos: &[Vec<[f32; 2]>]) {
    for poly in nogos {
        let pts = poly.iter().map(|p| f.screen(f64::from(p[0]), f64::from(p[1]))).collect::<Vec<_>>();
        dl.concave_poly_filled(&pts, col32(255, 64, 38, 70));
        dl.polyline(&pts, col32(255, 64, 38, 230), true, 1.5);
    }
}

/// The camera over `at` (x, z metres): what it looks at on the ground goes there (the walker stands there), its angles and distance kept;
/// the fly camera's eye goes there at the same height above the ground.
fn move_camera(e: &mut Editor, id: DocumentId, at: [f64; 2]) {
    let d = e.docs.find_as::<TileDocument>(id).expect("a tile");
    let ground = |x: f32, z: f32| d.project().height_at(f64::from(x), f64::from(z)) as f32;
    let (x, z) = (at[0] as f32, at[1] as f32);
    let p = if e.viewport.mode() == CameraMode::Fly {
        let eye = e.viewport.camera().eye;
        wce_foundation::math::Float3::new(x, eye.y + ground(x, z) - ground(eye.x, eye.z), z)
    } else {
        wce_foundation::math::Float3::new(x, ground(x, z), z)
    };

    e.viewport.controller().move_pivot(p);
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

    // The walls: their footprints' triangles, cut once (`MinimapSiege::wall_fills`).
    for (gate, tris) in &s.wall_fills {
        let col = if *gate { col32(170, 51, 204, 220) } else { col32(110, 110, 110, 220) };
        let at = |q: [f32; 2]| f.screen(f64::from(q[0]) / u, f64::from(q[1]) / u);
        for t in tris {
            dl.triangle_filled(at(t[0]), at(t[1]), at(t[2]), col);
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

/// The camera: its eye, what it sees (the horizontal field of view, the top-down view's rectangle) and what it looks at.
fn draw_camera(dl: &gui::DrawList, f: &Frame, e: &Editor) {
    let cam = e.viewport.camera();
    if cam.is_ortho() {
        let aspect = e.viewport.width().max(1) as f32 / e.viewport.height().max(1) as f32;
        let (right, up, _) = cam.axes();
        let (hw, hh) = cam.half_extents(0.0, aspect);
        let corner = |sx: f32, sy: f32| {
            let p = cam.eye + right * (sx * hw) + up * (sy * hh);
            f.screen(f64::from(p.x), f64::from(p.z))
        };

        let pts = [corner(-1.0, -1.0), corner(1.0, -1.0), corner(1.0, 1.0), corner(-1.0, 1.0)];
        dl.polyline(&pts, col32(255, 255, 255, 220), true, 1.5);
    }

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

    if !matches!(e.viewport.mode(), CameraMode::Walk | CameraMode::Fly) {
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

/// The radar (rows from the north) as the game's JPEG: RGB (the alpha dropped), baseline, `RADAR_JPEG_QUALITY`, with `RADAR_MARK` as
/// a comment after the JFIF header (APP0 has to follow SOI).
///
/// # Errors
///
/// The encoder fails.
pub fn radar_jpeg(size: u32, rgba: &[u8]) -> Result<Vec<u8>, String> {
    let rgb: Vec<u8> = rgba.as_chunks::<4>().0.iter().flat_map(|p| [p[0], p[1], p[2]]).collect();
    let mut jpeg = vec![];
    image::codecs::jpeg::JpegEncoder::new_with_quality(&mut jpeg, RADAR_JPEG_QUALITY)
        .encode(&rgb, size, size, image::ExtendedColorType::Rgb8)
        .map_err(|e| e.to_string())?;
    let len = u16::try_from(RADAR_MARK.len() + 2).expect("a short mark");
    let mut com = vec![0xFF, 0xFE];
    com.extend_from_slice(&len.to_be_bytes());
    com.extend_from_slice(RADAR_MARK);
    let at = if jpeg[2..4] == [0xFF, 0xE0] { 4 + usize::from(u16::from_be_bytes([jpeg[4], jpeg[5]])) } else { 2 };
    jpeg.splice(at..at, com);
    Ok(jpeg)
}

/// Whether a radar file is one the editor wrote (`RADAR_MARK` in a COM segment before the image data).
fn radar_is_generated(bytes: &[u8]) -> bool {
    if !bytes.starts_with(&[0xFF, 0xD8]) {
        return false;
    }
    let mut at = 2;
    while at + 4 <= bytes.len() && bytes[at] == 0xFF {
        let marker = bytes[at + 1];
        let len = usize::from(u16::from_be_bytes([bytes[at + 2], bytes[at + 3]]));
        if marker == 0xDA || len < 2 || at + 2 + len > bytes.len() {
            return false;
        }
        if marker == 0xFE && &bytes[at + 4..at + 2 + len] == RADAR_MARK {
            return true;
        }
        at += 2 + len;
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn radar_jpeg_decodes_and_is_marked() {
        let size = 16;
        let rgba: Vec<u8> = (0..size * size).flat_map(|i| [(i % 256) as u8, 120, 80, 255]).collect();
        let jpeg = radar_jpeg(size, &rgba).unwrap();
        assert!(radar_is_generated(&jpeg));
        assert_eq!(&jpeg[..4], &[0xFF, 0xD8, 0xFF, 0xE0], "JFIF's APP0 right after SOI");
        let img = image::load_from_memory_with_format(&jpeg, image::ImageFormat::Jpeg).unwrap();
        assert_eq!((img.width(), img.height(), img.color()), (size, size, image::ColorType::Rgb8));
        // A JPEG without the mark (a hand-made radar) is not ours, nor is anything that is not a JPEG.
        let mut plain = vec![];
        image::codecs::jpeg::JpegEncoder::new(&mut plain).encode(&[0; 3 * 4], 2, 2, image::ExtendedColorType::Rgb8).unwrap();
        assert!(!radar_is_generated(&plain));
        assert!(!radar_is_generated(b"\x89PNG"));
    }
}
