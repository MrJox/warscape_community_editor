//! The joints of a project's procedural walls with what they touch, once lowered (`export::lower_walls`): every end of a generated
//! piece must meet another piece's end exactly, in place, height and direction, as the game's own walls do. In game, joints off by a
//! fraction of a unit or a degree (a wall leaving a gate at an angle, a bastion on a curve) let ladders and towers dock while units could
//! not cross onto the wall (docs/plans/procedural_walls.md, "Docking").
//!
//!   cargo run --release -p wce_walls --example wall_joint_check -- <game folder> <mod packs a.pack;b.pack> <rpfm schema_att.ron>
//!       <project folder>
//!
//! The ends: a game piece's frame ends (its logic file's box at z = 0, `scene::anchors`), with its axis; a generated piece's bridging
//! columns (its destroyed level attaches one at each end, facing in). An end with no other end within [`OPEN`] is open (a free end, a
//! cap's outer end); one nearer must be within [`EXACT`] and [`ANGLE`].

use std::io::Cursor;

use rpfm_lib::files::{Decodeable, cs2_parsed::Cs2Parsed};
use wce_foundation::math::Double3;
use wce_tile::tile_project::TileProject;
use wce_tile::world_transform::world_transform;
use wce_vfs::{Config, GameProfile, Vfs};

/// Ends further apart than this are not a joint (scene units).
const OPEN: f64 = 2.0;

/// A joint's ends must be this close (scene units, horizontally and in height) and this parallel (degrees).
const EXACT: f64 = 1e-3;
const ANGLE: f64 = 0.01;

/// The prefix the lowered pieces get here.
const PREFIX: &str = "wce_check";

fn main() {
    if let Err(e) = run() {
        eprintln!("wall_joint_check: {e}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), String> {
    let a = std::env::args().skip(1).collect::<Vec<_>>();
    let [game, mods, schema, project] = a.as_slice() else {
        return Err("usage: wall_joint_check <game folder> <mod packs a.pack;b.pack> <rpfm schema_att.ron> <project folder>".to_owned());
    };

    let mut c = Config::new(std::path::PathBuf::from(game), GameProfile::ATTILA);
    c.mod_packs = Some(mods.split(';').filter(|s| !s.is_empty()).map(str::to_owned).collect());
    let vfs = Vfs::mount(&c).map_err(|e| e.to_string())?;
    let schema = rpfm_lib::schema::Schema::load(std::path::Path::new(schema), None).map_err(|e| e.to_string())?;
    let project = TileProject::open(std::path::Path::new(project)).map_err(|e| e.to_string())?;
    let unit = project.unit_scale;
    let ground = |x: f64, z: f64| project.height_at(x / unit, z / unit);
    let mut scene = project.scene.clone();
    let lowered = wce_walls::export::lower_walls(&mut scene, &ground, &vfs, &schema, PREFIX)?;
    let generated = lowered.files.into_iter().filter(|(n, _)| n.ends_with("_tech.cs2.parsed")).collect::<std::collections::HashMap<_, _>>();

    // The generated pieces' columns.
    let mut ends: Vec<(usize, String, Double3, [f64; 2])> = vec![];
    for e in scene.find_all("BUILDING_INSTANCE") {
        let key = scene.doc().get(e, "key").unwrap_or_default();
        if !key.starts_with(PREFIX) {
            continue;
        }

        let path = format!("rigidmodels/buildings/{key}/{key}_tech.cs2.parsed");
        let bytes = generated.get(&path).ok_or_else(|| format!("{key}: no logic file"))?;
        let j = serde_json::to_value(Cs2Parsed::decode(&mut Cursor::new(bytes), &None).map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
        let w = world_transform(&scene, e);
        for d in j["pieces"].as_array().into_iter().flatten().flat_map(|p| p["destructs"].as_array().into_iter().flatten()) {
            for f in d["file_refs"].as_array().into_iter().flatten().filter(|f| f["name"].as_str().unwrap_or_default().contains("bridgingcolumn")) {
                let t = &f["transform"];
                let g = |k: &str| t[k].as_f64().unwrap_or(0.0);
                let (p, x) = (w.position + w.rotation * Double3::new(g("m30"), g("m31"), g("m32")), w.rotation * Double3::new(g("m00"), g("m01"), g("m02")));
                ends.push((e, key.clone(), p, [x.x, x.z]));
            }
        }
    }

    let generated_ends = ends.len();

    // The game's pieces' frame ends.
    let extent = |k: &str| {
        let b = vfs.read(&format!("rigidmodels/buildings/{k}/{k}_tech.cs2.parsed")).ok()?;
        wce_walls::logic::extent_x(&b).ok().map(|(a, b)| (f64::from(a), f64::from(b)))
    };
    ends.extend(wce_walls::scene::anchors(&scene, &extent).into_iter().map(|a| (a.elem, a.key.clone(), Double3::new(a.at[0], a.at[1], a.at[2]), a.axis)));

    // Each generated end's nearest end of another piece.
    let (mut joints, mut open, mut bad) = (0, 0, 0);
    for (i, (e, k, p, ax)) in ends.iter().enumerate().take(generated_ends) {
        let near = ends.iter().enumerate().filter(|(j, o)| *j != i && o.0 != *e).min_by(|a, b| {
            let d = |q: &Double3| (q.x - p.x).hypot(q.z - p.z);
            d(&a.1.2).total_cmp(&d(&b.1.2))
        });
        let Some((_, (_, k2, q, bx))) = near else { continue };
        let d = (q.x - p.x).hypot(q.z - p.z);
        if d > OPEN {
            open += 1;
            continue;
        }

        let dot = (ax[0] * bx[0] + ax[1] * bx[1]) / (ax[0].hypot(ax[1]) * bx[0].hypot(bx[1]));
        let angle = dot.abs().clamp(0.0, 1.0).acos().to_degrees();
        let ok = d <= EXACT && (q.y - p.y).abs() <= EXACT && angle <= ANGLE;
        joints += 1;
        bad += usize::from(!ok);
        if !ok {
            println!("{k} -> {k2} at ({:.2}, {:.2}, {:.2}): {d:.3} apart, {:+.3} in height, {angle:.2} degrees  <- PROBLEM", p.x, p.y, p.z, q.y - p.y);
        }
    }

    println!("{generated_ends} ends of generated pieces: {joints} joints, {open} open, {bad} off");
    if bad > 0 {
        return Err(format!("{bad} joint(s) off"));
    }

    println!("all passed");
    Ok(())
}
