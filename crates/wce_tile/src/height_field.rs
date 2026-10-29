//! The tile height map as the editor edits it: height_map_0.png (16-bit grey) <-> metres.
//!
//! metres = min_height + raw / 65535 * scale, with min_height and scale taken from the tile's VARIATION in the tile database (TILES/*.xml).
//! The heights are kept in metres, so brushes are not bound by the range; saving fits the range to them as TEd does (`fit_range`). Brushes
//! work in pixels; every edit goes through an undo record holding the touched rectangle, so edit + undo restores the exact samples (and the
//! PNG bytes, through the kit-exact encoder).

use std::sync::Arc;

use wce_formats::png_file::PngFile;

use crate::brush_image::BrushImage;
use crate::falloff::FalloffLut;

pub type Result<T> = std::result::Result<T, String>;

#[derive(Clone, Debug, PartialEq)]
pub struct Brush {
    /// Centre in pixels.
    pub cx: f64,
    pub cy: f64,

    /// Pixels.
    pub radius: f64,

    /// 0..1 multiplier.
    pub strength: f64,

    /// 0 = hard edge, 1 = smooth bell over the whole radius.
    pub falloff: f64,

    /// A falloff curve instead of `falloff` (the Brush Falloff editor).
    pub curve: Option<FalloffLut>,

    /// An image brush and its turn (radians): its brightness over the square the radius spans is the weight (instead of the falloff or the
    /// curve).
    pub image: Option<(Arc<BrushImage>, f64)>,
}

impl Brush {
    /// The brush's weight at an offset (pixels) from its centre: 0 outside the radius.
    pub fn weight(&self, dx: f64, dy: f64) -> f64 {
        if let Some((image, angle)) = &self.image {
            return self.strength * image.at(dx, dy, self.radius, *angle);
        }

        let d = (dx * dx + dy * dy).sqrt() / self.radius;
        if d >= 1.0 {
            return 0.0;
        }

        if let Some(c) = &self.curve {
            return self.strength * c.at(d);
        }

        // Hard edge at falloff 0, cosine bell at falloff 1.
        let bell = 0.5 * (1.0 + (d * std::f64::consts::PI).cos());
        self.strength * (self.falloff * bell + (1.0 - self.falloff))
    }
}

