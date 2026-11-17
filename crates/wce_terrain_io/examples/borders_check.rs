//! The campaign borders and trade routes (`wce_terrain_io::attila::overlay`) against a RenderDoc capture of the game's draws: builds
//! `main_attila_map`'s overlay for the Western Roman Empire from the game's packs and compares every captured border curve (half-width
//! 0.225) and trade-route curve (0.05) with ours: control points, uv offset and scale, vertex parameters.
//!
//!   cargo run --release -p wce_terrain_io --example borders_check -- [map] [faction]
//!
//! `WCE_ATTILA_DIR` the game (default this machine's Steam folder), `WCE_BORDERS_CAPTURE` the capture's curve dump (one line a curve
//! and side: `<event> cp x0 z0 .. x3 z3 uvoff u uvscale s hw h t [..] n k`; default `%TEMP%/wce_scratch/borders/rd/afull/bverts.txt`).

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use wce_terrain_io::attila::overlay;
use wce_vfs::{Config, GameProfile, Vfs};

struct Drawn {
    cp: [f32; 8],
    uvoff: f32,
    uvscale: f32,
    ts: Vec<f32>,
}

fn capture(path: &Path, hw: &str) -> BTreeMap<u32, Vec<Drawn>> {
    let mut out: BTreeMap<u32, Vec<Drawn>> = BTreeMap::new();
    let text = std::fs::read_to_string(path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    for line in text.lines() {
        let w: Vec<&str> = line.split_whitespace().collect();
        let at = |k: &str| w.iter().position(|x| *x == k).and_then(|i| w.get(i + 1)).copied().unwrap_or("");
        if at("hw") != hw {
            continue;
        }
        let num = |s: &str| s.trim_matches(|c| c == '[' || c == ']' || c == ',').parse::<f32>().unwrap_or(f32::NAN);
        let cp: [f32; 8] = std::array::from_fn(|i| num(w[2 + i]));
        let ts = line.split('[').nth(1).and_then(|s| s.split(']').next()).unwrap_or("").split(',').map(|s| num(s.trim())).collect();
        out.entry(w[0].parse().unwrap_or(0)).or_default().push(Drawn { cp, uvoff: num(at("uvoff")), uvscale: num(at("uvscale")), ts });
    }
    out
}

fn flat(cp: &[[f32; 2]; 4]) -> [f32; 8] {
    std::array::from_fn(|i| cp[i / 2][i % 2])
}

fn max_err(a: &[f32; 8], b: &[f32; 8]) -> f32 {
    a.iter().zip(b).map(|(x, y)| (x - y).abs()).fold(0.0, f32::max)
}

fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let map = a.first().map_or("main_attila_map", String::as_str);
    let faction = a.get(1).map_or("att_fact_western_roman_empire", String::as_str);
    let game_dir = std::env::var_os("WCE_ATTILA_DIR")
        .map_or_else(|| PathBuf::from("D:/Juegos/Steam/steamapps/common/Total War Attila"), PathBuf::from);
    let cap_path = std::env::var_os("WCE_BORDERS_CAPTURE")
        .map_or_else(|| std::env::temp_dir().join("wce_scratch/borders/rd/afull/bverts.txt"), PathBuf::from);

    let game = GameProfile::from_env().clone();
    let mut cfg = Config::new(&game_dir, game.clone());
    cfg.mod_packs = Some(vec![]);
    let vfs = Vfs::mount(&cfg).expect("vfs");
    let schema =
        rpfm_lib::schema::Schema::load(&wce_foundation::settings::rpfm_schema_path(&game).expect("an rpfm schema"), None).expect("schema");
    let list = |p: &str| vfs.list(p, "").iter().map(|s| (*s).to_owned()).collect::<Vec<_>>();
    let read = |p: &str| vfs.read(p).ok().map(|b| b.to_vec());
    let startpos = |p: &str| read(p).or_else(|| std::fs::read(game_dir.join("data").join(p)).ok());

    for p in [overlay::pbd_path(map), overlay::ptd_path(map)] {
        let found = read(&p).map(|b| b.len());
        println!("{p}: {found:?} bytes");
        if let Some(b) = read(&p) {
            if p.ends_with(".pbd") {
                let d = wce_terrain_io::attila::borders::decode_pbd(&b).expect("pbd");
                let segs: usize = d.entries.iter().map(|e| e.segments.len()).sum();
                println!(
                    "  version {:?}, {} keys, {} entries, {segs} segments, crc ok {}",
                    d.version,
                    d.keys.len(),
                    d.entries.len(),
                    d.crc_ok
                );
            } else {
                let d = wce_terrain_io::attila::trade_routes::decode_ptd(&b).expect("ptd");
                println!(
                    "  version {}, {} keys, {} segments, tables {} / {}, crc ok {}",
                    d.version,
                    d.keys.len(),
                    d.segments.len(),
                    d.tables[0].len(),
                    d.tables[1].len(),
                    d.crc_ok
                );
            }
        }
    }

    let (infos, default) = overlay::factions(&schema, map, &list, &read, &startpos).expect("factions");
    println!("{} factions, {} playable, default {default:?}", infos.len(), infos.iter().filter(|i| i.playable).count());
    let t = std::time::Instant::now();
    let o = overlay::read(&schema, map, Some(faction), &list, &read, &startpos).expect("overlay");
    println!("campaign {}, faction {}, grid {:?}, built in {:.2?}", o.campaign, o.faction, o.grid, t.elapsed());
    for n in &o.notes {
        println!("  {n}");
    }
    let mut classes: BTreeMap<String, usize> = BTreeMap::new();
    for c in &o.borders {
        *classes.entry(format!("{:?}{}", c.colour, if c.internal { " internal" } else { "" })).or_default() += 1;
    }
    println!("chains by colour: {classes:?}");

    // Borders: each captured draw against the chain starting nearest its first point (summary.py's comparison).
    let cap = capture(&cap_path, "0.225");
    let (mut total, mut ok, mut cp_w, mut uv_w, mut ts_w) = (0, 0, 0f32, 0f32, 0f32);
    let mut bad: BTreeMap<u32, usize> = BTreeMap::new();
    for (ev, curves) in &cap {
        let p = (curves[0].cp[0], curves[0].cp[1]);
        let Some(chain) = o.borders.iter().min_by(|x, y| {
            let d = |c: &wce_terrain_io::attila::borders::BorderChain| (c.curves[0].cp[0][0] - p.0).hypot(c.curves[0].cp[0][1] - p.1);
            d(x).total_cmp(&d(y))
        }) else {
            continue;
        };
        for (i, d) in curves.iter().enumerate() {
            total += 1;
            let Some(m) = chain.curves.get(i) else {
                *bad.entry(*ev).or_default() += 1;
                continue;
            };
            let e = max_err(&d.cp, &flat(&m.cp));
            if e < 1e-3 {
                ok += 1;
                cp_w = cp_w.max(e);
                uv_w = uv_w.max((d.uvoff - m.uv_offset).abs()).max((d.uvscale - m.uv_scale).abs());
                for (x, y) in d.ts.iter().zip(&m.ts) {
                    ts_w = ts_w.max((x - y).abs());
                }
            } else {
                *bad.entry(*ev).or_default() += 1;
            }
        }
    }
    println!(
        "borders: {} draws, {total} curves, {ok} within 1e-3; among them max cp err {cp_w:.3e}, uv {uv_w:.3e}, t {ts_w:.3e}",
        cap.len()
    );
    println!("  draws with curves off (draw: curves): {bad:?}");

    // Trade routes: each captured draw against our segment of its curve count nearest in control points (match.py's comparison).
    let cap = capture(&cap_path, "0.05");
    let (mut cp_w, mut uv_w, mut ts_w) = (0f32, 0f32, 0f32);
    let mut used = vec![];
    for curves in cap.values() {
        let best = o
            .trade_routes
            .iter()
            .filter(|s| s.curves.len() == curves.len())
            .map(|s| (curves.iter().zip(&s.curves).map(|(d, m)| max_err(&d.cp, &flat(&m.cp))).fold(0.0, f32::max), s))
            .min_by(|x, y| x.0.total_cmp(&y.0));
        let Some((e, s)) = best else {
            println!("  trade draw of {} curves: no segment", curves.len());
            continue;
        };
        cp_w = cp_w.max(e);
        used.push(s.segment);
        for (d, m) in curves.iter().zip(&s.curves) {
            uv_w = uv_w.max((d.uvoff - m.uv_offset).abs()).max((d.uvscale - m.uv_scale).abs());
            for (x, y) in d.ts.iter().zip(&m.ts) {
                ts_w = ts_w.max((x - y).abs());
            }
        }
    }
    println!(
        "trade routes: {} draws matched to segments {used:?}; max cp err {cp_w:.3e}, uv {uv_w:.3e}, t {ts_w:.3e}; ours {} segments ({} sea legs)",
        cap.len(),
        o.trade_routes.len(),
        o.trade_routes.iter().filter(|s| s.sea_leg).count()
    );
}
