//! Minimal math for the renderer.
//!
//! Convention (from the game's vertex shaders: `clip.i = dp4(pos, cb[row i])`): matrices are stored row-major with column vectors,
//! `p' = M * p`, and uploaded as they are, one row per 16-byte register. The world is Y-up; depth is standard D3D (0 near, 1 far;
//! the sky is drawn at z = w).
//!
//! The operations keep the C++ editor's order of floating-point operations, so both give the same bits.

use std::ops::{Add, Mul, Neg, Sub};

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Float2 {
    pub x: f32,
    pub y: f32,
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Float3 {
    pub x: f32,
    pub y: f32,
    pub z: f32,
}

impl Float3 {
    pub const fn new(x: f32, y: f32, z: f32) -> Self {
        Self { x, y, z }
    }
}

impl Add for Float3 {
    type Output = Self;
    fn add(self, o: Self) -> Self {
        Self::new(self.x + o.x, self.y + o.y, self.z + o.z)
    }
}

impl Sub for Float3 {
    type Output = Self;
    fn sub(self, o: Self) -> Self {
        Self::new(self.x - o.x, self.y - o.y, self.z - o.z)
    }
}

impl Mul<f32> for Float3 {
    type Output = Self;
    fn mul(self, s: f32) -> Self {
        Self::new(self.x * s, self.y * s, self.z * s)
    }
}

impl Neg for Float3 {
    type Output = Self;
    fn neg(self) -> Self {
        Self::new(-self.x, -self.y, -self.z)
    }
}

pub fn dot(a: Float3, b: Float3) -> f32 {
    a.x * b.x + a.y * b.y + a.z * b.z
}

pub fn cross(a: Float3, b: Float3) -> Float3 {
    Float3::new(a.y * b.z - a.z * b.y, a.z * b.x - a.x * b.z, a.x * b.y - a.y * b.x)
}

pub fn length(a: Float3) -> f32 {
    dot(a, a).sqrt()
}

/// The vector scaled to length 1; a zero vector stays zero.
pub fn normalize(a: Float3) -> Float3 {
    let l = length(a);
    if l > 0.0 { a * (1.0 / l) } else { a }
}

/// A 4x4 matrix, row-major, for column vectors.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Float4x4 {
    pub m: [[f32; 4]; 4],
}

impl Default for Float4x4 {
    fn default() -> Self {
        Self::IDENTITY
    }
}

impl Float4x4 {
    pub const IDENTITY: Self = Self { m: [[1.0, 0.0, 0.0, 0.0], [0.0, 1.0, 0.0, 0.0], [0.0, 0.0, 1.0, 0.0], [0.0, 0.0, 0.0, 1.0]] };

    /// The 16 floats, row by row, as uploaded.
    pub fn data(&self) -> &[f32; 16] {
        self.m.as_flattened().try_into().unwrap_or(&[0.0; 16])
    }

    /// `M * p` with the perspective divide.
    pub fn transform_point(&self, p: Float3) -> Float3 {
        let m = &self.m;
        let w = m[3][0] * p.x + m[3][1] * p.y + m[3][2] * p.z + m[3][3];
        Float3::new(
            m[0][0] * p.x + m[0][1] * p.y + m[0][2] * p.z + m[0][3],
            m[1][0] * p.x + m[1][1] * p.y + m[1][2] * p.z + m[1][3],
            m[2][0] * p.x + m[2][1] * p.y + m[2][2] * p.z + m[2][3],
        ) * (1.0 / w)
    }
}

impl Mul for Float4x4 {
    type Output = Self;
    fn mul(self, o: Self) -> Self {
        let mut r = Self::IDENTITY;
        for i in 0..4 {
            for j in 0..4 {
                r.m[i][j] = 0.0;
                for k in 0..4 {
                    r.m[i][j] += self.m[i][k] * o.m[k][j];
                }
            }
        }

        r
    }
}

