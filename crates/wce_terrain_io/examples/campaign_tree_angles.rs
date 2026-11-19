//! The campaign forests' instances in a RenderDoc capture (`scripts/re/rd_campaign_frame.py`'s `instances.csv`: the tree clumps' (x, y,
//! z, angle) in the campaign world's units) against the map's `trees.campaign_tree_list`: each instance's nearest tree of the list, its
//! bytes and the angle the game drew it at (the list's turn byte against the shader's angle; docs/re/terry.md, "Forests").
//!
//!   cargo run --release -p wce_terrain_io --example campaign_tree_angles -- <frame folder> <event> [map]
//!
//! `WCE_ATTILA_DIR` the game (default this machine's Steam folder); the profile from `WCE_GAME`.

use std::io::Cursor;
use std::path::PathBuf;

use rpfm_lib::files::Decodeable;
use rpfm_lib::files::rigidmodel::RigidModel;

use wce_vfs::{Config, GameProfile, Vfs};

struct Tree {
    model: usize,
    p: [f32; 3],
    b: [u8; 3],
}

fn parse(b: &[u8]) -> Option<(Vec<String>, Vec<Tree>)> {
    let u32_at = |o: usize| b.get(o..o + 4).map(|s| u32::from_le_bytes([s[0], s[1], s[2], s[3]]));
    let f32_at = |o: usize| b.get(o..o + 4).map(|s| f32::from_le_bytes([s[0], s[1], s[2], s[3]]));
    let mut o = 4 + 16;
    let models = u32_at(o)? as usize;
    o += 4;
    let (mut names, mut trees) = (vec![], vec![]);
    for m in 0..models {
        let n = u16::from_le_bytes([*b.get(o)?, *b.get(o + 1)?]) as usize;
        names.push(String::from_utf8_lossy(b.get(o + 2..o + 2 + n)?).into_owned());
        o += 2 + n;
        let count = u32_at(o)? as usize;
        o += 4;
        for _ in 0..count {
            trees.push(Tree {
                model: m,
                p: [f32_at(o)?, f32_at(o + 4)?, f32_at(o + 8)?],
                b: [*b.get(o + 12)?, *b.get(o + 13)?, *b.get(o + 14)?],
            });
            o += 15;
        }
    }
    Some((names, trees))
}

fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let (Some(dir), Some(event)) = (a.first(), a.get(1)) else {
        eprintln!("usage: campaign_tree_angles <frame folder> <event> [map]");
        return;
    };
    let map = a.get(2).map_or("main_attila_map", String::as_str);
    let game_dir = std::env::var_os("WCE_ATTILA_DIR")
        .map_or_else(|| PathBuf::from("D:/Juegos/Steam/steamapps/common/Total War Attila"), PathBuf::from);
    let mut cfg = Config::new(&game_dir, GameProfile::from_env().clone());
    cfg.mod_packs = Some(vec![]);
    let vfs = Vfs::mount(&cfg).expect("vfs");
    let list = vfs.read(&format!("campaign_maps/{map}/display/trees/trees.campaign_tree_list")).expect("the tree list");
    let (names, trees) = parse(&list).expect("a tree list");
    println!("{} models, {} trees", names.len(), trees.len());

    // Each model's first mesh's pivot times the shader's 0.4 (what the game adds to the list's position, if it does).
    let pivots: Vec<Option<[f32; 3]>> = names
        .iter()
        .map(|n| {
            let b = vfs.read(&n.replace(char::from(92), "/").to_lowercase()).ok()?;
            let m = RigidModel::decode(&mut Cursor::new(b.to_vec()), &None).ok()?;
            let p = *m.lods().first()?.mesh_blocks().first()?.material().v_pivot();
            Some([p.x * 0.4, p.y * 0.4, p.z * 0.4])
        })
        .collect();

    // The models' own textures (what the game swaps by season).
    for n in &names {
        let Ok(b) = vfs.read(&n.replace(char::from(92), "/").to_lowercase()) else { continue };
        let Ok(m) = RigidModel::decode(&mut Cursor::new(b.to_vec()), &None) else { continue };
        let Some(block) = m.lods().first().and_then(|l| l.mesh_blocks().first()) else { continue };
        let t: Vec<String> = block.material().textures().iter().map(|t| format!("{:?}={}", t.tex_type(), t.path())).collect();
        println!("model {}: {}", n.rsplit(char::from(92)).next().unwrap_or(n), t.join(" "));
    }

    let csv = std::fs::read_to_string(PathBuf::from(dir).join("instances.csv")).expect("instances.csv");
    let prefix = format!("{event},");
    for line in csv.lines().filter(|l| l.starts_with(&prefix)) {
        let v: Vec<f32> = line.split(',').nth(2).unwrap_or("").split_whitespace().filter_map(|x| x.parse().ok()).collect();
        if v.len() < 4 {
            continue;
        }
        let d2 = |t: &Tree| (t.p[0] - v[0]).powi(2) + (t.p[2] - v[2]).powi(2);
        let Some(t) = trees.iter().min_by(|x, y| d2(x).total_cmp(&d2(y))) else { continue };
        let sixths = v[3] / (std::f32::consts::PI / 3.0);
        println!(
            "capture ({:.3}, {:.3}, {:.3}) angle {:.4} = {:.2} sixths | list ({:.3}, {:.3}, {:.3}) at {:.4} bytes {:?} {} pivot*0.4 {:?} delta {:.4} {:.4} {:.4}",
            v[0],
            v[1],
            v[2],
            v[3],
            sixths,
            t.p[0],
            t.p[1],
            t.p[2],
            d2(t).sqrt(),
            t.b,
            names[t.model].rsplit('/').next().unwrap_or(""),
            pivots[t.model].map(|p| p.map(|v| (v * 1e4).round() / 1e4)),
            v[0] - t.p[0],
            v[1] - t.p[1],
            v[2] - t.p[2]
        );
    }
}
