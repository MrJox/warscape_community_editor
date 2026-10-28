//! BOB's procedural vegetation generator (`EMPIREUTILITY::PROCEDURAL_OBJECTS`, EmpireUtility.AssemblyKit.dll), step for step: candidate points
//! from the seed, a group and an object for each, `group_objects`, `remove_objects_based_on_min_seperation`. Floating point is single precision
//! in the order BOB computes it, and every container is iterated in BOB's order, as both decide the random draws. docs/re/vegetation.md has
//! the addresses and what each step does.

use std::collections::HashSet;

use super::ca_map::{CaMap, Vec2Key};
use super::params::{GenerationParams, GroupParams, ObjectParams};

/// `1 / 65535` as BOB has it.
const K: f32 = f32::from_bits(0x3780_0080);

/// The MSVC `rand` LCG BOB draws with (`CA::Rand`).
#[derive(Clone, Copy, Debug)]
pub struct Rand(pub u32);

impl Rand {
    pub fn step(s: u32) -> u32 {
        s.wrapping_mul(0x343fd).wrapping_add(0x269ec3)
    }

    pub fn hi(&mut self) -> u32 {
        self.0 = Self::step(self.0);
        self.0 >> 16
    }

    /// `hi * (1 / 65535)`.
    pub fn unit(&mut self) -> f32 {
        self.hi() as f32 * K
    }
}

/// What the generator reads of a tile (`IMAGE_VALUE_DATA_PROVIDER`).
pub struct TileInputs {
    /// Height map in metres, `map_w` by `map_h`, row 0 first.
    pub heights: Vec<f32>,

    /// 16 blend values per pixel of the height map (4 textures x 4 channels), 0..1.
    pub blend: Vec<f32>,
    pub map_w: usize,
    pub map_h: usize,
    pub cells_x: u32,
    pub cells_y: u32,
    pub tri_density: u32,

    /// The tile's texture groups (TEXTURE_GROUP red, green, blue, alpha): texture i of the blend map.
    pub texture_groups: Vec<String>,

    /// Subtiles generation may use (x, y as BOB stores them: the mask's rows from the bottom).
    pub cells: HashSet<(i32, i32)>,

    /// Scene units per metre (2).
    pub render_scale: f32,

    /// `VegetationExclusionPolygonInstance` outlines, in generator coordinates.
    pub exclusions: Vec<Vec<(f32, f32)>>,

    /// Each outline's least and greatest y: a point outside `[least, greatest)` crosses none of its edges ([`point_in`] counts an edge
    /// from its lower end, excluding the upper), so it is outside without the edges being looked at.
    pub exclusion_y: Vec<(f32, f32)>,

    /// What the lists' heights are multiplied by.
    pub height_alpha: super::height_alpha::HeightAlpha,

    /// [`candidates`] by seed and density (bits), with the random state after them: the generators of a tile's categories share seeds
    /// and densities, and the points depend on nothing else.
    pub candidate_cache: CandidateCache,
}

/// [`TileInputs::candidate_cache`].
pub type CandidateCache = std::sync::Mutex<std::collections::HashMap<(u32, u32), std::sync::Arc<(Vec<Candidate>, u32)>>>;

/// A hasher for the candidates' `u32` keys (float bits): a multiply, as a set's order is not used (its keys are sorted).
#[derive(Default)]
struct U32Hasher(u64);

impl std::hash::Hasher for U32Hasher {
    fn finish(&self) -> u64 {
        self.0
    }

    fn write(&mut self, bytes: &[u8]) {
        for &b in bytes {
            self.0 = (self.0 ^ u64::from(b)).wrapping_mul(0x0100_0000_01b3);
        }
    }

    fn write_u32(&mut self, v: u32) {
        self.0 = u64::from(v).wrapping_mul(0x9e37_79b9_7f4a_7c15);
    }
}

impl TileInputs {
    fn border(&self) -> i32 {
        // trunc(0.5 - tri): -127 for 128.
        (0.5 - self.tri_density as f32) as i32
    }

    /// Pixels of the generator's grid along x and y: the tile plus a pixel each side.
    pub fn grid(&self) -> (u32, u32) {
        let b = self.border();
        let w = (self.cells_x as i32 + 2) * self.tri_density as i32 + 2 * b;
        let h = (self.cells_y as i32 + 2) * self.tri_density as i32 + 2 * b;
        (w.max(0) as u32, h.max(0) as u32)
    }

    /// The height map pixel a list writer samples for a generator position (ACTION_GENERATE_VEGETATION / ACTION_GRASS::run):
    /// `tri_density + p / cell`, truncated (x87 with the rounding set to chop).
    pub fn list_pixel(&self, x: f32, y: f32) -> Option<(usize, usize)> {
        let inv_cell = 1.0 / self.cell();
        let tri = f64::from(self.tri_density);
        let col = (tri + f64::from(x * inv_cell)).trunc() as i64;
        let row = (tri + f64::from(y * inv_cell)).trunc() as i64;
        (col >= 0 && row >= 0 && (col as usize) < self.map_w && (row as usize) < self.map_h).then_some((col as usize, row as usize))
    }

    /// A list's height at a [`Self::list_pixel`]: the height map times the height alpha map.
    pub fn list_height(&self, col: usize, row: usize) -> f32 {
        self.heights[row * self.map_w + col] * self.height_alpha.get(col, row)
    }

    /// Metres per pixel of the grid, in scene units: m_terrain_tile_size (128) / tri_density * render_scale.
    pub fn cell(&self) -> f32 {
        (128.0f32 / self.tri_density as f32) * self.render_scale
    }

    fn valid(&self, px: u32, py: u32) -> bool {
        let div = (0.5f64 - f64::from(self.border())) as f32;
        let kx = ((f64::from(px) / f64::from(div)) as f32).floor() as i32;
        let ky = ((f64::from(py) / f64::from(div)) as f32).floor() as i32;
        self.cells.contains(&(kx, ky))
    }

    fn map_index(&self, px: u32, py: u32) -> Option<usize> {
        let b = self.border();
        let x = px as i64 - i64::from(b);
        let y = py as i64 - i64::from(b);
        (x >= 0 && y >= 0 && (x as usize) < self.map_w && (y as usize) < self.map_h).then(|| y as usize * self.map_w + x as usize)
    }

