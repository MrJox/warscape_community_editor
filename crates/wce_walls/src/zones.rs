//! A piece's zone file (`<key>_zone_tech.xml`: zone, go and no-go regions, each a polygon of `<point x y z/>`) under a warp. Each region
//! gets points along its edges first; the rest of the text is kept as it is.

use crate::warp::{Warp, clip_to, subdivide};

/// A zone file warped; long edges get a point at least every `step` metres.
///
/// # Errors
///
/// A point has no x, y or z.
pub fn warp_zones(text: &str, warp: &dyn Warp, step: f32) -> Result<String, String> {
    let mut out = String::with_capacity(text.len() * 2);
    let mut region: Vec<[f32; 3]> = vec![];
    let mut indent = String::new();
    for line in text.split_inclusive('\n') {
        let t = line.trim_start();
        if t.starts_with("<point") {
            indent = line[..line.len() - t.len()].to_owned();
            region.push(point(t).ok_or_else(|| format!("zone file: bad point {}", t.trim()))?);
            continue;
        }

        if !region.is_empty() {
            let eol = if line.ends_with("\r\n") { "\r\n" } else { "\n" };
            let cut = clip_to(&region, &warp.clip_planes(), region.len() > 2);
            for (p, _) in subdivide(&cut, cut.len() > 2, step) {
                let [x, y, z] = warp.point(p);
                out.push_str(&format!("{indent}<point x=\"{x}\" y=\"{y}\" z=\"{z}\"/>{eol}"));
            }

            region.clear();
        }

        out.push_str(line);
    }

    Ok(out)
}

fn point(t: &str) -> Option<[f32; 3]> {
    let attr = |name: &str| -> Option<f32> {
        let at = t.find(&format!(" {name}=\""))? + name.len() + 3;
        t[at..].split('"').next()?.parse().ok()
    };

    Some([attr("x")?, attr("y")?, attr("z")?])
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::warp::Identity;

    #[test]
    fn regions_get_points_along_their_edges() {
        let text = "<regions>\n    <zone_regions>\n        <region name=\"region_zone01\">\n            <point x=\"20.2\" y=\"0.0\" z=\"2.5\"/>\n            \
                    <point x=\"-0.2\" y=\"0.0\" z=\"2.5\"/>\n            <point x=\"-0.2\" y=\"0.0\" z=\"-2.0\"/>\n            \
                    <point x=\"20.2\" y=\"0.0\" z=\"-2.0\"/>\n        </region>\n    </zone_regions>\n    <go_regions/>\n</regions>\n";
        let out = warp_zones(text, &Identity, 5.0).unwrap();
        assert_eq!(out.matches("<point").count(), 12);
        assert!(out.contains("            <point x=\"20.2\" y=\"0\" z=\"2.5\"/>\n"));
        assert!(out.ends_with("        </region>\n    </zone_regions>\n    <go_regions/>\n</regions>\n"));
    }
}
