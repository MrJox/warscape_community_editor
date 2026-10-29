//! Triangles (the building's frame) into a model's blocks: grouped by material, their vertices welded and stored around the level of
//! detail's pivot (the centre of its box, as the game's buildings have it), split where 16-bit indices run out. Also the vertices made
//! here: broken faces and rubble with box-projected UVs (tangent along dP/du, bitangent = tangent x normal: the game's convention,
//! destruction_calibrate), fragments moved to where they landed, and the inner side of shells.

use std::collections::HashMap;

use nalgebra::{Vector2, Vector4};
use rpfm_lib::files::rigidmodel::{Lod, MeshBlock};
use rpfm_lib::files::rigidmodel::vertices::Vertex;

use crate::fracture::{Tri, pos};
use crate::geom::{Aabb, V3, rotate};

/// What a block of the output is made of: one of the source's blocks (by index in its level of detail), or a made material.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Slot {
    Source(usize),
    Damage(u8),
    Rubble,
}

/// Triangles gathered per slot, in order of first use.
#[derive(Clone, Debug, Default)]
pub struct Gather {
    pub slots: Vec<(Slot, Vec<Tri>)>,
}

impl Gather {
    pub fn add(&mut self, slot: Slot, tris: impl IntoIterator<Item = Tri>) {
        let i = match self.slots.iter().position(|(s, _)| *s == slot) {
            Some(i) => i,
            None => {
                self.slots.push((slot, vec![]));
                self.slots.len() - 1
            }
        };

        self.slots[i].1.extend(tris);
    }

    pub fn bounds(&self) -> Aabb {
        Aabb::of(self.slots.iter().flat_map(|(_, t)| t.iter().flatten().map(pos)))
    }

    pub fn is_empty(&self) -> bool {
        self.slots.iter().all(|(_, t)| t.is_empty())
    }

    /// The blocks: each slot's material from `material(slot)` (a block whose mesh is replaced), named `name`, bound to `bone`, its
    /// vertices around `pivot`.
    pub fn blocks(&self, material: &dyn Fn(Slot) -> Option<MeshBlock>, name: &str, bone: i32, pivot: V3) -> Vec<MeshBlock> {
        let mut out = vec![];
        for (slot, tris) in &self.slots {
            if tris.is_empty() {
                continue;
            }

            let Some(template) = material(*slot) else { continue };
            for chunk in chunks(tris, &pivot) {
                let mut b = template.clone();
                let (vertices, indices) = chunk;
                let bb = Aabb::of(vertices.iter().map(|v| pos(v) + pivot));
                let mesh = b.mesh_mut();
                *mesh.vertices_mut() = vertices;
                *mesh.indices_mut() = indices;
                mesh.extra_indices_mut().clear();
                mesh.set_name(String::new());
                if !bb.is_empty() {
                    let lo = mesh.min_bb_mut();
                    (lo.x, lo.y, lo.z) = (bb.min.x, bb.min.y, bb.min.z);
                    let hi = mesh.max_bb_mut();
                    (hi.x, hi.y, hi.z) = (bb.max.x, bb.max.y, bb.max.z);
                }

                let m = b.material_mut();
                m.set_name(name.chars().take(31).collect());
                m.set_i_matrix_index(bone);
                m.set_i_parent_matrix_index(-1);
                let p = m.v_pivot_mut();
                (p.x, p.y, p.z) = (pivot.x, pivot.y, pivot.z);
                out.push(b);
            }
        }

        out
    }
}

/// Triangles as vertex and index lists, their positions around `pivot`, each under 65536 vertices.
fn chunks(tris: &[Tri], pivot: &V3) -> Vec<(Vec<Vertex>, Vec<u16>)> {
    let mut out = vec![];
    let mut verts: Vec<Vertex> = vec![];
    let mut idx: Vec<u16> = vec![];
    let mut seen: HashMap<Vec<u32>, u16> = HashMap::new();
    for t in tris {
        if verts.len() + 3 > 65_000 {
            out.push((std::mem::take(&mut verts), std::mem::take(&mut idx)));
            seen.clear();
        }

        for v in t {
            let mut v = v.clone();
            let p = v.position_mut();
            (p.x, p.y, p.z) = (p.x - pivot.x, p.y - pivot.y, p.z - pivot.z);
            let k = key(&v);
            let i = *seen.entry(k).or_insert_with(|| {
                verts.push(v);
                (verts.len() - 1) as u16
            });
            idx.push(i);
        }
    }

    if !idx.is_empty() {
        out.push((verts, idx));
    }

    out
}

fn key(v: &Vertex) -> Vec<u32> {
    let mut k = Vec::with_capacity(22);
    for x in [v.position().as_slice(), v.normal().as_slice(), v.tangent().as_slice(), v.bitangent().as_slice(), v.color().as_slice()] {
        k.extend(x.iter().map(|f| f.to_bits()));
    }
    k.extend(v.texture_coordinate().iter().chain(v.texture_coordinate2().iter()).map(|f| f.to_bits()));
    k
}

/// A level of detail's header (distance, numbers) with new blocks.
pub fn lod_like(source: &Lod, blocks: Vec<MeshBlock>) -> Lod {
    let mut l = source.clone();
    *l.mesh_blocks_mut() = blocks;
    l
}

