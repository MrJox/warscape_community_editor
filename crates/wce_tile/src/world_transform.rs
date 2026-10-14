//! World transforms of tile.ted nodes, for display and editing (double precision, not the exporter's float32 path).
//!
//! Convention measured against TEd's building export (docs/formats/tile_ted.md): a node's orientation euler(a,b,c) is the rotation
//! Rz(c) * Ry(a) * Rx(-b) (column vectors); parents compose before children; positions are rotated by the parent rotation and added. Scene
//! positions are in half metres horizontally (unit scale 2), heights in metres.

use crate::ted_scene::{TedScene, Vec3};

/// Row-major.
pub type Mat3 = [f64; 9];

pub const IDENTITY: Mat3 = [1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0];

fn rot_x(a: f64) -> Mat3 {
    let (c, s) = (a.cos(), a.sin());
    [1.0, 0.0, 0.0, 0.0, c, -s, 0.0, s, c]
}

fn rot_y(a: f64) -> Mat3 {
    let (c, s) = (a.cos(), a.sin());
    [c, 0.0, s, 0.0, 1.0, 0.0, -s, 0.0, c]
}

fn rot_z(a: f64) -> Mat3 {
    let (c, s) = (a.cos(), a.sin());
    [c, -s, 0.0, s, c, 0.0, 0.0, 0.0, 1.0]
}

pub fn mat_mul(a: &Mat3, b: &Mat3) -> Mat3 {
    let mut r = [0.0; 9];
    for i in 0..3 {
        for j in 0..3 {
            r[i * 3 + j] = a[i * 3] * b[j] + a[i * 3 + 1] * b[3 + j] + a[i * 3 + 2] * b[6 + j];
        }
    }

    r
}

pub fn rotate_vec(m: &Mat3, v: Vec3) -> Vec3 {
    [m[0] * v[0] + m[1] * v[1] + m[2] * v[2], m[3] * v[0] + m[4] * v[1] + m[5] * v[2], m[6] * v[0] + m[7] * v[1] + m[8] * v[2]]
}

pub fn mat_transpose(m: &Mat3) -> Mat3 {
    [m[0], m[3], m[6], m[1], m[4], m[7], m[2], m[5], m[8]]
}

/// Rz(c) * Ry(a) * Rx(-b).
pub fn euler_matrix(e: Vec3) -> Mat3 {
    mat_mul(&mat_mul(&rot_z(e[2]), &rot_y(e[0])), &rot_x(-e[1]))
}

pub fn rotation_y(angle: f64) -> Mat3 {
    rot_y(angle)
}

/// Euler angles (a, b, c) with `euler_matrix(e) == m`; gimbal lock resolves with c = 0.
pub fn euler_from_matrix(m: &Mat3) -> Vec3 {
    // m = Rz(c) Ry(a) Rx(beta), beta = -b: m[6] = -sin a, m[7] = sin(beta) cos a, m[8] = cos(beta) cos a, m[3] = sin c cos a, m[0] = cos c cos a.
    #[allow(clippy::manual_clamp, reason = "a NaN becomes 1 as in the C++ max(-1, min(1, x)); clamp would keep the NaN")]
    let sa = (-m[6]).min(1.0).max(-1.0);
    let a = sa.asin();
    let (beta, c) = if sa.abs() < 1.0 - 1e-12 { (m[7].atan2(m[8]), m[3].atan2(m[0])) } else { ((-m[5]).atan2(m[4]), 0.0) };
    [a, -beta, c]
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct WorldTransform {
    pub position: Vec3,
    pub rotation: Mat3,
}

impl Default for WorldTransform {
    fn default() -> Self {
        Self { position: [0.0; 3], rotation: IDENTITY }
    }
}

/// Position / rotation of `elem` in scene space (all ancestors applied).
pub fn world_transform(scene: &TedScene, elem: usize) -> WorldTransform {
    let mut chain = vec![];
    let mut e = Some(elem);
    while let Some(x) = e {
        chain.push(x);
        e = scene.doc().elements()[x].parent;
    }

    let mut w = WorldTransform::default();
    for &e in chain.iter().rev() {
        if let Some(p) = scene.position(e) {
            let r = rotate_vec(&w.rotation, p);
            for (a, b) in w.position.iter_mut().zip(r) {
                *a += b;
            }
        }

        if let Some(o) = scene.euler(e) {
            w.rotation = mat_mul(&w.rotation, &euler_matrix(o));
        }
    }

    w
}

fn parent_transform(scene: &TedScene, elem: usize) -> WorldTransform {
    scene.doc().elements()[elem].parent.map(|p| world_transform(scene, p)).unwrap_or_default()
}

/// Sets the local orientation of `elem` so that its world rotation becomes `world` (ancestors unchanged).
pub fn set_world_rotation(scene: &mut TedScene, elem: usize, world: &Mat3) {
    let pw = parent_transform(scene, elem);
    scene.set_euler(elem, euler_from_matrix(&mat_mul(&mat_transpose(&pw.rotation), world)));
}

/// Sets the local orientation of `elem` so that its world rotation becomes Ry(delta) * (current world rotation).
pub fn rotate_world_yaw(scene: &mut TedScene, elem: usize, delta: f64) {
    let pw = parent_transform(scene, elem);
    let local = euler_matrix(scene.euler(elem).unwrap_or([0.0; 3]));
    let world = mat_mul(&pw.rotation, &local);
    let rotated = mat_mul(&rot_y(delta), &world);
    scene.set_euler(elem, euler_from_matrix(&mat_mul(&mat_transpose(&pw.rotation), &rotated)));
}

/// Sets the local position of `elem` so that its world position becomes `world` (ancestors unchanged). The `children` wrappers have no
/// position or orientation, so walking real ancestors is enough.
pub fn set_world_position(scene: &mut TedScene, elem: usize, world: Vec3) {
    let pw = parent_transform(scene, elem);
    let d = [world[0] - pw.position[0], world[1] - pw.position[1], world[2] - pw.position[2]];
    scene.set_position(elem, rotate_vec(&mat_transpose(&pw.rotation), d));
}
