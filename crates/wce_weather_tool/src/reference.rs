//! A reference screenshot for matching a look (docs/editor.md, "Weather tool"): a PNG (a game screenshot) shown in a corner of the
//! viewport or over it at an opacity, fitted to the viewport keeping its proportions; and a colour comparison: a click picks the same
//! spot of the screenshot and of the render and says how they differ (brightness, warmth). It assumes the two are framed alike (the
//! camera put where the screenshot was taken): the editor cannot know where that was. The files' colours are inputs that are lit and
//! tone-mapped, so a picked colour is not written into the file: it guides the edit.

use std::path::{Path, PathBuf};

use wce_editor::editor::Editor;
use wce_editor::steam_detect::file_dialog;
use wce_editor::{gui, theme};
use wce_formats::png_file::PngFile;

const KEY: &str = "weather_reference";

/// How the screenshot is shown.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum Show {
    #[default]
    Off,
    Corner,
    Over,
}

/// One comparison: where (a share of the viewport's width and height), the screenshot's colour and the render's (RGB8).
#[derive(Clone, Copy, Debug)]
pub(crate) struct Pick {
    pub at: (f32, f32),
    pub reference: [u8; 3],
    pub render: [u8; 3],
}

#[derive(Default)]
pub(crate) struct Reference {
    pub(crate) path: String,
    image: Option<(u32, u32, Vec<u8>)>,
    made: bool,
    pub(crate) show: Show,
    pub(crate) opacity: f32,
    pub(crate) picking: bool,
    pub(crate) pick: Option<Pick>,
}

fn luma(c: [u8; 3]) -> f32 {
    0.2126 * f32::from(c[0]) + 0.7152 * f32::from(c[1]) + 0.0722 * f32::from(c[2])
}

/// The comparison in words: how much brighter or darker, warmer or cooler the render is than the screenshot there.
pub(crate) fn describe(p: &Pick) -> String {
    let (a, b) = (luma(p.reference), luma(p.render));
    let bright = if a < 1.0 {
        "the screenshot is black there".to_owned()
    } else {
        format!("the render is {:+.0} % as bright", (b / a - 1.0) * 100.0)
    };
    let warmth = |c: [u8; 3]| (f32::from(c[0]) + 1.0) / (f32::from(c[2]) + 1.0);
    let (wa, wb) = (warmth(p.reference), warmth(p.render));
    let warm = if (wb / wa - 1.0).abs() < 0.05 {
        "as warm".to_owned()
    } else if wb > wa {
        format!("warmer (red / blue {wb:.2} against {wa:.2})")
    } else {
        format!("cooler (red / blue {wb:.2} against {wa:.2})")
    };
    format!(
        "screenshot rgb({}, {}, {}), render rgb({}, {}, {}): {bright}, {warm}",
        p.reference[0], p.reference[1], p.reference[2], p.render[0], p.render[1], p.render[2]
    )
}

impl Reference {
    /// Reads a PNG (8-bit RGB or RGBA) as the reference.
    ///
    /// # Errors
    ///
    /// The file can't be read or is not such a PNG.
    pub(crate) fn load(&mut self, e: &mut Editor, path: &Path) -> Result<(), String> {
        let bytes = std::fs::read(path).map_err(|err| format!("{}: {err}", path.display()))?;
        let png = PngFile::parse(&bytes).map_err(|err| format!("{}: {err:?}", path.display()))?;
        let rgba = match (png.bit_depth, png.color_type) {
            (8, 6) => png.pixels,
            (8, 2) => png.pixels.chunks_exact(3).flat_map(|c| [c[0], c[1], c[2], 255]).collect(),
            (d, t) => return Err(format!("{}: a PNG of 8-bit RGB or RGBA is needed (this one: {d}-bit, colour type {t})", path.display())),
        };

        if self.made {
            e.images.forget(KEY);
        }

        e.images.submit(KEY, png.width, png.height, rgba.clone());
        self.made = true;
        self.image = Some((png.width, png.height, rgba));
        self.path = path.display().to_string();
        self.pick = None;
        if self.show == Show::Off {
            self.show = Show::Over;
        }

        if self.opacity <= 0.0 {
            self.opacity = 0.5;
        }

        Ok(())
    }

