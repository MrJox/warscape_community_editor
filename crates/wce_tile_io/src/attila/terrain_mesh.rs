//! Stage B: BOB's terrain mesh of a battle tile (`TOOLDATABUILDER::TILE_RAW_DATA::build_base_mesh`, ToolDataBuilderDll `FUN_10019750`):
//! a vertex per sample of the final heights, a grid of triangles over them, decimated by `TRIANGLE_MERGER` (`FUN_10003140`). The mesh
//! is `mesh.rigid_model_v2`'s surface, and rasterised it is `hf_height_map.data`. docs/formats/tile_compile.md, "Terrain mesh".

use super::bob_tile::calculate_normal;

/// A vertex flag (`build_base_mesh`'s per vertex and LOD byte): the merger never moves `LOCKED` vertices (the tile's border, every
/// 16th sample inside, the protected samples) and moves `FREE` ones anywhere; `EDGE` ones (the fringe's top row) only along the edge.
pub const LOCKED: u8 = 0;
pub const FREE: u8 = 2;
pub const EDGE: u8 = 3;
/// The fringe's bottom row (`build_base_mesh`'s flags for the vertices `FUN_10017120` adds): moves only into another of its row or a
/// `FRINGE_KEPT` one; `FRINGE_KEPT` at the corners and every 16th sample.
pub const FRINGE: u8 = 4;
pub const FRINGE_KEPT: u8 = 1;

/// A vertex of the mesh: its position (x, the height, z; metres), normal, the final alpha at it (the RMV2's w) and whether it is the
/// fringe's bottom (10 m below the edge; BOB's vertex +0x78).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Vertex {
    pub x: f32,
    pub h: f32,
    pub z: f32,
    pub normal: [f32; 3],
    pub alpha: f32,
    pub fringe: bool,
}

/// The vertices of a tile's mesh, one per sample of the final heights (`w` x `h`, rows as `final_heights.dds`: row 0 is the far
/// edge), row by row from z = 0: vertex `z * w + x` stands at (x, z) with the height of the field's row `h - 1 - z`, its normal
/// [`calculate_normal`] there, and the alpha (`final_alpha.dds`, the [`super::bob_tile::influence`]) of row z: BOB reads that
/// field unflipped (the influence's rows differ at the middle of a cell: 1 against 0.965 at row 64 of a 2 x 2 tile).
pub fn grid_vertices(heights: &[f32], alpha: &[f32], w: usize, h: usize, strength: f32, density: usize) -> Vec<Vertex> {
    wce_foundation::wce_prof!("stage_b.grid_vertices");
    // Metres per sample (`128 / density`, f32): x and z are in metres.
    let metres = 128.0f32 / density as f32;
    let mut out = Vec::with_capacity(w * h);
    for z in 0..h {
        let row = h - 1 - z;
        let mz = z as f32 * metres;
        for x in 0..w {
            let normal = calculate_normal(heights, w, h, x, row, strength);
            out.push(Vertex { x: x as f32 * metres, h: heights[row * w + x], z: mz, normal, alpha: alpha[z * w + x], fringe: false });
        }
    }

    out
}

/// The fringe (`FUN_10017120`, a tile without a mask): per edge (z = 0, z = h - 1, x = 0, x = w - 1) a copy of its vertices 10 m lower
/// appended to `verts`, then per square of the edge (`step` samples) two upright triangles appended to `tris`, top t and bottom b:
/// z = 0 (t i, t i+s, b i) (t i+s, b i+s, b i); z = h - 1 (t i+s, t i, b i) (t i+s, b i, b i+s); x = 0 (t j, b j, t j+s)
/// (t j+s, b j, b j+s); x = w - 1 (t j, t j+s, b j) (t j+s, b j+s, b j). The new vertices' flags (`build_base_mesh`) go to `flags`:
/// [`FRINGE_KEPT`] at the tile's corners and where x and z are both multiples of the kind's modulus, else [`FRINGE`].
pub fn add_fringe(verts: &mut Vec<Vertex>, flags: &mut Vec<u8>, tris: &mut Vec<[u32; 3]>, w: usize, h: usize, kind: MeshKind) {
    wce_foundation::wce_prof!("stage_b.add_fringe");
    let step = kind.step;
    let mut edge = |tops: Vec<usize>, tri: &dyn Fn(u32, u32, u32, u32) -> [[u32; 3]; 2]| {
        let base = verts.len();
        for &t in &tops {
            let mut v = verts[t];
            v.h -= 10.0;
            v.fringe = true;
            verts.push(v);
            let (x, z) = (t % w, t / w);
            let corner = (x == 0 || x == w - 1) && (z == 0 || z == h - 1);
            flags.push(if corner || (x % kind.modulus == 0 && z % kind.modulus == 0) { FRINGE_KEPT } else { FRINGE });
        }

        let n = tops.len();
        for i in (0..n - step.min(n)).step_by(step) {
            let [a, b] = tri(tops[i] as u32, tops[i + step] as u32, (base + i) as u32, (base + i + step) as u32);
            tris.push(a);
            tris.push(b);
        }
    };
    edge((0..w).collect(), &|t0, t1, b0, b1| [[t0, t1, b0], [t1, b1, b0]]);
    edge((0..w).map(|x| (h - 1) * w + x).collect(), &|t0, t1, b0, b1| [[t1, t0, b0], [t1, b0, b1]]);
    edge((0..h).map(|z| z * w).collect(), &|t0, t1, b0, b1| [[t0, b0, t1], [t1, b0, b1]]);
    edge((0..h).map(|z| z * w + w - 1).collect(), &|t0, t1, b0, b1| [[t0, t1, b0], [t1, b1, b0]]);
}

/// A tile's sample grid as `build_base_mesh` sees it: `w` x `h` samples, `t` samples per subtile (the variation's
/// `raw_data_tri_density`, BOB's tile size), and the entry's mask (subtiles from the bottom row; empty without one).
#[derive(Clone, Debug, PartialEq)]
pub struct Grid {
    pub w: usize,
    pub h: usize,
    pub t: usize,
    pub mask: Vec<bool>,
}

impl Grid {
    pub fn cells(&self) -> (usize, usize) {
        ((self.w - 1) / self.t, (self.h - 1) / self.t)
    }

    pub fn masked(&self) -> bool {
        !self.mask.is_empty()
    }

    /// The entry's width and height in subtiles (the grid's when it has none).
    pub fn cells_of_tile(&self, raw: &wce_tile_model::Tile) -> (usize, usize) {
        let entry = raw.entry.as_ref().map(|e| &e.root);
        let int = |k: &str| entry.and_then(|e| e.get(k)).and_then(|v| v.as_int()).map(|v| v as usize);
        let (cw, ch) = self.cells();
        (int("width").unwrap_or(cw), int("height").unwrap_or(ch))
    }

    /// `TILE::subtile_masked_valid(x, z)` (z up, as the grid's rows).
    pub fn valid(&self, x: i64, z: i64) -> bool {
        let (cw, ch) = self.cells();
        super::bob_tile::subtile_valid(&self.mask, cw, ch, x.clamp(i32::MIN as i64, i32::MAX as i64) as i32, z.clamp(i32::MIN as i64, i32::MAX as i64) as i32)
    }

