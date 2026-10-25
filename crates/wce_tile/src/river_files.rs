//! TEd's `river_spline<n>.xml`: the rivers of a tile as the export (BOB) reads them, written by TEd next to tile.ted when it saves.
//!
//! Measured on the 271 files of the Attila kit (`examples/river_files_check.rs` writes them again from tile.ted, byte for byte):
//! - one file per `SplineRiverInstance`, numbered in document order; BOB reads `river_spline0.xml`, `river_spline1.xml`, ... up to the
//!   first missing one. A river with fewer than 2 points is an empty `<SEGMENTED_SPLINE/>`.
//! - a `SPLINE` per segment i -> i + 1, its `POINT0` .. `POINT3` the point i, its `controlOut`, the point i + 1's `controlIn` and the point
//!   i + 1 (landscape and freeform alike);
//! - x and z are the scene position (the instance's transform applied) / 2 in a battle tile, x 5 in a campaign tile (whose map is a tenth
//!   of the size), y the scene height / 1.25 (today's TEd; see the factors below);
//! - `width`, `foam_amount` and `flow_speed` are point i's on all four POINTs, `alpha_fade` point i's on POINT0 and POINT1 and point i + 1's on
//!   POINT2 and POINT3;
//! - the width and height factors depend on the TEd that wrote the file, and many files were not written again when their tile was saved
//!   later. The rivers with a `flatten` attribute (today's TEd): width x 0.5 and height / 1.25 in 101 files, height / 1 in 21 (the fords),
//!   width x 5 in 23; the older freeform rivers' files do not follow their tile.ted at all (positions edited since). So a river is written with
//!   the factors of the file it has when they are known ones, else today's ([`Factors`]), and a tile's files are written only for rivers that
//!   were edited (the editor's save), so that saving does not move or widen an old river;
//! - attributes `width x y foam_amount z alpha_fade flow_speed`, numbers `%g`, CRLF, four-space indentation.

use wce_formats::c_num::{printf_g, strtod};
use wce_foundation::math::Double3;

use crate::ted_scene::{TedScene, parse_vec3};
use crate::world_transform::world_transform;

/// The scene's horizontal units per metre of the files in a battle tile, and in a campaign tile.
const HORIZONTAL: f64 = 2.0;
const HORIZONTAL_CAMPAIGN: f64 = 0.2;

/// The scene's height units per metre of the files.
const VERTICAL: f64 = 1.25;

/// The rivers of a tile: the instances in document order.
pub fn rivers(scene: &TedScene) -> Vec<usize> {
    scene.find_all("SplineRiverInstance")
}

/// How a file's numbers relate to tile.ted: the file's width is the point's times `width`, its height the scene height divided by `height`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Factors {
    pub width: f64,
    pub height: f64,
}

impl Factors {
    /// Today's TEd.
    pub const TODAY: Self = Self { width: 0.5, height: VERTICAL };

    /// The factors of a river's existing file (its first point against the river's first point) when they are ones TEd has used (width
    /// 0.5, 1 or 5; height 1.25 or 1), else today's.
    pub fn of(scene: &TedScene, river: usize, existing: Option<&[u8]>) -> Self {
        let (Some(text), Some((w, y))) = (existing, first_point(scene, river)) else { return Self::TODAY };
        let text = String::from_utf8_lossy(text);
        let attr = |name: &str| {
            let at = text.find(&format!(" {name}=\""))? + name.len() + 3;
            text[at..].split('"').next().and_then(strtod)
        };

        let near = |v: f64, known: &[f64]| known.iter().copied().find(|k| (v - k).abs() < 1e-3);
        let width = attr("width").filter(|_| w != 0.0).and_then(|f| near(f / w, &[0.5, 1.0, 5.0])).unwrap_or(Self::TODAY.width);
        let height = attr("y").filter(|f| f.abs() > 1e-3).and_then(|f| near(y / f, &[1.25, 1.0])).unwrap_or(Self::TODAY.height);
        Self { width, height }
    }
}

