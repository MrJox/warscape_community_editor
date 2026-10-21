//! The battle sea's surfaces as the game ships and plays them (docs/re/water.md, "The battle sea of a coastal battle"): the wind level's
//! row of `sea_surfaces_tables` and its two prebaked surfaces, `wind_level_<n>_sea` (256 x 256, a 70 m patch, 100 frames over 10 s) and
//! `wind_level_<n>_swell` (128 x 128, 800 m, 60 frames over 30 s), files at the root of data.pack.
//!
//! A surface file (read by fn 0x116889f0 / 0x1167fcd0): magic 0x12345678, version 1, `n`, the patch size, the loop length, the frame
//! count, two flag bytes, the height range (min, max: the shader's `g_vs_sea_info.zw` are min and max - min), the u16 range of the two
//! angle channels (min x, min z, max x, max z), the range of the two slope channels (4 floats), then the frames: a height image (1 byte a
//! texel), an angle image (2 bytes: x, z), a slope image (2 bytes, empty for the swell), each `w, h` first, and two lists of records:
//! 16 bytes (x, y: i32, the foam at this frame and at the next: f32, the whitecaps) and 8 bytes (none in the shipped files).
//!
//! What the game does with them, per frame (fn 0x117693a0, the jobs set up by fn 0x11794880): the frame pair and blend of the time
//! (fn 0x1169a360: `t mod loop * frames / loop`, its whole part and the fraction), and each surface's texture: the angle bytes of both
//! frames decoded to u16 (`min + byte * (max - min) / 255` per channel) and blended (R16G16; checked against a memory dump: exact to one
//! u16 on the rows the dump did not catch mid-update). Once, when the sea is created (fn 0x11769000): the whitecap array, a slice per sea
//! frame, red the blended height bytes and green the foam of a simulation run over three loops (fn 0x11794660: each frame's records
//! stamp `max(foam, a + (b - a) * w)`, fn 0x11719330 then multiplies every cell above 0.001 by `foam_decay_rate` and writes
//! `round(foam * 255 + 0.5)`).

use rayon::prelude::*;

/// A row of `sea_surfaces_tables` (version 2), the fields the editor uses.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SeaRow {
    pub key: String,
    pub foam_enabled: bool,
    pub foam_decay_rate: f32,
    pub froth_distortion_speed: f32,
    pub froth_distortion_amount: f32,
    pub sea_decay: f32,
    pub wave_trough_y_value: f32,

    /// The four `detail_normal_uv_*` columns in the file's order (RPFM names them scale1, speed1, scale2, speed2).
    pub detail_normal: [f32; 4],
}

impl SeaRow {
    /// `OceanSharedConstants.g_detail_normal_uv_scale1, scale2, speed1, speed2` from the row's columns: the engine copies them in another
    /// order than RPFM's names suggest (the dump of a calm battle: 15.8, 5, 7.8, 6.8 against the row's 7.8, 6.8, 15.8, 5).
    pub fn detail_normal_constants(&self) -> [f32; 4] {
        let d = self.detail_normal;
        [d[2], d[3], d[0], d[1]]
    }
}

/// A row of `wind_levels_tables`: the battle's wind and the sea surface it uses.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct WindLevel {
    pub key: String,
    pub sea_surface: String,
    pub sort_order: i32,
}

/// A little-endian reader over a byte slice.
struct Reader<'a> {
    b: &'a [u8],
    p: usize,
}

impl<'a> Reader<'a> {
    fn take(&mut self, n: usize) -> Option<&'a [u8]> {
        let s = self.b.get(self.p..self.p.checked_add(n)?)?;
        self.p += n;
        Some(s)
    }

    fn u8(&mut self) -> Option<u8> {
        self.take(1).map(|s| s[0])
    }

    fn u16(&mut self) -> Option<u16> {
        self.take(2).map(|s| u16::from_le_bytes([s[0], s[1]]))
    }

    fn u32(&mut self) -> Option<u32> {
        self.take(4).map(|s| u32::from_le_bytes([s[0], s[1], s[2], s[3]]))
    }

    fn f32(&mut self) -> Option<f32> {
        self.u32().map(f32::from_bits)
    }

    /// A DB string: u16 length, then the bytes.
    fn string_u8(&mut self) -> Option<String> {
        let n = self.u16()? as usize;
        self.take(n).map(|s| String::from_utf8_lossy(s).into_owned())
    }

    /// A DB table's header: the optional GUID and version markers, a byte, the row count.
    fn db_header(&mut self) -> Option<u32> {
        if self.b.get(self.p..self.p + 4) == Some(&[253, 254, 252, 255]) {
            self.p += 4;
            let n = self.u16()? as usize;
            self.take(n * 2)?;
        }

        if self.b.get(self.p..self.p + 4) == Some(&[252, 253, 254, 255]) {
            self.p += 8;
        }

        self.u8()?;
        self.u32()
    }
}

