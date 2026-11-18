//! The game's fit of a campaign border's hex cells to cubic Béziers, and the arc-length rules its border and trade-route draws use
//! (empire.retail.dll: the fit fn 0x10aae0b0 and its callees; docs/formats/campaign_borders.md).
//!
//! The game computes in single precision (x87 and SSE floats); this port keeps every value an `f32` stored in an `f64` and rounds where
//! the game stores a float ([`f`]), so its results match the game's to the last bits (capture A's 950 land curves within 5e-7, the
//! difference the x87's extended-precision intermediates).
//!
//! The frame: the hex grid's world, x = column · `colw`, z = row · `rowh` (+ `rowh` / 2 on odd columns), `rowh` = the world's height /
//! (rows + 0.5), R = `rowh` / √3 the hexes' radius, `colw` = 1.5 R (flat-top hexes, odd columns half a row up).

/// A cell (column, row); signed, a neighbour of an edge cell may be off the grid.
pub type Cell = (i32, i32);

/// A point (x, z) in the world.
pub type Pt = (f64, f64);

/// A cubic Bézier's control points.
pub type Cubic = [Pt; 4];

/// Rounds to single precision (where the game stores a float).
#[inline]
pub(crate) fn f(x: f64) -> f64 {
    x as f32 as f64
}

/// The hex grid (fn 0x10950410: a cell's centre).
#[derive(Clone, Copy, Debug)]
pub struct Grid {
    pub cols: i32,
    pub rows: i32,
    pub colw: f64,
    /// The hexes' radius.
    pub radius: f64,
    pub rowh: f64,
    half: f64,
    /// The trade routes' point scales (the grid's +0x64, +0x68): width / (1.5 (columns - 1)), height / (2 rows + 1).
    pub trade_x: f64,
    pub trade_z: f64,
}

impl Grid {
    /// A grid of `cols` x `rows` cells over a world `world_w` x `world_h` (`main_attila_map`: 1016 x 720, 678.5 x 556.14392; the
    /// world's size `REGIONS_DATA`'s second `Coord2d`). The cells come from the height: `rowh` = height / (rows + 0.5), R = `rowh` / √3,
    /// `colw` = 1.5 R (single precision; the width's 678.5 / 1015 is 1 to 2 ulps off).
    #[must_use]
    pub fn new(cols: u32, rows: u32, world_w: f64, world_h: f64) -> Self {
        let rowh = f(world_h / (f64::from(rows) + 0.5));
        let radius = f(rowh / f(3f64.sqrt()));
        let colw = f(radius * f(1.5));
        let trade_x = f(world_w / (1.5 * f64::from(cols.saturating_sub(1).max(1))));
        let trade_z = f(world_h / (2.0 * f64::from(rows) + 1.0));
        Self { cols: cols as i32, rows: rows as i32, colw, radius, rowh, half: f(rowh * 0.5), trade_x, trade_z }
    }

    /// A cell's centre (the column and row clamped to the grid's last).
    #[must_use]
    pub fn centre(&self, (c, r): Cell) -> Pt {
        let (c, r) = (c.min(self.cols - 1), r.min(self.rows - 1));
        let mut z = f(f64::from(r) * self.rowh);
        if c & 1 != 0 {
            z = f(z + self.half);
        }
        (f(f64::from(c) * self.colw), z)
    }
}

/// A cell's neighbours (fn 0x10b760a0): index 0 the next row, then clockwise.
#[must_use]
pub fn neighbours((c, r): Cell) -> [Cell; 6] {
    let q = r + (c & 1);
    [(c, r + 1), (c + 1, q), (c + 1, q - 1), (c, r - 1), (c - 1, q - 1), (c - 1, q)]
}

fn direction(a: Cell, b: Cell) -> Result<usize, String> {
    neighbours(a).iter().position(|&n| n == b).ok_or_else(|| format!("cells {a:?} and {b:?} are not neighbours"))
}

/// The hex distance of two cells (fn 0x10b9d4c0).
#[must_use]
pub fn hexdist(a: Cell, b: Cell) -> i32 {
    let dx = a.0 - b.0;
    let dy = ((a.0 + 1) >> 1) - ((b.0 + 1) >> 1) - b.1 + a.1;
    dx.abs().max(dy.abs()).max((dy - dx).abs())
}

