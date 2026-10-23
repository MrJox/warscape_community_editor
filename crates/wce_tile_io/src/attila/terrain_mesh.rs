//! Stage B: BOB's terrain mesh of a battle tile (`TOOLDATABUILDER::TILE_RAW_DATA::build_base_mesh`, ToolDataBuilderDll `FUN_10019750`):
//! a vertex per sample of the final heights, a grid of triangles over them, decimated by `TRIANGLE_MERGER` (`FUN_10003140`). The mesh
//! is `mesh.rigid_model_v2`'s surface, and rasterised it is `hf_height_map.data`. docs/formats/tile_compile.md, "Terrain mesh".

use super::bob_tile::calculate_normal;

/// A vertex flag (`build_base_mesh`'s per vertex and LOD byte): the merger never moves `LOCKED` vertices (the tile's border, every
/// 16th sample inside, the protected samples) and moves `FREE` ones anywhere; `EDGE` ones (the fringe's top row) only along the edge.
pub const LOCKED: u8 = 0;
pub const FREE: u8 = 2;
pub const EDGE: u8 = 3;

/// A vertex of the mesh: its position (x, the height, z; metres) and normal.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Vertex {
    pub x: f32,
    pub h: f32,
    pub z: f32,
    pub normal: [f32; 3],
}

/// The vertices of a tile's mesh, one per sample of the final heights (`w` x `h`, rows as `final_heights.dds`: row 0 is the far
/// edge), row by row from z = 0: vertex `z * w + x` stands at (x, z) with the height of the field's row `h - 1 - z`, its normal
/// [`calculate_normal`] there.
pub fn grid_vertices(heights: &[f32], w: usize, h: usize, strength: f32) -> Vec<Vertex> {
    let mut out = Vec::with_capacity(w * h);
    for z in 0..h {
        let row = h - 1 - z;
        for x in 0..w {
            out.push(Vertex { x: x as f32, h: heights[row * w + x], z: z as f32, normal: calculate_normal(heights, w, h, x, row, strength) });
        }
    }

    out
}

/// The vertex flags of a battle tile alone (`build_base_mesh`, step 1): locked near the edge, where the cell of x - 1, x + 1, z - 1 or
/// z + 1 (rounded down) is off the tile (so the border, and the column and row next to the far edges: x and z >= size - 2), and every
/// 16th sample both ways; the rest free; and locked where `protection_map.dds` (`cells x 128` square, R32F) is above 0.5: vertex
/// (x, z) reads its row z - 1.
pub fn grid_flags(w: usize, h: usize, protection: Option<(&[f32], usize, usize)>) -> Vec<u8> {
    let mut out = Vec::with_capacity(w * h);
    for z in 0..h {
        for x in 0..w {
            let edge = x == 0 || z == 0 || x + 2 >= w || z + 2 >= h;
            let mut flag = if edge || (x % 16 == 0 && z % 16 == 0) { LOCKED } else { FREE };
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
pub fn rasterise(verts: &[Vertex], tris: &[[u32; 3]], w: usize, h: usize) -> Vec<f32> {
    let mut out = vec![0.0f32; w * h];
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

                let (fx, fz) = (x as f32, pz);
                if fx < 0.0 || fx >= w as f32 || fz < 0.0 || fz >= h as f32 {
                    continue;
                }

                let row = (h as f32 - fz - 1.0) as usize;
                out[row * w + fx as usize] = height;
            }
        }
    }

    out
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
