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

/// The slope units cannot walk from (`MAXIMUM_PASSABLE_SLOPE_ANGLE_DEGREES`, the game's default; docs/re/terrain.md).
pub const NOGO_SLOPE_DEGREES: f32 = 30.0;

/// TEd's Blendmap Filters (docs/re/terrain.md): the paint lands only where the terrain's height (metres) and slope (degrees) are in
/// their ranges, a range that is `None` letting everything through. A hard mask, as TEd's.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct PaintFilter {
    pub height: Option<[f32; 2]>,
    pub slope: Option<[f32; 2]>,
}

impl PaintFilter {
    pub fn is_active(&self) -> bool {
        self.height.is_some() || self.slope.is_some()
    }

    pub fn passes(&self, height_m: f64, slope_degrees: f64) -> bool {
        let inside = |r: Option<[f32; 2]>, v: f64| r.is_none_or(|[lo, hi]| (f64::from(lo)..=f64::from(hi)).contains(&v));
        inside(self.height, height_m) && inside(self.slope, slope_degrees)
    }
}

/// What a paint stroke does to the pixels under it (TEd's Blend Map operations; docs/re/terrain.md).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub enum PaintMode {
    /// The channel gains the brush's weight of what the others hold.
    #[default]
    Normal,

    /// TEd's Auto-Flange: the channel's texture group with a graded rim of its variations. With the stroke's weight w (it builds up with
    /// every dab, towards 1): above `threshold` the pixel becomes all the group, its variation sliding from 0 (the brush's edge, a light
    /// touch) to 3 (its middle, more painting); below it only variation 0 fades in. Set from the pixel as the stroke found it, not added.
    AutoFlange { threshold: f32 },

    /// TEd's Rock-painting: the channel's weight follows the slope (`rock_weight`: none below about 26 degrees, all from about 72). Unlike
    /// TEd's (which ignores the brush and can take paint away), wce's moves towards that weight by the brush's weight and never lowers it.
    Rock,
}

/// Rock-painting's weight of a slope in degrees (TEd's `Ted::RockPaintOperation`): s = 1 - cos(slope), A = smoothstep(0.1, 0.7, s),
/// B = smoothstep(0.3, 0.9, s), A (1 - B) + B: 0.01 at 30 degrees, 0.24 at 45, 0.81 at 60, 1 from 72.5.
pub fn rock_weight(slope_degrees: f64) -> f64 {
    let smooth = |lo: f64, hi: f64, x: f64| {
        let t = ((x - lo) / (hi - lo)).clamp(0.0, 1.0);
        (3.0 - 2.0 * t) * t * t
    };

    let s = 1.0 - slope_degrees.to_radians().cos().clamp(0.0, 1.0);
    let (a, b) = (smooth(0.1, 0.7, s), smooth(0.3, 0.9, s));
    a * (1.0 - b) + b
}

/// Auto-Flange's pixel (TEd's `Ted::PM2::OpBlend2AutoFlange::apply`) from the pixel `source` as the stroke found it and the stroke's weight
/// `w` there, for texture `group` (its four variations) and `threshold`; `None` leaves the pixel as it is.
pub fn flange_pixel(source: &[u16; 16], group: usize, w: f64, threshold: f64) -> Option<[u16; 16]> {
    let g = group.min(3) * 4;
    let mut out = [0.0f64; 16];
    if w > threshold {
        let f = ((w - threshold) / (1.0 - threshold)).clamp(0.0, 1.0);
        let pos = g as f64 + 3.0 * f;
        let i = (pos.floor() as usize).min(g + 3);
        let frac = pos - i as f64;
        out[i] = 1.0 - frac;
        if frac > 0.0 && i < g + 3 {
            out[i + 1] = frac;
        }
    } else {
        let c0 = f64::from(source[g]) / 65535.0;
        if c0 >= 0.99 || threshold <= 0.0 {
            return None;
        }

        let k = (1.0 - w / threshold) / (1.0 - c0);
        for (o, &v) in out.iter_mut().zip(source) {
            *o = f64::from(v) / 65535.0 * k;
        }

        out[g] = w / threshold;
    }

    Some(quantize(&out))
}