/// The angles of a hex's vertices (the game's table).
const ANG: [f64; 6] = [0.5235984, 1.570795, 2.6179917, 3.6651886, 4.712385, 5.759582];

fn vtx_table(r: f64) -> [Pt; 6] {
    ANG.map(|a| (f(f(f(a).sin()) * r), f(f(f(a).cos()) * r)))
}

fn add(p: Pt, q: Pt) -> Pt {
    (f(p.0 + q.0), f(p.1 + q.1))
}

fn sub(p: Pt, q: Pt) -> Pt {
    (f(p.0 - q.0), f(p.1 - q.1))
}

/// Where a funnel point came from: the step (the chain position `i`, its cells, the direction `d` into it) and the point's index.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
struct Pid {
    idx: usize,
    d: usize,
    i: usize,
}

#[derive(Clone, Copy, Debug)]
struct Elem {
    p0: Pt,
    p3: Pt,
    h1: Pt,
    h2: Pt,
    pid: Pid,
    side: u8,
}

impl Elem {
    fn new(p0: Pt, pid: Pid, side: u8) -> Self {
        Self { p0, p3: (0.0, 0.0), h1: (0.0, 0.0), h2: (0.0, 0.0), pid, side }
    }

    fn dir(&self) -> Pt {
        sub(self.p3, self.p0)
    }
}

struct Iter<'a> {
    cells: &'a [Cell],
    i: usize,
    d: usize,
    cur: Cell,
    nxt: Cell,
}

impl<'a> Iter<'a> {
    fn new(cells: &'a [Cell], i: usize, d: usize) -> Self {
        Self { cells, i, d, cur: cells[i], nxt: cells[(i + 1) % cells.len()] }
    }

    fn rewind(&mut self, pid: Pid) {
        self.i = pid.i;
        self.d = pid.d;
        self.cur = self.cells[self.i];
        self.nxt = self.cells[(self.i + 1) % self.cells.len()];
    }

    fn advance(&mut self, e: usize) {
        self.d = e;
        self.i = (self.i + 1) % self.cells.len();
        self.cur = self.cells[self.i];
        self.nxt = self.cells[(self.i + 1) % self.cells.len()];
    }

    fn pid(&self, idx: usize) -> Pid {
        Pid { idx, d: self.d, i: self.i }
    }
}

/// The funnel (fns 0x10c01890 init, 0x10c355d0 a side A point, 0x10c4cf50 a side B point), rewinding to a new apex's portal.
struct Funnel {
    apex: Pt,
    a: Pt,
    b: Pt,
    a_id: Pid,
    b_id: Pid,
    restart: bool,
    rid: Pid,
    done: bool,
    elems: Vec<Elem>,
}

impl Funnel {
    fn new(start: Pt, it: &Iter) -> Self {
        let rid = it.pid(0);
        Self {
            apex: start,
            a: start,
            b: start,
            a_id: Pid::default(),
            b_id: Pid::default(),
            restart: true,
            rid,
            done: false,
            elems: vec![Elem::new(start, rid, 0)],
        }
    }

    /// The game's two products: (p.x - o.x) * (q.z - o.z) and (p.z - o.z) * (q.x - o.x).
    fn x(o: Pt, p: Pt, q: Pt) -> (f64, f64) {
        (f(f(p.0 - o.0) * f(q.1 - o.1)), f(f(p.1 - o.1) * f(q.0 - o.0)))
    }

    fn point_a(&mut self, p: Pt, idx: usize, it: &mut Iter) -> bool {
        if self.done {
            return false;
        }

        let cur = it.pid(idx);
        if self.restart {
            if (self.rid.i, self.rid.idx) != (cur.i, cur.idx) {
                return false;
            }

            (self.a, self.a_id, self.b, self.b_id) = (p, cur, self.apex, self.rid);
            self.restart = false;
        }

        let (l, r) = Self::x(self.apex, self.b, p);
        if l < r {
            if self.b == self.elems[0].p0 {
                self.done = true;
                return false;
            }

            self.elems.push(Elem::new(self.b, self.b_id, 0));
            self.apex = self.b;
            it.rewind(self.b_id);
            (self.restart, self.rid) = (true, self.b_id);
            return true;
        }

        let (l, r) = Self::x(self.apex, self.a, p);
        if l <= r {
            (self.a, self.a_id) = (p, cur);
        }

        false
    }

