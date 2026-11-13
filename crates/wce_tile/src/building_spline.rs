//! Building splines (`WceBuildingSpline`, not TEd's): a line drawn on the map and a list of buildings, laid along it end to end, each one
//! as long as its model, the next one after a gap. Meant for fences and palisades. The buildings are derived data, as a procedural road's
//! pieces are: made again from the line and its settings, written as plain `BUILDING_INSTANCE`s only when the tile is exported.
//!
//! A building is laid with its x along the line (as the kit's fences are modelled), on the chord of its stretch: from the point the stretch
//! starts at to the one it ends at, so a building whose pivot is at one end does not stick out of a bend (`road_layout`'s fences do the same).
//! Units: world metres; the instances' text is in scene units (`unit` a metre).

use wce_foundation::math::{Double3, Double3x3};

use crate::new_instances::{NewInstance, element_text};
use crate::road_layout::{Stations, ground_tilt};
use crate::ted_scene::format_number;
use crate::world_transform::{euler_from_matrix, euler_matrix};

/// The element of a building spline.
pub const BUILDING_SPLINE: &str = "WceBuildingSpline";

/// Its attributes: the buildings' keys, separated by commas, in the order they are laid when not random.
pub const KEYS: &str = "bspline_keys";

/// How far across a building the ground is read to tilt it (metres each way): as a road's fences.
pub const TILT_ACROSS: f64 = 0.5;

/// A building's footprint: where its model starts and ends along its x and along its z (metres, from its pivot). It is laid along the
/// longer of the two: a fence modelled along its z (`egyptian_highfence`) is turned a quarter so that its z runs along the line.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Footprint {
    pub x: (f64, f64),
    pub z: (f64, f64),
}

impl Footprint {
    /// A footprint `length` long along x about the pivot, `width` across.
    #[must_use]
    pub fn centred(length: f64, width: f64) -> Self {
        Self { x: (-length * 0.5, length * 0.5), z: (-width * 0.5, width * 0.5) }
    }

    /// Whether it is longer along its z than along its x.
    #[must_use]
    pub fn along_z(&self) -> bool {
        self.z.1 - self.z.0 > self.x.1 - self.x.0
    }

    /// Where it starts and ends along its long axis.
    #[must_use]
    pub fn along(&self) -> (f64, f64) {
        if self.along_z() { self.z } else { self.x }
    }
}

/// A building narrower than this along the line is laid as this long (metres): a building without a length known would stack up.
pub const MIN_LENGTH: f64 = 0.25;

/// The most buildings a spline lays.
pub const MAX_BUILDINGS: usize = 5000;

/// A building spline's settings (its attributes).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Settings {
    /// The buildings it lays, in order.
    pub keys: Vec<String>,

    /// Each building picked from `keys` by the seed (else they repeat in order).
    pub random: bool,
    pub seed: u32,

    /// The space between one building and the next (metres; below 0 they overlap).
    pub gap: f64,

    /// Each building tilted to the slope under it, end to end and across (else upright).
    pub align: bool,

    /// The line closes back to its first point.
    pub closed: bool,

    /// How far to the right of the drawn line the buildings stand, as it runs (metres; below 0: to its left).
    pub offset: f64,

    /// The chance (0 to 1) that a building is laid destroyed, at a random damaged level.
    pub damage: f64,

    /// The chance (0 to 1) that a building is turned end for end.
    pub flip: f64,

    /// How far each building may be turned about its middle, each way (degrees), and moved across the line (metres).
    pub yaw_jitter: f64,
    pub side_jitter: f64,
}

/// The keys of a list as written in [`KEYS`]: trimmed, empty ones left out.
#[must_use]
pub fn parse_keys(text: &str) -> Vec<String> {
    text.split(',').map(str::trim).filter(|k| !k.is_empty()).map(str::to_owned).collect()
}

/// A list of keys as [`KEYS`] holds it.
#[must_use]
pub fn keys_text(keys: &[String]) -> String {
    keys.join(",")
}