    /// What the generator reads of a pixel, computed once: its 16 channels (`FUN_1028bb40`: a channel, divided by its texture's sum when that
    /// is over 1) and its height (`FUN_1028bdc0`); zero for a pixel outside the usable cells or the map.
    pub(super) fn pixel(&self, px: u32, py: u32) -> Pixel {
        let mut out = Pixel { norm: [0.0; 16], height: 0.0 };
        if !self.valid(px, py) {
            return out;
        }

        let Some(i) = self.map_index(px, py) else { return out };
        out.height = self.heights[i];
        let pix = &self.blend[i * 16..i * 16 + 16];
        for t in 0..4 {
            let mut sum = 0.0f32;
            for c in 0..4 {
                sum = if c == 0 { pix[t * 4] + sum } else { sum + pix[t * 4 + c] };
            }

            for c in 0..4 {
                let v = pix[t * 4 + c];
                out.norm[t * 4 + c] = if 1.0 < sum { (1.0 / sum) * v } else { v };
            }
        }

        out
    }

    /// The textures `probability_blendmap_value_for_channel_at_pixel` reads for a group name: one channel of the group's textures (see
    /// [`GroupView`]); an unknown group reads the alphabetically first one.
    fn probability_textures(&self, name: &str) -> Vec<usize> {
        let textures = self.textures(name);
        if !textures.is_empty() {
            return textures;
        }

        let mut names: Vec<&String> = self.texture_groups.iter().collect();
        names.sort();
        names.first().map_or_else(Vec::new, |first| self.textures(first))
    }

    fn textures(&self, name: &str) -> Vec<usize> {
        self.texture_groups.iter().enumerate().filter(|(_, g)| *g == name).map(|(i, _)| i).collect()
    }

    fn excluded(&self, x: f32, y: f32) -> bool {
        self.exclusions.iter().zip(&self.exclusion_y).any(|(poly, &(lo, hi))| lo <= y && y < hi && point_in(poly, x, y))
    }
}

/// A pixel as the generator reads it ([`TileInputs::pixel`]).
pub(super) struct Pixel {
    pub(super) norm: [f32; 16],
    pub(super) height: f32,
}

impl Pixel {
    /// `summed_channel_blendmap`: the positive channels of every texture of the group (its textures, in order).
    fn summed(&self, textures: &[usize]) -> f32 {
        let mut acc = 0.0f32;
        for &t in textures {
            for c in 0..4 {
                let v = self.norm[t * 4 + c];
                if v > 0.0 {
                    acc = v + acc;
                }
            }
        }

        acc
    }
}

/// A group of the category, resolved once per generator run: its parameters, the textures of its channel group and of its probability
/// channel group, and the channel [`TileInputs::probability_value`] reads.
struct GroupView<'a> {
    params: Option<&'a GroupParams>,
    sum_textures: Vec<usize>,
    probability_textures: Vec<usize>,
    probability_channel: Option<usize>,
}

impl<'a> GroupView<'a> {
    fn new(params: &'a GenerationParams, inputs: &TileInputs, name: &String) -> Self {
        let Some(g) = params.groups.get(name) else { return Self { params: None, sum_textures: vec![], probability_textures: vec![], probability_channel: None } };
        let channel = inputs.probability_textures(&g.channel_group_for_probability).get((g.channel_number >> 2) as usize).map(|&t| (g.channel_number & 3) as usize + t * 4);
        Self { params: Some(g), sum_textures: inputs.textures(&g.channel_group), probability_textures: inputs.textures(&g.channel_group_for_probability), probability_channel: channel }
    }
}

/// `UTILITYLIB::OUTLINE` contains (FUN_10a7d7e0): a crossing count over the edges from each point to the next, half-open in y (from the
/// lower end, excluding the upper), crossing when (y - y0) * (x1 - x0) > (x - x0) * (y1 - y0) with (x0, y0) the lower end; no division.
pub fn point_in(poly: &[(f32, f32)], x: f32, y: f32) -> bool {
    let mut inside = false;
    let n = poly.len();
    for i in 0..n {
        let a = poly[i];
        let b = poly[(i + 1) % n];
        let (lo, hi) = if a.1 <= b.1 { (a, b) } else { (b, a) };
        if lo.1 <= y && y < hi.1 {
            let lhs = (y - lo.1) * (hi.0 - lo.0);
            let rhs = (x - lo.0) * (hi.1 - lo.1);
            if lhs > rhs {
                inside = !inside;
            }
        }
    }

    inside
}

/// A candidate (`PROCEDURAL_POINT_GENERATION::generate_points`).
#[derive(Clone, Copy, Debug)]
pub struct Candidate {
    pub x: f32,
    pub y: f32,
    pub px: u32,
    pub py: u32,
}

pub fn candidates(inputs: &TileInputs, density: f32, rand: &mut Rand) -> Vec<Candidate> {
    candidates_with(inputs, density, rand, &|x, y| inputs.excluded(x, y))
}

/// [`candidates`] through the tile's cache: the same points and random state after them.
pub(super) fn cached_candidates(inputs: &TileInputs, density: f32, rand: &mut Rand) -> std::sync::Arc<(Vec<Candidate>, u32)> {
    let key = (rand.0, density.to_bits());
    if let Some(hit) = inputs.candidate_cache.lock().ok().and_then(|c| c.get(&key).cloned()) {
        rand.0 = hit.1;
        return hit;
    }

    let points = candidates(inputs, density, rand);
    let made = std::sync::Arc::new((points, rand.0));
    if let Ok(mut c) = inputs.candidate_cache.lock() {
        c.insert(key, made.clone());
    }

    made
}

