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
        let c = self.layer.channels();
        self.layer.values().chunks(c).flat_map(|p| [p[0], p[1], p[2], 255]).collect()
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

        let px: Vec<u8> = if self.layer.channels() == 4 { vec![rgb[0], rgb[1], rgb[2], 255] } else { rgb.to_vec() };
        let hard = Brush { strength: 1.0, falloff: 0.0, curve: None, ..*b };
        for (x, y, dx, dy) in self.layer.under(b.cx, b.cy, b.radius) {
            if hard.weight(dx, dy) > 0.0 {
                self.layer.set_pixel(x, y, &px);
            }
        }

        Ok(())
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
        f.paint(&Brush { cx: 10.0, cy: 10.0, radius: 3.0, strength: 0.1, falloff: 1.0, curve: None }, GROUND_COLOURS[8]).unwrap();
        f.end_edit();
        assert_eq!(f.rgb(10, 12), GROUND_COLOURS[8]);
        assert_eq!(f.rgb(10, 13), [0, 0, 0]);
        let painted = PngFile::parse(&f.to_bytes().unwrap()).unwrap();
        assert_eq!(&painted.pixels[(12 * 40 + 10) * 4..(12 * 40 + 10) * 4 + 4], &[255, 96, 0, 255]);

        assert!(f.undo());
        assert_eq!(f.to_bytes().unwrap(), bytes);
    }
}