/// A river's first point: its width and its scene height.
fn first_point(scene: &TedScene, river: usize) -> Option<(f64, f64)> {
    let els = scene.doc().elements();
    let points = els[river].children.iter().find(|&&c| els[c].name == "points")?;
    let p = *els[*points].children.iter().find(|&&p| els[p].name == "point")?;
    let w = world_transform(scene, river);
    let pos = scene.doc().get(p, "position").and_then(|v| parse_vec3(&v, ""))?;
    Some((scene.doc().get(p, "width").and_then(|v| strtod(&v)).unwrap_or(0.0), (w.position + w.rotation * pos).y))
}

/// The text of one river's file with these factors ([`Factors::of`]).
pub fn river_file(scene: &TedScene, river: usize, factors: Factors, campaign: bool) -> Vec<u8> {
    let horizontal = if campaign { HORIZONTAL_CAMPAIGN } else { HORIZONTAL };
    let els = scene.doc().elements();
    let w = world_transform(scene, river);
    let doc = scene.doc();
    let world = |q: Double3| w.position + w.rotation * q;
    struct Point {
        pos: Double3,
        cin: Double3,
        cout: Double3,
        width: String,
        foam: String,
        alpha: String,
        flow: String,
    }

    let mut points = vec![];
    for &c in els[river].children.iter().filter(|&&c| els[c].name == "points") {
        for &p in els[c].children.iter().filter(|&&p| els[p].name == "point") {
            let vec = |name: &str| doc.get(p, name).and_then(|v| parse_vec3(&v, ""));
            let Some(pos) = vec("position") else { continue };
            let num = |n: &str| doc.get(p, n).and_then(|v| strtod(&v)).unwrap_or(0.0);
            points.push(Point {
                pos: world(pos), cin: world(vec("controlIn").unwrap_or(pos)), cout: world(vec("controlOut").unwrap_or(pos)),
                width: printf_g(num("width") * factors.width), foam: printf_g(num("foam_amount")), alpha: printf_g(num("alpha_fade")),
                flow: printf_g(num("flow_speed")),
            });
        }
    }

    if points.len() < 2 {
        return b"<SEGMENTED_SPLINE/>\r\n".to_vec();
    }

    let mut out = b"<SEGMENTED_SPLINE>\r\n".to_vec();
    for i in 0..points.len() - 1 {
        let (a, b) = (&points[i], &points[i + 1]);
        out.extend_from_slice(b"    <SPLINE>\r\n");
        for (k, (q, alpha)) in [(a.pos, &a.alpha), (a.cout, &a.alpha), (b.cin, &b.alpha), (b.pos, &b.alpha)].into_iter().enumerate() {
            out.extend_from_slice(
                format!(
                    "        <POINT{k} width=\"{}\" x=\"{}\" y=\"{}\" foam_amount=\"{}\" z=\"{}\" alpha_fade=\"{alpha}\" flow_speed=\"{}\"/>\r\n", a.width,
                    printf_g(q.x / horizontal), printf_g(q.y / factors.height), a.foam, printf_g(q.z / horizontal), a.flow,
                )
                .as_bytes(),
            );
        }

        out.extend_from_slice(b"    </SPLINE>\r\n");
    }

    out.extend_from_slice(b"</SEGMENTED_SPLINE>\r\n");
    out
}

/// Every river's file name and text, each with the factors of its file in `dir` (if there is one).
pub fn river_files(scene: &TedScene, dir: Option<&std::path::Path>, campaign: bool) -> Vec<(String, Vec<u8>)> {
    rivers(scene)
        .into_iter()
        .enumerate()
        .map(|(n, r)| {
            let name = format!("river_spline{n}.xml");
            let existing = dir.and_then(|d| std::fs::read(d.join(&name)).ok());
            let text = river_file(scene, r, Factors::of(scene, r, existing.as_deref()), campaign);
            (name, text)
        })
        .collect()
}