    /// The grid's step for a kind: the kind's, at most the grid's size and, masked, the subtile's.
    pub fn step(&self, kind: MeshKind) -> usize {
        let s = kind.step.min(self.w).min(self.h);
        if self.masked() { s.min(self.t) } else { s }
    }
}

/// [`grid_flags`] in general (`build_base_mesh`, step 1): a vertex is locked when the subtile of x - s, x + s (rounded down, z's own)
/// or z - s, z + s is not valid, at the tile's corners, where x and z are both multiples of the kind's modulus, and on a masked tile
/// when the subtile of x - s, x + s, z + s or z - s (divided, truncated) is not; and where the protection map says so.
pub fn grid_flags_masked(g: &Grid, kind: MeshKind, protection: Option<(&[f32], usize, usize)>) -> Vec<u8> {
    let (w, h, t) = (g.w as i64, g.h as i64, g.t as i64);
    let s = g.step(kind) as i64;
    let m = kind.modulus as i64;
    let mut out = Vec::with_capacity(g.w * g.h);
    for z in 0..h {
        for x in 0..w {
            let near = !g.valid((x - s).div_euclid(t), z / t)
                || !g.valid((x + s).div_euclid(t), z / t)
                || !g.valid(x / t, (z + s).div_euclid(t))
                || !g.valid(x / t, (z - s).div_euclid(t));
            let modulus = if near { 1 } else { m };
            let corner = (x == 0 || x == w - 1) && (z == 0 || z == h - 1);
            let mut free = !near && !corner && !(x % modulus == 0 && z % modulus == 0);
            if g.masked() && free && !(g.valid((x - s) / t, z / t) && g.valid((x + s) / t, z / t) && g.valid(x / t, (z + s) / t) && g.valid(x / t, (z - s) / t)) {
                free = false;
            }

            if let Some((p, pw, ph)) = protection
                && z >= 1
                && ((z - 1) as usize) < ph
                && (x as usize) < pw
                && p[(z as usize - 1) * pw + x as usize] > 0.5
            {
                free = false;
            }

            out.push(if free { FREE } else { LOCKED });
        }
    }

    out
}

/// [`grid_triangles`] with the mask (`build_base_mesh`, step 2): a square is left out when its corner (x, z)'s subtile (metres / 128,
/// truncated) is not valid and either (x + s, z)'s is not, in the same subtile column, or (x, z + s)'s is not, in the same column
/// (those two clamped to the last subtile).
pub fn grid_triangles_masked(g: &Grid, verts: &[Vertex], step: usize) -> Vec<[u32; 3]> {
    let (w, h) = (g.w, g.h);
    let (cw, ch) = g.cells();
    let mut out = vec![];
    let v = |x: usize, z: usize| (z * w + x) as u32;
    let sub = |i: u32| {
        let p = &verts[i as usize];
        ((p.x * 0.0078125) as i64, (p.z * 0.0078125) as i64)
    };
    for z in (0..h - step).step_by(step) {
        for x in (0..w - step).step_by(step) {
            let (a, b, c, d) = (v(x, z), v(x, z + step), v(x + step, z), v(x + step, z + step));
            if g.masked() {
                let (ax, az) = sub(a);
                let (mut bx, mut bz) = sub(b);
                let (mut cx, mut cz) = sub(c);
                let last = |v: &mut i64, n: usize| {
                    if *v == n as i64 {
                        *v = n as i64 - 1;
                    }
                };
                last(&mut cx, cw);
                last(&mut bx, cw);
                last(&mut cz, ch);
                last(&mut bz, ch);
                if cx < cw as i64 && cz < ch as i64 && bx < cw as i64 && bz < ch as i64 {
                    let (va, vc, vb) = (g.valid(ax, az), g.valid(cx, cz), g.valid(bx, bz));
                    if !va && ((!vc && ax == cx) || (!vb && ax == bx)) {
                        continue;
                    }
                }
            }

            out.push([a, b, c]);
            out.push([b, d, c]);
        }
    }

    out
}

/// A side of a subtile for the masked fringe: the neighbour's subtile, the grid vertices along the side and its triangle order.
type FringeSide = (i64, i64, Vec<usize>, fn(u32, u32, u32, u32) -> [[u32; 3]; 2]);

/// The fringe of a masked tile (`FUN_10017120`): per valid subtile (rows of subtiles from the bottom, x inner), per side whose
/// neighbour is not valid (left, right, below, above), its T + 1 grid vertices along that side each pushed again as a top (the same
/// vertex: BOB pushes the pointer twice; flag 3, moves along the edge) and a copy 10 m lower (flag 0), then per `step` two upright
/// triangles of tops t and bottoms b: left (t j, t i, b i) (t j, b i, b j); right (t i, t j, b i) (b i, t j, b j); below (t j, b i,
/// t i) (t j, b j, b i); above (t j, t i, b i) (b j, t j, b i).
pub fn add_fringe_masked(g: &Grid, verts: &mut Vec<Vertex>, flags: &mut Vec<u8>, tris: &mut Vec<[u32; 3]>, step: usize) {
    let (cw, ch) = g.cells();
    let (w, t) = (g.w, g.t);
    for cz in 0..ch as i64 {
        for cx in 0..cw as i64 {
            if !g.valid(cx, cz) {
                continue;
            }

            let (x0, z0) = (cx as usize * t, cz as usize * t);
            let sides: [FringeSide; 4] = [
                (cx - 1, cz, (z0..=z0 + t).map(|z| z * w + x0).collect(), |ti, tj, bi, bj| [[tj, ti, bi], [tj, bi, bj]]),
                (cx + 1, cz, (z0..=z0 + t).map(|z| z * w + x0 + t).collect(), |ti, tj, bi, bj| [[ti, tj, bi], [bi, tj, bj]]),
                (cx, cz - 1, (x0..=x0 + t).map(|x| z0 * w + x).collect(), |ti, tj, bi, bj| [[tj, bi, ti], [tj, bj, bi]]),
                (cx, cz + 1, (x0..=x0 + t).map(|x| (z0 + t) * w + x).collect(), |ti, tj, bi, bj| [[tj, ti, bi], [bj, tj, bi]]),
            ];
            for (nx, nz, tops, tri) in sides {
                if g.valid(nx, nz) {
                    continue;
                }

                let base = verts.len();
                for &i in &tops {
                    let top = verts[i];
                    let mut bottom = top;
                    bottom.h -= 10.0;
                    bottom.fringe = true;
                    verts.push(top);
                    verts.push(bottom);
                    flags.push(EDGE);
                    flags.push(LOCKED);
                }

                for i in (0..t).step_by(step) {
                    let (ti, tj) = ((base + 2 * i) as u32, (base + 2 * (i + step)) as u32);
                    let [a, b] = tri(ti, tj, ti + 1, tj + 1);
                    tris.push(a);
                    tris.push(b);
                }
            }
        }
    }
}

/// Infield (`mesh.rigid_model_v2`) and outfield (`outfield_mesh.rigid_model_v2`) numbers of `build_base_mesh` for a battle tile:
/// the grid's step, and the modulus of the samples locked inside (and of the fringe's kept bottoms).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MeshKind {
    pub step: usize,
    pub modulus: usize,
}

pub const INFIELD: MeshKind = MeshKind { step: 1, modulus: 16 };
pub const OUTFIELD: MeshKind = MeshKind { step: 16, modulus: 2048 };

