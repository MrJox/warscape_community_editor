//! The Map panel: the active map seen from above, one cell a texel. Its layers: the LF (shaded as the overview map), the placements (each
//! placed tile in its tile set's colour: a map of the packs has no tile map image, its placements stand for it) and a kit map's cell
//! layers (tile map, climate, culture, snow, season masks, layout). The wheel zooms around the cursor, the middle button pans, a click
//! moves the camera over the cell; the cell under the cursor is named (its colour in the tile database, the placement over it). With the
//! cell paint on, the left button paints instead (`input`, for the tool); a painted layer's picture is written where it changed.

use wce_editor::editor::Editor;
use wce_editor::gui::{self, Vec2, col32, v2};
use wce_map_view::MapView;
use wce_map_view::maps::minimap_pixels;
use wce_scene::render_scene::DocumentId;
use wce_terrain::LayerRole;
use wce_terrain_editor::MapDocument;
use wce_tile::battle_map::context::CELL;

/// The widest side of the LF picture (texels).
const LF_SIDE: u32 = 2048;

/// What the panel shows.
#[derive(Clone, Debug, PartialEq)]
enum Layer {
    Lf,
    Placements,

    /// A kit map's cell layer (its index in the model).
    Cells(usize),
}

#[derive(Default)]
pub struct MapPanel {
    layer: usize,

    /// The picture on the GPU: its key in `Editor::images`, what it is of (document, layer), its size in texels.
    shown: Option<(String, DocumentId, Layer, u32, u32)>,

    /// The view: the map cell at the middle (none: the whole map) and the zoom (1: the map fits).
    centre: Option<[f64; 2]>,
    zoom: f64,
    pan_from: Option<Vec2>,
    wheel_owned: bool,

    /// The cell under the cursor, said.
    hover: String,

    /// The picture's texels (for the ground), its id (a new picture, a new id), and whether it lies over the ground and how strong.
    texels: Option<std::sync::Arc<Vec<u8>>>,
    picture_id: u64,
    pub drape: bool,
    pub opacity: f32,

    /// Set by the tool before `draw`: the left button paints (no camera move), and the rectangle being dragged (cells, inclusive).
    pub paint: bool,
    pub outline: Option<[i64; 4]>,

    /// After `draw`: the cell under the cursor and the left button's state over the map this frame.
    pub input: Option<MapInput>,
}

/// The cursor over the map (`MapPanel::input`).
#[derive(Clone, Copy, Debug)]
pub struct MapInput {
    pub cell: (i64, i64),
    pub pressed: bool,
    pub down: bool,
    pub released: bool,
}

/// The layers a map has, with their names.
fn layers(d: &MapDocument) -> Vec<(Layer, String)> {
    let mut out = vec![(Layer::Lf, "LF heights".to_owned()), (Layer::Placements, "placements (tile sets)".to_owned())];
    if let Some(m) = d.model() {
        for (k, (role, item)) in m.layers.iter().enumerate() {
            let name = match role {
                LayerRole::TileMap => "tile map".to_owned(),
                LayerRole::Climate => "climates".to_owned(),
                LayerRole::Culture => "cultures".to_owned(),
                LayerRole::Snow(s) if s.is_empty() => "snow".to_owned(),
                LayerRole::Snow(s) => format!("snow ({s})"),
                LayerRole::SeasonMask(s) => format!("season mask ({s})"),
                LayerRole::Layout => "layout".to_owned(),
            };

            out.push((Layer::Cells(k), format!("{name} - {}", item.path)));
        }
    }

    out
}

/// A layer's picture: its size and RGBA texels (rows from the map's top).
fn picture(d: &MapDocument, layer: &Layer) -> Option<(u32, u32, Vec<u8>)> {
    let m = &d.placed;
    match layer {
        Layer::Lf => m.lf.as_ref().filter(|f| f.w > 1 && f.h > 1).map(|lf| minimap_pixels(lf, LF_SIDE)),
        Layer::Placements => {
            let (w, h) = (m.width, m.height);
            let mut px = vec![0u8; (w * h * 4) as usize];
            for p in px.chunks_exact_mut(4) {
                p.copy_from_slice(&[18, 18, 22, 255]);
            }

            for p in &m.placements {
                let c = d.db.tiles.iter().find(|t| t.location.eq_ignore_ascii_case(&p.location)).and_then(|t| t.set).map_or([200.0, 0.0, 200.0], |s| d.db.sets[s].colour);
                let r = p.rect();
                for y in r[1].max(0)..r[3].min(h as i32) {
                    for x in r[0].max(0)..r[2].min(w as i32) {
                        let o = ((y as u32 * w + x as u32) * 4) as usize;
                        px[o..o + 4].copy_from_slice(&[c[0] as u8, c[1] as u8, c[2] as u8, 255]);
                    }
                }
            }

            Some((w, h, px))
        }

        Layer::Cells(k) => {
            let (_, item) = d.model()?.layers.get(*k)?;
            let l = item.get();
            let mut px = Vec::with_capacity(l.cells().len() * 4);
            for &i in l.cells() {
                let c = l.palette()[usize::from(i)];
                px.extend_from_slice(&[c[0], c[1], c[2], 255]);
            }

            Some((l.width(), l.height(), px))
        }
    }
}

