//! Stage B: `terrain_outlines.xml`, the no-go areas BOB computes from the final heights (ToolDataBuilderDll `FUN_10088790` ->
//! `FUN_100877d0` -> EmpireUtility's `no_go_outlines_from_heights`). A sample is no-go where the terrain is steeper than the passable
//! slope (`HEIGHTFIELD_OUTLINE_DATA_PROVIDER`), the no-go field is blurred (`BLUR_OUTLINE_DATA_PROVIDER`), traced into outlines by
//! marching squares (`OUTLINE_CALCULATOR`) and simplified (`OUTLINE_SIMPLIFIER`). docs/formats/tile_compile.md, "terrain_outlines".

#![allow(clippy::needless_range_loop)]

/// The no-go samples of a field of heights (`w` x `h`, rows as `final_heights.dds`; EmpireUtility `FUN_10037df0` / `FUN_10a82a10`):
/// the normal by central differences (the sample itself past the edges) at a spacing of the tile database's `unit_scale` (2) per
/// sample, no-go where its upright component is below the sine of 90 degrees less the passable slope (`MAXIMUM_PASSABLE_SLOPE_ANGLE_
/// DEGREES`, 30), all in f32.
pub fn no_go(heights: &[f32], w: usize, h: usize, spacing: f32, max_slope_degrees: f32) -> Vec<bool> {
    let limit = (f64::from((90.0f32 - max_slope_degrees) * 0.017_453_292)).sin() as f32;
    let at = |x: usize, y: usize| heights[y * w + x];
    let mut out = Vec::with_capacity(w * h);
    for y in 0..h {
        for x in 0..w {
            let here = at(x, y);
            let left = if x > 0 { at(x - 1, y) } else { here };
            let right = if x + 1 < w { at(x + 1, y) } else { here };
            let up = if y > 0 { at(x, y - 1) } else { here };
            let down = if y + 1 < h { at(x, y + 1) } else { here };
            let (sy, sx) = (spacing * 2.0, spacing * 2.0);
            let ny = sy * sx;
            let nx = (down - up) * 0.0 - sy * (right - left);
            let nz = (right - left) * 0.0 - (down - up) * sx;
            let inv = 1.0f32 / (ny * ny + nx * nx + nz * nz).sqrt();
            // NaN normals count as no-go, as BOB's `limit <= y` test fails for them.
            out.push(!matches!(limit.partial_cmp(&(ny * inv)), Some(std::cmp::Ordering::Less | std::cmp::Ordering::Equal)));
        }
    }

    out
}

/// `BLUR_OUTLINE_DATA_PROVIDER::blur(3, 0.1)` (EmpireUtility `FUN_10a80e30`): a separable triangle kernel of radius 3 ((3 - |i|) / 3 x
/// 1 / 3), across then down, the samples clamped at the edges, set where the sum is above 0.1.
pub fn blur(field: &[bool], w: usize, h: usize) -> Vec<bool> {
    const R: i32 = 3;
    let k = (-R..=R).map(|i| (R - i.abs()) as f32 / R as f32 * (1.0f32 / R as f32)).collect::<Vec<_>>();
    let mut across = vec![0.0f32; w * h];
    for y in 0..h {
        for x in 0..w {
            let mut s = 0.0f32;
            for (j, i) in (-R..=R).enumerate() {
                let sx = (x as i32 + i).clamp(0, w as i32 - 1) as usize;
                s += if field[y * w + sx] { 1.0 } else { 0.0 } * k[j];
            }

            across[y * w + x] = s;
        }
    }

    let mut out = vec![false; w * h];
    for y in 0..h {
        for x in 0..w {
            let mut s = 0.0f32;
            for (j, i) in (-R..=R).enumerate() {
                let sy = (y as i32 + i).clamp(0, h as i32 - 1) as usize;
                s += across[sy * w + x] * k[j];
            }

            out[y * w + x] = s > 0.1 || s.is_nan();
        }
    }

    out
}

