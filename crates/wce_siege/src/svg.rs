//! A layout drawn as SVG (to look at what the generator made, next to CA's): the obstacles and walls of its input, the nodes' rectangles
//! and Boundaries, the edges, the perimeter, the approaches and the deployment zones.

use std::fmt::Write as _;

use crate::{EdgeKind, Input, Layout, NodeKind, P};

/// The drawing, `scale` pixels per unit, `title` on top.
pub fn svg(input: &Input, layout: &Layout, scale: f64, title: &str) -> String {
    // Around the perimeter (and the approaches), else the whole map.
    let mut pts: Vec<P> = layout.perimeter.clone();
    pts.extend(layout.approaches.iter().flatten());
    let (lo, hi) = if pts.is_empty() {
        input.bounds
    } else {
        let lo = pts.iter().fold([f64::MAX; 2], |a, p| [a[0].min(p[0]), a[1].min(p[1])]);
        let hi = pts.iter().fold([f64::MIN; 2], |a, p| [a[0].max(p[0]), a[1].max(p[1])]);
        ([lo[0] - 60.0, lo[1] - 60.0], [hi[0] + 60.0, hi[1] + 60.0])
    };
    let (w, h) = ((hi[0] - lo[0]) * scale, (hi[1] - lo[1]) * scale);
    let mut s = String::new();
    let _ = writeln!(s, r#"<svg xmlns="http://www.w3.org/2000/svg" width="{w:.0}" height="{:.0}" viewBox="0 -20 {w:.0} {:.0}" style="background:#fafaf5">"#, h + 20.0, h + 20.0);
    let _ = writeln!(s, r#"<text x="4" y="-6" font-size="12" font-family="sans-serif">{}</text>"#, escape(title));
    let pt = |p: &P| format!("{:.1},{:.1}", (p[0] - lo[0]) * scale, (p[1] - lo[1]) * scale);
    let poly = |s: &mut String, pts: &[P], closed: bool, style: &str| {
        if pts.len() < 2 {
            return;
        }

        let tag = if closed { "polygon" } else { "polyline" };
        let list = pts.iter().map(pt).collect::<Vec<_>>().join(" ");
        let _ = writeln!(s, r#"<{tag} points="{list}" style="fill-rule:evenodd;{style}"/>"#);
    };

    for (k, p) in &layout.attacker.polygons {
        let fill = match k {
            crate::ZoneKind::Additive => "fill:#e33;fill-opacity:0.08;stroke:#c00;stroke-width:1",
            crate::ZoneKind::Subtractive => "fill:#fff;fill-opacity:0.5;stroke:#c00;stroke-width:1;stroke-dasharray:4 2",
            _ => "fill:none;stroke:#c00;stroke-width:0.5;stroke-dasharray:1 3",
        };
        poly(&mut s, p, true, fill);
    }

    for (k, p) in &layout.defender.polygons {
        let fill = if *k == crate::ZoneKind::Additive { "fill:#33e;fill-opacity:0.06;stroke:#00c;stroke-width:1" } else { "fill:none;stroke:#00c;stroke-width:0.5;stroke-dasharray:1 3" };
        poly(&mut s, p, true, fill);
    }

    for p in &input.nogo {
        poly(&mut s, p, true, "fill:#f4c7c7;stroke:#d88;stroke-width:0.5");
    }

    for p in &input.obstacles {
        poly(&mut s, p, true, "fill:#ccc;stroke:none");
    }

    for wpc in &input.walls {
        for r in &wpc.footprint {
            poly(&mut s, r, true, if wpc.gate { "fill:#a3c;stroke:none" } else { "fill:#888;stroke:none" });
        }
    }

    poly(&mut s, &layout.perimeter, true, "fill:none;stroke:#36c;stroke-width:1.5");
    for n in &layout.nodes {
        let colour = match n.kind {
            NodeKind::Area => "#2a2",
            NodeKind::Intersection => "#36f",
            NodeKind::Entry => "#c0c",
        };
        poly(&mut s, &n.boundary, true, &format!("fill:{colour};fill-opacity:0.18;stroke:{colour};stroke-width:0.8"));
        poly(&mut s, &n.bbox, true, &format!("fill:{colour};fill-opacity:0.25;stroke:{colour};stroke-width:0.5"));
    }

    for e in &layout.edges {
        let style = match e.kind {
            EdgeKind::Street => "fill:none;stroke:#000;stroke-width:1.2",
            EdgeKind::AreaConnection => "fill:none;stroke:#0bb;stroke-width:2",
            EdgeKind::Wall => "fill:none;stroke:#e00;stroke-width:2",
        };
        poly(&mut s, &e.line, false, style);
        for side in &e.sides {
            poly(&mut s, side, false, "fill:none;stroke:#e6a;stroke-width:0.7");
        }
    }

    for a in &layout.approaches {
        poly(&mut s, a, false, "fill:none;stroke:#088;stroke-width:2.5");
    }

    for c in &input.captures {
        let _ = writeln!(s, r##"<circle cx="{:.1}" cy="{:.1}" r="{}" style="fill:#fc0;stroke:#a80"/>"##, (c.at[0] - lo[0]) * scale, (c.at[1] - lo[1]) * scale, if c.major { 5 } else { 3 });
    }

    s.push_str("</svg>\n");
    s
}

fn escape(t: &str) -> String {
    t.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;")
}
