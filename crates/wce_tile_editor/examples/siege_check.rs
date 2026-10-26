//! Generate SiegeAI (docs/plans/siege_ai.md) on the kit's tiles, next to CA's layouts: per tile, what was made, what the checks find in
//! ours and in CA's, and an SVG of each (`<out>/<tile>_ours.svg`, `_ca.svg`).
//!
//!   cargo run --release -p wce_tile_editor --example siege_check -- <game folder> <out folder> <tile folder>... [--mods a.pack;b.pack]
//!
//! A tile folder is a variation's (holding tile.ted, or tile.wce: a project's, Rome 2's extracted tiles) or any folder above: every one
//! under it is taken. `$WCE_GAME_KEY` picks the game.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Instant;

use wce_siege::check::{check, check_captures};
use wce_tile::tile_project::TileProject;
use wce_tile_editor::tile_document::TileDocument;
use wce_vfs::files::Files;
use wce_vfs::{Config, GameProfile, Vfs};

fn tiles(dir: &Path, out: &mut Vec<PathBuf>) {
    if wce_tile::tile_project::has_tile(dir) {
        out.push(dir.to_path_buf());
        return;
    }

    let Ok(rd) = std::fs::read_dir(dir) else { return };
    let mut subs: Vec<PathBuf> = rd.filter_map(Result::ok).map(|e| e.path()).filter(|p| p.is_dir()).collect();
    subs.sort();
    for s in subs {
        tiles(&s, out);
    }
}

fn main() -> Result<(), String> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mods = args.iter().position(|a| a == "--mods").and_then(|i| args.get(i + 1)).cloned();
    let positional: Vec<&String> = args.iter().enumerate().filter(|(i, a)| *a != "--mods" && (*i == 0 || args[i - 1] != "--mods")).map(|(_, a)| a).collect();
    let [game, out, dirs @ ..] = positional.as_slice() else {
        return Err("usage: siege_check <game folder> <out folder> <tile folder>... [--mods a.pack;b.pack]".to_owned());
    };

    // `$WCE_GAME_KEY` picks the game (Attila by default).
    let mut config = Config::new(PathBuf::from(game), GameProfile::from_env().clone());
    config.mod_packs = mods.map(|m| m.split(';').filter(|s| !s.is_empty()).map(str::to_owned).collect());
    let files: Arc<dyn Files> = Arc::new(Vfs::mount(&config).map_err(|e| e.to_string())?);
    std::fs::create_dir_all(out).map_err(|e| e.to_string())?;

    let mut list = vec![];
    for d in dirs {
        tiles(Path::new(d), &mut list);
    }

    let (mut ours_bad, mut ca_bad, mut round_trip_bad) = (0, 0, 0);
    for dir in &list {
        let parts: Vec<String> = dir.components().rev().take(3).map(|c| c.as_os_str().to_string_lossy().into_owned()).collect();
        let name = parts.into_iter().rev().collect::<Vec<_>>().join("_");
        let project = match TileProject::open(dir) {
            Ok(p) => p,
            Err(e) => {
                println!("{name}: does not open: {e}");
                continue;
            }
        };

        let mut doc = TileDocument::new(project, Some(files.clone()));
        let t0 = Instant::now();
        let input = doc.tile().siege_input();
        let t1 = Instant::now();
        let ours = wce_siege::generate::generate(&input);
        let t2 = Instant::now();
        let ca = doc.tile().current_siege_layout();
        let (ro, rc) = (check(&ours), check(&ca));
        let (co, cc) = (check_captures(&ours, &input.captures), check_captures(&ca, &input.captures));
        ours_bad += usize::from(!ro.problems.is_empty() || !co.is_empty());
        ca_bad += usize::from(!rc.problems.is_empty() || !cc.is_empty());
        println!(
            "{name}: input {} fort pieces ({} gates), {} obstacles, {} no-go, {} captures{} in {:.1} s; generated in {:.1} s",
            input.walls.len(), input.walls.iter().filter(|w| w.gate).count(), input.obstacles.len(), input.nogo.len(), input.captures.len(),
            if input.sea.is_some() { ", sea" } else { "" }, (t1 - t0).as_secs_f64(), (t2 - t1).as_secs_f64(),
        );
        println!("  ours: {}", ours.summary());
        println!("  CA:   {}", ca.summary());
        for w in &ours.warnings {
            println!("  warning: {w}");
        }

        for (who, r, c) in [("ours", &ro, &co), ("CA", &rc, &cc)] {
            for p in r.problems.iter().chain(c.iter()).take(6) {
                println!("  {who} problem: {p}");
            }

            if r.problems.len() + c.len() > 6 {
                println!("  {who}: {} more problems", r.problems.len() + c.len() - 6);
            }

            if !r.notes.is_empty() {
                println!("  {who}: {} notes, e.g. {}", r.notes.len(), r.notes[0]);
            }
        }

        let o = Path::new(out);
        let write = |file: String, layout: &wce_siege::Layout, who: &str| {
            let title = format!("{name}: {who}. {}", layout.summary());
            std::fs::write(o.join(file), wce_siege::svg::svg(&input, layout, 1.0, &title)).map_err(|e| e.to_string())
        };
        write(format!("{name}_ours.svg"), &ours, "generated")?;
        write(format!("{name}_ca.svg"), &ca, "CA")?;

        // Into the document: what it then exports is what was generated (the old layout out of the export), and it survives a save.
        if ours.perimeter.len() >= 3 {
            let made = doc.generate_siege_ai()?;
            let back = doc.tile().current_siege_layout();
            let same = back.nodes.len() == made.nodes.len() && back.edges.len() == made.edges.len() && back.approaches.len() == made.approaches.len()
                && back.defender.polygons.len() == made.defender.polygons.len() && back.attacker.polygons.len() == made.attacker.polygons.len()
                && back.nodes.iter().zip(&made.nodes).all(|(a, b)| a.kind == b.kind && a.bbox.iter().zip(&b.bbox).all(|(p, q)| (p[0] - q[0]).abs() < 0.01 && (p[1] - q[1]).abs() < 0.01));
            if !same {
                round_trip_bad += 1;
                println!("  document: exports {} (generated {})", back.summary(), made.summary());
            }

            let text = String::from_utf8_lossy(&doc.tile().tscene().save()).into_owned();
            if wce_tile::ted_scene::TedScene::parse(text.into_bytes()).is_err() {
                round_trip_bad += 1;
                println!("  document: the saved scene does not read back");
            }
        }
    }

    println!("{} tiles: {ours_bad} with problems in ours, {ca_bad} in CA's, {round_trip_bad} not exported as generated", list.len());
    Ok(())
}