/// [`candidates`] with the generator's `position_should_be_discarded` (the trees': the exclusion polygons; the grass': outside a box).
pub fn candidates_with(inputs: &TileInputs, density: f32, rand: &mut Rand, discard: &dyn Fn(f32, f32) -> bool) -> Vec<Candidate> {
    let (w, h) = inputs.grid();
    let cell = inputs.cell();
    if w == 0 || h == 0 {
        return vec![];
    }

    let total = (w * h) as f32;
    let n = cell * cell * total * density;
    if n == 0.0 {
        return vec![];
    }

    let mut count = n.ceil() as u64;
    let mut x1 = n;
    if count as f32 >= total {
        x1 = total - 1.0;
        count = x1.ceil() as u64;
    }

    let total2 = ((total / x1) * x1.ceil()).ceil();
    let t2k = total2 * K;
    let mut keys: HashSet<u32, std::hash::BuildHasherDefault<U32Hasher>> = HashSet::with_capacity_and_hasher(count as usize, Default::default());
    let mut s = rand.0;
    for _ in 0..count {
        loop {
            let u1 = Rand::step(s);
            let u2 = Rand::step(u1);
            s = u2;
            let v = ((u2 >> 16) as f32 * K) * t2k + total2 * ((u1 >> 16) as f32 * K);
            if !keys.contains(&v.to_bits()) && v < total2 {
                keys.insert(v.to_bits());
                break;
            }
        }
    }

    rand.0 = s;
    let mut sorted: Vec<f32> = keys.into_iter().map(f32::from_bits).collect();
    sorted.sort_by(f32::total_cmp);

    let mut out = Vec::new();
    let mut it = 0usize;
    let mut c = 0.0f32;
    let mut xpos = cell * 0.5;
    for i in 0..w {
        let mut ypos = cell * 0.5;
        for j in 0..h {
            c += 1.0;
            if c >= sorted[it] {
                it += 1;
                while it < sorted.len() && c >= sorted[it] {
                    it += 1;
                }

                if it >= sorted.len() {
                    return out;
                }

                if !discard(xpos, ypos) {
                    out.push(Candidate { x: xpos, y: ypos, px: i, py: j });
                }
            }

            ypos = cell + ypos;
        }

        xpos += cell;
    }

    out
}

/// A placed object (`CHOSEN_OBJECT_INFO`).
#[derive(Clone, Debug, PartialEq)]
pub struct Chosen {
    pub model: String,
    pub x: f32,
    pub y: f32,
    pub size: f32,
    pub min_separation: f32,
    pub grouping_probability: f32,
    pub grouping_radius: f32,
    pub width: f32,
    pub group: String,
    pub include_in_outfield: bool,
    pub kind: u32,

    /// A static tree's height (its instance's world y); 0 for a generated object, whose list height is the terrain's.
    pub height: f32,

    /// A static tree whose instance is `MSM_FREEFORM`.
    pub freeform: bool,
}

/// The cumulative pick of BOB's `MAP<float, ...>`: the first entry whose running total is >= r.
fn pick<T: Copy>(cum: &[(f32, T)], total: f32, rand: &mut Rand) -> Option<T> {
    let r = rand.unit() * total;
    cum.iter().find(|(c, _)| r <= *c).map(|(_, v)| *v)
}

struct Quadtree {
    nodes: Vec<QNode>,
}

struct QNode {
    min: (f32, f32),
    max: (f32, f32),
    kids: Option<[usize; 4]>,
    items: CaMap<Vec2Key, Chosen>,
}

impl Quadtree {
    fn new(min: (f32, f32), max: (f32, f32), tiers: u32) -> Self {
        let mut t = Self { nodes: vec![] };
        t.build(min, max, tiers);
        t
    }

    fn build(&mut self, min: (f32, f32), max: (f32, f32), tiers: u32) -> usize {
        let id = self.nodes.len();
        self.nodes.push(QNode { min, max, kids: None, items: CaMap::default() });
        if tiers > 1 {
            let mx = min.0 + (max.0 - min.0) * 0.5;
            let my = (max.1 - min.1) * 0.5 + min.1;
            let a = self.build((min.0, my), (mx, max.1), tiers - 1);
            let b = self.build((mx, my), max, tiers - 1);
            let c = self.build(min, (mx, my), tiers - 1);
            let d = self.build((mx, min.1), (max.0, my), tiers - 1);
            self.nodes[id].kids = Some([a, b, c, d]);
        }

        id
    }

    fn contains(n: &QNode, b: (f32, f32, f32, f32)) -> bool {
        n.min.0 <= b.0 && n.min.1 <= b.1 && b.2 <= n.max.0 && b.3 <= n.max.1
    }

    /// `find_container_node`.
    fn container(&self, b: (f32, f32, f32, f32)) -> usize {
        let mut n = 0;
        while Self::contains(&self.nodes[n], b) {
            let Some(kids) = self.nodes[n].kids else { return n };
            match kids.iter().find(|&&k| Self::contains(&self.nodes[k], b)) {
                Some(&k) => n = k,
                None => return n,
            }
        }

        n
    }

    /// The non-empty nodes a circle touches, depth first.
    fn query(&self, n: usize, c: (f32, f32, f32), out: &mut Vec<usize>) {
        let node = &self.nodes[n];
        match circle_box(c, (node.min.0, node.min.1, node.max.0, node.max.1)) {
            0 => self.subtree(n, out),
            1 => {
                if !node.items.is_empty() {
                    out.push(n);
                }

                if let Some(kids) = node.kids {
                    for k in kids {
                        self.query(k, c, out);
                    }
                }
            }
            _ => {}
        }
    }

    fn subtree(&self, n: usize, out: &mut Vec<usize>) {
        if !self.nodes[n].items.is_empty() {
            out.push(n);
        }

        if let Some(kids) = self.nodes[n].kids {
            for k in kids {
                self.subtree(k, out);
            }
        }
    }
}

/// `LINE_2::intersects` with a circle.
fn segment_hits(p0: (f32, f32), p1: (f32, f32), c: (f32, f32, f32)) -> bool {
    let dx = p0.0 - c.0;
    let dy = p0.1 - c.1;
    let fx = p1.0 - p0.0;
    let fy = p1.1 - p0.1;
    let a = fx * fx + fy * fy;
    let b = (fy * dy + fx * dx) * 2.0;
    let disc = b * b - ((dy * dy + dx * dx) - c.2 * c.2) * a * 4.0;
    if !(0.0 <= disc) || a == 0.0 {
        return false;
    }

    let s = if disc == 0.0 { 0.0 } else { disc.sqrt() };
    let inv = 1.0 / (a * 2.0);
    let t1 = (-b - s) * inv;
    let t2 = (s - b) * inv;
    let (ok, t) = if 0.0 <= t1 { (true, t1) } else { (0.0 <= t2, t2) };
    ok && (t2 <= 1.0 || t <= 1.0)
}

