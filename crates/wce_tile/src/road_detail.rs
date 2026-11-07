//! What stands along a procedural road (docs/plans/procedural_roads.md): the kit's path prefabs, their layouts bent to the road. A prefab's
//! parts are mostly projected decals, which take the ground's shape on their own, and short rigid buildings, which a curve hardly moves away
//! from: nothing is warped. Each part keeps its place along and across the prefab's long axis, and goes to that place along and across the road.
//!
//! Prefabs are in the file's units; the road is in world units (`unit` the file's per world's).

use wce_foundation::math::{Double3, Double3x3};

use crate::ted_scene::TedScene;
use crate::world_transform::{euler_from_matrix, euler_matrix, world_transform};

/// A part of a prefab.
#[derive(Clone, Debug)]
pub struct Part {
    text: Vec<u8>,

    /// Along and across the prefab's long axis, and over its middle height (file's units).
    along: f64,
    across: f64,
    up: f64,

    /// Its rotation in the prefab.
    rotation: Double3x3,

    /// A building: it stands, so it goes beside the road, never on it.
    solid: bool,
}

/// A prefab read for laying along a road.
#[derive(Clone, Debug)]
pub struct Layout {
    parts: Vec<Part>,

    /// Where its long axis points: radians from +z towards +x.
    angle: f64,

    /// Along the axis, from where its first part is to where the last is (file's units).
    length: f64,

    /// The furthest a part is from the axis.
    width: f64,
}

/// An instance laid along a road: its element's text, placed.
#[derive(Clone, Debug, PartialEq)]
pub struct Laid {
    pub text: Vec<u8>,

    /// In the file's units.
    pub position: [f64; 3],
}

/// The instances of a prefab that follow a road.
fn laid_kind(kind: &str) -> bool {
    matches!(kind, "ProjectedDecalInstance" | "BUILDING_INSTANCE" | "SIMPLE_MESH_INSTANCE")
}

/// Whether a part is a skirt of ground made for its one spot (`global_ground_pathborder`): moved along a road it is a mound across it.
fn shapes_the_ground(scene: &TedScene, e: usize) -> bool {
    scene.doc().get(e, "key").is_some_and(|k| k.contains("_ground_"))
}

/// Reads a prefab's group. `None`: it has no parts to lay.
#[must_use]
pub fn read_prefab(group_text: &[u8]) -> Option<Layout> {
    let mini = TedScene::parse_any(group_text.to_vec()).ok()?;
    let els = mini.doc().elements();
    let root = (0..els.len()).find(|&e| els[e].parent.is_none())?;
    let origin = world_transform(&mini, root).position;
    let mut found = vec![];
    for e in 0..els.len() {
        if laid_kind(&els[e].name) && !shapes_the_ground(&mini, e) {
            let w = world_transform(&mini, e);
            found.push((e, w.position - origin, w.rotation));
        }
    }

    if found.is_empty() {
        return None;
    }

    // The long axis: the main direction of the parts' places.
    let n = found.len() as f64;
    let (mx, mz) = found.iter().fold((0.0, 0.0), |a, f| (a.0 + f.1.x / n, a.1 + f.1.z / n));
    let (mut xx, mut zz, mut xz) = (0.0, 0.0, 0.0);
    for f in &found {
        let (x, z) = (f.1.x - mx, f.1.z - mz);
        (xx, zz, xz) = (xx + x * x, zz + z * z, xz + x * z);
    }

    let turn = 0.5 * (2.0 * xz).atan2(xx - zz);
    let (ax, az) = (turn.cos(), turn.sin());
    let mut ys = found.iter().map(|f| f.1.y).collect::<Vec<_>>();
    ys.sort_by(f64::total_cmp);
    let middle = ys[ys.len() / 2];
    let along = |p: &Double3| (p.x - mx) * ax + (p.z - mz) * az;
    let lo = found.iter().map(|f| along(&f.1)).fold(f64::MAX, f64::min);
    let hi = found.iter().map(|f| along(&f.1)).fold(f64::MIN, f64::max);
    let mut width = 0.0f64;
    let parts = found.iter().map(|&(e, p, rotation)| {
        let across = (p.x - mx) * az - (p.z - mz) * ax;
        width = width.max(across.abs());
        let solid = mini.doc().elements()[e].name != "ProjectedDecalInstance";
        Part { text: mini.element_text(e), along: along(&p) - lo, across, up: p.y - middle, rotation, solid }
    }).collect();

    Some(Layout { parts, angle: ax.atan2(az), length: hi - lo, width })
}