/// A building laid: its key, where its pivot is (world metres), how it is turned, and its destruction level (0: intact).
#[derive(Clone, Debug, PartialEq)]
pub struct Laid {
    pub key: String,
    pub position: [f64; 3],
    pub rotation: Double3x3,
    pub level: u32,
}

/// A number from 0 to 1 from the seed, the building's index and a salt.
fn roll(seed: u32, index: usize, salt: u64) -> f64 {
    let mut h = u64::from(seed).wrapping_mul(0x9E37_79B9_7F4A_7C15)
        ^ (index as u64).wrapping_mul(0xC2B2_AE3D_27D4_EB4F)
        ^ salt.wrapping_mul(0x1656_67B1_9E37_79F9);
    h ^= h >> 31;
    h = h.wrapping_mul(0xBF58_476D_1CE4_E5B9);
    h ^= h >> 29;
    h = h.wrapping_mul(0x94D0_49BB_1331_11EB);
    h ^= h >> 32;
    (h >> 11) as f64 / (1u64 << 53) as f64
}

fn unit(v: [f64; 2]) -> [f64; 2] {
    let l = v[0].hypot(v[1]);
    if l < 1e-12 { [1.0, 0.0] } else { [v[0] / l, v[1] / l] }
}

/// The heading of a direction on the ground as the files turn it (`euler_matrix`: Ry(a) takes +x to (cos a, 0, -sin a)).
fn heading(v: [f64; 2]) -> f64 {
    (-v[1]).atan2(v[0])
}

/// A line moved `offset` to its right as it runs (metres), each point along the mean of its segments' normals.
#[must_use]
pub fn offset_line(line: &[[f64; 2]], offset: f64, closed: bool) -> Vec<[f64; 2]> {
    let n = line.len();
    if n < 2 || offset.abs() < 1e-9 {
        return line.to_vec();
    }

    (0..n)
        .map(|i| {
            let (prev, next) = if closed && (i == 0 || i == n - 1) {
                // The first point and the last are the same: the way through them from the line's end into its start.
                (line[n - 2], line[1])
            } else {
                (line[i.saturating_sub(1)], line[(i + 1).min(n - 1)])
            };
            let t = unit([next[0] - prev[0], next[1] - prev[1]]);
            [line[i][0] + t[1] * offset, line[i][1] - t[0] * offset]
        })
        .collect()
}