/// `FUN_10a858a0`: 2 outside, 1 touching, 0 the box inside the circle.
fn circle_box(c: (f32, f32, f32), b: (f32, f32, f32, f32)) -> u8 {
    let (cx, cy, r) = c;
    let (x0, y0, x1, y1) = b;
    let rh = (r * 2.0) * 0.5;
    if x1 < cx - rh || y1 < cy - rh || rh + cx < x0 || !(y0 <= rh + cy) {
        return 2;
    }

    let r2 = r * r;
    let dy = cy - y1;
    if r2 < dy * dy + (cx - x0) * (cx - x0) {
        if cx - rh < x0 || cy - rh < y0 || x1 < rh + cx || y1 < rh + cy {
            let edges = [((x0, y0), (x1, y0)), ((x1, y0), (x1, y1)), ((x1, y1), (x0, y1)), ((x0, y1), (x0, y0))];
            if !edges.iter().any(|&(p, q)| segment_hits(p, q, c)) {
                return 2;
            }
        }

        return 1;
    }

    let d = |px: f32, py: f32| (cy - py) * (cy - py) + (cx - px) * (cx - px);
    if d(x1, y1) <= r2 && d(x0, y0) <= r2 && d(x1, y0) <= r2 { 0 } else { 1 }
}

/// One generator (`PROCEDURAL_OBJECTS` + `generate_objects`) of a category and object mask, its seed and density. The objects left, in
/// the order of BOB's generated map.
fn trace_window() -> Option<(f32, f32, f32)> {
    let v: Vec<f32> = std::env::var("WCE_VEG_TRACE").ok()?.split(',').filter_map(|x| x.parse().ok()).collect();
    (v.len() == 3).then(|| (v[0], v[1], v[2]))
}

pub fn generate(params: &GenerationParams, inputs: &TileInputs, category: usize, mask: u32, seed: u32, density: f32) -> Vec<Chosen> {
    generate_with(params, inputs, category, mask, seed, density, &[])
}

/// [`generate`], then `statics` added to the generated map (where nothing is at their position yet), as BOB adds the static trees to the tree
/// generator's map before writing: they don't take part in the generation, but their place in the map's order is the list's.
pub fn generate_with(params: &GenerationParams, inputs: &TileInputs, category: usize, mask: u32, seed: u32, density: f32, statics: &[Chosen]) -> Vec<Chosen> {
    with_statics(generate_map(params, inputs, category, mask, seed, density), statics)
}

/// A generated map with `statics` added (see [`generate_with`]), in the map's order.
pub(super) fn with_statics(mut live: CaMap<Vec2Key, Chosen>, statics: &[Chosen]) -> Vec<Chosen> {
    for c in statics {
        live.insert(Vec2Key(c.x, c.y), c.clone());
    }

    live.iter().map(|(_, c)| c.clone()).collect()
}

