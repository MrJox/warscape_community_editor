//! The Tile tool's reference image and its screenshot of the viewport.
//!
//! The reference image is a PNG (a city plan, a sketch, a game screenshot) to lay a tile out over, shown at an opacity: over the viewport
//! fitted to it (the Weather tool's reference, for a camera held still), or laid on the tile's ground from above, in metres, north (+z) up
//! and +x right like the Minimap, so that it stays where it is while the camera moves. On the ground it follows the terrain (a grid of
//! cells, each corner at the ground's height) and is drawn over the scene, not into it: a guide, not part of the tile. It is never saved
//! in the tile; the settings keep each tile's image and placement (`[tools.tile] reference_images`).
//!
//! The screenshot saves the viewport's last image (the render, without the chips and labels drawn over it) as a PNG named after the
//! tile and its variation, numbered, in the screenshot folder (Settings > Tile).

use std::path::{Path, PathBuf};

use wce_editor::editor::Editor;
use wce_editor::images::read_png_rgba8;
use wce_editor::steam_detect::file_dialog;
use wce_editor::{gui, theme};
use wce_editor_core::document::Document;
use wce_formats::png_file::PngFile;
use wce_foundation::log;
use wce_foundation::math::Float3;

use crate::settings::{ReferenceImage, ReferenceShow, SETTINGS_ID, TileSettings};
use crate::tool::{document, document_id};
use wce_tile_editor::tile_document::TileDocument;

const KEY: &str = "tile_reference";

/// The ground's cells along the picture's longer side (each cell's corners follow the terrain).
const GROUND_CELLS: usize = 32;

#[derive(Default)]
pub(crate) struct ReferenceUi {
    /// The tile whose image this is (its settings key); the image is read again when another tile is open.
    tile: Option<String>,

    /// The image's settings (saved when an edit ends).
    pub(crate) s: ReferenceImage,

    /// The picture's size in pixels once read.
    size: Option<(u32, u32)>,
    made: bool,
    dirty: bool,
}

impl ReferenceUi {
    /// Follows the open tile: its image and placement from the settings.
    fn follow(&mut self, e: &mut Editor) {
        let key = document(e).map(|d| d.source_key());
        if key == self.tile {
            return;
        }

        self.forget(e);
        self.tile = key.clone();
        self.s = key.and_then(|k| e.tool_settings::<TileSettings>(SETTINGS_ID).reference_images.get(&k).cloned()).unwrap_or_default();
        if !self.s.path.is_empty()
            && let Err(m) = self.read(e)
        {
            log::warn(format!("reference image: {m}"));
        }
    }

    fn forget(&mut self, e: &mut Editor) {
        if self.made {
            e.images.forget(KEY);
        }

        self.made = false;
        self.size = None;
    }

    /// Reads `s.path` into the picture shown.
    fn read(&mut self, e: &mut Editor) -> Result<(), String> {
        let (w, h, rgba) = read_png_rgba8(Path::new(&self.s.path))?;
        self.forget(e);
        e.images.submit(KEY, w, h, rgba);
        self.made = true;
        self.size = Some((w, h));
        Ok(())
    }

    /// Loads a PNG as the open tile's reference image. A first image is laid over the whole tile on the ground.
    ///
    /// # Errors
    ///
    /// No tile open, or the file can't be read or is not a PNG of 8-bit RGB or RGBA.
    pub(crate) fn load(&mut self, e: &mut Editor, path: &Path) -> Result<(), String> {
        self.follow(e);
        if self.tile.is_none() {
            return Err("no tile open".to_owned());
        }

        let old = std::mem::replace(&mut self.s.path, path.display().to_string());
        if let Err(m) = self.read(e) {
            self.s.path = old;
            return Err(m);
        }

        if self.s.show == ReferenceShow::Off {
            self.s.show = ReferenceShow::Ground;
        }

        if self.s.width <= 0.0 {
            self.fit_to_tile(e);
        }

        self.save(e);
        Ok(())
    }

