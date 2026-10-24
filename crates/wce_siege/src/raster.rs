//! The raster the generator works on: a grid of square cells over the map (scene units), masks on it, and what is done to them
//! (polygons filled, distance transform, flood fill, geodesic split, thinning, contours).

use std::cmp::Reverse;
use std::collections::{BinaryHeap, VecDeque};

use crate::P;

/// The grid: cell (i, j) covers x in [x0 + i cell, x0 + (i + 1) cell), the same for z with j.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Grid {
    pub x0: f64,
    pub z0: f64,
    pub cell: f64,
    pub w: usize,
    pub h: usize,
}

/// One flag per cell, row by row (index j w + i).
pub type Mask = Vec<bool>;

pub const N4: [(isize, isize); 4] = [(1, 0), (-1, 0), (0, 1), (0, -1)];
pub const N8: [(isize, isize); 8] = [(1, 0), (1, 1), (0, 1), (-1, 1), (-1, 0), (-1, -1), (0, -1), (1, -1)];

impl Grid {
    /// A grid covering [lo, hi] with cells of `cell`.
    pub fn new(lo: P, hi: P, cell: f64) -> Self {
        let w = (((hi[0] - lo[0]) / cell).ceil() as usize).max(1);
        let h = (((hi[1] - lo[1]) / cell).ceil() as usize).max(1);
        Self { x0: lo[0], z0: lo[1], cell, w, h }
    }

