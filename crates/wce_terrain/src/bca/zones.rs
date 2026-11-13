//! A BCA's deployment zones (Terry's `BattleCatchmentAreaGenerator`, Thrones of Britannia's TWeak_TerrainMetadataEditor; docs/re/terry.md,
//! "Deployment zones"), for one configuration:
//!
//! 1. the tile instances of the 8 x 8 cells around the centre (-4 .. 3, fn 0x1005a860), each one's region (fn 0x1005b250): its
//!    variation's deployment mask's region for the configuration, else for -1 ([`DeploymentMask`]); without one a tile in the
//!    deployment's `restriction` list gives none, any other its unmasked subtiles; in 1/128 of a cell, turned with the instance (its flip
//!    ignored), relative to the centre cell's corner;
//! 2. their union, cut to the battle rectangle (fn 0x10057700: -512 .. 511 inset by `1024 * 64 / (unit * 128 * 8) + 1`, 33 for a unit
//!    of 2);
//! 3. the Zonifier (fn 0x1005da20): the region's boundary as directed unit edges (each rectangle of the region's y-x banded form: its
//!    top left to right, bottom right to left, right side down, left side up, fn 0x1005c990; an edge meeting its reverse cancels both,
//!    fn 0x1005c450) in a map ordered by start point; polygons traced from the first edge left (fn 0x1005da20), each edge's start
//!    recorded; a polygon then becomes the hole of the first other polygon met stepping right from its rightmost vertex whose area has
//!    the other sign;
//! 4. each polygon scaled to cells (fn 0x1005bd40) and simplified (fn 0x1005bf10: Douglas-Peucker from its least to its greatest
//!    point and back, `deployment_coarseness` the distance);
//! 5. fewer than two: "Less than two zones created"; more: "More than two zones created - just kept largest two" (the largest by
//!    area, a stable sort); with `split` the two cut along their centroids' axis (fn 0x1005d390), `deployment_split_width` apart.

use std::collections::{BTreeMap, HashMap, HashSet};

use wce_tile::battle_map::fill::{Placement, rotate_in_tile_space};
use wce_tile::battle_map::tile_db::TileDatabase;

use super::{DeploymentMask, DeploymentSpec, Spec, in_list};

/// A region's resolution: units a cell.
pub const UNITS: i32 = 128;

/// A polygon (cells, relative to the BCA's centre cell's corner, y south) with its holes (and theirs).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Polygon {
    pub points: Vec<(f64, f64)>,
    pub children: Vec<Polygon>,
}

impl Polygon {
    /// Its signed area (positive: clockwise with y south, an outline; a hole's negative) with its children's, and the area-weighted
    /// centroid sums (fn 0x1005c1a0: the area summed in f32).
    pub fn moments(&self) -> (f32, f64, f64) {
        let (mut a, mut mx, mut my) = (0f32, 0f64, 0f64);
        if let Some(&last) = self.points.last() {
            let mut q = last;
            for &p in &self.points {
                let c = (p.1 * q.0 - q.1 * p.0) as f32;
                a += c;
                mx += (p.0 + q.0) * f64::from(c);
                my += (p.1 + q.1) * f64::from(c);
                q = p;
            }
        }

        a *= 0.5;
        mx *= 1.0 / 6.0;
        my *= 1.0 / 6.0;
        for c in &self.children {
            let (ca, cx, cy) = c.moments();
            a += ca;
            mx += cx;
            my += cy;
        }

        (a, mx, my)
    }

    pub fn area(&self) -> f32 {
        self.moments().0
    }

    /// Its centroid, as f32 (`QTU::to_ulib_vector`).
    fn centroid(&self) -> (f32, f32) {
        let (a, mx, my) = self.moments();
        let k = 1.0 / f64::from(a);
        ((k * mx) as f32, (my * k) as f32)
    }