    fn point_b(&mut self, p: Pt, idx: usize, it: &mut Iter) -> bool {
        if self.done {
            return false;
        }

        let cur = it.pid(idx);
        if self.restart {
            if (self.rid.i, self.rid.idx) != (cur.i, cur.idx) {
                return false;
            }

            (self.a, self.a_id, self.b, self.b_id) = (self.apex, self.rid, p, cur);
            self.restart = false;
        }

        let (l, r) = Self::x(self.apex, self.a, p);
        if r < l {
            if self.a == self.elems[0].p0 {
                self.done = true;
                return false;
            }

            self.elems.push(Elem::new(self.a, self.a_id, 2));
            self.apex = self.a;
            it.rewind(self.a_id);
            (self.restart, self.rid) = (true, self.a_id);
            return true;
        }

        let (l, r) = Self::x(self.apex, self.b, p);
        if r <= l {
            (self.b, self.b_id) = (p, cur);
        }

        false
    }
}

/// The portal points of one step (cur -> nxt, after direction d), side A (`true`) or B, in the game's order (fn 0x10ab3e40); and the
/// step's direction.
fn portals(g: &Grid, it: &Iter, s: &[Pt; 6]) -> Result<(usize, Vec<(bool, Pt)>), String> {
    let nb = neighbours(it.cur);
    let d = it.d;
    let e = direction(it.cur, it.nxt)?;
    let c = g.centre(it.cur);
    let n = g.centre(it.nxt);
    let cen = |k: usize| g.centre(nb[k % 6]);
    let s = |k: usize| s[k % 6];
    const A: bool = true;
    const B: bool = false;
    let pts = if e == d {
        let l = cen(d + 5);
        vec![
            (A, add(l, s(d + 2))),
            (B, add(c, s(d + 5))),
            (A, add(l, s(d + 1))),
            (B, add(n, s(d + 3))),
            (A, add(l, s(d))),
            (B, add(n, s(d + 4))),
        ]
    } else if (e + 1) % 6 == d {
        let l2 = cen(d + 4);
        vec![(B, add(n, s(d + 2))), (A, add(l2, s(d + 5))), (B, add(n, s(d + 3)))]
    } else {
        let l = cen(d + 5);
        let fw = cen(d);
        let mut pts = vec![(A, add(l, s(d + 2))), (A, add(l, s(d + 1))), (B, add(c, s(d + 5))), (A, add(fw, s(d + 3)))];
        if e == (d + 1) % 6 {
            pts.extend([(B, add(c, s(d))), (A, add(fw, s(d + 2))), (B, add(n, s(d + 4))), (A, add(fw, s(d + 1))), (B, add(n, s(d + 5)))]);
        } else if e == (d + 2) % 6 {
            let r1 = cen(d + 1);
            pts.extend([
                (A, add(fw, s(d + 2))),
                (B, add(c, s(d))),
                (A, add(r1, s(d + 4))),
                (B, add(c, s(d + 1))),
                (A, add(r1, s(d + 3))),
                (B, add(n, s(d + 5))),
                (A, add(r1, s(d + 2))),
                (B, add(n, s(d))),
            ]);
        } else {
            let r1 = cen(d + 1);
            let r2 = cen(d + 2);
            pts.extend([
                (A, add(fw, s(d + 2))),
                (B, add(c, s(d))),
                (A, add(r1, s(d + 4))),
                (A, add(r1, s(d + 3))),
                (B, add(c, s(d + 1))),
                (A, add(r2, s(d + 5))),
                (B, add(c, s(d + 2))),
                (A, add(r2, s(d + 4))),
                (B, add(n, s(d))),
                (A, add(r2, s(d + 3))),
                (B, add(n, s(d + 1))),
            ]);
        }
        pts
    };
    Ok((e, pts))
}

/// The radius of the small hexes the portal points sit on.
const SMALL: f64 = 0.15;