    pub fn len(&self) -> usize {
        self.w * self.h
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub fn mask(&self) -> Mask {
        vec![false; self.len()]
    }

    pub fn centre(&self, k: usize) -> P {
        [self.x0 + ((k % self.w) as f64 + 0.5) * self.cell, self.z0 + ((k / self.w) as f64 + 0.5) * self.cell]
    }

    /// The cell holding a point, `None` off the grid.
    pub fn at(&self, p: P) -> Option<usize> {
        let (i, j) = (((p[0] - self.x0) / self.cell).floor(), ((p[1] - self.z0) / self.cell).floor());
        (i >= 0.0 && j >= 0.0 && (i as usize) < self.w && (j as usize) < self.h).then(|| j as usize * self.w + i as usize)
    }

    /// The cell nearest a point, clamped onto the grid.
    pub fn clamp(&self, p: P) -> usize {
        let i = (((p[0] - self.x0) / self.cell).floor().max(0.0) as usize).min(self.w - 1);
        let j = (((p[1] - self.z0) / self.cell).floor().max(0.0) as usize).min(self.h - 1);
        j * self.w + i
    }

    /// The neighbour of cell k at offset (di, dj), `None` off the grid.
    pub fn step(&self, k: usize, (di, dj): (isize, isize)) -> Option<usize> {
        let (i, j) = ((k % self.w) as isize + di, (k / self.w) as isize + dj);
        (i >= 0 && j >= 0 && (i as usize) < self.w && (j as usize) < self.h).then(|| j as usize * self.w + i as usize)
    }

    pub fn on_border(&self, k: usize) -> bool {
        let (i, j) = (k % self.w, k / self.w);
        i == 0 || j == 0 || i + 1 == self.w || j + 1 == self.h
    }

    /// Sets the cells whose centres are inside the closed polygon (even-odd).
    pub fn fill_polygon(&self, m: &mut Mask, poly: &[P]) {
        for k in self.polygon_cells(poly) {
            m[k] = true;
        }
    }

    /// The cells whose centres are inside the closed polygon (even-odd).
    pub fn polygon_cells(&self, poly: &[P]) -> Vec<usize> {
        let mut out = vec![];
        if poly.len() < 3 {
            return out;
        }

        let (zmin, zmax) = poly.iter().fold((f64::MAX, f64::MIN), |(a, b), p| (a.min(p[1]), b.max(p[1])));
        let j0 = (((zmin - self.z0) / self.cell - 0.5).ceil().max(0.0)) as usize;
        let j1 = (((zmax - self.z0) / self.cell - 0.5).floor()).min(self.h as f64 - 1.0);
        if j1 < 0.0 {
            return out;
        }

        let mut xs = vec![];
        for j in j0..=j1 as usize {
            let z = self.z0 + (j as f64 + 0.5) * self.cell;
            xs.clear();
            for e in 0..poly.len() {
                let (a, b) = (poly[e], poly[(e + 1) % poly.len()]);
                if (a[1] <= z) != (b[1] <= z) {
                    xs.push(a[0] + (z - a[1]) / (b[1] - a[1]) * (b[0] - a[0]));
                }
            }

            xs.sort_by(f64::total_cmp);
            for pair in xs.as_chunks::<2>().0 {
                let i0 = ((pair[0] - self.x0) / self.cell - 0.5).ceil().max(0.0) as usize;
                let i1 = ((pair[1] - self.x0) / self.cell - 0.5).floor();
                if i1 < 0.0 {
                    continue;
                }

                for i in i0..=(i1 as usize).min(self.w - 1) {
                    out.push(j * self.w + i);
                }
            }
        }

        out
    }

    /// Sets the cells within `r` of the polyline.
    pub fn fill_line(&self, m: &mut Mask, line: &[P], r: f64) {
        for s in line.windows(2) {
            let (a, b) = (s[0], s[1]);
            let lo = [a[0].min(b[0]) - r, a[1].min(b[1]) - r];
            let hi = [a[0].max(b[0]) + r, a[1].max(b[1]) + r];
            let (k0, k1) = (self.clamp(lo), self.clamp(hi));
            for j in k0 / self.w..=k1 / self.w {
                for i in k0 % self.w..=k1 % self.w {
                    let k = j * self.w + i;
                    if seg_dist(self.centre(k), a, b) <= r {
                        m[k] = true;
                    }
                }
            }
        }
    }

    /// The distance (scene units) from each cell to the nearest set cell of `m` (exact Euclidean, Felzenszwalb and Huttenlocher);
    /// `f64::MAX` everywhere when nothing is set.
    pub fn distance(&self, m: &Mask) -> Vec<f64> {
        const INF: f64 = 1e20;
        let mut d: Vec<f64> = m.iter().map(|&s| if s { 0.0 } else { INF }).collect();
        let n = self.w.max(self.h);
        let (mut f, mut out, mut v, mut z) = (vec![0.0; n], vec![0.0; n], vec![0usize; n], vec![0.0; n + 1]);
        for j in 0..self.h {
            f[..self.w].copy_from_slice(&d[j * self.w..(j + 1) * self.w]);
            edt_1d(&f[..self.w], &mut out[..self.w], &mut v, &mut z);
            d[j * self.w..(j + 1) * self.w].copy_from_slice(&out[..self.w]);
        }

        for i in 0..self.w {
            for j in 0..self.h {
                f[j] = d[j * self.w + i];
            }

            edt_1d(&f[..self.h], &mut out[..self.h], &mut v, &mut z);
            for j in 0..self.h {
                d[j * self.w + i] = out[j];
            }
        }

        d.into_iter().map(|s| if s >= INF / 2.0 { f64::MAX } else { s.sqrt() * self.cell }).collect()
    }

    /// The cells within `r` of a set cell.
    pub fn dilate(&self, m: &Mask, r: f64) -> Mask {
        self.distance(m).into_iter().map(|d| d <= r).collect()
    }

    /// The set cells further than `r` from every unset cell (and from the grid's edge).
    pub fn erode(&self, m: &Mask, r: f64) -> Mask {
        let inv: Mask = m.iter().enumerate().map(|(k, &s)| !s || self.on_border(k)).collect();

        self.distance(&inv).into_iter().zip(m).map(|(d, &s)| s && d > r).collect()
    }

    /// Dilated then eroded: gaps narrower than 2 r closed.
    pub fn close(&self, m: &Mask, r: f64) -> Mask {
        if r <= 0.0 {
            return m.clone();
        }

        self.erode(&self.dilate(m, r), r)
    }

    /// The cells reachable from `seeds` through cells where `pass` is set (4-connected).
    pub fn flood(&self, pass: &Mask, seeds: impl IntoIterator<Item = usize>) -> Mask {
        let mut out = self.mask();
        let mut q = VecDeque::new();
        for s in seeds {
            if pass[s] && !out[s] {
                out[s] = true;
                q.push_back(s);
            }
        }

        while let Some(k) = q.pop_front() {
            for d in N4 {
                if let Some(n) = self.step(k, d)
                    && pass[n] && !out[n]
                {
                    out[n] = true;
                    q.push_back(n);
                }
            }
        }

        out
    }

    /// The mask with its holes (unset cells not reachable from the grid's edge through unset cells) set.
    pub fn fill_holes(&self, m: &Mask) -> Mask {
        let pass: Mask = m.iter().map(|&s| !s).collect();
        let outside = self.flood(&pass, (0..self.len()).filter(|&k| self.on_border(k)));
        outside.into_iter().map(|o| !o).collect()
    }

    /// The 8-connected components of the set cells: a label per cell (`usize::MAX` unset) and their sizes.
    pub fn components(&self, m: &Mask) -> (Vec<usize>, Vec<usize>) {
        let mut label = vec![usize::MAX; self.len()];
        let mut sizes = vec![];
        let mut q = VecDeque::new();
        for s in 0..self.len() {
            if !m[s] || label[s] != usize::MAX {
                continue;
            }

            let id = sizes.len();
            label[s] = id;
            q.push_back(s);
            let mut n = 0;
            while let Some(k) = q.pop_front() {
                n += 1;
                for d in N8 {
                    if let Some(c) = self.step(k, d)
                        && m[c] && label[c] == usize::MAX
                    {
                        label[c] = id;
                        q.push_back(c);
                    }
                }
            }

            sizes.push(n);
        }

        (label, sizes)
    }

    /// The set cells' split between sources: each cell reachable from a source through set cells (8-connected, steps of their length)
    /// within the source's reach gets the nearest one's index; `usize::MAX` elsewhere. Also the distance to it.
    pub fn geodesic_split(&self, m: &Mask, sources: &[(usize, f64)]) -> (Vec<usize>, Vec<f64>) {
        let mut owner = vec![usize::MAX; self.len()];
        let mut dist = vec![f64::MAX; self.len()];
        let mut heap = BinaryHeap::new();
        let key = |d: f64| Reverse((d * 1000.0) as u64);
        for (s, &(k, _)) in sources.iter().enumerate() {
            if dist[k] > 0.0 {
                dist[k] = 0.0;
                owner[k] = s;
                heap.push((key(0.0), k, s));
            }
        }

        while let Some((Reverse(dk), k, s)) = heap.pop() {
            let d = dk as f64 / 1000.0;
            if owner[k] != s || d > dist[k] + 1e-6 {
                continue;
            }

            for (di, dj) in N8 {
                let Some(n) = self.step(k, (di, dj)) else { continue };
                if !m[n] {
                    continue;
                }

                let nd = d + self.cell * if di != 0 && dj != 0 { std::f64::consts::SQRT_2 } else { 1.0 };
                if nd < dist[n] && nd <= sources[s].1 {
                    dist[n] = nd;
                    owner[n] = s;
                    heap.push((key(nd), n, s));
                }
            }
        }

        (owner, dist)
    }

    /// The mask thinned to lines one cell wide (Zhang and Suen), its topology kept.
    pub fn thin(&self, m: &Mask) -> Mask {
        let mut m: Mask = m.iter().enumerate().map(|(k, &s)| s && !self.on_border(k)).collect();

        let mut gone = vec![];
        loop {
            let mut changed = false;
            for pass in 0..2 {
                gone.clear();
                for k in 0..self.len() {
                    if !m[k] || self.on_border(k) {
                        continue;
                    }

                    // P2..P9 clockwise from north (north is -z here; the rule is symmetric under the flip).
                    let w = self.w;
                    let p = [m[k - w], m[k - w + 1], m[k + 1], m[k + w + 1], m[k + w], m[k + w - 1], m[k - 1], m[k - w - 1]];
                    let b = p.iter().filter(|&&x| x).count();
                    if !(2..=6).contains(&b) {
                        continue;
                    }

                    let a = (0..8).filter(|&i| !p[i] && p[(i + 1) % 8]).count();
                    if a != 1 {
                        continue;
                    }

                    let ok = if pass == 0 { !(p[0] && p[2] && p[4]) && !(p[2] && p[4] && p[6]) } else { !(p[0] && p[2] && p[6]) && !(p[0] && p[4] && p[6]) };
                    if ok {
                        gone.push(k);
                    }
                }

                for &k in &gone {
                    m[k] = false;
                }

                changed |= !gone.is_empty();
            }

            if !changed {
                return m;
            }
        }
    }

    /// The outlines of the set cells: closed loops through the cell corners, all turning the same way around the set cells (holes the
    /// other way).
    pub fn contours(&self, m: &Mask) -> Vec<Vec<P>> {
        let (w, h) = (self.w, self.h);
        let set = |i: isize, j: isize| i >= 0 && j >= 0 && (i as usize) < w && (j as usize) < h && m[j as usize * w + i as usize];

        // Directed boundary edges between corners (corner (i, j) at x0 + i cell): a set cell's side towards an unset one, set on the left
        // going around. Start corner -> direction.
        let mut next: std::collections::HashMap<(isize, isize), Vec<(isize, isize)>> = std::collections::HashMap::new();
        for j in 0..h as isize {
            for i in 0..w as isize {
                if !set(i, j) {
                    continue;
                }

                if !set(i, j - 1) {
                    next.entry((i + 1, j)).or_default().push((i, j));
                }

                if !set(i, j + 1) {
                    next.entry((i, j + 1)).or_default().push((i + 1, j + 1));
                }

                if !set(i - 1, j) {
                    next.entry((i, j)).or_default().push((i, j + 1));
                }

                if !set(i + 1, j) {
                    next.entry((i + 1, j + 1)).or_default().push((i + 1, j));
                }
            }
        }

        let mut loops = vec![];
        let mut starts: Vec<(isize, isize)> = next.keys().copied().collect();
        starts.sort_unstable();
        for s in starts {
            while next.get(&s).is_some_and(|v| !v.is_empty()) {
                let mut lp = vec![];
                let mut at = s;
                let mut prev_dir = (0isize, 0isize);
                while let Some(outs) = next.get_mut(&at).filter(|v| !v.is_empty()) {

                    // At a corner touched by two loops (cells meeting diagonally), turn the same way every time.
                    let pick = if outs.len() > 1 {
                        let turn = |t: &(isize, isize)| {
                            let d = (t.0 - at.0, t.1 - at.1);
                            prev_dir.0 * d.1 - prev_dir.1 * d.0
                        };
                        (0..outs.len()).max_by_key(|&i| turn(&outs[i])).unwrap_or(0)
                    } else {
                        0
                    };
                    let to = outs.swap_remove(pick);
                    prev_dir = (to.0 - at.0, to.1 - at.1);
                    lp.push([self.x0 + at.0 as f64 * self.cell, self.z0 + at.1 as f64 * self.cell]);
                    at = to;
                    if at == s {
                        break;
                    }
                }

                if lp.len() >= 4 {
                    loops.push(lp);
                }
            }
        }

        loops
    }
}

/// One row of the squared distance transform (`f`: 0 at features, a large number elsewhere; in cells).
fn edt_1d(f: &[f64], d: &mut [f64], v: &mut [usize], z: &mut [f64]) {
    let n = f.len();
    let mut k = 0usize;
    v[0] = 0;
    z[0] = f64::MIN;
    z[1] = f64::MAX;
    for q in 1..n {
        let s = loop {
            let p = v[k];
            let s = ((f[q] + (q * q) as f64) - (f[p] + (p * p) as f64)) / (2.0 * q as f64 - 2.0 * p as f64);
            if s <= z[k] && k > 0 {
                k -= 1;
            } else {
                break s;
            }
        };

        k += 1;
        v[k] = q;
        z[k] = s;
        z[k + 1] = f64::MAX;
    }

    k = 0;
    for (q, out) in d.iter_mut().enumerate().take(n) {
        while z[k + 1] < q as f64 {
            k += 1;
        }

        let p = v[k];
        *out = (q as f64 - p as f64).powi(2) + f[p];
    }
}

pub fn seg_dist(p: P, a: P, b: P) -> f64 {
    let (dx, dz) = (b[0] - a[0], b[1] - a[1]);
    let l2 = dx * dx + dz * dz;
    let t = if l2 > 0.0 { (((p[0] - a[0]) * dx + (p[1] - a[1]) * dz) / l2).clamp(0.0, 1.0) } else { 0.0 };
    (p[0] - a[0] - t * dx).hypot(p[1] - a[1] - t * dz)
}

/// Douglas and Peucker: the points of an open line kept within `tol` of it.
pub fn simplify(line: &[P], tol: f64) -> Vec<P> {
    if line.len() <= 2 {
        return line.to_vec();
    }

    let mut keep = vec![false; line.len()];
    keep[0] = true;
    keep[line.len() - 1] = true;
    let mut stack = vec![(0usize, line.len() - 1)];
    while let Some((a, b)) = stack.pop() {
        let (mut far, mut at) = (0.0, 0);
        for i in a + 1..b {
            let d = seg_dist(line[i], line[a], line[b]);
            if d > far {
                (far, at) = (d, i);
            }
        }

        if far > tol {
            keep[at] = true;
            stack.push((a, at));
            stack.push((at, b));
        }
    }

    line.iter().zip(keep).filter(|(_, k)| *k).map(|(p, _)| *p).collect()
}

/// A closed loop simplified: split at its two most distant points, each half simplified.
pub fn simplify_loop(lp: &[P], tol: f64) -> Vec<P> {
    if lp.len() <= 4 {
        return lp.to_vec();
    }

    let far = (1..lp.len()).max_by(|&a, &b| dist(lp[0], lp[a]).total_cmp(&dist(lp[0], lp[b]))).unwrap_or(1);
    let mut first = simplify(&lp[..=far], tol);
    let mut second = lp[far..].to_vec();
    second.push(lp[0]);
    let second = simplify(&second, tol);
    first.pop();
    first.extend_from_slice(&second[..second.len() - 1]);
    first
}

pub fn dist(a: P, b: P) -> f64 {
    (a[0] - b[0]).hypot(a[1] - b[1])
}

/// Even-odd point in polygon.
pub fn inside(p: P, poly: &[P]) -> bool {
    let mut c = false;
    for e in 0..poly.len() {
        let (a, b) = (poly[e], poly[(e + 1) % poly.len()]);
        if (a[1] > p[1]) != (b[1] > p[1]) && p[0] < a[0] + (p[1] - a[1]) / (b[1] - a[1]) * (b[0] - a[0]) {
            c = !c;
        }
    }

    c
}

/// Twice the signed area (shoelace).
pub fn area2(poly: &[P]) -> f64 {
    (0..poly.len()).map(|i| {
        let (a, b) = (poly[i], poly[(i + 1) % poly.len()]);
        a[0] * b[1] - b[0] * a[1]
    }).sum()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn distance_is_euclidean() {
        let g = Grid::new([0.0, 0.0], [20.0, 20.0], 1.0);
        let mut m = g.mask();
        m[g.at([5.5, 5.5]).unwrap()] = true;
        let d = g.distance(&m);
        assert!((d[g.at([8.5, 9.5]).unwrap()] - 5.0).abs() < 1e-9);
    }

    #[test]
    fn square_contour_and_fill() {
        let g = Grid::new([0.0, 0.0], [20.0, 20.0], 1.0);
        let mut m = g.mask();
        g.fill_polygon(&mut m, &[[4.0, 4.0], [10.0, 4.0], [10.0, 10.0], [4.0, 10.0]]);
        assert_eq!(m.iter().filter(|&&s| s).count(), 36);
        let c = g.contours(&m);
        assert_eq!(c.len(), 1);
        let s = simplify_loop(&c[0], 0.1);
        assert_eq!(s.len(), 4);
        assert!((area2(&s).abs() / 2.0 - 36.0).abs() < 1e-9);
    }

    #[test]
    fn thin_bar_is_a_line() {
        let g = Grid::new([0.0, 0.0], [40.0, 20.0], 1.0);
        let mut m = g.mask();
        g.fill_polygon(&mut m, &[[2.0, 7.0], [38.0, 7.0], [38.0, 13.0], [2.0, 13.0]]);
        let t = g.thin(&m);
        let n = t.iter().filter(|&&s| s).count();
        assert!((25..=36).contains(&n), "{n}");
    }
}
