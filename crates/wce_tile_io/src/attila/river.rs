//! A battle tile's river (BOB: ToolDataBuilderDll `FUN_1001c610`): the splines of `river_spline<n>.xml`, tessellated by
//! `WS_SCENE_NODE_DYNAMIC::tesselate_river_spline` (Warscape.AssemblyKit.dll) into a strip five vertices wide, written as the meshes'
//! type-90 block and rasterised into `hf_water_map.data`. All f32 as BOB's SSE code orders it. docs/formats/tile_compile.md has the RE.

use wce_tile_model::{Node, Tile};

pub use crate::conventions::RiverFormat;

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

/// A river vertex as the RMV2 block stores it ([`RiverFormat`]): Attila's x, y, z, flow speed, the width across (0 on the strip's
/// first edge), the distance along the river, alpha fade, the position across (0 to 1); Rome 2's the same values in another order.
pub type RiverVertex = [f32; 8];

/// The rivers' mesh: vertices and triangles (indices into the vertices), every river in file order, in a game's block format.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct RiverMesh {
    pub vertices: Vec<RiverVertex>,
    pub triangles: Vec<[u32; 3]>,
    pub format: RiverFormat,
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
/// square, (next c, this c, next c + 1) and (this c, this c + 1, next c + 1). The vertices in `format`'s order (Rome 2's: the same
/// rows, ASSUMPTION: its BOB lerps the foam as the other values).
pub fn tessellate(rivers: &[Vec<RiverSegment>], format: RiverFormat) -> RiverMesh {
    const ACROSS: usize = 5;
    let fixed = (0..7).scan(0.0f32, |v, _| {
        let out = *v;
        *v += 1.0 / 8.0;
        Some(out)
    });
    let fixed = fixed.collect::<Vec<_>>();
    let mut mesh = RiverMesh { format, ..Default::default() };
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
            let (alpha, width, flow, foam) = (lerp(s.alpha), lerp(s.width), lerp(s.flow), lerp(s.foam));
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
                let (x, z) = (p[0] + off_x, p[2] + off_z);
                mesh.vertices.push(match format {
                    RiverFormat::Attila => [x, p[1], z, flow, across * width, u * spline.total, alpha, across],
                    RiverFormat::Rome2 => [x, p[1], z, flow, across, u, alpha, foam],
                });
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

/// A row of a tessellated river: its centre (the vertex at 0.5 across), where it is along the river (0 to 1), and the values there.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RiverRow {
    pub centre: [f32; 3],
    pub along: f32,
    pub width: f32,
    pub flow: f32,
    pub alpha: f32,
    pub foam: f32,
}

/// The foam of a river whose block has none (Attila's): TEd's default.
const DEFAULT_FOAM: f32 = 0.1;

/// The rivers of a river block as [`tessellate`] made them: the strips (connected triangles) in the order of their first vertices, each
/// its rows of five vertices across in order along (a repeated row once). Rows without their five vertices are left out.
pub fn river_rows(mesh: &RiverMesh) -> Vec<Vec<RiverRow>> {
    fn root(p: &mut [usize], mut i: usize) -> usize {
        while p[i] != i {
            p[i] = p[p[i]];
            i = p[i];
        }

        i
    }

    let mut parent = (0..mesh.vertices.len()).collect::<Vec<_>>();
    for t in &mesh.triangles {
        let a = root(&mut parent, t[0] as usize);
        for &k in &t[1..] {
            let b = root(&mut parent, k as usize);
            parent[b] = a;
        }
    }

    // A vertex's place across (0 to 1) and along (Attila's in metres, Rome 2's 0 to 1).
    let place = |v: &RiverVertex| match mesh.format {
        RiverFormat::Attila => (v[7], v[5]),
        RiverFormat::Rome2 => (v[4], v[5]),
    };
    let used = mesh.triangles.iter().flatten().map(|&i| i as usize).collect::<std::collections::BTreeSet<_>>();
    let mut strips: Vec<(usize, Vec<usize>)> = vec![];
    for i in used {
        let r = root(&mut parent, i);
        match strips.iter_mut().find(|(k, _)| *k == r) {
            Some((_, v)) => v.push(i),
            None => strips.push((r, vec![i])),
        }
    }

    strips
        .into_iter()
        .filter_map(|(_, vs)| {
            let total = vs.iter().map(|&i| place(&mesh.vertices[i]).1).fold(0.0f32, f32::max);
            let mut by_along: std::collections::BTreeMap<u32, Vec<&RiverVertex>> = std::collections::BTreeMap::new();
            for &i in &vs {
                let v = &mesh.vertices[i];
                by_along.entry(place(v).1.to_bits()).or_default().push(v);
            }

            let mut rows = by_along
                .into_values()
                .filter_map(|mut row| {
                    row.sort_by(|a, b| place(a).0.total_cmp(&place(b).0));
                    row.dedup_by(|a, b| place(a).0 == place(b).0);
                    if row.len() != 5 {
                        return None;
                    }

                    let (c, e0, e4) = (row[2], row[0], row[4]);
                    let width = (e4[0] - e0[0]).hypot(e4[2] - e0[2]);
                    let (along, foam) = match mesh.format {
                        RiverFormat::Attila => (if total > 0.0 { c[5] / total } else { 0.0 }, DEFAULT_FOAM),
                        RiverFormat::Rome2 => (c[5], c[7]),
                    };
                    Some(RiverRow { centre: [c[0], c[1], c[2]], along, width, flow: c[3], alpha: c[6], foam })
                })
                .collect::<Vec<_>>();
            rows.sort_by(|a, b| a.along.total_cmp(&b.along));
            (rows.len() >= 2).then_some(rows)
        })
        .collect()
}

