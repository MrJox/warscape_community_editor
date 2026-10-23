//! Stage C (`attila::stage_c`) on the kit's raw tiles against BOB's fresh output: every tree list, procedural BMD (and its XML twin) and
//! grass list, written by `attila.tile_binary`, compared byte for byte with the oracle kit's file of the same name (case aside).
//!
//!   cargo run --release -p wce_tile_io --example stage_c_check -- <game folder> <oracle working_data> [filter]
//!
//! `WCE_STAGE_C_DUMP=DIR` writes our files into DIR/<tile>/; `WCE_VEG_PARAMS=DIR` reads the tree parameters from DIR.

use std::path::PathBuf;
use std::sync::Arc;

use rayon::prelude::*;
use wce_tile_io::attila::{TedProject, TileBinary, stage_b::PackIndex, stage_c};
use wce_tile_io::{TileFiles, TileFormat};
use wce_tile_model::Tile;

/// BOB's file with the fields it leaves uninitialised taken from ours: a procedural BMD's type-0 props' decal parameters
/// (`decal_parallax`, `decal_scale`: stack memory in BOB's files), written again; the XML twin with the same attributes of `type='0'`
/// markers replaced by ours, line by line.
fn mask_uninitialised(name: &str, theirs: &[u8], ours: &[u8]) -> Option<Vec<u8>> {
    use rpfm_lib::files::bmd::Bmd;
    use rpfm_lib::files::{Decodeable, Encodeable};
    use std::io::Cursor;
    if name.ends_with("_procedural_bmd_data.bin") {
        let mut a = Bmd::decode(&mut Cursor::new(theirs.to_vec()), &None).ok()?;
        let b = Bmd::decode(&mut Cursor::new(ours.to_vec()), &None).ok()?;
        if a.prop_list().markers().len() != b.prop_list().markers().len() {
            return None;
        }

        for (x, y) in a.prop_list_mut().markers_mut().iter_mut().zip(b.prop_list().markers()) {
            if *x.prop_type() == 0 {
                x.set_decal_parallax(*y.decal_parallax());
                x.set_decal_scale(*y.decal_scale());
            }
        }

        let mut out = vec![];
        a.encode(&mut out, &None).ok()?;
        return Some(out);
    }

    if name.ends_with("_procedural_bmd_data.xml") {
        let (a, b) = (String::from_utf8_lossy(theirs), String::from_utf8_lossy(ours));
        let (la, lb) = (a.split('\n').collect::<Vec<_>>(), b.split('\n').collect::<Vec<_>>());
        if la.len() != lb.len() {
            return None;
        }

        let fields = ["decal_params_parallax", "decal_params_scalex", "decal_params_scaley", "decal_params_scalez"];
        let attr = |line: &str, k: &str| line.find(&format!(" {k}='")).map(|i| {
            let start = i + k.len() + 3;
            (start, start + line[start..].find('\'').unwrap_or(0))
        });
        let out = la
            .iter()
            .zip(&lb)
            .map(|(x, y)| {
                if !x.contains("type='0'") {
                    return (*x).to_owned();
                }

                let mut x = (*x).to_owned();
                for k in fields {
                    if let (Some((s0, e0)), Some((s1, e1))) = (attr(&x, k), attr(y, k)) {
                        x.replace_range(s0..e0, &y[s1..e1]);
                    }
                }

                x
            })
            .collect::<Vec<_>>()
            .join("\n");
        return Some(out.into_bytes());
    }

    None
}

