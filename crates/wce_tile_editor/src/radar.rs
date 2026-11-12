//! The radar generator (docs/plans/radar.md): a battle map drawn as the game's hand-made loading screen radars are
//! (`loading_screen_radar.jpg`, 512 x 512, the tile without its border from above, north up): parchment with faint field strips, cream
//! roads and streets, olive forest blobs, a grey-blue sea, grey building blocks with black outlines and red roofs, walls as thick ink, the
//! cliffs' top edges in cyan with ink ticks down the slope; plus ink touches the hand-made ones lack (hachures on the slopes, a hill shade,
//! ripples along the shore).
//!
//! Pure: [`RadarInput`] holds what is drawn (rasters and outlines in the tile's metres), [`render`] draws it. What the tile gives is gathered
//! by `Tile::radar_input` (tile_document/radar.rs). Drawn at twice the size and averaged down.

use rayon::prelude::*;

/// A point on the ground (x, z), metres.
pub type P = [f64; 2];

/// A raster over an area of the ground: `w * h` values, rows along +z, cell (i, j) centred at `x0 + (i + 0.5) cell_x`, `z0 + (j + 0.5) cell_z`.
#[derive(Clone, Debug, Default)]
pub struct Field {
    pub x0: f64,
    pub z0: f64,
    pub w_m: f64,
    pub h_m: f64,
    pub w: usize,
    pub h: usize,
    pub v: Vec<f32>,
}

impl Field {
    /// Bilinear; 0 off the raster.
    pub fn sample(&self, x: f64, z: f64) -> f32 {
        if self.w == 0 || self.h == 0 {
            return 0.0;
        }

        let fx = (x - self.x0) / self.w_m * self.w as f64 - 0.5;
        let fz = (z - self.z0) / self.h_m * self.h as f64 - 0.5;
        if fx < -0.5 || fz < -0.5 || fx > self.w as f64 - 0.5 || fz > self.h as f64 - 0.5 {
            return 0.0;
        }

        let (i0, j0) = (fx.floor().max(0.0) as usize, fz.floor().max(0.0) as usize);
        let (i1, j1) = ((i0 + 1).min(self.w - 1), (j0 + 1).min(self.h - 1));
        let (tx, tz) = ((fx - i0 as f64).clamp(0.0, 1.0) as f32, (fz - j0 as f64).clamp(0.0, 1.0) as f32);
        let at = |i: usize, j: usize| self.v[j * self.w + i];
        let a = at(i0, j0) + (at(i1, j0) - at(i0, j0)) * tx;
        let b = at(i0, j1) + (at(i1, j1) - at(i0, j1)) * tx;
        a + (b - a) * tz
    }
}

/// A building's footprint and how it is drawn.
#[derive(Clone, Debug, Default)]
pub struct Building {
    /// Its footprint: polygons (outlines, or the triangles of its hull seen from above), filled together.
    pub polys: Vec<Vec<P>>,
    /// A red roof (else grey).
    pub red: bool,
}

/// A piece of a fort.
#[derive(Clone, Debug, Default)]
pub struct Wall {
    pub footprint: Vec<Vec<P>>,
    /// Its ends along the wall.
    pub line: [P; 2],
    pub gate: bool,
    pub palisade: bool,
}

/// What the radar draws, in the tile's metres.
#[derive(Clone, Debug, Default)]
pub struct RadarInput {
    /// The area drawn (x0, z0, w, h): the image's square is fitted around it.
    pub area: [f64; 4],

    /// The ground's height (metres, the sea at 0).
    pub height: Field,

    /// Whether ground under 0 m is sea.
    pub sea: bool,

    /// Water over the ground (rivers, lakes): its depth in metres.
    pub water: Option<Field>,

    /// How much of a road is at a point (0 .. 1): the blend map's thin layers.
    pub roads: Option<Field>,

    /// Roads as lines (the procedural roads' centre lines, the road decals): drawn `RadarStyle::road_width` wide.
    pub road_lines: Vec<Vec<P>>,

    /// The trees (their places).
    pub trees: Vec<P>,

    pub buildings: Vec<Building>,
    pub walls: Vec<Wall>,

    /// The streets of the settlement (centre lines) and how wide they are drawn (metres).
    pub streets: Vec<(Vec<P>, f64)>,

    /// The settlement's open areas (squares, the nodes' boundaries).
    pub plazas: Vec<Vec<P>>,

    /// The steep ground the game will not walk (outlines, even-odd).
    pub cliffs: Vec<Vec<P>>,

    /// The water planes' footprints: the steep ground in them is a bank (a stream bed's, a marsh's), not a cliff.
    pub water_planes: Vec<Vec<P>>,
}

/// How cliffs are drawn.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum CliffStyle {
    /// As CA's radars: the top edge a cyan line with ink ticks down the slope.
    Rim,

    /// Rock: the steep ground as grey stone in lit and shaded facets, cracked, outlined in ink, ticks down the slope.
    #[default]
    Rock,
}

/// How the radar is drawn (every colour sRGB).
#[derive(Clone, Debug)]
pub struct RadarStyle {
    /// The image's side (pixels).
    pub size: u32,
    pub seed: u32,

    pub paper: [f32; 3],
    pub paper_dark: [f32; 3],
    pub ink: [f32; 3],
    pub road: [f32; 3],
    pub forest: [f32; 3],
    pub forest_edge: [f32; 3],
    pub sea: [f32; 3],
    pub sea_deep: [f32; 3],
    pub block: [f32; 3],
    pub building: [f32; 3],
    pub roof: [f32; 3],
    /// The cliffs' top edges.
    pub cliff: [f32; 3],
    pub rock: [f32; 3],

    /// How wide roads are drawn (metres).
    pub road_width: f64,

    /// The blend map's roads are drawn too (when the tile has few road lines).
    pub blend_roads: bool,

    /// Trees per square metre from which the ground is a forest.
    pub forest_density: f64,

    /// How much the hill shade darkens and lightens (0 .. 1).
    pub relief: f32,

    /// Slopes from this many degrees get hachures; the ink is strongest at `cliff_slope`.
    pub hachure_slope: f64,
    pub cliff_slope: f64,

    /// Ripple lines along the shore.
    pub ripples: u32,

    /// Water shallower than this (metres) is drawn pale and without an ink edge.
    pub shallow_m: f32,

    pub cliff_style: CliffStyle,

    /// The rock's facets, across (metres).
    pub rock_facet_m: f64,
}