/// The largest distance (metres) of a fitted segment's curve from the rows it covers that is accepted.
const FIT_TOLERANCE: f32 = 0.02;

/// The most rows one segment covers.
const FIT_MOST_ROWS: usize = 400;

/// The length of the lead-in segment that holds a river's first row's values when they are not the next row's (metres).
const LEAD_IN: f32 = 0.5;

/// Splines that [`tessellate`] makes `rows` again: Bézier segments from row to row, each over the longest run of rows from the last one's
/// end that it passes within [`FIT_TOLERANCE`] and whose width, flow and foam are one (TEd's splines hold them per segment: point i's)
/// and alpha a line (lerped between the ends). A row's place on its segment is its share of the run (BOB samples the river by length
/// and a segment by its own parameter, linearly); the inner control points are the least-squares fit to the rows inside, the ends the
/// end rows.
pub fn fit_splines(rows: &[RiverRow]) -> Vec<RiverSegment> {
    let mut out = vec![];
    let mut a = 0;
    let mut owned;
    let mut rows = rows;
    // The first row's values differ from the next's (the first spline point's segment ends before it): a short lead-in with them, the
    // rest from its end.
    if let [r0, r1, ..] = rows
        && ((r0.width - r1.width).abs() > 0.01 || (r0.flow - r1.flow).abs() > 1e-4 || (r0.foam - r1.foam).abs() > 1e-4)
    {
        let (r0, r1) = (rows[0], rows[1]);
        let chord = (r1.centre[0] - r0.centre[0]).hypot(r1.centre[2] - r0.centre[2]);
        let f = if chord > 0.0 { (LEAD_IN / chord).min(0.5) } else { 0.5 };
        let start = RiverRow {
            centre: [0, 1, 2].map(|k| r0.centre[k] + (r1.centre[k] - r0.centre[k]) * f),
            along: r0.along + (r1.along - r0.along) * f,
            alpha: r0.alpha + (r1.alpha - r0.alpha) * f,
            ..r1
        };
        let mut lead = line(&r0, &start);
        (lead.width, lead.flow, lead.foam) = ([r0.width; 2], [r0.flow; 2], [r0.foam; 2]);
        out.push(lead);
        owned = rows.to_vec();
        owned[0] = start;
        rows = &owned;
    }

    while a + 1 < rows.len() {
        // Past a change of the values no run fits; a curve that misses the rows may fit a longer run (more rows inside to fit to).
        let (mut best, mut changes) = (None, None);
        for b in a + 1..rows.len().min(a + FIT_MOST_ROWS + 1) {
            match fit_segment(&rows[a..=b], a == 0) {
                Ok(seg) => best = Some((b, seg)),
                Err(Misfit::Values) => {
                    changes = Some(b);
                    break;
                }
                Err(Misfit::Curve) => {}
            }
        }

        // Two rows whose values differ: a line with the second's.
        let (b, seg) = best.unwrap_or_else(|| (a + 1, line(&rows[a], &rows[a + 1])));
        out.push(seg);
        a = b;

        // The values change right after the run: the spline's point lies between its last row (on the run's curve) and the next, whose
        // curve goes on from there. A line between the two bridges them, the curve after it fitted from the next row.
        if changes == Some(b + 1) {
            out.push(line(&rows[b], &rows[b + 1]));
            a = b + 1;
        }
    }

    out
}

