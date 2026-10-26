//! The tile blend map as the editor edits it: blend_map.tif, 16 little-endian 16-bit weights per pixel (channel = 4 * group + variation of the
//! entry's four TEXTURE_GROUPs), summing to 65535.
//!
//! A dab moves a pixel's weight towards one channel and scales the other 15 down, so that the pixel keeps summing to 65535 (wce's own brush:
//! TEd's is not reverse-engineered, its files are what is kept). The file's bytes before and after the samples are kept as read, so a map that
//! was not painted is written back byte for byte, and a painted one differs only in the painted samples. Undo: `PixelLayer`'s block records.

use std::io::Write;
use std::path::Path;

use wce_formats::image_containers::TiffFile;

use crate::height_field::Brush;
use crate::pixel_layer::PixelLayer;

pub type Result<T> = std::result::Result<T, String>;

/// The sum of a pixel's weights.
pub const FULL: u16 = u16::MAX;

#[derive(Clone, Debug, Default, PartialEq)]
pub struct BlendField {
    /// The file's bytes before the samples (the directory) and after them, verbatim.
    header: Vec<u8>,
    tail: Vec<u8>,
    layer: PixelLayer<u16>,
}

impl BlendField {

    /// # Errors
    ///
    /// The image is not 16 samples of 16 bits per pixel.
    pub fn from_tiff(tif: TiffFile) -> Result<Self> {
        let n = tif.width as usize * tif.height as usize;
        if tif.samples_per_pixel != 16 || tif.bits_per_sample != 16 || tif.width == 0 || tif.height == 0 || tif.pixels.len() < n * 32 {
            return Err("blend_map.tif is not 16 samples of 16 bits per pixel".to_owned());
        }

        let v = tif.pixels.as_chunks::<2>().0[..n * 16].iter().map(|b| u16::from_le_bytes(*b)).collect();
        let tail = tif.pixels[n * 32..].to_vec();
        Ok(Self { header: tif.header, tail, layer: PixelLayer::new(tif.width, tif.height, 16, v) })
    }

    /// # Errors
    ///
    /// The file does not read or is not a blend map.
    pub fn read(path: &Path) -> Result<Self> {
        let bytes = std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
        let tif = TiffFile::parse(&bytes).map_err(|e| format!("blend_map.tif: {e}"))?;
        Self::from_tiff(tif)
    }

    /// Writes the file: the header and tail as read, the samples as they are now.
    ///
    /// # Errors
    ///
    /// The file could not be written.
    pub fn write(&self, path: &Path) -> Result<()> {
        let fail = |e: std::io::Error| format!("{}: {e}", path.display());
        let file = std::fs::File::create(path).map_err(fail)?;
        let mut out = std::io::BufWriter::with_capacity(1 << 20, file);
        out.write_all(&self.header).map_err(fail)?;
        for row in self.layer.values().chunks(self.width() as usize * 16) {
            let bytes = row.iter().flat_map(|s| s.to_le_bytes()).collect::<Vec<_>>();
            out.write_all(&bytes).map_err(fail)?;
        }

        out.write_all(&self.tail).map_err(fail)?;
        out.flush().map_err(fail)
    }

    /// The file's bytes (as [`Self::write`] writes them).
    pub fn to_bytes(&self) -> Vec<u8> {
        let v = self.layer.values();
        let mut out = Vec::with_capacity(self.header.len() + v.len() * 2 + self.tail.len());
        out.extend_from_slice(&self.header);
        out.extend(v.iter().flat_map(|s| s.to_le_bytes()));
        out.extend_from_slice(&self.tail);
        out
    }

    pub fn width(&self) -> u32 {
        self.layer.width()
    }

    pub fn height(&self) -> u32 {
        self.layer.height()
    }

    /// The 16 weights of a pixel.
    pub fn pixel(&self, x: u32, y: u32) -> &[u16] {
        self.layer.pixel(x, y)
    }

    /// All the weights, 16 per pixel, row by row.
    pub fn weights(&self) -> &[u16] {
        self.layer.values()
    }

    /// The largest weight of every channel.
    pub fn peaks(&self) -> [u32; 16] {
        let mut peak = [0u32; 16];
        for p in self.weights().as_chunks::<16>().0 {
            for (m, &v) in peak.iter_mut().zip(p) {
                *m = (*m).max(u32::from(v));
            }
        }

        peak
    }

    /// Whether something changed since `take_changed` was last called.
    pub fn changed(&self) -> bool {
        self.layer.changed()
    }

    /// The rectangle changed since the last call (exclusive end), `None` when nothing changed.
    pub fn take_changed(&mut self) -> Option<(u32, u32, u32, u32)> {
        self.layer.take_changed()
    }