/// The rows of `db/sea_surfaces_tables/*` (version 2: key, five floats, foam_enabled, seventeen floats).
pub fn parse_sea_surfaces(bytes: &[u8]) -> Option<Vec<SeaRow>> {
    let mut r = Reader { b: bytes, p: 0 };
    let count = r.db_header()?;
    let mut rows = Vec::with_capacity(count as usize);
    for _ in 0..count {
        let key = r.string_u8()?;
        for _ in 0..5 {
            r.f32()?;
        }

        let foam_enabled = r.u8()? != 0;
        let mut f = [0.0f32; 17];
        for v in &mut f {
            *v = r.f32()?;
        }

        // foam_decay_rate, foam_fade_in_time, foam_cut_off_value, froth_value, froth_distortion_speed, froth_distortion_amount,
        // spray_cut_off_value, spray_speed, spray_duration, sea_shininess, sea_decay, reflection_flattening_factor, wave_trough_y_value,
        // the four detail normal columns.
        rows.push(SeaRow {
            key, foam_enabled, foam_decay_rate: f[0], froth_distortion_speed: f[4], froth_distortion_amount: f[5], sea_decay: f[10],
            wave_trough_y_value: f[12], detail_normal: [f[13], f[14], f[15], f[16]],
        });
    }

    Some(rows)
}

/// The rows of `db/wind_levels_tables/*` (version 0: key, sea_surface, magnitudeX, magnitudeY, sort_order).
pub fn parse_wind_levels(bytes: &[u8]) -> Option<Vec<WindLevel>> {
    let mut r = Reader { b: bytes, p: 0 };
    let count = r.db_header()?;
    let mut rows = Vec::with_capacity(count as usize);
    for _ in 0..count {
        let key = r.string_u8()?;
        let sea_surface = r.string_u8()?;
        r.f32()?;
        r.f32()?;
        let sort_order = r.u32()? as i32;
        rows.push(WindLevel { key, sea_surface, sort_order });
    }

    rows.sort_by_key(|w| w.sort_order);
    Some(rows)
}

/// A whitecap record: the cell and its foam at this frame and at the next.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct FoamRecord {
    pub x: i32,
    pub y: i32,
    pub a: f32,
    pub b: f32,
}

#[derive(Clone, Debug, Default)]
pub struct SurfaceFrame {
    /// n x n height bytes.
    pub heights: Vec<u8>,

    /// n x n angle byte pairs (x, z).
    pub angles: Vec<u8>,
    pub foam: Vec<FoamRecord>,
}

/// A prebaked sea or swell surface.
#[derive(Clone, Debug, Default)]
pub struct SeaSurface {
    pub n: usize,
    pub patch: f32,
    pub loop_seconds: f32,
    pub height_min: f32,
    pub height_max: f32,
    pub angle_min: [u16; 2],
    pub angle_max: [u16; 2],
    pub frames: Vec<SurfaceFrame>,
}

/// A frame of a surface at a time: the frame, the next one and the blend between them.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct FrameAt {
    pub frame: usize,
    pub next: usize,
    pub blend: f32,
}

impl SeaSurface {

    /// Reads a surface file; `None` when it isn't one.
    pub fn parse(bytes: &[u8]) -> Option<Self> {
        let mut r = Reader { b: bytes, p: 0 };
        if r.u32()? != 0x1234_5678 || r.u32()? != 1 {
            return None;
        }

        let n = r.u32()? as usize;
        let patch = r.f32()?;
        let loop_seconds = r.f32()?;
        let _frames = r.u32()?;
        r.u8()?;
        r.u8()?;
        let (height_min, height_max) = (r.f32()?, r.f32()?);
        let angle_min = [r.u16()?, r.u16()?];
        let angle_max = [r.u16()?, r.u16()?];
        for _ in 0..4 {
            r.f32()?;
        }

        let count = r.u32()? as usize;
        let mut frames = Vec::with_capacity(count);
        for _ in 0..count {
            let mut image = |bpp: usize| -> Option<Vec<u8>> {
                let (w, h) = (r.u32()? as usize, r.u32()? as usize);
                r.take(w.checked_mul(h)?.checked_mul(bpp)?).map(<[u8]>::to_vec)
            };

            let heights = image(1)?;
            let angles = image(2)?;
            let _slopes = image(2)?;
            let records = r.u32()? as usize;
            let mut foam = Vec::with_capacity(records);
            for _ in 0..records {
                foam.push(FoamRecord { x: r.u32()? as i32, y: r.u32()? as i32, a: r.f32()?, b: r.f32()? });
            }

            let other = r.u32()? as usize;
            r.take(other.checked_mul(8)?)?;
            if heights.len() != n * n || angles.len() != n * n * 2 {
                return None;
            }

            frames.push(SurfaceFrame { heights, angles, foam });
        }

        (!frames.is_empty()).then_some(Self { n, patch, loop_seconds, height_min, height_max, angle_min, angle_max, frames })
    }