    fn scale(&mut self, s: f64) {
        for p in &mut self.points {
            *p = (p.0 * s, p.1 * s);
        }

        for c in &mut self.children {
            c.scale(s);
        }
    }

    /// Douglas-Peucker (fn 0x1005bf10), its children too.
    fn simplify(&mut self, coarseness: f32) {
        let n = self.points.len();
        if n > 0 {
            let (lo, hi) = extremes(&self.points);
            let eps = coarseness * coarseness;
            let mut keep = HashSet::new();
            douglas_peucker(&self.points, hi, lo, eps, &mut keep);
            douglas_peucker(&self.points, lo, hi, eps, &mut keep);
            self.points = (0..n).filter(|i| keep.contains(i)).map(|i| self.points[i]).collect();
        }

        for c in &mut self.children {
            c.simplify(coarseness);
        }
    }
}

/// The indices of a polygon's least and greatest points, x then y (fn 0x10057cf0).
fn extremes(p: &[(f64, f64)]) -> (usize, usize) {
    let (mut lo, mut hi) = (0, 0);
    for (i, &(x, y)) in p.iter().enumerate().skip(1) {
        if x < p[lo].0 || (x == p[lo].0 && y < p[lo].1) {
            lo = i;
        } else if p[hi].0 < x || (p[hi].0 == x && p[hi].1 < y) {
            hi = i;
        }
    }

    (lo, hi)
}

/// The points kept between `a` and `b` (round the polygon from `a`), fn 0x1005b480: the farthest from the line a-b (squared, f32) if
/// farther than `eps`, recursively.
fn douglas_peucker(p: &[(f64, f64)], a: usize, b: usize, eps: f32, keep: &mut HashSet<usize>) {
    let n = p.len();
    let (mut best, mut at) = (-1f32, None);
    let mut i = (a + 1) % n;
    while i != b {
        let (dx, dy) = (p[b].0 - p[a].0, p[b].1 - p[a].1);
        let (vx, vy) = (p[i].0 - p[a].0, p[i].1 - p[a].1);
        let len = (dx * dx + dy * dy) as f32;
        let mut d = (vx * vx + vy * vy) as f32;
        if len != 0.0 {
            let dot = (vx * dx + vy * dy) as f32;
            d -= dot * dot / len;
        }

        if best < d {
            best = d;
            at = Some(i);
        }

        i = (i + 1) % n;
    }

    match at {
        Some(m) if best > eps => {
            douglas_peucker(p, a, m, eps, keep);
            douglas_peucker(p, m, b, eps, keep);
        }
        _ => {
            keep.insert(a);
            keep.insert(b);
        }
    }
}

/// The battle rectangle's inset (fn 0x10057700) for a tile database's unit scale.
pub fn battle_inset(unit: f32) -> i32 {
    (1024.0 * 64.0 / (unit * 128.0 * 8.0) + 1.0) as i32
}

/// A region of the 8 x 8 cells around a centre, at `UNITS` a cell: (x, y) from -512; a bit a unit, row by row.
struct Region {
    bits: Vec<u64>,

    /// The rows filled (y0, y1 exclusive, from 0).
    rows: (i32, i32),
}

const SIDE: i32 = 8 * UNITS;
const WORDS: usize = (SIDE / 64) as usize;

/// A band of the y-x banded form: rows y0 ..= y1, its spans (x0, x1 inclusive), from -512.
type Band = (i32, i32, Vec<(i32, i32)>);

impl Region {
    fn new() -> Self {
        Self { bits: vec![0; WORDS * SIDE as usize], rows: (SIDE, 0) }
    }