/// The marching squares table (EmpireUtility 0x10df6980): per case of a 2 x 2 window (bit 1 the cell, 2 the one left of it, 4 the one
/// above, 8 above left) the move, three edge points relative to (x + 1, y + 1) and the cells it marks. Entries 16 and 17 resolve the
/// saddles 6 and 9.
const TABLE: [[i16; 10]; 18] = [
    [0, 1, 0, 0, 0, 0, 0, 0, 0, 0],
    [1, 1, 0, -1, 0, -1, -1, 0, -1, 1],
    [2, 0, 1, -2, -1, -1, -1, -1, 0, 2],
    [3, 1, 0, -2, -1, -1, -1, 0, -1, 3],
    [4, 0, -1, 0, -1, -1, -1, -1, -2, 4],
    [5, 0, -1, -1, 0, -1, -1, -1, -2, 5],
    [6, 0, 1, -2, -1, -1, -1, -1, 0, 2],
    [7, 0, -1, -2, -1, -1, -1, -1, -2, 7],
    [8, -1, 0, -1, -2, -1, -1, -2, -1, 8],
    [9, -1, 0, -1, -2, -1, -1, -2, -1, 8],
    [10, 0, 1, -1, -2, -1, -1, -1, 0, 10],
    [11, 1, 0, -1, -2, -1, -1, 0, -1, 11],
    [12, -1, 0, 0, -1, -1, -1, -2, -1, 12],
    [13, -1, 0, -1, 0, -1, -1, -2, -1, 13],
    [14, 0, 1, 0, -1, -1, -1, -1, 0, 14],
    [15, 0, 0, 0, 0, 0, 0, 15, 0, 0],
    [6, 0, -1, 0, -1, -1, -1, -1, -2, 4],
    [9, 1, 0, -1, 0, -1, -1, 0, -1, 1],
];

type Point = (i64, i64);

/// `OUTLINE_CALCULATOR::add_point` (`FUN_10a8fa90`): a point back at the start closes the outline; past two points, one in line with
/// the last two (same x or same y) moves the last instead.
fn add_point(edges: &mut Vec<Point>, p: Point, closed: &mut bool) {
    if *closed {
        return;
    }

    if edges.first() == Some(&p) {
        *closed = true;
    }

    if edges.len() > 2 {
        let (a, b) = (edges[edges.len() - 2], edges[edges.len() - 1]);
        if a.0 == b.0 && a.0 == p.0 {
            edges.last_mut().expect("> 2").1 = p.1;
            return;
        }

        if a.1 == b.1 && a.1 == p.1 {
            edges.last_mut().expect("> 2").0 = p.0;
            return;
        }
    }

    edges.push(p);
}

/// A traced outline: its edge points (closed, the first repeated last) and the box of the cells walked.
struct Traced {
    points: Vec<Point>,
}

