//! The generated props and VFX of Assembly Kit tiles (the props and particle emitters of `<category>_procedural_bmd_data.bin`) against the
//! game's packed files, or with `--oracle-dir <kit>/working_data` against BOB's fresh output: per tile, category and kind, their entries,
//! ours, how many are the same (key, every float of the transform, and the marker's values), and whether the order is the same (or the
//! same within each key: the order of the keys themselves is BOB's marker table's, not reproduced).
//!
//!   cargo run --release -p wce_tile --example bmd_check -- <game> [filter] [--diff] [--oracle-dir <working_data dir>] [--params-dir <dir>]
//!
//! `--params-dir` reads the vegetation parameters from another folder than the kit's (a scratch kit's, with test parameter files).

use std::collections::HashMap;
use std::io::Cursor;
use std::path::PathBuf;

use rayon::prelude::*;
use rpfm_lib::files::Decodeable;
use rpfm_lib::files::bmd::Bmd;
use rpfm_lib::files::bmd::common::{Matrix, Transform4x4};
use wce_tile::tile_project::TileProject;
use wce_tile::vegetation::{self, GenerationParams};
use wce_vfs::{Config, GameProfile, Vfs};

const SUFFIX: &str = "_procedural_bmd_data.bin";

fn floats(t: &Transform4x4) -> [u32; 16] {
    [
        t.m00(), t.m01(), t.m02(), t.m03(), t.m10(), t.m11(), t.m12(), t.m13(), t.m20(), t.m21(), t.m22(), t.m23(), t.m30(), t.m31(), t.m32(), t.m33(),
    ]
    .map(|v| (v + 0.0).to_bits())
}

