//! Minimal math for the renderer.
//!
//! Convention (from the game's vertex shaders: `clip.i = dp4(pos, cb[row i])`): matrices are stored row-major with column vectors,
//! `p' = M * p`, and uploaded as they are, one row per 16-byte register. The world is Y-up; depth is standard D3D (0 near, 1 far;
//! the sky is drawn at z = w).
//!
//! The operations keep the C++ editor's order of floating-point operations, so both give the same bits.
//!
//! The renderer works in single precision ([`Float3`], [`Float4x4`]); tile scenes in double precision ([`Double3`], [`Double3x3`]), as TEd
//! does, with the same conventions.

use std::ops::{Add, AddAssign, Div, Index, IndexMut, Mul, Neg, Sub};

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

/// A double-precision vector: tile scene positions, offsets and euler angles.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Double3 {
    pub x: f64,
    pub y: f64,
    pub z: f64,
}

impl Double3 {
    pub const ZERO: Self = Self::new(0.0, 0.0, 0.0);

    pub const fn new(x: f64, y: f64, z: f64) -> Self {
        Self { x, y, z }
    }

    pub const fn to_array(self) -> [f64; 3] {
        [self.x, self.y, self.z]
    }

    /// The components changed by `f`, each on its own.
    pub fn map(self, f: impl Fn(f64) -> f64) -> Self {
        Self::new(f(self.x), f(self.y), f(self.z))
    }

    /// The largest absolute difference of a component.
    pub fn max_abs_diff(self, o: Self) -> f64 {
        (self.x - o.x).abs().max((self.y - o.y).abs()).max((self.z - o.z).abs())
    }
}

impl From<[f64; 3]> for Double3 {
    fn from([x, y, z]: [f64; 3]) -> Self {
        Self::new(x, y, z)
    }
}

impl From<Double3> for [f64; 3] {
    fn from(v: Double3) -> Self {
        v.to_array()
    }
}

/// Component `i` (0 = x).
impl Index<usize> for Double3 {
    type Output = f64;
    fn index(&self, i: usize) -> &f64 {
        match i {
            0 => &self.x,
            1 => &self.y,
            2 => &self.z,
            _ => panic!("Double3 has 3 components, not {}", i + 1),
        }
    }
}

impl IndexMut<usize> for Double3 {
    fn index_mut(&mut self, i: usize) -> &mut f64 {
        match i {
            0 => &mut self.x,
            1 => &mut self.y,
            2 => &mut self.z,
            _ => panic!("Double3 has 3 components, not {}", i + 1),
        }
    }
}

impl Add for Double3 {
    type Output = Self;
    fn add(self, o: Self) -> Self {
        Self::new(self.x + o.x, self.y + o.y, self.z + o.z)
    }
}

impl AddAssign for Double3 {
    fn add_assign(&mut self, o: Self) {
        *self = *self + o;
    }
}

impl Sub for Double3 {
    type Output = Self;
    fn sub(self, o: Self) -> Self {
        Self::new(self.x - o.x, self.y - o.y, self.z - o.z)
    }
}

impl Mul<f64> for Double3 {
    type Output = Self;
    fn mul(self, s: f64) -> Self {
        Self::new(self.x * s, self.y * s, self.z * s)
    }
}

impl Div<f64> for Double3 {
    type Output = Self;
    fn div(self, s: f64) -> Self {
        Self::new(self.x / s, self.y / s, self.z / s)
    }
}

impl Neg for Double3 {
    type Output = Self;
    fn neg(self) -> Self {
        Self::new(-self.x, -self.y, -self.z)
    }
}

/// A 3x3 double-precision matrix, row-major, for column vectors (a rotation of the tile scene).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Double3x3 {
    pub m: [[f64; 3]; 3],
}

impl Default for Double3x3 {
    fn default() -> Self {
        Self::IDENTITY
    }
}

impl Double3x3 {
    pub const IDENTITY: Self = Self { m: [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]] };

    pub fn rotation_x(a: f64) -> Self {
        let (c, s) = (a.cos(), a.sin());
        Self { m: [[1.0, 0.0, 0.0], [0.0, c, -s], [0.0, s, c]] }
    }

    pub fn rotation_y(a: f64) -> Self {
        let (c, s) = (a.cos(), a.sin());
        Self { m: [[c, 0.0, s], [0.0, 1.0, 0.0], [-s, 0.0, c]] }
    }

    pub fn rotation_z(a: f64) -> Self {
        let (c, s) = (a.cos(), a.sin());
        Self { m: [[c, -s, 0.0], [s, c, 0.0], [0.0, 0.0, 1.0]] }
    }

    /// The matrix with these columns.
    pub fn from_cols(x: Double3, y: Double3, z: Double3) -> Self {
        Self { m: [[x.x, y.x, z.x], [x.y, y.y, z.y], [x.z, y.z, z.z]] }
    }

    pub fn col(&self, j: usize) -> Double3 {
        Double3::new(self.m[0][j], self.m[1][j], self.m[2][j])
    }

    /// The 9 values, row by row.
    pub fn data(&self) -> &[f64; 9] {
        self.m.as_flattened().try_into().unwrap_or(&[0.0; 9])
    }

    /// The inverse of a rotation.
    pub fn transpose(&self) -> Self {
        let m = &self.m;
        Self { m: [[m[0][0], m[1][0], m[2][0]], [m[0][1], m[1][1], m[2][1]], [m[0][2], m[1][2], m[2][2]]] }
    }
}

impl Mul for Double3x3 {
    type Output = Self;
    fn mul(self, o: Self) -> Self {
        let (a, b) = (&self.m, &o.m);
        Self { m: std::array::from_fn(|i| std::array::from_fn(|j| a[i][0] * b[0][j] + a[i][1] * b[1][j] + a[i][2] * b[2][j])) }
    }
}

impl Mul<Double3> for Double3x3 {
    type Output = Double3;
    fn mul(self, v: Double3) -> Double3 {
        let m = &self.m;
        Double3::new(
            m[0][0] * v.x + m[0][1] * v.y + m[0][2] * v.z,
            m[1][0] * v.x + m[1][1] * v.y + m[1][2] * v.z,
            m[2][0] * v.x + m[2][1] * v.y + m[2][2] * v.z,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn double_rotations() {
        let (a, b) = (Double3x3::rotation_y(0.3), Double3x3::rotation_z(-1.2));
        let ab = a * b;
        let v = Double3::new(1.0, 2.0, 3.0);

        // (A B) v = A (B v), and a rotation's transpose undoes it.
        assert!((ab * v).max_abs_diff(a * (b * v)) < 1e-12);
        assert!((ab.transpose() * (ab * v)).max_abs_diff(v) < 1e-12);
        assert_eq!(Double3x3::IDENTITY * v, v);
        assert_eq!(Double3x3::from_cols(ab.col(0), ab.col(1), ab.col(2)), ab);

        // Rz(90 degrees) takes x to y.
        assert!((Double3x3::rotation_z(std::f64::consts::FRAC_PI_2) * Double3::new(1.0, 0.0, 0.0)).max_abs_diff(Double3::new(0.0, 1.0, 0.0)) < 1e-15);
    }

    #[test]
    fn double_components() {
        let mut v = Double3::from([1.0, -2.0, 4.0]);
        v[1] = 5.0;
        v += Double3::new(1.0, 1.0, 1.0);
        assert_eq!(v.to_array(), [2.0, 6.0, 5.0]);
        assert_eq!((-v * 0.5 - v).map(f64::abs), Double3::new(3.0, 9.0, 7.5));
    }
}
