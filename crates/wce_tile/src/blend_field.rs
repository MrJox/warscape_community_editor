//! The tile blend map as the editor edits it: blend_map.tif, 16 little-endian 16-bit weights per pixel (channel = 4 * group + variation of the
//! entry's four TEXTURE_GROUPs), summing to 65535.
//!
//! A dab moves a pixel's weight towards one channel and scales the other 15 down, so that the pixel keeps summing to 65535 (wce's own brush:
//! TEd's is not reverse-engineered, its files are what is kept). The file's bytes before and after the samples are kept as read, so a map that
//! was not painted is written back byte for byte, and a painted one differs only in the painted samples. Undo records hold the 64 x 64 pixel
//! blocks a stroke touched (a 64 x 64 tile's map is 2.3 GB: no whole copies).

use std::collections::HashMap;
use std::io::Write;
use std::path::Path;

use wce_formats::image_containers::TiffFile;

use crate::height_field::Brush;

pub type Result<T> = std::result::Result<T, String>;

/// The sum of a pixel's weights.
pub const FULL: u16 = u16::MAX;

/// Pixels a side of an undo block.
const BLOCK: u32 = 64;

/// The blocks a stroke changed: their samples before and after.
#[derive(Clone, Debug, Default, PartialEq)]
struct Record {
    blocks: Vec<(u32, Vec<u16>, Vec<u16>)>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct BlendField {
    w: u32,
    h: u32,

    /// The file's bytes before the samples (the directory) and after them, verbatim.
    header: Vec<u8>,
    tail: Vec<u8>,
    v: Vec<u16>,

    editing: bool,

    /// The open edit's blocks as they were at its start.
    saved: HashMap<u32, Vec<u16>>,
    undo: Vec<Record>,
    redo: Vec<Record>,

    /// The rectangle changed since the display last took it (exclusive end).
    changed: Option<(u32, u32, u32, u32)>,
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
        Ok(Self { w: tif.width, h: tif.height, header: tif.header, tail, v, ..Default::default() })
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
        for row in self.v.chunks(self.w as usize * 16) {
            let bytes = row.iter().flat_map(|s| s.to_le_bytes()).collect::<Vec<_>>();
            out.write_all(&bytes).map_err(fail)?;
        }

        out.write_all(&self.tail).map_err(fail)?;
        out.flush().map_err(fail)
    }