/// What a colour of a layer stands for in the tile database: a tile set, a tile, a variation (the tile map), a climate.
fn colour_name(d: &MapDocument, role: Option<&LayerRole>, c: [u8; 4]) -> String {
    let eq = |k: [f32; 3]| k[0] == f32::from(c[0]) && k[1] == f32::from(c[1]) && k[2] == f32::from(c[2]);
    let db = &d.db;
    if role == Some(&LayerRole::Climate) {
        return db.climates.iter().find(|k| eq(k.colour)).map_or_else(|| "no climate of the database".to_owned(), |k| k.name.clone());
    }

    if role == Some(&LayerRole::TileMap) {
        if c[..3] == [0, 0, 0] {
            return "nothing (black)".to_owned();
        }

        if let Some(s) = db.sets.iter().find(|s| eq(s.colour)) {
            return format!("tile set {}", s.name);
        }

        if let Some(t) = db.tiles.iter().find(|t| eq(t.colour)) {
            return format!("tile {}", t.location);
        }

        if let Some((t, v)) = db.tiles.iter().find_map(|t| t.variations.iter().find(|v| eq(v.colour)).map(|v| (t, v))) {
            return format!("variation {} of {}", v.location, t.location);
        }

        return "in no tile database entry".to_owned();
    }

    String::new()
}

impl MapPanel {
    pub fn new() -> Self {
        Self { opacity: 0.5, ..Default::default() }
    }

    /// The picture over the ground of the 3D view (the scene's ground overlay), when on; taken off otherwise.
    pub fn apply_drape(&self, e: &mut Editor, doc: DocumentId, view: &MapView) {
        let overlay = (|| {
            if !self.drape {
                return None;
            }

            let (_, shown_doc, _, w, h) = self.shown.as_ref()?;
            let d = e.docs.find_as::<MapDocument>(*shown_doc).filter(|_| *shown_doc == doc)?;
            let f = view.frame()?;
            let (sx, sy) = (f64::from(*w) / f64::from(d.placed.width.max(1)), f64::from(*h) / f64::from(d.placed.height.max(1)));
            let m = &f.to_map.m;
            let to_texel = [
                [(m[0][0] * sx) as f32, (m[0][1] * sx) as f32, (m[0][2] * sx) as f32],
                [(m[1][0] * sy) as f32, (m[1][1] * sy) as f32, (m[1][2] * sy) as f32],
            ];

            Some(wce_scene::render_scene::GroundOverlay { to_texel, w: *w, h: *h, rgba: self.texels.clone()?, id: self.picture_id, opacity: self.opacity, layer: view.layer() })
        })();

        e.scene.set_ground_overlay(overlay);
    }

    /// Cells of a kit map's layer (its index in the model) changed in `rect` (x0, y0, x1, y1; end exclusive): the picture there, when it
    /// shows that layer; the ground's copy when `drape` (a whole picture: at a stroke's end, not every dab).
    pub fn cells_changed(&mut self, e: &mut Editor, doc: DocumentId, k: usize, rect: [u32; 4], drape: bool) {
        let Some((key, shown_doc, Layer::Cells(shown), w, _)) = &self.shown else { return };
        if *shown_doc != doc || *shown != k {
            return;
        }

        let (key, w) = (key.clone(), *w);
        let Some(l) = e.docs.find_as::<MapDocument>(doc).and_then(|d| d.cell_layer(k)) else { return };
        let Some(texels) = self.texels.as_mut() else { return };
        let px = std::sync::Arc::make_mut(texels);
        for y in rect[1]..rect[3].min(l.height()) {
            for x in rect[0]..rect[2].min(l.width()) {
                let c = l.colour(x, y);
                let o = ((y * w + x) * 4) as usize;
                px[o..o + 4].copy_from_slice(&[c[0], c[1], c[2], 255]);
            }
        }

        e.images.patch(&key, texels.clone(), rect);
        if drape {
            self.picture_id += 1;
        }
    }

