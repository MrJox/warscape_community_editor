//! A CPU renderer for a layout: used by the CLI / MCP `render` verb so a coding agent can look at what it edited.
//! The editor's canvas paints the same draw list with Dear ImGui instead (real fonts, GPU textures).
//!
//! Pictures come from the caller (`load`), text is drawn with a small built-in 5x7 font (upper case only), so
//! the result shows where text sits and what it says, not how the game's fonts look.

use std::rc::Rc;

use crate::cuf::Cuf;
use crate::draw::{DrawItem, DrawKind, DrawOptions, pieces};
use crate::images::Rgba8Image;
use crate::model::UiModel;

pub struct Canvas {
    pub w: u32,
    pub h: u32,
    /// RGBA, rows top to bottom.
    pub px: Vec<u8>,
}

#[derive(Clone, Debug)]
pub struct RenderOptions {
    /// Layout pixels to canvas pixels.
    pub scale: f32,
    /// Layout position of the canvas's top left corner.
    pub origin: (f32, f32),
    pub width: u32,
    pub height: u32,
    pub background: [u8; 4],
    /// Draw a thin outline around every component.
    pub outlines: bool,
    /// Index into `UiModel::entries()` to highlight.
    pub selected: Option<usize>,
    /// What to draw (hidden components, templates, a state).
    pub draw: DrawOptions,
    /// Reference layers (see `related`), drawn before the layout (parents) and after it (children).
    pub below: Vec<DrawItem>,
    pub above: Vec<DrawItem>,
}

impl RenderOptions {
    /// Fit the whole layout (the root's size) at `scale`.
    pub fn fit(model: &UiModel, scale: f32) -> Self {
        let e = model.entries();
        let (w, h) = e.first().map(|r| (r.w.max(1), r.h.max(1))).unwrap_or((640, 480));
        Self { scale, origin: (0.0, 0.0), width: (w as f32 * scale).ceil() as u32, height: (h as f32 * scale).ceil() as u32, background: [24, 24, 28, 255], outlines: false, selected: None, draw: DrawOptions::default(), below: vec![], above: vec![] }
    }
}