/// A vertex made here: position, normal, tangent, bitangent, UV (also the second UV), the rest from a template vertex of the material.
pub fn vertex(template: &Vertex, p: V3, n: V3, t: V3, b: V3, uv: (f32, f32)) -> Vertex {
    let mut v = template.clone();
    v.set_position(Vector4::new(p.x, p.y, p.z, 1.0));
    v.set_normal(Vector4::new(n.x, n.y, n.z, template.normal().w));
    v.set_tangent(Vector4::new(t.x, t.y, t.z, template.tangent().w));
    v.set_bitangent(Vector4::new(b.x, b.y, b.z, template.bitangent().w));
    v.set_texture_coordinate(Vector2::new(uv.0, uv.1));
    v.set_texture_coordinate2(Vector2::new(uv.0, uv.1));
    v
}

/// The box projection of a face of normal `n`: the U axis on the face (from the world axis the face is least turned from), and V = U x n
/// (the game's UVs are mirrored: a bitangent along dP/dv is tangent x normal).
pub fn projection(n: &V3) -> (V3, V3) {
    let a = if n.y.abs() >= n.x.abs() && n.y.abs() >= n.z.abs() { V3::x() } else if n.x.abs() >= n.z.abs() { V3::z() } else { V3::x() };
    let u = (a - n * n.dot(&a)).try_normalize(1e-6).unwrap_or_else(|| crate::geom::basis(n).0);
    (u, u.cross(n))
}

/// A planar polygon (counter-clockwise about `n`) as triangles with box-projected UVs spanning `span` m.
pub fn polygon_tris(template: &Vertex, poly: &[V3], n: &V3, span: f32) -> Vec<Tri> {
    let (u, v) = projection(n);
    let mk = |p: &V3| vertex(template, *p, *n, u, v, (p.dot(&u) / span, p.dot(&v) / span));
    (1..poly.len().saturating_sub(1)).map(|i| [mk(&poly[0]), mk(&poly[i]), mk(&poly[i + 1])]).collect()
}

/// A triangle of a surface with its own normals per vertex (the rubble heap), UVs projected from above.
pub fn surface_tri(template: &Vertex, t: &[(V3, V3); 3], span: f32) -> Tri {
    t.clone().map(|(p, n)| {
        let (u, v) = projection(&n);
        vertex(template, p, n, u, v, (p.x / span, -p.z / span))
    })
}

/// A vertex turned by `q` and moved: `R (p - origin) + to`.
pub fn moved(v: &Vertex, origin: &V3, q: [f32; 4], to: &V3) -> Vertex {
    let mut out = v.clone();
    let p = rotate(q, &(pos(v) - origin)) + to;
    out.set_position(Vector4::new(p.x, p.y, p.z, 1.0));
    let turn = |d: &Vector4<f32>| {
        let r = rotate(q, &V3::new(d.x, d.y, d.z));
        Vector4::new(r.x, r.y, r.z, d.w)
    };
    out.set_normal(turn(v.normal()));
    out.set_tangent(turn(v.tangent()));
    out.set_bitangent(turn(v.bitangent()));
    out
}

/// The inner side of a shell's triangle: moved in along its normals by `t`, facing the other way.
pub fn inner(tri: &Tri, t: f32) -> Tri {
    let mut out = tri.clone();
    out.swap(1, 2);
    for v in &mut out {
        let n = V3::new(v.normal().x, v.normal().y, v.normal().z);
        let p = pos(v) - n * t;
        v.set_position(Vector4::new(p.x, p.y, p.z, 1.0));
        let (nn, b) = (*v.normal(), *v.bitangent());
        v.set_normal(Vector4::new(-nn.x, -nn.y, -nn.z, nn.w));
        v.set_bitangent(Vector4::new(-b.x, -b.y, -b.z, b.w));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn projected_faces_follow_the_game_convention() {
        let n = V3::new(0.0, 0.0, -1.0);
        let (u, v) = projection(&n);
        assert!((u.cross(&n) - v).norm() < 1e-6, "bitangent = tangent x normal");
        let poly = [V3::new(0.0, 0.0, 0.0), V3::new(0.0, 1.0, 0.0), V3::new(1.0, 1.0, 0.0), V3::new(1.0, 0.0, 0.0)];
        let tris = polygon_tris(&Vertex::default(), &poly, &n, 2.0);
        assert_eq!(tris.len(), 2);
        // dP/du goes along the tangent.
        let t = &tris[0];
        let (p0, p1) = (pos(&t[0]), pos(&t[1]));
        let du = t[1].texture_coordinate().x - t[0].texture_coordinate().x;
        let dp = p1 - p0;
        assert!(dp.dot(&u) * du >= 0.0);
        let mut g = Gather::default();
        g.add(Slot::Rubble, tris);
        let mut tpl = MeshBlock::default();
        tpl.material_mut().set_name("x".into());
        let blocks = g.blocks(&|_| Some(tpl.clone()), "piece01_destruct02_lod01", -1, V3::new(0.5, 0.5, 0.0));
        assert_eq!(blocks[0].mesh().vertices().len(), 4, "welded");
        assert_eq!(blocks[0].mesh().indices().len(), 6);
    }
}