/// The inverse by cofactors; all zeros for a singular matrix.
pub fn inverse(a: &Float4x4) -> Float4x4 {
    let m = a.data();
    let mut inv = [0.0f32; 16];
    inv[0] = m[5] * m[10] * m[15] - m[5] * m[11] * m[14] - m[9] * m[6] * m[15] + m[9] * m[7] * m[14] + m[13] * m[6] * m[11] - m[13] * m[7] * m[10];
    inv[4] = -m[4] * m[10] * m[15] + m[4] * m[11] * m[14] + m[8] * m[6] * m[15] - m[8] * m[7] * m[14] - m[12] * m[6] * m[11] + m[12] * m[7] * m[10];
    inv[8] = m[4] * m[9] * m[15] - m[4] * m[11] * m[13] - m[8] * m[5] * m[15] + m[8] * m[7] * m[13] + m[12] * m[5] * m[11] - m[12] * m[7] * m[9];
    inv[12] = -m[4] * m[9] * m[14] + m[4] * m[10] * m[13] + m[8] * m[5] * m[14] - m[8] * m[6] * m[13] - m[12] * m[5] * m[10] + m[12] * m[6] * m[9];
    inv[1] = -m[1] * m[10] * m[15] + m[1] * m[11] * m[14] + m[9] * m[2] * m[15] - m[9] * m[3] * m[14] - m[13] * m[2] * m[11] + m[13] * m[3] * m[10];
    inv[5] = m[0] * m[10] * m[15] - m[0] * m[11] * m[14] - m[8] * m[2] * m[15] + m[8] * m[3] * m[14] + m[12] * m[2] * m[11] - m[12] * m[3] * m[10];
    inv[9] = -m[0] * m[9] * m[15] + m[0] * m[11] * m[13] + m[8] * m[1] * m[15] - m[8] * m[3] * m[13] - m[12] * m[1] * m[11] + m[12] * m[3] * m[9];
    inv[13] = m[0] * m[9] * m[14] - m[0] * m[10] * m[13] - m[8] * m[1] * m[14] + m[8] * m[2] * m[13] + m[12] * m[1] * m[10] - m[12] * m[2] * m[9];
    inv[2] = m[1] * m[6] * m[15] - m[1] * m[7] * m[14] - m[5] * m[2] * m[15] + m[5] * m[3] * m[14] + m[13] * m[2] * m[7] - m[13] * m[3] * m[6];
    inv[6] = -m[0] * m[6] * m[15] + m[0] * m[7] * m[14] + m[4] * m[2] * m[15] - m[4] * m[3] * m[14] - m[12] * m[2] * m[7] + m[12] * m[3] * m[6];
    inv[10] = m[0] * m[5] * m[15] - m[0] * m[7] * m[13] - m[4] * m[1] * m[15] + m[4] * m[3] * m[13] + m[12] * m[1] * m[7] - m[12] * m[3] * m[5];
    inv[14] = -m[0] * m[5] * m[14] + m[0] * m[6] * m[13] + m[4] * m[1] * m[14] - m[4] * m[2] * m[13] - m[12] * m[1] * m[6] + m[12] * m[2] * m[5];
    inv[3] = -m[1] * m[6] * m[11] + m[1] * m[7] * m[10] + m[5] * m[2] * m[11] - m[5] * m[3] * m[10] - m[9] * m[2] * m[7] + m[9] * m[3] * m[6];
    inv[7] = m[0] * m[6] * m[11] - m[0] * m[7] * m[10] - m[4] * m[2] * m[11] + m[4] * m[3] * m[10] + m[8] * m[2] * m[7] - m[8] * m[3] * m[6];
    inv[11] = -m[0] * m[5] * m[11] + m[0] * m[7] * m[9] + m[4] * m[1] * m[11] - m[4] * m[3] * m[9] - m[8] * m[1] * m[7] + m[8] * m[3] * m[5];
    inv[15] = m[0] * m[5] * m[10] - m[0] * m[6] * m[9] - m[4] * m[1] * m[10] + m[4] * m[2] * m[9] + m[8] * m[1] * m[6] - m[8] * m[2] * m[5];
    let det = m[0] * inv[0] + m[1] * inv[4] + m[2] * inv[8] + m[3] * inv[12];
    let id = if det != 0.0 { 1.0 / det } else { 0.0 };
    let mut r = Float4x4::IDENTITY;
    for (i, value) in inv.iter().enumerate() {
        r.m[i / 4][i % 4] = value * id;
    }

    r
}

pub fn translation(t: Float3) -> Float4x4 {
    let mut r = Float4x4::IDENTITY;
    r.m[0][3] = t.x;
    r.m[1][3] = t.y;
    r.m[2][3] = t.z;
    r
}

pub fn rotation_x(a: f32) -> Float4x4 {
    let (s, c) = (a.sin(), a.cos());
    let mut r = Float4x4::IDENTITY;
    r.m[1][1] = c;
    r.m[1][2] = -s;
    r.m[2][1] = s;
    r.m[2][2] = c;
    r
}

pub fn rotation_y(a: f32) -> Float4x4 {
    let (s, c) = (a.sin(), a.cos());
    let mut r = Float4x4::IDENTITY;
    r.m[0][0] = c;
    r.m[0][2] = s;
    r.m[2][0] = -s;
    r.m[2][2] = c;
    r
}

pub fn rotation_z(a: f32) -> Float4x4 {
    let (s, c) = (a.sin(), a.cos());
    let mut r = Float4x4::IDENTITY;
    r.m[0][0] = c;
    r.m[0][1] = -s;
    r.m[1][0] = s;
    r.m[1][1] = c;
    r
}

pub fn scaling(s: f32) -> Float4x4 {
    let mut r = Float4x4::IDENTITY;
    r.m[0][0] = s;
    r.m[1][1] = s;
    r.m[2][2] = s;
    r
}

/// Left-handed look-at: the camera looks down +Z in view space.
pub fn look_at_lh(eye: Float3, target: Float3, up: Float3) -> Float4x4 {
    let z = normalize(target - eye);
    let x = normalize(cross(up, z));
    let y = cross(z, x);
    let mut r = Float4x4::IDENTITY;
    r.m[0] = [x.x, x.y, x.z, -dot(x, eye)];
    r.m[1] = [y.x, y.y, y.z, -dot(y, eye)];
    r.m[2] = [z.x, z.y, z.z, -dot(z, eye)];
    r
}

/// D3D-style perspective projection (depth 0..1), left-handed.
pub fn perspective_lh(fov_y: f32, aspect: f32, zn: f32, zf: f32) -> Float4x4 {
    let ys = 1.0 / (fov_y * 0.5).tan();
    let xs = ys / aspect;
    let mut r = Float4x4::IDENTITY;
    r.m[0][0] = xs;
    r.m[1][1] = ys;
    r.m[2][2] = zf / (zf - zn);
    r.m[2][3] = -zn * zf / (zf - zn);
    r.m[3][2] = 1.0;
    r.m[3][3] = 0.0;
    r
}

/// D3D-style orthographic projection (depth 0..1), left-handed.
pub fn ortho_lh(w: f32, h: f32, zn: f32, zf: f32) -> Float4x4 {
    let mut r = Float4x4::IDENTITY;
    r.m[0][0] = 2.0 / w;
    r.m[1][1] = 2.0 / h;
    r.m[2][2] = 1.0 / (zf - zn);
    r.m[2][3] = -zn / (zf - zn);
    r
}