/// One generator's map (see [`generate`]), for callers that take more than one list from the same run.
pub(super) fn generate_map(params: &GenerationParams, inputs: &TileInputs, category: usize, mask: u32, seed: u32, density: f32) -> CaMap<Vec2Key, Chosen> {
    let objects = &params.objects;

    // The category's objects of the generator's humidity (0, arid), by group, in the category map's order.
    let mut groups: CaMap<String, CaMap<String, usize>> = CaMap::default();
    for (key, &o) in params.by_category[category].iter() {
        if params.by_humidity[0].contains(key) {
            let (inner, _) = groups.insert(objects[o].group.clone(), CaMap::default());
            inner.insert(key.clone(), o);
        }
    }

    let mut rand = Rand(seed);
    let points = {
        wce_foundation::wce_prof!("veg.candidates");
        cached_candidates(inputs, density, &mut rand)
    };
    let points = &points.0;
    if points.is_empty() {
        return CaMap::default();
    }

    let (mut minx, mut miny, mut maxx, mut maxy) = (f32::MAX, f32::MAX, 0.0f32, 0.0f32);
    for p in points {
        minx = minx.min(p.x);
        miny = miny.min(p.y);
        maxx = maxx.max(p.x);
        maxy = maxy.max(p.y);
    }

    let mut tree = Quadtree::new((minx - 0.5, miny - 0.5), (maxx + 0.5, maxy + 0.5), 7);
    let h_scene = inputs.cells_y as f32 * 128.0 * inputs.render_scale;
    let tw = trace_window();
    let traced = |x: f32, y: f32| tw.is_some_and(|(cx, cz, r)| (x / inputs.render_scale - cx).abs() <= r && ((h_scene - y) / inputs.render_scale - cz).abs() <= r);
    let lp = |x: f32, y: f32| (x / inputs.render_scale, (h_scene - y) / inputs.render_scale);
    let mut live: CaMap<Vec2Key, Chosen> = CaMap::default();
    let mut blends: CaMap<Vec2Key, f32> = CaMap::default();
    let mut chosen_order: Vec<(f32, f32, f32)> = vec![];

    // A group's objects (of the generator's mask, in the group's order: `group_objects`) weighed at a blend and height into `cum`; the total.
    let object_weights = |objs: &[usize], blend: f32, h: f32, cum: &mut Vec<(f32, usize)>| {
        cum.clear();
        let mut total = 0.0f32;
        for &o in objs {
            let op: &ObjectParams = &objects[o];
            let bp = op.blend.blend_probability(blend, op.probability);
            if bp != 0.0 {
                let hp = op.height.height_probability(h, op.probability);
                if hp != 0.0 {
                    total = (hp + bp) + total;
                    cum.push((total, o));
                }
            }
        }

        total
    };

    let group_list: Vec<(&String, &CaMap<String, usize>)> = groups.iter().collect();

    // The groups' parameters and channels, resolved once (millions of candidates read them).
    let group_views: Vec<GroupView> = group_list.iter().map(|(name, _)| GroupView::new(params, inputs, name)).collect();
    let group_objects: Vec<Vec<usize>> =
        group_list.iter().map(|(_, inner)| inner.iter().map(|(_, &o)| o).filter(|&o| objects[o].kind & mask == objects[o].kind).collect()).collect();

    // The pick's weights, reused from candidate to candidate.
    let scratch = std::cell::RefCell::new((Vec::<(f32, usize)>::new(), Vec::<(f32, usize)>::new()));

    // One candidate from a random state, without side effects: the group, its blend sum, the object and size (if any) and the state after.
    let choose = |p: &Candidate, state: u32| -> (Option<(usize, f32, Option<(usize, f32)>)>, u32) {
        let mut rand = Rand(state);
        let pixel = inputs.pixel(p.px, p.py);
        let h = pixel.height;
        let mut scratch = scratch.borrow_mut();
        let (cum, objects_cum) = &mut *scratch;
        cum.clear();
        let mut total = 0.0f32;
        for (i, view) in group_views.iter().enumerate() {
            let Some(g) = view.params else { continue };
            if pixel.summed(&view.sum_textures) >= 0.05 {
                let v = view.probability_channel.map_or(0.0, |c| pixel.norm[c]);
                let p1 = if v == 0.0 { 0.0 } else { g.channel.blend_probability(v, g.probability) };
                if p1 != 0.0 {
                    let p2 = g.height.height_probability(h, g.probability);
                    if p2 != 0.0 {
                        total = (p2 + p1) + total;
                        cum.push((total, i));
                    }
                }
            }
        }

        if cum.is_empty() {
            return (None, rand.0);
        }

        let Some(gi) = pick(cum, total, &mut rand) else { return (None, rand.0) };
        let blend = pixel.summed(&group_views[gi].probability_textures);
        let total = object_weights(&group_objects[gi], blend, h, objects_cum);
        if objects_cum.is_empty() {
            return (Some((gi, blend, None)), rand.0);
        }

        let Some(o) = pick(objects_cum, total, &mut rand) else { return (Some((gi, blend, None)), rand.0) };
        let size = objects[o].random_size(&mut rand, blend);
        (Some((gi, blend, Some((o, size)))), rand.0)
    };

    probe::run(points, rand.0, inputs, &|p, s| {
        let (r, next) = choose(p, s);
        (r.and_then(|(_, _, o)| o).map(|(o, size)| (objects[o].model.clone(), size)), next)
    });

    let choose_scope = wce_foundation::profile::Scope::new(wce_foundation::profile::slot("veg.choose"));
    for p in points {
        let (result, next) = choose(p, rand.0);
        rand.0 = next;
        let Some((gi, blend, object)) = result else { continue };
        let (gname, _) = group_list[gi];
        let g: &GroupParams = params.groups.get(gname).expect("listed");
        let key = Vec2Key(p.x, p.y);
        blends.insert(key, blend);
        let Some((o, size)) = object else { continue };
        let op = &objects[o];
        let c = Chosen {
            model: op.model.clone(),
            x: p.x,
            y: p.y,
            size,
            min_separation: op.min_separation,
            grouping_probability: g.grouping_probability,
            grouping_radius: g.grouping_radius,
            width: op.width,
            group: op.group.clone(),
            include_in_outfield: op.include_in_outfield,
            kind: op.kind,
            height: 0.0,
            freeform: false,
        };

        if traced(p.x, p.y) {
            eprintln!("stage1 {:?} {} group {} ms {}", lp(p.x, p.y), op.model, op.group, op.min_separation);
        }

        chosen_order.push((op.min_separation, p.x, p.y));
        live.insert(key, c.clone());
        let n = tree.container((p.x - 0.01, p.y - 0.01, p.x + 0.01, p.y + 0.01));
        tree.nodes[n].items.insert(key, c);
    }

    drop(choose_scope);
    let group_scope = wce_foundation::profile::Scope::new(wce_foundation::profile::slot("veg.group"));
    // group_objects: in the order of a std::map keyed (min_sep, x, y).
    chosen_order.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.total_cmp(&b.1)).then(a.2.total_cmp(&b.2)));
    chosen_order.dedup();
    let (mut cum, mut nodes, mut keys): (Vec<(f32, usize)>, Vec<usize>, Vec<Vec2Key>) = (vec![], vec![], vec![]);
    for &(_, lx, ly) in &chosen_order {
        let lkey = Vec2Key(lx, ly);
        let Some(lead) = live.get(&lkey) else { continue };
        if lead.grouping_probability == 0.0 || lead.grouping_radius == 0.0 {
            continue;
        }

        let (gp, gr) = (lead.grouping_probability, lead.grouping_radius);
        // The leader's group among the category's (none: no object is valid for it), resolved once.
        let lgroup = group_list.iter().position(|(name, _)| **name == lead.group);
        if !(rand.unit() <= gp) {
            continue;
        }

        nodes.clear();
        tree.query(0, (lx, ly, gr), &mut nodes);
        if traced(lx, ly) {
            eprintln!("leader {:?} gr {gr} nodes {:?}", lp(lx, ly), nodes.iter().map(|&n| (n, tree.nodes[n].min, tree.nodes[n].max, tree.nodes[n].items.keys().iter().map(|k| lp(k.0, k.1)).collect::<Vec<_>>())).collect::<Vec<_>>());
        }
        for &n in &nodes {
            // The node's keys as they are when its turn comes (the loop changes values, not keys).
            keys.clear();
            keys.extend(tree.nodes[n].items.iter().map(|(k, _)| *k));
            for &okey in &keys {
                if okey == lkey {
                    continue;
                }

                let dx = lx - okey.0;
                let dy = ly - okey.1;
                if !(dy * dy + dx * dx <= gr * gr) {
                    continue;
                }

                let cell = inputs.cell();
                let opx = (okey.0 / cell).floor().max(0.0) as u32;
                let opy = (okey.1 / cell).floor().max(0.0) as u32;
                let Some(gi) = lgroup else { continue };
                let pixel = inputs.pixel(opx, opy);
                if !valid_group(&group_views[gi], &pixel) {
                    continue;
                }

                let Some(&b) = blends.get(&okey) else { continue };
                let hh = pixel.height;
                let total = object_weights(&group_objects[gi], b, hh, &mut cum);
                if cum.is_empty() {
                    continue;
                }

                let Some(o) = pick(&cum, total, &mut rand) else { continue };
                if !live.contains(&okey) || !tree.nodes[n].items.contains(&okey) {
                    continue;
                }

                let op = &objects[o];
                if traced(okey.0, okey.1) {
                    eprintln!("group  {:?} <- leader {:?}: {} ms {}", lp(okey.0, okey.1), lp(lx, ly), op.model, op.min_separation);
                }

                let s1 = op.random_size(&mut rand, b);
                let le = live.get_mut(&okey).expect("checked");
                le.model.clone_from(&op.model);
                le.group.clone_from(&op.group);
                le.min_separation = op.min_separation;
                le.size = s1;
                le.include_in_outfield = op.include_in_outfield;
                let s2 = op.random_size(&mut rand, b);
                let te = tree.nodes[n].items.get_mut(&okey).expect("checked");
                te.model.clone_from(&op.model);
                te.group.clone_from(&op.group);
                te.min_separation = op.min_separation;
                te.size = s2;
                te.include_in_outfield = op.include_in_outfield;
            }
        }
    }

    drop(group_scope);
    wce_foundation::wce_prof!("veg.min_separation");
    // remove_objects_based_on_min_seperation over a copy of the generated map.
    let snapshot: Vec<Vec2Key> = live.keys();
    for ekey in snapshot {
        let Some(e) = live.get(&ekey) else { continue };
        let (ms, ew) = (e.min_separation, e.width);
        if !(0.0 < ms) {
            continue;
        }

        let mut nodes = vec![];
        tree.query(0, (ekey.0, ekey.1, ms), &mut nodes);
        for n in nodes {
            let entries: Vec<(Vec2Key, f32, f32)> = tree.nodes[n].items.iter().map(|(k, v)| (*k, v.min_separation, v.width)).collect();
            let mut i = 0;
            while i < entries.len() {
                let (okey, oms, ow) = entries[i];
                i += 1;
                if okey == ekey {
                    continue;
                }

                let near = if ow <= 0.0 {
                    let d2 = (ekey.1 - okey.1) * (ekey.1 - okey.1) + (ekey.0 - okey.0) * (ekey.0 - okey.0);
                    !(ms * ms < d2)
                } else {
                    let hw = ow * 0.5;
                    circle_box((ekey.0, ekey.1, ms), (okey.0 - hw, okey.1 - hw, okey.0 + hw, okey.1 + hw)) != 2
                };

                let remove = near || {
                    if ew <= 0.0 {
                        let d2 = (okey.1 - ekey.1) * (okey.1 - ekey.1) + (okey.0 - ekey.0) * (okey.0 - ekey.0);
                        d2 <= oms * oms
                    } else {
                        let hw = ew * 0.5;
                        circle_box((okey.0, okey.1, oms), (ekey.0 - hw, ekey.1 - hw, ekey.0 + hw, ekey.1 + hw)) != 2
                    }
                };

                if remove {
                    if traced(okey.0, okey.1) {
                        eprintln!("remove {:?} by {:?} (ms {} vs {}) in live {}", lp(okey.0, okey.1), lp(ekey.0, ekey.1), ms, oms, live.contains(&okey));
                    }

                    live.remove(&okey);
                    tree.nodes[n].items.remove(&okey);
                    // BOB's loop advances twice after an erase: the entry after the removed one is never looked at.
                    i += 1;
                }
            }
        }
    }

    live
}