    fn fill(&mut self, x0: i32, y0: i32, x1: i32, y1: i32, clip: (i32, i32)) {
        let (lo, hi) = (clip.0 + SIDE / 2, clip.1 + SIDE / 2);
        let (x0, x1) = ((x0 + SIDE / 2).max(lo), (x1 + SIDE / 2).min(hi + 1));
        let (y0, y1) = ((y0 + SIDE / 2).max(lo), (y1 + SIDE / 2).min(hi + 1));
        if x0 >= x1 || y0 >= y1 {
            return;
        }

        self.rows = (self.rows.0.min(y0), self.rows.1.max(y1));
        for y in y0..y1 {
            let row = &mut self.bits[y as usize * WORDS..(y as usize + 1) * WORDS];
            let mut x = x0;
            while x < x1 {
                let (w, b) = ((x / 64) as usize, x % 64);
                let n = (64 - b).min(x1 - x);
                let m = if n == 64 { u64::MAX } else { ((1u64 << n) - 1) << b };
                row[w] |= m;
                x += n;
            }
        }
    }

    #[cfg(test)]
    fn clear(&mut self, x: i32, y: i32) {
        let (x, y) = (x + SIDE / 2, y + SIDE / 2);
        self.bits[y as usize * WORDS + (x / 64) as usize] &= !(1u64 << (x % 64));
    }

    /// A row's maximal spans (from 0, inclusive).
    fn spans(&self, y: i32) -> Vec<(i32, i32)> {
        let row = &self.bits[y as usize * WORDS..(y as usize + 1) * WORDS];
        let mut out = vec![];
        let mut x = 0;
        while x < SIDE {
            let (w, b) = ((x / 64) as usize, x % 64);
            let v = row[w] >> b;
            if v == 0 {
                x = (w as i32 + 1) * 64;
                continue;
            }

            let tz = v.trailing_zeros() as i32;
            if tz > 0 {
                x += tz;
                continue;
            }

            let ones = (!v).trailing_zeros() as i32;
            let run = ones.min(64 - b);
            match out.last_mut() {
                Some((_, e)) if *e == x - 1 => *e = x + run - 1,
                _ => out.push((x, x + run - 1)),
            }

            x += run;
        }

        out
    }

    /// Its y-x banded form (Qt's QRegion): rows of maximal spans, rows with the same spans merged.
    fn bands(&self) -> Vec<Band> {
        let o = SIDE / 2;
        let mut out: Vec<Band> = vec![];
        let mut last: Option<(i32, Vec<(i32, i32)>)> = None;
        for y in self.rows.0..self.rows.1 {
            let s = self.spans(y);
            match out.last_mut() {
                Some(b) if !s.is_empty() && last.as_ref().is_some_and(|(ly, ls)| *ly == y - 1 && *ls == s) => b.1 = y - o,
                _ if !s.is_empty() => out.push((y - o, y - o, s.iter().map(|&(a, b)| (a - o, b - o)).collect())),
                _ => {}
            }

            last = Some((y, s));
        }

        out
    }

    /// Its rectangles (x0, y0, x1, y1 inclusive), band by band, left to right.
    #[cfg(test)]
    fn rects(&self) -> Vec<(i32, i32, i32, i32)> {
        self.bands().iter().flat_map(|(y0, y1, s)| s.iter().map(|&(a, b)| (a, *y0, b, *y1)).collect::<Vec<_>>()).collect()
    }
}

/// Directed unit edges by start point, each start's ends in the order Terry inserts them (std::multimap).
type Edges = BTreeMap<(i32, i32), Vec<(i32, i32)>>;

/// The parts of the spans `a` no span of `b` covers (both sorted, inclusive).
fn uncovered(a: &[(i32, i32)], b: &[(i32, i32)]) -> Vec<(i32, i32)> {
    let mut out = vec![];
    for &(s, e) in a {
        let mut x = s;
        for &(bs, be) in b {
            if be < x || bs > e {
                continue;
            }

            if bs > x {
                out.push((x, bs - 1));
            }

            x = x.max(be + 1);
        }

        if x <= e {
            out.push((x, e));
        }
    }

    out
}

