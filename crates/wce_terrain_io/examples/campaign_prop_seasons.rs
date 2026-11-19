//! A campaign map's props by model and season mask (`terrain/campaigns/<map>/prop_marker.markers` in the packs: `seasons`, spring 1,
//! summer 2, autumn 4, winter 8): how many of each model show in which seasons.
//!
//!   cargo run --release -p wce_terrain_io --example campaign_prop_seasons -- [map] [model filter]
//!
//! `WCE_ATTILA_DIR` the game (default this machine's Steam folder); the profile from `WCE_GAME`.

use std::collections::BTreeMap;
use std::path::PathBuf;

use wce_formats::markers::{KeyPart, MarkersFile, Value};
use wce_vfs::{Config, GameProfile, Vfs};

fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let map = a.first().map_or("main_attila_map", String::as_str);
    let filter = a.get(1).map_or(String::new(), |f| f.to_lowercase());
    let game_dir = std::env::var_os("WCE_ATTILA_DIR")
        .map_or_else(|| PathBuf::from("D:/Juegos/Steam/steamapps/common/Total War Attila"), PathBuf::from);
    let mut cfg = Config::new(&game_dir, GameProfile::from_env().clone());
    cfg.mod_packs = Some(vec![]);
    let vfs = Vfs::mount(&cfg).expect("vfs");
    let bytes = vfs.read(&format!("terrain/campaigns/{map}/prop_marker.markers")).expect("prop_marker.markers");
    let f = MarkersFile::parse(&bytes).expect("markers");
    let mut model_of = vec![String::new(); f.records.len()];
    for g in f.groups.iter().flatten() {
        let key = g
            .key
            .iter()
            .find_map(|k| match k {
                KeyPart::Wide(w) => Some(String::from_utf16_lossy(w)),
                KeyPart::Narrow(n) => Some(String::from_utf8_lossy(n).into_owned()),
                KeyPart::U32(_) => None,
            })
            .unwrap_or_default();
        for &i in &g.indices {
            if let Some(m) = model_of.get_mut(i as usize) {
                m.clone_from(&key);
            }
        }
    }

    let mut counts: BTreeMap<String, BTreeMap<String, usize>> = BTreeMap::new();
    for (i, model) in model_of.iter().enumerate() {
        if !model.to_lowercase().contains(&filter) {
            continue;
        }
        let Ok(fields) = f.fields(i) else { continue };
        let seasons = fields.iter().find(|(n, _)| *n == "seasons").map_or("-".to_owned(), |(_, v)| match v {
            Value::U32(s) => format!("{s:04b}"),
            Value::U8(s) => format!("{s:04b}"),
            other => format!("{other:?}"),
        });
        *counts.entry(model.clone()).or_default().entry(seasons).or_default() += 1;
    }
    for (model, by) in counts {
        println!("{model}: {by:?}");
    }
}