/// Lays the buildings along a line (world metres, x and z: the curve, densely sampled; when `settings.closed`, its last point is its first).
/// `extent(key)`: its footprint ([`Footprint`]); `levels(key)`: how many destruction levels
/// it has (1: none); `ground(x, z)`: the terrain's height.
#[must_use]
pub fn lay(
    line: &[[f64; 2]],
    settings: &Settings,
    extent: &dyn Fn(&str) -> Footprint,
    levels: &dyn Fn(&str) -> u32,
    ground: &dyn Fn(f64, f64) -> f64,
) -> Vec<Laid> {
    if line.len() < 2 || settings.keys.is_empty() {
        return vec![];
    }

    let line = offset_line(line, settings.offset, settings.closed);
    let st = Stations::new(&line);
    let total = st.length();
    if total < 1e-6 {
        return vec![];
    }

    // A point past the line's end: round again on a closed one, on along its last way on an open one.
    let end = st.at(total);
    let point = |s: f64| -> [f64; 2] {
        if s <= total {
            st.at(s).0
        } else if settings.closed {
            st.at(s - total).0
        } else {
            [end.0[0] + end.1[0] * (s - total), end.0[1] + end.1[1] * (s - total)]
        }
    };

    let n = settings.keys.len();
    let mut out = vec![];
    let mut s = 0.0;
    for i in 0..MAX_BUILDINGS {
        let key = if settings.random {
            &settings.keys[((roll(settings.seed, i, 1) * n as f64) as usize).min(n - 1)]
        } else {
            &settings.keys[i % n]
        };
        let footprint = extent(key);
        let (lo, hi) = footprint.along();
        let (lo, hi) =
            if hi - lo >= MIN_LENGTH { (lo, hi) } else { ((lo + hi) * 0.5 - MIN_LENGTH * 0.5, (lo + hi) * 0.5 + MIN_LENGTH * 0.5) };
        let length = hi - lo;

        // A building more than half past the end is not laid (on a closed line: one that would cover the first).
        if s + length * 0.5 > total {
            break;
        }

        // On the chord of its stretch, from where it starts to where it ends.
        let (a, b) = (point(s), point(s + length));
        let mut dir = unit([b[0] - a[0], b[1] - a[1]]);
        let right = [dir[1], -dir[0]];
        let side = (roll(settings.seed, i, 2) * 2.0 - 1.0) * settings.side_jitter;
        let mid = [(a[0] + b[0]) * 0.5 + right[0] * side, (a[1] + b[1]) * 0.5 + right[1] * side];

        if roll(settings.seed, i, 3) < settings.flip {
            dir = [-dir[0], -dir[1]];
        }

        let yaw = (roll(settings.seed, i, 4) * 2.0 - 1.0) * settings.yaw_jitter.to_radians();
        if yaw != 0.0 {
            let (c, sn) = (yaw.cos(), yaw.sin());
            dir = [dir[0] * c - dir[1] * sn, dir[0] * sn + dir[1] * c];
        }

        // The model's middle over the stretch's: its pivot is where its middle is not.
        let centre = (lo + hi) * 0.5;
        // Its long axis along the line: a building modelled along its z turned a quarter first (Ry(90) takes +z to +x).
        let along_z = footprint.along_z();
        let quarter = if along_z { std::f64::consts::FRAC_PI_2 } else { 0.0 };
        let mut rotation = euler_matrix(Double3::new(heading(dir) + quarter, 0.0, 0.0));
        let axis = if along_z { Double3::new(0.0, 0.0, 1.0) } else { Double3::new(1.0, 0.0, 0.0) };
        let position = if settings.align {
            rotation = ground_tilt(dir, mid, [length * 0.5, TILT_ACROSS], ground) * rotation;
            let back = rotation * (axis * -centre);
            [mid[0] + back.x, ground(mid[0], mid[1]) + back.y, mid[1] + back.z]
        } else {
            let at = [mid[0] - dir[0] * centre, mid[1] - dir[1] * centre];
            [at[0], ground(at[0], at[1]), at[1]]
        };

        let max = levels(key);
        let level = if max >= 2 && roll(settings.seed, i, 5) < settings.damage {
            1 + (roll(settings.seed, i, 6) * f64::from(max - 1)).floor().min(f64::from(max - 2)) as u32
        } else {
            0
        };

        out.push(Laid { key: key.clone(), position, rotation, level });
        s += (length + settings.gap).max(MIN_LENGTH);
    }

    out
}