/// The region's boundary as fn 0x1005c990 leaves it: every rectangle's sides as directed unit edges (its top left to right, bottom right
/// to left, then its right side down and left side up, x by x and y by y), an edge meeting its reverse cancelling both (fn 0x1005c450).
/// Built from the bands directly: sides never cancel, a band's top only against the band just above's bottom.
fn edges(region: &Region) -> Edges {
    let bands = region.bands();
    let mut timed: BTreeMap<(i32, i32), Vec<((usize, i64), (i32, i32))>> = BTreeMap::new();
    let mut put = |a: (i32, i32), b: (i32, i32), t: (usize, i64)| timed.entry(a).or_default().push((t, b));
    let mut rect = 0usize;
    let mut first: Vec<usize> = vec![];
    for (y0, y1, s) in &bands {
        first.push(rect);
        for &(x0, x1) in s {
            let w = i64::from(x1 - x0 + 1);
            for y in *y0..=*y1 {
                let j = i64::from(y - y0);
                put((x1 + 1, y), (x1 + 1, y + 1), (rect, 2 * w + 2 * j));
                put((x0, y + 1), (x0, y), (rect, 2 * w + 2 * j + 1));
            }

            rect += 1;
        }
    }

    for (k, (y0, y1, s)) in bands.iter().enumerate() {
        let above = (k > 0 && bands[k - 1].1 + 1 == *y0).then(|| bands[k - 1].2.as_slice()).unwrap_or(&[]);
        let below = (k + 1 < bands.len() && bands[k + 1].0 == y1 + 1).then(|| bands[k + 1].2.as_slice()).unwrap_or(&[]);
        let index = |x: i32| first[k] + s.iter().position(|&(a, b)| a <= x && x <= b).unwrap_or(0);
        for (a, b) in uncovered(s, above) {
            for x in a..=b {
                let r = index(x);
                put((x, *y0), (x + 1, *y0), (r, 2 * i64::from(x - s[r - first[k]].0)));
            }
        }

        for (a, b) in uncovered(s, below) {
            for x in a..=b {
                let r = index(x);
                put((x + 1, y1 + 1), (x, y1 + 1), (r, 2 * i64::from(x - s[r - first[k]].0) + 1));
            }
        }
    }

    timed
        .into_iter()
        .map(|(k, mut v)| {
            v.sort_by_key(|e| e.0);
            (k, v.into_iter().map(|e| e.1).collect())
        })
        .collect()
}

/// fn 0x1005c990 as it is: each edge added, its reverse cancelled (the reference for `edges`).
#[cfg(test)]
fn edges_by_insertion(region: &Region) -> Edges {
    let mut m = Edges::new();
    let mut add = |a: (i32, i32), b: (i32, i32)| {
        if let Some(v) = m.get_mut(&b)
            && let Some(i) = v.iter().position(|&e| e == a)
        {
            v.remove(i);
            if v.is_empty() {
                m.remove(&b);
            }

            return;
        }

        m.entry(a).or_default().push(b);
    };

    for (x0, y0, x1, y1) in region.rects() {
        for x in x0..=x1 {
            add((x, y0), (x + 1, y0));
            add((x + 1, y1 + 1), (x, y1 + 1));
        }

        for y in y0..=y1 {
            add((x1 + 1, y), (x1 + 1, y + 1));
            add((x0, y + 1), (x0, y));
        }
    }

    m
}

fn remove_edge(m: &mut Edges, a: (i32, i32), b: (i32, i32)) {
    if let Some(v) = m.get_mut(&a) {
        if let Some(i) = v.iter().position(|&e| e == b) {
            v.remove(i);
        }

        if v.is_empty() {
            m.remove(&a);
        }
    }
}

/// A traced polygon's points (units) and its holes' indices.
type Node = (Vec<(i32, i32)>, Vec<usize>);

fn build(arena: &[Node], i: usize) -> Polygon {
    Polygon {
        points: arena[i].0.iter().map(|&(x, y)| (f64::from(x), f64::from(y))).collect(),
        children: arena[i].1.iter().map(|&c| build(arena, c)).collect(),
    }
}