/// The vertex flags of a battle tile alone (`build_base_mesh`, step 1): locked near the edge, where the cell of x - s, x + s, z - s
/// or z + s (rounded down) is off the tile (s the grid's step: the border, and x and z >= size - 1 - s, BOB's asymmetry), and where
/// x and z are both multiples of the kind's modulus; the rest free; and locked where `protection_map.dds` (`cells x 128` square,
/// R32F) is above 0.5: vertex (x, z) reads its row z - 1.
pub fn grid_flags(w: usize, h: usize, kind: MeshKind, protection: Option<(&[f32], usize, usize)>) -> Vec<u8> {
    wce_foundation::wce_prof!("stage_b.grid_flags");
    let s = kind.step;
    let mut out = Vec::with_capacity(w * h);
    for z in 0..h {
        for x in 0..w {
            let edge = x < s || z < s || x + 1 + s > w - 1 || z + 1 + s > h - 1;
            let mut flag = if edge || (x % kind.modulus == 0 && z % kind.modulus == 0) { LOCKED } else { FREE };
            if let Some((p, pw, ph)) = protection
                && z >= 1
                && z - 1 < ph
                && x < pw
                && p[(z - 1) * pw + x] > 0.5
            {
                flag = LOCKED;
            }

            out.push(flag);
        }
    }

    out
}

/// The grid's triangles (`build_base_mesh`, step 2): per square of `step` samples, row by row, (x, z) (x, z + s) (x + s, z) and
/// (x, z + s) (x + s, z + s) (x + s, z).
pub fn grid_triangles(w: usize, h: usize, step: usize) -> Vec<[u32; 3]> {
    wce_foundation::wce_prof!("stage_b.grid_triangles");
    let mut out = vec![];
    let v = |x: usize, z: usize| (z * w + x) as u32;
    for z in (0..h - step).step_by(step) {
        for x in (0..w - step).step_by(step) {
            out.push([v(x, z), v(x, z + step), v(x + step, z)]);
            out.push([v(x, z + step), v(x + step, z + step), v(x + step, z)]);
        }
    }

    out
}

/// BOB's `TRIANGLE_MERGER`: removes vertices by collapsing them into a neighbour, in passes of a growing longest edge.
struct Merger<'a> {
    verts: &'a [Vertex],
    flags: &'a [u8],
    tris: Vec<[u32; 3]>,
    adj: Vec<Vec<u32>>,
    cos_angle: f32,
    max_height_step: f32,
    max_edge: f32,
}

impl Merger<'_> {
    fn live(t: &[u32; 3]) -> bool {
        t[0] != t[1] && t[0] != t[2] && t[1] != t[2]
    }

    /// `FUN_10002fe0`: the triangles of the list, and per vertex the triangles using it.
    fn rebuild(&mut self, list: &[[u32; 3]]) {
        self.tris.clear();
        self.tris.extend_from_slice(list);
        for a in &mut self.adj {
            a.clear();
        }

        for (i, t) in list.iter().enumerate() {
            for &v in t {
                self.adj[v as usize].push(i as u32);
            }
        }
    }

    /// `FUN_10003440`: whether `v` may go: every neighbour's normal within the angle of its own (|n . n'| >= cos), and within the
    /// height step.
    fn removable(&self, v: u32) -> bool {
        let flag = self.flags[v as usize];
        if flag == 0 || flag == 1 {
            return false;
        }

        let me = &self.verts[v as usize];
        for &t in &self.adj[v as usize] {
            let t = self.tris[t as usize];
            if !Self::live(&t) {
                continue;
            }

            let others = if t[0] == v {
                [t[1], t[2]]
            } else if t[1] == v {
                [t[0], t[2]]
            } else {
                [t[0], t[1]]
            };
            for o in others {
                let other = &self.verts[o as usize];
                let (a, b) = (other.normal, me.normal);
                let dot = a[1] * b[1] + a[0] * b[0] + a[2] * b[2];
                if dot.abs() < self.cos_angle {
                    return false;
                }

                let of = self.flags[o as usize];
                if flag != 1 && flag != 4 && of != 1 && of != 4 && (other.h - me.h).abs() > self.max_height_step {
                    return false;
                }
            }
        }

        true
    }

    /// `FUN_10002770`: whether triangle (a, b, c) is too thin: the sine of its angle at `a` (in the ground plane) below 0.1, or an edge
    /// at `a` of no length. (Triangles standing upright, the fringe's, are measured in their own plane.)
    fn thin(&self, a: u32, b: u32, c: u32) -> bool {
        if a == b || a == c || b == c {
            return false;
        }

        let (p, q, r) = (&self.verts[a as usize], &self.verts[b as usize], &self.verts[c as usize]);
        let upright_z = p.z == q.z && q.z == r.z;
        let upright_x = p.x == q.x && q.x == r.x;
        let (ab0, ab1, ac0, ac1) = if upright_z || upright_x {
            if upright_z {
                (q.x - p.x, q.h - p.h, r.x - p.x, r.h - p.h)
            } else {
                (q.z - p.z, q.h - p.h, r.z - p.z, r.h - p.h)
            }
        } else {
            (q.x - p.x, q.z - p.z, r.x - p.x, r.z - p.z)
        };
        let ac = ac0 * ac0 + ac1 * ac1;
        let ab = ab0 * ab0 + ab1 * ab1;
        if ac == 0.0 || ab == 0.0 {
            return true;
        }

        let inv_ac = 1.0f32 / ac.sqrt();
        let inv_ab = 1.0f32 / ab.sqrt();
        // The cross product of the two unit edges: only its upright component is not 0, and BOB takes its length.
        let cross = ab1 * inv_ab * inv_ac * ac0 - ac1 * inv_ac * inv_ab * ab0;
        let sine = (cross * cross).sqrt();
        sine < 0.1 || sine.is_nan()
    }

    /// `FUN_10002d60`: whether triangle (a, b, c) has an edge longer than this pass allows or turned over.
    fn too_long_or_flipped(&self, a: u32, b: u32, c: u32) -> bool {
        if a == b || a == c || b == c {
            return false;
        }

        let (p, q, r) = (&self.verts[a as usize], &self.verts[b as usize], &self.verts[c as usize]);
        let l = self.max_edge * self.max_edge;
        let d = |s: &Vertex, t: &Vertex| (t.z - s.z) * (t.z - s.z) + (t.x - s.x) * (t.x - s.x);
        if d(p, q) <= l && d(p, r) <= l && d(q, r) <= l {
            let cross = (r.z - p.z) * (q.x - p.x) - (q.z - p.z) * (r.x - p.x);
            if cross <= 0.0 {
                return false;
            }
        }

        true
    }

    /// `FUN_100039f0`: whether `v` may collapse into `u`: the flags agree, and every triangle of `v` remapped neither collapses onto a
    /// locked vertex nor comes out thin, too long or turned over.
    fn collapsible(&self, v: u32, u: u32) -> bool {
        let fv = self.flags[v as usize];
        if fv == 0 {
            return false;
        }

        let fu = self.flags[u as usize];
        if (fv == 3 && fu != 3 && fu != 0) || (fv == 4 && fu != 4 && fu != 1) {
            return false;
        }

        for &t in &self.adj[v as usize] {
            let t = self.tris[t as usize];
            if !Self::live(&t) {
                continue;
            }

            let [a, b, c] = t.map(|i| if i == v { u } else { i });
            if fu > 1 && !Self::live(&[a, b, c]) && [a, b, c].iter().any(|&i| self.flags[i as usize] < 2) {
                return false;
            }

            if self.thin(a, b, c) || self.too_long_or_flipped(a, b, c) {
                return false;
            }
        }

        true
    }

    /// `FUN_10003cc0`: where `v` goes: of its neighbours it may collapse into (per triangle, in corner order, repeats kept), the first
    /// that is not free, else the nearest (ties in that order).
    fn target(&self, v: u32) -> Option<u32> {
        let mut cands = vec![];
        for &t in &self.adj[v as usize] {
            let t = self.tris[t as usize];
            if !Self::live(&t) {
                continue;
            }

            for u in t {
                if u != v && self.collapsible(v, u) {
                    cands.push(u);
                }
            }
        }

        if cands.is_empty() {
            return None;
        }

        if let Some(&u) = cands.iter().find(|&&u| self.flags[u as usize] != FREE) {
            return Some(u);
        }

        let me = &self.verts[v as usize];
        let dist = |u: u32| {
            let o = &self.verts[u as usize];
            let (dx, dz) = (o.x - me.x, o.z - me.z);
            dx * dx + dz * dz
        };
        // MSVC's insertion sort (fewer than 33 candidates): stable.
        cands.sort_by(|&a, &b| dist(a).partial_cmp(&dist(b)).unwrap_or(std::cmp::Ordering::Equal));
        Some(cands[0])
    }

    /// `FUN_10004090`: collapses `v` into its target, if it may go.
    fn collapse(&mut self, v: u32) {
        if !self.removable(v) {
            return;
        }

        let Some(u) = self.target(v) else { return };
        let moved = std::mem::take(&mut self.adj[v as usize]);
        self.adj[u as usize].extend_from_slice(&moved);
        for i in 0..self.adj[u as usize].len() {
            let t = &mut self.tris[self.adj[u as usize][i] as usize];
            for c in t.iter_mut() {
                if *c == v {
                    *c = u;
                }
            }

            if !Self::live(t) {
                *t = [0, 0, 0];
            }
        }
    }
}

