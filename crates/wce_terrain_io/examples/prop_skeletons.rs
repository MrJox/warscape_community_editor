//! Exploration: the campaign props' rigid models (from the game's packs) by their RMV2 header's skeleton name, against BOB's
//! `prop_marker.markers` `unknown_74`: which models get a 1.
//!
//!   WCE_GAME=<game folder> cargo run --release -p wce_terrain_io --example prop_skeletons -- <.terry> <prop_marker.markers>

use std::collections::BTreeMap;

use wce_formats::markers::{MarkersFile, Value};
use wce_terrain::terry::{EntityKind, TerryProject};
use wce_vfs::{Config, GameProfile, Vfs};

fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let game = std::env::var("WCE_GAME").expect("WCE_GAME");
    let vfs = Vfs::mount(&Config::new(game, GameProfile::from_env().clone())).expect("vfs");
    let p = TerryProject::parse(std::fs::read(&a[0]).expect("terry")).expect("xml");
    let mk = MarkersFile::parse(&std::fs::read(&a[1]).expect("markers")).expect("markers");
    // model -> (skeleton, records with 0, records with 1)
    let mut models: BTreeMap<String, (String, u32, u32)> = BTreeMap::new();
    let props = p.entities.iter().filter_map(|e| if let EntityKind::Prop(m) = p.kind(e) { Some(m) } else { None });
    for (i, m) in props.enumerate() {
        let f = mk.fields(i).expect("fields");
        let flag = f.iter().any(|x| x.0 == "unknown_74" && matches!(x.1, Value::U8(1)));
        let entry = models.entry(m.clone()).or_insert_with(|| {
            let skel = match vfs.read(&m.to_lowercase().replace('\\', "/")) {
                Ok(b) if b.len() >= 140 => {
                    let s = &b[12..140];
                    format!("v{} {:?}", u32::from_le_bytes([b[4], b[5], b[6], b[7]]), String::from_utf8_lossy(&s[..s.iter().position(|&c| c == 0).unwrap_or(s.len())]))
                }
                Ok(_) => "short".to_owned(),
                Err(e) => format!("missing ({e})"),
            };

            (skel, 0, 0)
        });

        if flag { entry.2 += 1 } else { entry.1 += 1 }
    }

    for (m, (s, zero, one)) in &models {
        println!("{zero:5} {one:5}  {s:30} {m}");
    }
}
