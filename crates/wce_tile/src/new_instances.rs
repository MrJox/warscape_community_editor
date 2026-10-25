//! New instances of any type of tile.ted: their text in TEd's layout, from the type's defaults ([`crate::instance_templates`]) with the
//! attributes, contents and points of this one. The text is at column 0 with `\n` line ends, as [`crate::ted_scene::TedScene::insert_element_text`]
//! takes it.

use std::collections::BTreeMap;

use wce_foundation::math::Double3;

use crate::instance_templates::InstanceTemplate;
use crate::ted_scene::format_number;

/// The element of a procedural wall (docs/plans/procedural_walls.md): not TEd's, so named as tile.wce's convention asks
/// (`project_files`).
pub const WALL_SPLINE: &str = "WceWallSpline";

/// The defaults of the editor's own types, which the kit has none of (`instance_templates` is generated from its tiles): a procedural
/// wall is an open polyline with the polygons' attributes and its own.
pub const EDITOR_TEMPLATES: &[InstanceTemplate] = &[InstanceTemplate {
    kind: WALL_SPLINE,
    attrs: &[
        ("flags", " OIF_VISIBLE|OIF_ACTIVE|OIF_DRAGGABLE"), ("recenter_polygon", "0"), ("lf_offset", "0"), ("line_colour", "argb(1.0,0.8,0.0,1.0)"),
        ("polygon_closed", "0"), ("position", "vector(0,0,0)"), ("selected_line_colour", "argb(1.0,0.0,0.0,1.0)"), ("name", "Wall"),
        ("polygon_shape", "PS_POLYGON"), ("area", "0"), ("surface_mode", "MSM_LANDSCAPE"), ("orientation", "euler(0,0,0)"),
        ("wall_base", "barbarian_fort_straight"), ("wall_tolerance", "0.5"), ("wall_flip", "false"), ("wall_access_every", "200"),
        ("wall_defence_every", "0"),
    ],
    wrappers: &["points"],
    point_attrs: &[("position", "0,0,0")],
}];

/// The defaults of a type: the kit's, else the editor's own.
pub fn template(kind: &str) -> Option<&'static InstanceTemplate> {
    crate::instance_templates::template(kind).or_else(|| EDITOR_TEMPLATES.iter().find(|t| t.kind == kind))
}

/// A point of a polygon or a spline: its local position and attributes other than the template's (`controlIn`, `width`, ...).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct NewPoint {
    pub position: Double3,
    pub attrs: BTreeMap<String, String>,
}

impl NewPoint {
    pub fn at(position: Double3) -> Self {
        Self { position, attrs: BTreeMap::new() }
    }
}

/// What a new instance is made of: its type, the attributes that are not the type's defaults (raw values), the elements it holds (texts at
/// column 0) and its points (local to it).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct NewInstance {
    pub kind: String,
    pub attrs: BTreeMap<String, String>,
    pub children: Vec<Vec<u8>>,
    pub points: Vec<NewPoint>,
}

impl NewInstance {
    pub fn new(kind: &str) -> Self {
        Self { kind: kind.to_owned(), ..Default::default() }
    }

    pub fn attr(mut self, name: &str, value: impl Into<String>) -> Self {
        self.attrs.insert(name.to_owned(), value.into());
        self
    }

    pub fn position(self, p: Double3) -> Self {
        self.attr("position", vector(p))
    }
}

/// `vector(x,y,z)` (full precision: tile.wce's).
pub fn vector(p: Double3) -> String {
    format!("vector({},{},{})", format_number(p.x), format_number(p.y), format_number(p.z))
}

/// `x,y,z` of a point (full precision: tile.wce's).
pub fn point_text(p: Double3) -> String {
    format!("{},{},{}", format_number(p.x), format_number(p.y), format_number(p.z))
}

/// The area a polygon's points enclose on the ground (x, z).
pub fn polygon_area(points: &[Double3]) -> f64 {
    let mut area = 0.0;
    for i in 0..points.len() {
        let (p, q) = (points[i], points[(i + 1) % points.len()]);
        area += p.x * q.z - q.x * p.z;
    }

    area.abs() / 2.0
}

/// A circle as TEd makes one (`PS_CIRCLE`, a capture location's boundary): 20 points every 18 degrees, the first on +z, clockwise seen
/// from above.
pub fn circle_points(radius: f64) -> Vec<Double3> {
    (0..20).map(|i| {
        let a = f64::from(i) * std::f64::consts::TAU / 20.0;
        Double3::new(radius * a.sin(), 0.0, radius * a.cos())
    }).collect()
}

