//! The game's bitmap fonts (`font/<name>_<size>.cuf`) and its text layout, ported from the decompiled `WARSCAPE::SPRITE_FONT`
//! (constructor, `render`, `render_kerned_into_image`).
//!
//! File: `CUF0` (Latin fonts) or `CUF1` (Arial Unicode, the same layout with tens of thousands of glyphs), a u16 (skipped), ten i16 (font height, line spacing, underline height, underline thickness, ascender, descender,
//! space width, character spacing, bounding width, bounding height), u16 glyph count, u32 size of the pixel data, then `u16[65536]`
//! character -> glyph (0xFFFF none), `glyph count` metrics (i8 top, i8 advance, u8 width, u8 height), `u32` offsets of each glyph's pixels,
//! the pixels (8 bit alpha, `width * height` per glyph, rows top to bottom), and the kerning table (u16 count, u16 first character,
//! `count * count` i8: the pen advance after the first character when the second follows).

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Metric {
    /// Rows from the bitmap's top edge down to the line's baseline (a glyph is drawn `top` above it); -128: no bitmap.
    pub top: i8,
    pub advance: i8,
    pub width: u8,
    pub height: u8,
}

#[derive(Debug, Clone)]
pub struct Cuf {
    pub font_height: i16,
    pub line_spacing: i16,
    pub ascender: i16,
    pub descender: i16,
    pub space_width: i16,
    pub character_spacing: i16,
    pub bounding_width: i16,
    pub bounding_height: i16,
    index: Vec<u16>,
    metrics: Vec<Metric>,
    first_pixel: Vec<u32>,
    pixels: Vec<u8>,
    kerning_count: usize,
    kerning_first: usize,
    kerning: Vec<i8>,
}

/// A glyph placed on a line: top left corner of its bitmap, relative to the text origin.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Placed {
    pub x: i32,
    pub y: i32,
    pub glyph: usize,
}

struct Reader<'a> {
    d: &'a [u8],
    p: usize,
}

impl Reader<'_> {
    fn take(&mut self, n: usize) -> Result<&[u8], String> {
        if self.p + n > self.d.len() {
            return Err("font file is truncated".into());
        }

        let r = &self.d[self.p..self.p + n];
        self.p += n;
        Ok(r)
    }

    fn i16(&mut self) -> Result<i16, String> {
        Ok(i16::from_le_bytes(self.take(2)?.try_into().unwrap()))
    }

    fn u16(&mut self) -> Result<u16, String> {
        Ok(u16::from_le_bytes(self.take(2)?.try_into().unwrap()))
    }

    fn u32(&mut self) -> Result<u32, String> {
        Ok(u32::from_le_bytes(self.take(4)?.try_into().unwrap()))
    }
}

impl Cuf {
    /// # Errors
    ///
    /// Why the bytes are not a font this reads.
    pub fn parse(data: &[u8]) -> Result<Self, String> {
        let mut r = Reader { d: data, p: 0 };
        if !r.take(4)?.starts_with(b"CUF") {
            return Err("not a CUF font".into());
        }

        r.u16()?;
        let font_height = r.i16()?;
        let line_spacing = r.i16()?;
        r.i16()?;
        r.i16()?;
        let ascender = r.i16()?;
        let descender = r.i16()?;
        let space_width = r.i16()?;
        let character_spacing = r.i16()?;
        let bounding_width = r.i16()?;
        let bounding_height = r.i16()?;
        let count = r.u16()? as usize;
        let pixel_bytes = r.u32()? as usize;
        let index: Vec<u16> = r.take(0x20000)?.chunks_exact(2).map(|c| u16::from_le_bytes([c[0], c[1]])).collect();
        let metrics: Vec<Metric> = r.take(4 * count)?.chunks_exact(4).map(|c| Metric { top: c[0] as i8, advance: c[1] as i8, width: c[2], height: c[3] }).collect();
        let first_pixel: Vec<u32> = r.take(4 * count)?.chunks_exact(4).map(|c| u32::from_le_bytes([c[0], c[1], c[2], c[3]])).collect();
        let pixels = r.take(pixel_bytes)?.to_vec();
        let kerning_count = r.u16()? as usize;
        let kerning_first = r.u16()? as usize;
        let kerning: Vec<i8> = r.take(kerning_count * kerning_count)?.iter().map(|b| *b as i8).collect();
        Ok(Self { font_height, line_spacing, ascender, descender, space_width, character_spacing, bounding_width, bounding_height, index, metrics, first_pixel, pixels, kerning_count, kerning_first, kerning })
    }