    /// The screenshot fitted into a rectangle keeping its proportions: its corners.
    fn fit(&self, lo: gui::Vec2, size: gui::Vec2) -> Option<(gui::Vec2, gui::Vec2)> {
        let (w, h, _) = self.image.as_ref()?;
        let k = (size.x / *w as f32).min(size.y / *h as f32);
        let (fw, fh) = (*w as f32 * k, *h as f32 * k);
        let o = gui::v2(lo.x + (size.x - fw) * 0.5, lo.y + (size.y - fh) * 0.5);
        Some((o, gui::v2(o.x + fw, o.y + fh)))
    }

    /// Compares the screenshot and the render at a pixel of the viewport image (the screenshot fitted to the whole viewport).
    ///
    /// # Errors
    ///
    /// No screenshot, or the spot is outside it.
    pub(crate) fn compare_at(&mut self, e: &Editor, x: i32, y: i32) -> Result<Pick, String> {
        let (w, h, rgba) = self.image.as_ref().ok_or("no reference screenshot loaded")?;
        let (vw, vh, view) = e.read_viewport_rgba8().ok_or("the viewport has no image yet")?;
        let (fx, fy) = (x as f32 / vw as f32, y as f32 / vh as f32);
        let (lo, hi) = self.fit(gui::v2(0.0, 0.0), gui::v2(vw as f32, vh as f32)).ok_or("no reference screenshot loaded")?;
        let (u, v) = ((x as f32 - lo.x) / (hi.x - lo.x), (y as f32 - lo.y) / (hi.y - lo.y));
        if !(0.0..1.0).contains(&u) || !(0.0..1.0).contains(&v) {
            return Err("that spot is outside the screenshot (it keeps its proportions)".to_owned());
        }

        let at = |img: &[u8], iw: u32, px: u32, py: u32| {
            let i = (py as usize * iw as usize + px as usize) * 4;
            img.get(i..i + 3).map(|c| [c[0], c[1], c[2]])
        };
        let reference =
            at(rgba, *w, ((u * *w as f32) as u32).min(w - 1), ((v * *h as f32) as u32).min(h - 1)).ok_or("outside the screenshot")?;
        let render = at(&view, vw, (x.max(0) as u32).min(vw - 1), (y.max(0) as u32).min(vh - 1)).ok_or("outside the viewport")?;
        let pick = Pick { at: (fx, fy), reference, render };
        self.pick = Some(pick);
        Ok(pick)
    }

    /// Over the viewport: the screenshot in a corner or over it, and the last comparison's spot.
    pub(crate) fn draw_overlay(&self, e: &Editor, origin: gui::Vec2, size: gui::Vec2) {
        let Some(img) = e.images.get(KEY) else { return };
        let dl = gui::window_draw_list();
        let white = |a: f32| gui::col32(255, 255, 255, (a.clamp(0.0, 1.0) * 255.0) as u8);
        match self.show {
            Show::Off => {}
            Show::Over => {
                if let Some((lo, hi)) = self.fit(origin, size) {
                    dl.image(img.id, lo, hi, gui::v2(0.0, 0.0), gui::v2(1.0, 1.0), white(self.opacity));
                }
            }

            Show::Corner => {
                let box_size = gui::v2(size.x * 0.33, size.y * 0.33);
                let corner = gui::v2(origin.x + size.x - box_size.x - 10.0, origin.y + size.y - box_size.y - 10.0);
                if let Some((lo, hi)) = self.fit(corner, box_size) {
                    dl.rect_filled(gui::v2(lo.x - 3.0, lo.y - 3.0), gui::v2(hi.x + 3.0, hi.y + 3.0), gui::col32(10, 10, 12, 220), 3.0);
                    dl.image(img.id, lo, hi, gui::v2(0.0, 0.0), gui::v2(1.0, 1.0), white(1.0));
                }
            }
        }

        if let Some(p) = self.pick {
            let at = gui::v2(origin.x + p.at.0 * size.x, origin.y + p.at.1 * size.y);
            dl.ellipse(at, gui::v2(7.0, 7.0), gui::col32(10, 10, 12, 230), 16, 3.0);
            dl.ellipse(at, gui::v2(7.0, 7.0), gui::col32(255, 210, 80, 255), 16, 1.5);
        }

        if self.picking {
            let text = "Click a spot to compare the screenshot and the render there (Esc: cancel)";
            let w = gui::calc_text_width(text);
            let top = gui::v2(origin.x + size.x * 0.5 - w * 0.5, origin.y + 64.0);
            dl.rect_filled(gui::v2(top.x - 8.0, top.y - 4.0), gui::v2(top.x + w + 8.0, top.y + 18.0), gui::col32(10, 10, 12, 200), 4.0);
            dl.text(top, theme::u32(theme::modified()), text);
        }
    }