/// 16 weights (summing about 1) as samples summing to [`FULL`], the largest remainders getting the rounding.
fn quantize(w: &[f64; 16]) -> [u16; 16] {
    let sum: f64 = w.iter().sum();
    let scale = if sum > 0.0 { 65535.0 / sum } else { 0.0 };
    let mut out = [0u16; 16];
    let mut frac = [(0.0f64, 0usize); 16];
    let mut given = 0u32;
    for (i, &v) in w.iter().enumerate() {
        let x = v.max(0.0) * scale;
        out[i] = x.floor() as u16;
        given += u32::from(out[i]);
        frac[i] = (x - x.floor(), i);
    }

    if sum <= 0.0 {
        out[0] = FULL;
        return out;
    }

    frac.sort_by(|a, b| b.0.total_cmp(&a.0).then(a.1.cmp(&b.1)));
    for &(_, i) in frac.iter().take(65535u32.saturating_sub(given) as usize) {
        out[i] += 1;
    }

    out
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct BlendField {
    /// The file's bytes before the samples (the directory) and after them, verbatim.
    header: Vec<u8>,
    tail: Vec<u8>,
    layer: PixelLayer<u16>,

    /// The open stroke's weight per pixel (y * w + x), built up dab by dab as TEd's painter does: acc + (1 - acc) * the dab's weight.
    acc: std::collections::HashMap<u64, f32>,
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
        Ok(Self { header: tif.header, tail, layer: PixelLayer::new(tif.width, tif.height, 16, v), acc: Default::default() })
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

    /// Paints `channel` (0..16) under the brush: per pixel, the brush's weight (0..1) of what the other channels hold moves to it; only
    /// the pixels `mask` lets through.
    ///
    /// # Errors
    ///
    /// No edit is open, or no such channel.
    pub fn paint(&mut self, b: &Brush, channel: usize, mask: &dyn Fn(u32, u32) -> bool) -> Result<()> {
        self.paint_mode(b, channel, PaintMode::Normal, mask, &|_, _| 0.0)
    }

    /// [`Self::paint`] in a mode: Auto-Flange paints `channel`'s group, Rock-painting reads the slope (degrees) at a pixel from `slope`.
    ///
    /// # Errors
    ///
    /// No edit is open, or no such channel.
    pub fn paint_mode(&mut self, b: &Brush, channel: usize, mode: PaintMode, mask: &dyn Fn(u32, u32) -> bool, slope: &dyn Fn(u32, u32) -> f64) -> Result<()> {
        if !self.layer.is_editing() {
            return Err("begin_edit() before editing".to_owned());
        }

        if channel >= 16 {
            return Err(format!("no blend channel {channel}"));
        }

        let w = u64::from(self.width());
        for (x, y, dx, dy) in self.layer.under(b.cx, b.cy, b.radius) {
            let a = b.weight(dx, dy).min(1.0);
            if a <= 0.0 || !mask(x, y) {
                continue;
            }

            let p: [u16; 16] = self.layer.pixel(x, y).try_into().expect("16 weights");
            let after = match mode {
                PaintMode::Normal => Some(paint_pixel(&p, channel, a)),
                PaintMode::AutoFlange { threshold } => {
                    let acc = self.acc.entry(u64::from(y) * w + u64::from(x)).or_insert(0.0);
                    *acc += (1.0 - *acc) * a as f32;
                    let source: [u16; 16] = self.layer.original(x, y).try_into().expect("16 weights");
                    flange_pixel(&source, channel / 4, f64::from(*acc), f64::from(threshold))
                }

                PaintMode::Rock => {
                    let (c, t) = (f64::from(p[channel]) / 65535.0, rock_weight(slope(x, y)));
                    (t > c && c < 1.0).then(|| paint_pixel(&p, channel, (t - c) * a / (1.0 - c)))
                }
            };

            if let Some(after) = after {
                self.layer.set_pixel(x, y, &after);
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
        self.acc.clear();
        if self.layer.begin_edit() { Ok(()) } else { Err("edit already open".to_owned()) }
    }

    /// Stores the blocks the open operation changed.
    pub fn end_edit(&mut self) {
        self.acc.clear();
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
    fn rock_weight_follows_teds_curve() {
        assert!(rock_weight(20.0) < 1e-9);
        assert!((rock_weight(45.0) - 0.244).abs() < 0.01);
        assert!((rock_weight(60.0) - 0.808).abs() < 0.01);
        assert!((rock_weight(80.0) - 1.0).abs() < 1e-9);
    }

    #[test]
    fn auto_flange_slides_through_the_group_variations() {
        let mut p = [0u16; 16];
        p[0] = FULL;
        let mid = flange_pixel(&p, 2, 0.5, 0.0).unwrap();
        assert_eq!((mid[9], mid[10]), (32768, 32767), "half way: between variations 1 and 2 of group 2");
        assert_eq!(flange_pixel(&p, 2, 1.0, 0.0).unwrap()[11], FULL, "full weight: variation 3");
        let rim = flange_pixel(&p, 2, 0.25, 0.5).unwrap();
        assert_eq!((rim[8], rim[0]), (32767, 32768), "under the threshold variation 0 fades in (the tie goes to the lower channel)");
        assert!(flange_pixel(&rim, 2, 0.1, 0.5).is_some() && flange_pixel(&[0, 0, 0, 0, 0, 0, 0, 0, FULL, 0, 0, 0, 0, 0, 0, 0], 2, 0.1, 0.5).is_none());
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
        f.paint(&Brush { cx: 63.0, cy: 64.0, radius: 9.0, strength: 0.8, falloff: 0.5, ..Default::default() }, 6, &|_, _| true).unwrap();
        f.paint(&Brush { cx: 140.0, cy: 2.0, radius: 20.0, strength: 1.0, falloff: 0.0, ..Default::default() }, 15, &|x, _| x != 149).unwrap();
        f.end_edit();
        assert!(f.pixel(63, 64)[6] > 0 && f.pixel(148, 0)[15] == FULL && f.pixel(149, 0)[15] == 0, "the mask keeps column 149");
        assert_eq!(f.take_changed(), Some((55, 0, 149, 70)));

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