    pub fn glyph_count(&self) -> usize {
        self.metrics.len()
    }

    /// Height of a line: the larger of the font height and the line spacing.
    pub fn line_height(&self) -> i32 {
        i32::from(self.font_height.max(self.line_spacing))
    }

    pub fn glyph_of(&self, c: char) -> Option<usize> {
        let cp = c as u32;
        if cp > 0xffff {
            return None;
        }

        let g = self.index[cp as usize] as usize;
        (g != 0xffff && g < self.metrics.len()).then_some(g)
    }

    pub fn metric(&self, glyph: usize) -> Metric {
        self.metrics[glyph]
    }

    /// The alpha bitmap of a glyph, `width * height` bytes (empty for a glyph without one).
    pub fn bitmap(&self, glyph: usize) -> &[u8] {
        let m = self.metrics[glyph];
        let start = self.first_pixel[glyph] as usize;
        let n = m.width as usize * m.height as usize;
        if m.top == -128 || start + n > self.pixels.len() { &[] } else { &self.pixels[start..start + n] }
    }

    fn kerned(&self, a: char, b: char) -> Option<i32> {
        let (a, b) = (a as usize, b as usize);
        let (f, n) = (self.kerning_first, self.kerning_count);
        (a >= f && b >= f && a < f + n && b < f + n).then(|| i32::from(self.kerning[(b - f) + (a - f) * n]))
    }

    /// How far the pen moves after `c` when `next` follows (the game's rules: kerning table inside its range, else the glyph's advance for a
    /// space and `width + 1` for any other glyph; a tab goes to the next multiple of 64).
    fn advance(&self, c: char, next: char, pen: i32) -> i32 {
        match c {
            '\t' => ((pen + 64) & !63) - pen,
            ' ' | '\u{a0}' => self.glyph_of(c).map(|g| i32::from(self.metrics[g].advance)).unwrap_or(i32::from(self.space_width)),
            c if (c as u32) < 32 => 0,
            c => {
                let drawn = self.glyph_of(c).is_some_and(|g| self.metrics[g].top != -128);
                if drawn {
                    if let Some(k) = self.kerned(c, next) {
                        return k;
                    }
                }

                match self.glyph_of(c) {
                    Some(g) => i32::from(self.metrics[g].width) + 1,
                    None => 0,
                }
            }
        }
    }

    /// Width in pixels of one line of text.
    pub fn line_width(&self, line: &str) -> i32 {
        let cs: Vec<char> = line.chars().collect();
        let mut pen = 0;
        for (i, c) in cs.iter().enumerate() {
            pen += self.advance(*c, cs.get(i + 1).copied().unwrap_or('\0'), pen);
        }

        pen
    }

    /// Splits text into lines of at most `width` pixels, breaking after spaces (and at line breaks); a word longer than a line is cut.
    pub fn wrap(&self, text: &str, width: i32) -> Vec<String> {
        let mut lines = Vec::new();
        for para in text.split(['\n', '\u{2028}']) {
            let para = para.trim_end_matches('\r');
            let mut cur = String::new();
            for word in para.split_inclusive(' ') {
                let trial = format!("{cur}{word}");
                if !cur.is_empty() && width > 0 && self.line_width(trial.trim_end()) > width {
                    lines.push(cur.trim_end().to_string());
                    cur = word.to_string();
                } else {
                    cur = trial;
                }
            }

            lines.push(cur.trim_end().to_string());
        }

        lines
    }

    /// Places the glyphs of one line starting at `x`, with the line's top edge at `line_top`.
    pub fn place_line(&self, line: &str, x: i32, line_top: i32, out: &mut Vec<Placed>) {
        let cs: Vec<char> = line.chars().collect();
        let mut pen = 0;
        for (i, c) in cs.iter().enumerate() {
            if let Some(g) = self.glyph_of(*c) {
                let m = self.metrics[g];
                if m.top != -128 && m.width > 0 && m.height > 0 {
                    out.push(Placed { x: x + pen, y: line_top + self.line_height() - i32::from(m.top), glyph: g });
                }
            }

            pen += self.advance(*c, cs.get(i + 1).copied().unwrap_or('\0'), pen);
        }
    }