/// `TRIANGLE_MERGER::process` (`FUN_10003140`) with BOB's numbers for a tile's first LOD: `cos_angle` the cosine of the conversion
/// params' `triangle_decimation_angle_factors0` (degrees), edges up to 120 m, height steps up to 3 m. Passes 1 ..= 50 allow edges of
/// pass x 2.4 m; each tries every vertex in order; after 4 passes in a row without a change it skips 10. The triangles left, in order.
pub fn merge(verts: &[Vertex], flags: &[u8], tris: &[[u32; 3]], cos_angle: f32) -> Vec<[u32; 3]> {
    wce_foundation::wce_prof!("stage_b.merge");
    const MAX_EDGE: f32 = 120.0;
    let passes = ((0.166_666_67f32 * MAX_EDGE) as u32).max(50);
    let step = MAX_EDGE / passes as f32;
    let mut m = Merger { verts, flags, tris: vec![], adj: vec![vec![]; verts.len()], cos_angle, max_height_step: 3.0, max_edge: 0.0 };
    let mut list = tris.to_vec();
    let (mut pass, mut length, mut stale) = (0u32, 1u32, 0);
    while pass < passes {
        m.max_edge = length as f32 * step;
        m.rebuild(&list);
        for v in 0..verts.len() as u32 {
            m.collapse(v);
        }

        let before = list.len();
        list = m.tris.iter().copied().filter(Merger::live).collect();
        if before == list.len() {
            stale += 1;
            if stale > 3 {
                pass += 10;
                length += 10;
                stale = 0;
            }
        }

        length += 1;
        pass += 1;
    }

    list
}

/// The angle threshold's cosine as BOB computes it: degrees x BOB's pi (3.1415925, an ulp below f32's) x the f32 of 1 / 180, `fcos`
/// rounded to f32.
pub fn cos_degrees(degrees: f32) -> f32 {
    let rad = degrees * f32::from_bits(0x4049_0fda) * f32::from_bits(0x3bb6_0b61);
    f64::from(rad).cos() as f32
}

/// The project's `protection_map.dds` (TEd's R32F template: a 128-byte header, then the samples), as (samples, width, height).
pub fn protection_map(raw: &wce_tile_model::Tile) -> Option<(Vec<f32>, usize, usize)> {
    wce_foundation::wce_prof!("stage_b.protection_map");
    let f = raw.files.iter().find(|f| f.path.eq_ignore_ascii_case("protection_map.dds"))?;
    let b = f.data.bytes().ok()?;
    let u32_at = |o: usize| b.get(o..o + 4).map(|s| u32::from_le_bytes(s.try_into().expect("4 bytes")) as usize);
    let (h, w) = (u32_at(12)?, u32_at(16)?);
    if u32_at(84)? != 0x72 {
        return None;
    }

    let px = b.get(128..128 + w * h * 4)?.as_chunks::<4>().0.iter().map(|c| f32::from_le_bytes(*c)).collect();
    Some((px, w, h))
}

/// BOB's "Rasterize final heightmap" (ToolDataBuilderDll `FUN_1001bf20`): the mesh's triangles, in order, into a field of `w` x `h`
/// zeros (rows as `final_heights.dds`). Per triangle (a, b, c), every sample of its box in whole metres, widened by one below, gets
/// the height interpolated at it when its barycentric weights are all within [-0.0001, 1.0001]; a later triangle overwrites. In f32,
/// as BOB's SSE code orders it (one sample per metre: the variation's `raw_data_tri_density` is the tile size, 128).
pub fn rasterise(verts: &[Vertex], tris: &[[u32; 3]], w: usize, h: usize, density: usize) -> Vec<f32> {
    wce_foundation::wce_prof!("stage_b.rasterise");
    let mut out = vec![0.0f32; w * h];
    rasterise_into(&mut out, verts, tris, w, h, density);
    out
}

/// [`rasterise`] into a field already filled (the water map's starts at -1000).
pub fn rasterise_into(out: &mut [f32], verts: &[Vertex], tris: &[[u32; 3]], w: usize, h: usize, density: usize) {
    // Metres to samples: 1 / (128 / density), f32 (a quarter for the mountains' 32).
    let per_metre = 1.0f32 / (128.0f32 / density as f32);
    for t in tris {
        let [a, b, c] = t.map(|i| &verts[i as usize]);
        let xs = [a.x as i32, b.x as i32, c.x as i32];
        let zs = [a.z as i32, b.z as i32, c.z as i32];
        let (x0, x1) = (*xs.iter().min().expect("3") - 1, *xs.iter().max().expect("3"));
        let (z0, z1) = (*zs.iter().min().expect("3") - 1, *zs.iter().max().expect("3"));
        let (bx, bz, cx, cz) = (b.x - a.x, b.z - a.z, c.x - a.x, c.z - a.z);
        let d00 = bx * bx + bz * bz;
        let d01 = cx * bx + cz * bz;
        let d11 = cx * cx + cz * cz;
        let det = d11 * d00 - d01 * d01;
        for z in z0..=z1 {
            let pz = z as f32;
            for x in x0..=x1 {
                let (px, qz) = (x as f32 - a.x, pz - a.z);
                let d20 = px * bx + qz * bz;
                let d21 = px * cx + qz * cz;
                let (u, v, wt) = if det != 0.0 {
                    let inv = 1.0f32 / det;
                    let v = (d20 * d11 - d21 * d01) * inv;
                    let wt = (d21 * d00 - d20 * d01) * inv;
                    ((1.0 - v) - wt, v, wt)
                } else {
                    (-1.0, -1.0, -1.0)
                };
                let height = c.h * wt + b.h * v + a.h * u;
                let inside = |k: f32| (-0.0001..=1.000_1).contains(&k);
                if !(inside(u) && inside(v) && inside(wt)) {
                    continue;
                }

                // The sample: the metre scaled and truncated (`fistp` with chop; the row on the x87). Several metres share a sample
                // when density < 128: the last written wins.
                let (fx, fz) = (per_metre * x as f32, per_metre * pz);
                if fx < 0.0 || fx >= w as f32 || fz < 0.0 || fz >= h as f32 {
                    continue;
                }

                let row = ((h as f64 - f64::from(fz)) - 1.0) as usize;
                out[row * w + fx as usize] = height;
            }
        }
    }
}

