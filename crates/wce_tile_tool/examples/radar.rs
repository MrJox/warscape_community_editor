//! Draw the radar (docs/plans/radar.md) of tiles to PNGs, to work on its look: `<out>/<tile>_radar.png`, and the tile's own
//! `loading_screen_radar.jpg` beside it when it has one (`<tile>_ca.jpg`).
//!
//!   cargo run --release -p wce_tile_tool --example radar -- <game folder> <out folder> <tile folder>... [--mods a.pack;b.pack] [--kit <kit>]
//!
//! A tile folder is a variation's (holding tile.ted or tile.wce). `$WCE_GAME_KEY` picks the game; `$RADAR_TREES` the tree list;
//! `$RADAR_TREE_DUMP` / `$RADAR_SLOPE_DUMP` write the trees / the slopes beside it (1 m a pixel), `$RADAR_DEBUG` prints the input.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Instant;

use wce_formats::png_file::PngFile;
use wce_tile::tile_project::TileProject;
use wce_tile_editor::radar::RadarStyle;
use wce_tile_editor::tile_document::TileDocument;
use wce_vfs::files::Files;
use wce_vfs::{Config, GameProfile, Vfs};

fn flag(args: &[String], name: &str) -> Option<String> {
    args.iter().position(|a| a == name).and_then(|i| args.get(i + 1)).cloned()
}

