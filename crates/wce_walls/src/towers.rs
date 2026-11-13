//! The capture points of a wall's towers (docs/plans/kit_walls.md, follow-up 8). In the game a tower fires for whoever holds the capture
//! location its building names (`capture_location`, as vanilla's `*_fort_tower_c_*` name theirs: a `capture_point_tower_only` location,
//! `CLT_MINOR`). A wall's towers (its pieces whose key says `tower`: the 15 m sets' tower sections and corners, Gondor's curved towers,
//! the towers that end the 15 m walls) each get one, made with the wall: its boundary a rectangle over the top of the wall behind the tower
//! (the tower piece's walkway, from joint to joint, as wide as the walk; a tower ending a wall: the walk of the next piece, a straight
//! long), at the walk's height.
//!
//! Scene units, as the pieces (`pieces::Piece`: origin, yaw).

use wce_tile::new_instances::NewInstance;

use crate::pieces::Piece;

/// A tower's capture location: its name (what the tower's `capture_location` says), the tower's index among the wall's pieces, its place
/// (the rectangle's middle, at the walk's height) and its boundary (x, z, the world).
#[derive(Clone, Debug, PartialEq)]
pub struct TowerCapture {
    pub name: String,
    pub piece: usize,
    pub at: [f64; 3],
    pub ring: Vec<[f64; 2]>,
}

/// Whether a piece is a tower (its key says so; not a bastion, not a watchtower standing apart, which no wall lays).
#[must_use]
pub fn is_tower(key: &str) -> bool {
    let k = key.to_lowercase();
    k.contains("tower") && !k.contains("watchtower")
}

/// The name of a wall's `n`-th tower's capture location: unique in the tile (`wall_key`: `scene::wall_key`).
#[must_use]
pub fn capture_name(wall_key: &str, n: usize) -> String {
    format!("wce_wall{}_tower{}", wall_key.replace(|c: char| !c.is_ascii_alphanumeric(), "_"), n + 1)
}

/// How far a tower ending a wall's rectangle goes along the wall from its joint (scene units: a straight of the 15 m sets).
const CAP_REACH: f64 = 20.0;

/// Half the walk's width where a piece's logic file says nothing of it (scene units).
const HALF_WALK: f64 = 3.0;

/// The rectangle over the walk behind a tower placed at `origin` turned by `yaw` (its logic file `logic`): x, z of its corners (the
/// world), and the walk's height there. `None` without joints.
#[must_use]
pub fn tower_ring(logic: &[u8], origin: [f64; 3], yaw: f64) -> Option<(Vec<[f64; 2]>, f64)> {
    let joints = crate::kit::joints(logic);
    let edges = crate::kit::joint_edges(logic, &joints);
    let (sn, cs) = yaw.sin_cos();
    let world = |x: f64, z: f64| [origin[0] + x * cs - z * sn, origin[2] + x * sn + z * cs];

    // Across each joint: the walk's edges (at its height), else a walk of the usual width.
    let band = |k: usize| {
        let walk = edges.get(k).into_iter().flatten().filter(|e| e.y_lo.abs() <= 0.1 && e.y_hi.abs() <= 0.1).fold(None, |b: Option<(f64, f64)>, e| {
            let (lo, hi) = (f64::from(e.lo), f64::from(e.hi));
            Some(b.map_or((lo, hi), |(a, c)| (a.min(lo), c.max(hi))))
        });
        walk.unwrap_or((-HALF_WALK, HALF_WALK))
    };
    let side = |k: usize| {
        let j = &joints[k];
        let at = [f64::from(j.at[0]), f64::from(j.at[2])];
        let out = [f64::from(j.out[0]), f64::from(j.out[1])];
        (at, out, [-out[1], out[0]])
    };

    let mut pts = vec![];
    match joints.len() {
        0 => return None,
        1 => {
            // A tower ending the wall: the walk of the wall it ends, along the way out of its joint.
            let (at, out, t) = side(0);
            let (lo, hi) = band(0);
            for (d, w) in [(0.0, lo), (0.0, hi), (CAP_REACH, hi), (CAP_REACH, lo)] {
                pts.push([at[0] + out[0] * d + t[0] * w, at[1] + out[1] * d + t[1] * w]);
            }
        }

        _ => {
            for k in 0..2 {
                let (at, _, t) = side(k);
                let (lo, hi) = band(k);
                pts.push([at[0] + t[0] * lo, at[1] + t[1] * lo]);
                pts.push([at[0] + t[0] * hi, at[1] + t[1] * hi]);
            }
        }
    }

    // In order round their middle.
    let c = pts.iter().fold([0.0; 2], |a, p| [a[0] + p[0] / pts.len() as f64, a[1] + p[1] / pts.len() as f64]);
    pts.sort_by(|a, b| (a[1] - c[1]).atan2(a[0] - c[0]).total_cmp(&(b[1] - c[1]).atan2(b[0] - c[0])));
    let walk = origin[1] + f64::from(joints[0].at[1]);
    Some((pts.into_iter().map(|p| world(p[0], p[1])).collect(), walk))
}