impl Default for Brush {
    fn default() -> Self {
        Self { cx: 0.0, cy: 0.0, radius: 8.0, strength: 1.0, falloff: 0.5, curve: None, image: None }
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
struct Record {
    x0: u32,
    y0: u32,
    x1: u32,
    y1: u32,
    before: Vec<f32>,
    after: Vec<f32>,
}

/// TEd's height range of a map that is flat (every sample the same): the range is `[h, h + 50]` (`TerrainIO.cpp`, docs/formats/tile_ted.md).
pub const FLAT_SCALE: f64 = 50.0;

#[derive(Clone, Debug, Default, PartialEq)]
pub struct HeightField {
    w: u32,
    h: u32,

    /// The range the samples are stored with in the file: the entry's min_height / scale as the map was read, or as it was last fitted.
    min: f64,
    scale: f64,

    /// The heights in metres, as offsets from `base` (the min_height they were read with): 32-bit floats keep a sample within 1e-7 of the
    /// range, far below a 16-bit step. Edits are not bound by the range: the range follows the heights when the map is saved (`fit_range`).
    base: f64,
    v: Vec<f32>,
    editing: bool,

    /// Touched rectangle of the open edit (exclusive end).
    tx0: u32,
    ty0: u32,
    tx1: u32,
    ty1: u32,
    touched: bool,

    /// Copy of the samples at `begin_edit`.
    snapshot: Vec<f32>,
    undo: Vec<Record>,
    redo: Vec<Record>,
}

impl HeightField {

    /// # Errors
    ///
    /// The image is not 16-bit grey.
    pub fn from_png(png: &PngFile, min_height: f64, scale: f64) -> Result<Self> {
        if png.bit_depth != 16 || png.color_type != 0 {
            return Err("height map must be 16-bit grey".to_owned());
        }

        let v = png.pixels.as_chunks::<2>().0.iter().take(png.width as usize * png.height as usize).map(|p| (f64::from(u16::from_be_bytes(*p)) / 65535.0 * scale) as f32).collect();
        Ok(Self { w: png.width, h: png.height, min: min_height, scale, base: min_height, v, ..Default::default() })
    }

    /// A flat map at `metres`, with the range TEd gives it (`[metres, metres + FLAT_SCALE]`, every sample 0).
    pub fn flat(w: u32, h: u32, metres: f64) -> Self {
        Self { w, h, min: metres, scale: FLAT_SCALE, base: metres, v: vec![0.0; w as usize * h as usize], ..Default::default() }
    }

    /// The samples as 16-bit values of the range (min_height, scale), rounded to nearest and clamped.
    pub fn quantized(&self, min_height: f64, scale: f64) -> Vec<u16> {
        self.v.iter().map(|&o| quantize(self.base + f64::from(o), min_height, scale)).collect()
    }

    /// Replaces the pixels of `base` (header, chunk layout stay) with the samples in the field's range and returns a PNG that `write`
    /// encodes with the kit encoder.
    pub fn to_png(&self, base: &PngFile) -> PngFile {
        let mut p = base.clone();
        p.pixels = self.quantized(self.min, self.scale).iter().flat_map(|r| r.to_be_bytes()).collect();
        p.pixels_edited = true;
        p
    }

    /// A blank 16-bit grey PNG of the field's size, for [`Self::to_png`].
    pub fn blank_png(&self) -> PngFile {
        PngFile { width: self.w, height: self.h, bit_depth: 16, color_type: 0, ..Default::default() }
    }

    pub fn width(&self) -> u32 {
        self.w
    }

    pub fn height(&self) -> u32 {
        self.h
    }

    /// The 16-bit sample in the field's range.
    pub fn raw(&self, x: u32, y: u32) -> u16 {
        self.to_raw(self.metres(x, y))
    }

    pub fn metres(&self, x: u32, y: u32) -> f64 {
        self.base + f64::from(self.v[y as usize * self.w as usize + x as usize])
    }

    pub fn to_metres(&self, r: u16) -> f64 {
        self.min + f64::from(r) / 65535.0 * self.scale
    }

    /// Clamped, rounded to nearest.
    pub fn to_raw(&self, m: f64) -> u16 {
        quantize(m, self.min, self.scale)
    }

    /// Another min_height / scale for the same 16-bit samples (before any edit: the samples are read again with these numbers).
    pub fn recalibrate(&mut self, min_height: f64, scale: f64) {
        let raw = self.quantized(self.min, self.scale);
        self.v = raw.iter().map(|&r| (f64::from(r) / 65535.0 * scale) as f32).collect();
        self.base = min_height;
        self.min = min_height;
        self.scale = scale;
    }

    /// The range the heights are stored with from now on (the heights stay).
    pub fn set_range(&mut self, min_height: f64, scale: f64) {
        self.min = min_height;
        self.scale = scale;
    }

    /// The range TEd saves the heights with (`TerrainIO.cpp`): min_height = the lowest sample, scale = the highest minus the lowest, no padding
    /// (a flat map: `FLAT_SCALE`). While the lowest and highest samples are within half a step of the field's range, that range is kept, so
    /// that a map that was not edited is saved with the numbers it was read with, bytes and entry unchanged.
    pub fn fit_range(&self) -> (f64, f64) {
        let (lo, hi) = self.v.iter().fold((f32::INFINITY, f32::NEG_INFINITY), |(lo, hi), &o| (lo.min(o), hi.max(o)));
        if self.v.is_empty() {
            return (self.min, self.scale);
        }

        let (lo, hi) = (self.base + f64::from(lo), self.base + f64::from(hi));
        let half_step = self.scale.abs() / 65535.0 * 0.5;
        if (lo - self.min).abs() <= half_step && (hi - (self.min + self.scale)).abs() <= half_step {
            return (self.min, self.scale);
        }

        (lo, if hi > lo { hi - lo } else { FLAT_SCALE })
    }

    pub fn min_height(&self) -> f64 {
        self.min
    }

    pub fn scale(&self) -> f64 {
        self.scale
    }

    /// A hash of the samples `sample` reads for points in the pixel rectangle `x0..x1`, `y0..y1` (clamped to the map) and of their base:
    /// equal while none of them changed (a cache of what was worked out from the heights there).
    pub fn region_hash(&self, x0: f64, y0: f64, x1: f64, y1: f64) -> u64 {
        let clamp = |v: f64, n: u32| (v.max(0.0) as u32).min(n.saturating_sub(1));
        let (a, b) = (clamp(x0.floor(), self.w), clamp(x1.floor() + 1.0, self.w));
        let (c, d) = (clamp(y0.floor(), self.h), clamp(y1.floor() + 1.0, self.h));
        let mut h = 0xcbf2_9ce4_8422_2325u64 ^ self.base.to_bits();
        for y in c..=d {
            let row = y as usize * self.w as usize;
            for &s in &self.v[row + a as usize..=row + b as usize] {
                h = (h ^ u64::from(s.to_bits())).wrapping_mul(0x0100_0000_01b3);
            }
        }

        h
    }

    /// Bilinear, metres.
    pub fn sample(&self, x: f64, y: f64) -> f64 {
        let x = x.clamp(0.0, f64::from(self.w - 1));
        let y = y.clamp(0.0, f64::from(self.h - 1));
        let (x0, y0) = (x as u32, y as u32);
        let (x1, y1) = ((x0 + 1).min(self.w - 1), (y0 + 1).min(self.h - 1));
        let (fx, fy) = (x - f64::from(x0), y - f64::from(y0));
        let a = self.metres(x0, y0) * (1.0 - fx) + self.metres(x1, y0) * fx;
        let b = self.metres(x0, y1) * (1.0 - fx) + self.metres(x1, y1) * fx;
        a * (1.0 - fy) + b * fy
    }

    fn touch(&mut self, x: u32, y: u32) {
        if !self.touched {
            (self.tx0, self.ty0, self.tx1, self.ty1, self.touched) = (x, y, x + 1, y + 1, true);
            return;
        }

        self.tx0 = self.tx0.min(x);
        self.ty0 = self.ty0.min(y);
        self.tx1 = self.tx1.max(x + 1);
        self.ty1 = self.ty1.max(y + 1);
    }

    /// Runs `f` over the samples under the brush: (field, x, y, weight, offset now) -> the new offset (metres above `base`).
    fn apply(&mut self, b: &Brush, f: impl Fn(&Self, i32, i32, f64, f64) -> f64) -> Result<()> {
        if !self.editing {
            return Err("begin_edit() before editing".to_owned());
        }

        let x0 = ((b.cx - b.radius).floor() as i32).max(0);
        let x1 = ((b.cx + b.radius).ceil() as i32).min(self.w as i32 - 1);
        let y0 = ((b.cy - b.radius).floor() as i32).max(0);
        let y1 = ((b.cy + b.radius).ceil() as i32).min(self.h as i32 - 1);
        for y in y0..=y1 {
            for x in x0..=x1 {
                let wgt = b.weight(f64::from(x) - b.cx, f64::from(y) - b.cy);
                if wgt <= 0.0 {
                    continue;
                }

                let i = y as usize * self.w as usize + x as usize;
                let before = self.v[i];
                let after = f(self, x, y, wgt, f64::from(before)) as f32;
                if after != before {
                    self.v[i] = after;
                    self.touch(x as u32, y as u32);
                }
            }
        }

        Ok(())
    }

    /// Adds metres (negative lowers).
    ///
    /// # Errors
    ///
    /// No edit is open.
    pub fn raise(&mut self, b: &Brush, metres: f64) -> Result<()> {
        self.apply(b, |_, _, _, w, cur| cur + metres * w)
    }

    /// Adds fractal noise (four octaves of value noise, -1 .. 1, features `scale` pixels across), `metres` at full weight: the same pattern
    /// wherever the brush goes (a function of the pixel), so that dabs deepen the same bumps.
    ///
    /// # Errors
    ///
    /// No edit is open.
    pub fn noise(&mut self, b: &Brush, metres: f64, scale: f64) -> Result<()> {
        let scale = scale.max(1.0);
        self.apply(b, |_, x, y, w, cur| cur + metres * w * fractal_noise(f64::from(x) / scale, f64::from(y) / scale))
    }

    /// Sets one sample to a height (metres, absolute) during an edit.
    ///
    /// # Errors
    ///
    /// No edit is open, or no such sample.
    pub fn set_metres(&mut self, x: u32, y: u32, metres: f64) -> Result<()> {
        if !self.editing {
            return Err("begin_edit() before editing".to_owned());
        }

        if x >= self.w || y >= self.h {
            return Err("no such sample".to_owned());
        }

        let i = y as usize * self.w as usize + x as usize;
        let v = (metres - self.base) as f32;
        if self.v[i] != v {
            self.v[i] = v;
            self.touch(x, y);
        }

        Ok(())
    }

    /// Blends towards the local average (of the samples before this stroke step).
    ///
    /// # Errors
    ///
    /// No edit is open.
    pub fn smooth(&mut self, b: &Brush) -> Result<()> {
        let src = self.v.clone();
        let (w, h) = (self.w as i32, self.h as i32);
        self.apply(b, |_, x, y, wgt, cur| {
            let (mut sum, mut n) = (0.0, 0);
            for dy in -1..=1 {
                for dx in -1..=1 {
                    let (xx, yy) = (x + dx, y + dy);
                    if xx < 0 || yy < 0 || xx >= w || yy >= h {
                        continue;
                    }

                    sum += f64::from(src[yy as usize * w as usize + xx as usize]);
                    n += 1;
                }
            }

            let avg = sum / f64::from(n);
            cur + (avg - cur) * wgt.min(1.0)
        })
    }

    /// Blends towards a level.
    ///
    /// # Errors
    ///
    /// No edit is open.
    pub fn flatten(&mut self, b: &Brush, target_metres: f64) -> Result<()> {
        let target = target_metres - self.base;
        self.apply(b, |_, _, _, w, cur| cur + (target - cur) * w.min(1.0))
    }

    /// Flattens to the height at the brush centre.
    ///
    /// # Errors
    ///
    /// No edit is open.
    pub fn set_level_under(&mut self, b: &Brush) -> Result<()> {
        let cx = b.cx.clamp(0.0, f64::from(self.w - 1)) as u32;
        let cy = b.cy.clamp(0.0, f64::from(self.h - 1)) as u32;
        self.flatten(b, self.metres(cx, cy))
    }

    /// A strip between two points, from `m0` to `m1` metres, with a linear edge falloff.
    ///
    /// # Errors
    ///
    /// No edit is open.
    #[allow(clippy::too_many_arguments, reason = "the C++ signature: a segment, its width and two heights")]
    pub fn ramp(&mut self, x0: f64, y0: f64, x1: f64, y1: f64, width: f64, m0: f64, m1: f64) -> Result<()> {
        if !self.editing {
            return Err("begin_edit() before editing".to_owned());
        }

        let (dx, dy) = (x1 - x0, y1 - y0);
        let len2 = dx * dx + dy * dy;
        if len2 <= 0.0 {
            return Ok(());
        }

        let xa = ((x0.min(x1) - width).floor() as i32).max(0);
        let xb = ((x0.max(x1) + width).ceil() as i32).min(self.w as i32 - 1);
        let ya = ((y0.min(y1) - width).floor() as i32).max(0);
        let yb = ((y0.max(y1) + width).ceil() as i32).min(self.h as i32 - 1);
        for y in ya..=yb {
            for x in xa..=xb {
                let (fx, fy) = (f64::from(x), f64::from(y));
                let t = ((fx - x0) * dx + (fy - y0) * dy) / len2;
                if !(0.0..=1.0).contains(&t) {
                    continue;
                }

                let (px, py) = (x0 + t * dx, y0 + t * dy);
                let d = (fx - px).hypot(fy - py);
                if d > width {
                    continue;
                }

                let (target, k) = (m0 + (m1 - m0) * t - self.base, 1.0 - d / width);
                let i = y as usize * self.w as usize + x as usize;
                let cur = self.v[i];
                let after = (f64::from(cur) + (target - f64::from(cur)) * k) as f32;
                if after != cur {
                    self.v[i] = after;
                    self.touch(x as u32, y as u32);
                }
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
        if self.editing {
            return Err("edit already open".to_owned());
        }

        self.editing = true;
        self.touched = false;
        self.snapshot = self.v.clone();
        Ok(())
    }

    /// Stores the touched rectangle of the open operation.
    pub fn end_edit(&mut self) {
        if !self.editing {
            return;
        }

        self.editing = false;
        if !self.touched {
            return;
        }

        let mut r = Record { x0: self.tx0, y0: self.ty0, x1: self.tx1, y1: self.ty1, ..Default::default() };
        for y in self.ty0..self.ty1 {
            for x in self.tx0..self.tx1 {
                let i = y as usize * self.w as usize + x as usize;
                r.before.push(self.snapshot[i]);
                r.after.push(self.v[i]);
            }
        }

        self.undo.push(r);
        self.redo.clear();
        self.snapshot.clear();
    }

    fn put(&mut self, r: &Record, values: &[f32]) {
        let mut it = values.iter();
        for y in r.y0..r.y1 {
            for x in r.x0..r.x1 {
                self.v[y as usize * self.w as usize + x as usize] = *it.next().expect("a value per touched sample");
            }
        }
    }

    pub fn undo(&mut self) -> bool {
        let Some(r) = self.undo.pop() else { return false };
        self.put(&r, &r.before);
        self.redo.push(r);
        true
    }

    pub fn redo(&mut self) -> bool {
        let Some(r) = self.redo.pop() else { return false };
        self.put(&r, &r.after);
        self.undo.push(r);
        true
    }

    pub fn is_editing(&self) -> bool {
        self.editing
    }

    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }

    pub fn edit_depth(&self) -> usize {
        self.undo.len()
    }

    /// The 16-bit samples in the field's range.
    pub fn samples(&self) -> Vec<u16> {
        self.quantized(self.min, self.scale)
    }
}

/// Four octaves of value noise at (x, y) (one lattice cell a unit at the first octave), -1 .. 1, the octaves halving in weight.
pub fn fractal_noise(x: f64, y: f64) -> f64 {
    fn hash(i: i64, j: i64) -> f64 {
        let mut h = (i as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15) ^ (j as u64).wrapping_mul(0xC2B2_AE3D_27D4_EB4F);
        h ^= h >> 31;
        h = h.wrapping_mul(0xBF58_476D_1CE4_E5B9);
        h ^= h >> 29;
        (h >> 11) as f64 / (1u64 << 53) as f64 * 2.0 - 1.0
    }

    fn value(x: f64, y: f64) -> f64 {
        let (i, j) = (x.floor() as i64, y.floor() as i64);
        let (fx, fy) = (x - x.floor(), y - y.floor());
        let s = |t: f64| t * t * (3.0 - 2.0 * t);
        let (sx, sy) = (s(fx), s(fy));
        let a = hash(i, j) + (hash(i + 1, j) - hash(i, j)) * sx;
        let b = hash(i, j + 1) + (hash(i + 1, j + 1) - hash(i, j + 1)) * sx;
        a + (b - a) * sy
    }

    let (mut sum, mut amp, mut freq, mut norm) = (0.0, 1.0, 1.0, 0.0);
    for octave in 0..4 {
        // Each octave shifted, so that the lattices do not line up.
        let shift = f64::from(octave) * 17.31;
        sum += amp * value(x * freq + shift, y * freq - shift);
        norm += amp;
        amp *= 0.5;
        freq *= 2.0;
    }

    sum / norm
}

/// A height as a 16-bit sample of the range (min_height, scale): rounded to nearest, clamped.
fn quantize(m: f64, min_height: f64, scale: f64) -> u16 {
    let r = (m - min_height) / scale * 65535.0;
    r.clamp(0.0, 65535.0).round() as u16
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_noise_is_bounded_repeatable_and_not_flat() {
        let samples = (0..4000).map(|i| fractal_noise(f64::from(i % 63) * 0.37, f64::from(i / 63) * 0.41)).collect::<Vec<_>>();
        assert!(samples.iter().all(|v| (-1.0..=1.0).contains(v)));
        assert_eq!(fractal_noise(3.3, -7.1), fractal_noise(3.3, -7.1));
        let mean = samples.iter().sum::<f64>() / samples.len() as f64;
        let spread = samples.iter().map(|v| (v - mean).powi(2)).sum::<f64>() / samples.len() as f64;
        assert!(mean.abs() < 0.1 && spread.sqrt() > 0.1, "mean {mean}, deviation {}", spread.sqrt());
    }

    #[test]
    fn a_noise_stroke_roughens_inside_the_brush_only() {
        let mut f = HeightField::flat(64, 64, 10.0);
        f.begin_edit().unwrap();
        f.noise(&Brush { cx: 32.0, cy: 32.0, radius: 12.0, strength: 1.0, falloff: 0.5, ..Default::default() }, 4.0, 4.0).unwrap();
        f.end_edit();
        let inside = (24..40).flat_map(|y| (24..40).map(move |x| (x, y))).map(|(x, y)| (f.metres(x, y) - 10.0).abs()).fold(0.0, f64::max);
        assert!(inside > 0.5, "inside {inside}");
        assert_eq!(f.metres(2, 2), 10.0);
        assert!(f.undo() && f.metres(32, 32) == 10.0);
    }
}
