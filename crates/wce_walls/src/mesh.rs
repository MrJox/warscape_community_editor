//! A piece's mesh (`.rigid_model_v2`) under a warp. A mesh's vertices are stored around its material's pivot (`v_pivot`, the centre of
//! its box): the piece's frame, the logic file's, is vertex + pivot, and the meshes' boxes (`min_bb`, `max_bb`) are in it. Triangles
//! are first cut into strips along x of that frame at `cuts` (the walls are made of triangles up to 24 m long, which would bend
//! as chords), the new vertices interpolated from the old; then positions are warped, normals, tangents and bitangents turned, the boxes
//! recomputed. The pivot, UVs and materials are kept.

use std::io::Cursor;

use nalgebra::{Vector2, Vector4};
use rpfm_lib::files::rigidmodel::{Mesh, RigidModel};
use rpfm_lib::files::rigidmodel::vertices::Vertex;
use rpfm_lib::files::{Decodeable, DecodeableExtraData, Encodeable};

use crate::warp::{Cuts, Warp, turn3};

/// A mesh file warped, its triangles cut along x at `cuts` first.
///
/// # Errors
///
/// The file doesn't read or doesn't write back, or a mesh gets more vertices than its 16-bit indices can reach.
pub fn warp_mesh(bytes: &[u8], warp: &dyn Warp, cuts: &Cuts) -> Result<Vec<u8>, String> {
    warp_mesh_with(bytes, warp, cuts, &[], None, &[])
}

/// Another building's part brought into a piece's mesh (a step section): the triangles of its mesh over `x` of its frame (and inside
/// `zy`, z then y, when given), warped by its own `warp`. Its blocks keep their materials.
pub struct MeshSplice<'a> {
    pub mesh: &'a [u8],
    pub warp: &'a dyn Warp,
    pub x: (f32, f32),
    pub zy: Option<((f32, f32), (f32, f32))>,

    /// Only what lies within `band.2` (y) of the line from `(x.0, band.0)` to `(x.1, band.1)`: a stair without the walls around it.
    pub band: Option<(f32, f32, f32)>,
}

/// If a point of a part's frame is in what a splice brings: over `x`, inside `zy`, near the `band` line.
pub fn splice_holds(c: [f32; 3], x: (f32, f32), zy: Option<((f32, f32), (f32, f32))>, band: Option<(f32, f32, f32)>) -> bool {
    let near = |(y0, y1, half): (f32, f32, f32)| {
        let t = ((c[0] - x.0) / (x.1 - x.0).max(1e-6)).clamp(0.0, 1.0);
        (c[1] - (y0 + (y1 - y0) * t)).abs() <= half
    };

    c[0] >= x.0 - 1e-3 && c[0] <= x.1 + 1e-3 && zy.is_none_or(|((z0, z1), (y0, y1))| c[2] >= z0 && c[2] <= z1 && c[1] >= y0 && c[1] <= y1) && band.is_none_or(near)
}

/// [`warp_mesh`] with step sections: the base's triangles over the `drop` x ranges (of its frame, and inside `drop_zy` when given) left
/// out, the splices' blocks added to each level of detail (the part's level of the same number, or its last).
///
/// # Errors
///
/// A file doesn't read, or the result doesn't write back.
pub fn warp_mesh_with(bytes: &[u8], warp: &dyn Warp, cuts: &Cuts, drop: &[(f32, f32)], drop_zy: Option<((f32, f32), (f32, f32))>, splices: &[MeshSplice]) -> Result<Vec<u8>, String> {
    let mut model = decode(bytes)?;
    let in_box = |c: [f32; 3]| drop_zy.is_none_or(|((z0, z1), (y0, y1))| c[2] >= z0 && c[2] <= z1 && c[1] >= y0 && c[1] <= y1);
    let kept = |c: [f32; 3]| !(in_box(c) && drop.iter().any(|&(a, b)| c[0] > a && c[0] < b));
    for lod in model.lods_mut() {
        for block in lod.mesh_blocks_mut() {
            warp_block(block, warp, cuts, &kept)?;
        }
    }

    for sp in splices {
        let part = decode(sp.mesh)?;
        let part_cuts = Cuts { step: cuts.step, at: vec![sp.x.0, sp.x.1] };
        let holds = |c: [f32; 3]| splice_holds(c, sp.x, sp.zy, sp.band);

        let n = model.lods().len();
        for (k, lod) in model.lods_mut().iter_mut().enumerate().take(n) {
            let Some(from) = part.lods().get(k).or(part.lods().last()) else { continue };
            for block in from.mesh_blocks() {
                let mut b = block.clone();
                warp_block(&mut b, sp.warp, &part_cuts, &holds)?;
                if !b.mesh().indices().is_empty() {
                    lod.mesh_blocks_mut().push(b);
                }
            }
        }
    }

    encode(&mut model)
}