fn funnel_path(g: &Grid, cells: &[Cell]) -> Result<Funnel, String> {
    let s = vtx_table(f(SMALL));
    let big = vtx_table(g.radius);
    let k = direction(cells[0], cells[1])?;
    let start = add(g.centre(cells[0]), big[(k + 4) % 6]);
    let mut it = Iter::new(cells, 0, k);
    let mut fun = Funnel::new(start, &it);
    let last = cells[cells.len() - 1];
    for _ in 0..200_000 {
        let (e, pts) = portals(g, &it, &s)?;
        let mut emitted = false;
        for (idx, &(side, p)) in pts.iter().enumerate() {
            if if side { fun.point_a(p, idx, &mut it) } else { fun.point_b(p, idx, &mut it) } {
                emitted = true;
                break;
            }
        }

        if emitted {
            continue;
        }

        if it.nxt == last {
            return Ok(fun);
        }

        it.advance(e);
    }

    Err("the funnel's walk did not end".to_owned())
}

const FLT_MAX: f64 = f32::MAX as f64;
const TWO_PI_OVER_65536: f64 = 9.58738019107841e-05;
const INV: f64 = 10430.3779296875;
const MERGE_EPS: f64 = 0.06283179670572281;

fn norm(v: Pt) -> Pt {
    let k = f(1.0 / f(f(f(v.1 * v.1) + f(v.0 * v.0)).sqrt()));
    (f(v.0 * k), f(v.1 * k))
}

/// An angle quantised to 16 bits (x87 rounding: half to even).
fn qangle(v: Pt) -> i64 {
    (f(f(v.0.atan2(v.1)) * f(INV))).round_ties_even() as i64
}

fn midpoint(e: &Elem) -> Pt {
    (f(f(e.p0.0 + e.p3.0) * 0.5), f(f(e.p3.1 + e.p0.1) * 0.5))
}

fn perp(v: Pt) -> Pt {
    (-v.1, v.0)
}

/// The post-processing (fn 0x10c37260): the end point, merged near-collinear neighbours, the tangents, the short segments.
fn postprocess(g: &Grid, cells: &[Cell], fun: Funnel) -> Result<Vec<Elem>, String> {
    let big = vtx_table(g.radius);
    let n = cells.len();
    let j = direction(cells[n - 2], cells[n - 1])?;
    let end = add(g.centre(cells[n - 1]), big[(j + 5) % 6]);
    let mut e = fun.elems;
    for k in 0..e.len() - 1 {
        e[k].p3 = e[k + 1].p0;
    }
    let last = e.len() - 1;
    e[last].p3 = end;
    for i in 1..e.len() {
        let a = qangle(e[i - 1].dir());
        let b = qangle(e[i].dir());
        let diff = ((((b - a + 32768) & 0xffff) - 32768) as f64).abs();
        if f(diff * f(TWO_PI_OVER_65536)) < f(MERGE_EPS) {
            e[i].p0 = e[i - 1].p0;
            e[i].pid = e[i - 1].pid;
            e[i - 1].p3 = e[i - 1].p0;
        }
    }
    e.retain(|x| x.p0 != x.p3);
    if e.is_empty() {
        return Err("no segment left".to_owned());
    }

    for i in 0..e.len() - 1 {
        let d0 = norm(e[i].dir());
        let d1 = norm(e[i + 1].dir());
        e[i + 1].h1 = norm((f(d1.0 + d0.0), f(d0.1 + d1.1)));
    }
    e[0].h1 = norm(e[0].dir());
    let last = e.len() - 1;
    let dl = norm(e[last].dir());
    e[last].h2 = (-dl.0, -dl.1);
    Ok(short_segments(e, f(SMALL)))
}