/// The Zonifier (fn 0x1005da20): a region's polygons with their holes, in units.
fn zonify(region: &Region) -> Vec<Polygon> {
    let mut edges = edges(region);

    // Trace: arena of polygons (points, children), the top-level list.
    let mut arena: Vec<Node> = vec![];
    let mut seen: HashMap<(i32, i32), usize> = HashMap::new();
    let mut max_x = f32::NEG_INFINITY as f64;
    while let Some((&s, ends)) = edges.iter().next() {
        let id = arena.len();
        let mut e = (s, ends[0]);
        let mut pts = vec![s];
        loop {
            seen.insert(e.0, id);
            max_x = max_x.max(f64::from(e.0.0));
            let Some(next) = edges.get(&e.1).filter(|v| !v.is_empty()) else { break };
            let lo = (e.1, next[0]);
            let cross = ((lo.1.1 - lo.0.1) * (e.1.0 - e.0.0) - (e.1.1 - e.0.1) * (lo.1.0 - lo.0.0)) as f32;
            let mut chosen = lo;
            if cross != 0.0 {
                if next.len() > 1 && cross > 0.0 {
                    chosen = (e.1, next[1]);
                }

                pts.push(e.1);
            }

            remove_edge(&mut edges, e.0, e.1);
            e = chosen;
        }

        remove_edge(&mut edges, e.0, e.1);
        arena.push((pts, vec![]));
    }

    // Each polygon's area with its holes' (fn 0x1005c1a0's sum: its own, then its holes' as they come).
    let mut areas: Vec<f32> = (0..arena.len()).map(|i| build(&arena, i).area()).collect();

    // Holes: from each polygon's rightmost vertex step right to a polygon of the other sign.
    let mut list: Vec<usize> = (0..arena.len()).collect();
    let mut k = 0;
    while k < list.len() {
        let p = list[k];
        let pts = &arena[p].0;
        let mut m = 0;
        for (i, q) in pts.iter().enumerate() {
            if pts[m].0 < q.0 {
                m = i;
            }
        }

        let (vx, vy) = pts[m];
        let mut x = f64::from(vx) + 1.0;
        let mut outer = None;
        while x <= max_x {
            if let Some(&q) = seen.get(&(x as i32, vy))
                && q != p
                && (areas[q] < 0.0) != (areas[p] < 0.0)
            {
                outer = Some(q);
                break;
            }

            x += 1.0;
        }

        match outer {
            Some(q) => {
                arena[q].1.push(p);
                areas[q] += areas[p];
                list.remove(k);
            }
            None => k += 1,
        }
    }

    list.iter().map(|&i| build(&arena, i)).collect()
}

/// Clips a polygon's outline to the left of a line (x0, y0, x1, y1), fn 0x10057b00 (Sutherland-Hodgman, f32).
fn clip(points: &[(f64, f64)], l: [f32; 4]) -> Vec<(f64, f64)> {
    let mut out = vec![];
    let Some(&last) = points.last() else { return out };
    let side = |p: (f32, f32)| (p.1 - l[1]) * (l[2] - l[0]) + -(l[3] - l[1]) * (p.0 - l[0]);
    let mut prev = (last.0 as f32, last.1 as f32);
    for &c in points {
        let cur = (c.0 as f32, c.1 as f32);
        let sc = side(cur);
        if (sc > 0.0) != (side(prev) > 0.0) {
            let (ly, lx) = (l[3] - l[1], l[2] - l[0]);
            let d = ly * (cur.0 - prev.0) - lx * (cur.1 - prev.1);
            let t = if d != 0.0 { (lx * (prev.1 - l[1]) - ly * (prev.0 - l[0])) / d } else { 0.0 };
            out.push((f64::from((cur.0 - prev.0) * t + prev.0), f64::from((cur.1 - prev.1) * t + prev.1)));
        }

        if sc > 0.0 {
            out.push(c);
        }

        prev = cur;
    }

    out
}