fn main() -> Result<(), String> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mods = flag(&args, "--mods");
    let kit = flag(&args, "--kit").map(PathBuf::from);
    let positional: Vec<&String> = args.iter().enumerate().filter(|(i, a)| !a.starts_with("--") && (*i == 0 || !args[i - 1].starts_with("--"))).map(|(_, a)| a).collect();
    let [game, out, dirs @ ..] = positional.as_slice() else {
        return Err("usage: radar <game folder> <out folder> <tile folder>... [--mods a.pack;b.pack] [--kit <kit>]".to_owned());
    };

    let mut config = Config::new(PathBuf::from(game), GameProfile::from_env().clone());
    config.mod_packs = mods.map(|m| m.split(';').filter(|s| !s.is_empty()).map(str::to_owned).collect());
    let vfs = Arc::new(Vfs::mount(&config).map_err(|e| e.to_string())?);
    let files: Arc<dyn Files> = vfs.clone();
    let game = match wce_tile_io::attila::workflow::Game::from_vfs(vfs, kit.as_deref()) {
        Ok(g) => Some(g),
        Err(e) => {
            println!("no game data (water planes left out): {e}");
            None
        }
    };
    std::fs::create_dir_all(out).map_err(|e| e.to_string())?;

    let style = RadarStyle::default();
    for d in dirs {
        let dir = Path::new(d);
        let parts: Vec<String> = dir.components().rev().take(3).map(|c| c.as_os_str().to_string_lossy().into_owned()).collect();
        let name = parts.into_iter().rev().collect::<Vec<_>>().join("_");
        let project = match TileProject::open(dir) {
            Ok(p) => p,
            Err(e) => {
                println!("{name}: does not open: {e}");
                continue;
            }
        };

        let t0 = Instant::now();
        let mut doc = TileDocument::new(project, Some(files.clone()));
        // `$RADAR_TREES` picks the tree list (the editor shows the climate's).
        if let Ok(list) = std::env::var("RADAR_TREES") {
            doc.tile_mut().display_mut().tree_list = list;
        }
        let water = match wce_tile_tool::stage_b_of(&doc, game.as_ref(), true) {
            Ok((w, nogos)) => {
                doc.tile_mut().set_auto_nogos(nogos);
                Some(w)
            }
            Err(e) => {
                println!("{name}: no stage B ({e}): no rivers or cliffs");
                None
            }
        };
        let t1 = Instant::now();
        let input = doc.tile_mut().radar_input(water.as_ref());
        let t2 = Instant::now();
        let img = wce_tile_editor::radar::render(&input, &style);
        let t3 = Instant::now();
        println!(
            "{name}: {} trees, {} buildings, {} wall pieces, {} streets, {} squares, {} cliffs{}{}{}; stage B {:.1} s, input {:.1} s, drawn {:.1} s",
            input.trees.len(), input.buildings.len(), input.walls.len(), input.streets.len(), input.plazas.len(), input.cliffs.len(),
            if input.sea { ", sea" } else { "" }, if input.water.is_some() { ", water" } else { "" }, if input.roads.is_some() { ", roads" } else { "" },
            (t1 - t0).as_secs_f64(), (t2 - t1).as_secs_f64(), (t3 - t2).as_secs_f64(),
        );

        if std::env::var("RADAR_SLOPE_DUMP").is_ok() {
            // The slope (white 45 degrees and up), the sea blue, the steep outlines red: 1 m a pixel.
            let [ax, az, aw, ah] = input.area;
            let (w, h) = (aw as usize, ah as usize);
            let hf = &input.height;
            let mut px = vec![255u8; w * h * 4];
            for j in 0..h {
                for i in 0..w {
                    let (x, z) = (ax + i as f64 + 0.5, az + ah - j as f64 - 0.5);
                    let y = hf.sample(x, z);
                    let (dx, dz) = (hf.sample(x + 1.0, z) - hf.sample(x - 1.0, z), hf.sample(x, z + 1.0) - hf.sample(x, z - 1.0));
                    let deg = (f64::from((dx * dx + dz * dz).sqrt() * 0.5)).atan().to_degrees();
                    let v = (deg / 45.0 * 255.0).min(255.0) as u8;
                    let k = (j * w + i) * 4;
                    px[k..k + 3].copy_from_slice(&if y < 0.0 { [v / 2, v / 2, 160] } else { [v, v, v] });
                }
            }
            let png = PngFile { width: w as u32, height: h as u32, bit_depth: 8, color_type: 6, pixels: px, pixels_edited: true, ..PngFile::default() };
            std::fs::write(Path::new(out).join(format!("{name}_slope.png")), png.encode().map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
        }

        if std::env::var("RADAR_TREE_DUMP").is_ok() {
            // The trees, a dark pixel each, 1 m a pixel.
            let [ax, az, aw, ah] = input.area;
            let (w, h) = (aw as usize, ah as usize);
            let mut px = vec![255u8; w * h * 4];
            for t in &input.trees {
                let (i, j) = ((t[0] - ax).floor(), (az + ah - t[1]).floor());
                if i >= 0.0 && j >= 0.0 && (i as usize) < w && (j as usize) < h {
                    let k = (j as usize * w + i as usize) * 4;
                    px[k..k + 3].copy_from_slice(&[0, 90, 0]);
                }
            }
            let png = PngFile { width: w as u32, height: h as u32, bit_depth: 8, color_type: 6, pixels: px, pixels_edited: true, ..PngFile::default() };
            std::fs::write(Path::new(out).join(format!("{name}_trees.png")), png.encode().map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
        }

        if std::env::var("RADAR_DEBUG").is_ok() {
            let t = doc.tile();
            println!("  area {:?}, extent {} x {}, unit {}, border {}", input.area, t.extent_x(), t.extent_z(), t.unit(), doc.project().border_m());
            println!("  height field {} x {} over {:?}", input.height.w, input.height.h, [input.height.x0, input.height.z0, input.height.w_m, input.height.h_m]);
            for b in input.buildings.iter().take(3) {
                println!("  building {} polygons, first {:?}", b.polys.len(), b.polys.first());
            }
            for w in input.walls.iter().take(2) {
                println!("  wall {:?}", w.footprint.iter().map(|r| r.len()).collect::<Vec<_>>());
            }
            println!("  street {:?}", input.streets.first().map(|s| (&s.0[..s.0.len().min(3)], s.1)));
        }

        let png = PngFile { width: img.size, height: img.size, bit_depth: 8, color_type: 6, pixels: img.rgba, pixels_edited: true, ..PngFile::default() };
        let bytes = png.encode().map_err(|e| e.to_string())?;
        std::fs::write(Path::new(out).join(format!("{name}_radar.png")), bytes).map_err(|e| e.to_string())?;
        let ca = dir.join("loading_screen_radar.jpg");
        if ca.exists() {
            let _ = std::fs::copy(&ca, Path::new(out).join(format!("{name}_ca.jpg")));
        }
    }

    Ok(())
}
