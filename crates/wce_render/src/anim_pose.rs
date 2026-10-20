//! A building animation's first frame: the pose the editor draws a mesh that only exists as an animation in (the siege engines' wheels,
//! `..._left_wheel_looped_anim`, and their weighted bodies, `..._weighted_anim`). A tile's siege engine stands still, so its first frame is
//! what a battle shows before it moves.
//!
//! `.anim`: wce_formats::anim.
//!
//! `.bone_inv_trans_mats` (next to a weighted mesh): a u32 (1), the bone count, per bone a 4 x 3 row-vector matrix (the last row the
//! translation): the inverse of the bone's bind pose.
//!
//! A rigid ("building" skeleton) mesh follows one bone: its vertices are in the bone's frame. A weighted ("generic" skeleton) mesh blends its
//! bones: `sum(w * world * inverse bind)`.

use wce_foundation::math::Float4x4;

/// The first frame of an animation: each bone's world matrix (column vectors, model space).
#[derive(Clone, Debug, Default)]
pub struct Pose {
    pub bones: Vec<Float4x4>,
}

struct Reader<'a> {
    b: &'a [u8],
    p: usize,
}

impl Reader<'_> {
    fn u32(&mut self) -> Option<u32> {
        let s = self.b.get(self.p..self.p + 4)?;
        self.p += 4;
        Some(u32::from_le_bytes([s[0], s[1], s[2], s[3]]))
    }

    fn f32(&mut self) -> Option<f32> {
        self.u32().map(f32::from_bits)
    }
}

/// A rotation and translation as a column-vector matrix.
fn bone_matrix(t: [f32; 3], q: [f32; 4]) -> Float4x4 {
    let len = (q[0] * q[0] + q[1] * q[1] + q[2] * q[2] + q[3] * q[3]).sqrt().max(1e-6);
    let [x, y, z, w] = q.map(|v| v / len);
    let mut m = Float4x4::IDENTITY;
    m.m[0] = [1.0 - 2.0 * (y * y + z * z), 2.0 * (x * y - z * w), 2.0 * (x * z + y * w), t[0]];
    m.m[1] = [2.0 * (x * y + z * w), 1.0 - 2.0 * (x * x + z * z), 2.0 * (y * z - x * w), t[1]];
    m.m[2] = [2.0 * (x * z - y * w), 2.0 * (y * z + x * w), 1.0 - 2.0 * (x * x + y * y), t[2]];
    m
}

/// The first frame of a `.anim`; `None` when it isn't one this reads.
pub fn first_frame(bytes: &[u8]) -> Option<Pose> {
    let anim = wce_formats::anim::Animation::decode(bytes, 1)?;
    let mut bones: Vec<Float4x4> = Vec::with_capacity(anim.parents.len());
    for (i, (t, q)) in anim.frames[0].iter().enumerate() {
        let local = bone_matrix(*t, *q);
        let world = match usize::try_from(anim.parents[i]).ok().filter(|&p| p < i) {
            Some(p) => bones[p] * local,
            None => local,
        };

        bones.push(world);
    }

    Some(Pose { bones })
}

/// The inverse bind matrices of a `.bone_inv_trans_mats`, as column-vector matrices.
pub fn inverse_binds(bytes: &[u8]) -> Option<Vec<Float4x4>> {
    let mut r = Reader { b: bytes, p: 0 };
    r.u32()?;
    let n = r.u32()? as usize;
    (0..n).map(|_| {
        let f = (0..12).map(|_| r.f32()).collect::<Option<Vec<_>>>()?;
        let mut m = Float4x4::IDENTITY;
        for c in 0..4 {
            for row in 0..3 {
                m.m[row][c] = f[c * 3 + row];
            }
        }

        Some(m)
    }).collect()
}

/// The skinning matrices of a pose: per bone its world matrix times its inverse bind (the bind pose's inverse from the pose itself when the
/// mesh has no bind file: a rigid mesh is in its bone's frame, so no bind is applied).
pub fn skin_matrices(pose: &Pose, binds: Option<&[Float4x4]>) -> Vec<Float4x4> {
    pose.bones.iter().enumerate().map(|(i, w)| match binds.and_then(|b| b.get(i)) {
        Some(inv) => *w * *inv,
        None => *w,
    }).collect()
}

/// For checks: how far the skinning matrices are from the identity (0: the first frame is the bind pose).
pub fn distance_from_bind(skin: &[Float4x4]) -> f32 {
    let id = Float4x4::IDENTITY;
    skin.iter().flat_map(|m| (0..4).flat_map(move |r| (0..4).map(move |c| (m.m[r][c] - id.m[r][c]).abs()))).fold(0.0, f32::max)
}

#[cfg(test)]
mod tests {
    use super::*;
    use wce_foundation::math::inverse;

    #[test]
    fn bone_matrix_turns_about_x() {
        // -90 degrees about x turns (0, 0, 1) into (0, 1, 0); then the translation.
        let s = std::f32::consts::FRAC_1_SQRT_2;
        let m = bone_matrix([1.0, 2.0, 3.0], [-s, 0.0, 0.0, s]);
        let p = m.transform_point(wce_foundation::math::Float3::new(0.0, 0.0, 1.0));
        assert!((p.x - 1.0).abs() < 1e-5 && (p.y - 3.0).abs() < 1e-5 && (p.z - 3.0).abs() < 1e-5, "{p:?}");
        let back = inverse(&m) * m;
        assert!(distance_from_bind(&[back]) < 1e-5);
    }
}