/// `split`: the two zones' outlines cut by two lines along their centroids' axis, `width` apart (fn 0x1005d390); each side's pieces
/// the children of an empty polygon.
fn split(zones: &[Polygon], width: f32) -> Option<(Polygon, Polygon)> {
    let c1 = zones[1].centroid();
    let c0 = zones[0].centroid();
    let (dx, dy) = (c1.0 - c0.0, c1.1 - c0.1);
    let len = (dx * dx + dy * dy).sqrt();
    if len <= 0.0 {
        return None;
    }

    let m = (1.0 / len * dx + c0.0, dy * (1.0 / len) + c0.1);
    let (ux, uy) = (m.0 - c0.0, m.1 - c0.1);
    let hw = width * 0.5;
    let a = [-uy * hw + c0.0, ux * hw + c0.1, -uy * hw + m.0, ux * hw + m.1];
    let b = [uy * hw + m.0, -ux * hw + m.1, uy * hw + c0.0, -ux * hw + c0.1];
    let (mut sa, mut sb) = (Polygon::default(), Polygon::default());
    for z in zones {
        sa.children.push(Polygon { points: clip(&z.points, a), children: vec![] });
        sb.children.push(Polygon { points: clip(&z.points, b), children: vec![] });
    }

    Some((sa, sb))
}