/// Runs of short segments replaced by a point (squared lengths up to (1.05 r)²); a leading or trailing run is kept (the game's
/// closed-chain case is not traced).
fn short_segments(mut e: Vec<Elem>, r: f64) -> Vec<Elem> {
    let thr = f(r * f(1.0499999523162842));
    let thr = f(thr * thr);
    let sq = |x: &Elem| {
        let d = x.dir();
        f(f(d.0 * d.0) + f(d.1 * d.1))
    };
    let mut i = 0;
    while i < e.len() {
        if sq(&e[i]) > thr {
            i += 1;
            continue;
        }

        let mut j = i;
        while j < e.len() && sq(&e[j]) <= thr {
            j += 1;
        }

        if j >= e.len() || i == 0 {
            i = j;
            continue;
        }

        let k_of = |s: &Elem| if s.side == 0 { f(0.2) } else { f(-0.2) };
        match j - i {
            1 => {
                let s = e[i];
                let k = k_of(&s);
                let pn = norm(perp(s.dir()));
                let m = midpoint(&s);
                let p = (f(f(f(f(pn.0 * r) * k)) + m.0), f(m.1 + f(f(pn.1 * r) * k)));
                e[i - 1].p3 = p;
                e[j].p0 = p;
                e[j].h1 = s.dir();
                e.remove(i);
            }
            2 => {
                let (s1, s2) = (e[i], e[i + 1]);
                let p = s1.p3;
                e[i - 1].p3 = p;
                e[j].p0 = p;
                let a = norm(s1.dir());
                let b = norm(s2.dir());
                e[j].h1 = (f(b.0 + a.0), f(b.1 + a.1));
                e.drain(i..i + 2);
            }
            3 => {
                let (s1, s2) = (e[i], e[i + 1]);
                let k = k_of(&s1);
                let pn = norm(perp(s2.dir()));
                let m = midpoint(&s2);
                let p = (f(m.0 + f(f(pn.0 * r) * k)), f(m.1 + f(f(pn.1 * r) * k)));
                e[i - 1].p3 = p;
                e[j].p0 = p;
                e[j].h1 = s2.dir();
                e.drain(i..i + 3);
            }
            _ => return e,
        }
    }

    e
}

/// The handles' constraints from a portal point (fn 0x10c89f20; side A, or B with the signs turned).
fn constraint(side_a: bool, t0: Pt, t1: Pt, e: &Elem, u: Pt, l: f64, p: Pt, c: &mut [f64; 2]) {
    let w = sub(p, e.p0);
    // x87: the products and the sum at extended precision, stored as a float.
    let proj = f((w.1 * u.1 + w.0 * u.0) / l);
    if proj <= 0.0 {
        return;
    }

    if proj > 0.5 {
        if proj < 1.0 {
            let v = sub(p, e.p3);
            let s = f(f(t1.1 * v.0) - f(t1.0 * v.1));
            if s != 0.0 && (if side_a { s < 0.0 } else { s > 0.0 }) {
                c[1] = c[1].min(f(f(u.1 * v.0) - f(u.0 * v.1)).abs());
            }
        }
    } else {
        let s = f(f(t0.1 * w.0) - f(w.1 * t0.0));
        if s != 0.0 && (if side_a { s > 0.0 } else { s < 0.0 }) {
            c[0] = c[0].min(f(f(u.1 * w.0) - f(u.0 * w.1)).abs());
        }
    }
}

fn handle_len(c: f64, s_raw: f64, dotv: f64, l: f64, sign: f64) -> f64 {
    let mut ln;
    if c == FLT_MAX {
        ln = f(f(f(dotv * l)) * f(0.4 * sign));
    } else {
        let s = s_raw.abs();
        ln = c;
        if s > f(0.01) {
            ln = f(c / s);
        }
        let lim = f(l * f(0.4));
        if lim < ln {
            ln = lim;
        }
    }

    if ln <= f(0.15) { f(0.15) } else { ln }
}

