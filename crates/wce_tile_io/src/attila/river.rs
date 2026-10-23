//! A battle tile's river (BOB: ToolDataBuilderDll `FUN_1001c610`): the splines of `river_spline<n>.xml`, tessellated by
//! `WS_SCENE_NODE_DYNAMIC::tesselate_river_spline` (Warscape.AssemblyKit.dll) into a strip five vertices wide, written as the meshes'
//! type-90 block and rasterised into `hf_water_map.data`. All f32 as BOB's SSE code orders it. docs/formats/tile_compile.md has the RE.

use wce_tile_model::{Node, Tile};

/// A segment of a river file (`WS_SCENE_NODE_DYNAMIC::RIVER_SPLINE`, as `FUN_10011a20` reads it): the cubic Bézier's four points
/// (z flipped into the mesh's), and per end (POINT0, POINT3) the foam, alpha fade, flow speed and width.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RiverSegment {
    pub points: [[f32; 3]; 4],
    pub foam: [f32; 2],
    pub alpha: [f32; 2],
    pub flow: [f32; 2],
    pub width: [f32; 2],
}

/// A river vertex as the RMV2 block stores it: x, y, z, flow speed, the width across (0 on the strip's first edge), the distance
/// along the river, alpha fade, the position across (0 to 1).
pub type RiverVertex = [f32; 8];

/// The rivers' mesh: vertices and triangles (indices into the vertices), every river in file order.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct RiverMesh {
    pub vertices: Vec<RiverVertex>,
    pub triangles: Vec<[u32; 3]>,
}

/// `CA::UniString::parse(float &)` (CALibs.AssemblyKit.dll), which `XML_VALUE::as_float32` reads attributes with: in f32, the
/// integer digits as `v x 10 + d`, each fraction digit `d x scale + v` with the scale times 0.1 (f32) each time, an exponent by
/// repeated multiplies by 10 or 0.1, the sign last. Not correctly rounded: 100.731 is an ulp off `strtof`'s. Anything left over
/// (spaces, a `+`) fails it.
pub fn ca_parse_f32(text: &str) -> Option<f32> {
    let b = text.as_bytes();
    let (neg, mut i) = if b.first() == Some(&b'-') { (true, 1) } else { (false, 0) };
    let tenth = f32::from_bits(0x3dcc_cccd);
    let mut v = 0.0f32;
    while let Some(d @ b'0'..=b'9') = b.get(i) {
        v = f32::from(d - b'0') + v * 10.0;
        i += 1;
    }

    if b.get(i) == Some(&b'.') {
        i += 1;
        let mut scale = 1.0f32;
        while let Some(d @ b'0'..=b'9') = b.get(i) {
            scale *= tenth;
            v += f32::from(d - b'0') * scale;
            i += 1;
        }
    }

    if matches!(b.get(i), Some(b'e' | b'E')) {
        i += 1;
        let minus = b.get(i) == Some(&b'-');
        if minus {
            i += 1;
        }

        let mut n = 0u32;
        while let Some(d @ b'0'..=b'9') = b.get(i) {
            n = n.wrapping_mul(10).wrapping_add(u32::from(d - b'0'));
            i += 1;
        }

        for _ in 0..n {
            v *= if minus { tenth } else { 10.0 };
        }
    }

    (i == b.len()).then_some(if neg { -v } else { v })
}

/// An attribute as BOB reads it: its text (as the file had it, else `%g`) through [`ca_parse_f32`]; 0 when that fails.
fn f32_attr(n: &Node, k: &str) -> f32 {
    let Some(p) = n.props().iter().find(|p| p.name == k) else { return 0.0 };
    let text = n.verbatim(p.name).map(str::to_owned).or_else(|| p.value.as_f64().map(wce_formats::c_num::printf_g)).or_else(|| p.value.as_str().map(str::to_owned));
    text.and_then(|t| ca_parse_f32(&t)).unwrap_or(0.0)
}