    /// The file's bytes (as [`Self::write`] writes them).
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(self.header.len() + self.v.len() * 2 + self.tail.len());
        out.extend_from_slice(&self.header);
        out.extend(self.v.iter().flat_map(|s| s.to_le_bytes()));
        out.extend_from_slice(&self.tail);
        out
    }

    pub fn width(&self) -> u32 {
        self.w
    }

    pub fn height(&self) -> u32 {
        self.h
    }

    /// The 16 weights of a pixel.
    pub fn pixel(&self, x: u32, y: u32) -> &[u16] {
        let i = (y as usize * self.w as usize + x as usize) * 16;
        &self.v[i..i + 16]
    }

    /// All the weights, 16 per pixel, row by row.
    pub fn weights(&self) -> &[u16] {
        &self.v
    }

    /// The largest weight of every channel.
    pub fn peaks(&self) -> [u32; 16] {
        let mut peak = [0u32; 16];
        for p in self.v.as_chunks::<16>().0 {
            for (m, &v) in peak.iter_mut().zip(p) {
                *m = (*m).max(u32::from(v));
            }
        }

        peak
    }

    fn block_of(&self, x: u32, y: u32) -> u32 {
        (y / BLOCK) * self.w.div_ceil(BLOCK) + x / BLOCK
    }

    fn block_rect(&self, b: u32) -> (u32, u32, u32, u32) {
        let per_row = self.w.div_ceil(BLOCK);
        let (x0, y0) = ((b % per_row) * BLOCK, (b / per_row) * BLOCK);
        (x0, y0, (x0 + BLOCK).min(self.w), (y0 + BLOCK).min(self.h))
    }

    fn block_samples(&self, b: u32) -> Vec<u16> {
        let (x0, y0, x1, y1) = self.block_rect(b);
        let mut out = Vec::with_capacity(((x1 - x0) * (y1 - y0) * 16) as usize);
        for y in y0..y1 {
            let i = (y as usize * self.w as usize + x0 as usize) * 16;
            out.extend_from_slice(&self.v[i..i + (x1 - x0) as usize * 16]);
        }

        out
    }

    fn put_block(&mut self, b: u32, samples: &[u16]) {
        let (x0, y0, x1, y1) = self.block_rect(b);
        let n = (x1 - x0) as usize * 16;
        for (k, y) in (y0..y1).enumerate() {
            let i = (y as usize * self.w as usize + x0 as usize) * 16;
            self.v[i..i + n].copy_from_slice(&samples[k * n..(k + 1) * n]);
        }

        self.mark(x0, y0, x1, y1);
    }

    fn mark(&mut self, x0: u32, y0: u32, x1: u32, y1: u32) {
        self.changed = Some(match self.changed {
            None => (x0, y0, x1, y1),
            Some((a, b, c, d)) => (a.min(x0), b.min(y0), c.max(x1), d.max(y1)),
        });
    }

    /// The rectangle changed since the last call (exclusive end), `None` when nothing changed.
    pub fn take_changed(&mut self) -> Option<(u32, u32, u32, u32)> {
        self.changed.take()
    }

    /// Paints `channel` (0..16) under the brush: per pixel, the brush's weight (0..1) of what the other channels hold moves to it.
    ///
    /// # Errors
    ///
    /// No edit is open, or no such channel.
    pub fn paint(&mut self, b: &Brush, channel: usize) -> Result<()> {
        if !self.editing {
            return Err("begin_edit() before editing".to_owned());
        }

        if channel >= 16 {
            return Err(format!("no blend channel {channel}"));
        }

        let x0 = ((b.cx - b.radius).floor() as i64).max(0) as u32;
        let x1 = ((b.cx + b.radius).ceil() as i64).min(i64::from(self.w) - 1);
        let y0 = ((b.cy - b.radius).floor() as i64).max(0) as u32;
        let y1 = ((b.cy + b.radius).ceil() as i64).min(i64::from(self.h) - 1);
        if x1 < 0 || y1 < 0 {
            return Ok(());
        }

        let (x1, y1) = (x1 as u32, y1 as u32);
        let (mut tx0, mut ty0, mut tx1, mut ty1) = (u32::MAX, u32::MAX, 0, 0);
        for y in y0..=y1 {
            for x in x0..=x1 {
                let a = b.weight(f64::from(x) - b.cx, f64::from(y) - b.cy).min(1.0);
                if a <= 0.0 {
                    continue;
                }

                let i = (y as usize * self.w as usize + x as usize) * 16;
                let p: [u16; 16] = self.v[i..i + 16].try_into().expect("16 weights");
                let after = paint_pixel(&p, channel, a);
                if after == p {
                    continue;
                }

                let block = self.block_of(x, y);
                if !self.saved.contains_key(&block) {
                    let samples = self.block_samples(block);
                    self.saved.insert(block, samples);
                }

                self.v[i..i + 16].copy_from_slice(&after);
                (tx0, ty0, tx1, ty1) = (tx0.min(x), ty0.min(y), tx1.max(x + 1), ty1.max(y + 1));
            }
        }

        if tx1 > tx0 {
            self.mark(tx0, ty0, tx1, ty1);
        }

        Ok(())
    }

    /// Marks the start of an operation for undo.
    ///
    /// # Errors
    ///
    /// An edit is already open.
    pub fn begin_edit(&mut self) -> Result<()> {
        if self.editing {
            return Err("edit already open".to_owned());
        }

        self.editing = true;
        self.saved.clear();
        Ok(())
    }

    /// Stores the blocks the open operation changed.
    pub fn end_edit(&mut self) {
        if !self.editing {
            return;
        }

        self.editing = false;
        if self.saved.is_empty() {
            return;
        }

        let mut blocks = std::mem::take(&mut self.saved).into_iter().map(|(b, before)| {
            let after = self.block_samples(b);
            (b, before, after)
        }).collect::<Vec<_>>();

        blocks.sort_by_key(|b| b.0);
        self.undo.push(Record { blocks });
        self.redo.clear();
    }

    pub fn is_editing(&self) -> bool {
        self.editing
    }

    pub fn undo(&mut self) -> bool {
        let Some(r) = self.undo.pop() else { return false };
        for (b, before, _) in &r.blocks {
            self.put_block(*b, before);
        }

        self.redo.push(r);
        true
    }

    pub fn redo(&mut self) -> bool {
        let Some(r) = self.redo.pop() else { return false };
        for (b, _, after) in &r.blocks {
            self.put_block(*b, after);
        }

        self.undo.push(r);
        true
    }

    pub fn edit_depth(&self) -> usize {
        self.undo.len()
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
        f.paint(&Brush { cx: 63.0, cy: 64.0, radius: 9.0, strength: 0.8, falloff: 0.5 }, 6).unwrap();
        f.paint(&Brush { cx: 140.0, cy: 2.0, radius: 20.0, strength: 1.0, falloff: 0.0 }, 15).unwrap();
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