/// A block cut at `cuts`, the triangles whose centre (the piece's frame) `keep` refuses left out, warped.
fn warp_block(block: &mut rpfm_lib::files::rigidmodel::MeshBlock, warp: &dyn Warp, cuts: &Cuts, keep: &dyn Fn([f32; 3]) -> bool) -> Result<(), String> {
    let pivot = block.material().v_pivot();
    let pivot = [pivot.x, pivot.y, pivot.z];
    let mesh = block.mesh_mut();
    slice_mesh(mesh, cuts, pivot[0])?;
    clip_mesh(mesh, pivot, &warp.clip_planes())?;
    keep_triangles(mesh, pivot, keep);
    let (mut lo, mut hi) = ([f32::MAX; 3], [f32::MIN; 3]);
    for v in mesh.vertices_mut() {
        let p = [v.position().x + pivot[0], v.position().y + pivot[1], v.position().z + pivot[2]];
        let a = warp.yaw(p);
        let q = warp.point(p);
        let pos = v.position_mut();
        (pos.x, pos.y, pos.z) = (q[0] - pivot[0], q[1] - pivot[1], q[2] - pivot[2]);
        for i in 0..3 {
            lo[i] = lo[i].min(q[i]);
            hi[i] = hi[i].max(q[i]);
        }

        turned(v.normal_mut(), a);
        turned(v.tangent_mut(), a);
        turned(v.bitangent_mut(), a);
    }

    if lo[0] <= hi[0] {
        let min = mesh.min_bb_mut();
        (min.x, min.y, min.z) = (lo[0], lo[1], lo[2]);
        let max = mesh.max_bb_mut();
        (max.x, max.y, max.z) = (hi[0], hi[1], hi[2]);
    }

    Ok(())
}

/// The triangles whose centre (vertex + pivot) `keep` takes, the vertices they use (all of them when it takes every triangle).
fn keep_triangles(mesh: &mut Mesh, pivot: [f32; 3], keep: &dyn Fn([f32; 3]) -> bool) {
    if !mesh.extra_indices().is_empty() {
        return;
    }

    let v = mesh.vertices();
    let centre = |t: &[u16]| {
        let p = |i: u16| { let q = v[i as usize].position(); [q.x + pivot[0], q.y + pivot[1], q.z + pivot[2]] };
        let (a, b, c) = (p(t[0]), p(t[1]), p(t[2]));
        [(a[0] + b[0] + c[0]) / 3.0, (a[1] + b[1] + c[1]) / 3.0, (a[2] + b[2] + c[2]) / 3.0]
    };

    let tris = mesh.indices().chunks_exact(3).collect::<Vec<_>>();
    if tris.iter().all(|t| keep(centre(t))) {
        return;
    }

    let indices = tris.into_iter().filter(|t| keep(centre(t))).flatten().copied().collect::<Vec<_>>();
    let (vertices, indices) = compact(mesh.vertices(), &indices);
    *mesh.vertices_mut() = vertices;
    *mesh.indices_mut() = indices;
}