const GLYPHS: &[(char, [u8; 7])] = &[
    ('A', [0b01110, 0b10001, 0b10001, 0b11111, 0b10001, 0b10001, 0b10001]),
    ('B', [0b11110, 0b10001, 0b10001, 0b11110, 0b10001, 0b10001, 0b11110]),
    ('C', [0b01110, 0b10001, 0b10000, 0b10000, 0b10000, 0b10001, 0b01110]),
    ('D', [0b11110, 0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b11110]),
    ('E', [0b11111, 0b10000, 0b10000, 0b11110, 0b10000, 0b10000, 0b11111]),
    ('F', [0b11111, 0b10000, 0b10000, 0b11110, 0b10000, 0b10000, 0b10000]),
    ('G', [0b01110, 0b10001, 0b10000, 0b10111, 0b10001, 0b10001, 0b01111]),
    ('H', [0b10001, 0b10001, 0b10001, 0b11111, 0b10001, 0b10001, 0b10001]),
    ('I', [0b01110, 0b00100, 0b00100, 0b00100, 0b00100, 0b00100, 0b01110]),
    ('J', [0b00111, 0b00010, 0b00010, 0b00010, 0b00010, 0b10010, 0b01100]),
    ('K', [0b10001, 0b10010, 0b10100, 0b11000, 0b10100, 0b10010, 0b10001]),
    ('L', [0b10000, 0b10000, 0b10000, 0b10000, 0b10000, 0b10000, 0b11111]),
    ('M', [0b10001, 0b11011, 0b10101, 0b10101, 0b10001, 0b10001, 0b10001]),
    ('N', [0b10001, 0b11001, 0b10101, 0b10011, 0b10001, 0b10001, 0b10001]),
    ('O', [0b01110, 0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b01110]),
    ('P', [0b11110, 0b10001, 0b10001, 0b11110, 0b10000, 0b10000, 0b10000]),
    ('Q', [0b01110, 0b10001, 0b10001, 0b10001, 0b10101, 0b10010, 0b01101]),
    ('R', [0b11110, 0b10001, 0b10001, 0b11110, 0b10100, 0b10010, 0b10001]),
    ('S', [0b01111, 0b10000, 0b10000, 0b01110, 0b00001, 0b00001, 0b11110]),
    ('T', [0b11111, 0b00100, 0b00100, 0b00100, 0b00100, 0b00100, 0b00100]),
    ('U', [0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b01110]),
    ('V', [0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b01010, 0b00100]),
    ('W', [0b10001, 0b10001, 0b10001, 0b10101, 0b10101, 0b11011, 0b10001]),
    ('X', [0b10001, 0b10001, 0b01010, 0b00100, 0b01010, 0b10001, 0b10001]),
    ('Y', [0b10001, 0b10001, 0b01010, 0b00100, 0b00100, 0b00100, 0b00100]),
    ('Z', [0b11111, 0b00001, 0b00010, 0b00100, 0b01000, 0b10000, 0b11111]),
    ('0', [0b01110, 0b10001, 0b10011, 0b10101, 0b11001, 0b10001, 0b01110]),
    ('1', [0b00100, 0b01100, 0b00100, 0b00100, 0b00100, 0b00100, 0b01110]),
    ('2', [0b01110, 0b10001, 0b00001, 0b00010, 0b00100, 0b01000, 0b11111]),
    ('3', [0b11110, 0b00001, 0b00001, 0b01110, 0b00001, 0b00001, 0b11110]),
    ('4', [0b00010, 0b00110, 0b01010, 0b10010, 0b11111, 0b00010, 0b00010]),
    ('5', [0b11111, 0b10000, 0b11110, 0b00001, 0b00001, 0b10001, 0b01110]),
    ('6', [0b00110, 0b01000, 0b10000, 0b11110, 0b10001, 0b10001, 0b01110]),
    ('7', [0b11111, 0b00001, 0b00010, 0b00100, 0b01000, 0b01000, 0b01000]),
    ('8', [0b01110, 0b10001, 0b10001, 0b01110, 0b10001, 0b10001, 0b01110]),
    ('9', [0b01110, 0b10001, 0b10001, 0b01111, 0b00001, 0b00010, 0b01100]),
    ('.', [0, 0, 0, 0, 0, 0b01100, 0b01100]),
    (',', [0, 0, 0, 0, 0b01100, 0b00100, 0b01000]),
    (':', [0, 0b01100, 0b01100, 0, 0b01100, 0b01100, 0]),
    ('-', [0, 0, 0, 0b11111, 0, 0, 0]),
    ('_', [0, 0, 0, 0, 0, 0, 0b11111]),
    ('/', [0b00001, 0b00001, 0b00010, 0b00100, 0b01000, 0b10000, 0b10000]),
    ('(', [0b00010, 0b00100, 0b01000, 0b01000, 0b01000, 0b00100, 0b00010]),
    (')', [0b01000, 0b00100, 0b00010, 0b00010, 0b00010, 0b00100, 0b01000]),
    ('%', [0b11001, 0b11010, 0b00010, 0b00100, 0b01000, 0b01011, 0b10011]),
    ('!', [0b00100, 0b00100, 0b00100, 0b00100, 0b00100, 0, 0b00100]),
    ('?', [0b01110, 0b10001, 0b00001, 0b00010, 0b00100, 0, 0b00100]),
    ('+', [0, 0b00100, 0b00100, 0b11111, 0b00100, 0b00100, 0]),
    ('\'', [0b00100, 0b00100, 0b01000, 0, 0, 0, 0]),
];

fn glyph(c: char) -> [u8; 7] {
    let u = c.to_ascii_uppercase();
    GLYPHS.iter().find(|(g, _)| *g == u).map(|(_, b)| *b).unwrap_or([0b11111, 0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b11111])
}

impl Canvas {
    pub fn new(w: u32, h: u32, bg: [u8; 4]) -> Self {
        let mut px = Vec::with_capacity((w * h * 4) as usize);
        for _ in 0..w * h {
            px.extend_from_slice(&bg);
        }
        Self { w, h, px }
    }

    fn blend(&mut self, x: i32, y: i32, c: [u8; 4]) {
        if x < 0 || y < 0 || x >= self.w as i32 || y >= self.h as i32 || c[3] == 0 {
            return;
        }
        let i = ((y as u32 * self.w + x as u32) * 4) as usize;
        let a = c[3] as u32;
        for k in 0..3 {
            self.px[i + k] = ((c[k] as u32 * a + self.px[i + k] as u32 * (255 - a)) / 255) as u8;
        }
        self.px[i + 3] = 255;
    }

    fn rect_outline(&mut self, x0: i32, y0: i32, x1: i32, y1: i32, c: [u8; 4]) {
        for x in x0..=x1 {
            self.blend(x, y0, c);
            self.blend(x, y1, c);
        }
        for y in y0..=y1 {
            self.blend(x0, y, c);
            self.blend(x1, y, c);
        }
    }