/// The project's rivers (`FUN_10011a20`): `river_spline0.xml`, `river_spline1.xml`, ... up to the first missing one; per `SPLINE`
/// its `POINT0` .. `POINT3`. z becomes `((-((z + 128) - half)) + half) - 128` with half the tile's `(height + 2) x 128 / 2`.
pub fn river_splines(raw: &Tile, cells_y: usize) -> Vec<Vec<RiverSegment>> {
    let half = ((cells_y + 2) * 128) as f32 * 0.5;
    let flip = |z: f32| ((-((z + 128.0) - half)) + half) - 128.0;
    let mut out = vec![];
    for n in 0.. {
        let Some(file) = raw.rivers.iter().find(|r| r.index == Some(n)) else { break };
        let segments = file
            .root
            .children
            .iter()
            .filter(|s| s.class == "SPLINE")
            .filter_map(|s| {
                let p = [s.child("POINT0")?, s.child("POINT1")?, s.child("POINT2")?, s.child("POINT3")?];
                let ends = |k: &str| [f32_attr(p[0], k), f32_attr(p[3], k)];
                Some(RiverSegment {
                    points: p.map(|q| [f32_attr(q, "x"), f32_attr(q, "y"), flip(f32_attr(q, "z"))]),
                    foam: ends("foam_amount"),
                    alpha: ends("alpha_fade"),
                    flow: ends("flow_speed"),
                    width: ends("width"),
                })
            })
            .collect();
        out.push(segments);
    }

    out
}

/// `UTILITYDLL::SEGMENTED_SPLINE_3`: cubic Bézier segments, their lengths, where each starts along the whole and the total.
struct Spline {
    segments: Vec<[[f32; 3]; 4]>,
    lengths: Vec<f32>,
    starts: Vec<f32>,
    total: f32,
}

/// The Bézier basis BOB weights the four points with (`FUN_106975d0`): point j's weight is `t^3, t^2, t, 1` dotted with row j.
const BASIS: [[f32; 4]; 4] = [[-1.0, 3.0, -3.0, 1.0], [3.0, -6.0, 3.0, 0.0], [-3.0, 3.0, 0.0, 0.0], [1.0, 0.0, 0.0, 0.0]];

/// `FUN_1021c320`: the points weighted by `powers` (t^3, t^2, t, 1 for the position; 3t^2, 2t, 1, 0 for the tangent).
fn weigh(p: &[[f32; 3]; 4], powers: [f32; 4]) -> [f32; 3] {
    let w = BASIS.map(|row| ((powers[0] * row[0] + powers[1] * row[1]) + powers[2] * row[2]) + powers[3] * row[3]);
    [0, 1, 2].map(|k| ((p[0][k] * w[0] + p[1][k] * w[1]) + p[2][k] * w[2]) + p[3][k] * w[3])
}

fn position_powers(t: f32) -> [f32; 4] {
    [(t * t) * t, t * t, t, 1.0]
}

fn tangent_powers(t: f32) -> [f32; 4] {
    [(3.0 * t) * t, 2.0 * t, 1.0, 0.0]
}

fn same(a: [f32; 3], b: [f32; 3]) -> bool {
    a[0] == b[0] && a[1] == b[1] && a[2] == b[2]
}

fn mid(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [0, 1, 2].map(|k| (a[k] + b[k]) * 0.5)
}

impl Spline {
    fn new() -> Self {
        Self { segments: vec![], lengths: vec![], starts: vec![], total: 0.0 }
    }

    /// `FUN_10379db0`: a segment, a repeated end point replaced by the middle of its neighbours (both: a straight line); its length a
    /// 1000-step sum of the tangent's length (`FUN_1037acb0`: t from 0 by 1 / 1000 while below 1, sqrtss, times the step), a
    /// straight one's the distance between its ends.
    fn push(&mut self, mut p: [[f32; 3]; 4]) {
        let (first, last) = (same(p[0], p[1]), same(p[2], p[3]));
        let straight = first || last;
        if first && !last {
            p[1] = mid(p[0], p[2]);
        } else if !first && last {
            p[2] = mid(p[3], p[1]);
        } else if first && last {
            p[1] = mid(p[0], p[3]);
            p[2] = p[1];
        }

        self.segments.push(p);
        let length = if straight {
            let d = [0, 1, 2].map(|k| p[0][k] - p[3][k]);
            ((d[0] * d[0] + d[1] * d[1]) + d[2] * d[2]).sqrt()
        } else {
            let step = 1.0f32 / 1000.0;
            let (mut t, mut sum) = (0.0f32, 0.0f32);
            while 1.0 > t {
                let d = weigh(&p, tangent_powers(t));
                sum += ((d[0] * d[0] + d[1] * d[1]) + d[2] * d[2]).sqrt();
                t += step;
            }

            sum * step
        };
        self.lengths.push(length);
        self.starts.push(self.total);
        self.total += length;
    }