    /// Shows the `k`-th layer of the list (scripts).
    pub fn set_layer(&mut self, k: usize) {
        self.layer = k;
    }

    pub fn draw(&mut self, e: &mut Editor, doc: DocumentId, view: &MapView) {
        let open = gui::begin("Map", None, 0);
        let hovered = open && self.draw_body(e, doc, view);
        gui::end();

        // The wheel over the map is the zoom's; given back when the cursor leaves it.
        if hovered {
            gui::own_mouse_wheel(Some(gui::get_id("terrain map wheel")));
            self.wheel_owned = true;
        } else if self.wheel_owned {
            gui::own_mouse_wheel(None);
            self.wheel_owned = false;
        }
    }

    /// True when the cursor is over the map.
    fn draw_body(&mut self, e: &mut Editor, doc: DocumentId, view: &MapView) -> bool {
        let Some(d) = e.docs.find_as::<MapDocument>(doc) else {
            gui::text_disabled("Open a map in the Terrain panel.");
            return false;
        };

        let list = layers(d);
        self.layer = self.layer.min(list.len() - 1);
        let names: Vec<&str> = list.iter().map(|(_, n)| n.as_str()).collect();
        let mut k = self.layer as i32;
        if gui::combo("layer", &mut k, &names) {
            self.layer = k.max(0) as usize;
        }

        let layer = list[self.layer].0.clone();
        let role = match &layer {
            Layer::Cells(k) => d.model().and_then(|m| m.layers.get(*k)).map(|(r, _)| r.clone()),
            _ => None,
        };

        // The picture, made when the document or the layer changes.
        if self.shown.as_ref().is_none_or(|s| s.1 != doc || s.2 != layer) {
            let key = format!("terrain/map/{doc}");
            if let Some((old, ..)) = self.shown.take() {
                e.images.forget(&old);
            }

            let d = e.docs.find_as::<MapDocument>(doc).expect("found above");
            self.texels = None;
            if let Some((w, h, px)) = picture(d, &layer) {
                let texels = std::sync::Arc::new(px);
                e.images.forget(&key);
                e.images.submit_patchable(&key, w, h, texels.to_vec());
                self.texels = Some(texels);
                self.picture_id += 1;
                self.shown = Some((key, doc, layer.clone(), w, h));
            }
        }

        gui::checkbox("on the ground", &mut self.drape);
        if self.drape {
            gui::same_line();
            gui::set_next_item_width(120.0);
            gui::slider_float("opacity", &mut self.opacity, 0.05, 1.0, "%.2f");
        }

        let d = e.docs.find_as::<MapDocument>(doc).expect("found above");
        let (mw, mh) = (f64::from(d.placed.width.max(1)), f64::from(d.placed.height.max(1)));
        if !self.hover.is_empty() {
            gui::text_wrapped(&self.hover);
        } else {
            gui::text_disabled("wheel: zoom, middle button: pan, click: the camera there");
        }

        let avail = gui::content_region_avail();
        if avail.x < 32.0 || avail.y < 32.0 {
            return false;
        }

        let origin = gui::cursor_screen_pos();
        gui::invisible_button("##terrain map", avail, gui::BUTTON_LEFT | gui::BUTTON_MIDDLE);
        let (hovered, active) = (gui::is_item_hovered(), gui::is_item_active());
        let (lo, hi) = (origin, v2(origin.x + avail.x, origin.y + avail.y));
        let mid = v2((lo.x + hi.x) * 0.5, (lo.y + hi.y) * 0.5);
        let fit = (f64::from(avail.x) / mw).min(f64::from(avail.y) / mh);
        if self.zoom < 1.0 {
            self.zoom = 1.0;
        }

        let mut centre = self.centre.unwrap_or([mw * 0.5, mh * 0.5]);
        let mut scale = fit * self.zoom;
        let to_cell = |p: Vec2, centre: [f64; 2], scale: f64| [centre[0] + f64::from(p.x - mid.x) / scale, centre[1] + f64::from(p.y - mid.y) / scale];
        let mouse = gui::mouse_pos();
        if hovered {
            let wheel = f64::from(gui::io().MouseWheel);
            if wheel != 0.0 {
                let at = to_cell(mouse, centre, scale);
                self.zoom = (self.zoom * 1.25f64.powf(wheel)).clamp(1.0, 256.0);
                scale = fit * self.zoom;
                let moved = to_cell(mouse, centre, scale);
                centre = [centre[0] + at[0] - moved[0], centre[1] + at[1] - moved[1]];
                self.centre = Some(centre);
            }
        }

        if active && gui::is_mouse_down(2) {
            if let Some(from) = self.pan_from {
                centre = [centre[0] - f64::from(mouse.x - from.x) / scale, centre[1] - f64::from(mouse.y - from.y) / scale];
                self.centre = Some(centre);
            }

            self.pan_from = Some(mouse);
        } else {
            self.pan_from = None;
        }

        let screen = |cx: f64, cy: f64| v2(mid.x + ((cx - centre[0]) * scale) as f32, mid.y + ((cy - centre[1]) * scale) as f32);
        let dl = gui::window_draw_list();
        dl.push_clip_rect(lo, hi);
        dl.rect_filled(lo, hi, col32(24, 25, 28, 255), 0.0);
        if let Some((key, ..)) = &self.shown
            && let Some(img) = e.images.get(key)
        {
            dl.image(img.id, screen(0.0, 0.0), screen(mw, mh), v2(0.0, 0.0), v2(1.0, 1.0), col32(255, 255, 255, 255));
        }

        dl.rect(screen(0.0, 0.0), screen(mw, mh), col32(255, 255, 255, 90), 0.0, 1.0);
        if let Some(o) = self.outline {
            dl.rect(screen(o[0] as f64, o[1] as f64), screen(o[2] as f64 + 1.0, o[3] as f64 + 1.0), col32(255, 220, 60, 255), 0.0, 2.0);
        }

        // The camera and where it looks.
        if let Some(f) = view.frame() {
            let cam = e.viewport.camera();
            let (ex, ey) = f.cell_of(f64::from(cam.eye.x), f64::from(cam.eye.z));
            let (tx, ty) = f.cell_of(f64::from(cam.target.x), f64::from(cam.target.z));
            let reach = f64::from(cam.zfar) / CELL;
            let (dx, dy) = (tx - ex, ty - ey);
            let l = dx.hypot(dy).max(1e-9);
            dl.line(screen(ex, ey), screen(ex + dx / l * reach.min(l.max(8.0)), ey + dy / l * reach.min(l.max(8.0))), col32(255, 255, 255, 160), 1.0);
            dl.circle_filled(screen(ex, ey), 4.0, col32(230, 60, 50, 255));
        }

        dl.pop_clip_rect();

        // The cell under the cursor; a click moves the camera over it (paints, with the cell paint on).
        self.hover.clear();
        self.input = None;
        if hovered || (self.paint && active) {
            let c = to_cell(mouse, centre, scale);
            self.input = Some(MapInput {
                cell: (c[0].floor() as i64, c[1].floor() as i64), pressed: hovered && gui::is_mouse_clicked(0), down: active && gui::is_mouse_down(0),
                released: gui::is_mouse_released(0),
            });
        }

        if hovered {
            let c = to_cell(mouse, centre, scale);
            let (x, y) = (c[0].floor(), c[1].floor());
            if x >= 0.0 && y >= 0.0 && x < mw && y < mh {
                let (x, y) = (x as i32, y as i32);
                let mut s = format!("cell {x}, {y}");
                if let Layer::Cells(k) = &layer
                    && let Some((_, item)) = d.model().and_then(|m| m.layers.get(*k))
                {
                    let l = item.get();
                    if (x as u32) < l.width() && (y as u32) < l.height() {
                        let col = l.colour(x as u32, y as u32);
                        let name = colour_name(d, role.as_ref(), col);
                        s += &format!("  colour {}, {}, {}{}", col[0], col[1], col[2], if name.is_empty() { String::new() } else { format!(": {name}") });
                    }
                }

                if let Some(&i) = d.placed.in_rect([x, y, x + 1, y + 1]).first() {
                    let p = &d.placed.placements[i];
                    s += &format!("  placement {}\\{} at {}, {}", p.location, p.variation, p.x, p.y);
                }

                self.hover = s;
            }

            if gui::is_mouse_clicked(0)
                && !self.paint
                && let Some(f) = view.frame()
            {
                let (fx, fz) = f.from_map.apply(c[0], c[1]);
                let cam = e.viewport.camera();
                let dlt = wce_foundation::math::Float3::new(fx as f32 - cam.target.x, 0.0, fz as f32 - cam.target.z);
                e.viewport.controller().set_camera(&wce_scene::camera::Camera { eye: cam.eye + dlt, target: cam.target + dlt, ..cam });
            }
        }

        hovered
    }
}
