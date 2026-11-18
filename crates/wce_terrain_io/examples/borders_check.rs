//! The campaign borders and trade routes (`wce_terrain_io::attila::overlay`) against RenderDoc captures of the game's draws: builds
//! `main_attila_map`'s overlay for the Western Roman Empire from the game's packs and compares every captured border curve (half-width
//! 0.225; land and sea chains apart) and trade-route curve (0.05) with ours: control points, uv offset and scale, vertex parameters,
//! and the borders' colour.
//!
//!   cargo run --release -p wce_terrain_io --example borders_check -- [map] [faction]
//!
//! `WCE_ATTILA_DIR` the game (default this machine's Steam folder); `WCE_BORDERS_CAPTURES` the capture folders, `;`-separated, each a
//! `bverts.txt` (one line a curve and side: `<event> cp x0 z0 .. x3 z3 uvoff u uvscale s hw h t [..] n k`) and optionally a `bd.txt`
//! (`== ev <event>` blocks with `g_border_colour_a=r g b a`); default `%TEMP%/wce_scratch/borders/rd/afull` and, when present,
//! `borders/rd/b`, `borders2/rd/rain`, `borders2/rd/w1`, `borders2/rd/w2`.

use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};

use wce_terrain_io::attila::borders::BorderChain;
use wce_terrain_io::attila::overlay::{self, CampaignOverlay};
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

/// The border colour of each draw (`bd.txt`), 0 to 255.
fn colours(path: &Path) -> HashMap<u32, [i32; 3]> {
    let mut out = HashMap::new();
    let mut ev = 0;
    for line in std::fs::read_to_string(path).unwrap_or_default().lines() {
        if let Some(rest) = line.strip_prefix("== ev ") {
            ev = rest.split_whitespace().next().and_then(|v| v.parse().ok()).unwrap_or(0);
        }
        if let Some(c) = line.split("g_border_colour_a=").nth(1) {
            let v: Vec<f32> = c.split(';').next().unwrap_or("").split_whitespace().filter_map(|x| x.parse().ok()).collect();
            if v.len() >= 3 {
                out.insert(ev, [0, 1, 2].map(|i| (v[i] * 255.0).round() as i32));
            }
        }
    }
    out
}

fn flat(cp: &[[f32; 2]; 4]) -> [f32; 8] {
    std::array::from_fn(|i| cp[i / 2][i % 2])
}

fn max_err(a: &[f32; 8], b: &[f32; 8]) -> f32 {
    a.iter().zip(b).map(|(x, y)| (x - y).abs()).fold(0.0, f32::max)
}

#[derive(Default)]
struct Tally {
    draws: usize,
    curves: usize,
    ok: usize,
    cp: f32,
    uv: f32,
    t: f32,
    colour_ok: usize,
    colour_seen: usize,
    bad: BTreeMap<u32, usize>,
}

impl Tally {
    fn print(&self, what: &str) {
        println!(
            "  {what}: {} draws, {} curves, {} within 1e-3; max cp err {:.3e}, uv {:.3e}, t {:.3e}; colour {}/{}{}",
            self.draws,
            self.curves,
            self.ok,
            self.cp,
            self.uv,
            self.t,
            self.colour_ok,
            self.colour_seen,
            if self.bad.is_empty() { String::new() } else { format!("; off (draw: curves) {:?}", self.bad) }
        );
    }
}