    /// `find`: the segment whose share of the total holds `u` (the last one otherwise).
    fn find(&self, u: f32) -> usize {
        if self.total == 0.0 {
            return 0;
        }

        let mut acc = 0.0f32;
        for (i, &l) in self.lengths.iter().enumerate() {
            let share = l / self.total;
            if acc <= u && u <= share + acc {
                return i;
            }

            acc += share;
        }

        self.lengths.len() - 1
    }

    /// The segment and its own parameter at `u` (0 to 1 along the whole).
    fn local(&self, u: f32) -> (usize, f32) {
        let u = if u.abs() > 1.0 { u % 1.0 } else { u };
        let u = if u < 0.0 { 1.0 + u } else { u };
        let i = self.find(u);
        (i, ((f64::from(u * self.total - self.starts[i])) / f64::from(self.lengths[i])) as f32)
    }

    fn position(&self, u: f32) -> [f32; 3] {
        let (i, t) = self.local(u);
        weigh(&self.segments[i], position_powers(t))
    }

    fn tangent(&self, u: f32) -> [f32; 3] {
        let (i, t) = self.local(u);
        weigh(&self.segments[i], tangent_powers(t))
    }

    /// `SEGMENTED_SPLINE_3::optimise_spline`: of `n = floor(total x density + 0.5)` evenly spaced points (`k / (n - 1)`, x87), keep
    /// `(i - 1) / (n - 1)` where the direction from the last kept point to point `last` and that from `i - 1` to `i` differ (dot below
    /// `1 - tolerance`); 0 and 1 around them, `extra` merged in, sorted, repeats dropped.
    fn optimise(&self, density: f32, extra: &[f32], tolerance: f32) -> Vec<f32> {
        let n = (self.total * density + 0.5).floor() as i64 as u32;
        let mut out = vec![0.0f32];
        if n > 3 {
            let limit = 1.0 - tolerance;
            let span = (n - 1) as f32;
            let at = |k: u32| (f64::from(k) / f64::from(span)) as f32;
            let unit = |a: [f32; 3], b: [f32; 3]| {
                let d = [b[0] - a[0], b[1] - a[1], b[2] - a[2]];
                let inv = 1.0 / ((d[1] * d[1] + d[0] * d[0]) + d[2] * d[2]).sqrt();
                [d[0] * inv, d[1] * inv, d[2] * inv]
            };
            let mut last = 1;
            for i in 2..n {
                let back = *out.last().expect("0 first");
                let n1 = unit(self.position(back), self.position(at(last)));
                let previous = at(i - 1);
                let (c, d) = (self.position(previous), self.position(at(i)));
                let d2 = [d[0] - c[0], d[1] - c[1], d[2] - c[2]];
                let inv = 1.0 / ((d2[1] * d2[1] + d2[0] * d2[0]) + d2[2] * d2[2]).sqrt();
                let dot = (d2[1] * inv * n1[1] + d2[0] * inv * n1[0]) + d2[2] * inv * n1[2];
                if limit > dot {
                    out.push(previous);
                    last = i;
                }
            }
        }

        out.push(1.0);
        if !extra.is_empty() {
            out.extend_from_slice(extra);
            out.sort_by(f32::total_cmp);
            // `std::unique` without the erase: the values move to the front, the tail keeps what was there (with 0 in both lists,
            // the last sample comes twice).
            let mut kept = 0;
            for i in 1..out.len() {
                if out[kept] != out[i] {
                    kept += 1;
                    out[kept] = out[i];
                }
            }
        }

        out
    }
}