/// The handles (fns 0x10c361c0, 0x10bead10): each segment's handle lengths limited by the portal points between its apexes.
fn handles(g: &Grid, cells: &[Cell], mut e: Vec<Elem>) -> Result<Vec<Elem>, String> {
    let table = vtx_table(f(SMALL));
    for i in 0..e.len().saturating_sub(1) {
        let (cur, nxt) = (e[i], e[i + 1]);
        let t0 = norm(cur.h1);
        let tn = norm(nxt.h1);
        let t1 = (-tn.0, -tn.1);
        let dv = cur.dir();
        let l = f(f(f(dv.0 * dv.0) + f(dv.1 * dv.1)).sqrt());
        let u = norm(dv);
        let mut c = [FLT_MAX, FLT_MAX];
        let mut it = Iter::new(cells, cur.pid.i, cur.pid.d);
        let stop = nxt.pid.i;
        for _ in 0..cells.len() + 5 {
            let (ee, pts) = portals(g, &it, &table)?;
            for (side, p) in pts {
                constraint(side, t0, t1, &cur, u, l, p, &mut c);
            }
            if it.i == stop {
                break;
            }
            it.advance(ee);
        }

        let la = handle_len(c[0], f(f(u.1 * t0.0) - f(u.0 * t0.1)), f(f(u.0 * t0.0) + f(u.1 * t0.1)), l, 1.0);
        let lb = handle_len(c[1], f(f(u.1 * t1.0) - f(u.0 * t1.1)), f(f(u.0 * t1.0) + f(u.1 * t1.1)), l, -1.0);
        e[i].h1 = (f(t0.0 * la), f(t0.1 * la));
        e[i].h2 = (f(t1.0 * lb), f(t1.1 * lb));
    }

    Ok(e)
}

/// A chain of cells (each the previous one's neighbour, at least two) as the game's cubic Béziers.
///
/// # Errors
///
/// Two consecutive cells are not neighbours, or the walk does not end.
pub fn fit(g: &Grid, cells: &[Cell]) -> Result<Vec<Cubic>, String> {
    if cells.len() < 2 {
        return Err("a chain of fewer than two cells".to_owned());
    }

    let fun = funnel_path(g, cells)?;
    let e = postprocess(g, cells, fun)?;
    let e = handles(g, cells, e)?;
    Ok(e.iter().map(|x| [x.p0, add(x.p0, x.h1), add(x.p3, x.h2), x.p3]).collect())
}

/// The cubic basis rows per control point (the vertex shader's `g_bezier_basis`).
const BASIS: [[f64; 4]; 4] = [[-1.0, 3.0, -3.0, 1.0], [3.0, -6.0, 3.0, 0.0], [-3.0, 3.0, 0.0, 0.0], [1.0, 0.0, 0.0, 0.0]];
const W0: [f64; 4] = [-3.0, 3.0, 0.0, 0.0];
const W1: [f64; 4] = [6.0, -12.0, 6.0, 0.0];
const W2: [f64; 4] = [-3.0, 9.0, -9.0, 3.0];

/// The cumulative speed |B'(i/n)|, i = 0..n-1 (fn 0x1082e8f0).
fn speed_lut(cp: &Cubic, n: usize) -> Vec<f64> {
    let mut lut = Vec::with_capacity(n);
    let mut acc = 0.0;
    let inv = f(1.0 / f(n as f64));
    for i in 0..n {
        let t = f(f(i as f64) * inv);
        let a = f(f(t * 3.0) * t);
        let b = f(t + t);
        let w: [f64; 4] = std::array::from_fn(|k| f(f(f(a * BASIS[k][0]) + f(b * BASIS[k][1])) + BASIS[k][2]));
        let dx = f((0..4).fold(0.0, |s, k| s + f(w[k] * cp[k].0)));
        let dz = f((0..4).fold(0.0, |s, k| s + f(w[k] * cp[k].1)));
        acc = f(acc + f(f(f(dx * dx) + f(dz * dz)).sqrt()));
        lut.push(acc);
    }

    lut
}

/// The parameter at a fraction `s` of the table's length (fn 0x10848d00).
fn lut_inverse(lut: &[f64], s: f64) -> f64 {
    let n = lut.len();
    let fl = f(lut[n - 1] * s);
    let j = lut.partition_point(|&v| v < fl);
    if j == 0 {
        return f(fl / f(f(n as f64) * lut[0]));
    }

    if j == n {
        return s;
    }

    f(f(f(f(fl - lut[j - 1]) / f(lut[j] - lut[j - 1])) + f(j as f64)) / f(n as f64))
}