/// The zones of a BCA centred on `centre` for a deployment and a configuration (-1: the one of a deployment without `dual_config`): its
/// two zones (or fewer) and its errors.
#[allow(clippy::too_many_arguments)]
pub fn zones(
    spec: &Spec,
    dep: &DeploymentSpec,
    db: &TileDatabase,
    w: u32,
    h: u32,
    instances: &[Placement],
    cells: &[i32],
    masks: &HashMap<(usize, usize), DeploymentMask>,
    config: i32,
    centre: (i32, i32),
    inset: i32,
) -> (Vec<Polygon>, Vec<String>) {
    let mut gathered = vec![];
    for dy in -4..4 {
        for dx in -4..4 {
            let (x, y) = (centre.0 + dx, centre.1 + dy);
            if x < 0 || y < 0 || x >= w as i32 || y >= h as i32 {
                continue;
            }

            let j = cells[(y as u32 * w + x as u32) as usize];
            if j >= 0 && !gathered.contains(&(j as usize)) {
                gathered.push(j as usize);
            }
        }
    }

    let clip_to = (-SIDE / 2 + inset, SIDE / 2 - 1 - inset);
    let mut memo = HashMap::new();
    let mut restricted = |p: &Placement| *memo.entry((p.tile, p.variation)).or_insert_with(|| in_list(db, &dep.restriction, p));
    let mut region = Region::new();
    for &i in &gathered {
        let p = &instances[i];
        let t = &db.tiles[p.tile];
        let rot = p.flags & 0xf0;
        if let Some(m) = masks.get(&(p.tile, p.variation)) {
            let mut rects = m.key(config);
            if rects.is_empty() && config != -1 {
                rects = m.key(-1);
            }

            if !rects.is_empty() {
                let (tw, th) = (t.width * UNITS, t.height * UNITS);
                let (ox, oy) = ((i32::from(p.x) - centre.0) * UNITS, (i32::from(p.y) - centre.1) * UNITS);
                for &(x0, y0, x1, y1) in rects {
                    let a = rotate_in_tile_space(tw, th, rot, x0, y0);
                    let b = rotate_in_tile_space(tw, th, rot, x1, y1);
                    region.fill(ox + a.0.min(b.0), oy + a.1.min(b.1), ox + a.0.max(b.0) + 1, oy + a.1.max(b.1) + 1, clip_to);
                }

                continue;
            }
        }

        if restricted(p) {
            continue;
        }

        for sy in 0..t.height {
            for sx in 0..t.width {
                if !t.subtile_masked_valid(sx, sy) {
                    continue;
                }

                let (rx, ry) = rotate_in_tile_space(t.width, t.height, rot, sx, sy);
                let (cx, cy) = (i32::from(p.x) + rx - centre.0, i32::from(p.y) + ry - centre.1);
                region.fill(cx * UNITS, cy * UNITS, (cx + 1) * UNITS, (cy + 1) * UNITS, clip_to);
            }
        }
    }

    let mut zs = zonify(&region);
    for z in &mut zs {
        z.scale(1.0 / f64::from(UNITS));
        z.simplify(spec.deployment_coarseness);
    }

    let mut errors = vec![];
    if zs.len() < 2 {
        errors.push("Less than two zones created".to_owned());
        return (zs, errors);
    }

    if zs.len() > 2 {
        errors.push("More than two zones created - just kept largest two".to_owned());
        let mut keyed: Vec<(f32, Polygon)> = zs.into_iter().map(|z| (z.area(), z)).collect();
        keyed.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));
        zs = keyed.into_iter().take(2).map(|(_, z)| z).collect();
    }

    if dep.split
        && let Some((a, b)) = split(&zs, spec.deployment_split_width)
    {
        zs = vec![a, b];
    }

    (zs, errors)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn region(rects: &[(i32, i32, i32, i32)]) -> Region {
        let mut r = Region::new();
        for &(x0, y0, x1, y1) in rects {
            r.fill(x0, y0, x1, y1, (-SIDE / 2, SIDE / 2 - 1));
        }

        r
    }

    #[test]
    fn two_squares_are_two_clockwise_zones() {
        let z = zonify(&region(&[(0, 0, 10, 10), (20, 0, 30, 5)]));
        assert_eq!(z.len(), 2);
        assert_eq!(z[0].points, vec![(0.0, 0.0), (10.0, 0.0), (10.0, 10.0), (0.0, 10.0)]);
        assert_eq!(z[0].area(), 100.0);
        assert_eq!(z[1].area(), 50.0);
    }

    #[test]
    fn a_hole_belongs_to_its_outline() {
        let mut r = region(&[(0, 0, 30, 30)]);
        for y in 10..20 {
            for x in 10..20 {
                r.clear(x, y);
            }
        }

        let z = zonify(&r);
        assert_eq!(z.len(), 1);
        assert_eq!(z[0].children.len(), 1);
        assert!(z[0].children[0].area() < 0.0);
        assert_eq!(z[0].area(), 800.0);
    }

    #[test]
    fn simplifying_drops_near_points() {
        let mut p = Polygon { points: vec![(0.0, 0.0), (5.0, 0.01), (10.0, 0.0), (10.0, 10.0), (0.0, 10.0)], children: vec![] };
        p.simplify(0.05);
        assert_eq!(p.points, vec![(0.0, 0.0), (10.0, 0.0), (10.0, 10.0), (0.0, 10.0)]);
    }

    /// The edges built from the bands are fn 0x1005c990's, insertion order at pinch points included, on random regions.
    #[test]
    fn edges_from_bands_are_the_inserted_ones() {
        let mut seed = 12345u64;
        let mut rnd = |n: i32| {
            seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
            ((seed >> 33) % n as u64) as i32
        };

        for _ in 0..300 {
            let mut r = Region::new();
            for _ in 0..1 + rnd(12) {
                let (x, y) = (rnd(40) - 20, rnd(40) - 20);
                r.fill(x, y, x + 1 + rnd(12), y + 1 + rnd(12), (-SIDE / 2, SIDE / 2 - 1));
            }

            for _ in 0..rnd(30) {
                r.clear(rnd(40) - 20, rnd(40) - 20);
            }

            assert_eq!(edges(&r), edges_by_insertion(&r));
        }
    }

    #[test]
    fn the_battle_inset_of_unit_2() {
        assert_eq!(battle_inset(2.0), 33);
    }
}
