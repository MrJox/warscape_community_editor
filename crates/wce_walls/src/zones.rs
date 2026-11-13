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

/// The zone regions reshaped around stairs across the walkway (`steps`: their x ranges, the base's frame; `stair_z`: the z they span),
/// as the stepped bastion's are: the inner region (the one reaching furthest in z) takes the stair's whole width over its range, the
/// others give way to a strip outside it, with a slanted change over `ZONE_EASE` on either side (user 2026-11-05: our stair across the
/// walkway lay in two regions, the bastion's in one). Only regions that are rectangles along x (the straight bases') are reshaped.
pub fn reshape_for_steps(text: &str, steps: &[(f32, f32)], stair_z: (f32, f32)) -> String {
    if steps.is_empty() {
        return text.to_owned();
    }

    // The regions' point lines, by region.
    let lines = text.split_inclusive('\n').collect::<Vec<_>>();
    let mut regions: Vec<(usize, usize)> = vec![];
    let mut i = 0;
    while i < lines.len() {
        if lines[i].trim_start().starts_with("<point") {
            let start = i;
            while i < lines.len() && lines[i].trim_start().starts_with("<point") {
                i += 1;
            }

            regions.push((start, i));
        } else {
            i += 1;
        }
    }

    let rect = |(a, b): (usize, usize)| -> Option<(f32, f32, f32, f32)> {
        let pts = lines[a..b].iter().map(|l| point(l.trim_start())).collect::<Option<Vec<_>>>()?;
        let (x0, x1) = pts.iter().fold((f32::MAX, f32::MIN), |(lo, hi), p| (lo.min(p[0]), hi.max(p[0])));
        let (z0, z1) = pts.iter().fold((f32::MAX, f32::MIN), |(lo, hi), p| (lo.min(p[2]), hi.max(p[2])));
        (pts.len() == 4
            && pts
                .iter()
                .all(|p| ((p[0] - x0).abs() < 1e-3 || (p[0] - x1).abs() < 1e-3) && ((p[2] - z0).abs() < 1e-3 || (p[2] - z1).abs() < 1e-3)))
        .then_some((x0, x1, z0, z1))
    };

    let rects = regions.iter().map(|&r| rect(r)).collect::<Vec<_>>();
    if rects.len() < 2 || rects.iter().any(Option::is_none) {
        return text.to_owned();
    }

    let rects = rects.into_iter().flatten().collect::<Vec<_>>();
    let inner = (0..rects.len()).max_by(|&a, &b| rects[a].3.total_cmp(&rects[b].3)).unwrap_or(0);
    let mut steps = steps.to_vec();
    steps.sort_by(|a, b| a.0.total_cmp(&b.0));

    let mut out = String::with_capacity(text.len() * 2);
    let mut at = 0;
    for (k, &(a, b)) in regions.iter().enumerate() {
        out.extend(lines[at..a].iter().copied());
        let indent = &lines[a][..lines[a].len() - lines[a].trim_start().len()];
        let eol = if lines[a].ends_with("\r\n") { "\r\n" } else { "\n" };
        let (x0, x1, z0, z1) = rects[k];

        // The edge towards the stair, with a notch over each step: the inner region's outer edge goes out to the stair's outer side,
        // the others' inner edge back to a strip outside it. The notch's points in the order the edge runs.
        let (edge_z, notch_z) = if k == inner { (z0, stair_z.0 - 0.05) } else { (z1, (stair_z.0 - 0.2).max(z0 + 0.05)) };
        let mut notch = vec![];
        for &(sa, sb) in &steps {
            let (pa, pb) = ((sa - ZONE_EASE).max(x0), (sb + ZONE_EASE).min(x1));
            notch.extend([[pa, edge_z], [sa.max(x0), notch_z], [sb.min(x1), notch_z], [pb, edge_z]]);
        }

        let mut pts: Vec<[f32; 2]> = if k == inner {
            // (x1, z1), (x0, z1), (x0, z0), the notches along z0 from x0 to x1, (x1, z0).
            let mut v = vec![[x1, z1], [x0, z1], [x0, z0]];
            v.extend(notch);
            v.push([x1, z0]);
            v
        } else {
            // (x1, z1), the notches along z1 from x1 to x0, (x0, z1), (x0, z0), (x1, z0).
            let mut v = vec![[x1, z1]];
            v.extend(notch.into_iter().rev());
            v.extend([[x0, z1], [x0, z0], [x1, z0]]);
            v
        };
        pts.dedup();
        for [x, z] in pts {
            out.push_str(&format!("{indent}<point x=\"{x}\" y=\"0.0\" z=\"{z}\"/>{eol}"));
        }

        at = b;
    }

    out.extend(lines[at..].iter().copied());
    out
}

/// Over how much along the wall a zone region's edge goes from its line to a stair's side (the stepped bastion's: about one).
const ZONE_EASE: f32 = 1.0;

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

    /// Around a stair across the walkway the inner region takes its whole width and the outer gives way, as the stepped bastion's.
    #[test]
    fn zones_make_room_for_a_stair() {
        let text = "<regions>\n    <zone_regions>\n        <region name=\"region_zone01\">\n            <point x=\"11.5\" y=\"0.0\" z=\"6.0\"/>\n            \
                    <point x=\"-11.5\" y=\"0.0\" z=\"6.0\"/>\n            <point x=\"-11.5\" y=\"0.0\" z=\"0.0\"/>\n            <point x=\"11.5\" y=\"0.0\" z=\"0.0\"/>\n        \
                    </region>\n        <region name=\"region_zone02\">\n            <point x=\"11.5\" y=\"0.0\" z=\"9.3\"/>\n            <point x=\"-11.5\" y=\"0.0\" z=\"9.3\"/>\n            \
                    <point x=\"-11.5\" y=\"0.0\" z=\"6.2\"/>\n            <point x=\"11.5\" y=\"0.0\" z=\"6.2\"/>\n        </region>\n    </zone_regions>\n    <go_regions/>\n</regions>\n";
        let out = reshape_for_steps(text, &[(2.0, 5.0)], (0.5, 9.2));
        let regions = out
            .split("<region ")
            .skip(1)
            .map(|r| r.lines().filter_map(|l| point(l.trim_start())).map(|p| [p[0], p[2]]).collect::<Vec<_>>())
            .collect::<Vec<_>>();
        assert_eq!(regions[0], vec![[11.5, 6.0], [6.0, 6.0], [5.0, 0.3], [2.0, 0.3], [1.0, 6.0], [-11.5, 6.0], [-11.5, 0.0], [11.5, 0.0]]);
        assert_eq!(
            regions[1],
            vec![[11.5, 9.3], [-11.5, 9.3], [-11.5, 6.2], [1.0, 6.2], [2.0, 0.45], [5.0, 0.45], [6.0, 6.2], [11.5, 6.2]]
        );
        assert!(out.contains("<go_regions/>"));
        assert_eq!(reshape_for_steps(text, &[], (0.5, 9.2)), text);
    }

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