    /// Lays the picture over the whole tile (inside it, keeping its proportions), unturned.
    pub(crate) fn fit_to_tile(&mut self, e: &mut Editor) {
        let (Some(d), Some((w, h))) = (document(e), self.size) else { return };
        let t = d.tile();
        let (ex, ez) = (t.extent_x() as f32, t.extent_z() as f32);
        self.s.centre = [ex * 0.5, ez * 0.5];
        self.s.width = ex.min(ez * w as f32 / h.max(1) as f32);
        self.s.rotation = 0.0;
        self.dirty = true;
    }

    /// Writes the image's settings for the open tile.
    pub(crate) fn save(&mut self, e: &mut Editor) {
        self.dirty = false;
        let Some(key) = self.tile.clone() else { return };
        let s = self.s.clone();
        e.update_tool_settings::<TileSettings>(SETTINGS_ID, |t| {
            if s.path.is_empty() {
                t.reference_images.remove(&key);
            } else {
                t.reference_images.insert(key, s);
            }
        });
    }

    /// Sets how it is shown, its opacity and where it lies on the ground (centre x, z, width, turn), each when given (a command's).
    pub(crate) fn set(&mut self, e: &mut Editor, show: Option<ReferenceShow>, opacity: Option<f32>, placement: Option<[f32; 4]>) {
        self.follow(e);
        if let Some(s) = show {
            self.s.show = s;
        }

        if let Some(o) = opacity {
            self.s.opacity = o.clamp(0.0, 1.0);
        }

        if let Some([x, z, w, r]) = placement {
            (self.s.centre, self.s.width, self.s.rotation) = ([x, z], w.max(0.0), r);
        }

        self.save(e);
    }

    /// The picture's corners on the ground (top left, top right, bottom right, bottom left: north up), at a share (u, v) of it.
    fn ground_at(&self, u: f32, v: f32) -> Option<(f32, f32)> {
        let (w, h) = self.size?;
        let width = self.s.width;
        let height = width * h as f32 / w.max(1) as f32;
        let (lx, lz) = ((u - 0.5) * width, (0.5 - v) * height);
        let (sin, cos) = self.s.rotation.to_radians().sin_cos();
        // Clockwise seen from above (north up, +x right).
        Some((self.s.centre[0] + lx * cos + lz * sin, self.s.centre[1] - lx * sin + lz * cos))
    }

    /// Over the viewport, under the tool's chips: the picture over the image or on the ground.
    pub(crate) fn draw_overlay(&mut self, e: &mut Editor, origin: gui::Vec2, size: gui::Vec2) {
        self.follow(e);
        if self.dirty && !gui::is_any_item_active() {
            self.save(e);
        }

        let (Some(img), Some((w, h))) = (e.images.get(KEY), self.size) else { return };
        let tint = gui::col32(255, 255, 255, (self.s.opacity.clamp(0.0, 1.0) * 255.0) as u8);
        let dl = gui::window_draw_list();
        match self.s.show {
            ReferenceShow::Off => {}
            ReferenceShow::Screen => {
                let k = (size.x / w as f32).min(size.y / h as f32);
                let (fw, fh) = (w as f32 * k, h as f32 * k);
                let lo = gui::v2(origin.x + (size.x - fw) * 0.5, origin.y + (size.y - fh) * 0.5);
                dl.image(img.id, lo, gui::v2(lo.x + fw, lo.y + fh), gui::v2(0.0, 0.0), gui::v2(1.0, 1.0), tint);
            }

            ReferenceShow::Ground => {
                let Some(d) = document_id(e).and_then(|id| e.docs.find_as::<TileDocument>(id)) else { return };
                let t = d.tile();
                if self.s.width <= 0.0 {
                    return;
                }

                // A grid over the picture, finer along its longer side; each corner at the ground's height, projected.
                let (nu, nv) = if w >= h {
                    (GROUND_CELLS, ((GROUND_CELLS * h as usize) / w.max(1) as usize).max(1))
                } else {
                    (((GROUND_CELLS * w as usize) / h.max(1) as usize).max(1), GROUND_CELLS)
                };
                let density = gui::io().DisplayFramebufferScale.x.max(1e-3);
                let mut pts = vec![None; (nu + 1) * (nv + 1)];
                for j in 0..=nv {
                    for i in 0..=nu {
                        let (u, v) = (i as f32 / nu as f32, j as f32 / nv as f32);
                        let Some((x, z)) = self.ground_at(u, v) else { continue };
                        let p = Float3::new(x, t.terrain_height(x, z) + 0.05, z);
                        pts[j * (nu + 1) + i] =
                            e.viewport.project(p).map(|(px, py)| gui::v2(origin.x + px / density, origin.y + py / density));
                    }
                }

                dl.push_clip_rect(origin, gui::v2(origin.x + size.x, origin.y + size.y));
                for j in 0..nv {
                    for i in 0..nu {
                        let at = |i: usize, j: usize| pts[j * (nu + 1) + i];
                        let (Some(a), Some(b), Some(c), Some(d)) = (at(i, j), at(i + 1, j), at(i + 1, j + 1), at(i, j + 1)) else {
                            continue;
                        };
                        let uv = |i: usize, j: usize| gui::v2(i as f32 / nu as f32, j as f32 / nv as f32);
                        dl.image_quad(img.id, [a, b, c, d], [uv(i, j), uv(i + 1, j), uv(i + 1, j + 1), uv(i, j + 1)], tint);
                    }
                }

                dl.pop_clip_rect();
            }
        }
    }