fn main() -> Result<(), String> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let game = PathBuf::from(&args[0]);
    let oracle = PathBuf::from(&args[1]);
    let filter = args.get(2).cloned().unwrap_or_default();
    let raw_data = game.join("assembly_kit/raw_data");
    let (mut params, grass) = stage_c::kit_params(&raw_data)?;
    // WCE_VEG_PARAMS=DIR: the tree parameters from another folder (the kit's plus a test file, as BOB had them for an oracle).
    if let Some(dir) = std::env::var_os("WCE_VEG_PARAMS") {
        params = wce_tile::vegetation::GenerationParams::load(std::path::Path::new(&dir))?;
    }
    let settings = raw_data.join("terrain/tiles/battle/_tile_database/_settings.xml");
    let packs = PackIndex::new(&game.join("data"));
    let exists = |p: &str| packs.exists(p);
    let ctx = stage_c::StageC { params: &params, grass: grass.as_ref(), settings: Some(&settings), model_exists: &exists };
    let dump = std::env::var_os("WCE_STAGE_C_DUMP").map(PathBuf::from);

    // The oracle's tiles: folders under terrain/tiles/battle with a grass list.
    let base = oracle.join("terrain/tiles/battle");
    let mut tiles = vec![];
    let mut stack = vec![base.clone()];
    while let Some(d) = stack.pop() {
        for e in std::fs::read_dir(&d).into_iter().flatten().flatten() {
            let p = e.path();
            if p.is_dir() {
                stack.push(p);
            } else if p.file_name().is_some_and(|n| n == "default.grass_list.bin") {
                tiles.push(d.strip_prefix(&base).map_err(|e| e.to_string())?.to_string_lossy().into_owned());
            }
        }
    }

    tiles.retain(|t| t.contains(&filter));
    tiles.sort();
    let lines: Vec<String> = tiles
        .par_iter()
        .map(|t| -> String {
            let run = || -> Result<Vec<String>, String> {
                let files = TileFiles::from_kit_dir(&raw_data.join("terrain/tiles/battle").join(t)).map_err(|e| e.to_string())?;
                let (raw, _) = TedProject.read(&files).map_err(|e| e.to_string())?;
                let (out, notes) = stage_c::stage_c_documents(&raw, &ctx, stage_c::Outputs::ALL)?;
                let mut tile = Tile { source_format: Some(wce_tile_io::attila::tile_binary::ID), ..Default::default() };
                for name in &out.xml {
                    tile.source.insert(name.clone(), Arc::new(wce_tile_io::attila::compile::bmd_xml_style()));
                }
                tile.documents = out.documents;
                let (written, _) = TileBinary.write(&tile).map_err(|e| e.to_string())?;
                let theirs: std::collections::HashMap<String, PathBuf> = std::fs::read_dir(base.join(t))
                    .map_err(|e| e.to_string())?
                    .flatten()
                    .map(|e| (e.file_name().to_string_lossy().to_lowercase(), e.path()))
                    .collect();
                let mut lines = notes.notes;
                let (mut same, mut masked, mut total) = (0, 0, 0);
                for (name, blob) in &written.folder {
                    let ours = blob.bytes().map_err(|e| e.to_string())?;
                    if let Some(d) = &dump {
                        let dir = d.join(t);
                        std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
                        std::fs::write(dir.join(name), &ours).map_err(|e| e.to_string())?;
                    }

                    total += 1;
                    match theirs.get(&name.to_lowercase()).map(std::fs::read) {
                        Some(Ok(b)) if b == ours[..] => same += 1,
                        Some(Ok(b)) if mask_uninitialised(name, &b, &ours).is_some_and(|b| b == ours[..]) => masked += 1,
                        Some(Ok(b)) => {
                            let at = b.iter().zip(ours.iter()).position(|(x, y)| x != y).unwrap_or(b.len().min(ours.len()));
                            lines.push(format!("  {name}: differs at {at} (theirs {} bytes, ours {})", b.len(), ours.len()));
                        }
                        _ => lines.push(format!("  {name}: not in the oracle")),
                    }
                }

                lines.insert(0, format!("{t}\t{same} / {total} byte for byte, {masked} more with BOB's uninitialised prop fields masked"));
                Ok(lines)
            };

            match run() {
                Ok(l) => l.join("\n"),
                Err(e) => format!("{t}\terror: {e}"),
            }
        })
        .collect();

    for l in lines {
        println!("{l}");
    }

    Ok(())
}