/// The turn about y by `yaw` (+z towards +x).
fn yaw_matrix(yaw: f64) -> Double3x3 {
    euler_matrix(Double3::new(yaw, 0.0, 0.0))
}

fn pick(seed: u32, n: u32) -> u32 {
    let mut h = u64::from(seed).wrapping_mul(0x9E37_79B9_7F4A_7C15) ^ u64::from(n).wrapping_mul(0xC2B2_AE3D_27D4_EB4F);
    h ^= h >> 31;
    (h.wrapping_mul(0xBF58_476D_1CE4_E5B9) >> 40) as u32
}

/// A line's distances, for finding places on it.
pub struct Stations<'a> {
    line: &'a [[f64; 2]],
    along: Vec<f64>,
}

impl<'a> Stations<'a> {
    #[must_use]
    pub fn new(line: &'a [[f64; 2]]) -> Self {
        let mut along = Vec::with_capacity(line.len());
        let mut d = 0.0;
        for (i, p) in line.iter().enumerate() {
            if i > 0 {
                d += (p[0] - line[i - 1][0]).hypot(p[1] - line[i - 1][1]);
            }

            along.push(d);
        }

        Self { line, along }
    }

    #[must_use]
    pub fn length(&self) -> f64 {
        self.along.last().copied().unwrap_or(0.0)
    }

    /// The point at a distance along the line and where the line goes there (a unit vector).
    #[must_use]
    pub fn at(&self, s: f64) -> ([f64; 2], [f64; 2]) {
        let i = self.along.partition_point(|&d| d <= s).clamp(1, self.line.len().max(2) - 1);
        let (a, b) = (self.line[i - 1], self.line[i]);
        let span = (self.along[i] - self.along[i - 1]).max(1e-9);
        let f = ((s - self.along[i - 1]) / span).clamp(0.0, 1.0);
        let len = (b[0] - a[0]).hypot(b[1] - a[1]).max(1e-9);
        ([a[0] + (b[0] - a[0]) * f, a[1] + (b[1] - a[1]) * f], [(b[0] - a[0]) / len, (b[1] - a[1]) / len])
    }
}

/// Lays prefabs along a line, one after another with a little ground between them, chosen by the seed. `half_width` (world units): what is
/// wider than that goes in towards the line. `ground`: the terrain's height (world units); `skip`: the stretches left out (a bridge's).
#[must_use]
pub fn lay(layouts: &[Layout], line: &[[f64; 2]], unit: f64, half_width: f64, ground: &dyn Fn(f64, f64) -> f64, skip: &dyn Fn(f64) -> bool, seed: u32) -> Vec<Laid> {
    let mut out = vec![];
    let stations = Stations::new(line);
    let total = stations.length();
    if layouts.is_empty() || total <= 0.0 {
        return out;
    }

    let (mut s, mut n) = (0.0, 0);
    while s < total {
        let layout = &layouts[pick(seed, n) as usize % layouts.len()];
        let length = (layout.length / unit).max(4.0);
        let fit = if layout.width > 0.0 { (half_width * unit / layout.width).min(1.0) } else { 1.0 };
        for part in &layout.parts {
            let at = s + part.along / unit;
            if at > total || skip(at) {
                continue;
            }

            let (p, t) = stations.at(at);
            let across = part.across / unit * fit;
            let across = if part.solid { across.signum() * across.abs().max(half_width + 1.0) } else { across };
            let (x, z) = (p[0] + t[1] * across, p[1] - t[0] * across);
            let turn = t[0].atan2(t[1]) - layout.angle;
            let rotation = yaw_matrix(turn) * part.rotation;
            let position = [x * unit, ground(x, z) * unit + part.up, z * unit];
            if let Some(text) = placed_text(&part.text, position, &rotation) {
                out.push(Laid { text, position });
            }
        }

        s += length + 2.0 + f64::from(pick(seed, n + 1000) % 7);
        n += 1;
    }

    out
}