/// `OUTLINE_CALCULATOR::compute_outlines` (`FUN_10a99770`) with a minimum size: row by row, an unmarked set cell starting an outline (a
/// case 1, or a case 9 whose upper left cell is already traced) not inside one found before is walked by the table until the outline
/// closes (or w h / 4 steps), its cells marked with the outline's number; it is kept when its walk is wider or taller than the
/// minimum.
fn trace(set: &[bool], w: usize, h: usize, min_size: i64) -> Vec<Traced> {
    let mut g = set.iter().map(|&s| u32::from(s)).collect::<Vec<_>>();
    let get = |g: &[u32], x: i64, y: i64| g[y as usize * w + x as usize];
    let case = |g: &[u32], x: i64, y: i64| {
        let (wi, hi) = (w as i64, h as i64);
        let mut c = 0u8;
        if y < hi {
            if x < wi && get(g, x, y) >= 1 {
                c |= 1;
            }
            if x > 0 && x - 1 < wi && get(g, x - 1, y) >= 1 {
                c |= 2;
            }
        }

        if y > 0 && y - 1 < hi {
            if x > 0 && x - 1 < wi && get(g, x - 1, y - 1) >= 1 {
                c |= 8;
            }
            if x < wi && get(g, x, y - 1) >= 1 {
                c |= 4;
            }
        }

        c
    };
    let mut out: Vec<(Traced, [i64; 4])> = vec![];
    let (mut cx, mut cy) = (0i64, 0i64);
    let mut id = 2u32;
    let limit = (w * h) as i64 >> 2;
    loop {
        // `get_data` (`FUN_10a815f0`): the next start.
        let mut found = false;
        while cy < h as i64 {
            while cx < w as i64 {
                if get(&g, cx, cy) == 1 {
                    let c = case(&g, cx, cy);
                    if c == 1 || (c == 9 && get(&g, cx - 1, cy - 1) > 1) {
                        found = true;
                        break;
                    }
                }
                cx += 1;
            }

            if found {
                break;
            }
            cy += 1;
            cx = 0;
        }

        if !found {
            break;
        }

        let inside = out.iter().any(|(t, b)| b[0] <= cx && cx <= b[2] && b[1] <= cy && cy <= b[3] && inside_outline(&t.points, cx, cy));
        if inside {
            cx += 1;
            continue;
        }

        let (mut x, mut y, mut px, mut py, mut pc) = (cx, cy, cx, cy, 0u8);
        let (mut edges, mut closed, mut n) = (vec![], false, 0i64);
        let mut bb = [x, y, x, y];
        loop {
            let c = case(&g, x, y);
            let e = match c {
                9 if pc == 0 || y < py => 17,
                6 if x < px => 16,
                _ => c as usize,
            };
            let t = TABLE[e];
            if !closed {
                let p = |k: usize| (i64::from(t[k]) + x + 1, i64::from(t[k + 1]) + y + 1);
                let (p1, p2, p3) = (p(3), p(5), p(7));
                if edges.len() < 3 || (p1 != edges[edges.len() - 2] && p2 != edges[edges.len() - 1]) {
                    add_point(&mut edges, p1, &mut closed);
                    add_point(&mut edges, p2, &mut closed);
                }

                add_point(&mut edges, p3, &mut closed);
            }

            let flags = t[9];
            for (bit, mx, my) in [(1, 0, 0), (2, -1, 0), (4, 0, -1), (8, -1, -1)] {
                if flags & bit != 0 {
                    g[(y + my) as usize * w + (x + mx) as usize] = id;
                }
            }

            (px, py, pc) = (x, y, c);
            x += i64::from(t[1]);
            y += i64::from(t[2]);
            bb = [bb[0].min(x), bb[1].min(y), bb[2].max(x), bb[3].max(y)];
            n += 1;
            if closed || n >= limit {
                break;
            }
        }

        if n != limit && (min_size == 0 || min_size < bb[2] - bb[0] || min_size < bb[3] - bb[1]) {
            out.push((Traced { points: edges }, bb));
        }

        id += 1;
        cx += 1;
    }

    out.into_iter().map(|(t, _)| t).collect()
}

/// `OUTLINE_DATA_CONTAINER::is_point_inside_outline`: the crossing rule.
fn inside_outline(points: &[Point], x: i64, y: i64) -> bool {
    let mut odd = false;
    let n = points.len();
    for i in 0..n {
        let ((xi, yi), (xj, yj)) = (points[i], points[(i + n - 1) % n]);
        if (yi > y) != (yj > y) && (x as f64) < ((xj - xi) * (y - yi)) as f64 / (yj - yi) as f64 + xi as f64 {
            odd = !odd;
        }
    }

    odd
}

type P = (f32, f32);
type Line = (f32, f32, f32, f32);

/// `LINE_2::intersects` (`FUN_10a85b10`, no end flags, no epsilon): whether two segments meet, and where along the first (`a`).
fn intersects(a: Line, b: Line) -> Option<f32> {
    let (f8, f6) = (a.3 - a.1, a.2 - a.0);
    let (f7, f12) = (b.2 - b.0, b.3 - b.1);
    let den = f12 * f6 - f7 * f8;
    if den.abs() <= 0.0 || den.is_nan() {
        return None;
    }

    let (f10, f11) = (a.0 - b.0, a.1 - b.1);
    let inv = 1.0f32 / den;
    let t = (f7 * f11 - f12 * f10) * inv;
    if !(0.0..=1.0).contains(&t) {
        return None;
    }

    let u = (f6 * f11 - f8 * f10) * inv;
    (0.0..=1.0).contains(&u).then_some(t)
}

/// `FUN_10a85fc0`: whether `line` crosses an edge of the outline from `start` on (the last edge closing it), not at the edge's ends.
fn crosses(line: Line, points: &[P], start: usize) -> bool {
    let n = points.len();
    if n < 2 {
        return false;
    }

    (start..n).any(|k| {
        let (s, e) = (points[k], points[(k + 1) % n]);
        intersects((s.0, s.1, e.0, e.1), line).is_some_and(|t| t != 0.0 && t != 1.0)
    })
}