/// Counts and order of two lists of comparable entries.
fn compare(kind: &str, theirs: &[String], mine: &[String], diff: bool) -> Vec<String> {
    let mut left: HashMap<&String, usize> = HashMap::new();
    for t in theirs {
        *left.entry(t).or_default() += 1;
    }

    let mut same = 0;
    for m in mine {
        if let Some(n) = left.get_mut(m).filter(|n| **n > 0) {
            *n -= 1;
            same += 1;
        }
    }

    // Each key's entries in the same order, whatever the order of the keys (BOB's marker table order is not reproduced).
    let of_key = |l: &[String], k: &str| l.iter().filter(|e| e.split(' ').next() == Some(k)).cloned().collect::<Vec<_>>();
    let per_key_same = theirs.iter().all(|t| {
        let k = t.split(' ').next().unwrap_or_default();
        of_key(theirs, k) == of_key(mine, k)
    });
    let order = if theirs == mine { "same" } else if per_key_same && theirs.len() == mine.len() { "same per key" } else { "differs" };
    let per_key = |l: &[String]| {
        let mut m: Vec<(String, usize)> = vec![];
        for e in l {
            let k = e.split(' ').next().unwrap_or_default().to_owned();
            match m.iter_mut().find(|(kk, _)| *kk == k) {
                Some(x) => x.1 += 1,
                None => m.push((k, 1)),
            }
        }
        m
    };
    let mut out = vec![format!("{kind}\ttheirs {}\tmine {}\tsame {same}\torder {order}", theirs.len(), mine.len())];
    if diff && theirs != mine {
        out.push(format!("  keys theirs {:?}\n  keys mine   {:?}", per_key(theirs), per_key(mine)));
        let mut pool: Vec<&String> = mine.iter().collect();
        let mut only_theirs = vec![];
        for t in theirs {
            match pool.iter().position(|m| *m == t) {
                Some(i) => {
                    pool.swap_remove(i);
                }
                None => only_theirs.push(t),
            }
        }
        for t in only_theirs.iter().take(3) {
            out.push(format!("  only theirs {t}"));
        }
        for m in pool.iter().take(3) {
            out.push(format!("  only mine   {m}"));
        }
        for (i, (t, m)) in theirs.iter().zip(mine).enumerate().filter(|(_, (t, m))| t != m).take(4) {
            out.push(format!("  #{i} theirs {t}\n  #{i} mine   {m}"));
        }
    }

    out
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let game = PathBuf::from(&args[0]);
    let filter = args.get(1).filter(|a| !a.starts_with("--")).cloned().unwrap_or_default();
    let diff = args.iter().any(|a| a == "--diff");
    let oracle_dir: Option<PathBuf> = args.iter().position(|a| a == "--oracle-dir").and_then(|i| args.get(i + 1)).map(PathBuf::from);
    let params_dir = args
        .iter()
        .position(|a| a == "--params-dir")
        .and_then(|i| args.get(i + 1))
        .map(PathBuf::from)
        .unwrap_or_else(|| game.join("assembly_kit/working_data/BattleTerrain/vegetation"));
    let raw = game.join("assembly_kit").join("raw_data");
    let vfs = Vfs::mount(&Config::new(game.clone(), GameProfile::from_env().clone())).expect("vfs");
    let params = GenerationParams::load(&params_dir).expect("params");
    let mut tiles: HashMap<String, Vec<String>> = HashMap::new();
    for l in vfs.list("terrain/tiles/battle/", SUFFIX) {
        let (folder, file) = l.rsplit_once('/').expect("a file");
        tiles.entry(folder.to_owned()).or_default().push(file.trim_end_matches(SUFFIX).to_owned());
    }

    let mut folders: Vec<(String, Vec<String>)> = tiles.into_iter().filter(|(f, _)| f.contains(&filter) && raw.join(f).join("tile.ted").is_file()).collect();
    folders.sort();
    let results: Vec<String> = folders
        .par_iter()
        .map(|(folder, names)| {
            let Ok(project) = TileProject::open(&raw.join(folder)) else { return format!("{folder}\topen failed") };
            let inputs = match vegetation::tile_inputs(&project) {
                Ok(i) => i,
                Err(e) => return format!("{folder}\tinputs: {e}"),
            };

            let Some(seeds) = vegetation::seeds(&project.scene) else { return format!("{folder}\tno seeds") };
            let mut names = names.clone();
            names.sort();
            let mut lines = vec![];
            for name in names {
                let Some(category) = params.category(&name) else { continue };
                let bytes = match oracle_dir.as_ref() {
                    Some(dir) => std::fs::read_dir(dir.join(folder)).ok().and_then(|rd| {
                        rd.flatten().find(|e| e.file_name().to_string_lossy().to_lowercase() == format!("{name}{SUFFIX}")).and_then(|e| std::fs::read(e.path()).ok())
                    }),
                    None => vfs.read(&format!("{folder}/{name}{SUFFIX}")).ok(),
                };
                let Some(bytes) = bytes else { continue };
                let Ok(bmd) = Bmd::decode(&mut Cursor::new(bytes), &None) else {
                    lines.push(format!("{folder}\t{name}\tdoes not decode"));
                    continue;
                };

                // VFX: the particle emitters' markers.
                let theirs: Vec<String> = bmd
                    .particle_emitter_list()
                    .markers()
                    .iter()
                    .map(|m| {
                        format!("{} {} {:?} rate {} scale {} outfield {}", m.key(), m.instance_name(), floats(m.transform()), m.emission_rate(), m.scale(), m.use_in_outfield())
                    })
                    .collect();
                let mine: Vec<String> = vegetation::generated_vfx(&inputs, &params, category, seeds)
                    .iter()
                    .map(|v| {
                        let t = [1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, v.x, v.y, v.z, 1.0f32].map(|f| (f + 0.0).to_bits());
                        format!("{} {} {:?} rate 1 scale {} outfield {}", v.key, v.instance_name, t, v.scale, v.use_in_outfield)
                    })
                    .collect();
                if !(theirs.is_empty() && mine.is_empty()) {
                    lines.extend(compare(&format!("{folder}\t{name}\tvfx"), &theirs, &mine, diff));
                }

                // Props (and decals, type 1): the prop list's markers.
                let theirs: Vec<String> = bmd
                    .prop_list()
                    .markers()
                    .iter()
                    .map(|m| {
                        // A prop's (type 0) decal parameters are uninitialised memory in BOB's files: left out.
                        let s = m.decal_scale();
                        let decal = if *m.prop_type() == 0 {
                            String::from("-")
                        } else {
                            format!("{} {:?}", m.decal_parallax().to_bits(), [*s.x(), *s.y(), *s.z()].map(f32::to_bits))
                        };
                        format!("{} {:?} type {} outfield {} decal {decal} tiled {}", m.key(), floats(m.transform()), m.prop_type(), m.allow_in_outfield(), *m.decal_tiled() != 0)
                    })
                    .collect();
                let mine: Vec<String> = vegetation::generated_props(&inputs, &params, category, seeds)
                    .iter()
                    .map(|p| {
                        let decal = if p.prop_type == 0 { String::from("-") } else { format!("{} {:?}", p.decal_parallax.to_bits(), p.decal_scale.map(f32::to_bits)) };
                        format!("{} {:?} type {} outfield {} decal {decal} tiled {}", p.key, p.transform.map(|f| (f + 0.0).to_bits()), p.prop_type, p.allow_in_outfield, p.decal_tiled)
                    })
                    .collect();
                if !(theirs.is_empty() && mine.is_empty()) {
                    lines.extend(compare(&format!("{folder}\t{name}\tprops"), &theirs, &mine, diff));
                }
            }

            lines.join("\n")
        })
        .filter(|l| !l.is_empty())
        .collect();

    for r in results {
        println!("{r}");
    }
}