/// A building's height-map mesh (`FUN_1008a2f0`): the CS2's meshes tagged `height_map_mesh...` (a string attribute, `class_rigidINFO`
/// in the files), each moved by its scene node's transform (the first node of its name: position, rotation, scale), one list of
/// vertices with the triangles renumbered into it.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct HeightMesh {
    pub positions: Vec<[f32; 3]>,
    pub faces: Vec<[u32; 3]>,
}

impl HeightMesh {
    pub fn of(f: &wce_formats::cas2::Cas2File) -> Self {
        let mut out = Self::default();
        for m in &f.meshes {
            if !m.strings.iter().any(|(_, vs)| vs.iter().any(|v| v.starts_with("height_map_mesh"))) {
                continue;
            }

            let t = f.nodes.iter().position(|n| n.name == m.name).map_or(IDENTITY_3X4, |i| node_matrix(f, i));
            let base = out.positions.len() as u32;
            for &[x, y, z] in &m.positions {
                out.positions.push([
                    x * t[0] + t[1] * y + z * t[2] + t[3],
                    y * t[5] + x * t[4] + z * t[6] + t[7],
                    y * t[9] + t[8] * x + z * t[10] + t[11],
                ]);
            }

            out.faces.extend(m.faces.iter().map(|f| f.map(|i| i + base)));
        }

        out
    }
}

const IDENTITY_3X4: [f32; 12] = [1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0];

/// A scene node's 3 x 4 matrix (rows; translation last), its parents' applied. (The files read so far have identity rotations and no
/// scale: BOB's own composition is not checked against a rotated node.)
fn node_matrix(f: &wce_formats::cas2::Cas2File, i: usize) -> [f32; 12] {
    let n = &f.nodes[i];
    let [qx, qy, qz, qw] = n.rotation.unwrap_or([0.0, 0.0, 0.0, 1.0]);
    let [sx, sy, sz] = n.scale.unwrap_or([1.0; 3]);
    let [tx, ty, tz] = n.position.unwrap_or([0.0; 3]);
    let r = [
        [1.0 - 2.0 * (qy * qy + qz * qz), 2.0 * (qx * qy - qz * qw), 2.0 * (qx * qz + qy * qw)],
        [2.0 * (qx * qy + qz * qw), 1.0 - 2.0 * (qx * qx + qz * qz), 2.0 * (qy * qz - qx * qw)],
        [2.0 * (qx * qz - qy * qw), 2.0 * (qy * qz + qx * qw), 1.0 - 2.0 * (qx * qx + qy * qy)],
    ];
    let local = [
        r[0][0] * sx, r[0][1] * sy, r[0][2] * sz, tx,
        r[1][0] * sx, r[1][1] * sy, r[1][2] * sz, ty,
        r[2][0] * sx, r[2][1] * sy, r[2][2] * sz, tz,
    ];
    if n.parent < 0 || n.parent as usize >= f.nodes.len() || n.parent as usize == i {
        return local;
    }

    let p = node_matrix(f, n.parent as usize);
    let mut out = [0.0f32; 12];
    for row in 0..3 {
        for col in 0..4 {
            let mut v = p[row * 4] * local[col] + p[row * 4 + 1] * local[4 + col] + p[row * 4 + 2] * local[8 + col];
            if col == 3 {
                v += p[row * 4 + 3];
            }

            out[row * 4 + col] = v;
        }
    }

    out
}

/// The matrix BOB_Tile stamps a building's height mesh with (`FUN_1000bb10`): TEd's world matrix with its x and z rows halved (TEd's
/// half metres to samples).
pub fn stamp_matrix(world: &[f32; 16]) -> [f32; 16] {
    let mut m = *world;
    for i in (0..4).chain(8..12) {
        m[i] *= 0.5;
    }

    m
}

/// `WARSCAPE::rasterise_max_heights` (Warscape.AssemblyKit.dll 0x1056edb0): each triangle of the mesh, moved by `m` (rows x, y, z),
/// raises the samples of the field (`w` x `h`, row z, column x, not flipped) inside it to its height there. The box of a triangle is
/// its corners' truncated x and z widened by one, clamped to the field less its last row and column; a sample is inside by the
/// crossing rule over the edges (a c), (b a), (c b); its height interpolates by the areas of the sub-triangles. All SSE f32.
pub fn rasterise_max_heights(mesh: &HeightMesh, m: &[f32; 16], field: &mut [f32], w: usize, h: usize) {
    wce_foundation::wce_prof!("stage_b.rasterise_max_heights");
    let tr = |p: &[f32; 3]| {
        let row = |r: usize| p[0] * m[r * 4] + p[1] * m[r * 4 + 1] + p[2] * m[r * 4 + 2] + 1.0 * m[r * 4 + 3];
        [row(0), row(1), row(2)]
    };
    for f in &mesh.faces {
        let [a, b, c] = f.map(|i| tr(&mesh.positions[i as usize]));
        let xs = [a[0] as i32, b[0] as i32, c[0] as i32];
        let zs = [a[2] as i32, b[2] as i32, c[2] as i32];
        let (x0, x1) = (*xs.iter().min().expect("3") - 1, *xs.iter().max().expect("3") + 1);
        let (z0, z1) = (*zs.iter().min().expect("3") - 1, *zs.iter().max().expect("3") + 1);
        let (wl, hl) = (w as i32 - 1, h as i32 - 1);
        if x1 < 0 || wl < x0 || z1 < 0 || hl < z0 {
            continue;
        }

        let (e1x, e1z, e2x, e2z) = (b[0] - a[0], b[2] - a[2], c[0] - a[0], c[2] - a[2]);
        let ny = e1z * e2x - e1x * e2z;
        let area = (ny * ny).sqrt() * 0.5;
        for z in z0.max(0)..z1.min(hl) {
            for x in x0.max(0)..x1.min(wl) {
                if !inside(x as f32, z as f32, &a, &b, &c) {
                    continue;
                }

                let (dx, dz) = (x as f32 - a[0], z as f32 - a[2]);
                let by = dz * e2x - dx * e2z;
                let cy = dz * e1x - dx * e1z;
                let wb = (by * by).sqrt() / 2.0 / area;
                let wc = (cy * cy).sqrt() / 2.0 / area;
                let height = b[1] * wb + c[1] * wc + ((1.0 - wb) - wc) * a[1];
                let s = &mut field[z as usize * w + x as usize];
                if *s <= height {
                    *s = height;
                }
            }
        }
    }
}

