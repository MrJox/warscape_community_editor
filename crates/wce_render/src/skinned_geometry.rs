//! A unit's part as triangles in the world, on the CPU, as `renderer/skinned.rs` places it on the GPU: what an editor picks with a click
//! and outlines. LOD 0 only. A weighted mesh (Weighted and Cinematic vertices) is skinned by the instance's palette, two or four bones
//! (`anim_pose::vertex_weights`), and left out when its part hangs from a bone (the renderer draws only static meshes there); a static
//! mesh (weapons, shields, bosses) is carried by its part's bone or its material's matrix index, its part's offset and its material's
//! pivot. Collision shapes, ropes and the other formats are left out.

use std::io::Cursor;

use rpfm_lib::files::rigidmodel::RigidModel;
use rpfm_lib::files::rigidmodel::vertices::VertexFormat;
use rpfm_lib::files::{Decodeable, DecodeableExtraData};
use wce_foundation::math::{Float3, Float4x4, translation};
use wce_scene::render_scene::SkinnedPart;

use crate::anim_pose::vertex_weights;

/// One mesh block of a part's model.
#[derive(Clone, Debug, Default)]
struct Block {
    skinned: bool,
    positions: Vec<Float3>,
    bones: Vec<[u8; 4]>,
    weights: Vec<[f32; 4]>,
    indices: Vec<u16>,

    /// The material's matrix index (a static mesh's bone when its part has none), -1 for none.
    bone: i32,
    pivot: Float3,
}

/// A model's LOD 0, kept to place it again every frame.
#[derive(Clone, Debug, Default)]
pub struct PartGeometry {
    blocks: Vec<Block>,
}

fn matrix_of(rows: &[f32; 12]) -> Float4x4 {
    let mut m = Float4x4::IDENTITY;
    for (r, row) in m.m.iter_mut().take(3).enumerate() {
        row.copy_from_slice(&rows[r * 4..r * 4 + 4]);
    }

    m
}

impl PartGeometry {
    /// A model's file read; `None` when it doesn't decode or has no LOD.
    pub fn decode(bytes: &[u8]) -> Option<Self> {
        let model = RigidModel::decode(&mut Cursor::new(bytes), &Some(DecodeableExtraData::default())).ok()?;
        let lod = model.lods().first()?;
        let mut blocks = vec![];
        for block in lod.mesh_blocks() {
            let material = block.material();
            let format = *material.vertex_format();
            let skinned = match format {
                VertexFormat::Weighted | VertexFormat::Cinematic => true,
                VertexFormat::Static => false,
                _ => continue,
            };
            let mesh = block.mesh();
            let two_bones = format == VertexFormat::Weighted;
            let mut b = Block {
                skinned,
                bone: *material.i_matrix_index(),
                pivot: Float3::new(material.v_pivot().x, material.v_pivot().y, material.v_pivot().z),
                indices: mesh.indices().clone(),
                ..Block::default()
            };
            for v in mesh.vertices() {
                let p = v.position();
                b.positions.push(Float3::new(p.x, p.y, p.z));
                if skinned {
                    let idx = *v.bone_indices();
                    b.bones.push([idx[0], idx[1], idx[2], idx[3]]);
                    b.weights.push(vertex_weights(two_bones, (*v.weights()).into()));
                }
            }

            if !b.indices.is_empty() {
                blocks.push(b);
            }
        }

        Some(Self { blocks })
    }

    /// The part's triangles in the world: `world` is the instance's entity matrix, `palette` its skin matrices and `bones` its bones' world
    /// matrices (`SkinnedInstance`).
    pub fn triangles(&self, part: &SkinnedPart, world: &Float4x4, palette: &[[f32; 12]], bones: &[[f32; 12]]) -> Vec<[Float3; 3]> {
        let mut out = vec![];
        for b in &self.blocks {
            let placed: Vec<Float3> = if b.skinned {
                if part.bone.is_some() {
                    continue;
                }

                let skin: Vec<Float4x4> = palette.iter().map(matrix_of).collect();
                b.positions
                    .iter()
                    .zip(b.bones.iter().zip(&b.weights))
                    .map(|(p, (idx, w))| {
                        let mut q = Float3::new(0.0, 0.0, 0.0);
                        for k in 0..4 {
                            if w[k] > 0.0
                                && let Some(m) = skin.get(usize::from(idx[k]))
                            {
                                q = q + m.transform_point(*p) * w[k];
                            }
                        }

                        world.transform_point(q)
                    })
                    .collect()
            } else {
                let bone = part.bone.or(u32::try_from(b.bone).ok()).and_then(|k| bones.get(k as usize));
                let carrier = bone.map_or(*world, |m| *world * matrix_of(m)) * matrix_of(&part.offset) * translation(b.pivot);
                b.positions.iter().map(|p| carrier.transform_point(*p)).collect()
            };

            for t in b.indices.as_chunks::<3>().0 {
                let at = |i: u16| placed.get(usize::from(i)).copied();
                if let (Some(a), Some(c), Some(d)) = (at(t[0]), at(t[1]), at(t[2])) {
                    out.push([a, c, d]);
                }
            }
        }

        out
    }
}

/// Where a ray first meets a triangle (Möller-Trumbore): its distance along `dir`, both faces.
pub fn ray_triangle(origin: Float3, dir: Float3, t: &[Float3; 3]) -> Option<f32> {
    let cross = |a: Float3, b: Float3| Float3::new(a.y * b.z - a.z * b.y, a.z * b.x - a.x * b.z, a.x * b.y - a.y * b.x);
    let dot = |a: Float3, b: Float3| a.x * b.x + a.y * b.y + a.z * b.z;
    let (e1, e2) = (t[1] - t[0], t[2] - t[0]);
    let p = cross(dir, e2);
    let det = dot(e1, p);
    if det.abs() < 1e-12 {
        return None;
    }

    let inv = 1.0 / det;
    let s = origin - t[0];
    let u = dot(s, p) * inv;
    if !(0.0..=1.0).contains(&u) {
        return None;
    }

    let q = cross(s, e1);
    let v = dot(dir, q) * inv;
    if v < 0.0 || u + v > 1.0 {
        return None;
    }

    let d = dot(e2, q) * inv;
    (d > 0.0).then_some(d)
}

#[cfg(test)]
mod tests {
    use wce_foundation::math::{Float3, normalize};

    use super::ray_triangle;

    #[test]
    fn a_ray_meets_a_triangle_in_front_only() {
        let t = [Float3::new(-1.0, -1.0, 5.0), Float3::new(1.0, -1.0, 5.0), Float3::new(0.0, 1.0, 5.0)];
        let o = Float3::new(0.0, 0.0, 0.0);
        assert!((ray_triangle(o, Float3::new(0.0, 0.0, 1.0), &t).expect("a hit") - 5.0).abs() < 1e-5);
        assert!(ray_triangle(o, Float3::new(0.0, 0.0, -1.0), &t).is_none(), "behind");
        assert!(ray_triangle(o, normalize(Float3::new(1.0, 0.0, 1.0)), &t).is_none(), "beside");
    }
}