/// `LINE_2::intersects_convex_shape` (`FUN_10a840a0`, Cyrus-Beck) against an outline's box.
fn touches_box(line: Line, b: [f32; 4]) -> bool {
    let [x0, y0, x1, y1] = b;
    let edges = [(x0, y1, x1, y1), (x1, y1, x1, y0), (x1, y0, x0, y0), (x0, y0, x0, y1)];
    let (lx0, ly0, lx1, ly1) = line;
    let (mut enter, mut exit): (Option<f32>, Option<f32>) = (None, None);
    for (ex0, ey0, ex1, ey1) in edges {
        let (edx, edy) = (ex1 - ex0, ey1 - ey0);
        let den = edy * (lx1 - lx0) - (ly1 - ly0) * edx;
        let s = -edy * (lx1 - lx0) + edx * (ly1 - ly0);
        if den == 0.0 {
            if s == 0.0 {
                let side = (ly0 - ey0) * edx + -edy * (lx0 - ex0);
                if side > 0.0 {
                    return false;
                }
            }
            continue;
        }

        let t = (edx * (ly0 - ey0) - edy * (lx0 - ex0)) / den;
        if s >= 0.0 {
            if exit.is_none_or(|e| t < e) {
                exit = Some(t);
            }
            if enter.is_none() {
                continue;
            }
        } else if !enter.is_some_and(|e| t <= e) {
            enter = Some(t);
        }

        if let (Some(en), Some(ex)) = (enter, exit)
            && en > ex
        {
            return false;
        }
    }

    match (enter, exit) {
        (Some(en), Some(ex)) => !(ex < 0.0 || en.max(0.0) > 1.0),
        (Some(_), None) => true,
        _ => false,
    }
}

/// `OUTLINE_SIMPLIFIER` (`FUN_10a98c10`) over the outlines (their points, boxes).
struct Simplifier {
    outlines: Vec<Vec<P>>,
    boxes: Vec<[f32; 4]>,
}

impl Simplifier {
    /// `is_valid_outline` (`FUN_10a86110`): the segment from the last kept point to `end` crosses none of the kept segments, none of
    /// the outline's edges from `end` on, nor any other outline whose box it touches.
    fn valid(&self, c: usize, end: usize, kept: &[P]) -> bool {
        let last = kept[kept.len() - 1];
        let to = self.outlines[c][end];
        let line = (last.0, last.1, to.0, to.1);
        for d in 0..self.outlines.len() {
            if d == c {
                for k in 0..kept.len() - 1 {
                    let seg = (kept[k].0, kept[k].1, kept[k + 1].0, kept[k + 1].1);
                    if intersects(seg, line).is_some_and(|t| t != 0.0 && t != 1.0) {
                        return false;
                    }
                }

                if crosses(line, &self.outlines[c], end) {
                    return false;
                }
            } else if touches_box(line, self.boxes[d]) && crosses(line, &self.outlines[d], 0) {
                return false;
            }
        }

        true
    }

    /// One `simplify(iterations, error, lookahead, expand, contract)`: per pass and outline (those a pass left unchanged rest until
    /// the next call), from each kept point the target among the next `lookahead` (from two on, the last point excluded) whose skipped
    /// points lie within `error` of the shortcut (and on an allowed side of it) with the least error (ties to the later), taken when
    /// valid; an outline left with more than two points takes the result.
    fn simplify(&mut self, iterations: usize, error: f32, lookahead: usize, expand: bool, contract: bool) {
        let mut done = vec![false; self.outlines.len()];
        for _ in 0..iterations {
            for c in 0..self.outlines.len() {
                if done[c] {
                    continue;
                }

                let pts = self.outlines[c].clone();
                let n = pts.len();
                let mut kept: Vec<P> = vec![];
                let (mut lo, mut hi) = ((0.0f32, 0.0f32), (0.0f32, 0.0f32));
                let mut i = 0;
                while i < n {
                    let p = pts[i];
                    kept.push(p);
                    if i == 0 {
                        (lo, hi) = (p, p);
                    } else {
                        lo = (lo.0.min(p.0), lo.1.min(p.1));
                        hi = (hi.0.max(p.0), hi.1.max(p.1));
                    }

                    let (mut best_err, mut best) = (error * 2.0, None);
                    let mut j = i + 2;
                    let mut tried = 0;
                    while j + 1 < n && tried < lookahead {
                        let (dx, dy) = (pts[j].0 - p.0, pts[j].1 - p.1);
                        let mut worst = 0.0f32;
                        let mut e = 0.0f32;
                        for q in &pts[i + 1..j] {
                            let len = (dy * dy + dx * dx).sqrt();
                            let t = if len <= 0.0 {
                                0.0
                            } else {
                                let inv = 1.0f32 / len;
                                ((q.1 - p.1) * inv * dy * inv + (q.0 - p.0) * inv * dx * inv).clamp(0.0, 1.0)
                            };
                            let (a, b) = ((dx * t + p.0) - q.0, (dy * t + p.1) - q.1);
                            let dist = (a * a + b * b).sqrt();
                            worst = worst.max(dist);
                            e = worst;
                            if error <= dist {
                                break;
                            }

                            let side = -dx * (q.1 - p.1) + dy * (q.0 - p.0);
                            if (!contract && side > 0.0) || (!expand && side <= 0.0) {
                                e = error;
                                break;
                            }
                        }

                        if e < error && e <= best_err {
                            best_err = e;
                            best = Some(j);
                        }

                        tried += 1;
                        j += 1;
                    }

                    match best {
                        Some(b) if self.valid(c, b, &kept) => i = b,
                        _ => i += 1,
                    }
                }

                if kept.len() > 2 {
                    if kept.len() == n {
                        done[c] = true;
                    }

                    self.outlines[c] = kept;
                    self.boxes[c] = [lo.0, lo.1, hi.0, hi.1];
                }
            }
        }
    }
}