fn check(dir: &Path, o: &CampaignOverlay) {
    println!("== {}", dir.display());
    let col = colours(&dir.join("bd.txt"));
    let (mut land, mut sea) = (Tally::default(), Tally::default());
    let mut unmatched = vec![];
    for (ev, curves) in &capture(&dir.join("bverts.txt"), "0.225") {
        let p = (curves[0].cp[0], curves[0].cp[1]);
        let d = |c: &BorderChain| (c.curves[0].cp[0][0] - p.0).hypot(c.curves[0].cp[0][1] - p.1);
        let Some(chain) = o.borders.iter().min_by(|x, y| d(x).total_cmp(&d(y))) else { continue };
        if d(chain) > 0.01 {
            unmatched.push(*ev);
            continue;
        }
        let t = if chain.sea { &mut sea } else { &mut land };
        t.draws += 1;
        if let Some(c) = col.get(ev) {
            t.colour_seen += 1;
            t.colour_ok += usize::from(*c == chain.rgb.map(|v| (v * 255.0).round() as i32));
        }
        for (i, dr) in curves.iter().enumerate() {
            t.curves += 1;
            let Some(m) = chain.curves.get(i) else {
                *t.bad.entry(*ev).or_default() += 1;
                continue;
            };
            let e = max_err(&dr.cp, &flat(&m.cp));
            if e < 1e-3 {
                t.ok += 1;
                t.cp = t.cp.max(e);
                t.uv = t.uv.max((dr.uvoff - m.uv_offset).abs()).max((dr.uvscale - m.uv_scale).abs());
                for (x, y) in dr.ts.iter().zip(&m.ts) {
                    t.t = t.t.max((x - y).abs());
                }
            } else {
                *t.bad.entry(*ev).or_default() += 1;
            }
        }
    }
    land.print("land borders");
    sea.print("sea borders");
    if !unmatched.is_empty() {
        println!("  border draws starting at no chain of ours: {unmatched:?}");
    }

    // Trade routes: each draw against our drawn segment of its curve count nearest in control points.
    let cap = capture(&dir.join("bverts.txt"), "0.05");
    let (mut cp_w, mut uv_w, mut ts_w) = (0f32, 0f32, 0f32);
    let mut used = vec![];
    for curves in cap.values() {
        let best = o
            .trade_routes
            .iter()
            .filter(|s| s.drawn && s.curves.len() == curves.len())
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
    println!("  trade routes: {} draws matched to segments {used:?}; max cp err {cp_w:.3e}, uv {uv_w:.3e}, t {ts_w:.3e}", cap.len());
}

fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let map = a.first().map_or("main_attila_map", String::as_str);
    let faction = a.get(1).map_or("att_fact_western_roman_empire", String::as_str);
    let game_dir = std::env::var_os("WCE_ATTILA_DIR")
        .map_or_else(|| PathBuf::from("D:/Juegos/Steam/steamapps/common/Total War Attila"), PathBuf::from);
    let captures: Vec<PathBuf> = match std::env::var("WCE_BORDERS_CAPTURES") {
        Ok(v) => v.split(';').filter(|s| !s.is_empty()).map(PathBuf::from).collect(),
        Err(_) => {
            let t = std::env::temp_dir().join("wce_scratch");
            let a = t.join("borders/rd/afull");
            let more = ["borders/rd/b", "borders2/rd/rain", "borders2/rd/w1", "borders2/rd/w2"].map(|p| t.join(p));
            std::iter::once(a).chain(more.into_iter().filter(|p| p.join("bverts.txt").is_file())).collect()
        }
    };

    let game = GameProfile::from_env().clone();
    let mut cfg = Config::new(&game_dir, game.clone());
    cfg.mod_packs = Some(vec![]);
    let vfs = Vfs::mount(&cfg).expect("vfs");
    let schema =
        rpfm_lib::schema::Schema::load(&wce_foundation::settings::rpfm_schema_path(&game).expect("an rpfm schema"), None).expect("schema");
    let list = |p: &str| vfs.list(p, "").iter().map(|s| (*s).to_owned()).collect::<Vec<_>>();
    let read = |p: &str| vfs.read(p).ok().map(|b| b.to_vec());
    let startpos = |p: &str| read(p).or_else(|| std::fs::read(game_dir.join("data").join(p)).ok());

    if let Some(b) = read(&overlay::pbd_path(map)) {
        let d = wce_terrain_io::attila::borders::decode_pbd(&b).expect("pbd");
        let segs: usize = d.entries.iter().map(|e| e.segments.len()).sum();
        println!(
            "borders.pbd: version {:?}, {} keys, {} entries, {segs} segments, crc ok {}",
            d.version,
            d.keys.len(),
            d.entries.len(),
            d.crc_ok
        );
    }
    if let Some(b) = read(&overlay::ptd_path(map)) {
        let d = wce_terrain_io::attila::trade_routes::decode_ptd(&b).expect("ptd");
        println!(
            "trade_routes.ptd: version {}, {} keys, {} segments, tables {} / {}, crc ok {}",
            d.version,
            d.keys.len(),
            d.segments.len(),
            d.tables[0].len(),
            d.tables[1].len(),
            d.crc_ok
        );
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
        let kind = format!("{}{:?}{}", if c.sea { "sea " } else { "" }, c.colour, if c.internal { " internal" } else { "" });
        *classes.entry(kind).or_default() += 1;
    }
    println!("chains by colour: {classes:?}");
    println!(
        "trade segments: {} ({} drawn, {} sea legs)",
        o.trade_routes.len(),
        o.trade_routes.iter().filter(|s| s.drawn).count(),
        o.trade_routes.iter().filter(|s| s.sea_leg).count()
    );

    for dir in &captures {
        check(dir, &o);
    }
}