    /// Lays text out in a box: wrapped to the width, aligned (`halign` 0 left, 1 centre, 2 right; `valign` 0 top, 1 middle, 2 bottom).
    /// Positions are relative to the box's top left corner.
    pub fn layout(&self, text: &str, w: i32, h: i32, halign: i32, valign: i32) -> Vec<Placed> {
        let lines = self.wrap(text, w);
        let lh = self.line_height();
        let total = lh * lines.len() as i32;
        let mut y = match valign {
            1 => (h - total) / 2,
            2 => h - total,
            _ => 0,
        };
        let mut out = Vec::new();
        for l in &lines {
            let lw = self.line_width(l);
            let x = match halign {
                1 => (w - lw) / 2,
                2 => w - lw,
                _ => 0,
            };
            self.place_line(l, x, y, &mut out);
            y += lh;
        }

        out
    }
}

/// All glyph bitmaps of a font in one RGBA picture (white, the glyph's alpha), for drawing text as textured quads.
pub struct Atlas {
    pub w: u32,
    pub h: u32,
    /// Top left of each glyph's bitmap in the picture (a one pixel border surrounds every glyph).
    pub origin: Vec<(u32, u32)>,
    pub rgba: Vec<u8>,
}

impl Cuf {
    /// The glyph positions of `atlas()` without building the picture.
    pub fn atlas_origins(&self) -> Vec<(u32, u32)> {
        const W: u32 = 1024;
        let (mut x, mut y, mut row_h) = (1u32, 1u32, 0u32);
        let mut origin = Vec::with_capacity(self.metrics.len());
        for m in &self.metrics {
            let (gw, gh) = (u32::from(m.width), u32::from(m.height));
            if x + gw + 1 > W {
                x = 1;
                y += row_h + 1;
                row_h = 0;
            }

            origin.push((x, y));
            x += gw + 1;
            row_h = row_h.max(gh);
        }

        origin
    }

    pub fn atlas(&self) -> Atlas {
        const W: u32 = 1024;
        let (mut x, mut y, mut row_h) = (1u32, 1u32, 0u32);
        let mut origin = Vec::with_capacity(self.metrics.len());
        for m in &self.metrics {
            let (gw, gh) = (u32::from(m.width), u32::from(m.height));
            if x + gw + 1 > W {
                x = 1;
                y += row_h + 1;
                row_h = 0;
            }

            origin.push((x, y));
            x += gw + 1;
            row_h = row_h.max(gh);
        }

        let h = (y + row_h + 1).next_power_of_two().max(16);
        let mut rgba = vec![255u8; (W * h * 4) as usize];
        for px in rgba.chunks_exact_mut(4) {
            px[3] = 0;
        }

        for (g, m) in self.metrics.iter().enumerate() {
            let bmp = self.bitmap(g);
            let (ox, oy) = origin[g];
            for row in 0..m.height as u32 {
                for col in 0..m.width as u32 {
                    if let Some(a) = bmp.get((row * m.width as u32 + col) as usize) {
                        rgba[(((oy + row) * W + ox + col) * 4 + 3) as usize] = *a;
                    }
                }
            }
        }

        Atlas { w: W, h, origin, rgba }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A tiny font: 'A' (3x2, top 2) and space.
    fn tiny() -> Cuf {
        let mut idx = vec![0xffffu16; 65536];
        idx[b'A' as usize] = 0;
        idx[b' ' as usize] = 1;
        Cuf {
            font_height: 8,
            line_spacing: 6,
            ascender: 6,
            descender: 2,
            space_width: 3,
            character_spacing: 1,
            bounding_width: 4,
            bounding_height: 4,
            index: idx,
            metrics: vec![Metric { top: 2, advance: 4, width: 3, height: 2 }, Metric { top: -128, advance: 3, width: 0, height: 0 }],
            first_pixel: vec![0, 0],
            pixels: vec![255; 6],
            kerning_count: 0,
            kerning_first: 0,
            kerning: vec![],
        }
    }

    #[test]
    fn advances_and_places() {
        let f = tiny();
        // two As: width + 1 each (no kerning range), a space is its own advance
        assert_eq!(f.line_width("AA"), 8);
        assert_eq!(f.line_width("A A"), 4 + 3 + 4);
        let mut v = vec![];
        f.place_line("AA", 10, 20, &mut v);
        assert_eq!(v.len(), 2);
        assert_eq!((v[0].x, v[0].y), (10, 20 + 8 - 2));
        assert_eq!(v[1].x, 14);
    }

    #[test]
    fn wraps_at_spaces() {
        let f = tiny();
        let l = f.wrap("AA AA AA", 12);
        assert_eq!(l, vec!["AA", "AA", "AA"]);
        assert_eq!(f.wrap("A\nA", 100), vec!["A", "A"]);
    }
}