impl Default for RadarStyle {
    fn default() -> Self {
        Self {
            size: 512,
            seed: 0x5eed,
            paper: [152.0, 120.0, 80.0],
            paper_dark: [112.0, 84.0, 54.0],
            ink: [34.0, 27.0, 20.0],
            road: [228.0, 210.0, 170.0],
            forest: [92.0, 96.0, 50.0],
            forest_edge: [48.0, 50.0, 26.0],
            sea: [96.0, 114.0, 124.0],
            sea_deep: [72.0, 88.0, 104.0],
            block: [128.0, 116.0, 100.0],
            building: [104.0, 94.0, 84.0],
            roof: [164.0, 70.0, 48.0],
            cliff: [112.0, 186.0, 206.0],
            rock: [150.0, 140.0, 124.0],
            road_width: 6.0,
            blend_roads: true,
            forest_density: 0.03,
            relief: 0.22,
            hachure_slope: 12.0,
            cliff_slope: 35.0,
            ripples: 3,
            shallow_m: 0.4,
            cliff_style: CliffStyle::Rock,
            rock_facet_m: 3.0,
        }
    }
}

/// An sRGB image, `size * size * 4`.
#[derive(Clone, Debug, Default)]
pub struct RadarImage {
    pub size: u32,
    pub rgba: Vec<u8>,
}

/// The canvas: what each pixel is in metres.
#[derive(Clone, Copy)]
struct Frame {
    n: usize,
    x0: f64,
    /// The top (north) edge.
    z1: f64,
    /// Metres per pixel.
    m: f64,
}

impl Frame {
    fn world(&self, i: usize, j: usize) -> P {
        [self.x0 + (i as f64 + 0.5) * self.m, self.z1 - (j as f64 + 0.5) * self.m]
    }

    fn pixel(&self, p: P) -> [f64; 2] {
        [(p[0] - self.x0) / self.m, (self.z1 - p[1]) / self.m]
    }
}

/// A coverage mask over the canvas (0 .. 1).
#[derive(Clone)]
struct Mask {
    n: usize,
    v: Vec<f32>,
}

impl Mask {
    fn new(n: usize) -> Self {
        Self { n, v: vec![0.0; n * n] }
    }

    fn from_fn(n: usize, f: impl Fn(usize, usize) -> f32 + Sync) -> Self {
        Self { n, v: (0..n * n).into_par_iter().map(|k| f(k % n, k / n)).collect() }
    }


    /// A box blur of radius `r`, three times (about a Gaussian).
    fn blurred(&self, r: usize) -> Self {
        let mut m = self.clone();
        for _ in 0..3 {
            m = m.box_pass(r, true).box_pass(r, false);
        }
        m
    }

    fn box_pass(&self, r: usize, horizontal: bool) -> Self {
        let n = self.n;
        let mut out = vec![0.0f32; n * n];
        out.par_chunks_mut(n).enumerate().for_each(|(line, row)| {
            let get = |k: i64| {
                let k = k.clamp(0, n as i64 - 1) as usize;
                if horizontal { self.v[line * n + k] } else { self.v[k * n + line] }
            };
            let r = r as i64;
            let mut sum: f32 = (-r..=r).map(get).sum();
            let inv = 1.0 / (2 * r + 1) as f32;
            for (k, o) in row.iter_mut().enumerate() {
                *o = sum * inv;
                sum += get(k as i64 + r + 1) - get(k as i64 - r);
            }
        });

        if horizontal {
            Self { n, v: out }
        } else {
            // The pass ran along columns into rows: transpose back.
            let mut t = vec![0.0f32; n * n];
            t.par_chunks_mut(n).enumerate().for_each(|(j, row)| {
                for (i, o) in row.iter_mut().enumerate() {
                    *o = out[i * n + j];
                }
            });
            Self { n, v: t }
        }
    }

    /// The band along the mask's edge (0 .. 1), about `r` pixels wide.
    fn edge(&self, r: usize) -> Self {
        let b = self.blurred(r);
        Self { n: self.n, v: b.v.iter().map(|&x| (x * (1.0 - x) * 4.0).clamp(0.0, 1.0)).collect() }
    }

    /// Thresholded with a soft step of `soft` around `t`.
    fn step(&self, t: f32, soft: f32) -> Self {
        Self { n: self.n, v: self.v.iter().map(|&x| smooth(t - soft, t + soft, x)).collect() }
    }

    fn max_with(&mut self, o: &Mask) {
        for (a, b) in self.v.iter_mut().zip(&o.v) {
            *a = a.max(*b);
        }
    }
}

