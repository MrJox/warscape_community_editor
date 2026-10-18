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

    /// `FUN_1028bdc0`.
    pub(super) fn height(&self, px: u32, py: u32) -> f32 {
        if !self.valid(px, py) {
            return 0.0;
        }

        self.map_index(px, py).map_or(0.0, |i| self.heights[i])
    }

    /// `FUN_1028bb40`: a channel, divided by its texture's sum when that is over 1.
    pub(super) fn channel(&self, px: u32, py: u32, ch: usize) -> f32 {
        if !self.valid(px, py) {
            return 0.0;
        }

        let Some(i) = self.map_index(px, py) else { return 0.0 };
        let pix = &self.blend[i * 16..i * 16 + 16];
        let t = ch >> 2;
        let mut sum = 0.0f32;
        for c in 0..4 {
            sum = if c == 0 { pix[t * 4] + sum } else { sum + pix[t * 4 + c] };
        }

        let v = pix[ch];
        if 1.0 < sum { (1.0 / sum) * v } else { v }
    }

    fn textures(&self, name: &str) -> Vec<usize> {
        self.texture_groups.iter().enumerate().filter(|(_, g)| *g == name).map(|(i, _)| i).collect()
    }

    /// `summed_channel_blendmap`: the positive channels of every texture of the group.
    fn summed(&self, px: u32, py: u32, name: &str) -> f32 {
        let mut acc = 0.0f32;
        for t in self.textures(name) {
            for c in 0..4 {
                let v = self.channel(px, py, t * 4 + c);
                if v > 0.0 {
                    acc = v + acc;
                }
            }
        }

        acc
    }

    /// `probability_blendmap_value_for_channel_at_pixel`: one channel of the group's textures; an unknown group reads the alphabetically
    /// first one.
    fn probability_value(&self, px: u32, py: u32, name: &str, number: u32) -> f32 {
        let mut textures = self.textures(name);
        if textures.is_empty() {
            let mut names: Vec<&String> = self.texture_groups.iter().collect();
            names.sort();
            let Some(first) = names.first() else { return 0.0 };
            textures = self.textures(first);
        }

        let Some(&t) = textures.get((number >> 2) as usize) else { return 0.0 };
        self.channel(px, py, (number & 3) as usize + t * 4)
    }

    fn excluded(&self, x: f32, y: f32) -> bool {
        self.exclusions.iter().any(|poly| point_in(poly, x, y))
    }
}

pub fn point_in(poly: &[(f32, f32)], x: f32, y: f32) -> bool {
    let mut c = false;
    let n = poly.len();
    for i in 0..n {
        let (xi, yi) = poly[i];
        let (xj, yj) = poly[(i + n - 1) % n];
        if (yi > y) != (yj > y) && x < (xj - xi) * (y - yi) / (yj - yi) + xi {
            c = !c;
        }
    }

    c
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
    let mut keys = HashSet::new();
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
pub fn generate(params: &GenerationParams, inputs: &TileInputs, category: usize, mask: u32, seed: u32, density: f32) -> Vec<Chosen> {
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
    let points = candidates(inputs, density, &mut rand);
    if points.is_empty() {
        return vec![];
    }

    let (mut minx, mut miny, mut maxx, mut maxy) = (f32::MAX, f32::MAX, 0.0f32, 0.0f32);
    for p in &points {
        minx = minx.min(p.x);
        miny = miny.min(p.y);
        maxx = maxx.max(p.x);
        maxy = maxy.max(p.y);
    }

    let mut tree = Quadtree::new((minx - 0.5, miny - 0.5), (maxx + 0.5, maxy + 0.5), 7);
    let mut live: CaMap<Vec2Key, Chosen> = CaMap::default();
    let mut blends: CaMap<Vec2Key, f32> = CaMap::default();
    let mut chosen_order: Vec<(f32, f32, f32)> = vec![];

    let object_weights = |inner: &CaMap<String, usize>, blend: f32, h: f32| {
        let mut cum: Vec<(f32, usize)> = vec![];
        let mut total = 0.0f32;
        for (_, &o) in inner.iter() {
            let op: &ObjectParams = &objects[o];
            if op.kind & mask != op.kind {
                continue;
            }

            let bp = op.blend.blend_probability(blend, op.probability);
            if bp != 0.0 {
                let hp = op.height.height_probability(h, op.probability);
                if hp != 0.0 {
                    total = (hp + bp) + total;
                    cum.push((total, o));
                }
            }
        }

        (cum, total)
    };

    let group_list: Vec<(&String, &CaMap<String, usize>)> = groups.iter().collect();

    // One candidate from a random state, without side effects: the group, its blend sum, the object and size (if any) and the state after.
    let choose = |p: &Candidate, state: u32| -> (Option<(usize, f32, Option<(usize, f32)>)>, u32) {
        let mut rand = Rand(state);
        let h = inputs.height(p.px, p.py);
        let mut cum: Vec<(f32, usize)> = vec![];
        let mut total = 0.0f32;
        for (i, (name, _)) in group_list.iter().enumerate() {
            let Some(g) = params.groups.get(name) else { continue };
            if inputs.summed(p.px, p.py, &g.channel_group) >= 0.05 {
                let v = inputs.probability_value(p.px, p.py, &g.channel_group_for_probability, g.channel_number);
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

        let Some(gi) = pick(&cum, total, &mut rand) else { return (None, rand.0) };
        let g: &GroupParams = params.groups.get(group_list[gi].0).expect("listed");
        let blend = inputs.summed(p.px, p.py, &g.channel_group_for_probability);
        let (cum, total) = object_weights(group_list[gi].1, blend, h);
        if cum.is_empty() {
            return (Some((gi, blend, None)), rand.0);
        }

        let Some(o) = pick(&cum, total, &mut rand) else { return (Some((gi, blend, None)), rand.0) };
        let size = objects[o].random_size(&mut rand, blend);
        (Some((gi, blend, Some((o, size)))), rand.0)
    };

    probe::run(&points, rand.0, inputs, &|p, s| {
        let (r, next) = choose(p, s);
        (r.and_then(|(_, _, o)| o).map(|(o, size)| (objects[o].model.clone(), size)), next)
    });

    for p in &points {
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
        };

        chosen_order.push((op.min_separation, p.x, p.y));
        live.insert(key, c.clone());
        let n = tree.container((p.x - 0.01, p.y - 0.01, p.x + 0.01, p.y + 0.01));
        tree.nodes[n].items.insert(key, c);
    }

    // group_objects: in the order of a std::map keyed (min_sep, x, y).
    chosen_order.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.total_cmp(&b.1)).then(a.2.total_cmp(&b.2)));
    chosen_order.dedup();
    for &(_, lx, ly) in &chosen_order {
        let lkey = Vec2Key(lx, ly);
        let Some(lead) = live.get(&lkey) else { continue };
        if lead.grouping_probability == 0.0 || lead.grouping_radius == 0.0 {
            continue;
        }

        let (gp, gr, lgroup) = (lead.grouping_probability, lead.grouping_radius, lead.group.clone());
        if !(rand.unit() <= gp) {
            continue;
        }

        let mut nodes = vec![];
        tree.query(0, (lx, ly, gr), &mut nodes);
        for n in nodes {
            for okey in tree.nodes[n].items.keys() {
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
                if !valid_group(params, inputs, &groups, opx, opy, &lgroup) {
                    continue;
                }

                let Some(&b) = blends.get(&okey) else { continue };
                let hh = inputs.height(opx, opy);
                let Some(inner) = groups.get(&lgroup) else { continue };
                let (cum, total) = object_weights(inner, b, hh);
                if cum.is_empty() {
                    continue;
                }

                let Some(o) = pick(&cum, total, &mut rand) else { continue };
                if !live.contains(&okey) || !tree.nodes[n].items.contains(&okey) {
                    continue;
                }

                let op = &objects[o];
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
                    live.remove(&okey);
                    tree.nodes[n].items.remove(&okey);
                    // BOB's loop advances twice after an erase: the entry after the removed one is never looked at.
                    i += 1;
                }
            }
        }
    }

    live.iter().map(|(_, c)| c.clone()).collect()
}

