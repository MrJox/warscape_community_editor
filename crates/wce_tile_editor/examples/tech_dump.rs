//! What a building's logic file (`<key>_tech.cs2.parsed`) holds, per destruction level: the platforms (their outline's bounds and heights,
//! flags), the collision outlines, the pipes (type, ends) and the formation lines; for reviewing what piece-to-piece snapping could use.
//!
//!   cargo run --release -p wce_tile_editor --example tech_dump -- <game folder> <key>...

use std::io::Cursor;
use std::path::PathBuf;

use rpfm_lib::files::cs2_parsed::Cs2Parsed;
use rpfm_lib::files::{Decodeable, DecodeableExtraData};
use serde_json::Value;
use wce_vfs::{Config, GameProfile, Vfs};

fn num(v: &Value) -> f64 {
    v.as_f64().unwrap_or(0.0)
}

fn list(v: &Value) -> &[Value] {
    v.as_array().map_or(&[], Vec::as_slice)
}

/// The bounds (x, y, z) of a list of points with x, y, z.
fn bounds(pts: &[Value]) -> String {
    let (mut lo, mut hi) = ([f64::MAX; 3], [f64::MIN; 3]);
    for p in pts {
        for (k, c) in ["x", "y", "z"].iter().enumerate() {
            lo[k] = lo[k].min(num(&p[c]));
            hi[k] = hi[k].max(num(&p[c]));
        }
    }

    format!("x {:.2}..{:.2} y {:.2}..{:.2} z {:.2}..{:.2}", lo[0], hi[0], lo[1], hi[1], lo[2], hi[2])
}

fn main() -> Result<(), String> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let Some(game) = args.first() else { return Err("usage: tech_dump <game folder> <key>...".to_owned()) };
    let vfs = Vfs::mount(&Config::new(PathBuf::from(game), GameProfile::from_env().clone())).map_err(|e| e.to_string())?;
    for key in &args[1..] {
        let path = format!("rigidmodels/buildings/{key}/{key}_tech.cs2.parsed");
        let Ok(bytes) = vfs.read(&path) else {
            println!("{key}: no logic file");
            continue;
        };

        let parsed = Cs2Parsed::decode(&mut Cursor::new(bytes), &Some(DecodeableExtraData::default())).map_err(|e| e.to_string())?;
        let v = serde_json::to_value(&parsed).map_err(|e| e.to_string())?;
        println!("== {key}");
        for piece in list(&v["pieces"]) {
            println!(" piece {}", piece["name"].as_str().unwrap_or_default());
            for d in list(&piece["destructs"]) {
                println!("  level {}", d["index"]);
                for p in list(&d["platforms"]) {
                    let pts = list(&p["vertices"]["outline"]);
                    println!("   platform {} points, {} flags {} {} {}", pts.len(), bounds(pts), p["flag_1"], p["flag_2"], p["flag_3"]);
                    if std::env::var("TECH_DUMP_POINTS").is_ok() {
                        println!("     {}", pts.iter().map(|q| format!("({:.2} {:.2} {:.2})", num(&q["x"]), num(&q["y"]), num(&q["z"]))).collect::<Vec<_>>().join(" "));
                    }
                }

                for o in list(&d["collision_outlines"]) {
                    let pts = list(&o["vertices"]["outline"]);
                    println!("   outline {} points, {}", pts.len(), bounds(pts));
                }

                for p in list(&d["pipes"]) {
                    let pts = list(&p["line"]["outline"]);
                    println!("   pipe {} {} points, {}", p["line_type"].as_str().unwrap_or_default(), pts.len(), bounds(pts));
                }

                for l in list(&d["ef_lines"]) {
                    println!("   formation {} from ({:.2} {:.2} {:.2}) to ({:.2} {:.2} {:.2})", l["action"].as_str().unwrap_or_default(),
                        num(&l["start"]["x"]), num(&l["start"]["y"]), num(&l["start"]["z"]), num(&l["end"]["x"]), num(&l["end"]["y"]), num(&l["end"]["z"]));
                }
            }
        }
    }

    Ok(())
}