    /// The frame pair at a time (fn 0x1169a360).
    pub fn frame_at(&self, t: f32) -> FrameAt {
        let count = self.frames.len().max(1);
        let looped = t.rem_euclid(self.loop_seconds.max(1e-3));
        let exact = looped * count as f32 / self.loop_seconds.max(1e-3);
        let frame = (exact as usize).min(count - 1);
        FrameAt { frame, next: (frame + 1) % count, blend: exact - frame as f32 }
    }

    /// The surface's texture at a frame pair: the blended angles, R16G16 (x, z) in rows.
    /// (Every frame: on the threads.)
    pub fn angle_texels(&self, at: FrameAt) -> Vec<u16> {
        let (a, b) = (&self.frames[at.frame].angles, &self.frames[at.next].angles);
        let scale = [0, 1].map(|c| (f32::from(self.angle_max[c]) - f32::from(self.angle_min[c])) / 255.0);
        a.par_iter().zip(b.par_iter()).enumerate().map(|(i, (&x, &y))| {
            let c = i & 1;
            let v = f32::from(self.angle_min[c]) + (f32::from(x) + (f32::from(y) - f32::from(x)) * at.blend) * scale[c];
            v.round().clamp(0.0, 65535.0) as u16
        }).collect()
    }

    /// The height at a world point (bilinear, the patch tiled), blended between the frame pair.
    pub fn height_at(&self, at: FrameAt, x: f32, z: f32) -> f32 {
        let n = self.n;
        let (fx, fz) = ((x / self.patch).rem_euclid(1.0) * n as f32, (z / self.patch).rem_euclid(1.0) * n as f32);
        let (i0, j0) = (fx as usize % n, fz as usize % n);
        let (i1, j1) = ((i0 + 1) % n, (j0 + 1) % n);
        let (u, v) = (fx.fract(), fz.fract());
        let (a, b) = (&self.frames[at.frame].heights, &self.frames[at.next].heights);
        let at_cell = |i: usize, j: usize| {
            let k = j * n + i;
            f32::from(a[k]) + (f32::from(b[k]) - f32::from(a[k])) * at.blend
        };

        let byte = (at_cell(i0, j0) * (1.0 - u) + at_cell(i1, j0) * u) * (1.0 - v) + (at_cell(i0, j1) * (1.0 - u) + at_cell(i1, j1) * u) * v;
        self.height_min + byte * (self.height_max - self.height_min) / 255.0
    }

    /// The whitecap array (fn 0x11769000): a slice per frame, RG8 (the blended heights, the foam), after the foam simulation has run
    /// over three loops at the frame rate.
    pub fn foam_slices(&self, decay: f32) -> Vec<u8> {
        let (n, count) = (self.n, self.frames.len());
        let mut foam = vec![0.0f32; n * n];
        let mut slices = vec![0u8; count * n * n * 2];
        let step = self.loop_seconds / count as f32;
        for i in 0..count * 3 {
            let at = self.frame_at(i as f32 * step);
            for r in &self.frames[at.frame].foam {
                let Some(cell) = (r.x >= 0 && r.y >= 0 && (r.x as usize) < n && (r.y as usize) < n).then(|| r.y as usize * n + r.x as usize) else { continue };
                let v = r.a + (r.b - r.a) * at.blend;
                foam[cell] = foam[cell].max(v);
            }

            let (a, b) = (&self.frames[at.frame].heights, &self.frames[at.next].heights);
            let slice = &mut slices[at.frame * n * n * 2..(at.frame + 1) * n * n * 2];
            for (k, f) in foam.iter_mut().enumerate() {
                let (ha, hb) = (f32::from(a[k]) / 255.0, f32::from(b[k]) / 255.0);
                slice[2 * k] = (((hb - ha) * at.blend + ha) * 255.0).round() as i32 as u8;
                slice[2 * k + 1] = if *f > 0.001 {
                    *f *= decay;
                    (*f * 255.0 + 0.5).round() as i32 as u8
                } else {
                    0
                };
            }
        }

        slices
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn surface(frames: usize, loop_seconds: f32) -> SeaSurface {
        SeaSurface { n: 2, patch: 1.0, loop_seconds, height_min: -1.0, height_max: 1.0, frames: vec![SurfaceFrame::default(); frames], ..Default::default() }
    }

    #[test]
    fn frames_at_times() {
        // The memory dump of a calm battle: g_foam_anim_data (33, 34, 0.65) at t = 13.365 s, a 10 s loop of 100 frames.
        let s = surface(100, 10.0);
        let at = s.frame_at(13.365);
        assert_eq!((at.frame, at.next), (33, 34));
        assert!((at.blend - 0.65).abs() < 1e-3);
        assert_eq!(s.frame_at(9.99).next, 0);
        assert_eq!(s.frame_at(-0.05).frame, 99);
    }
}