    /// The Weather scene panel's section.
    pub(crate) fn draw_panel(&mut self, e: &mut Editor) {
        if !gui::collapsing_header("Reference screenshot", 0) {
            return;
        }

        gui::set_next_item_width(-90.0);
        let enter = gui::input_text("##refpath", Some("a game screenshot (.png)"), &mut self.path, 512, gui::INPUT_ENTER_RETURNS_TRUE);
        if gui::is_item_hovered() {
            gui::set_tooltip("A PNG to match the look against: a game screenshot taken from where the viewport's camera is.");
        }

        gui::same_line();
        let mut pick_file = None;
        if gui::button("Load...") {
            let start = Path::new(&self.path).parent().map(Path::to_path_buf).unwrap_or_default();
            pick_file = file_dialog(false, &start, &[("PNG".to_owned(), "*.png".to_owned())]);
        }

        if gui::is_item_hovered() {
            gui::set_tooltip("Pick the screenshot.");
        }

        if enter && !self.path.trim().is_empty() {
            pick_file = Some(PathBuf::from(self.path.trim()));
        }

        if let Some(p) = pick_file
            && let Err(m) = self.load(e, &p)
        {
            e.status(&m, true);
        }

        if self.image.is_none() {
            return;
        }

        for (show, name, tip) in [
            (Show::Off, "hidden", "The screenshot is not shown (picks still compare against it)."),
            (Show::Corner, "corner", "The screenshot in the viewport's bottom right corner."),
            (Show::Over, "over", "The screenshot over the viewport, at the opacity beside."),
        ] {
            if gui::radio_button(&format!("{name}##refshow"), self.show == show) {
                self.show = show;
            }

            if gui::is_item_hovered() {
                gui::set_tooltip(tip);
            }

            gui::same_line();
        }

        gui::set_next_item_width(140.0);
        gui::begin_disabled(self.show != Show::Over);
        gui::slider_float("##refopacity", &mut self.opacity, 0.0, 1.0, "opacity %.2f");
        gui::end_disabled();
        if gui::is_item_hovered_allow_disabled() {
            gui::set_tooltip("How opaque the screenshot is over the viewport: slide it to look from one to the other.");
        }

        gui::same_line();
        if gui::button("Compare a spot") {
            self.picking = true;
        }

        if gui::is_item_hovered() {
            gui::set_tooltip(
                "Then click the viewport: the screenshot's colour and the render's there, and how they differ. The file's colours are lit \
                 and tone-mapped, so the picked colour is a guide for the edit, not written into the file.",
            );
        }

        if let Some(p) = &self.pick {
            let sw = |c: [u8; 3]| [f32::from(c[0]) / 255.0, f32::from(c[1]) / 255.0, f32::from(c[2]) / 255.0, 1.0];
            for (c, name) in [(p.reference, "screenshot"), (p.render, "render")] {
                let lo = gui::cursor_screen_pos();
                gui::window_draw_list().rect_filled(lo, gui::v2(lo.x + 18.0, lo.y + 18.0), theme::u32(sw(c)), 2.0);
                gui::dummy(gui::v2(18.0, 18.0));
                gui::same_line();
                gui::text_disabled(name);
                gui::same_line();
            }

            gui::new_line();
            gui::text_wrapped(&describe(p));
        }
    }
}
