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

/// A river whose file has its real heights (`wce_river_height="game"`), not the scene's over 1.25: the game draws a river at its file's
/// height, in the ground's metres, so TEd's factor pulls every river towards 0 and one on raised ground goes under it (docs/re/water.md).
/// The rivers the editor makes have it; those without it keep their file's factor, as they join their neighbours at those heights.
pub const RIVER_HEIGHT_ATTR: &str = "wce_river_height";

/// A river whose foam, flow speed and alpha fade the editor's river shader draws in an Attila battle (`wce_river_shader="1"`;
/// docs/re/water.md): set when one of them is edited on a river the editor didn't make (those with [`RIVER_HEIGHT_ATTR`] are drawn so
/// already). A mark of its own: the height mark would change the heights a kit river is exported at.
pub const RIVER_SHADER_ATTR: &str = "wce_river_shader";

/// A river whose points' flow speeds follow their widths (`wce_flow_by_width="1"`, user 2026-11-28): each point's flow is the river's
/// [`FLOW_BASE_ATTR`] times its mean width over the point's, as water speeds up through a narrow and slows in a wide stretch; the
/// points' own `flow_speed` are then not used (the export and the editor's water take [`width_flows`]).
pub const FLOW_BY_WIDTH_ATTR: &str = "wce_flow_by_width";

/// The flow at the river's mean width when it follows the widths ([`FLOW_BY_WIDTH_ATTR`]); [`DEFAULT_FLOW`] without it.
pub const FLOW_BASE_ATTR: &str = "wce_flow_base";

/// TEd's flow speed, the game's own speed of a river's ripples (the editor's river shader).
pub const DEFAULT_FLOW: f64 = 0.5;

/// How far a point's flow may get from the base when it follows the widths: from a quarter to four times (a point next to nothing wide).
const FLOW_BY_WIDTH_RANGE: (f64, f64) = (0.25, 4.0);

/// The points' flow speeds of a river whose flow follows its widths ([`FLOW_BY_WIDTH_ATTR`]), in its points' order; none when it doesn't.
pub fn width_flows(scene: &TedScene, river: usize) -> Option<Vec<f64>> {
    let doc = scene.doc();
    if !doc.get(river, FLOW_BY_WIDTH_ATTR).is_some_and(|v| v.trim() == "1" || v.trim() == "true") {
        return None;
    }

    let base = doc.get(river, FLOW_BASE_ATTR).and_then(|v| strtod(&v)).unwrap_or(DEFAULT_FLOW);
    let els = doc.elements();
    let widths = els[river]
        .children
        .iter()
        .filter(|&&c| els[c].name == "points")
        .flat_map(|&c| els[c].children.iter().filter(|&&p| els[p].name == "point").copied().collect::<Vec<_>>())
        .map(|p| doc.get(p, "width").and_then(|v| strtod(&v)).unwrap_or(0.0))
        .collect::<Vec<_>>();
    let mean = widths.iter().sum::<f64>() / widths.len().max(1) as f64;
    Some(
        widths
            .iter()
            .map(|&w| {
                let k = if w > 1e-6 { mean / w } else { FLOW_BY_WIDTH_RANGE.1 };
                base * k.clamp(FLOW_BY_WIDTH_RANGE.0, FLOW_BY_WIDTH_RANGE.1)
            })
            .collect(),
    )
}

/// The marks that make a river one the editor's river shader draws ([`RIVER_HEIGHT_ATTR`], [`RIVER_SHADER_ATTR`]).
pub const SHADER_RIVER_ATTRS: [&str; 2] = [RIVER_HEIGHT_ATTR, RIVER_SHADER_ATTR];

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
        let found = Self::of_file(scene, river, existing);
        Self { height: if scene.doc().get(river, RIVER_HEIGHT_ATTR).is_some() { 1.0 } else { found.height }, ..found }
    }

    fn of_file(scene: &TedScene, river: usize, existing: Option<&[u8]>) -> Self {
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

    let flows = width_flows(scene, river);
    let mut points = vec![];
    for &c in els[river].children.iter().filter(|&&c| els[c].name == "points") {
        for &p in els[c].children.iter().filter(|&&p| els[p].name == "point") {
            let vec = |name: &str| doc.get(p, name).and_then(|v| parse_vec3(&v, ""));
            let Some(pos) = vec("position") else { continue };
            let num = |n: &str| doc.get(p, n).and_then(|v| strtod(&v)).unwrap_or(0.0);
            points.push(Point {
                pos: world(pos),
                cin: world(vec("controlIn").unwrap_or(pos)),
                cout: world(vec("controlOut").unwrap_or(pos)),
                width: printf_g(num("width") * factors.width),
                foam: printf_g(num("foam_amount")),
                alpha: printf_g(num("alpha_fade")),
                flow: printf_g(flows.as_ref().and_then(|f| f.get(points.len()).copied()).unwrap_or_else(|| num("flow_speed"))),
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

#[cfg(test)]
mod flow_tests {
    use super::*;

    fn river(attrs: &str) -> TedScene {
        TedScene::parse_any(
            format!(
                r#"<project version="16"><scene><SplineRiverInstance name="r" position="vector(0,0,0)" orientation="euler(0,0,0)"{attrs}><points>
<point position="0,0,0" controlIn="0,0,0" controlOut="0,0,0" width="40" foam_amount="0.1" alpha_fade="1" flow_speed="0.5"/>
<point position="20,0,0" controlIn="20,0,0" controlOut="20,0,0" width="20" foam_amount="0.1" alpha_fade="1" flow_speed="0.5"/>
<point position="40,0,0" controlIn="40,0,0" controlOut="40,0,0" width="60" foam_amount="0.1" alpha_fade="1" flow_speed="0.5"/>
</points></SplineRiverInstance></scene></project>"#
            )
            .into_bytes(),
        )
        .unwrap()
    }

    #[test]
    fn a_rivers_flow_follows_its_widths_when_asked() {
        let off = river("");
        let r = rivers(&off)[0];
        assert_eq!(width_flows(&off, r), None, "off: the points' own");

        let on = river(r#" wce_flow_by_width="1" wce_flow_base="0.6""#);
        let f = width_flows(&on, r).unwrap();
        // Mean width 40: as wide, the base; half as wide, twice; one and a half as wide, two thirds.
        assert!((f[0] - 0.6).abs() < 1e-9 && (f[1] - 1.2).abs() < 1e-9 && (f[2] - 0.4).abs() < 1e-9, "{f:?}");

        // The file the export reads has them (point i's flow on its segment's four points).
        let text = String::from_utf8(river_file(&on, r, Factors::of(&on, r, None), false)).unwrap();
        assert!(text.contains("flow_speed=\"0.6\"") && text.contains("flow_speed=\"1.2\""), "{text}");
        assert!(!text.contains("flow_speed=\"0.5\""));
    }
}