    /// The Tile panel's section.
    pub(crate) fn draw_section(&mut self, e: &mut Editor) {
        self.follow(e);
        if !gui::collapsing_header("Reference image", 0) {
            return;
        }

        gui::set_next_item_width(-90.0);
        let enter =
            gui::input_text("##tilerefpath", Some("a plan or picture (.png)"), &mut self.s.path, 512, gui::INPUT_ENTER_RETURNS_TRUE);
        if gui::is_item_hovered() {
            gui::set_tooltip(
                "A PNG to lay the tile out over (a city plan, a sketch, a screenshot). Kept in the settings for this tile, never saved in it.",
            );
        }

        gui::same_line();
        let mut pick = None;
        if gui::button("Load...##tileref") {
            let start = Path::new(&self.s.path).parent().map(Path::to_path_buf).unwrap_or_default();
            pick = file_dialog(false, &start, &[("PNG".to_owned(), "*.png".to_owned())]);
        }

        if gui::is_item_hovered() {
            gui::set_tooltip("Pick the picture.");
        }

        if enter && !self.s.path.trim().is_empty() {
            pick = Some(PathBuf::from(self.s.path.trim()));
        }

        if let Some(p) = pick
            && let Err(m) = self.load(e, &p)
        {
            e.status(&m, true);
        }

        if self.size.is_none() {
            return;
        }

        let mut changed = false;
        for (show, name, tip) in [
            (ReferenceShow::Off, "hidden", "The picture is not shown."),
            (
                ReferenceShow::Ground,
                "on the ground",
                "Laid on the tile from above, in metres: it stays where it is while the camera moves.",
            ),
            (
                ReferenceShow::Screen,
                "on the screen",
                "Over the viewport, fitted to it: it does not follow the camera (hold the camera still, e.g. the top-down one).",
            ),
        ] {
            if gui::radio_button(&format!("{name}##tilerefshow"), self.s.show == show) {
                self.s.show = show;
                changed = true;
            }

            if gui::is_item_hovered() {
                gui::set_tooltip(tip);
            }

            gui::same_line();
        }

        gui::new_line();
        gui::set_next_item_width(-1.0);
        changed |= gui::slider_float("##tilerefopacity", &mut self.s.opacity, 0.0, 1.0, "opacity %.2f");
        if gui::is_item_hovered() {
            gui::set_tooltip("How opaque the picture is: slide it to look from one to the other.");
        }

        gui::begin_disabled(self.s.show != ReferenceShow::Ground);
        gui::set_next_item_width(-110.0);
        changed |= gui::drag_float2("centre (x, z m)##tileref", &mut self.s.centre, 0.5, "%.1f");
        if gui::is_item_hovered_allow_disabled() {
            gui::set_tooltip("Where the picture's centre is on the tile (metres; the tile runs from 0 to its size).");
        }

        gui::set_next_item_width(-110.0);
        changed |= gui::drag_float("width (m)##tileref", &mut self.s.width, 0.5, 1.0, 100_000.0, "%.1f");
        if gui::is_item_hovered_allow_disabled() {
            let (w, h) = self.size.unwrap_or((1, 1));
            gui::set_tooltip(&format!(
                "How wide the picture is on the ground (metres; {:.1} m tall, its proportions kept: {:.3} m a pixel).",
                self.s.width * h as f32 / w.max(1) as f32,
                self.s.width / w.max(1) as f32
            ));
        }

        gui::set_next_item_width(-110.0);
        changed |= gui::drag_float("turn (deg)##tileref", &mut self.s.rotation, 0.25, -360.0, 360.0, "%.2f");
        if gui::is_item_hovered_allow_disabled() {
            gui::set_tooltip("The picture's turn, clockwise seen from above (north up).");
        }

        if gui::button("Fit to the tile##tileref") {
            self.fit_to_tile(e);
        }

        if gui::is_item_hovered_allow_disabled() {
            gui::set_tooltip("Lays the picture over the whole tile, inside it, unturned.");
        }

        gui::end_disabled();
        gui::same_line();
        if gui::button("Remove##tileref") {
            self.forget(e);
            self.s = ReferenceImage::default();
            self.save(e);
            return;
        }

        if gui::is_item_hovered() {
            gui::set_tooltip("Forgets this tile's reference image (the file is not touched).");
        }

        if changed {
            self.dirty = true;
        }

        if self.s.show == ReferenceShow::Screen {
            gui::text_colored(theme::warning(), "On the screen: it lines up only while the camera stays still.");
        }
    }
}