/// `terrain_outlines.xml` from the final heights (`w` x `h`, rows as `final_heights.dds`): the no-go samples ([`no_go`], 2 units per
/// sample, 30 degrees), [`blur`]red, traced (outlines spanning more than 6 cells), in TEd's units (2 per cell), simplified three times
/// (3 passes at 0.99 with a lookahead of 10; 1 at 3 growing only; 1 at 1), written as `<OUTLINE>`s of `<position x='%f' y='%f'/>`
/// with x and `h - y` in cells. (The go outlines a tile has, which BOB rasterises out of the no-go field first, and masked tiles,
/// whose outlines must lie in valid subtiles, are not handled yet.)
/// `FUN_10a81810`: whether `p` is inside the polygon by crossings, the edges (first, last), (first, second), ...; an edge counts
/// when `p.y` lies in `[low y, high y)` and `p` is left of it (`(b.x - a.x)(p.y - a.y) > (p.x - a.x)(b.y - a.y)` from the lower end).
fn contains(points: &[(f32, f32)], p: (f32, f32)) -> bool {
    use std::cmp::Ordering::{Equal, Less};
    // BOB's test is `!(l < r || l == r)`: a NaN product counts as a crossing.
    let above = |l: f32, r: f32| !matches!(l.partial_cmp(&r), Some(Less | Equal));
    let mut inside = false;
    let mut edge = |a: (f32, f32), b: (f32, f32)| {
        let crossed = if a.1 <= b.1 {
            a.1 <= p.1 && p.1 < b.1 && above((b.0 - a.0) * (p.1 - a.1), (p.0 - a.0) * (b.1 - a.1))
        } else {
            b.1 <= p.1 && p.1 < a.1 && above((a.0 - b.0) * (p.1 - b.1), (a.1 - b.1) * (p.0 - b.0))
        };
        if crossed {
            inside = !inside;
        }
    };
    edge(points[0], points[points.len() - 1]);
    for k in 1..points.len() {
        edge(points[k - 1], points[k]);
    }

    inside
}