/// `valid_group_for_position`, for a group of the category (one that isn't is never valid) at a pixel.
fn valid_group(view: &GroupView, pixel: &Pixel) -> bool {
    let Some(g) = view.params else { return false };
    if !(pixel.summed(&view.sum_textures) >= 0.05) {
        return false;
    }

    let v = pixel.summed(&view.probability_textures);
    v != 0.0 && g.channel.blend_probability(v, g.probability) != 0.0
}

/// A tree of a generated list, in the list's frame (`tree_list.bin`).
#[derive(Clone, Debug, PartialEq)]
pub struct ListTree {
    /// `BattleTerrain/vegetation/` + model + `.rigid_model_v2`.
    pub path: String,
    pub x: f32,
    pub y: f32,
    pub z: f32,
    pub scale_byte: u8,

    /// A static tree from an `MSM_FREEFORM` instance.
    pub freeform: bool,
}

/// BOB_Vegetation's output: the trees (type 1) of a generator in the list frame, in the list's order (x and z in metres, z from the far edge), on the terrain;
/// trees whose model file doesn't exist are left out (BOB logs them).
pub fn to_tree_list(inputs: &TileInputs, chosen: &[Chosen], model_exists: &dyn Fn(&str) -> bool) -> Vec<ListTree> {
    let rs = inputs.render_scale;
    let w_m = inputs.cells_x as f32 * 128.0 * rs;
    let h_m = inputs.cells_y as f32 * 128.0 * rs;
    let mut out = vec![];
    for c in chosen.iter().filter(|c| c.kind == super::params::TREE) {
        let x = c.x * (1.0 / rs);
        let z = (h_m - c.y) * (1.0 / rs);
        if !(0.0 < x && x <= w_m && 0.0 < z && z < h_m) {
            continue;
        }

        // A stored height (a static tree's) is used as it is; otherwise the terrain's, if the pixel is on the height map.
        let y = if c.height != 0.0 {
            c.height
        } else {
            let Some((col, row)) = inputs.list_pixel(c.x, c.y) else { continue };
            inputs.list_height(col, row)
        };

        let path = format!("BattleTerrain/vegetation/{}.rigid_model_v2", c.model);
        if !model_exists(&path) {
            continue;
        }

        // The byte is the low 8 bits of ceil(size * 127): sizes over 2 wrap.
        out.push(ListTree { path, x, y, z, scale_byte: ((c.size * 127.0).ceil() as i64 & 0xff) as u8, freeform: c.freeform });
    }

    // Written through a vector sorted by path (case and all) with MSVC's std::sort, which is not stable: it decides the order of a model's
    // trees in the list, and with it their yaws in the game.
    super::msvc_sort::sort(&mut out, &|a: &ListTree, b: &ListTree| a.path < b.path);
    out
}

/// A `PROP` of a category's `_procedural_bmd_data` (a prop object, type 2; decals are props of type 1).
#[derive(Clone, Debug, PartialEq)]
pub struct ListProp {
    /// `RigidModels/` + model + `.rigid_model_v2` (decals: `RigidModels/Decals/` + model + `.rigid_model_v2`).
    pub key: String,

    /// Row-major, translation in the last row (m30, m31, m32).
    pub transform: [f32; 16],

    /// 0 for a prop, 1 for a decal.
    pub prop_type: u32,
    pub allow_in_outfield: bool,
    pub decal_parallax: f32,
    pub decal_scale: [f32; 3],
    pub decal_tiled: bool,
}