    /// PNG bytes of the canvas.
    ///
    /// # Errors
    ///
    /// If the PNG encoder fails.
    pub fn to_png(&self) -> Result<Vec<u8>, String> {
        use image::ImageEncoder;
        let mut out = Vec::new();
        image::codecs::png::PngEncoder::new(&mut out).write_image(&self.px, self.w, self.h, image::ExtendedColorType::Rgba8).map_err(|e| e.to_string())?;
        Ok(out)
    }
}

fn sample(img: &Rgba8Image, u: f32, v: f32) -> [u8; 4] {
    // bilinear, u and v in pixels of the source
    let x = (u - 0.5).clamp(0.0, img.w as f32 - 1.0);
    let y = (v - 0.5).clamp(0.0, img.h as f32 - 1.0);
    let (x0, y0) = (x.floor() as u32, y.floor() as u32);
    let (x1, y1) = ((x0 + 1).min(img.w - 1), (y0 + 1).min(img.h - 1));
    let (fx, fy) = (x - x0 as f32, y - y0 as f32);
    let mut out = [0u8; 4];
    for k in 0..4 {
        let a = img.at(x0, y0)[k] as f32 * (1.0 - fx) + img.at(x1, y0)[k] as f32 * fx;
        let b = img.at(x0, y1)[k] as f32 * (1.0 - fx) + img.at(x1, y1)[k] as f32 * fx;
        out[k] = (a * (1.0 - fy) + b * fy).round() as u8;
    }
    out
}

fn draw_image(c: &mut Canvas, o: &RenderOptions, it: &DrawItem, img: &Rgba8Image, tiled: bool, flip: (bool, bool), tint: [u8; 4], margins: [f32; 4]) {
    if img.w == 0 || img.h == 0 {
        return;
    }

    let s = o.scale;
    for p in pieces(it.x, it.y, it.w, it.h, img.w as f32, img.h as f32, margins, tiled) {
        let (dx0, dy0) = ((p.x - o.origin.0) * s, (p.y - o.origin.1) * s);
        let (dx1, dy1) = (dx0 + p.w * s, dy0 + p.h * s);
        let (px0, py0) = (dx0.floor().max(0.0) as i32, dy0.floor().max(0.0) as i32);
        let (px1, py1) = ((dx1.ceil() as i32).min(c.w as i32), (dy1.ceil() as i32).min(c.h as i32));
        for py in py0..py1 {
            for px in px0..px1 {
                let fx = ((px as f32 + 0.5 - dx0) / (dx1 - dx0)).clamp(0.0, 1.0);
                let fy = ((py as f32 + 0.5 - dy0) / (dy1 - dy0)).clamp(0.0, 1.0);
                let (mut fu, mut fv) = (p.u0 + (p.u1 - p.u0) * fx, p.v0 + (p.v1 - p.v0) * fy);
                if flip.0 {
                    fu = 1.0 - fu;
                }

                if flip.1 {
                    fv = 1.0 - fv;
                }

                let t = sample(img, fu * img.w as f32, fv * img.h as f32);
                let col = [(t[0] as u32 * tint[0] as u32 / 255) as u8, (t[1] as u32 * tint[1] as u32 / 255) as u8, (t[2] as u32 * tint[2] as u32 / 255) as u8, (t[3] as u32 * tint[3] as u32 / 255) as u8];
                c.blend(px, py, col);
            }
        }
    }
}

/// Text with the game's bitmap font: glyph alpha times the colour, at the canvas scale.
#[allow(clippy::too_many_arguments)]
fn draw_game_text(c: &mut Canvas, o: &RenderOptions, it: &DrawItem, cuf: &Cuf, text: &str, color: [u8; 4], halign: i32, valign: i32, inset: [i32; 4]) {
    let (bw, bh) = (it.w as i32 - inset[0] - inset[2], it.h as i32 - inset[1] - inset[3]);
    let s = o.scale;
    for p in cuf.layout(text, bw, bh, halign, valign) {
        let m = cuf.metric(p.glyph);
        let bmp = cuf.bitmap(p.glyph);
        let (gx, gy) = (it.x + inset[0] as f32 + p.x as f32, it.y + inset[1] as f32 + p.y as f32);
        let (dx0, dy0) = ((gx - o.origin.0) * s, (gy - o.origin.1) * s);
        let (dx1, dy1) = (dx0 + m.width as f32 * s, dy0 + m.height as f32 * s);
        for py in (dy0.floor().max(0.0) as i32)..(dy1.ceil() as i32).min(c.h as i32) {
            for px in (dx0.floor().max(0.0) as i32)..(dx1.ceil() as i32).min(c.w as i32) {
                let u = ((px as f32 + 0.5 - dx0) / s).clamp(0.0, m.width as f32 - 1.0);
                let v = ((py as f32 + 0.5 - dy0) / s).clamp(0.0, m.height as f32 - 1.0);
                let a = bmp[(v as usize) * m.width as usize + u as usize] as u32 * color[3] as u32 / 255;
                c.blend(px, py, [color[0], color[1], color[2], a as u8]);
            }
        }
    }
}