/// The go outlines cut out of the no-go field (`no_go_outlines_from_heights` -> `FUN_100c0eb0`, a TBB `TBB_HEIGHTFIELD_OUTLINE_RASTERISE`
/// per outline): the outline's points into the field's space (`FUN_100877d0` inverts the output transform: x 2 gx, y -2 gy + 2 h,
/// spacing 2), its box clipped to the field's ((w - 1) x 2, (h - 1) x 2), the samples `[floor(min / 2), floor(max / 2))` both ways;
/// per chunk of 64 rows (TBB's range, from the first row) y = rows into the chunk x 2 + the first row's, then + 2 per row, x from the
/// first column's + 2 per column, all f32; a no-go sample inside the outline ([`contains`]) is cleared.
fn cut_go_outlines(field: &mut [bool], w: usize, h: usize, go: &[Vec<(f32, f32)>]) {
    let (sx, sy) = (2.0f32, 2.0f32);
    let (bx, by) = (((w - 1) as f32) * sx, ((h - 1) as f32) * sy);
    let top = 2.0 * h as f32;
    for outline in go {
        let points = outline.iter().map(|&(x, y)| (2.0 * x, -2.0 * y + top)).collect::<Vec<_>>();
        let (mut lo, mut hi) = ((f32::MAX, f32::MAX), (-f32::MAX, -f32::MAX));
        for &(x, y) in &points {
            lo = (lo.0.min(x), lo.1.min(y));
            hi = (hi.0.max(x), hi.1.max(y));
        }

        if !(lo.0 <= bx && 0.0 <= hi.0 && lo.1 <= by && 0.0 <= hi.1) {
            continue;
        }

        let (lo, hi) = ((lo.0.max(0.0), lo.1.max(0.0)), (hi.0.min(bx), hi.1.min(by)));
        let floor = |v: f32| v.floor() as i64;
        let (c0, r0) = (floor(lo.0 * 0.5), floor(0.5 * lo.1));
        let (c1, r1) = (floor(hi.0 * 0.5), floor(hi.1 * 0.5));
        let (ox, oy) = ((c0 as f64 * f64::from(sx)) as f32, (r0 as f64 * f64::from(sy)) as f32);
        let mut begin = r0;
        while begin < r1 {
            let end = (begin + 64).min(r1);
            let mut py = ((begin - r0) as f32) * sy + oy;
            for row in begin..end {
                let mut px = ox;
                for col in c0..c1 {
                    let i = row as usize * w + col as usize;
                    if field[i] && contains(&points, (px, py)) {
                        field[i] = false;
                    }

                    px += sx;
                }

                py += sy;
            }

            begin = end;
        }
    }
}

pub fn terrain_outlines(heights: &[f32], w: usize, h: usize, go: &[Vec<(f32, f32)>], mask: &[bool], cells: (usize, usize)) -> Vec<u8> {
    wce_foundation::wce_prof!("stage_b.terrain_outlines");
    let mut field = no_go(heights, w, h, 2.0, 30.0);
    cut_go_outlines(&mut field, w, h, go);
    let field = blur(&field, w, h);
    let traced = trace(&field, w, h, 6);
    let outlines = traced
        .iter()
        .map(|t| t.points[..t.points.len() - 1].iter().map(|&(x, y)| (2.0 * x as f32, 2.0 * y as f32)).collect::<Vec<_>>())
        .collect::<Vec<_>>();
    let boxes = outlines
        .iter()
        .map(|o| {
            let (mut lo, mut hi) = (o[0], o[0]);
            for &p in o {
                lo = (lo.0.min(p.0), lo.1.min(p.1));
                hi = (hi.0.max(p.0), hi.1.max(p.1));
            }
            [lo.0, lo.1, hi.0, hi.1]
        })
        .collect();
    let mut s = Simplifier { outlines, boxes };
    s.simplify(3, 0.99, 10, true, true);
    s.simplify(1, 3.0, 4, true, false);
    s.simplify(1, 1.0, 4, true, true);
    // `FUN_100886f0`: an outline stays only when every point's subtile (`(int)(x x f32(1 / 256))`, the field's space) is
    // `subtile_masked_valid`: inside the tile, and in the mask (BOB passes the field's y, rows from the top, where the mask's rows
    // count from the bottom). Points on the far edges (2 (w - 1)) fall in the subtile past the last, so outlines touching the right or
    // bottom edge go.
    // The tile's subtiles (the entry's size: with fewer than 128 samples per subtile the scale no longer matches, as in BOB).
    let (cells_x, cells_y) = (cells.0 as i32, cells.1 as i32);
    let scale = 1.0f32 / (128.0 * 2.0);
    let valid = |o: &&Vec<(f32, f32)>| {
        o.iter().all(|&(x, y)| {
            let (cx, cy) = ((scale * x) as i32, (scale * y) as i32);
            super::bob_tile::subtile_valid(mask, cells_x as usize, cells_y as usize, cx, cy)
        })
    };
    let mut out = String::from("<OUTLINE_LIST>\n");
    for o in s.outlines.iter().filter(valid) {
        out.push_str("<OUTLINE>\n");
        for &(x, y) in o {
            out.push_str(&format!("<position x='{:.6}' y='{:.6}'/>", x / 2.0, h as f32 - y / 2.0));
        }
        out.push_str("</OUTLINE>\n");
    }

    out.push_str("</OUTLINE_LIST>\n");
    out.into_bytes()
}