/// `FUN_1056eba0`: the crossing rule (a point left of an odd number of the edges it straddles in z).
fn inside(x: f32, z: f32, a: &[f32; 3], b: &[f32; 3], c: &[f32; 3]) -> bool {
    let mut odd = false;
    for (p, q) in [(a, c), (b, a), (c, b)] {
        if (z < p[2]) != (z < q[2]) && x < (q[0] - p[0]) * (z - p[2]) / (q[2] - p[2]) + p[0] {
            odd = !odd;
        }
    }

    odd
}

/// f32 to IEEE half, rounded to nearest even (BOB's vertex positions).
pub fn f16_bits(v: f32) -> u16 {
    let b = v.to_bits();
    let sign = ((b >> 16) & 0x8000) as u16;
    let exp = ((b >> 23) & 0xff) as i32;
    let man = b & 0x7f_ffff;
    if exp == 0xff {
        return sign | 0x7c00 | if man != 0 { 0x200 } else { 0 };
    }

    let e = exp - 127 + 15;
    if e >= 0x1f {
        return sign | 0x7c00;
    }

    if e <= 0 {
        if e < -10 {
            return sign;
        }

        let m = man | 0x80_0000;
        let shift = (14 - e) as u32;
        let half = 1u32 << (shift - 1);
        let rest = m & ((1 << shift) - 1);
        let mut out = m >> shift;
        if rest > half || (rest == half && out & 1 == 1) {
            out += 1;
        }

        return sign | out as u16;
    }

    let rest = man & 0x1fff;
    let mut out = ((e as u32) << 10) | (man >> 13);
    if rest > 0x1000 || (rest == 0x1000 && out & 1 == 1) {
        out += 1;
    }

    sign | out as u16
}

/// `mesh.rigid_model_v2` as BOB writes a tile's mesh (`TERRAIN_MESH_WRITER_V2`, one LOD). The `MESH_SPLITTER` (`FUN_10004970`) cuts
/// the triangle list into blocks: a block takes triangles until it holds 65,000 vertices or more, and the triangle that filled it
/// starts the next block again; a block's vertices are numbered by first use (the vertex list cleaner), each as f16 x, height, z,
/// alpha; its indices u16. The header: RMV2 version 6, 1 LOD, a zero skeleton name; the LOD's block count, vertex and index bytes,
/// its offset (0xa0) and distance (70). Per block: type 0x61, size, header size 0xa8, vertex count, index offset and count, the
/// bounds of the whole mesh's f32 positions, the shader `rigid_default` (BOB leaves the rest of its 32 bytes uninitialised: zeros
/// here), `TerrainBase<k>` (the block's number), 32 zeros, the tile's size in metres x and z, its samples per row, 1, 16, the count
/// of the block's indices of `is_surface` triangles; its vertices, indices.
pub fn rmv2(verts: &[Vertex], tris: &[[u32; 3]], is_surface: impl Fn(&[u32; 3]) -> bool, size: (u32, u32), row: u32, river: &super::river::RiverMesh) -> Vec<u8> {
    wce_foundation::wce_prof!("stage_b.rmv2");
    struct Block {
        order: Vec<u32>,
        indices: Vec<u16>,
        surface: u32,
    }

    let mut blocks = vec![];
    let mut number = vec![u32::MAX; verts.len()];
    let mut block = Block { order: vec![], indices: vec![], surface: 0 };
    let mut k = 0;
    while k < tris.len() {
        let t = &tris[k];
        for &i in t {
            if number[i as usize] == u32::MAX {
                number[i as usize] = block.order.len() as u32;
                block.order.push(i);
            }

            block.indices.push(number[i as usize] as u16);
        }

        if is_surface(t) {
            block.surface += 3;
        }

        // The splitter moves to the next triangle only while the block has room: the triangle that fills a block starts the next
        // one again (BOB's blocks share it).
        if block.order.len() < 65_000 {
            k += 1;
            continue;
        }

        for &i in &block.order {
            number[i as usize] = u32::MAX;
        }

        blocks.push(std::mem::replace(&mut block, Block { order: vec![], indices: vec![], surface: 0 }));
    }

    if !block.indices.is_empty() || blocks.is_empty() {
        blocks.push(block);
    }
    let (mut lo, mut hi) = ([f32::MAX; 3], [f32::MIN; 3]);
    for &i in blocks.iter().flat_map(|b| &b.order) {
        let v = &verts[i as usize];
        for (k, c) in [v.x, v.h, v.z].into_iter().enumerate() {
            lo[k] = lo[k].min(c);
            hi[k] = hi[k].max(c);
        }
    }

    // The river's block (type 90): its vertices numbered by first use, 8 f32 each.
    let mut number = vec![u32::MAX; river.vertices.len()];
    let (mut river_order, mut river_indices) = (vec![], vec![]);
    for &i in river.triangles.iter().flatten() {
        if number[i as usize] == u32::MAX {
            number[i as usize] = river_order.len() as u32;
            river_order.push(i);
        }

        river_indices.push(number[i as usize] as u16);
    }

    let vbytes = blocks.iter().map(|b| b.order.len() as u32 * 8).sum::<u32>() + river_order.len() as u32 * 32;
    let ibytes = blocks.iter().map(|b| b.indices.len() as u32 * 2).sum::<u32>() + river_indices.len() as u32 * 2;
    let mut o = b"RMV2".to_vec();
    let put = |o: &mut Vec<u8>, v: u32| o.extend_from_slice(&v.to_le_bytes());
    put(&mut o, 6);
    put(&mut o, 1);
    o.resize(0x8c, 0);
    let count = blocks.len() as u32 + u32::from(!river_order.is_empty());
    for v in [count, vbytes, ibytes, 0xa0, 70.0f32.to_bits()] {
        put(&mut o, v);
    }

    for (k, b) in blocks.iter().enumerate() {
        let (vb, ib) = (b.order.len() as u32 * 8, b.indices.len() as u32 * 2);
        for v in [0x61, 0xa8 + vb + ib, 0xa8, b.order.len() as u32, 0xa8 + vb, b.indices.len() as u32] {
            put(&mut o, v);
        }

        for v in lo.into_iter().chain(hi) {
            put(&mut o, v.to_bits());
        }

        let name = |o: &mut Vec<u8>, s: &str| {
            let start = o.len();
            o.extend_from_slice(s.as_bytes());
            o.resize(start + 32, 0);
        };
        name(&mut o, "rigid_default");
        name(&mut o, &format!("TerrainBase{k}"));
        o.resize(o.len() + 32, 0);
        for v in [size.0, size.1, row, 1, 16, b.surface] {
            put(&mut o, v);
        }

        for &i in &b.order {
            let v = &verts[i as usize];
            for c in [v.x, v.h, v.z, v.alpha] {
                o.extend_from_slice(&f16_bits(c).to_le_bytes());
            }
        }

        for &i in &b.indices {
            o.extend_from_slice(&i.to_le_bytes());
        }
    }

    if !river_order.is_empty() {
        // `FUN_100228f0`: the bounds of the river's vertices, y at -0.1 and 0.1.
        let (mut lo, mut hi) = ([f32::MAX, -0.1, f32::MAX], [f32::MIN, 0.1, f32::MIN]);
        for v in &river.vertices {
            for k in [0, 2] {
                lo[k] = lo[k].min(v[k]);
                hi[k] = hi[k].max(v[k]);
            }
        }

        let (vb, ib) = (river_order.len() as u32 * 32, river_indices.len() as u32 * 2);
        for v in [0x5a, 0x50 + vb + ib, 0x50, river_order.len() as u32, 0x50 + vb, river_indices.len() as u32] {
            put(&mut o, v);
        }

        for v in lo.into_iter().chain(hi) {
            put(&mut o, v.to_bits());
        }

        let start = o.len();
        o.extend_from_slice(b"rigid_default");
        o.resize(start + 32, 0);
        for &i in &river_order {
            for c in river.vertices[i as usize] {
                put(&mut o, c.to_bits());
            }
        }

        for &i in &river_indices {
            o.extend_from_slice(&i.to_le_bytes());
        }
    }

    o
}

