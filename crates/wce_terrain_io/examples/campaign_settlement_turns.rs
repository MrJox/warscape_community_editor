//! The settlements' buildings and sprawl in a RenderDoc capture (`scripts/re/rd_campaign_frame.py`'s folder: the rigid models' instance
//! matrices in the G-buffer) against `wce_terrain_io::attila::cities`: each slot and sprawl piece's nearest instance, its distance, the
//! angle the game turned it by (degrees, about y) and its scale. The sprawl's turns are known (the hex's byte times 60 degrees): they
//! check the angle's convention; the slots' turns are what the startpos does not hold (docs/re/terry.md, "Settlement models").
//!
//!   cargo run --release -p wce_terrain_io --example campaign_settlement_turns -- <frame folder> [campaign] [map] [region filter]
//!
//! `WCE_ATTILA_DIR` the game (default this machine's Steam folder); the profile from `WCE_GAME`.

use std::collections::HashMap;
use std::path::PathBuf;

use wce_terrain_io::attila::cities::{DisplayTables, read};
use wce_vfs::{Config, GameProfile, Vfs};

struct Inst {
    event: u32,
    x: f64,
    z: f64,
    angle: f64,
    scale: f64,
}

fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let Some(dir) = a.first().map(PathBuf::from) else {
        eprintln!("usage: campaign_settlement_turns <frame folder> [campaign] [map] [region filter]");
        return;
    };
    let campaign = a.get(1).map_or("main_attila", String::as_str);
    let map = a.get(2).map_or("main_attila_map", String::as_str);
    let filter = a.get(3).cloned().unwrap_or_default();
    let game_dir = std::env::var_os("WCE_ATTILA_DIR")
        .map_or_else(|| PathBuf::from("D:/Juegos/Steam/steamapps/common/Total War Attila"), PathBuf::from);
    let game = GameProfile::from_env().clone();
    let mut cfg = Config::new(&game_dir, game.clone());
    cfg.mod_packs = Some(vec![]);
    let vfs = Vfs::mount(&cfg).expect("vfs");
    let schema =
        rpfm_lib::schema::Schema::load(&wce_foundation::settings::rpfm_schema_path(&game).expect("an rpfm schema"), None).expect("schema");
    let list = |p: &str| vfs.list(p, "").iter().map(|s| (*s).to_owned()).collect::<Vec<_>>();
    let read_vfs = |p: &str| vfs.read(p).ok().map(|b| b.to_vec());
    let tables = DisplayTables::read(&schema, &list, &read_vfs);
    // The settlements' rotations table (`campaign_settlement_display_settlement_rotations`): its rows.
    let rotations =
        wce_terrain_io::attila::cities::table_rows(&schema, "campaign_settlement_display_settlement_rotations_tables", &list, &read_vfs);
    println!("settlement rotations: {} rows", rotations.len());
    for r in rotations.iter().take(12) {
        println!("  {r:?}");
    }
    let map_data = vfs.read(&format!("campaign_maps/{map}/map_data.esf")).expect("map_data.esf");
    let startpos = std::fs::read(game_dir.join(format!("data/campaigns/{campaign}/startpos.esf"))).expect("startpos.esf");
    let cities = read(&map_data, &startpos, &tables).expect("cities");

    // The rigid G-buffer draws' instances (rigidcommon vs30_main into the G-buffer's targets).
    let csv = |name: &str| -> Vec<Vec<String>> {
        std::fs::read_to_string(dir.join(name))
            .unwrap_or_else(|e| panic!("{name}: {e}"))
            .lines()
            .skip(1)
            .map(|l| l.split(',').map(str::to_owned).collect())
            .collect()
    };
    let names: HashMap<String, String> = csv("shader_names.csv").into_iter().map(|r| (r[0].clone(), r[3].clone())).collect();
    let rigid: std::collections::HashSet<u32> = csv("events.csv")
        .into_iter()
        .filter(|r| r.len() > 8 && r[1] == "draw" && r[5].split(' ').count() >= 3)
        .filter(|r| names.get(&r[7]).is_some_and(|n| n.ends_with("rigidcommon.hlsl_vs_vs30_main")))
        .filter_map(|r| r[0].parse().ok())
        .collect();
    // Each rigid draw's diffuse texture (`texture_names.csv` from `campaign_frame --textures`, where named).
    let tex_names: HashMap<String, String> = std::fs::read_to_string(dir.join("texture_names.csv"))
        .unwrap_or_default()
        .lines()
        .skip(1)
        .filter_map(|l| l.split_once(',').map(|(a, b)| (a.to_owned(), b.to_owned())))
        .collect();
    let diffuse: HashMap<u32, String> = csv("events.csv")
        .into_iter()
        .filter_map(|r| {
            let e: u32 = r.first()?.parse().ok()?;
            let id = r.get(13)?.split(' ').find_map(|t| t.strip_prefix("s_diffuse_map="))?.to_owned();
            Some((e, tex_names.get(&id).cloned().unwrap_or(format!("#{id}"))))
        })
        .collect();
    let mut inst = vec![];
    for r in csv("instances.csv") {
        let Ok(event) = r[0].parse::<u32>() else { continue };
        if !rigid.contains(&event) {
            continue;
        }
        let v: Vec<f64> = r[2].split_whitespace().filter_map(|x| x.parse().ok()).collect();
        if v.len() < 12 {
            continue;
        }
        let scale = (v[0] * v[0] + v[4] * v[4] + v[8] * v[8]).sqrt();
        inst.push(Inst { event, x: v[3], z: v[11], angle: v[2].atan2(v[0]).to_degrees(), scale });
    }
    println!(
        "{} rigid instances in {} draws; {} slots, {} sprawl pieces",
        inst.len(),
        rigid.len(),
        cities.slots.len(),
        cities.sprawl.len()
    );

    let near = |x: f64, z: f64| inst.iter().map(|i| ((i.x - x).hypot(i.z - z), i)).min_by(|a, b| a.0.total_cmp(&b.0));
    for s in cities.slots.iter().filter(|s| s.region.contains(&filter) && s.model.is_some()) {
        let Some((d, i)) = near(s.world.0, s.world.1) else { continue };
        if d > 1.0 {
            continue;
        }
        // Every instance standing on it (a settlement can be several models at one centre), with its draw's diffuse texture.
        if std::env::var_os("WCE_ALL_AT_SLOT").is_some() {
            for j in inst.iter().filter(|j| (j.x - s.world.0).hypot(j.z - s.world.1) < 0.01) {
                let tex = diffuse.get(&j.event).map_or("?", String::as_str);
                println!(
                    "    at it: event {} angle {:.1} scale {:.4} {}",
                    j.event,
                    j.angle,
                    j.scale,
                    tex.rsplit('/').next().unwrap_or(tex)
                );
            }
        }
        // The anchor cell's hex record: in a footprint, in a zone, water, road shape and turn, coast shape and turn.
        let hex = cities.hexes.get(&s.cell);
        println!(
            "slot {} #{} {} cell {:?} {} turn {} | event {} at {:.3} angle {:.1} scale {:.4} | hex {:?}",
            s.region,
            s.index,
            s.kind,
            s.cell,
            s.model.as_deref().unwrap_or("").rsplit('/').next().unwrap_or(""),
            s.turn,
            i.event,
            d,
            i.angle,
            i.scale,
            hex
        );
        // The cells around it (two cells each way) with a road or river edge, or a road mask: offset (columns, rows) and fields.
        let (c0, r0) = (i32::from(s.cell.0), i32::from(s.cell.1));
        for dr in -2..=2 {
            for dc in -2..=2 {
                let Some(h) = cities.hexes.get(&((c0 + dc) as u16, (r0 + dr) as u16)) else { continue };
                if h[7] != 0 || h[8] != 0 || h[9] != 0 || h[3] != 0 {
                    println!(
                        "    ({dc:+}, {dr:+}) foot {} road edges {:06b} river {:06b} road mask {:06b} road shape {} turn {}",
                        h[0], h[7], h[8], h[9], h[3], h[4]
                    );
                }
            }
        }
    }
    // Every slot in the startpos's order with the capture's turn where an instance stands exactly on it (`order.csv`: region, index,
    // kind, has a model, the capture's sixths or -1), for working out the game's draws.
    let mut order = String::from(
        "region,index,kind,model,turn
",
    );
    for s in &cities.slots {
        let t = near(s.world.0, s.world.1)
            .filter(|(d, _)| *d < 0.01 && s.model.is_some())
            .map_or(-1, |(_, i)| ((i.angle / 60.0).round() as i64).rem_euclid(6));
        order += &format!(
            "{},{},{},{},{t}
",
            s.region,
            s.index,
            s.kind,
            u8::from(s.model.is_some())
        );
    }
    std::fs::write(dir.join("order.csv"), order).expect("order.csv");

    for s in cities.sprawl.iter().filter(|s| s.region.contains(&filter) && s.model.is_some()) {
        let Some((d, i)) = near(s.world.0, s.world.1) else { continue };
        if d > 0.2 {
            continue;
        }
        println!(
            "sprawl {} cell {:?} {} turn {} ({} deg) | event {} at {:.3} angle {:.1} scale {:.4}",
            s.region,
            s.cell,
            s.piece,
            s.turn,
            s.turn * 60,
            i.event,
            d,
            i.angle,
            i.scale
        );
    }
}