/// BOB_Vegetation's output for the props (type 2) of a generator, in the generated map's order, with the trees' bounds and heights: scaled by
/// the size, unrotated, and the marker's defaults (not clamped, not fauna, snow and destruction on). The model file isn't checked. BOB leaves
/// a prop's decal parameters uninitialised (stack garbage in its files); they are written as 0.
pub fn to_prop_list(inputs: &TileInputs, chosen: &[Chosen]) -> Vec<ListProp> {
    let rs = inputs.render_scale;
    let w_m = inputs.cells_x as f32 * 128.0 * rs;
    let h_m = inputs.cells_y as f32 * 128.0 * rs;
    let mut out = vec![];
    for c in chosen.iter().filter(|c| c.kind == super::params::PROP) {
        let x = c.x * (1.0 / rs);
        let z = (h_m - c.y) * (1.0 / rs);
        if !(0.0 < x && x <= w_m && 0.0 < z && z < h_m) {
            continue;
        }

        let Some((col, row)) = inputs.list_pixel(c.x, c.y) else { continue };
        let y = inputs.list_height(col, row);
        let s = c.size;
        out.push(ListProp {
            key: format!("RigidModels/{}.rigid_model_v2", c.model),
            transform: [s, 0.0, 0.0, 0.0, 0.0, s, 0.0, 0.0, 0.0, 0.0, s, 0.0, x, y, z, 1.0],
            prop_type: 0,
            allow_in_outfield: c.include_in_outfield,
            decal_parallax: 0.0,
            decal_scale: [0.0; 3],
            decal_tiled: true,
        });
    }

    out
}

/// `FUN_10013ba0` / `FUN_10013cc0`: a 3x3 Sobel sum around (col, row) over 9, the coordinates clamped to `w` x `h` and the row stride `w`
/// (BOB passes the tile's extent in scene units, not the field's size). `transpose` gives the y gradient.
fn sobel(field: &[f32], w: f32, h: f32, col: i32, row: i32, transpose: bool) -> f32 {
    const K: [f32; 9] = [1.0, 0.0, -1.0, 2.0, 0.0, -2.0, 1.0, 0.0, -1.0];
    let (max_c, max_r, stride) = ((w - 1.0) as i32, (h - 1.0) as i32, w as i32);
    let mut sum = 0.0f32;
    for o in 0..3 {
        for i in 0..3 {
            // Plain: kernel row o, column i at (col - 1 + i, row - 1 + o). Transposed: kernel [i][o] at the same pixel.
            let (c, r, k) = if transpose { (col - 1 + i, row - 1 + o, K[(i * 3 + o) as usize]) } else { (col - 1 + i, row - 1 + o, K[(o * 3 + i) as usize]) };
            let c = c.clamp(0, max_c.max(0));
            let r = r.clamp(0, max_r.max(0));
            let v = field.get((r * stride + c) as usize).copied().unwrap_or(0.0);
            sum += v * k;
        }
    }

    (f64::from(sum) / 9.0) as f32
}

/// BOB_Vegetation's output for the decals (type 4) of a generator, as props of type 1 in the generated map's order: rotated from up to the
/// normal of the height field's Sobel gradients at (x / cell, y / cell), with the normal taken as (gx, gy, 1) (so a flat decal stands at
/// 90 degrees about x), scaled by the size; decal parallax 1 and the size as the decal scale, not tiled.
pub fn to_decal_list(inputs: &TileInputs, chosen: &[Chosen]) -> Vec<ListProp> {
    let rs = inputs.render_scale;
    let w_m = inputs.cells_x as f32 * 128.0 * rs;
    let h_m = inputs.cells_y as f32 * 128.0 * rs;
    let inv_cell = 1.0 / inputs.cell();
    let mut out = vec![];
    for c in chosen.iter().filter(|c| c.kind == super::params::DECAL) {
        let x = c.x * (1.0 / rs);
        let z = (h_m - c.y) * (1.0 / rs);
        if !(0.0 < x && x <= w_m && 0.0 < z && z < h_m) {
            continue;
        }

        let Some((col, row)) = inputs.list_pixel(c.x, c.y) else { continue };
        let y = inputs.list_height(col, row);

        let (pc, pr) = ((c.x * inv_cell) as i32, (c.y * inv_cell) as i32);
        let a = sobel(&inputs.heights, w_m, h_m, pc, pr, false);
        let b = sobel(&inputs.heights, w_m, h_m, pc, pr, true);
        let len = (b * b + a * a + 1.0).sqrt();
        let inv = 1.0 / len;
        let (na, nb) = (a * inv, b * inv);

        // The quaternion from up (0, 1, 0) to the normal (identity when it is vertical enough).
        let (mut qx, mut qy, mut qz, mut qw) = (0.0f32, 0.0f32, 0.0f32, 1.0f32);
        if nb < f32::from_bits(0x3f7f_f972) {
            let angle = f64::from(nb).acos() as f32;
            let (ax, az) = (inv, -na);
            let l2 = (ax * ax + az * az).sqrt();
            let (ux, uz) = if l2 > 0.0 { let r = 1.0 / l2; (r * ax, r * az) } else { (1.0, 0.0) };
            let half = f64::from(angle * 0.5);
            let (s, co) = (half.sin() as f32, half.cos() as f32);
            qx = ux * s;
            qy = s * 0.0;
            qz = uz * s;
            qw = co;
        }

        let xx2 = qx * 2.0 * qx;
        let yy2 = qy * 2.0 * qy;
        let xy2 = qx * 2.0 * qy;
        let xz2 = qx * 2.0 * qz;
        let zz2 = qz * 2.0 * qz;
        let wz2 = qw * 2.0 * qz;
        let yz2 = qz * (qy * 2.0);
        let wy2 = qw * 2.0 * qy;
        let wx2 = qw * 2.0 * qx;
        let r = [
            [(1.0 - yy2) - zz2, wz2 + xy2, xz2 - wy2],
            [xy2 - wz2, (1.0 - xx2) - zz2, wx2 + yz2],
            [wy2 + xz2, yz2 - wx2, (1.0 - xx2) - yy2],
        ];
        let s = c.size;
        out.push(ListProp {
            key: format!("RigidModels/Decals/{}.rigid_model_v2", c.model),
            transform: decal_matrix(&r, s, [x, y, z]),
            prop_type: 1,
            allow_in_outfield: c.include_in_outfield,
            decal_parallax: 1.0,
            decal_scale: [s, s, s],
            decal_tiled: false,
        });
    }

    out
}

