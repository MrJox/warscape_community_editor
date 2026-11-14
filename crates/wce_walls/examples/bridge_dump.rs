//! What units walk on in the kit's bridges: each piece's platforms by destruction level, and its pipes and docking.
//!
//!   cargo run --release -p wce_walls --example bridge_dump -- <game folder> <key>...

use std::io::Cursor;
use std::path::PathBuf;

use rpfm_lib::files::{Decodeable, cs2_parsed::Cs2Parsed};
use serde_json::Value;
use wce_vfs::{Config, GameProfile, Vfs};
use wce_walls::building::BuildingFiles;

fn main() {
    let a = std::env::args().skip(1).collect::<Vec<_>>();
    let vfs = Vfs::mount(&Config::new(PathBuf::from(&a[0]), GameProfile::ATTILA)).expect("the game's packs");
    for key in &a[1..] {
        let Ok(b) = BuildingFiles::read(&vfs, key) else {
            println!("{key}: no folder");
            continue;
        };

        let names = b.files.keys().map(|k| k.rsplit('/').next().unwrap_or(k).to_owned()).collect::<Vec<_>>();
        println!("== {key}: {names:?}");
        let Some((_, bytes)) = b.files.iter().find(|(k, _)| k.ends_with("_tech.cs2.parsed")) else { continue };
        let parsed = Cs2Parsed::decode(&mut Cursor::new(bytes), &None).expect("a logic file");
        let v = serde_json::to_value(&parsed).expect("json");
        println!("top: {:?}", v.as_object().map(|o| o.keys().cloned().collect::<Vec<_>>()));
        for piece in v["pieces"].as_array().cloned().unwrap_or_default() {
            println!("piece {}", piece["name"]);
            for d in piece["destructs"].as_array().cloned().unwrap_or_default() {
                let platforms = d["platforms"].as_array().cloned().unwrap_or_default();
                println!(
                    " level {} {} platforms {} pipes {} docking {}",
                    d["index"],
                    d["name"],
                    platforms.len(),
                    d["pipes"].as_array().map_or(0, Vec::len),
                    d["docking_lines"].as_array().map_or(0, Vec::len)
                );
                if d["index"].as_u64() != Some(0) {
                    continue;
                }

                for p in platforms {
                    let pts = p["vertices"]["outline"].as_array().cloned().unwrap_or_default();
                    let text = pts
                        .iter()
                        .map(|q: &Value| {
                            format!(
                                "({:.2},{:.2},{:.2})",
                                q["x"].as_f64().unwrap_or(0.0),
                                q["y"].as_f64().unwrap_or(0.0),
                                q["z"].as_f64().unwrap_or(0.0)
                            )
                        })
                        .collect::<Vec<_>>();
                    let flags = p.as_object().map(|o| {
                        o.iter().filter(|(k, _)| *k != "vertices" && *k != "normal").map(|(k, x)| format!("{k}={x}")).collect::<Vec<_>>()
                    });
                    println!("  {} {flags:?}", text.join(" "));
                }
            }
        }
    }
}