/// `tesselate_river_spline`: per river, its spline sampled where `optimise_spline` (density 20, tolerance 0.02) keeps points,
/// merged with 0, 1/8 .. 6/8; at each, five vertices across the width (the tangent's ground direction turned left, from -width / 2 to
/// width / 2), the segment's foam / alpha / flow / width lerped by the segment's own parameter (clamped); then two triangles per
/// square, (next c, this c, next c + 1) and (this c, this c + 1, next c + 1).
pub fn tessellate(rivers: &[Vec<RiverSegment>]) -> RiverMesh {
    const ACROSS: usize = 5;
    let fixed = (0..7).scan(0.0f32, |v, _| {
        let out = *v;
        *v += 1.0 / 8.0;
        Some(out)
    });
    let fixed = fixed.collect::<Vec<_>>();
    let mut mesh = RiverMesh::default();
    for river in rivers {
        let mut spline = Spline::new();
        for s in river {
            spline.push(s.points);
        }

        if spline.segments.is_empty() {
            continue;
        }

        let samples = spline.optimise(20.0, &fixed, f32::from_bits(0x3ca3_d70a));
        let base = mesh.vertices.len() as u32;
        for &u in &samples {
            let i = spline.find(u);
            let s = &river[i];
            let along = u * spline.total;
            let c = ((f64::from(along - spline.starts[i])) / f64::from(spline.lengths[i])) as f32;
            let c = c.clamp(0.0, 1.0);
            let lerp = |e: [f32; 2]| (e[1] - e[0]) * c + e[0];
            let (alpha, width, flow) = (lerp(s.alpha), lerp(s.width), lerp(s.flow));
            let p = spline.position(u);
            let half = 0.5 * width;
            let t = spline.tangent(u);
            let (mut dx, mut dz) = (t[0], t[2]);
            let len2 = dx * dx + dz * dz;
            if 0.0 < len2 {
                let inv = 1.0 / len2.sqrt();
                dx *= inv;
                dz *= inv;
            }

            for b in 0..ACROSS {
                let across = (f64::from(b as u32) / f64::from((ACROSS - 1) as u32)) as f32;
                let s = (half - -half) * across + -half;
                let off_z = s * -dx + 0.0 * dx;
                let off_x = s * dz + 0.0 * dz;
                mesh.vertices.push([p[0] + off_x, p[1], p[2] + off_z, flow, across * width, u * spline.total, alpha, across]);
            }
        }

        let a = ACROSS as u32;
        for row in 0..samples.len() as u32 - 1 {
            for c in 0..a - 1 {
                let (this, next) = (base + row * a + c, base + (row + 1) * a + c);
                mesh.triangles.push([next, this, next + 1]);
                mesh.triangles.push([this, this + 1, next + 1]);
            }
        }
    }

    mesh
}

/// IEEE half to f32 (exact).
fn f16(bits: u16) -> f32 {
    let (sign, exp, frac) = (u32::from(bits >> 15), u32::from((bits >> 10) & 0x1f), u32::from(bits & 0x3ff));
    let v = match exp {
        0 => frac as f32 * (1.0 / 16_777_216.0),
        31 => if frac == 0 { f32::INFINITY } else { f32::NAN },
        _ => f32::from_bits(((exp + 112) << 23) | (frac << 13)),
    };
    if sign == 1 { -v } else { v }
}

/// `FUN_10088840`: whether sample `(x, z)` lies in triangle (a, b, c) (x, z of each) by the crossing rule over the edges (c, b),
/// (a, c), (b, a), each crossing `x' = (z - z0) (x1 - x0) / (z1 - z0) + x0` (in that order of operations) right of x.
fn crossing(x: i32, z: i32, a: [f32; 3], b: [f32; 3], c: [f32; 3]) -> bool {
    let (px, pz) = (x as f32, z as f32);
    let mut inside = false;
    if (c[2] > pz) != (b[2] > pz) && (pz - c[2]) * (b[0] - c[0]) / (b[2] - c[2]) + c[0] > px {
        inside = true;
    }
    if (a[2] > pz) != (c[2] > pz) && (c[0] - a[0]) * (pz - a[2]) / (c[2] - a[2]) + a[0] > px {
        inside = !inside;
    }
    if (b[2] > pz) != (a[2] > pz) && (pz - b[2]) * (a[0] - b[0]) / (a[2] - b[2]) + b[0] > px {
        inside = !inside;
    }

    inside
}

