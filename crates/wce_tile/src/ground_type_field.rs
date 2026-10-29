//! The tile's `ground_type_map.png` as the editor edits it: 8-bit RGB(A) over the bordered tile, black but where a ground type is painted
//! in its colour (`ground_types`), alpha 255 (every painted pixel of the kit's tiles). The brush writes hard values: a pixel under it is
//! that ground type, or black when erasing. The PNG's header and chunks are kept; a map that was not painted is written back as read
//! (its source bytes), a painted one through the kit's encoder. Undo: `PixelLayer`'s block records.

use std::path::Path;

use wce_formats::png_file::PngFile;

use crate::height_field::Brush;
use crate::pixel_layer::PixelLayer;

pub type Result<T> = std::result::Result<T, String>;

#[derive(Clone, Debug, Default)]
pub struct GroundTypeField {
    /// The file as read (its header, chunks and source bytes).
    base: PngFile,
    layer: PixelLayer<u8>,
}

impl GroundTypeField {

    /// # Errors
    ///
    /// The image is not 8-bit RGB or RGBA.
    pub fn from_png(png: PngFile) -> Result<Self> {
        if png.bit_depth != 8 || (png.color_type != 6 && png.color_type != 2) {
            return Err("ground_type_map.png is not 8-bit RGB(A)".to_owned());
        }

        let bpp = png.bytes_per_pixel();
        let n = png.width as usize * png.height as usize * bpp;
        if png.pixels.len() < n {
            return Err("ground_type_map.png is truncated".to_owned());
        }

        let layer = PixelLayer::new(png.width, png.height, bpp, png.pixels[..n].to_vec());
        Ok(Self { base: png, layer })
    }

    /// # Errors
    ///
    /// The file does not read or is not a ground type map.
    pub fn read(path: &Path) -> Result<Self> {
        let bytes = std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
        Self::from_png(PngFile::parse(&bytes).map_err(|e| format!("ground_type_map.png: {e}"))?)
    }

    /// The file's bytes: as read when nothing was painted, else the pixels as they are now through the kit's encoder.
    ///
    /// # Errors
    ///
    /// The PNG does not encode.
    pub fn to_bytes(&self) -> Result<Vec<u8>> {
        let mut png = self.base.clone();
        if png.pixels[..self.layer.values().len()] == *self.layer.values() && !png.original.is_empty() {
            return Ok(png.original);
        }

        png.pixels[..self.layer.values().len()].copy_from_slice(self.layer.values());
        png.pixels_edited = true;
        if png.has_ancillary { png.encode() } else { png.write() }.map_err(|e| e.to_string())
    }

    pub fn width(&self) -> u32 {
        self.layer.width()
    }

    pub fn height(&self) -> u32 {
        self.layer.height()
    }

    /// The colour of a pixel.
    pub fn rgb(&self, x: u32, y: u32) -> [u8; 3] {
        let p = self.layer.pixel(x, y);
        [p[0], p[1], p[2]]
    }

    /// The map as RGBA (alpha 255), row by row.
    pub fn rgba(&self) -> Vec<u8> {
        let mut out = vec![0u8; self.width() as usize * self.height() as usize * 4];
        self.rgba_into(&mut out, (0, 0, self.width(), self.height()));
        out
    }

    /// Writes a rectangle of the map (x0, y0, x1, y1; end exclusive) into an image of the whole map in `rgba`'s layout.
    pub fn rgba_into(&self, out: &mut [u8], (x0, y0, x1, y1): (u32, u32, u32, u32)) {
        let (w, c) = (self.width() as usize, self.layer.channels());
        let v = self.layer.values();
        for y in y0 as usize..y1 as usize {
            let (a, b) = (y * w + x0 as usize, y * w + x1 as usize);
            for (o, p) in out[a * 4..b * 4].chunks_exact_mut(4).zip(v[a * c..b * c].chunks_exact(c)) {
                o.copy_from_slice(&[p[0], p[1], p[2], 255]);
            }
        }
    }

    /// Whether something changed since `take_changed` was last called.
    pub fn changed(&self) -> bool {
        self.layer.changed()
    }

    /// The rectangle changed since the last call (exclusive end), `None` when nothing changed.
    pub fn take_changed(&mut self) -> Option<(u32, u32, u32, u32)> {
        self.layer.take_changed()
    }

    /// Paints `rgb` (a ground type's colour, black to erase) on every pixel under the brush.
    ///
    /// # Errors
    ///
    /// No edit is open.
    pub fn paint(&mut self, b: &Brush, rgb: [u8; 3]) -> Result<()> {
        if !self.layer.is_editing() {
            return Err("begin_edit() before editing".to_owned());
        }

        let px = [rgb[0], rgb[1], rgb[2], 255];
        let px = &px[..self.layer.channels()];
        let hard = Brush { cx: b.cx, cy: b.cy, radius: b.radius, strength: 1.0, falloff: 0.0, ..Default::default() };
        // A round hard brush covers one run of pixels per row: filled a run at a time.
        let (x0, y0, x1, y1) = self.layer.under_bounds(b.cx, b.cy, b.radius);
        for y in y0..=y1 {
            let inside = |x: i64| hard.weight(x as f64 - b.cx, y as f64 - b.cy) > 0.0;
            let Some(first) = (x0..=x1).find(|&x| inside(x)) else { continue };
            let last = (first..=x1).rev().find(|&x| inside(x)).unwrap_or(first);
            self.layer.fill_span(y as u32, first as u32, last as u32 + 1, px);
        }

        Ok(())
    }

