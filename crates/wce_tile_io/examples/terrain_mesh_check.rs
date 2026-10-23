//! Stage B's terrain files against a fresh BOB run: `wce_tile_io::attila::terrain_mesh::terrain_files` on a raw tile folder (kit
//! layout) against BOB's working folder: `outfield_mesh.rigid_model_v2`, `mesh.rigid_model_v2` (the shader name's uninitialised tail
//! masked) and `hf_height_map.data` (its samples compared when it differs); `final_heights.dds` and `final_alpha.dds` against BOB's
//! raw folder (the working one's path with `raw_data`).
//!
//!   cargo run --release -p wce_tile_io --example terrain_mesh_check -- <raw tile folder> <BOB's working tile folder> <kit raw_data>

use std::path::PathBuf;

use wce_formats::chmf::ChmfFile;
use wce_tile_io::attila::TedProject;
use wce_tile_io::attila::terrain_mesh::{building_stamps, decimation_angle, terrain_files};
use wce_tile_io::{TileFiles, TileFormat};

fn main() {
    let args = std::env::args().skip(1).map(PathBuf::from).collect::<Vec<_>>();
    let (raw, work, kit) = (&args[0], &args[1], &args[2]);
    let files = TileFiles::from_kit_dir(raw).expect("the raw tile");
    let (project, _) = TedProject.read(&files).expect("the project");
    let (stamps, missing) = building_stamps(&project, kit);
    println!("{} building stamps{}", stamps.len(), if missing.is_empty() { String::new() } else { format!("; no mesh file for {missing:?}") });
    let t0 = std::time::Instant::now();
    // BOB's VFS for the water planes' models: the game's packs (the kit's `../../data`), searched only when a model is asked for.
    let data = kit.join("../../data");
    let models = |key: &str| -> Option<Vec<u8>> {
        use rpfm_lib::files::pack::Pack;
        let key = key.replace('\\', "/").to_lowercase();
        let attila = rpfm_lib::games::supported_games::SupportedGames::default().game("attila")?.clone();
        let mut packs = std::fs::read_dir(&data).ok()?.flatten().map(|e| e.path()).filter(|p| p.extension().is_some_and(|x| x == "pack")).collect::<Vec<_>>();
        packs.sort();
        for p in packs {
            let Ok(mut pack) = Pack::read_and_merge(std::slice::from_ref(&p), &attila, true, false, false) else { continue };
            let Some(name) = pack.files().keys().find(|n| n.to_lowercase() == key).cloned() else { continue };
            if let Some(Ok(Some(bytes))) = pack.files_mut().get_mut(&name).map(|f| f.encode(&None, false, false, true)) {
                return Some(bytes);
            }
        }
        None
    };
    let out = terrain_files(&project, kit, decimation_angle(kit), &stamps, &models).expect("stage B");
    println!("built in {:?}", t0.elapsed());
    for (name, ours) in out {
        // BOB writes the final heights and alpha into the raw folder: look there too.
        let raw_oracle = PathBuf::from(work.to_string_lossy().replace("working_data", "raw_data")).join(&name);
        let Ok(theirs) = std::fs::read(work.join(&name)).or_else(|_| std::fs::read(raw_oracle)) else {
            println!("{name}: BOB wrote none");
            continue;
        };
        if let Ok(dir) = std::env::var("WCE_DUMP") {
            std::fs::write(PathBuf::from(dir).join(&name), &ours).expect("dump");
        }
        if name.ends_with(".data") {
            let (o, t) = (ChmfFile::parse(&ours).expect("ours"), ChmfFile::parse(&theirs).expect("BOB's"));
            let ((_, _, os), (_, _, ts)) = (o.region_samples(0).expect("ours"), t.region_samples(0).expect("BOB's"));
            let diff = os.iter().zip(&ts).filter(|(a, b)| a != b).count();
            println!("{name}: {}; {diff} of {} samples differ, min {} / {}, max {} / {}", if ours == theirs { "identical" } else { "differs" }, os.len(), o.min, t.min, o.max, t.max);
            continue;
        }

        if !name.ends_with(".rigid_model_v2") {
            let diff = ours.iter().zip(&theirs).position(|(a, b)| a != b);
            println!("{name}: {} ({} bytes ours, {} BOB's{})", if ours == theirs { "identical" } else { "differs" }, ours.len(), theirs.len(),
                diff.map(|i| format!(", first difference at {i}")).unwrap_or_default());
            continue;
        }

        // BOB leaves the tail of each block's shader name (after `rigid_default\0`, block + 0x3e .. + 0x50) uninitialised.
        let masked = |b: &[u8]| {
            let mut out = b.to_vec();
            let u32_at = |o: usize| b.get(o..o + 4).map_or(0, |s| u32::from_le_bytes(s.try_into().unwrap()) as usize);
            let mut block = 0xa0;
            for _ in 0..u32_at(0x8c) {
                if block + 0x50 > out.len() {
                    break;
                }

                out[block + 0x3e..block + 0x50].fill(0);
                block += u32_at(block + 4).max(1);
            }

            out
        };
        let (om, tm) = (masked(&ours), masked(&theirs));
        let diff = om.iter().zip(&tm).enumerate().filter(|(_, (a, b))| a != b).map(|(i, _)| i).collect::<Vec<_>>();
        println!("{name}: {} bytes ours, {} BOB's; {} bytes differ{}", om.len(), tm.len(), diff.len(), diff.first().map(|i| format!(", first at {i:#x}")).unwrap_or_default());
    }
}