/// `valid_group_for_position`.
fn valid_group(params: &GenerationParams, inputs: &TileInputs, groups: &CaMap<String, CaMap<String, usize>>, px: u32, py: u32, group: &str) -> bool {
    if !groups.contains(&group.to_owned()) {
        return false;
    }

    let Some(g) = params.groups.get(&group.to_owned()) else { return false };
    if !(inputs.summed(px, py, &g.channel_group) >= 0.05) {
        return false;
    }

    let v = inputs.summed(px, py, &g.channel_group_for_probability);
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
}

/// BOB_Vegetation's output: the trees (type 1) of a generator in the list frame, in the list's order (x and z in metres, z from the far edge), on the terrain;
/// trees whose model file doesn't exist are left out (BOB logs them).
pub fn to_tree_list(inputs: &TileInputs, chosen: &[Chosen], model_exists: &dyn Fn(&str) -> bool) -> Vec<ListTree> {
    let rs = inputs.render_scale;
    let w_m = inputs.cells_x as f32 * 128.0 * rs;
    let h_m = inputs.cells_y as f32 * 128.0 * rs;
    let inv_cell = 1.0 / inputs.cell();
    let tri = inputs.tri_density as f32;
    let mut out = vec![];
    for c in chosen.iter().filter(|c| c.kind == super::params::TREE) {
        let x = c.x * (1.0 / rs);
        let z = (h_m - c.y) * (1.0 / rs);
        if !(0.0 < x && x <= w_m && 0.0 < z && z < h_m) {
            continue;
        }

        let col = (tri + c.x * inv_cell).round_ties_even() as i64;
        let row = (tri + c.y * inv_cell).round_ties_even() as i64;
        if col < 0 || row < 0 || col as usize >= inputs.map_w || row as usize >= inputs.map_h {
            continue;
        }

        let path = format!("BattleTerrain/vegetation/{}.rigid_model_v2", c.model);
        if !model_exists(&path) {
            continue;
        }

        let y = inputs.heights[row as usize * inputs.map_w + col as usize];

        // The byte is the low 8 bits of ceil(size * 127): sizes over 2 wrap.
        out.push(ListTree { path, x, y, z, scale_byte: ((c.size * 127.0).ceil() as i64 & 0xff) as u8 });
    }

    // Written through a vector sorted by path (case and all) with MSVC's std::sort, which is not stable: it decides the order of a model's
    // trees in the list, and with it their yaws in the game.
    super::msvc_sort::sort(&mut out, &|a: &ListTree, b: &ListTree| a.path < b.path);
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