fn draw_text(c: &mut Canvas, o: &RenderOptions, it: &DrawItem, text: &str, color: [u8; 4], size: f32, halign: i32, valign: i32) {
    let s = o.scale;
    let cell = (size * s / 9.0).max(1.0).floor().max(1.0) as i32; // glyph pixel size
    let lines: Vec<&str> = text.split(['\n', '\r']).filter(|l| !l.is_empty()).collect();
    let line_h = 8 * cell;
    let total_h = lines.len() as i32 * line_h;
    let bx = ((it.x - o.origin.0) * s) as i32;
    let by = ((it.y - o.origin.1) * s) as i32;
    let bw = (it.w * s) as i32;
    let bh = (it.h * s) as i32;
    let mut y = match valign {
        1 => by + (bh - total_h) / 2,
        2 => by + bh - total_h,
        _ => by,
    };
    for line in lines {
        let lw = line.chars().count() as i32 * 6 * cell;
        let mut x = match halign {
            1 => bx + (bw - lw) / 2,
            2 => bx + bw - lw,
            _ => bx,
        };
        for ch in line.chars() {
            if ch != ' ' {
                let g = glyph(ch);
                for (r, row) in g.iter().enumerate() {
                    for col in 0..5 {
                        if row & (1 << (4 - col)) != 0 {
                            for dy in 0..cell {
                                for dx in 0..cell {
                                    c.blend(x + col * cell + dx, y + r as i32 * cell + dy, color);
                                }
                            }
                        }
                    }
                }
            }
            x += 6 * cell;
        }
        y += line_h;
    }
}

/// Paints the layout. `load` returns the picture for a normalised path (`ui/skins/default/x.png`), or None.
/// A font for text: the state's font name, its size and its font category -> the font and the colour the category gives.
pub type FontResolver<'a> = &'a mut dyn FnMut(&str, i32, &str) -> Option<(Rc<Cuf>, Option<[u8; 4]>)>;

pub fn render(model: &UiModel, o: &RenderOptions, load: &mut dyn FnMut(&str) -> Option<Rc<Rgba8Image>>, fonts: FontResolver) -> Canvas {
    let mut c = Canvas::new(o.width, o.height, o.background);
    let main = model.draw_list_with(&o.draw);
    for it in o.below.iter().chain(main.iter()).chain(o.above.iter()).cloned() {
        match &it.kind {
            DrawKind::Image { path, tiled, flip_x, flip_y, tint, margins } => {
                if let Some(img) = load(path) {
                    draw_image(&mut c, o, &it, &img, *tiled, (*flip_x, *flip_y), *tint, *margins);
                } else {
                    let s = o.scale;
                    let (x0, y0) = (((it.x - o.origin.0) * s) as i32, ((it.y - o.origin.1) * s) as i32);
                    c.rect_outline(x0, y0, x0 + (it.w * s) as i32, y0 + (it.h * s) as i32, [200, 60, 200, 160]);
                }
            }
            DrawKind::Text { text, color, size, halign, valign, font, category, inset } => match fonts(font, *size as i32, category) {
                Some((cuf, cat_color)) => draw_game_text(&mut c, o, &it, &cuf, text, cat_color.unwrap_or(*color), *halign, *valign, *inset),
                None => draw_text(&mut c, o, &it, text, *color, *size, *halign, *valign),
            },
        }
    }
    if o.outlines || o.selected.is_some() {
        let ents = model.entries();
        let shown = model.shown(&ents, &o.draw);
        let rects = model.geometry(&ents, &o.draw);
        for (i, r) in rects.iter().enumerate() {
            let sel = o.selected == Some(i);
            if !(o.outlines || sel) || r.w <= 0.0 || r.h <= 0.0 || !shown[i] {
                continue;
            }
            let s = o.scale;
            let x0 = ((r.x - o.origin.0) * s) as i32;
            let y0 = ((r.y - o.origin.1) * s) as i32;
            let col = if sel { [255, 200, 0, 255] } else { [80, 160, 255, 70] };
            c.rect_outline(x0, y0, x0 + (r.w * s) as i32, y0 + (r.h * s) as i32, col);
        }
    }
    c
}