/// A part's element, where it goes.
pub fn placed_text(text: &[u8], position: [f64; 3], rotation: &Double3x3) -> Option<Vec<u8>> {
    let mut mini = TedScene::parse_any(text.to_vec()).ok()?;
    let els = mini.doc().elements();
    let root = (0..els.len()).find(|&e| els[e].parent.is_none())?;
    mini.set_position(root, Double3::new(position[0], position[1], position[2]));
    mini.set_euler(root, euler_from_matrix(rotation));

    // As the kit's tiles keep a freeform instance: the height again, over the tile's LF of 0.
    mini.set_attr(root, "lf_offset", &crate::ted_scene::format_number(position[1]));
    Some(mini.save())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn prefab() -> Vec<u8> {
        br#"<UserInstanceGroup flags="" position="vector(100,5,200)" name="p" orientation="euler(0,0,0)"><children>
            <ProjectedDecalInstance flags="" scale="vector(5,10,-10)" position="vector(0,0,-10)" key="a" name="d" surface_mode="MSM_FREEFORM" orientation="euler(0,0,0)"/>
            <ProjectedDecalInstance flags="" scale="vector(5,10,-10)" position="vector(0,0,10)" key="b" name="d" surface_mode="MSM_FREEFORM" orientation="euler(0,0,0)"/>
            <UserInstanceGroup flags="" position="vector(0,2,0)" name="g" orientation="euler(0,0,0)"><children>
                <BUILDING_INSTANCE flags="" position="vector(1,0,0)" key="c" name="c" surface_mode="MSM_FREEFORM" orientation="euler(0,0,0)"/>
            </children></UserInstanceGroup>
        </children></UserInstanceGroup>"#.to_vec()
    }

    #[test]
    fn a_prefab_is_read_along_its_long_axis() {
        let l = read_prefab(&prefab()).unwrap();
        assert_eq!(l.parts.len(), 3);
        assert!((l.length - 20.0).abs() < 1e-6, "{}", l.length);
        assert!((l.width - 1.0).abs() < 0.5, "{}", l.width);
    }

    #[test]
    fn no_parts_nothing_to_lay() {
        assert!(read_prefab(br#"<UserInstanceGroup flags="" position="vector(0,0,0)" name="p"><children/></UserInstanceGroup>"#).is_none());
    }

    #[test]
    fn parts_follow_the_road_and_keep_their_order() {
        let l = read_prefab(&prefab()).unwrap();
        let line = (0..=100).map(|i| [f64::from(i), 0.0]).collect::<Vec<_>>();
        let laid = lay(&[l], &line, 1.0, 3.0, &|_, _| 7.0, &|_| false, 1);
        assert!(!laid.is_empty());
        assert!(laid.iter().all(|p| p.position[2].abs() < 5.0 && p.position[0] <= 100.0), "{laid:?}");
        assert!(laid.iter().all(|p| (p.position[1] - 7.0).abs() < 3.0), "{laid:?}");
    }

    #[test]
    fn a_stretch_left_out_has_nothing() {
        let l = read_prefab(&prefab()).unwrap();
        let line = (0..=100).map(|i| [f64::from(i), 0.0]).collect::<Vec<_>>();
        let laid = lay(&[l], &line, 1.0, 3.0, &|_, _| 0.0, &|s| (40.0..60.0).contains(&s), 1);
        assert!(laid.iter().all(|p| !(41.0..59.0).contains(&p.position[0])), "{laid:?}");
    }

    #[test]
    fn the_same_seed_lays_the_same() {
        let l = read_prefab(&prefab()).unwrap();
        let line = (0..=100).map(|i| [f64::from(i), 0.0]).collect::<Vec<_>>();
        let a = lay(&[l.clone()], &line, 1.0, 3.0, &|_, _| 0.0, &|_| false, 4);
        assert_eq!(a, lay(&[l], &line, 1.0, 3.0, &|_, _| 0.0, &|_| false, 4));
    }

    #[test]
    fn a_place_on_a_bent_line_is_on_it() {
        let line = [[0.0, 0.0], [10.0, 0.0], [10.0, 10.0]];
        let s = Stations::new(&line);
        assert_eq!(s.length(), 20.0);
        let (p, t) = s.at(15.0);
        assert!((p[0] - 10.0).abs() < 1e-9 && (p[1] - 5.0).abs() < 1e-9);
        assert_eq!(t, [0.0, 1.0]);
    }
}
