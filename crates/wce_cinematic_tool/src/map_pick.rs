//! Picking where the battle is on its map: a picture of the battle map's heights (`lf_height_map.dds`, coloured as the Tile tool's
//! minimap), the position marked; a click moves it there (the battle XML's `tile_map_position`, 0 to 1 across and down).

use std::sync::Arc;
use std::sync::mpsc::{Receiver, channel};

use wce_editor::editor::Editor;
use wce_editor::gui::{self, v2};
use wce_vfs::files::Files;

/// The picture's widest side, pixels.
const SIDE: u32 = 1024;

#[derive(Default)]
pub struct MapPicker {
    /// The map shown and its picture's key in the editor's images (none: it has no height map).
    shown: Option<(String, Option<String>)>,
    loading: Option<(String, Receiver<Option<(u32, u32, Vec<u8>)>>)>,
    open: bool,
    /// The picture's zoom (1: fitted) and the point of the map at its centre.
    zoom: f32,
    centre: [f32; 2],
}

impl MapPicker {
    pub fn open(&mut self, at: [f32; 2]) {
        self.open = true;
        self.zoom = 1.0;
        self.centre = at;
    }

    fn picture(&mut self, e: &mut Editor, files: Option<&Arc<dyn Files>>, map: &str) -> Option<Option<wce_editor::images::ImageRef>> {
        if let Some((m, rx)) = &self.loading
            && let Ok(img) = rx.try_recv()
        {
            let key = img.map(|(w, h, px)| {
                let key = format!("cinematic/map/{m}");
                e.images.submit(&key, w, h, px);
                key
            });
            self.shown = Some((m.clone(), key));
            self.loading = None;
        }
        match &self.shown {
            Some((m, key)) if m == map => return Some(key.as_ref().and_then(|k| e.images.get(k))),
            _ => {}
        }
        if self.loading.as_ref().is_none_or(|(m, _)| m != map)
            && let Some(files) = files.cloned()
        {
            let (tx, rx) = channel();
            let path = format!("terrain/battles/{}/lf_height_map.dds", map.trim_matches('/'));
            std::thread::spawn(move || {
                let img = files
                    .read(&path)
                    .ok()
                    .and_then(|b| wce_tile::battle_map::l16_dds_heights(&b))
                    .filter(|lf| lf.w > 1 && lf.h > 1)
                    .map(|lf| wce_map_view::maps::minimap_pixels(&lf, SIDE));
                let _ = tx.send(img);
            });
            self.loading = Some((map.to_owned(), rx));
        }
        None
    }

    /// The window while open; the position clicked, if any.
    pub fn window(&mut self, e: &mut Editor, files: Option<&Arc<dyn Files>>, map: &str, at: [f32; 2]) -> Option<[f32; 2]> {
        if !self.open {
            return None;
        }
        let pic = self.picture(e, files, map);
        gui::set_next_window_size(v2(720.0, 600.0), gui::COND_FIRST_USE_EVER);
        let mut open = true;
        if !gui::begin(&format!("Battle position: {map}###cin_map_pick"), Some(&mut open), 0) {
            gui::end();
            self.open = open;
            return None;
        }
        self.open = open;
        let mut picked = None;
        match pic {
            None => gui::text_disabled("Reading the map's heights..."),
            Some(None) => gui::text_disabled("This map has no height map in the packs"),
            Some(Some(img)) => {
                let avail = gui::content_region_avail();
                let origin = gui::cursor_screen_pos();
                let fit = (avail.x / img.w as f32).min((avail.y - 4.0) / img.h as f32).max(0.01);
                let k = fit * self.zoom;
                let (w, h) = (img.w as f32 * k, img.h as f32 * k);
                // The map's point at the middle of the panel.
                let mid = v2(origin.x + avail.x * 0.5, origin.y + avail.y * 0.5);
                let lo = v2(mid.x - self.centre[0] * w, mid.y - self.centre[1] * h);
                let to_s = |p: [f32; 2]| v2(lo.x + p[0] * w, lo.y + p[1] * h);
                let to_m = |s: gui::Vec2| [((s.x - lo.x) / w).clamp(0.0, 1.0), ((s.y - lo.y) / h).clamp(0.0, 1.0)];
                let dl = gui::window_draw_list();
                dl.push_clip_rect(origin, v2(origin.x + avail.x, origin.y + avail.y));
                dl.image(img.id, lo, v2(lo.x + w, lo.y + h), v2(0.0, 0.0), v2(1.0, 1.0), 0xFFFF_FFFF);
                let c = to_s(at);
                dl.circle_filled(c, 7.0, gui::col32(10, 10, 12, 200));
                dl.circle_filled(c, 5.0, gui::col32(255, 210, 80, 255));
                dl.line(v2(c.x - 14.0, c.y), v2(c.x + 14.0, c.y), gui::col32(255, 210, 80, 255), 1.5);
                dl.line(v2(c.x, c.y - 14.0), v2(c.x, c.y + 14.0), gui::col32(255, 210, 80, 255), 1.5);
                dl.pop_clip_rect();
                gui::invisible_button("##mappick", avail, gui::BUTTON_LEFT | gui::BUTTON_RIGHT);
                if gui::is_item_hovered() {
                    let m = to_m(gui::mouse_pos());
                    gui::set_tooltip(&format!("{:.4}, {:.4}  ·  click: the battle here  ·  wheel: zoom  ·  right drag: pan", m[0], m[1]));
                    let wheel = gui::io().MouseWheel;
                    if wheel != 0.0 {
                        // Zoom about the mouse.
                        let before = m;
                        self.zoom = (self.zoom * if wheel > 0.0 { 1.25 } else { 0.8 }).clamp(1.0, 40.0);
                        let k2 = fit * self.zoom;
                        let (w2, h2) = (img.w as f32 * k2, img.h as f32 * k2);
                        let mp = gui::mouse_pos();
                        self.centre = [before[0] - (mp.x - mid.x) / w2, before[1] - (mp.y - mid.y) / h2];
                    }
                    if gui::is_mouse_down(1) {
                        let d = gui::io().MouseDelta;
                        self.centre = [self.centre[0] - d.x / w, self.centre[1] - d.y / h];
                    }
                }
                self.centre = self.centre.map(|v| v.clamp(0.0, 1.0));
                if gui::is_item_clicked() {
                    picked = Some(to_m(gui::mouse_pos()));
                }
            }
        }
        gui::end();
        picked
    }
}