/// The capture locations of a wall's towers (`placed`: its pieces, key and frame; `read`: a building's logic file): one per tower, named
/// by the wall's key and their order.
#[must_use]
pub fn tower_captures(wall_key: &str, placed: &[(String, Piece)], read: &dyn Fn(&str) -> Option<Vec<u8>>) -> Vec<TowerCapture> {
    let mut out = vec![];
    // The game's pieces only (a generated piece's key is the tile's, whatever its name says).
    for (i, (key, p)) in placed.iter().enumerate().filter(|(_, (k, p))| p.rigid && is_tower(k)) {
        let Some(logic) = read(key) else { continue };
        let Some((ring, walk)) = tower_ring(&logic, p.origin, p.yaw) else { continue };
        let n = ring.len() as f64;
        let c = ring.iter().fold([0.0; 2], |a, q| [a[0] + q[0] / n, a[1] + q[1] / n]);
        out.push(TowerCapture { name: capture_name(wall_key, out.len()), piece: i, at: [c[0], walk, c[1]], ring });
    }

    out
}

/// A tower's capture location as an element (its boundary polygon a child, its points relative to it, as TEd's), at `at` in its parent's
/// frame (the world's for a layer at the origin; `lower` sets it again in the world).
///
/// # Errors
///
/// The templates are missing.
pub fn capture_element(c: &TowerCapture) -> Result<NewInstance, String> {
    use wce_foundation::math::Double3;
    use wce_tile::new_instances::{NewPoint, element_text, polygon_area};

    let points = c.ring.iter().map(|q| Double3::new(q[0] - c.at[0], 0.0, q[1] - c.at[2])).collect::<Vec<_>>();
    let mut boundary = NewInstance::new("CaptureLocationPolygonInstance")
        .attr("polygon_shape", "PS_POLYGON").attr("polygon_closed", "1").attr("surface_mode", "MSM_FREEFORM")
        .attr("area", wce_tile::ted_scene::format_number(polygon_area(&points)));
    boundary.points = points.into_iter().map(NewPoint::at).collect();

    let mut n = NewInstance::new("CaptureLocationInstance").position(Double3::new(c.at[0], c.at[1], c.at[2]))
        .attr("name", c.name.as_str()).attr("database_type", "capture_point_tower_only").attr("type", "CLT_MINOR")
        .attr("flag_position", "vector(0,0,0)").attr("surface_mode", "MSM_FREEFORM");
    n.children.push(element_text(&boundary)?);
    Ok(n)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_are_unique_per_wall_and_tower() {
        assert_eq!(capture_name("3", 0), "wce_wall3_tower1");
        assert_eq!(capture_name("3.1", 1), "wce_wall3_1_tower2");
        assert!(is_tower("roman_fort_tower_c_straight_with_extension") && is_tower("roman_fort_tower_end_left"));
        assert!(!is_tower("roman_fort_straight_with_solid_squared_bastion") && !is_tower("barbarian_fort_watchtower_b"));
    }
}