/// The records of a kit table (`raw_data/db/<table>.xml`): per record its fields, in order.
fn kit_table(raw_data: &std::path::Path, name: &str) -> Vec<Vec<(String, String)>> {
    let text = std::fs::read_to_string(raw_data.join("db").join(format!("{name}.xml"))).unwrap_or_default();
    let open = format!("<{name} ");
    text.split(&open)
        .skip(1)
        .map(|rec| {
            let rec = rec.split(&format!("</{name}>")).next().unwrap_or_default();
            rec.split('<')
                .filter_map(|t| {
                    let (tag, value) = t.split_once('>')?;
                    (!tag.starts_with('/') && !tag.contains(' ')).then(|| (tag.to_owned(), value.to_owned()))
                })
                .collect()
        })
        .collect()
}

/// A file of the kit by its path below `raw_data`, whatever the case of its name (BOB runs on Windows).
fn kit_file(raw_data: &std::path::Path, rel: &str) -> Option<std::path::PathBuf> {
    let p = raw_data.join(rel.replace('\\', "/"));
    let name = p.file_name()?.to_str()?.to_lowercase();
    std::fs::read_dir(p.parent()?).ok()?.flatten().map(|e| e.path()).find(|e| e.file_name().and_then(|n| n.to_str()).is_some_and(|n| n.to_lowercase() == name))
}

/// A building's height mesh and its [`stamp_matrix`].
pub type Stamp = (std::sync::Arc<HeightMesh>, [f32; 16]);

/// The height-mesh stamps of a project's buildings (BOB_Tile `FUN_1000bb10`): each `BUILDING_INSTANCE` whose `battlefield_buildings`
/// row has `height_map_mesh` 1, its [`HeightMesh`] from `models_building.cs2_file` under the kit's `raw_data`, and its
/// [`stamp_matrix`]. The keys without a mesh file come back apart (BOB fails the tile: "Height map mesh file ... is missing").
pub fn building_stamps(raw: &wce_tile_model::Tile, raw_data: &std::path::Path) -> (Vec<Stamp>, Vec<String>) {
    wce_foundation::wce_prof!("stage_b.building_stamps");
    let get = |r: &Vec<(String, String)>, k: &str| r.iter().find(|(f, _)| f == k).map(|(_, v)| v.clone()).unwrap_or_default();
    let with_mesh = kit_table(raw_data, "battlefield_buildings").iter().filter(|r| get(r, "height_map_mesh") == "1").map(|r| get(r, "key")).collect::<std::collections::HashSet<_>>();
    let cs2 = kit_table(raw_data, "models_building").iter().map(|r| (get(r, "key"), get(r, "cs2_file"))).collect::<std::collections::HashMap<_, _>>();
    let mut meshes = std::collections::HashMap::new();
    let (mut out, mut missing) = (vec![], vec![]);
    for (key, world) in super::compile::buildings(raw) {
        if !with_mesh.contains(&key) {
            continue;
        }

        let mesh = meshes.entry(key.clone()).or_insert_with(|| {
            let path = cs2.get(&key).and_then(|c| kit_file(raw_data, c))?;
            let f = wce_formats::cas2::Cas2File::parse(&std::fs::read(path).ok()?).ok()?;
            Some(std::sync::Arc::new(HeightMesh::of(&f)))
        });
        match mesh {
            Some(m) => out.push((m.clone(), stamp_matrix(&world))),
            None if !missing.contains(&key) => missing.push(key),
            None => {}
        }
    }

    (out, missing)
}

/// The decimation angle of the kit's battle tiles (`_tile_database/_settings.xml`, `CONVERSION_PARAMS`
/// `triangle_decimation_angle_factors0`; 20 degrees in the vanilla kit and when missing).
pub fn decimation_angle(raw_data: &std::path::Path) -> f32 {
    let text = std::fs::read_to_string(raw_data.join("terrain/tiles/battle/_tile_database/_settings.xml")).unwrap_or_default();
    text.split("triangle_decimation_angle_factors0=\"").nth(1).and_then(|t| t.split('"').next()).and_then(|v| v.parse().ok()).unwrap_or(20.0)
}

/// The kit's tile sets' linking tiles (`_settings.xml`'s `TILE_SET` `name` and `linking_tile`), for [`super::bob_tile::InfluenceLinks`].
pub fn linking_tiles(raw_data: &std::path::Path) -> std::collections::HashMap<String, String> {
    wce_foundation::wce_prof!("stage_b.linking_tiles");
    let text = std::fs::read_to_string(raw_data.join("terrain/tiles/battle/_tile_database/_settings.xml")).unwrap_or_default();
    let attr = |t: &str, k: &str| t.split(&format!(" {k}=\"")).nth(1).and_then(|v| v.split('"').next()).map(str::to_owned);
    text.split("<TILE_SET ").skip(1).filter_map(|t| Some((attr(t, "name")?, attr(t, "linking_tile")?))).collect()
}

/// The ground and the water of a battle tile on stage B's grid (`w` x `h` vertices of the tile without its border, `t` per 128-metre
/// cell, row j along +z): BOB's final heights (the height map with the variation's numbers, eased into the linking tiles) and the water
/// surface `hf_water_map.data` is made from (the rivers' triangles and the water planes rasterised; -1000 where there is no water; `None`
/// without either). The same calls as [`terrain_files`]; for the editor's water depth view.
pub struct WaterFields {
    pub heights: Vec<f32>,
    pub water: Option<Vec<f32>>,
    pub w: usize,
    pub h: usize,
    pub t: usize,
}