/// A decal's transform as BOB composes it (FUN_100477a0, the decal case): the size's scale matrix times the rotation, then times the
/// translation, as full 4 x 4 products whose zero terms are summed in, in BOB's order; the sums decide the sign of the zeros. The last
/// column is written as 0, 0, 0, 1.
fn decal_matrix(r: &[[f32; 3]; 3], s: f32, t: [f32; 3]) -> [f32; 16] {
    let z = |v: f32| v * 0.0;
    let a = [
        [(r[0][0] * s + z(r[1][0])) + z(r[2][0]), (r[0][1] * s + z(r[1][1])) + z(r[2][1]), (r[0][2] * s + z(r[1][2])) + z(r[2][2])],
        [(r[1][0] * s + z(r[0][0])) + z(r[2][0]), (r[1][1] * s + z(r[0][1])) + z(r[2][1]), (r[1][2] * s + z(r[0][2])) + z(r[2][2])],
        [r[2][0] * s + (z(r[0][0]) + z(r[1][0])), (z(r[0][1]) + z(r[1][1])) + r[2][1] * s, r[2][2] * s + (z(r[0][2]) + z(r[1][2]))],
        [(z(r[0][0]) + z(r[1][0])) + z(r[2][0]), (z(r[0][1]) + z(r[1][1])) + z(r[2][1]), (z(r[0][2]) + z(r[1][2])) + z(r[2][2])],
    ];
    let mut m = [0.0f32; 16];
    for i in 0..3 {
        let [a0, a1, a2] = a[i];
        m[i * 4] = ((a0 + z(a1)) + z(a2)) + z(t[0]);
        m[i * 4 + 1] = ((a1 + z(a0)) + z(a2)) + z(t[1]);
        m[i * 4 + 2] = ((a2 + z(a0)) + z(a1)) + z(t[2]);
    }

    let [a0, a1, a2] = a[3];
    m[12] = ((a0 + z(a1)) + z(a2)) + t[0];
    m[13] = ((a1 + z(a0)) + z(a2)) + t[1];
    m[14] = (a2 + (z(a0) + z(a1))) + t[2];
    m[15] = 1.0;
    m
}

/// A `PARTICLE_EMITTER` of a category's `_procedural_bmd_data` (a VFX object, type 8).
#[derive(Clone, Debug, PartialEq)]
pub struct ListVfx {
    /// `vfx/` + model + `.rigid_model_v2`.
    pub key: String,

    /// The marker's `instance_name`: the model.
    pub instance_name: String,
    pub x: f32,
    pub y: f32,
    pub z: f32,

    /// The marker's `scale`: the object's size.
    pub scale: f32,
    pub use_in_outfield: bool,
}

/// BOB_Vegetation's output for the VFX objects (type 8) of a generator, in the generated map's order, with the trees' bounds and heights.
/// The marker has an identity rotation at (x, y, z), emission rate 1, autoplay, and snow and destruction inside and outside on. Unlike the
/// trees, the model file isn't checked.
pub fn to_vfx_list(inputs: &TileInputs, chosen: &[Chosen]) -> Vec<ListVfx> {
    let rs = inputs.render_scale;
    let w_m = inputs.cells_x as f32 * 128.0 * rs;
    let h_m = inputs.cells_y as f32 * 128.0 * rs;
    let mut out = vec![];
    for c in chosen.iter().filter(|c| c.kind == super::params::VFX) {
        let x = c.x * (1.0 / rs);
        let z = (h_m - c.y) * (1.0 / rs);
        if !(0.0 < x && x <= w_m && 0.0 < z && z < h_m) {
            continue;
        }

        let Some((col, row)) = inputs.list_pixel(c.x, c.y) else { continue };
        out.push(ListVfx {
            key: format!("vfx/{}.rigid_model_v2", c.model),
            instance_name: c.model.clone(),
            x,
            y: inputs.list_height(col, row),
            z,
            scale: c.size,
            use_in_outfield: c.include_in_outfield,
        });
    }

    out
}

/// Debugging aid: with an oracle set (the tree list BOB wrote, by pixel), reports the candidates where the random offset that reproduces
/// BOB's pick changes. Off unless the checker sets the oracle.
pub mod probe {
    use std::cell::RefCell;
    use std::collections::HashMap;

    use super::{Candidate, Rand, TileInputs};

    thread_local! {
        pub static ORACLE: RefCell<Option<HashMap<(u32, u32), (String, u8)>>> = const { RefCell::new(None) };
        pub static REPORT: RefCell<Vec<String>> = const { RefCell::new(vec![]) };
    }

    pub fn run(points: &[Candidate], start: u32, _inputs: &TileInputs, choose: &dyn Fn(&Candidate, u32) -> (Option<(String, f32)>, u32)) {
        ORACLE.with_borrow(|oracle| {
            let Some(oracle) = oracle else { return };
            let mut state = start;
            let mut offset = 0usize;
            let mut found_ok = 0;
            let mut report = vec![];
            for (i, p) in points.iter().enumerate() {
                if let Some((model, byte)) = oracle.get(&(p.px, p.py)) {
                    let matches = |s: u32| {
                        let (r, _) = choose(p, s);
                        r.is_some_and(|(m, size)| m.replace('\\', "/").to_lowercase() == *model && ((size * 127.0).ceil() as i64 & 0xff) as u8 == *byte)
                    };
                    let at = |k: usize| {
                        let mut s = state;
                        for _ in 0..k {
                            s = Rand::step(s);
                        }
                        s
                    };
                    if report.len() < 20 {
                        let (mine, _) = choose(p, at(offset));
                        report.push(format!("  oracle ({}, {}) theirs {model} {byte} mine {:?}", p.px, p.py, mine.map(|(m, s)| (m, ((s * 127.0).ceil() as i64 & 0xff) as u8))));
                    }

                    if matches(at(offset)) {
                        found_ok += 1;
                    } else if let Some(k) = (0..40).find(|&k| matches(at(k))) {
                        report.push(format!("candidate {i} ({}, {}): offset {offset} -> {k} after {found_ok} matches", p.px, p.py));
                        offset = k;
                        if report.len() > 12 {
                            break;
                        }
                    }
                }

                state = choose(p, state).1;
            }

            REPORT.with_borrow_mut(|r| *r = report);
        });
    }
}