/// A mesh's triangles cut into strips along x of the piece's frame (vertex x + `pivot_x`) at `cuts`. A mesh with extra indices is left
/// as it is (they are not understood).
fn slice_mesh(mesh: &mut Mesh, cuts: &Cuts, pivot_x: f32) -> Result<(), String> {
    if !mesh.extra_indices().is_empty() {
        return Ok(());
    }

    let mut vertices = mesh.vertices().clone();
    let mut indices = Vec::with_capacity(mesh.indices().len());
    for t in mesh.indices().chunks_exact(3) {
        let tri = [&vertices[t[0] as usize], &vertices[t[1] as usize], &vertices[t[2] as usize]];
        let (lo, hi) = tri.iter().fold((f32::MAX, f32::MIN), |(lo, hi), v| (lo.min(v.position().x + pivot_x), hi.max(v.position().x + pivot_x)));
        let bounds = cuts.bounds(lo, hi);
        if bounds.len() <= 2 {
            indices.extend_from_slice(t);
            continue;
        }

        let polygon = tri.map(Clone::clone).to_vec();
        for w in bounds.windows(2) {
            let (a, b) = (w[0] - pivot_x, w[1] - pivot_x);
            let slab = clip(&clip(&polygon, a, true), b, false);
            if slab.len() < 3 {
                continue;
            }

            let base = vertices.len();
            if base + slab.len() > usize::from(u16::MAX) {
                return Err(format!("{}: too many vertices after cutting", mesh.name()));
            }

            vertices.extend(slab.iter().cloned());
            for i in 1..slab.len() - 1 {
                indices.extend_from_slice(&[base as u16, (base + i) as u16, (base + i + 1) as u16]);
            }
        }
    }

    let (vertices, indices) = compact(&vertices, &indices);
    *mesh.vertices_mut() = vertices;
    *mesh.indices_mut() = indices;
    Ok(())
}

/// A mesh's triangles cut to the half-planes (`a x + b z <= c` of the piece's frame, vertex + pivot); the parts outside left out.
fn clip_mesh(mesh: &mut Mesh, pivot: [f32; 3], planes: &[[f32; 3]]) -> Result<(), String> {
    if planes.is_empty() || !mesh.extra_indices().is_empty() {
        return Ok(());
    }

    let side = |v: &Vertex, [a, b, c]: [f32; 3]| c - (a * (v.position().x + pivot[0]) + b * (v.position().z + pivot[2]));
    let verts = mesh.vertices();
    if verts.iter().all(|v| planes.iter().all(|&pl| side(v, pl) >= -1e-4)) {
        return Ok(());
    }

    let mut vertices = verts.clone();
    let mut indices = vec![];
    for t in mesh.indices().chunks_exact(3) {
        let tri = [&vertices[t[0] as usize], &vertices[t[1] as usize], &vertices[t[2] as usize]];
        if tri.iter().all(|v| planes.iter().all(|&pl| side(v, pl) >= -1e-4)) {
            indices.extend_from_slice(t);
            continue;
        }

        let mut poly = tri.map(Clone::clone).to_vec();
        for &pl in planes {
            poly = clip_by(&poly, |v| side(v, pl));
            if poly.len() < 3 {
                break;
            }
        }

        if poly.len() < 3 {
            continue;
        }

        let base = vertices.len();
        if base + poly.len() > usize::from(u16::MAX) {
            return Err(format!("{}: too many vertices after clipping", mesh.name()));
        }

        vertices.extend(poly.iter().cloned());
        for i in 1..poly.len() - 1 {
            indices.extend_from_slice(&[base as u16, (base + i) as u16, (base + i + 1) as u16]);
        }
    }

    let (vertices, indices) = compact(&vertices, &indices);
    *mesh.vertices_mut() = vertices;
    *mesh.indices_mut() = indices;
    Ok(())
}

/// The part of a polygon where `side >= 0` (Sutherland-Hodgman, the vertices interpolated).
pub fn clip_by(polygon: &[Vertex], side: impl Fn(&Vertex) -> f32) -> Vec<Vertex> {
    let mut out = vec![];
    for i in 0..polygon.len() {
        let (a, b) = (&polygon[i], &polygon[(i + 1) % polygon.len()]);
        let (sa, sb) = (side(a), side(b));
        if sa >= 0.0 {
            out.push(a.clone());
        }

        if (sa >= 0.0) != (sb >= 0.0) {
            out.push(lerp(a, b, sa / (sa - sb)));
        }
    }

    out
}