/// A straight segment between two rows with the second's values (the first is the segment before's end).
fn line(r0: &RiverRow, r1: &RiverRow) -> RiverSegment {
    let (p0, p3) = (r0.centre, r1.centre);
    let at = |t: f32| [0, 1, 2].map(|k| p0[k] + (p3[k] - p0[k]) * t);
    RiverSegment {
        points: [p0, at(1.0 / 3.0), at(2.0 / 3.0), p3],
        foam: [r1.foam; 2],
        alpha: [r0.alpha, r1.alpha],
        flow: [r1.flow; 2],
        width: [r1.width; 2],
    }
}

/// Why a run of rows does not fit one segment.
enum Misfit {
    /// Its width, flow or foam change, or its alpha is not a line.
    Values,

    /// The curve misses a row.
    Curve,
}

/// A segment over `rows` when it passes them (the first row is the segment before's end, but on the river's first segment).
fn fit_segment(rows: &[RiverRow], first: bool) -> Result<RiverSegment, Misfit> {
    let (r0, r1) = (&rows[0], &rows[rows.len() - 1]);
    let own = if first { rows } else { &rows[1..] };
    // The width comes from the edge vertices: f32 noise of a few 1e-5 of it.
    let one = |f: fn(&RiverRow) -> f32, tolerance: f32| own.iter().all(|r| (f(r) - f(&own[0])).abs() <= tolerance);
    if !(one(|r| r.width, 0.01) && one(|r| r.flow, 1e-4) && one(|r| r.foam, 1e-4)) {
        return Err(Misfit::Values);
    }

    let span = r1.along - r0.along;
    if span <= 0.0 {
        return Err(Misfit::Values);
    }

    let t = |r: &RiverRow| (r.along - r0.along) / span;
    let inner = &rows[1..rows.len() - 1];
    if inner.iter().any(|r| (r0.alpha + (r1.alpha - r0.alpha) * t(r) - r.alpha).abs() > 1e-3) {
        return Err(Misfit::Values);
    }

    let mut seg = line(r0, r1);
    seg.foam = [own[0].foam; 2];
    seg.flow = [own[0].flow; 2];
    seg.width = [own[0].width; 2];
    let (p0, p3) = (r0.centre, r1.centre);
    if inner.len() >= 2 {
        // Least squares for P1 and P2 (the basis 3t(1-t)^2, 3t^2(1-t)), each coordinate on its own.
        let (mut s11, mut s12, mut s22) = (0.0f64, 0.0f64, 0.0f64);
        let (mut q1, mut q2) = ([0.0f64; 3], [0.0f64; 3]);
        for r in inner {
            let t = f64::from(t(r));
            let s = 1.0 - t;
            let (b0, b1, b2, b3) = (s * s * s, 3.0 * t * s * s, 3.0 * t * t * s, t * t * t);
            s11 += b1 * b1;
            s12 += b1 * b2;
            s22 += b2 * b2;
            for k in 0..3 {
                let rest = f64::from(r.centre[k]) - b0 * f64::from(p0[k]) - b3 * f64::from(p3[k]);
                q1[k] += b1 * rest;
                q2[k] += b2 * rest;
            }
        }

        let det = s11 * s22 - s12 * s12;
        if det.abs() > 1e-12 {
            seg.points[1] = [0, 1, 2].map(|k| ((s22 * q1[k] - s12 * q2[k]) / det) as f32);
            seg.points[2] = [0, 1, 2].map(|k| ((s11 * q2[k] - s12 * q1[k]) / det) as f32);
        }
    }

    let worst = inner
        .iter()
        .map(|r| {
            let q = weigh(&seg.points, position_powers(t(r)));
            let d = [0, 1, 2].map(|k| q[k] - r.centre[k]);
            (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt()
        })
        .fold(0.0f32, f32::max);
    // Between two rows the curve keeps near their chord, as BOB's rows of a curve do (a row is kept where the river has turned by about
    // 11 degrees, an arc's sag then up to 2.5 % of its chord): halfway, within 3 % of the chord and 2 cm, its height within 5 cm and
    // 0.5 % of the chord.
    let wanders = rows.windows(2).any(|w| {
        let q = weigh(&seg.points, position_powers((t(&w[0]) + t(&w[1])) * 0.5));
        let m = [0, 1, 2].map(|k| (w[0].centre[k] + w[1].centre[k]) * 0.5);
        let chord = (w[1].centre[0] - w[0].centre[0]).hypot(w[1].centre[2] - w[0].centre[2]);
        (q[0] - m[0]).hypot(q[2] - m[2]) > 0.03 * chord + FIT_TOLERANCE || (q[1] - m[1]).abs() > 0.05 + 0.005 * chord
    });
    if worst <= FIT_TOLERANCE && !wanders { Ok(seg) } else { Err(Misfit::Curve) }
}

/// A river block as the terrain meshes hold it after the terrain's blocks.
pub struct RiverBlock {
    pub bytes: Vec<u8>,

    /// What it adds to the LOD's vertex and index byte counts.
    pub vertex_bytes: u32,
    pub index_bytes: u32,
}

/// The mesh's river block (its format's type), none without triangles: the vertices numbered by first use, 8 f32 each, u16 indices;
/// the bounds `FUN_100228f0` gives it (the vertices' x and z, y at -0.1 and 0.1), the material `rigid_default`.
pub fn river_block(river: &RiverMesh) -> Option<RiverBlock> {
    let mut number = vec![u32::MAX; river.vertices.len()];
    let (mut order, mut indices) = (vec![], vec![]);
    for &i in river.triangles.iter().flatten() {
        if number[i as usize] == u32::MAX {
            number[i as usize] = order.len() as u32;
            order.push(i);
        }

        indices.push(number[i as usize] as u16);
    }

    if order.is_empty() {
        return None;
    }

    let (mut lo, mut hi) = ([f32::MAX, -0.1, f32::MAX], [f32::MIN, 0.1, f32::MIN]);
    for v in &river.vertices {
        for k in [0, 2] {
            lo[k] = lo[k].min(v[k]);
            hi[k] = hi[k].max(v[k]);
        }
    }

    let (vb, ib) = (order.len() as u32 * 32, indices.len() as u32 * 2);
    let mut o = vec![];
    let put = |o: &mut Vec<u8>, v: u32| o.extend_from_slice(&v.to_le_bytes());
    for v in [river.format.block_type(), 0x50 + vb + ib, 0x50, order.len() as u32, 0x50 + vb, indices.len() as u32] {
        put(&mut o, v);
    }

    for v in lo.into_iter().chain(hi) {
        put(&mut o, v.to_bits());
    }

    o.extend_from_slice(b"rigid_default");
    o.resize(0x50, 0);
    for &i in &order {
        for c in river.vertices[i as usize] {
            put(&mut o, c.to_bits());
        }
    }

    for &i in &indices {
        o.extend_from_slice(&i.to_le_bytes());
    }

    Some(RiverBlock { bytes: o, vertex_bytes: vb, index_bytes: ib })
}

/// The river block of a terrain mesh (RMV2 version 6, its first LOD): the vertices, the triangles and the block's format; none
/// without one (or bytes that are not such a mesh).
pub fn read_river_block(b: &[u8]) -> Option<RiverMesh> {
    let blocks = mesh_blocks(b)?;
    let &(at, kind, _) = blocks.iter().find(|(_, kind, _)| RiverFormat::of_block(*kind).is_some())?;
    let u32_at = |a: usize| b.get(a..a + 4).map(|s| u32::from_le_bytes([s[0], s[1], s[2], s[3]]));
    let (voff, vcount, ioff, icount) = (u32_at(at + 8)? as usize, u32_at(at + 12)? as usize, u32_at(at + 16)? as usize, u32_at(at + 20)? as usize);
    let vb = b.get(at + voff..at + voff + vcount * 32)?;
    let vertices = vb.chunks_exact(32).map(|c| std::array::from_fn(|k| f32::from_le_bytes([c[k * 4], c[k * 4 + 1], c[k * 4 + 2], c[k * 4 + 3]]))).collect();
    let ib = b.get(at + ioff..at + ioff + icount * 2)?;
    let indices = ib.chunks_exact(2).map(|c| u32::from(u16::from_le_bytes([c[0], c[1]]))).collect::<Vec<_>>();
    let triangles = indices.chunks_exact(3).map(|t| [t[0], t[1], t[2]]).collect();
    Some(RiverMesh { vertices, triangles, format: RiverFormat::of_block(kind)? })
}

/// The first LOD's blocks of an RMV2 version 6 mesh: (offset, type, size) each. The LOD table after the 0x8c byte header: block
/// count, vertex bytes, index bytes, the first block's offset, the distance.
fn mesh_blocks(b: &[u8]) -> Option<Vec<(usize, u32, usize)>> {
    let u32_at = |a: usize| b.get(a..a + 4).map(|s| u32::from_le_bytes([s[0], s[1], s[2], s[3]]));
    if b.get(..4) != Some(b"RMV2") || u32_at(8)? == 0 {
        return None;
    }

    let (count, mut at) = (u32_at(0x8c)?, u32_at(0x98)? as usize);
    let mut out = vec![];
    for _ in 0..count {
        let (kind, size) = (u32_at(at)?, u32_at(at + 4)? as usize);
        if size == 0 || at + size > b.len() {
            return None;
        }

        out.push((at, kind, size));
        at += size;
    }

    Some(out)
}

/// A terrain mesh with its river block replaced by `river`'s (removed when it has no triangles, added after the other blocks when the
/// mesh had none); the other blocks and the bytes after them as they were. None for bytes that are not such a mesh (one LOD).
pub fn with_river_block(b: &[u8], river: &RiverMesh) -> Option<Vec<u8>> {
    let blocks = mesh_blocks(b)?;
    let u32_at = |a: usize| u32::from_le_bytes([b[a], b[a + 1], b[a + 2], b[a + 3]]);
    if u32_at(8) != 1 {
        return None;
    }

    let end = blocks.last().map_or(u32_at(0x98) as usize, |&(at, _, size)| at + size);
    let (mut count, mut vbytes, mut ibytes) = (u32_at(0x8c), u32_at(0x90), u32_at(0x94));
    let mut out = b[..u32_at(0x98) as usize].to_vec();
    for &(at, kind, size) in &blocks {
        if RiverFormat::of_block(kind).is_some() {
            count -= 1;
            vbytes -= u32_at(at + 12) * 32;
            ibytes -= u32_at(at + 20) * 2;
        } else {
            out.extend_from_slice(&b[at..at + size]);
        }
    }

    if let Some(r) = river_block(river) {
        out.extend_from_slice(&r.bytes);
        count += 1;
        vbytes += r.vertex_bytes;
        ibytes += r.index_bytes;
    }

    out.extend_from_slice(&b[end..]);
    for (k, v) in [count, vbytes, ibytes].into_iter().enumerate() {
        out[0x8c + 4 * k..0x90 + 4 * k].copy_from_slice(&v.to_le_bytes());
    }

    Some(out)
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
        let mesh = tessellate(&[vec![seg]], RiverFormat::Attila);
        assert_eq!(mesh.vertices.len(), 9 * 5);
        assert_eq!(mesh.triangles.len(), 8 * 4 * 2);
        let first = &mesh.vertices[..5];
        assert_eq!(first.iter().map(|v| v[2]).collect::<Vec<_>>(), [52.0, 51.0, 50.0, 49.0, 48.0]);
        assert_eq!(mesh.vertices[40][5], mesh.vertices[35][5]);
    }

    /// Two curved segments, the second narrower and slower, the alpha fading out at the end.
    fn bend() -> Vec<RiverSegment> {
        vec![
            RiverSegment {
                points: [[100.0, -6.0, 100.0], [140.0, -6.0, 100.0], [180.0, -6.1, 120.0], [200.0, -6.2, 160.0]],
                foam: [0.1; 2],
                alpha: [1.0; 2],
                flow: [0.5; 2],
                width: [30.0; 2],
            },
            RiverSegment {
                points: [[200.0, -6.2, 160.0], [220.0, -6.3, 200.0], [260.0, -6.3, 220.0], [300.0, -6.3, 210.0]],
                foam: [0.2; 2],
                alpha: [1.0, 0.0],
                flow: [0.1; 2],
                width: [20.0; 2],
            },
        ]
    }

    #[test]
    fn rome2_vertices_hold_the_values_in_its_order() {
        let mesh = tessellate(&[bend()], RiverFormat::Rome2);
        let attila = tessellate(&[bend()], RiverFormat::Attila);
        assert_eq!(mesh.vertices.len(), attila.vertices.len());
        for (r, a) in mesh.vertices.iter().zip(&attila.vertices) {
            // Position and flow alike; across and along as shares; alpha; the foam of the segment.
            assert_eq!(r[..4], a[..4]);
            assert_eq!(r[4], a[7]);
            assert!((r[5] * attila.vertices.last().expect("rows")[5] - a[5]).abs() < 1e-2);
            assert_eq!(r[6], a[6]);
            assert!(r[7] == 0.1 || r[7] == 0.2);
        }
    }

    #[test]
    fn fitted_splines_make_the_rows_again() {
        for format in [RiverFormat::Attila, RiverFormat::Rome2] {
            let rows = river_rows(&tessellate(&[bend()], format));
            assert_eq!(rows.len(), 1);
            let fitted = fit_splines(&rows[0]);
            assert!(fitted.len() <= 4, "{} segments", fitted.len());
            let again = river_rows(&tessellate(&[fitted.clone()], format));
            assert_eq!(again.len(), 1);

            // The rows lie on the fitted curve (sampled densely: to the polyline of the samples).
            let curve = fitted.iter().flat_map(|s| (0..=400).map(|i| weigh(&s.points, position_powers(i as f32 / 400.0)))).collect::<Vec<_>>();
            for r in &rows[0] {
                let to = |a: [f32; 3], b: [f32; 3]| {
                    let (dx, dz) = (b[0] - a[0], b[2] - a[2]);
                    let len2 = dx * dx + dz * dz;
                    let t = if len2 > 0.0 { (((r.centre[0] - a[0]) * dx + (r.centre[2] - a[2]) * dz) / len2).clamp(0.0, 1.0) } else { 0.0 };
                    (r.centre[0] - a[0] - dx * t).hypot(r.centre[2] - a[2] - dz * t)
                };
                let off = curve.windows(2).map(|w| to(w[0], w[1])).fold(f32::MAX, f32::min);
                assert!(off < 0.02, "{format:?}: a row {off} m from the fitted curve: {r:?}");
            }

            // The width comes from the edge vertices: f32 noise.
            let last = fitted.last().expect("segments");
            assert!((fitted[0].width[0] - 30.0).abs() < 1e-3 && (last.width[0] - 20.0).abs() < 1e-3);
            assert_eq!((fitted[0].flow[0], last.flow[0], last.alpha[1]), (0.5, 0.1, 0.0));
        }
    }

    #[test]
    fn the_river_block_is_replaced_in_a_mesh() {
        // A mesh of one terrain block (its type and size only) and the river.
        let put = |o: &mut Vec<u8>, v: u32| o.extend_from_slice(&v.to_le_bytes());
        let mut mesh = b"RMV2".to_vec();
        put(&mut mesh, 6);
        put(&mut mesh, 1);
        mesh.resize(0x8c, 0);
        let terrain = {
            let mut b = vec![];
            for v in [0x42, 0x60, 0x50, 2, 0x58, 4] {
                put(&mut b, v);
            }

            b.resize(0x60, 7);
            b
        };
        for v in [1, 32, 8, 0xa0, 0] {
            put(&mut mesh, v);
        }

        mesh.resize(0xa0, 0);
        mesh.extend_from_slice(&terrain);
        let river = tessellate(&[bend()], RiverFormat::Rome2);
        let with = with_river_block(&mesh, &river).expect("a mesh");
        let read = read_river_block(&with).expect("a river block");
        assert_eq!((read.format, read.triangles.len()), (RiverFormat::Rome2, river.triangles.len()));
        assert_eq!(&with[0xa0..0x100], &terrain[..]);

        // The counts: the terrain's plus the river's; replaced again by nothing, the mesh as it was.
        let u = |b: &[u8], at: usize| u32::from_le_bytes([b[at], b[at + 1], b[at + 2], b[at + 3]]);
        let block = river_block(&river).expect("triangles");
        assert_eq!([u(&with, 0x8c), u(&with, 0x90), u(&with, 0x94)], [2, 32 + block.vertex_bytes, 8 + block.index_bytes]);
        assert_eq!(with_river_block(&with, &RiverMesh::default()).expect("a mesh"), mesh);
    }
}