fn attr_text(out: &mut Vec<u8>, name: &str, value: &str) {
    out.push(b' ');
    out.extend_from_slice(name.as_bytes());
    out.extend_from_slice(b"=\"");
    out.extend_from_slice(value.as_bytes());
    out.push(b'"');
}

/// The instance's text: the type's attributes in TEd's order (its own values where given; attributes the type does not have, after
/// them), then its wrappers, as the type holds them, with its contents and points.
///
/// # Errors
///
/// A type without defaults.
pub fn element_text(n: &NewInstance) -> Result<Vec<u8>, String> {
    let t = template(&n.kind).ok_or_else(|| format!("no defaults for {}", n.kind))?;
    let mut out = format!("<{}", n.kind).into_bytes();
    for (k, v) in t.attrs {
        attr_text(&mut out, k, n.attrs.get(*k).map_or(*v, String::as_str));
    }

    for (k, v) in &n.attrs {
        if !t.attrs.iter().any(|(a, _)| a == k) {
            attr_text(&mut out, k, v);
        }
    }

    if t.wrappers.is_empty() {
        out.extend_from_slice(b"/>\n");
        return Ok(out);
    }

    out.extend_from_slice(b">\n");
    for w in t.wrappers {
        out.extend_from_slice(format!("    <{w}>\n").as_bytes());
        match *w {
            "points" => {
                for p in &n.points {
                    out.extend_from_slice(b"        <point");
                    for (k, v) in t.point_attrs {
                        let v = if *k == "position" { point_text(p.position) } else { p.attrs.get(*k).cloned().unwrap_or_else(|| (*v).to_owned()) };
                        attr_text(&mut out, k, &v);
                    }

                    out.extend_from_slice(b"/>\n");
                }
            }

            _ => {
                for child in &n.children {
                    for line in child.split_inclusive(|&c| c == b'\n') {
                        out.extend_from_slice(b"        ");
                        out.extend_from_slice(line);
                    }
                }
            }
        }

        out.extend_from_slice(format!("    </{w}>\n").as_bytes());
    }

    out.extend_from_slice(format!("</{}>\n", n.kind).as_bytes());
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_new_wall_is_written_with_its_points_and_defaults() {
        let mut n = NewInstance::new(WALL_SPLINE).position(Double3::new(1.0, 0.0, 2.0)).attr("wall_base", "roman_fort_straight");
        n.points = vec![NewPoint::at(Double3::new(-5.0, 0.0, 0.0)), NewPoint::at(Double3::new(5.0, 0.0, 0.0))];
        let text = String::from_utf8(element_text(&n).unwrap()).unwrap();
        assert!(text.starts_with("<WceWallSpline flags=") && text.contains("wall_base=\"roman_fort_straight\"") && text.contains("wall_tolerance=\"0.5\""), "{text}");
        assert!(text.contains("<points>") && text.matches("<point ").count() == 2 && text.ends_with("</WceWallSpline>\n"), "{text}");
    }

    #[test]
    fn a_new_instance_is_written_in_teds_layout() {
        let light = element_text(&NewInstance::new("POINT_LIGHT_INSTANCE").position(Double3::new(1.0, 2.0, 3.0)).attr("name", "Torch")).unwrap();
        let text = String::from_utf8(light).unwrap();
        assert!(text.starts_with("<POINT_LIGHT_INSTANCE random_offset=\"0\" flags=\" OIF_VISIBLE|OIF_ACTIVE\""), "{text}");
        assert!(text.contains("position=\"vector(1,2,3)\"") && text.contains("name=\"Torch\"") && text.ends_with("/>\n"), "{text}");

        let mut zone = NewInstance::new("ZONE_POLYGON_INSTANCE");
        zone.points = [Double3::new(0.0, 0.0, 0.0), Double3::new(10.0, 0.0, 0.0), Double3::new(0.0, 0.0, 10.0)].into_iter().map(NewPoint::at).collect();
        let text = String::from_utf8(element_text(&zone).unwrap()).unwrap();
        let lines = text.lines().collect::<Vec<_>>();
        assert_eq!(&lines[1..], ["    <children>", "    </children>", "    <points>", "        <point position=\"0,0,0\"/>", "        <point position=\"10,0,0\"/>",
            "        <point position=\"0,0,10\"/>", "    </points>", "</ZONE_POLYGON_INSTANCE>"]);
        assert!((polygon_area(&[Double3::new(0.0, 0.0, 0.0), Double3::new(10.0, 0.0, 0.0), Double3::new(0.0, 0.0, 10.0)]) - 50.0).abs() < 1e-9);
        let c = circle_points(15.0);
        assert!((c[1].x - 4.63531).abs() < 1e-4 && (c[1].z - 14.2659).abs() < 1e-4, "TEd's second point of a 15 m circle");
    }
}