/// A cubic's length over [t0, t1]: a left Riemann sum of |B'(t)|, 1000 steps (fn 0x1082a900).
#[must_use]
pub fn curve_length(cp: &Cubic, t0: f64, t1: f64) -> f64 {
    let steps = 1000;
    let dt = f(f(t1 - t0) / f(f64::from(steps)));
    let mut t = f(t0);
    let mut acc = 0.0;
    for _ in 0..steps {
        let t2 = f(t * t);
        let w: [f64; 4] = std::array::from_fn(|k| f(f(f(t2 * W2[k]) + f(W1[k] * t)) + W0[k]));
        let dx = f(f(f(w[0] * cp[0].0) + f(w[1] * cp[1].0)) + f(f(w[2] * cp[2].0) + f(w[3] * cp[3].0)));
        let dz = f(f(f(w[0] * cp[0].1) + f(w[1] * cp[1].1)) + f(f(w[2] * cp[2].1) + f(w[3] * cp[3].1)));
        acc = f(acc + f(f(f(dz * dz) + f(dx * dx)).sqrt()));
        t = f(t + dt);
    }

    f(acc * dt)
}

/// The parameters of a curve's 16 vertices: equal steps of its arc length (from a 100-entry speed table); a chain's last curve adds a
/// 17th vertex at t = 1.
#[must_use]
pub fn vertex_ts(cp: &Cubic) -> [f32; 16] {
    let lut = speed_lut(cp, 100);
    std::array::from_fn(|i| lut_inverse(&lut, f(f(i as f64) * f(0.0625))) as f32)
}

/// A chain's curves as drawn (fns 0x10ab2ed0, 0x10ac9bb0): zero-length curves dropped, a handle on its end point replaced by the
/// midpoint (such a curve's length its chord), each curve's uv offset and scale (its share of the chain's length) and vertex
/// parameters; and the chain's length.
#[must_use]
pub fn chain_curves(curves: &[Cubic]) -> (Vec<(Cubic, f32, f32, [f32; 16])>, f64) {
    let mid = |a: Pt, b: Pt| (f(f(a.0 + b.0) * 0.5), f(f(a.1 + b.1) * 0.5));
    let mut out = vec![];
    for &[p0, p1, p2, p3] in curves {
        if p0 == p3 {
            continue;
        }

        let (cp, deg) = if p0 == p1 && p2 != p3 {
            ([p0, mid(p2, p0), p2, p3], true)
        } else if p0 == p1 && p2 == p3 {
            let m = mid(p0, p3);
            ([p0, m, m, p3], true)
        } else if p0 != p1 && p2 == p3 {
            ([p0, p1, mid(p1, p3), p3], true)
        } else {
            ([p0, p1, p2, p3], false)
        };
        let l = if deg { f(((p0.1 - p3.1).powi(2) + (p0.0 - p3.0).powi(2)).sqrt()) } else { curve_length(&cp, 0.0, 1.0) };
        out.push((cp, l));
    }

    let total = out.iter().fold(0.0, |s, c| f(s + c.1));
    let mut acc = 0.0;
    let res = out
        .into_iter()
        .map(|(cp, l)| {
            let r = (cp, f(acc / total) as f32, f(l / total) as f32, vertex_ts(&cp));
            acc = f(acc + l);
            r
        })
        .collect();
    (res, total)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn neighbours_round_trip_and_distance() {
        for c in [(10, 10), (11, 10)] {
            for (k, n) in neighbours(c).iter().enumerate() {
                assert_eq!(hexdist(c, *n), 1);
                assert_eq!(neighbours(*n)[(k + 3) % 6], c);
            }
        }
    }

    #[test]
    fn fits_a_short_chain() {
        let g = Grid::new(1016, 720, 678.5, 556.1439208984375);
        assert_eq!((g.rowh as f32, g.radius as f32, g.colw as f32), (0.77188605, 0.44564864, 0.66847295));
        let mut cells = vec![(400, 390)];
        for d in [1, 1, 0, 1, 2, 1, 0, 0, 5] {
            let last = cells[cells.len() - 1];
            cells.push(neighbours(last)[d]);
        }
        let curves = fit(&g, &cells).expect("fit");
        assert!(!curves.is_empty());
        for w in curves.windows(2) {
            assert_eq!(w[0][3], w[1][0]);
        }
        let (drawn, total) = chain_curves(&curves);
        assert!(total > 0.0);
        let uv: f32 = drawn.iter().map(|c| c.2).sum();
        assert!((uv - 1.0).abs() < 1e-5);
        assert!(drawn.iter().all(|c| c.3[0] == 0.0 && c.3.windows(2).all(|w| w[0] <= w[1])));
    }
}