/// [`WaterFields`] of a raw tile.
///
/// # Errors
///
/// As [`terrain_files`].
pub fn water_fields(raw: &wce_tile_model::Tile, raw_data: &std::path::Path, models: &dyn Fn(&str) -> Option<Vec<u8>>) -> Result<WaterFields, String> {
    use super::bob_tile::{HeightNumbers, InfluenceLinks, final_heights};
    let linking = linking_tiles(raw_data);
    let links = &InfluenceLinks::of(raw, &linking);
    let hm = raw.map(wce_tile_model::MapKind::Height, 0).ok_or("the project has no height map")?;
    let wce_tile_model::Samples::U16(hs) = &hm.raster.samples else { return Err("the height map is not 16 bit".into()) };
    let n = HeightNumbers::of(raw);
    let t = n.tri_density;
    if t == 0 || 128 % t != 0 {
        return Err(format!("raw_data_tri_density {t} does not divide 128"));
    }

    let (mut heights, w, h) = final_heights(hs, hm.raster.width as usize, hm.raster.height as usize, &n, links)?;
    super::tile_links::blend_links(raw, raw_data, &linking, &mut heights, w, h)?;
    let river = super::river::tessellate(&super::river::river_splines(raw, (h - 1) / t));
    let planes = super::compile::instances(raw, "SIMPLE_MESH_INSTANCE");
    let water = (!river.triangles.is_empty() || !planes.is_empty()).then(|| {
        let verts = river.vertices.iter().map(|v| Vertex { x: v[0], h: v[1], z: v[2], normal: [0.0; 3], alpha: 0.0, fringe: false }).collect::<Vec<_>>();
        let mut water = vec![-1000.0f32; w * h];
        rasterise_into(&mut water, &verts, &river.triangles, w, h, t);
        super::river::rasterise_water_planes(&mut water, w, h, t, &planes, models);
        water
    });

    Ok(WaterFields { heights, water, w, h, t })
}

/// Stage B's terrain files of a battle tile (BOB's `process_tile_internal`): `outfield_mesh.rigid_model_v2`, `mesh.rigid_model_v2`,
/// `hf_height_map.data` (the infield mesh rasterised, the stamps raised into it) and `terrain_outlines.xml`, from the project's height map, the variation's
/// numbers, its tile links (the height influence joins them, [`super::tile_links::blend_links`] eases the heights into the linking
/// tiles', under the kit's `raw_data`) and `protection_map.dds`; with a river (its block in both meshes) or water planes,
/// `hf_water_map.data`. `models` gives a model's bytes by its key (BOB's VFS: the game's packs) for the water planes.
///
/// # Errors
///
/// The project has no 16-bit height map, or a linking tile's heights are missing.
pub fn terrain_files(
    raw: &wce_tile_model::Tile,
    raw_data: &std::path::Path,
    angle: f32,
    stamps: &[Stamp],
    models: &dyn Fn(&str) -> Option<Vec<u8>>,
) -> Result<Vec<(String, Vec<u8>)>, String> {
    use super::bob_tile::{CELL, HeightNumbers, InfluenceLinks, chmf, final_heights, influence};
    let linking = linking_tiles(raw_data);
    let links = &InfluenceLinks::of(raw, &linking);
    let hm = raw.map(wce_tile_model::MapKind::Height, 0).ok_or("the project has no height map")?;
    let wce_tile_model::Samples::U16(hs) = &hm.raster.samples else { return Err("the height map is not 16 bit".into()) };
    let n = HeightNumbers::of(raw);
    let t = n.tri_density;
    // BOB's mesh steps (1, 8, 16) and its 128-metre subtile assume a density dividing 128; with another the fringe's pairs run past
    // the subtile (BOB asserts "base_index1 < vertices.size()").
    if t == 0 || 128 % t != 0 {
        return Err(format!("raw_data_tri_density {t} does not divide 128"));
    }
    let (mut heights, w, h) = final_heights(hs, hm.raster.width as usize, hm.raster.height as usize, &n, links)?;
    super::tile_links::blend_links(raw, raw_data, &linking, &mut heights, w, h)?;
    // BOB writes these two into the raw folder: the blended final heights and the influence (with the links).
    let mut out = vec![
        ("final_heights.dds".to_owned(), super::bob_tile::dds_r32f(&heights, w, h)),
        ("final_alpha.dds".to_owned(), super::bob_tile::dds_r32f(&influence(t, (w - 1) / t, (h - 1) / t, links), w, h)),
    ];
    // The vertices' alpha: the influence BOB's mesh pass builds (`FUN_10013a40` without the links, the extra ones still).
    let alpha = influence(t, (w - 1) / t, (h - 1) / t, &links.without_links());
    let base = grid_vertices(&heights, &alpha, w, h, n.normal_strength, t);
    let prot = protection_map(raw);
    let prot = prot.as_ref().map(|(p, pw, ph)| (p.as_slice(), *pw, *ph));
    let cos = cos_degrees(angle);
    let river = super::river::tessellate(&super::river::river_splines(raw, (h - 1) / t));
    let g = Grid { w, h, t, mask: links.mask.clone() };
    // The outfield's step: 1 << ((density == 128) + 3).
    let outfield = MeshKind { step: if t == CELL { 16 } else { 8 }, ..OUTFIELD };
    let spacing = (128 / t) as u32;

    // The two meshes are made from the same vertices independently: side by side on the thread pool, their files taken in order.
    let mesh = |(name, kind): (&str, MeshKind)| {
        let mut out = vec![];
        let mut verts = base.clone();
        let (mut flags, mut grid);
        if g.masked() {
            let step = g.step(kind);
            flags = grid_flags_masked(&g, kind, prot);
            grid = grid_triangles_masked(&g, &verts, step);
            add_fringe_masked(&g, &mut verts, &mut flags, &mut grid, step);
        } else {
            flags = grid_flags(w, h, kind, prot);
            grid = grid_triangles(w, h, kind.step);
            add_fringe(&mut verts, &mut flags, &mut grid, w, h, kind);
        }

        let list = merge(&verts, &flags, &grid, cos);
        let is_surface = |t: &[u32; 3]| !t.iter().any(|&i| verts[i as usize].fringe);
        let surface = list.iter().filter(|t| is_surface(t)).copied().collect::<Vec<_>>();
        out.push((name.to_owned(), rmv2(&verts, &list, is_surface, ((w - 1) as u32, (h - 1) as u32), w as u32, &river)));
        if kind == INFIELD {
            let mut field = rasterise(&verts, &surface, w, h, t);
            for (mesh, m) in stamps {
                rasterise_max_heights(mesh, m, &mut field, w, h);
            }

            out.push(("hf_height_map.data".to_owned(), chmf(&field, w, h, spacing)));
        }

        out
    };

    let (outfield_files, infield_files) = rayon::join(|| mesh(("outfield_mesh.rigid_model_v2", outfield)), || mesh(("mesh.rigid_model_v2", INFIELD)));
    out.extend(outfield_files);
    out.extend(infield_files);

    // `hf_water_map.data` (`FUN_10089b80`): a field of -1000, the river's triangles rasterised into it (as the terrain's), then the
    // water planes; none without either.
    let planes = super::compile::instances(raw, "SIMPLE_MESH_INSTANCE");
    if !river.triangles.is_empty() || !planes.is_empty() {
        let verts = river.vertices.iter().map(|v| Vertex { x: v[0], h: v[1], z: v[2], normal: [0.0; 3], alpha: 0.0, fringe: false }).collect::<Vec<_>>();
        let mut water = vec![-1000.0f32; w * h];
        rasterise_into(&mut water, &verts, &river.triangles, w, h, t);
        super::river::rasterise_water_planes(&mut water, w, h, t, &planes, models);
        out.push(("hf_water_map.data".to_owned(), chmf(&water, w, h, spacing)));
    }

    out.push(("terrain_outlines.xml".to_owned(), super::terrain_outlines::terrain_outlines(&heights, w, h, &super::compile::go_outlines(raw), &links.mask, g.cells_of_tile(raw))));
    Ok(out)
}
