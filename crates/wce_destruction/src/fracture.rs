//! A level of detail's triangles split by the removed cells: each triangle is clipped against the cells it reaches (every vertex
//! attribute interpolated, so UVs, normals and tangents are kept exactly); what lies in a cell goes to that cell's fragment, the rest
//! stays. Blocks that are not plain static meshes (decals, terrain blends) are not cut: a triangle goes whole to the cell of its centre.
//! Positions are in the building's frame here (vertex + the block's pivot).

use std::collections::BTreeMap;

use rpfm_lib::files::rigidmodel::Lod;
use rpfm_lib::files::rigidmodel::materials::MaterialType;
use rpfm_lib::files::rigidmodel::vertices::{Vertex, VertexFormat};
use wce_walls::mesh::clip_by;

use crate::cells::Cells;
use crate::geom::{Aabb, V3, newell};

pub type Tri = [Vertex; 3];

pub fn pos(v: &Vertex) -> V3 {
    let p = v.position();
    V3::new(p.x, p.y, p.z)
}

/// Whether a block is cut (a plain static mesh), or goes whole to a cell.
pub fn is_cut(mtype: MaterialType, format: VertexFormat) -> bool {
    format == VertexFormat::Static && matches!(mtype, MaterialType::DefaultMaterial | MaterialType::TiledDirtmap | MaterialType::Dirtmap | MaterialType::ShipAmbientmap)
}

/// A level of detail's triangles per block, in the building's frame.
pub fn lod_triangles(lod: &Lod) -> Vec<Vec<Tri>> {
    lod.mesh_blocks().iter().map(|b| {
        let p = b.material().v_pivot();
        let (v, ix) = (b.mesh().vertices(), b.mesh().indices());
        ix.chunks_exact(3).map(|t| [0, 1, 2].map(|k| {
            let mut x = v[t[k] as usize].clone();
            let q = x.position_mut();
            (q.x, q.y, q.z) = (q.x + p.x, q.y + p.y, q.z + p.z);
            x
        })).collect()
    }).collect()
}

/// A level of detail split by the cells: per block what never breaks, and per removed cell, per block, what is in it.
#[derive(Clone, Debug, Default)]
pub struct Split {
    pub kept: Vec<Vec<Tri>>,
    pub cells: BTreeMap<usize, Vec<Vec<Tri>>>,
}

impl Split {
    /// What stands at damage level `l` (0: intact): what never breaks and the cells removed after `l`, per block.
    pub fn standing(&self, cells: &Cells, l: usize) -> Vec<Vec<Tri>> {
        let mut out = self.kept.clone();
        for (c, blocks) in &self.cells {
            if cells.rank(*c) > l {
                for (b, tris) in blocks.iter().enumerate() {
                    out[b].extend(tris.iter().cloned());
                }
            }
        }
        out
    }
}

/// Splits a level of detail's blocks (`cut`: per block whether it is cut).
pub fn split(blocks: &[Vec<Tri>], cut: &[bool], cells: &Cells) -> Split {
    let shapes = cells.shapes.iter().map(|(&i, s)| (i, s.bounds())).collect::<BTreeMap<_, _>>();
    let mut out = Split { kept: vec![vec![]; blocks.len()], cells: BTreeMap::new() };
    for (b, tris) in blocks.iter().enumerate() {
        for t in tris {
            let tb = Aabb::of(t.iter().map(pos));
            if !cut.get(b).copied().unwrap_or(false) {
                let c = (pos(&t[0]) + pos(&t[1]) + pos(&t[2])) / 3.0;
                match cells.cell_of(&c).filter(|&i| cells.level[i].is_some() && cells.domain.contains(&c) && shapes.contains_key(&i)) {
                    Some(i) => out.cells.entry(i).or_insert_with(|| vec![vec![]; blocks.len()])[b].push(t.clone()),
                    None => out.kept[b].push(t.clone()),
                }
                continue;
            }

            let cands = shapes.iter().filter(|(_, sb)| sb.overlaps(&tb)).map(|(&i, _)| i).collect::<Vec<_>>();
            if cands.is_empty() {
                out.kept[b].push(t.clone());
                continue;
            }

            let mut polys = vec![];
            assign(t.to_vec(), &cands, cells, &mut polys);
            for (cell, poly) in polys {
                let tris = fan(&poly);
                match cell {
                    Some(c) => out.cells.entry(c).or_insert_with(|| vec![vec![]; blocks.len()])[b].extend(tris),
                    None => out.kept[b].extend(tris),
                }
            }
        }
    }

    out
}