/// The vertices the triangles use, each once (the cuts make a copy of a vertex per slab; the triangles that were cut leave theirs unused).
pub fn compact(vertices: &[Vertex], indices: &[u16]) -> (Vec<Vertex>, Vec<u16>) {
    let key = |v: &Vertex| {
        let mut k = Vec::with_capacity(24);
        for x in [v.position().as_slice(), v.normal().as_slice(), v.tangent().as_slice(), v.bitangent().as_slice(), v.color().as_slice(), v.weights().as_slice()] {
            k.extend(x.iter().map(|f| f.to_bits()));
        }

        k.extend(v.texture_coordinate().iter().chain(v.texture_coordinate2().iter()).map(|f| f.to_bits()));
        k.extend(v.bone_indices().iter().map(|&b| u32::from(b)));
        k
    };

    let mut seen = std::collections::HashMap::new();
    let mut out = vec![];
    let new = indices.iter().map(|&i| {
        let v = &vertices[i as usize];
        *seen.entry(key(v)).or_insert_with(|| {
            out.push(v.clone());
            (out.len() - 1) as u16
        })
    }).collect();
    (out, new)
}

/// The part of a polygon on one side of the plane x = `at` (`keep_above`: x >= at).
fn clip(polygon: &[Vertex], at: f32, keep_above: bool) -> Vec<Vertex> {
    let side = |v: &Vertex| if keep_above { v.position().x - at } else { at - v.position().x };
    let mut out = vec![];
    for i in 0..polygon.len() {
        let (a, b) = (&polygon[i], &polygon[(i + 1) % polygon.len()]);
        let (sa, sb) = (side(a), side(b));
        if sa >= 0.0 {
            out.push(a.clone());
        }

        if (sa >= 0.0) != (sb >= 0.0) {
            // From the vertex with the lower x, so both triangles of an edge get the same point.
            let (p, q) = if a.position().x <= b.position().x { (a, b) } else { (b, a) };
            let t = (at - p.position().x) / (q.position().x - p.position().x);
            let mut v = lerp(p, q, t);
            v.position_mut().x = at;
            out.push(v);
        }
    }

    out.dedup_by(|a, b| (a.position() - b.position()).norm_squared() < 1e-12);
    if out.len() > 1 && (out[0].position() - out[out.len() - 1].position()).norm_squared() < 1e-12 {
        out.pop();
    }

    out
}

/// A vertex between two: what varies smoothly is interpolated (directions renormalised), the rest (bone indices, unknown bytes) taken
/// from the nearer one.
pub fn lerp(a: &Vertex, b: &Vertex, t: f32) -> Vertex {
    let mut v = if t < 0.5 { a.clone() } else { b.clone() };
    let l4 = |x: &Vector4<f32>, y: &Vector4<f32>| x + (y - x) * t;
    let l2 = |x: &Vector2<f32>, y: &Vector2<f32>| x + (y - x) * t;
    let dir = |x: &Vector4<f32>, y: &Vector4<f32>| {
        let mut d = l4(x, y);
        let n = (d.x * d.x + d.y * d.y + d.z * d.z).sqrt();
        if n > 0.0 {
            (d.x, d.y, d.z) = (d.x / n, d.y / n, d.z / n);
        }
        d
    };

    v.set_position(l4(a.position(), b.position()));
    v.set_position2(l4(a.position2(), b.position2()));
    v.set_texture_coordinate(l2(a.texture_coordinate(), b.texture_coordinate()));
    v.set_texture_coordinate2(l2(a.texture_coordinate2(), b.texture_coordinate2()));
    v.set_uk_2(l2(a.uk_2(), b.uk_2()));
    v.set_normal(dir(a.normal(), b.normal()));
    v.set_tangent(dir(a.tangent(), b.tangent()));
    v.set_bitangent(dir(a.bitangent(), b.bitangent()));
    v.set_color(l4(a.color(), b.color()));
    v.set_weights(l4(a.weights(), b.weights()));
    v.set_tree_colour_wind(l4(a.tree_colour_wind(), b.tree_colour_wind()));
    v
}

