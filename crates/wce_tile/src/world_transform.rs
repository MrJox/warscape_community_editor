//! World transforms of tile.ted nodes, for display and editing (double precision, not the exporter's float32 path).
//!
//! Convention measured against TEd's building export (docs/formats/tile_ted.md): a node's orientation euler(a,b,c) is the rotation
//! Rz(c) * Ry(a) * Rx(-b) (column vectors); parents compose before children; positions are rotated by the parent rotation and added. Scene
//! positions are in half metres horizontally (unit scale 2), heights in metres.

use wce_foundation::math::{Double3, Double3x3};

use crate::ted_scene::TedScene;

/// Rz(c) * Ry(a) * Rx(-b).
pub fn euler_matrix(e: Double3) -> Double3x3 {
    Double3x3::rotation_z(e.z) * Double3x3::rotation_y(e.x) * Double3x3::rotation_x(-e.y)
}

/// Euler angles (a, b, c) with `euler_matrix(e) == m`; gimbal lock resolves with c = 0.
pub fn euler_from_matrix(m: &Double3x3) -> Double3 {
    // m = Rz(c) Ry(a) Rx(beta), beta = -b: m20 = -sin a, m21 = sin(beta) cos a, m22 = cos(beta) cos a, m10 = sin c cos a, m00 = cos c cos a.
    let m = &m.m;
    #[allow(clippy::manual_clamp, reason = "a NaN becomes 1 as in the C++ max(-1, min(1, x)); clamp would keep the NaN")]
    let sa = (-m[2][0]).min(1.0).max(-1.0);
    let a = sa.asin();
    let (beta, c) = if sa.abs() < 1.0 - 1e-12 { (m[2][1].atan2(m[2][2]), m[1][0].atan2(m[0][0])) } else { ((-m[1][2]).atan2(m[1][1]), 0.0) };
    Double3::new(a, -beta, c)
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct WorldTransform {
    pub position: Double3,
    pub rotation: Double3x3,
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
            w.position += w.rotation * p;
        }

        if let Some(o) = scene.euler(e) {
            w.rotation = w.rotation * euler_matrix(o);
        }
    }

    w
}

fn parent_transform(scene: &TedScene, elem: usize) -> WorldTransform {
    scene.doc().elements()[elem].parent.map(|p| world_transform(scene, p)).unwrap_or_default()
}

/// Sets the local orientation of `elem` so that its world rotation becomes `world` (ancestors unchanged).
pub fn set_world_rotation(scene: &mut TedScene, elem: usize, world: &Double3x3) {
    let pw = parent_transform(scene, elem);
    scene.set_euler(elem, euler_from_matrix(&(pw.rotation.transpose() * *world)));
}

/// Sets the local orientation of `elem` so that its world rotation becomes Ry(delta) * (current world rotation).
pub fn rotate_world_yaw(scene: &mut TedScene, elem: usize, delta: f64) {
    let pw = parent_transform(scene, elem);
    let local = euler_matrix(scene.euler(elem).unwrap_or_default());
    let world = pw.rotation * local;
    let rotated = Double3x3::rotation_y(delta) * world;
    scene.set_euler(elem, euler_from_matrix(&(pw.rotation.transpose() * rotated)));
}

/// Sets the local position of `elem` so that its world position becomes `world` (ancestors unchanged). The `children` wrappers have no
/// position or orientation, so walking real ancestors is enough.
pub fn set_world_position(scene: &mut TedScene, elem: usize, world: Double3) {
    let pw = parent_transform(scene, elem);
    scene.set_position(elem, pw.rotation.transpose() * (world - pw.position));
}