/// The screenshot folder: the settings', else `<projects folder>/screenshots`.
pub(crate) fn screenshot_folder(e: &Editor) -> PathBuf {
    e.tool_settings::<TileSettings>(SETTINGS_ID).screenshot_folder.unwrap_or_else(|| e.projects_dir().join("screenshots"))
}

/// Saves the viewport's last image as a PNG: at `path`, or named after the open tile and numbered in the screenshot folder. Where it went.
///
/// # Errors
///
/// The viewport has no image yet, or the file can't be written.
pub(crate) fn save_screenshot(e: &mut Editor, path: Option<&Path>) -> Result<PathBuf, String> {
    let (w, h, rgba) = e.read_viewport_rgba8().ok_or("the viewport has no image yet")?;
    let path = match path {
        Some(p) => p.to_path_buf(),
        None => {
            let dir = screenshot_folder(e);
            std::fs::create_dir_all(&dir).map_err(|err| format!("{}: {err}", dir.display()))?;
            // The tile's folder and its variation's (`<tile>/<variation>`).
            let name = |p: Option<&Path>| p.and_then(Path::file_name).map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
            let stem = document(e)
                .map(|d| {
                    let dir = &d.project().dir;
                    [name(dir.parent()), name(Some(dir))].into_iter().filter(|s| !s.is_empty()).collect::<Vec<_>>().join("_")
                })
                .filter(|s| !s.is_empty())
                .unwrap_or_else(|| "tile".to_owned());
            (1..).map(|n| dir.join(format!("{stem}_{n:03}.png"))).find(|p| !p.exists()).expect("a free name")
        }
    };

    let png = PngFile {
        width: w,
        height: h,
        bit_depth: 8,
        color_type: 2,
        pixels: rgba.as_chunks::<4>().0.iter().flat_map(|px| [px[0], px[1], px[2]]).collect(),
        ..Default::default()
    };
    let bytes = png.encode().map_err(|err| format!("{}: {err:?}", path.display()))?;
    std::fs::write(&path, bytes).map_err(|err| format!("{}: {err}", path.display()))?;
    Ok(path)
}