    /// Paints `channel` (0..16) under the brush: per pixel, the brush's weight (0..1) of what the other channels hold moves to it.
    ///
    /// # Errors
    ///
    /// No edit is open, or no such channel.
    pub fn paint(&mut self, b: &Brush, channel: usize) -> Result<()> {
        if !self.layer.is_editing() {
            return Err("begin_edit() before editing".to_owned());
        }

        if channel >= 16 {
            return Err(format!("no blend channel {channel}"));
        }

        for (x, y, dx, dy) in self.layer.under(b.cx, b.cy, b.radius) {
            let a = b.weight(dx, dy).min(1.0);
            if a <= 0.0 {
                continue;
            }

            let p: [u16; 16] = self.layer.pixel(x, y).try_into().expect("16 weights");
            self.layer.set_pixel(x, y, &paint_pixel(&p, channel, a));
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

    /// Stores the blocks the open operation changed.
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

/// A pixel with `a` (0..1) of what the other channels hold moved to `channel`: the channel gains at least one step while it is not full,
/// the others keep their proportions, and the 16 weights sum to [`FULL`] (largest remainders get the rounding).
pub fn paint_pixel(p: &[u16; 16], channel: usize, a: f64) -> [u16; 16] {
    let full = u32::from(FULL);
    let c = u32::from(p[channel]).min(full);
    if c == full && p.iter().enumerate().all(|(i, &v)| i == channel || v == 0) {
        return *p;
    }

    let gain = (f64::from(full - c) * a.clamp(0.0, 1.0)).ceil() as u32;
    let target = (c + gain.max(1)).min(full);
    let rest = full - target;
    let others: u32 = p.iter().enumerate().filter(|&(i, _)| i != channel).map(|(_, &v)| u32::from(v)).sum();

    let mut out = [0u16; 16];
    out[channel] = target as u16;
    if rest == 0 || others == 0 {
        // Nothing else to scale: the rest (a pixel that did not sum to FULL) goes to the channel too.
        out[channel] = if others == 0 { FULL } else { target as u16 };
        return out;
    }

    let mut given = 0u32;
    let mut frac = [(0u64, 0usize); 16];
    for (i, &v) in p.iter().enumerate() {
        if i == channel {
            continue;
        }

        let exact = u64::from(v) * u64::from(rest);
        let q = (exact / u64::from(others)) as u32;
        out[i] = q as u16;
        given += q;
        frac[i] = (exact % u64::from(others), i);
    }

    // The steps lost to rounding, to the largest remainders (the lower channel first on a tie).
    let mut order = frac.iter().copied().filter(|&(_, i)| i != channel).collect::<Vec<_>>();
    order.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)));
    for &(_, i) in order.iter().take((rest - given) as usize) {
        out[i] += 1;
    }

    out
}

/// BOB's reduction of a pixel's 16 weights (docs/re/terrain.md): per group, the blend weight (the sum of its four) and the index (the
/// weighted mean variation / 3), as bytes.
pub fn reduce_pixel(p: &[u16]) -> ([u8; 4], [u8; 4]) {
    let q8 = |v: f64| (v * 255.0 + 0.5).clamp(0.0, 255.0) as u8;
    let (mut blend, mut index) = ([0u8; 4], [0u8; 4]);
    for g in 0..4 {
        let s = f64::from(p[g * 4]) + f64::from(p[g * 4 + 1]) + f64::from(p[g * 4 + 2]) + f64::from(p[g * 4 + 3]);
        blend[g] = q8(s / 65535.0);
        index[g] = if s > 0.0 { q8((f64::from(p[g * 4 + 1]) + 2.0 * f64::from(p[g * 4 + 2]) + 3.0 * f64::from(p[g * 4 + 3])) / s / 3.0) } else { 0 };
    }

    (blend, index)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn field(w: u32, h: u32) -> BlendField {
        let mut px = vec![0u8; (w * h * 32) as usize];
        for p in px.chunks_mut(32) {
            p[..2].copy_from_slice(&FULL.to_le_bytes());
        }

        let mut bytes = TiffFile::ted_header(w, h);
        bytes.extend_from_slice(&px);
        BlendField::from_tiff(TiffFile::parse(&bytes).unwrap()).unwrap()
    }

    #[test]
    fn a_dab_keeps_the_sum_and_the_proportions() {
        let mut p = [0u16; 16];
        (p[0], p[1], p[5]) = (30_000, 20_000, 15_535);
        let q = paint_pixel(&p, 9, 0.5);
        assert_eq!(q.iter().map(|&v| u32::from(v)).sum::<u32>(), 65_535);
        assert_eq!(q[9], 32_768);
        assert!((f64::from(q[0]) / f64::from(q[1]) - 1.5).abs() < 1e-3);
        assert_eq!(paint_pixel(&q, 9, 1.0), { let mut f = [0u16; 16]; f[9] = FULL; f });
    }

    #[test]
    fn a_weak_dab_still_moves_and_a_full_pixel_stays() {
        let mut p = [0u16; 16];
        p[0] = FULL;
        assert_eq!(paint_pixel(&p, 3, 1e-9)[3], 1);
        assert_eq!(paint_pixel(&p, 0, 0.7), p);
    }

    #[test]
    fn a_stroke_undoes_and_writes_the_same_bytes() {
        let tmp = tempfile::tempdir().unwrap();
        let mut f = field(150, 70);
        f.write(&tmp.path().join("a.tif")).unwrap();
        let original = std::fs::read(tmp.path().join("a.tif")).unwrap();

        f.begin_edit().unwrap();
        f.paint(&Brush { cx: 63.0, cy: 64.0, radius: 9.0, strength: 0.8, falloff: 0.5, ..Default::default() }, 6).unwrap();
        f.paint(&Brush { cx: 140.0, cy: 2.0, radius: 20.0, strength: 1.0, falloff: 0.0, ..Default::default() }, 15).unwrap();
        f.end_edit();
        assert!(f.pixel(63, 64)[6] > 0 && f.pixel(149, 0)[15] == FULL);
        assert_eq!(f.take_changed(), Some((55, 0, 150, 70)));

        f.write(&tmp.path().join("b.tif")).unwrap();
        let painted = std::fs::read(tmp.path().join("b.tif")).unwrap();
        assert_eq!(painted.len(), original.len());
        assert_eq!(painted[..242], original[..242]);

        assert!(f.undo());
        f.write(&tmp.path().join("c.tif")).unwrap();
        assert_eq!(std::fs::read(tmp.path().join("c.tif")).unwrap(), original);
        assert!(f.redo());
        f.write(&tmp.path().join("d.tif")).unwrap();
        assert_eq!(std::fs::read(tmp.path().join("d.tif")).unwrap(), painted);
        assert_eq!(f.to_bytes(), painted);
    }
}