/// A polygon given to the first candidate cell holding part of it; the parts outside go on to the other candidates.
fn assign(poly: Vec<Vertex>, cands: &[usize], cells: &Cells, out: &mut Vec<(Option<usize>, Vec<Vertex>)>) {
    let Some((&c, rest_cands)) = cands.split_first() else {
        out.push((None, poly));
        return;
    };

    let shape = &cells.shapes[&c];
    if !shape.bounds().overlaps(&Aabb::of(poly.iter().map(pos))) {
        assign(poly, rest_cands, cells, out);
        return;
    }

    let mut rest = poly;
    for f in &shape.faces {
        let outside = clip_by(&rest, |v| f.plane.side(&pos(v)));
        if outside.len() >= 3 && area(&outside) > 1e-7 {
            assign(outside, rest_cands, cells, out);
        }

        rest = clip_by(&rest, |v| -f.plane.side(&pos(v)));
        if rest.len() < 3 || area(&rest) <= 1e-7 {
            return;
        }
    }

    out.push((Some(c), rest));
}

fn area(poly: &[Vertex]) -> f32 {
    newell(&poly.iter().map(pos).collect::<Vec<_>>()).norm() * 0.5
}

/// A convex polygon as a fan of triangles.
pub fn fan(poly: &[Vertex]) -> Vec<Tri> {
    (1..poly.len().saturating_sub(1)).map(|i| [poly[0].clone(), poly[i].clone(), poly[i + 1].clone()]).filter(|t| {
        let (a, b, c) = (pos(&t[0]), pos(&t[1]), pos(&t[2]));
        (b - a).cross(&(c - a)).norm_squared() > 1e-12
    }).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::params::Params;
    use nalgebra::{Vector2, Vector4};

    fn vertex(x: f32, y: f32, z: f32) -> Vertex {
        let mut v = Vertex::default();
        v.set_position(Vector4::new(x, y, z, 1.0));
        v.set_texture_coordinate(Vector2::new(x / 20.0, y / 20.0));
        v.set_normal(Vector4::new(0.0, 0.0, -1.0, 0.0));
        v
    }

    #[test]
    fn a_wall_face_splits_without_losing_area_or_uvs() {
        let b = Aabb { min: V3::new(-10.0, 0.0, -2.0), max: V3::new(10.0, 16.0, 10.0) };
        let p = Params::defaults("x", &b, "fort_wall");
        let cells = Cells::make(&b, &p);
        let face = vec![vec![[vertex(-10.0, 0.0, -2.0), vertex(10.0, 16.0, -2.0), vertex(10.0, 0.0, -2.0)], [vertex(-10.0, 0.0, -2.0), vertex(-10.0, 16.0, -2.0), vertex(10.0, 16.0, -2.0)]]];
        let s = split(&face, &[true], &cells);
        let total = |tris: &[Tri]| tris.iter().map(|t| (pos(&t[1]) - pos(&t[0])).cross(&(pos(&t[2]) - pos(&t[0]))).norm() * 0.5).sum::<f32>();
        let area = total(&s.kept[0]) + s.cells.values().map(|b| total(&b[0])).sum::<f32>();
        assert!((area - 320.0).abs() < 0.05, "{area}");
        assert!(!s.cells.is_empty() && total(&s.kept[0]) > 50.0);
        for t in s.cells.values().flat_map(|b| b[0].iter()).chain(&s.kept[0]) {
            for v in t {
                assert!((v.texture_coordinate().x - v.position().x / 20.0).abs() < 1e-4, "the UV follows the position");
            }
        }

        let standing = s.standing(&cells, 1);
        assert!(total(&standing[0]) > total(&s.kept[0]) - 1e-3 && total(&standing[0]) < area);
    }
}