/// `TOOLDATABUILDER::rasterized_water_planes` (ToolDataBuilderDll `FUN_10088960`): every `SIMPLE_MESH_INSTANCE` (its model through
/// `model`, BOB's VFS; TEd's world matrix) into `field` (`w` x `h`, row = z, not flipped). Per model: RMV2, the first LOD's first
/// block, which must be a water block (type 83: else BOB stops here); its pivot (block + 0x4b4 before version 6, + 0x274 after);
/// vertices of 12 bytes from f16 x, y, z, w; u16 triangles. The matrix moved by the pivot (`t = ((m0 px + m1 py) + m2 pz) + m3` per
/// row), each vertex `((r0 x + r1 y) + r2 z) + t w` per row, x and z halved into metres. Per triangle (idx1, idx2, idx0 as a, b, c),
/// every whole sample of its truncated box widened by one that [`crossing`] keeps takes the height interpolated by the areas
/// (`FUN_100140b0`'s sub-triangle cross products; a's weight rounded to f32, the sum on the x87), overwriting.
pub fn rasterise_water_planes(field: &mut [f32], w: usize, h: usize, density: usize, planes: &[(String, [f32; 16])], model: &dyn Fn(&str) -> Option<Vec<u8>>) {
    // The sample of a metre: `(1 / (128 / density)) x m`, truncated.
    let per_metre = 1.0f32 / (128.0f32 / density as f32);
    let u32_at = |b: &[u8], o: usize| b.get(o..o + 4).map(|s| u32::from_le_bytes(s.try_into().expect("4 bytes")) as usize);
    let f32_at = |b: &[u8], o: usize| b.get(o..o + 4).map(|s| f32::from_le_bytes(s.try_into().expect("4 bytes")));
    for (key, m) in planes {
        let Some(b) = model(key) else { return };
        if b.get(..4) != Some(b"RMV2") {
            return;
        }

        let version = u32_at(&b, 4).unwrap_or(0);
        if u32_at(&b, 8).unwrap_or(0) == 0 {
            continue;
        }

        let (count_at, offset_at, pivot_at) = if version < 6 { (0x10c, 0x118, 0x4b4) } else { (0x8c, 0x98, 0x274) };
        if u32_at(&b, count_at).unwrap_or(0) == 0 {
            continue;
        }

        let Some(block) = u32_at(&b, offset_at) else { return };
        if b.get(block..block + 2) != Some(&[0x53, 0]) {
            return;
        }

        let (Some(px), Some(py), Some(pz)) = (f32_at(&b, block + pivot_at), f32_at(&b, block + pivot_at + 4), f32_at(&b, block + pivot_at + 8)) else { return };
        let (Some(verts), Some(indices), Some(count)) = (u32_at(&b, block + 8), u32_at(&b, block + 0x10), u32_at(&b, block + 0x14)) else { return };
        let t = [0, 1, 2, 3].map(|k| ((m[4 * k] * px + m[4 * k + 1] * py) + m[4 * k + 2] * pz) + m[4 * k + 3]);
        let vertex = |i: usize| -> Option<[f32; 3]> {
            let o = block + verts + i * 12;
            let v = [0, 1, 2, 3].map(|k| b.get(o + 2 * k..o + 2 * k + 2).map(|s| f16(u16::from_le_bytes([s[0], s[1]]))));
            let [Some(x), Some(y), Some(z), Some(wv)] = v else { return None };
            let row = |k: usize| ((m[4 * k] * x + m[4 * k + 1] * y) + m[4 * k + 2] * z) + t[k] * wv;
            Some([row(0) * 0.5, row(1), row(2) * 0.5])
        };
        let index = |k: usize| b.get(block + indices + 2 * k..block + indices + 2 * k + 2).map(|s| usize::from(u16::from_le_bytes([s[0], s[1]])));
        if count == 0 {
            continue;
        }

        for tri in 0..(count - 1) / 3 + 1 {
            let (Some(i0), Some(i1), Some(i2)) = (index(3 * tri), index(3 * tri + 1), index(3 * tri + 2)) else { return };
            let (Some(c), Some(a), Some(bv)) = (vertex(i0), vertex(i1), vertex(i2)) else { return };
            let xs = [c[0] as i32, a[0] as i32, bv[0] as i32];
            let zs = [c[2] as i32, a[2] as i32, bv[2] as i32];
            let (x0, x1) = (xs.iter().min().expect("3") - 1, xs.iter().max().expect("3") + 1);
            let (z0, z1) = (zs.iter().min().expect("3") - 1, zs.iter().max().expect("3") + 1);
            // The whole triangle's area (the y components of the cross product are 0, as BOB's 3D product with y = 0 has them).
            let (e0x, e0z, e1x, e1z) = (a[0] - c[0], a[2] - c[2], bv[0] - c[0], bv[2] - c[2]);
            let cross = |p: f32, q: f32, r: f32| f64::from((p * p + q * q) + r * r).sqrt() * 0.5;
            let total = cross(e1z * 0.0 - e0z * 0.0, e1x * e0z - e0x * e1z, e0x * 0.0 - e1x * 0.0);
            let inv = (1.0 / total) as f32;
            for z in z0..z1 {
                for x in x0..x1 {
                    if !crossing(x, z, a, bv, c) {
                        continue;
                    }

                    let (fx, fz) = (x as f32, z as f32);
                    let (dz, dx) = (fz - c[2], fx - c[0]);
                    let ua = (cross(e1z * 0.0 - dz * 0.0, dz * e1x - dx * e1z, dx * 0.0 - e1x * 0.0) * f64::from(inv)) as f32;
                    let ub = cross(e0z * 0.0 - dz * 0.0, dz * e0x - dx * e0z, dx * 0.0 - e0x * 0.0) * f64::from(inv);
                    let height = ((f64::from(a[1]) * f64::from(ua) + f64::from(bv[1]) * ub) + f64::from(c[1]) * ((1.0 - f64::from(ua)) - ub)) as f32;
                    let (sx, sz) = (per_metre * fx, per_metre * fz);
                    if sx < 0.0 || sx >= w as f32 || sz < 0.0 || sz >= h as f32 {
                        continue;
                    }

                    field[sz as usize * w + sx as usize] = height;
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn numbers_as_ca_parses_them() {
        // 100.731: CA's digit by digit sum lands an ulp below the correctly rounded f32.
        assert_eq!(ca_parse_f32("100.731"), Some(f32::from_bits(0x42c9_7645)));
        assert_ne!(ca_parse_f32("100.731"), Some(100.731));
        assert_eq!(ca_parse_f32("-0.5"), Some(-0.5));
        assert_eq!(ca_parse_f32("35"), Some(35.0));
        assert_eq!(ca_parse_f32("1e2"), Some(100.0));
        assert_eq!(ca_parse_f32("+1"), None);
        assert_eq!(ca_parse_f32("1 "), None);
    }

    #[test]
    fn halves() {
        assert_eq!([0x3c00, 0xc500, 0x0000, 0x3800, 0x0001].map(f16), [1.0, -5.0, 0.0, 0.5, 1.0 / 16_777_216.0]);
    }

    #[test]
    fn a_straight_river() {
        // One straight segment 10 m long along x, 4 m wide (across from the left bank, z + 2): samples at 0, 1/8 .. 6/8 and 1 (twice: BOB's unique keeps the tail).
        let p = |x: f32| [x, 0.0, 50.0];
        let seg = RiverSegment { points: [p(0.0), p(10.0 / 3.0), p(20.0 / 3.0), p(10.0)], foam: [0.0; 2], alpha: [1.0; 2], flow: [0.5; 2], width: [4.0; 2] };
        let mesh = tessellate(&[vec![seg]]);
        assert_eq!(mesh.vertices.len(), 9 * 5);
        assert_eq!(mesh.triangles.len(), 8 * 4 * 2);
        let first = &mesh.vertices[..5];
        assert_eq!(first.iter().map(|v| v[2]).collect::<Vec<_>>(), [52.0, 51.0, 50.0, 49.0, 48.0]);
        assert_eq!(mesh.vertices[40][5], mesh.vertices[35][5]);
    }
}