/// A vertex's direction turned about the vertical axis.
fn turned(d: &mut Vector4<f32>, a: f32) {
    let t = turn3([d.x, d.y, d.z], a);
    (d.x, d.y, d.z) = (t[0], t[1], t[2]);
}

/// The longest triangle edge of a mesh file's first level of detail, in metres along x (how far apart the points a warp moves are).
///
/// # Errors
///
/// The file doesn't read.
pub fn longest_edge(bytes: &[u8]) -> Result<f32, String> {
    let model = decode(bytes)?;
    let mut longest = 0.0f32;
    for block in model.lods().iter().take(1).flat_map(|l| l.mesh_blocks()) {
        let (v, ix) = (block.mesh().vertices(), block.mesh().indices());
        for t in ix.chunks_exact(3) {
            for (a, b) in [(t[0], t[1]), (t[1], t[2]), (t[2], t[0])] {
                let (a, b) = (&v[a as usize], &v[b as usize]);
                longest = longest.max((a.position().x - b.position().x).abs());
            }
        }
    }

    Ok(longest)
}

/// If a mesh file writes back to the same bytes unchanged (what a warp's output can be trusted on).
///
/// # Errors
///
/// The file doesn't read or doesn't write back.
pub fn round_trips(bytes: &[u8]) -> Result<bool, String> {
    Ok(encode(&mut decode(bytes)?)? == bytes)
}

fn decode(bytes: &[u8]) -> Result<RigidModel, String> {
    RigidModel::decode(&mut Cursor::new(bytes), &Some(DecodeableExtraData::default())).map_err(|e| format!("rmv2: {e}"))
}

fn encode(model: &mut RigidModel) -> Result<Vec<u8>, String> {
    let mut out = vec![];
    model.encode(&mut out, &None).map_err(|e| format!("rmv2: {e}"))?;
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn vertex(x: f32, y: f32, z: f32, u: f32) -> Vertex {
        let mut v = Vertex::default();
        v.set_position(Vector4::new(x, y, z, 1.0));
        v.set_texture_coordinate(Vector2::new(u, 0.0));
        v.set_normal(Vector4::new(0.0, 0.0, -1.0, 0.0));
        v
    }

    #[test]
    fn long_triangles_are_cut_into_strips() {
        let mut mesh = Mesh::default();
        *mesh.vertices_mut() = vec![vertex(0.0, 0.0, -2.0, 0.0), vertex(20.0, 0.0, -2.0, 1.0), vertex(20.0, 8.0, -2.0, 1.0)];
        *mesh.indices_mut() = vec![0, 1, 2];
        slice_mesh(&mut mesh, &Cuts::every(1.0), 0.0).unwrap();

        let v = mesh.vertices();
        let tris = mesh.indices().chunks_exact(3).collect::<Vec<_>>();
        assert!(tris.len() >= 20);
        let mut area = 0.0;
        for t in &tris {
            let (a, b, c) = (v[t[0] as usize].position().xyz(), v[t[1] as usize].position().xyz(), v[t[2] as usize].position().xyz());
            let span = [a.x, b.x, c.x].iter().fold(0.0f32, |m, x| m.max((x - a.x).abs()));
            assert!(span <= 1.0 + 1e-4);
            // Same winding as the original (normal towards -z).
            assert!((b - a).cross(&(c - a)).z > -1e-6);
            area += 0.5 * (b - a).cross(&(c - a)).norm();
        }
        assert!((area - 80.0).abs() < 1e-3);

        // The UV follows the position.
        for t in &tris {
            for &i in t.iter() {
                assert!((v[i as usize].texture_coordinate().x - v[i as usize].position().x / 20.0).abs() < 1e-5);
            }
        }
    }
}