/// A building laid as an instance's text (`BUILDING_INSTANCE`, freeform at its height as the kit's tiles keep one, the height again in
/// `lf_offset`), `unit` scene units a metre.
///
/// # Errors
///
/// The building's defaults are missing.
pub fn instance_text(laid: &Laid, unit: f64) -> Result<Vec<u8>, String> {
    let [x, y, z] = laid.position;
    let e = euler_from_matrix(&laid.rotation);
    let n = format_number;
    let instance = NewInstance::new("BUILDING_INSTANCE")
        .position(Double3::new(x * unit, y * unit, z * unit))
        .attr("key", laid.key.clone())
        .attr("name", laid.key.clone())
        .attr("orientation", format!("euler({},{},{})", n(e.x), n(e.y), n(e.z)))
        .attr("surface_mode", "MSM_FREEFORM")
        .attr("lf_offset", n(y * unit))
        .attr("destruction_level", laid.level.to_string());
    element_text(&instance)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn settings(keys: &[&str]) -> Settings {
        Settings { keys: keys.iter().map(|k| (*k).to_owned()).collect(), seed: 1, ..Default::default() }
    }

    fn flat(_: f64, _: f64) -> f64 {
        0.0
    }

    /// A building along its x, 0.2 m thick.
    fn fx(lo: f64, hi: f64) -> Footprint {
        Footprint { x: (lo, hi), z: (-0.1, 0.1) }
    }

    #[test]
    fn a_building_modelled_along_its_z_is_turned_along_the_line() {
        // 4 m along z from its pivot, 0.3 m thick: laid every 4 m with its z along the line (+x here).
        let line = [[0.0, 0.0], [12.0, 0.0]];
        let along_z = Footprint { x: (-0.15, 0.15), z: (0.0, 4.0) };
        let laid = lay(&line, &settings(&["high"]), &|_| along_z, &|_| 1, &flat);
        assert_eq!(laid.len(), 3);
        for (i, l) in laid.iter().enumerate() {
            let z = l.rotation * Double3::new(0.0, 0.0, 1.0);
            assert!((z.x - 1.0).abs() < 1e-9, "{z:?}");
            assert!((l.position[0] - 4.0 * i as f64).abs() < 1e-9 && l.position[2].abs() < 1e-9, "{i}: {:?}", l.position);
        }

        // Aligned on a slope: its z follows it, its pivot (its start) on the ground.
        let slope = |x: f64, _: f64| x * 0.2;
        let s = Settings { align: true, ..settings(&["high"]) };
        let laid = lay(&line, &s, &|_| along_z, &|_| 1, &slope);
        let z = laid[1].rotation * Double3::new(0.0, 0.0, 1.0);
        assert!((z.y / z.x - 0.2).abs() < 1e-6, "{z:?}");
        let [x1, y1, _] = laid[1].position;
        assert!((y1 - x1 * 0.2).abs() < 1e-6, "{:?}", laid[1].position);
    }

    #[test]
    fn buildings_go_end_to_end_with_the_gap() {
        // Pivot at one end, 5 m along x (as lowfence_3).
        let line = [[0.0, 0.0], [20.0, 0.0]];
        let mut s = settings(&["fence"]);
        let laid = lay(&line, &s, &|_| fx(0.0, 5.0), &|_| 1, &flat);
        assert_eq!(laid.len(), 4);
        for (i, l) in laid.iter().enumerate() {
            assert!((l.position[0] - 5.0 * i as f64).abs() < 1e-9 && l.position[2].abs() < 1e-9, "{i}: {:?}", l.position);
            let x = l.rotation * Double3::new(1.0, 0.0, 0.0);
            assert!((x.x - 1.0).abs() < 1e-9, "{x:?}");
        }

        s.gap = 1.0;
        let laid = lay(&line, &s, &|_| fx(0.0, 5.0), &|_| 1, &flat);
        // The fourth would end 3 m past the line's end, more than half of it: not laid.
        assert_eq!(laid.iter().map(|l| l.position[0].round() as i64).collect::<Vec<_>>(), vec![0, 6, 12]);
    }

    #[test]
    fn a_centred_pivot_stands_in_the_middle_of_its_stretch() {
        let line = [[0.0, 0.0], [0.0, 10.0]];
        let laid = lay(&line, &settings(&["post"]), &|_| fx(-1.0, 1.0), &|_| 1, &flat);
        assert_eq!(laid.len(), 5);
        assert!((laid[0].position[2] - 1.0).abs() < 1e-9 && laid[0].position[0].abs() < 1e-9, "{:?}", laid[0].position);
        // Its x along +z: heading -90 degrees.
        let x = laid[0].rotation * Double3::new(1.0, 0.0, 0.0);
        assert!((x.z - 1.0).abs() < 1e-9, "{x:?}");
    }

    #[test]
    fn keys_repeat_in_order_or_by_the_seed() {
        let line = [[0.0, 0.0], [100.0, 0.0]];
        let s = settings(&["a", "b", "c"]);
        let keys = |s: &Settings| lay(&line, s, &|_| fx(0.0, 2.0), &|_| 1, &flat).into_iter().map(|l| l.key).collect::<Vec<_>>();
        assert_eq!(keys(&s)[..6], ["a", "b", "c", "a", "b", "c"]);

        let random = Settings { random: true, ..s.clone() };
        let a = keys(&random);
        assert_eq!(a, keys(&random), "the same seed, the same buildings");
        assert!(["a", "b", "c"].iter().all(|k| a.iter().any(|x| x == k)), "{a:?}");
        assert_ne!(a, keys(&Settings { seed: 2, ..random }));
    }

    #[test]
    fn the_offset_moves_them_to_the_right_as_the_line_runs() {
        // Running along +x, right is -z (the side a road's right fence is on, `road_layout::fence_runs`).
        let line = [[0.0, 0.0], [10.0, 0.0]];
        let s = Settings { offset: 3.0, ..settings(&["f"]) };
        let laid = lay(&line, &s, &|_| fx(0.0, 5.0), &|_| 1, &flat);
        assert!(laid.iter().all(|l| (l.position[2] + 3.0).abs() < 1e-9), "{:?}", laid[0].position);
    }

    #[test]
    fn a_slope_tilts_them_when_aligned() {
        // Rising 1 m every 5 m along x.
        let line = [[0.0, 0.0], [10.0, 0.0]];
        let slope = |x: f64, _: f64| x * 0.2;
        let s = Settings { align: true, ..settings(&["f"]) };
        let laid = lay(&line, &s, &|_| fx(0.0, 5.0), &|_| 1, &slope);
        let x = laid[0].rotation * Double3::new(1.0, 0.0, 0.0);
        assert!((x.y / x.x - 0.2).abs() < 1e-6, "{x:?}");
        // Its pivot (its start) on the ground, as its middle is.
        let [x0, y0, _] = laid[0].position;
        assert!(x0.abs() < 0.1 && (y0 - x0 * 0.2).abs() < 1e-6, "{:?}", laid[0].position);

        let upright = lay(&line, &settings(&["f"]), &|_| fx(0.0, 5.0), &|_| 1, &slope);
        let x = upright[0].rotation * Double3::new(1.0, 0.0, 0.0);
        assert!(x.y.abs() < 1e-9);
    }

    #[test]
    fn damage_and_flips_follow_their_chances() {
        let line = [[0.0, 0.0], [1000.0, 0.0]];
        let s = Settings { damage: 0.4, flip: 0.5, ..settings(&["f"]) };
        let laid = lay(&line, &s, &|_| fx(-1.0, 1.0), &|_| 3, &flat);
        let destroyed = laid.iter().filter(|l| l.level > 0).count();
        assert!(laid.iter().all(|l| l.level < 3));
        assert!((150..250).contains(&destroyed), "{destroyed} of {}", laid.len());
        let flipped = laid.iter().filter(|l| (l.rotation * Double3::new(1.0, 0.0, 0.0)).x < 0.0).count();
        assert!((200..300).contains(&flipped), "{flipped} of {}", laid.len());
        // A building with one level is never destroyed.
        assert!(lay(&line, &s, &|_| fx(-1.0, 1.0), &|_| 1, &flat).iter().all(|l| l.level == 0));
    }

    #[test]
    fn a_closed_line_goes_round() {
        let square = [[0.0, 0.0], [10.0, 0.0], [10.0, 10.0], [0.0, 10.0], [0.0, 0.0]];
        let s = Settings { closed: true, ..settings(&["f"]) };
        let laid = lay(&square, &s, &|_| fx(0.0, 5.0), &|_| 1, &flat);
        assert_eq!(laid.len(), 8);
    }

    #[test]
    fn keys_read_and_write() {
        assert_eq!(parse_keys(" a, b ,,c"), ["a", "b", "c"]);
        assert_eq!(keys_text(&parse_keys("a,b")), "a,b");
    }

    #[test]
    fn the_text_is_a_freeform_building() {
        let laid = Laid { key: "lowfence_3".into(), position: [1.0, 2.0, 3.0], rotation: Double3x3::default(), level: 1 };
        let t = String::from_utf8(instance_text(&laid, 2.0).unwrap()).unwrap();
        assert!(t.starts_with("<BUILDING_INSTANCE"), "{t}");
        assert!(
            t.contains("key=\"lowfence_3\"")
                && t.contains("vector(2,4,6)")
                && t.contains("MSM_FREEFORM")
                && t.contains("destruction_level=\"1\""),
            "{t}"
        );
    }
}