    /// Sets one pixel's colour during an edit (alpha 255); false when it had it.
    ///
    /// # Panics
    ///
    /// No edit is open.
    pub fn set_rgb(&mut self, x: u32, y: u32, rgb: [u8; 3]) -> bool {
        let px = [rgb[0], rgb[1], rgb[2], 255];
        self.layer.set_pixel(x, y, &px[..self.layer.channels()])
    }

    /// Marks the start of an operation for undo.
    ///
    /// # Errors
    ///
    /// An edit is already open.
    pub fn begin_edit(&mut self) -> Result<()> {
        if self.layer.begin_edit() { Ok(()) } else { Err("edit already open".to_owned()) }
    }

    pub fn end_edit(&mut self) {
        self.layer.end_edit();
    }

    pub fn is_editing(&self) -> bool {
        self.layer.is_editing()
    }

    pub fn undo(&mut self) -> bool {
        self.layer.undo()
    }

    pub fn redo(&mut self) -> bool {
        self.layer.redo()
    }

    pub fn edit_depth(&self) -> usize {
        self.layer.edit_depth()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ground_types::GROUND_COLOURS;

    #[test]
    fn painting_and_undoing_gives_the_bytes_back() {
        let png = PngFile { width: 40, height: 30, bit_depth: 8, color_type: 6, pixels: [0, 0, 0, 255].repeat(1200), pixels_edited: true, ..Default::default() };
        let bytes = png.encode().unwrap();
        let mut f = GroundTypeField::from_png(PngFile::parse(&bytes).unwrap()).unwrap();
        assert_eq!(f.to_bytes().unwrap(), bytes);

        f.begin_edit().unwrap();
        f.paint(&Brush { cx: 10.0, cy: 10.0, radius: 3.0, strength: 0.1, falloff: 1.0, ..Default::default() }, GROUND_COLOURS[8]).unwrap();
        f.end_edit();
        assert_eq!(f.rgb(10, 12), GROUND_COLOURS[8]);
        assert_eq!(f.rgb(10, 13), [0, 0, 0]);
        let painted = PngFile::parse(&f.to_bytes().unwrap()).unwrap();
        assert_eq!(&painted.pixels[(12 * 40 + 10) * 4..(12 * 40 + 10) * 4 + 4], &[255, 96, 0, 255]);

        assert!(f.undo());
        assert_eq!(f.to_bytes().unwrap(), bytes);
    }

    #[test]
    fn the_runs_of_a_dab_are_the_pixels_inside_the_brush() {
        // 150 x 140: dabs over the 64 x 64 undo blocks' edges and the map's, RGB and RGBA.
        for color_type in [2u8, 6] {
            let bpp = if color_type == 6 { 4 } else { 3 };
            let png = PngFile { width: 150, height: 140, bit_depth: 8, color_type, pixels: vec![0; 150 * 140 * bpp], ..Default::default() };
            let mut f = GroundTypeField::from_png(png).unwrap();
            let mut want = vec![[0u8; 3]; 150 * 140];
            let dabs = [(63.5, 64.2, 7.3, 8), (0.0, 0.0, 4.0, 1), (149.0, 139.0, 10.0, 3), (100.0, 20.0, 0.5, 11), (64.0, 64.0, 2.0, 0)];
            f.begin_edit().unwrap();
            for &(cx, cy, radius, ground) in &dabs {
                let rgb = GROUND_COLOURS[ground];
                f.paint(&Brush { cx, cy, radius, ..Default::default() }, rgb).unwrap();
                let hard = Brush { cx, cy, radius, strength: 1.0, falloff: 0.0, ..Default::default() };
                for (k, p) in want.iter_mut().enumerate() {
                    let (x, y) = ((k % 150) as f64, (k / 150) as f64);
                    if (x - cx).abs() <= radius.ceil() + 1.0 && (y - cy).abs() <= radius.ceil() + 1.0 && hard.weight(x - cx, y - cy) > 0.0 {
                        *p = rgb;
                    }
                }
            }

            f.end_edit();
            for (k, p) in want.iter().enumerate() {
                assert_eq!(f.rgb((k % 150) as u32, (k / 150) as u32), *p, "pixel {k}, colour type {color_type}");
            }

            assert!(f.undo());
            assert!(f.layer.values().iter().all(|&v| v == 0));
            assert!(f.redo());
            assert!(want.iter().enumerate().all(|(k, p)| f.rgb((k % 150) as u32, (k / 150) as u32) == *p));
        }
    }
}