fn smooth(a: f32, b: f32, x: f32) -> f32 {
    let t = ((x - a) / (b - a)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// Hash noise: 0 .. 1.
fn hash(x: i64, y: i64, seed: u32) -> f32 {
    let mut h = (x as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15) ^ (y as u64).wrapping_mul(0xC2B2_AE3D_27D4_EB4F) ^ u64::from(seed).wrapping_mul(0x1656_67B1_9E37_79F9);
    h ^= h >> 31;
    h = h.wrapping_mul(0xBF58_476D_1CE4_E5B9);
    h ^= h >> 29;
    (h >> 40) as f32 / (1u64 << 24) as f32
}

/// Value noise, smooth, 0 .. 1.
fn value_noise(x: f64, y: f64, seed: u32) -> f32 {
    let (xi, yi) = (x.floor(), y.floor());
    let (tx, ty) = ((x - xi) as f32, (y - yi) as f32);
    let (sx, sy) = (tx * tx * (3.0 - 2.0 * tx), ty * ty * (3.0 - 2.0 * ty));
    let (xi, yi) = (xi as i64, yi as i64);
    let a = hash(xi, yi, seed) + (hash(xi + 1, yi, seed) - hash(xi, yi, seed)) * sx;
    let b = hash(xi, yi + 1, seed) + (hash(xi + 1, yi + 1, seed) - hash(xi, yi + 1, seed)) * sx;
    a + (b - a) * sy
}

/// Fractal noise of `octaves`, 0 .. 1 (about).
fn fbm(x: f64, y: f64, octaves: u32, seed: u32) -> f32 {
    let (mut sum, mut amp, mut f, mut norm) = (0.0, 0.5, 1.0, 0.0);
    for o in 0..octaves {
        sum += amp * value_noise(x * f, y * f, seed.wrapping_add(o * 7919));
        norm += amp;
        amp *= 0.5;
        f *= 2.03;
    }
    sum / norm
}

/// Fills closed polygons (even-odd) into a mask, sampled at the pixels' centres.
fn fill(frame: &Frame, polys: &[&[P]]) -> Mask {
    let n = frame.n;
    let mut m = Mask::new(n);
    // Edges in pixel space.
    let mut edges: Vec<([f64; 2], [f64; 2])> = vec![];
    for poly in polys {
        if poly.len() < 3 {
            continue;
        }
        for k in 0..poly.len() {
            let (a, b) = (frame.pixel(poly[k]), frame.pixel(poly[(k + 1) % poly.len()]));
            if a[1] != b[1] {
                edges.push((a, b));
            }
        }
    }

    if edges.is_empty() {
        return m;
    }

    m.v.par_chunks_mut(n).enumerate().for_each(|(j, row)| {
        let y = j as f64 + 0.5;
        let mut xs: Vec<f64> = edges.iter().filter(|(a, b)| (a[1] <= y) != (b[1] <= y)).map(|(a, b)| a[0] + (y - a[1]) / (b[1] - a[1]) * (b[0] - a[0])).collect();
        xs.sort_by(f64::total_cmp);
        for pair in xs.chunks(2) {
            let [x0, x1] = [pair[0], *pair.get(1).unwrap_or(&pair[0])];
            let (i0, i1) = ((x0 - 0.5).ceil().max(0.0) as usize, ((x1 - 0.5).floor().min(n as f64 - 1.0)).max(-1.0));
            if i1 < 0.0 {
                continue;
            }
            for v in row.iter_mut().take(i1 as usize + 1).skip(i0) {
                *v = 1.0;
            }
        }
    });
    m
}

/// Strokes open lines `width` pixels wide (round ends), anti-aliased.
fn stroke(frame: &Frame, lines: &[(&[P], f64)]) -> Mask {
    let n = frame.n;
    let mut segs: Vec<([f64; 2], [f64; 2], f64)> = vec![];
    for (line, width) in lines {
        let px: Vec<[f64; 2]> = line.iter().map(|&p| frame.pixel(p)).collect();
        for w in px.windows(2) {
            segs.push((w[0], w[1], *width * 0.5));
        }
        if px.len() == 1 {
            segs.push((px[0], px[0], *width * 0.5));
        }
    }

    // Bin the segments by rows of 32 pixels.
    const BIN: usize = 32;
    let bins = n.div_ceil(BIN);
    let mut by_bin: Vec<Vec<usize>> = vec![vec![]; bins];
    for (k, (a, b, r)) in segs.iter().enumerate() {
        let lo = ((a[1].min(b[1]) - r - 1.0).max(0.0) as usize / BIN).min(bins - 1);
        let hi = ((a[1].max(b[1]) + r + 1.0).max(0.0) as usize / BIN).min(bins - 1);
        for bin in by_bin.iter_mut().take(hi + 1).skip(lo) {
            bin.push(k);
        }
    }

    let mut m = Mask::new(n);
    m.v.par_chunks_mut(n).enumerate().for_each(|(j, row)| {
        let y = j as f64 + 0.5;
        for &k in &by_bin[j / BIN] {
            let (a, b, r) = segs[k];
            if y < a[1].min(b[1]) - r - 1.0 || y > a[1].max(b[1]) + r + 1.0 {
                continue;
            }
            let (i0, i1) = ((a[0].min(b[0]) - r - 1.0).max(0.0) as usize, ((a[0].max(b[0]) + r + 1.0).max(0.0) as usize).min(n - 1));
            let d = [b[0] - a[0], b[1] - a[1]];
            let len2 = d[0] * d[0] + d[1] * d[1];
            for (i, v) in row.iter_mut().enumerate().take(i1 + 1).skip(i0) {
                let p = [i as f64 + 0.5 - a[0], y - a[1]];
                let t = if len2 > 0.0 { ((p[0] * d[0] + p[1] * d[1]) / len2).clamp(0.0, 1.0) } else { 0.0 };
                let (dx, dy) = (p[0] - t * d[0], p[1] - t * d[1]);
                let dist = (dx * dx + dy * dy).sqrt();
                let c = (r + 0.5 - dist).clamp(0.0, 1.0) as f32;
                *v = v.max(c);
            }
        }
    });
    m
}

/// Fills polygons as a union (each non-zero on its own: overlapping triangles do not cancel).
fn fill_union(frame: &Frame, polys: &[&[P]]) -> Mask {
    // In batches whose polygons are filled one at a time into the mask.
    let n = frame.n;
    let mut m = Mask::new(n);
    for p in polys {
        if p.len() < 3 {
            continue;
        }
        let px: Vec<[f64; 2]> = p.iter().map(|&q| frame.pixel(q)).collect();
        let (y0, y1) = px.iter().fold((f64::MAX, f64::MIN), |(a, b), q| (a.min(q[1]), b.max(q[1])));
        let (j0, j1) = ((y0 - 0.5).ceil().max(0.0) as usize, ((y1 - 0.5).floor().min(n as f64 - 1.0)));
        if j1 < 0.0 {
            continue;
        }
        for j in j0..=j1 as usize {
            let y = j as f64 + 0.5;
            let mut xs: Vec<f64> = (0..px.len()).filter_map(|k| {
                let (a, b) = (px[k], px[(k + 1) % px.len()]);
                ((a[1] <= y) != (b[1] <= y)).then(|| a[0] + (y - a[1]) / (b[1] - a[1]) * (b[0] - a[0]))
            }).collect();
            xs.sort_by(f64::total_cmp);
            for pair in xs.chunks(2) {
                let [x0, x1] = [pair[0], *pair.get(1).unwrap_or(&pair[0])];
                let i0 = (x0 - 0.5).ceil().max(0.0) as usize;
                let i1 = (x1 - 0.5).floor().min(n as f64 - 1.0);
                if i1 < 0.0 {
                    continue;
                }
                for i in i0..=i1 as usize {
                    m.v[j * n + i] = 1.0;
                }
            }
        }
    }
    m
}

/// A line along a mask's edge, on its inside, about 1.5 pixels wide.
fn crisp_edge(m: &Mask) -> Mask {
    let n = m.n;
    Mask::from_fn(n, |i, j| {
        let v = m.v[j * n + i];
        if v <= 0.0 {
            return 0.0;
        }
        let mut lo = v;
        for (di, dj) in [(-1i64, 0i64), (1, 0), (0, -1), (0, 1), (-1, -1), (1, 1), (-1, 1), (1, -1)] {
            let (x, y) = (i as i64 + di, j as i64 + dj);
            let o = if x < 0 || y < 0 || x >= n as i64 || y >= n as i64 { 0.0 } else { m.v[y as usize * n + x as usize] };
            lo = lo.min(o);
        }
        v - lo
    })
}

/// The colour canvas.
struct Canvas {
    n: usize,
    c: Vec<[f32; 3]>,
}

impl Canvas {
    /// Paints `colour` over the canvas through `mask` at `opacity`.
    fn paint(&mut self, mask: &Mask, colour: [f32; 3], opacity: f32) {
        self.c.par_iter_mut().zip(mask.v.par_iter()).for_each(|(c, &a)| {
            let a = a * opacity;
            if a > 0.0 {
                for k in 0..3 {
                    c[k] += (colour[k] - c[k]) * a;
                }
            }
        });
    }

    /// Paints a colour per pixel through `mask`.
    fn paint_with(&mut self, mask: &Mask, colour: impl Fn(usize, usize, [f32; 3]) -> [f32; 3] + Sync) {
        let n = self.n;
        self.c.par_iter_mut().zip(mask.v.par_iter()).enumerate().for_each(|(k, (c, &a))| {
            if a > 0.0 {
                let to = colour(k % n, k / n, *c);
                for q in 0..3 {
                    c[q] += (to[q] - c[q]) * a;
                }
            }
        });
    }

    /// Multiplies by `f(i, j)` (shade, grain).
    fn multiply(&mut self, f: impl Fn(usize, usize) -> f32 + Sync) {
        let n = self.n;
        self.c.par_iter_mut().enumerate().for_each(|(k, c)| {
            let m = f(k % n, k / n);
            for v in c.iter_mut() {
                *v *= m;
            }
        });
    }
}

/// Draws the radar.
pub fn render(input: &RadarInput, style: &RadarStyle) -> RadarImage {
    let out = style.size.max(16) as usize;
    let n = out * 2;
    let [ax, az, aw, ah] = input.area;
    let side = aw.max(ah);
    let frame = Frame { n, x0: ax + (aw - side) * 0.5, z1: az + ah + (side - ah) * 0.5, m: side / n as f64 };
    let px_m = 1.0 / frame.m;
    let seed = style.seed;

    // The ground's heights and slopes on the canvas.
    let heights: Vec<f32> = (0..n * n).into_par_iter().map(|k| {
        let p = frame.world(k % n, k / n);
        input.height.sample(p[0], p[1])
    }).collect();
    let h_at = |i: i64, j: i64| heights[(j.clamp(0, n as i64 - 1) as usize) * n + i.clamp(0, n as i64 - 1) as usize];
    // The gradient (dh/dx east, dh/dz north) per pixel.
    let grad: Vec<[f32; 2]> = (0..n * n).into_par_iter().map(|k| {
        let (i, j) = ((k % n) as i64, (k / n) as i64);
        let s = (2.0 * frame.m) as f32;
        [(h_at(i + 1, j) - h_at(i - 1, j)) / s, (h_at(i, j - 1) - h_at(i, j + 1)) / s]
    }).collect();
    let slope_deg = |k: usize| f64::from((grad[k][0] * grad[k][0] + grad[k][1] * grad[k][1]).sqrt()).atan().to_degrees();

    // Water: the sea under 0 m, rivers and lakes.
    let water = Mask::from_fn(n, |i, j| {
        let k = j * n + i;
        let sea = input.sea && heights[k] < 0.0;
        let p = frame.world(i, j);
        let wet = input.water.as_ref().is_some_and(|w| w.sample(p[0], p[1]) > 0.01);
        f32::from(u8::from(sea || wet))
    });
    let water_soft = water.blurred(1);
    // The sea alone (its shore gets ripples and sea cliffs), and the water inland (rivers, water planes: still water, no cliffs at its
    // banks).
    let sea_only = Mask::from_fn(n, |i, j| f32::from(u8::from(input.sea && heights[j * n + i] < 0.0)));
    let inland = Mask { n, v: water.v.iter().zip(&sea_only.v).map(|(w, s)| (w - s).max(0.0)).collect() };
    let mut inland_near = inland.blurred(((3.0 * px_m) as usize).max(2));
    if !input.water_planes.is_empty() {
        let polys: Vec<&[P]> = input.water_planes.iter().map(Vec::as_slice).collect();
        inland_near.max_with(&fill_union(&frame, &polys));
    }

    // 1. Paper: the base, broad stains, grain, the faint plots of fields the hand-made ones have.
    let mut canvas = Canvas { n, c: vec![style.paper; n * n] };
    let paper_k = 1.0 / n as f64;
    canvas.c.par_iter_mut().enumerate().for_each(|(k, c)| {
        let (x, y) = ((k % n) as f64 * paper_k, (k / n) as f64 * paper_k);
        let stain = fbm(x * 4.0, y * 4.0, 5, seed);
        let blotch = fbm(x * 11.0 + 3.1, y * 11.0 - 1.7, 3, seed ^ 0xb10c);
        let grain = hash((k % n) as i64, (k / n) as i64, seed ^ 0x9a1) - 0.5;
        // Fibres along the image's rows (user 2026-11-12: horizontal, not vertical).
        let fibre = value_noise(x * 60.0, y * 900.0, seed ^ 0xf1b) - 0.5;
        let t = (stain - 0.5) * 1.3 + (blotch - 0.5) * 0.5;
        for q in 0..3 {
            c[q] = style.paper[q] + (style.paper_dark[q] - style.paper[q]) * t.clamp(-0.6, 1.0) * 0.8;
            c[q] *= 1.0 + grain * 0.12 + fibre * 0.05;
        }
    });

    // Plots: parcels of fields in strips, turned with the lie of the land, each a little lighter or darker, their edges faintly darker.
    canvas.multiply(|i, j| plot_shade(frame.world(i, j), seed));

    // 2. Relief: a soft hill shade from the north-west.
    let light = {
        let l = [-1.0f32, 1.0, 1.4];
        let len = (l[0] * l[0] + l[1] * l[1] + l[2] * l[2]).sqrt();
        [l[0] / len, l[1] / len, l[2] / len]
    };
    let flat = light[2];
    canvas.multiply(|i, j| {
        let k = j * n + i;
        if water.v[k] > 0.5 {
            return 1.0;
        }
        let [gx, gz] = grad[k];
        let nl = (gx * gx + gz * gz + 1.0).sqrt();
        let shade = (-gx * light[0] - gz * light[1] + light[2]) / nl;
        1.0 + (shade - flat) * style.relief * 1.6
    });

    // 3. Cliffs, as the hand-made radars draw them: the steep ground faintly grey, its top edge a cyan line with ink ticks hanging down
    // the slope.
    let mut rock = Mask::from_fn(n, |i, j| {
        let k = j * n + i;
        if water.v[k] > 0.5 { 0.0 } else { smooth(style.cliff_slope as f32 - 4.0, style.cliff_slope as f32 + 2.0, slope_deg(k) as f32) }
    });
    if !input.cliffs.is_empty() {
        let polys: Vec<&[P]> = input.cliffs.iter().map(Vec::as_slice).collect();
        rock.max_with(&fill(&frame, &polys));
    }
    let rock = rock.blurred(1).step(0.45, 0.15);
    canvas.paint_with(&rock, |i, j, c| {
        let p = frame.world(i, j);
        let t = fbm(p[0] / 6.0, p[1] / 6.0, 3, seed ^ 0x40c);
        let mut r = style.rock;
        for q in 0..3 {
            r[q] = r[q] * (0.85 + 0.3 * t) * 0.45 + c[q] * 0.55;
        }
        r
    });
    // Sea cliffs: the land along the shore standing well above the ground just off it (the steep band at the waterline is too thin for
    // the slope test above).
    let sea_rim = if sea_only.v.iter().any(|&w| w > 0.5) {
        let near = sea_only.blurred(1);
        let low = min_filter(&heights, n, ((8.0 * px_m) as usize).max(2));
        Some(Mask::from_fn(n, |i, j| {
            let k = j * n + i;
            let shore = f32::from(u8::from(near.v[k] > 0.02 && water.v[k] < 0.5));
            shore * smooth(3.5, 6.0, heights[k] - low[k]) * f32::from(u8::from(inland_near.v[k] < 0.02))
        }).blurred(1).step(0.35, 0.12))
    } else {
        None
    };

    // Drawn after the water: a sea cliff's rim is along the shore, whose ink would cover it.
    let mut cliff_rims: Option<(Mask, Mask, Mask)> = None;
    if rock.v.iter().any(|&r| r > 0.5) || sea_rim.is_some() {
        // The top edge: just outside the steep ground, where the ground bulges (higher than around it).
        let hm = Mask { n, v: heights.clone() };
        let around = hm.blurred(((6.0 * px_m) as usize).max(3));
        let grown = rock.blurred(((2.5 * px_m) as usize).max(2)).step(0.12, 0.06);
        let rim = Mask::from_fn(n, |i, j| {
            let k = j * n + i;
            let band = (grown.v[k] - rock.v[k]).max(0.0);
            band * smooth(0.0, 0.4, heights[k] - around.v[k]) * (1.0 - water.v[k])
        });
        let rim = rim.blurred(1).step(0.3, 0.12);
        // Not at the banks of a river or a water plane.
        let mut rim = Mask { n, v: rim.v.iter().zip(&inland_near.v).map(|(r, w)| r * (1.0 - smooth(0.0, 0.05, *w))).collect() };
        if let Some(sea) = &sea_rim {
            rim.max_with(sea);
        }
        let ticks = cliff_ticks(&frame, &rim, &grad, 3.0 * px_m, 6.0 * px_m, seed);
        // The rock: where the rim's line is (the same places, a little wider), not the whole steep ground.
        let area = rim.blurred(((1.0 * px_m) as usize).max(1)).step(0.2, 0.08);
        let area = Mask { n, v: area.v.iter().zip(&water.v).map(|(a, w)| a * (1.0 - w)).collect() };
        cliff_rims = Some((rim, ticks, area));
    }

    // 4. Hachures: short ink strokes down the slopes, more and darker the steeper (fewer on the lit side).
    let hach = hachures(&frame, &heights, &grad, &water, style, light);
    canvas.paint(&hach, style.ink, 0.55);

    // 5. Forests: where the trees are dense, olive, edged darker.
    if !input.trees.is_empty() {
        let cell = 4.0;
        let (gw, gh) = (((aw / cell).ceil() as usize).max(1), ((ah / cell).ceil() as usize).max(1));
        let mut count = vec![0.0f32; gw * gh];
        for t in &input.trees {
            let (gi, gj) = (((t[0] - ax) / cell).floor(), ((t[1] - az) / cell).floor());
            if gi >= 0.0 && gj >= 0.0 && (gi as usize) < gw && (gj as usize) < gh {
                count[gj as usize * gw + gi as usize] += 1.0 / (cell * cell) as f32;
            }
        }
        let density = Field { x0: ax, z0: az, w_m: gw as f64 * cell, h_m: gh as f64 * cell, w: gw, h: gh, v: count };
        let raw = Mask::from_fn(n, |i, j| {
            let p = frame.world(i, j);
            density.sample(p[0], p[1])
        });
        // About 12 m around, its edge wobbled.
        let soft = raw.blurred(((4.0 * px_m) as usize).max(1));
        let t = style.forest_density as f32;
        let forest = Mask::from_fn(n, |i, j| {
            let k = j * n + i;
            let wobble = (fbm(i as f64 / 24.0, j as f64 / 24.0, 3, seed ^ 0xf0) - 0.5) * 0.4;
            smooth(t * 0.85, t * 1.15, soft.v[k] * (1.0 + wobble)) * (1.0 - water.v[k])
        });
        canvas.paint_with(&forest, |i, j, c| {
            let p = frame.world(i, j);
            let t = fbm(p[0] / 9.0, p[1] / 9.0, 3, seed ^ 0xf00);
            let mut f = style.forest;
            for q in 0..3 {
                f[q] = f[q] * (0.82 + 0.36 * t) * 0.9 + c[q] * 0.1;
            }
            f
        });
        canvas.paint(&forest.edge(1), style.forest_edge, 0.85);
    }

    // 6. Water: the sea and the rivers, teal, deeper darker, an ink shore and ripples off it.
    if water.v.iter().any(|&w| w > 0.0) {
        let depth_of = |i: usize, j: usize| {
            let k = j * n + i;
            let p = frame.world(i, j);
            let river = input.water.as_ref().map_or(0.0, |w| w.sample(p[0], p[1]));
            let sea = if input.sea { (-heights[k]).max(0.0) } else { 0.0 };
            river.max(sea)
        };
        // Shallow water (a plane barely over mud, a marsh's puddles) shows too, only a little lighter, full from `style.shallow_m` deep.
        let cover = Mask::from_fn(n, |i, j| {
            let k = j * n + i;
            water_soft.v[k] * (0.85 + 0.15 * smooth(0.0, style.shallow_m, depth_of(i, j)).max(sea_only.v[k]))
        });
        let deep = Mask::from_fn(n, |i, j| {
            let k = j * n + i;
            water.v[k] * f32::from(u8::from(sea_only.v[k] > 0.5 || depth_of(i, j) >= style.shallow_m))
        });
        canvas.paint_with(&cover, |i, j, _| {
            let p = frame.world(i, j);
            let d = smooth(0.0, 25.0, depth_of(i, j));
            let t = fbm(p[0] / 40.0, p[1] / 40.0, 4, seed ^ 0x5ea) - 0.5;
            // Streaks on the sea only: still water inland.
            // Along the image's rows, as the paper's fibres.
            let streak = (value_noise(p[0] / 30.0, p[1] / 3.0, seed ^ 0x5eb) - 0.5) * sea_only.v[j * n + i];
            let mut c = [0.0; 3];
            for q in 0..3 {
                c[q] = (style.sea[q] + (style.sea_deep[q] - style.sea[q]) * d) * (1.0 + t * 0.16 + streak * 0.05);
            }
            c
        });

        // Ripples: lines at set distances off the shore, out on the water.
        let far = sea_only.blurred(((6.0 * px_m) as usize).max(2));
        for r in 0..style.ripples {
            let level = 0.62 + 0.11 * r as f32;
            let width = 0.025;
            let line = Mask::from_fn(n, |i, j| {
                let k = j * n + i;
                let v = far.v[k];
                (1.0 - ((v - level).abs() / width).min(1.0)) * sea_only.v[k]
            });
            canvas.paint(&line, style.sea_deep, 0.55 - 0.12 * r as f32);
        }
        // The ink edge: full along deep water, light along shallow water.
        canvas.paint(&water.edge(1), style.ink, 0.35);
        canvas.paint(&deep.edge(1), style.ink, 0.8);
    }

    match &cliff_rims {
        Some((rim, ticks, _)) if style.cliff_style == CliffStyle::Rim => {
            canvas.paint(ticks, style.ink, 0.75);
            canvas.paint(&rim.blurred(1).step(0.1, 0.08), style.ink, 0.6);
            canvas.paint(rim, style.cliff, 1.0);
        }

        Some((_, ticks, area)) => {
            let facet = style.rock_facet_m;
            canvas.paint_with(area, |i, j, _| {
                let k = j * n + i;
                let p = frame.world(i, j);
                let (d1, d2, cx, cz) = voronoi(p[0] / facet, p[1] / facet, seed ^ 0x70c);
                // Each facet turned its own way: lit or shaded by the light, the slope's own shade on top.
                let a = f64::from(hash(cx, cz, seed ^ 0x70d)) * std::f64::consts::TAU;
                let tilt = 0.6f32;
                let (fx, fz) = ((a.cos() as f32) * tilt + grad[k][0] * 0.5, (a.sin() as f32) * tilt + grad[k][1] * 0.5);
                let lit = ((-fx * light[0] - fz * light[1] + light[2]) / (fx * fx + fz * fz + 1.0).sqrt()).clamp(0.0, 1.0);
                let crack = smooth(0.0, 0.1, (d2 - d1) as f32);
                let grain = fbm(p[0] / 2.0, p[1] / 2.0, 2, seed ^ 0x70e);
                let shade = (0.78 + 0.45 * (lit - light[2])) * (0.68 + 0.32 * crack) * (0.92 + 0.16 * grain);
                [style.rock[0] * shade, style.rock[1] * shade, style.rock[2] * shade]
            });
            canvas.paint(ticks, style.ink, 0.3);
            canvas.paint(&area.edge(1), style.ink, 0.8);
        }

        None => {}
    }

    // 7. Roads: the lines, and the blend map's roads where the tile has few lines; cream, softly edged.
    let use_blend = style.blend_roads && input.road_lines.len() < 50;
    let mut road_mask: Option<Mask> = None;
    if let Some(roads) = input.roads.as_ref().filter(|_| use_blend) {
        let m = Mask::from_fn(n, |i, j| {
            let p = frame.world(i, j);
            roads.sample(p[0], p[1])
        });
        // At least `road_width` across.
        let grow = ((style.road_width * 0.5 * px_m) as usize).max(1);
        road_mask = Some(m.blurred(grow).step(0.22, 0.1));
    }
    if !input.road_lines.is_empty() {
        let lines: Vec<(&[P], f64)> = input.road_lines.iter().map(|l| (l.as_slice(), style.road_width * px_m)).collect();
        let m = stroke(&frame, &lines);
        // Not where the lines cover most of the ground around (the paths between a vineyard's rows, a yard's paving): roads are lines.
        let around = m.blurred(((10.0 * px_m) as usize).max(2));
        let m = Mask { n, v: m.v.iter().zip(&around.v).map(|(a, c)| a * (1.0 - smooth(0.4, 0.55, *c))).collect() }.blurred(1);
        match road_mask.as_mut() {
            Some(r) => r.max_with(&m),
            None => road_mask = Some(m),
        }
    }
    if let Some(m) = road_mask {
        let m = Mask { n, v: m.v.iter().zip(&water.v).map(|(a, w)| a * (1.0 - w)).collect() };
        canvas.paint(&m.edge(1), style.paper_dark, 0.35);
        canvas.paint(&m, style.road, 0.85);
    }

    // 8. The settlement: its squares, its streets, its building blocks, the buildings, the walls.
    if !input.plazas.is_empty() {
        let polys: Vec<&[P]> = input.plazas.iter().map(Vec::as_slice).collect();
        let plazas = fill(&frame, &polys).blurred(1);
        canvas.paint(&plazas, style.road, 0.18);
    }

    if !input.streets.is_empty() {
        let lines: Vec<(&[P], f64)> = input.streets.iter().map(|(l, w)| (l.as_slice(), w.max(style.road_width) * px_m)).collect();
        let streets = stroke(&frame, &lines);
        canvas.paint(&streets.blurred(1), style.road, 0.9);
    }

    if !input.buildings.is_empty() {
        let polys: Vec<&[P]> = input.buildings.iter().flat_map(|b| b.polys.iter().map(Vec::as_slice)).collect();
        let bmask = fill_union(&frame, &polys);

        // Blocks: the buildings grown together over the gaps between them, grey, outlined.
        let block = bmask.blurred(((3.0 * px_m) as usize).max(1)).step(0.22, 0.08);
        canvas.paint_with(&block, |i, j, c| {
            let t = fbm(i as f64 / 7.0, j as f64 / 7.0, 2, seed ^ 0xb1);
            let mut b = style.block;
            for q in 0..3 {
                b[q] = b[q] * (0.9 + 0.2 * t) * 0.85 + c[q] * 0.15;
            }
            b
        });
        canvas.paint(&block.edge(1), style.ink, 0.8);

        canvas.paint(&bmask, style.building, 0.85);
        let red: Vec<&[P]> = input.buildings.iter().filter(|b| b.red).flat_map(|b| b.polys.iter().map(Vec::as_slice)).collect();
        if !red.is_empty() {
            let rmask = fill_union(&frame, &red);
            canvas.paint(&rmask, style.roof, 0.85);
        }
        canvas.paint(&crisp_edge(&bmask), style.ink, 0.75);
    }

    // Walls: thick ink, as the hand-made radars draw them; gates a gap of the street's colour across.
    if !input.walls.is_empty() {
        let rings: Vec<&[P]> = input.walls.iter().flat_map(|w| w.footprint.iter().map(Vec::as_slice)).collect();
        let lines: Vec<(&[P], f64)> = input.walls.iter().filter(|w| w.line[0] != w.line[1]).map(|w| (&w.line[..], if w.palisade { 2.5 } else { 3.5 } * px_m)).collect();
        let mut m = fill(&frame, &rings);
        m.max_with(&stroke(&frame, &lines));
        let m = m.blurred(1).step(0.15, 0.1);
        canvas.paint(&m, style.ink, 0.95);

        let gates: Vec<&[P]> = input.walls.iter().filter(|w| w.gate).flat_map(|w| w.footprint.iter().map(Vec::as_slice)).collect();
        if !gates.is_empty() {
            let g = fill(&frame, &gates).blurred(1).step(0.5, 0.1);
            canvas.paint(&g, style.paper_dark, 0.9);
        }
    }

    // 9. Age: a vignette and a last grain.
    canvas.multiply(|i, j| {
        let (x, y) = ((i as f32 + 0.5) / n as f32 - 0.5, (j as f32 + 0.5) / n as f32 - 0.5);
        let r = (x * x + y * y).sqrt() * 1.41;
        let edge = fbm(f64::from(x) * 6.0, f64::from(y) * 6.0, 3, seed ^ 0xed) - 0.5;
        1.0 - smooth(0.55, 1.15, r + edge * 0.25) * 0.28
    });

    // Down to the image's size (2 x 2 averaged).
    let mut rgba = vec![255u8; out * out * 4];
    rgba.par_chunks_mut(out * 4).enumerate().for_each(|(j, row)| {
        for i in 0..out {
            let mut s = [0.0f32; 3];
            for (di, dj) in [(0, 0), (1, 0), (0, 1), (1, 1)] {
                let c = canvas.c[(j * 2 + dj) * n + i * 2 + di];
                for q in 0..3 {
                    s[q] += c[q] * 0.25;
                }
            }
            for q in 0..3 {
                row[i * 4 + q] = s[q].round().clamp(0.0, 255.0) as u8;
            }
        }
    });

    RadarImage { size: out as u32, rgba }
}

/// The least of `v` (an `n` x `n` raster) within `r` pixels (a square), per pixel.
fn min_filter(v: &[f32], n: usize, r: usize) -> Vec<f32> {
    let pass = |src: &[f32], horizontal: bool| -> Vec<f32> {
        (0..n * n).into_par_iter().map(|k| {
            let (i, j) = (k % n, k / n);
            if horizontal {
                (i.saturating_sub(r)..=(i + r).min(n - 1)).map(|x| src[j * n + x]).fold(f32::MAX, f32::min)
            } else {
                (j.saturating_sub(r)..=(j + r).min(n - 1)).map(|y| src[y * n + i]).fold(f32::MAX, f32::min)
            }
        }).collect()
    };
    pass(&pass(v, true), false)
}

/// Distances to the nearest and second nearest points of a jittered grid (cells of 1), and the nearest point's cell.
fn voronoi(x: f64, y: f64, seed: u32) -> (f64, f64, i64, i64) {
    let (cx, cy) = (x.floor() as i64, y.floor() as i64);
    let (mut d1, mut d2, mut best) = (f64::MAX, f64::MAX, (0, 0));
    for dy in -1..=1 {
        for dx in -1..=1 {
            let (gx, gy) = (cx + dx, cy + dy);
            let px = gx as f64 + f64::from(hash(gx, gy, seed));
            let py = gy as f64 + f64::from(hash(gx, gy, seed ^ 0x55));
            let d = ((x - px).powi(2) + (y - py).powi(2)).sqrt();
            if d < d1 {
                d2 = d1;
                d1 = d;
                best = (gx, gy);
            } else if d < d2 {
                d2 = d;
            }
        }
    }
    (d1, d2, best.0, best.1)
}

/// The fields' pattern on the paper at a point: a multiplier about 1.
fn plot_shade(p: P, seed: u32) -> f32 {
    // Parcels of about 80 m, turned the same way over regions of about 300 m (the nearest of a jittered grid's points).
    let region = 300.0;
    let (gx, gz) = (p[0] / region, p[1] / region);
    let (cx, cz) = (gx.floor() as i64, gz.floor() as i64);
    let mut best = (f64::MAX, 0i64, 0i64);
    for dz in -1..=1 {
        for dx in -1..=1 {
            let (x, z) = (cx + dx, cz + dz);
            let d = (gx - (x as f64 + f64::from(hash(x, z, seed ^ 0x51)))).powi(2) + (gz - (z as f64 + f64::from(hash(x, z, seed ^ 0x52)))).powi(2);
            if d < best.0 {
                best = (d, x, z);
            }
        }
    }
    let a = f64::from(hash(best.1, best.2, seed ^ 0x9a)) * std::f64::consts::PI;
    let (sn, cs) = a.sin_cos();
    let (u, v) = (p[0] * cs - p[1] * sn, p[0] * sn + p[1] * cs);
    let parcel = 80.0;
    let (pu, pv) = ((u / parcel).floor() as i64, (v / parcel).floor() as i64);
    // Strips across a parcel, 10 .. 30 m wide, along u or v.
    let along_u = hash(pu, pv, seed ^ 0x61) < 0.5;
    let strip = 10.0 + 20.0 * f64::from(hash(pu, pv, seed ^ 0x62));
    let (x, y) = if along_u { (u, v) } else { (v, u) };
    let k = ((y - (if along_u { pv } else { pu }) as f64 * parcel) / strip).floor() as i64;
    let tone = (hash(pu * 31 + k, pv * 17 - k, seed ^ 0x63) - 0.5) * 0.14;
    // Edges: within 0.8 m of a strip's or a parcel's side.
    let fy = (y.rem_euclid(parcel)).rem_euclid(strip);
    let (fu, fv) = (u.rem_euclid(parcel), v.rem_euclid(parcel));
    let near = |f: f64, w: f64| f < 0.8 || f > w - 0.8;
    let edge = near(fy, strip) || near(fu, parcel) || near(fv, parcel);
    let _ = x;
    let wild = fbm(p[0] / 250.0, p[1] / 250.0, 2, seed ^ 0x64);
    // Fields fade out here and there (pasture, rough ground).
    let show = smooth(0.35, 0.55, wild);
    1.0 + (tone - if edge { 0.05 } else { 0.0 }) * show
}

/// Ticks down a cliff from its top edge: a short stroke down the slope every `every` pixels along the edge.
fn cliff_ticks(frame: &Frame, rim: &Mask, grad: &[[f32; 2]], every: f64, len: f64, seed: u32) -> Mask {
    let n = frame.n;
    let cell = every.max(2.0) as usize;
    let mut lines: Vec<Vec<P>> = vec![];
    for cj in 0..n / cell {
        for ci in 0..n / cell {
            // The strongest rim pixel of the cell.
            let mut best = (0.5f32, 0usize, 0usize);
            for j in cj * cell..(cj + 1) * cell {
                for i in ci * cell..(ci + 1) * cell {
                    let v = rim.v[j * n + i];
                    if v > best.0 {
                        best = (v, i, j);
                    }
                }
            }
            if best.0 <= 0.5 {
                continue;
            }
            let (i, j) = (best.1, best.2);
            // Down the slope a little further on, where it is steep.
            let [gx, gz] = grad[j * n + i];
            let g = (gx * gx + gz * gz).sqrt();
            if g < 1e-3 {
                continue;
            }
            let p = frame.world(i, j);
            let l = len * frame.m * (0.8 + 0.4 * f64::from(hash(ci as i64, cj as i64, seed ^ 0x71)));
            let d = [-f64::from(gx / g) * l, -f64::from(gz / g) * l];
            lines.push(vec![p, [p[0] + d[0], p[1] + d[1]]]);
        }
    }
    let strokes: Vec<(&[P], f64)> = lines.iter().map(|l| (l.as_slice(), 1.4)).collect();
    stroke(frame, &strokes)
}

/// Hachures: strokes along the fall line on a jittered grid, longer and darker the steeper.
fn hachures(frame: &Frame, heights: &[f32], grad: &[[f32; 2]], water: &Mask, style: &RadarStyle, light: [f32; 3]) -> Mask {
    let n = frame.n;
    let _ = heights;
    let spacing = 7.0f64;
    let mut lines: Vec<Vec<P>> = vec![];
    let cells = (n as f64 / spacing) as i64;
    let seed = style.seed ^ 0x4ac;
    for cj in 0..cells {
        for ci in 0..cells {
            let x = (ci as f64 + f64::from(hash(ci, cj, seed))) * spacing;
            let y = (cj as f64 + f64::from(hash(ci, cj, seed ^ 1))) * spacing;
            let (i, j) = (x as usize, y as usize);
            if i >= n || j >= n {
                continue;
            }
            let k = j * n + i;
            if water.v[k] > 0.0 {
                continue;
            }
            let [gx, gz] = grad[k];
            let g = f64::from((gx * gx + gz * gz).sqrt());
            let deg = g.atan().to_degrees();
            if deg < style.hachure_slope {
                continue;
            }

            // Fewer on the lit side.
            let lit = f64::from((-gx * light[0] - gz * light[1]) / g as f32).max(0.0);
            let t = ((deg - style.hachure_slope) / (style.cliff_slope - style.hachure_slope)).clamp(0.0, 1.0);
            if f64::from(hash(ci, cj, seed ^ 2)) > 0.35 + 0.65 * t - 0.3 * lit {
                continue;
            }

            // Down the slope, in pixels (z north is up the image).
            let (dx, dy) = (f64::from(gx) / g, -f64::from(gz) / g);
            let len = spacing * (0.45 + 0.6 * t);
            let p = frame.world(i, j);
            let half = len * 0.5 * frame.m;
            lines.push(vec![[p[0] - dx * half, p[1] + dy * half], [p[0] + dx * half, p[1] - dy * half]]);
        }
    }

    let strokes: Vec<(&[P], f64)> = lines.iter().map(|l| (l.as_slice(), 1.2)).collect();
    stroke(frame, &strokes)
}

/// The blend map's roads: the parts of its thin layers that are narrow and long (`docs/plans/radar.md`, "Roads"). `weights` are the 16
/// channels interleaved (`w * h * 16`, rows along +z) over (x0, z0, w_m, h_m).
pub fn blend_roads(weights: &[u16], w: usize, h: usize, area: [f64; 4]) -> Field {
    let [x0, z0, w_m, h_m] = area;
    let mpp = w_m / w as f64;
    // Narrower than about 10 m.
    let r = ((5.0 / mpp).round() as usize).max(1);
    let min_extent_px = 120.0 / mpp;

    let mut road = vec![0.0f32; w * h];
    for c in 0..16 {
        let mask: Vec<bool> = (0..w * h).into_par_iter().map(|k| weights[k * 16 + c] > 32768).collect();
        let count = mask.iter().filter(|&&m| m).count();
        let cover = count as f64 / (w * h) as f64;
        if count == 0 || cover > 0.3 {
            continue;
        }
        // What is left of the layer once its wide parts are taken away, and their rims with them.
        let opened = dilate(&erode(&mask, w, h, r), w, h, r + 2);
        let thin: Vec<bool> = mask.iter().zip(&opened).map(|(&m, &o)| m && !o).collect();
        let thin_n = thin.iter().filter(|&&t| t).count();
        if (thin_n as f64) < 0.2 * count as f64 {
            continue;
        }

        // Keep the long pieces.
        for comp in components(&thin, w, h) {
            let (mut lo, mut hi) = ([usize::MAX; 2], [0usize; 2]);
            for &k in &comp {
                let (i, j) = (k % w, k / w);
                lo = [lo[0].min(i), lo[1].min(j)];
                hi = [hi[0].max(i), hi[1].max(j)];
            }
            let ext = (((hi[0] - lo[0]) as f64).powi(2) + ((hi[1] - lo[1]) as f64).powi(2)).sqrt();
            if ext < min_extent_px {
                continue;
            }
            for k in comp {
                road[k] = road[k].max(f32::from(weights[k * 16 + c]) / 65535.0);
            }
        }
    }

    Field { x0, z0, w_m, h_m, w, h, v: road }
}

fn erode(m: &[bool], w: usize, h: usize, r: usize) -> Vec<bool> {
    let pass = |m: &[bool], horizontal: bool| -> Vec<bool> {
        (0..w * h).into_par_iter().map(|k| {
            let (i, j) = (k % w, k / w);
            if horizontal {
                (i.saturating_sub(r)..=(i + r).min(w - 1)).all(|x| m[j * w + x])
            } else {
                (j.saturating_sub(r)..=(j + r).min(h - 1)).all(|y| m[y * w + i])
            }
        }).collect()
    };
    pass(&pass(m, true), false)
}

fn dilate(m: &[bool], w: usize, h: usize, r: usize) -> Vec<bool> {
    let pass = |m: &[bool], horizontal: bool| -> Vec<bool> {
        (0..w * h).into_par_iter().map(|k| {
            let (i, j) = (k % w, k / w);
            if horizontal {
                (i.saturating_sub(r)..=(i + r).min(w - 1)).any(|x| m[j * w + x])
            } else {
                (j.saturating_sub(r)..=(j + r).min(h - 1)).any(|y| m[y * w + i])
            }
        }).collect()
    };
    pass(&pass(m, true), false)
}

/// The 8-connected components of a mask (pixel indices).
fn components(m: &[bool], w: usize, h: usize) -> Vec<Vec<usize>> {
    let mut seen = vec![false; w * h];
    let mut out = vec![];
    for start in 0..w * h {
        if !m[start] || seen[start] {
            continue;
        }
        let mut comp = vec![];
        let mut stack = vec![start];
        seen[start] = true;
        while let Some(k) = stack.pop() {
            comp.push(k);
            let (i, j) = ((k % w) as i64, (k / w) as i64);
            for dj in -1..=1 {
                for di in -1..=1 {
                    let (x, y) = (i + di, j + dj);
                    if x < 0 || y < 0 || x >= w as i64 || y >= h as i64 {
                        continue;
                    }
                    let q = y as usize * w + x as usize;
                    if m[q] && !seen[q] {
                        seen[q] = true;
                        stack.push(q);
                    }
                }
            }
        }
        out.push(comp);
    }
    out
}
