//! Exploration: a campaign map's `.terry` y against its LF: for the props on the landscape (`ECLandscapeAbove` active), the LF sample
//! (0 .. 1) under them and a least-squares line y = a v + b, with the residuals.
//!
//!   cargo run --release -p wce_terrain_io --example campaign_y_fit -- <kit raw_data> <map>

use std::path::PathBuf;

use wce_terrain::terry::{EntityKind, TerryProject};
use wce_terrain::{LfRole, MapKind};
use wce_terrain_io::{MapFiles, attila};

fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let dir = PathBuf::from(&a[0]).join("terrain/campaigns").join(&a[1]);
    let files = MapFiles::from_dir(&dir).expect("folder");
    let m = attila::read(&a[1], MapKind::Campaign, &files).expect("map");
    let f = m.lf(LfRole::Land).expect("LF").get();
    let (fw, fh) = (f.width() as usize, f.height() as usize);
    let lf: Vec<u16> = f.to_le_rows().as_chunks::<2>().0.iter().map(|c| u16::from_le_bytes(*c)).collect();
    let p = TerryProject::parse(std::fs::read(dir.join(format!("{}.terry", a[1]))).expect("terry")).expect("xml");
    // The .terry frame: 0.32 a cell from the bottom-left corner, z north; the LF's samples a cell.
    let cells_w = fw as f64 / 8.0;
    let per = fw as f64 / cells_w;
    let mut pts = vec![];
    for e in &p.entities {
        if !matches!(p.kind(e), EntityKind::Prop(_)) {
            continue;
        }

        let above = p.component(e, "ECLandscapeAbove").and_then(|c| p.doc.get(c, "active"));
        if above.as_deref() != Some("true") {
            continue;
        }

        let w = p.world_position(e.id);
        let flip = std::env::var("FLIP").is_ok();
        let (cx, cy) = (w[0] / 0.32, if flip { w[2] / 0.32 } else { fh as f64 / per - w[2] / 0.32 });
        let (sx, sy) = ((cx * per) as usize, (cy * per) as usize);
        if sx >= fw || sy >= fh {
            continue;
        }

        pts.push((f64::from(lf[sy * fw + sx]) / 65535.0, w[1]));
    }

    let n = pts.len() as f64;
    let (sx, sy) = pts.iter().fold((0.0, 0.0), |a, p| (a.0 + p.0, a.1 + p.1));
    let (mx, my) = (sx / n, sy / n);
    let (sxy, sxx) = pts.iter().fold((0.0, 0.0), |a, p| (a.0 + (p.0 - mx) * (p.1 - my), a.1 + (p.0 - mx) * (p.0 - mx)));
    let slope = sxy / sxx;
    let b = my - slope * mx;
    let mut res: Vec<f64> = pts.iter().map(|p| (p.1 - (slope * p.0 + b)).abs()).collect();
    res.sort_by(f64::total_cmp);
    println!("{} props: y = {slope:.4} v + {b:.4}; residual median {:.4}, 90% {:.4}, max {:.4}", pts.len(), res[res.len() / 2], res[res.len() * 9 / 10], res[res.len() - 1]);
    for p in pts.iter().take(8) {
        println!("  v {:.5} y {:.4} fit {:.4}", p.0, p.1, slope * p.0 + b);
    }
}
