//! The Weather gallery (docs/editor.md, "Weather tool"): the .environment files the Files panel lists, each drawn on the editor's scene
//! from the camera of when the gallery was opened or refreshed, as thumbnails; a click opens one. One thumbnail is drawn per frame by a
//! preview renderer of its own (`preview.rs`): its temporal state starts over for each, and a few frames a quarter of a second apart let
//! the exposure settle and the precipitation fall.

use std::collections::VecDeque;

use wce_editor::editor::Editor;
use wce_editor::gui;
use wce_formats::xml_doc::XmlDoc;
use wce_scene::camera::Camera;
use wce_scene::environment::read_environment;

/// How many files the gallery draws at most (the Files panel's filter narrows them).
const MAX: usize = 60;

/// A thumbnail's size (points) and the frames that settle it.
const THUMB: (f32, f32) = (224.0, 126.0);
const FRAMES: u32 = 16;
const STEP_S: f32 = 0.25;

#[derive(Default)]
pub(crate) struct Gallery {
    pub(crate) open: bool,
    preview: crate::preview::Preview,

    /// The camera the thumbnails are drawn from, and the round they belong to (a refresh starts a new one).
    camera: Option<Camera>,
    round: u64,
    queue: VecDeque<String>,
    queued: Vec<String>,
    made: Vec<String>,
}

fn key(round: u64, file: &str) -> String {
    format!("weather_gallery/{round}/{file}")
}

impl Gallery {
    /// Starts over from the current camera (the old thumbnails are dropped).
    pub(crate) fn refresh(&mut self, e: &mut Editor) {
        for k in self.made.drain(..) {
            e.images.forget(&k);
        }

        self.round += 1;
        self.camera = Some(e.viewport.camera());
        self.queue.clear();
        self.queued.clear();
    }

    /// Draws the next thumbnail waiting, if the gallery is open.
    pub(crate) fn update(&mut self, e: &mut Editor) {
        let now = e.clock.time();
        if !self.open {
            self.preview.drop_if_idle(now);
            return;
        }

        let (Some(camera), Some(file)) = (self.camera.clone(), self.queue.pop_front()) else {
            self.preview.drop_if_idle(now);
            return;
        };

        let Some(env) = e.vfs().and_then(|v| v.read(&file).ok()).and_then(|b| XmlDoc::parse(b).ok()).map(|d| read_environment(&d)) else {
            return;
        };

        let density = gui::io().DisplayFramebufferScale.x.max(1.0);
        let px = ((THUMB.0 * density) as u32, (THUMB.1 * density) as u32);
        if !self.preview.render(e, env, camera, px, FRAMES, STEP_S, true) {
            return;
        }

        if let Some(rgba) = self.preview.target.read_rgba8(e.device(), e.context()) {
            let k = key(self.round, &file);
            e.images.submit(&k, px.0, px.1, rgba);
            self.made.push(k);
        }
    }

    /// The gallery's window over `files` (the Files panel's listing); the file clicked.
    pub(crate) fn draw(&mut self, e: &mut Editor, files: &[String]) -> Option<String> {
        if !self.open {
            return None;
        }

        if self.camera.is_none() {
            self.refresh(e);
        }

        let mut open = true;
        if !gui::begin("Weather gallery", Some(&mut open), 0) {
            gui::end();
            self.open = open;
            return None;
        }

        self.open = open;
        let files = files.iter().filter(|f| f.ends_with(".environment")).take(MAX).cloned().collect::<Vec<_>>();
        if gui::button("Refresh from this camera") {
            self.refresh(e);
        }

        if gui::is_item_hovered() {
            gui::set_tooltip("Draws the thumbnails again from where the viewport's camera is now.");
        }

        gui::same_line();
        gui::text_disabled(&format!(
            "{} files of the Files panel's list{}, each on this scene; click one to open it",
            files.len(),
            if files.len() == MAX { " (the first ones: narrow it with its filter)" } else { "" }
        ));

        let per_row = ((gui::content_region_avail().x + 8.0) / (THUMB.0 + 8.0)).floor().max(1.0) as usize;
        let mut picked = None;
        for (i, file) in files.iter().enumerate() {
            if i % per_row != 0 {
                gui::same_line();
            }

            gui::begin_group();
            match e.images.get(&key(self.round, file)) {
                Some(img) => {
                    if gui::invisible_button(&format!("##g{i}"), gui::v2(THUMB.0, THUMB.1), gui::BUTTON_LEFT) {
                        picked = Some(file.clone());
                    }

                    let (lo, hi) = gui::item_rect();
                    let dl = gui::window_draw_list();
                    dl.image(img.id, lo, hi, gui::v2(0.0, 0.0), gui::v2(1.0, 1.0), gui::col32(255, 255, 255, 255));
                    if gui::is_item_hovered() {
                        dl.rect(lo, hi, gui::col32(255, 210, 80, 255), 2.0, 2.0);
                    }
                }

                None => {
                    if !self.queued.contains(file) {
                        self.queued.push(file.clone());
                        self.queue.push_back(file.clone());
                    }

                    gui::dummy(gui::v2(THUMB.0, THUMB.1));
                    let (lo, hi) = gui::item_rect();
                    gui::window_draw_list().rect(lo, hi, gui::col32(80, 80, 80, 255), 2.0, 1.0);
                }
            }

            if gui::is_item_hovered() {
                gui::set_tooltip(&format!("{file}\nClick: open it"));
            }

            let name = file.rsplit('/').next().unwrap_or(file).trim_end_matches(".environment");
            gui::push_text_wrap_pos(gui::cursor_pos_x() + THUMB.0);
            gui::text_disabled(name);
            gui::pop_text_wrap_pos();
            gui::end_group();
        }

        gui::end();
        picked
    }
}
