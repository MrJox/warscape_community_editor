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

/// TEd's / BOB's euler to rotation in f32 (`FUN_100af1e0` of QtToolUtility, docs/re/vegetation.md): a row-major 4 x 4 matrix, the
/// translation (0) in the last column.
pub fn ted_rotation_f32(e: [f32; 3]) -> [f32; 16] {
    let sincos = |a: f32| ((f64::from(a)).sin() as f32, (f64::from(a)).cos() as f32);
    let (s0, c0) = sincos(e[0]);
    let f1 = -s0;
    let (s1, c1) = sincos(-e[1]);
    let mut f2 = (1.0 - c1) * c0;
    let f3 = f2 * c0 + c1;
    let mut f6 = f1 * s1;
    let f4 = -f6;
    let f5 = f1 * (1.0 - c1) * f1 + c1;
    f2 *= f1;
    let f7 = s1 * c0;
    let l14 = f2 * s0;
    let f11 = f3 * c0 - l14;
    let f8 = -f7;
    let f9 = f6 * c0 - f8 * s0;
    f2 *= c0;
    let f10 = f2 - f5 * s0;
    let f1b = f5 * c0 + l14;
    let f2b = f3 * s0 + f2;
    f6 = f8 * c0 + f6 * s0;
    let (s2, c2) = sincos(e[2]);
    let f3b = -s2;
    let mut m = [0.0f32; 16];
    m[0] = f3b * f9 + f11 * c2;
    m[4] = f11 * s2 + f9 * c2;
    m[8] = f10;
    m[1] = f3b * c1 + c2 * f4;
    m[5] = s2 * f4 + c2 * c1;
    m[9] = f7;
    m[2] = f3b * f6 + f2b * c2;
    m[6] = f6 * c2 + f2b * s2;
    m[10] = f1b;
    m[15] = 1.0;
    m
}

/// `QTU::TedEntity::world_transform`'s product in f32: the parent's matrix times the child's, each element summed left to right.
pub fn ted_mul_f32(w: &[f32; 16], local: &[f32; 16]) -> [f32; 16] {
    let mut r = [0.0f32; 16];
    for i in 0..4 {
        for j in 0..4 {
            r[i * 4 + j] = w[i * 4] * local[j] + w[i * 4 + 1] * local[4 + j] + w[i * 4 + 2] * local[8 + j] + w[i * 4 + 3] * local[12 + j];
        }
    }

    r
}

/// `QTU::TedEntity::world_transform` in BOB's float arithmetic (QtToolUtility): each element's local matrix from its `position` and
/// `orientation` (`sscanf` floats; [`ted_rotation_f32`]; unit scale), the world matrix the parent's times the local one ([`ted_mul_f32`]).
pub fn ted_world_matrix_f32(scene: &TedScene, elem: usize) -> [f32; 16] {
    let doc = scene.doc();
    let vec3 = |e: usize, attr: &str, prefix: &str| -> [f32; 3] {
        let parse = || -> Option<[f32; 3]> {
            let v = doc.get(e, attr)?;
            let inner = v.trim().strip_prefix(prefix)?.strip_prefix('(')?.strip_suffix(')')?;
            let mut it = inner.split(',').map(|t| t.trim().parse::<f64>().ok().map(|f| f as f32));
            Some([it.next()??, it.next()??, it.next()??])
        };
        parse().unwrap_or([0.0; 3])
    };
    let mut chain = vec![];
    let mut e = Some(elem);
    while let Some(x) = e {
        chain.push(x);
        e = doc.elements()[x].parent;
    }

    let mut world: Option<[f32; 16]> = None;
    for &x in chain.iter().rev() {
        if doc.get(x, "position").is_none() && doc.get(x, "orientation").is_none() {
            continue;
        }

        let mut local = ted_rotation_f32(vec3(x, "orientation", "euler"));
        let p = vec3(x, "position", "vector");
        (local[3], local[7], local[11]) = (p[0], p[1], p[2]);
        world = Some(match world {
            None => local,
            Some(w) => ted_mul_f32(&w, &local),
        });
    }

    world.unwrap_or(ted_rotation_f32([0.0; 3]))
}
