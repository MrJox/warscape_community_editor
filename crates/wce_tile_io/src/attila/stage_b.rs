//! Stage B of the compile: what BOB's Tile processor makes of a project, as the exported tile's files: `blend.dds`, `index.dds`,
//! `normal.dds`, `ground_types.dds` and `tile.agf` (`bob_tile::textures`); `outfield_mesh.rigid_model_v2`, `mesh.rigid_model_v2`,
//! `hf_height_map.data`, `hf_water_map.data` and `terrain_outlines.xml` (`terrain_mesh::terrain_files`). docs/formats/tile_compile.md,
//! stage B.

use std::path::Path;

use wce_tile_model::Tile;

use super::compile::CompileNotes;

/// What stage B needs besides the project: the kit's `raw_data` (the tile database's `_settings.xml`, the linking tiles' final
/// heights, the db tables and the buildings' raw height-map meshes) and a model's bytes by key (BOB's VFS: the game's packs, for the
/// water planes).
#[derive(Clone, Copy)]
pub struct StageB<'a> {
    pub raw_data: &'a Path,
    pub models: &'a dyn Fn(&str) -> Option<Vec<u8>>,
}

/// Files by name, as they go into the exported tile's folder.
pub type Files = Vec<(String, Vec<u8>)>;

/// The files BOB writes into the raw folder rather than the exported tile.
const RAW_FOLDER_FILES: [&str; 2] = ["final_heights.dds", "final_alpha.dds"];

/// Stage B's files of a project, in the order BOB's processor declares them, with notes on what BOB itself would not finish.
///
/// # Errors
///
/// The project lacks a blend or height map, or a linking tile's heights are missing.
pub fn stage_b_files(raw: &Tile, ctx: &StageB) -> Result<(Files, CompileNotes), String> {
    let mut notes = CompileNotes::default();
    let mut out = super::bob_tile::textures(raw)?;
    let (stamps, missing) = super::terrain_mesh::building_stamps(raw, ctx.raw_data);
    if !missing.is_empty() {
        notes.notes.push(format!(
            "stage B: no height-map mesh for {} (BOB: \"Height map mesh file ... is missing\", no hf_height_map.data nor terrain_outlines.xml)",
            missing.join(", ")
        ));
    }

    let angle = super::terrain_mesh::decimation_angle(ctx.raw_data);
    let terrain = super::terrain_mesh::terrain_files(raw, ctx.raw_data, angle, &stamps, ctx.models)?;
    out.extend(terrain.into_iter().filter(|(name, _)| !RAW_FOLDER_FILES.contains(&name.as_str())));
    Ok((out, notes))
}

/// A model lookup over a game's `data` folder, as BOB's VFS finds the water planes' models: the packs in name order, the first that
/// holds the path (case aside) wins. The packs' file lists are indexed on the first call.
pub fn pack_models(data: &Path) -> impl Fn(&str) -> Option<Vec<u8>> + Send + Sync + use<> {
    use rpfm_lib::files::pack::Pack;
    use std::collections::HashMap;
    use std::path::PathBuf;
    use std::sync::OnceLock;

    let data = data.to_path_buf();
    let index: OnceLock<HashMap<String, (PathBuf, String)>> = OnceLock::new();
    let attila = || rpfm_lib::games::supported_games::SupportedGames::default().game("attila").cloned();
    move |key: &str| {
        let index = index.get_or_init(|| {
            let mut packs = std::fs::read_dir(&data).into_iter().flatten().flatten().map(|e| e.path()).filter(|p| p.extension().is_some_and(|x| x == "pack")).collect::<Vec<_>>();
            packs.sort();
            let mut out = HashMap::new();
            let Some(game) = attila() else { return out };
            for p in packs {
                let Ok(pack) = Pack::read_and_merge(std::slice::from_ref(&p), &game, true, false, false) else { continue };
                for name in pack.files().keys() {
                    out.entry(name.to_lowercase()).or_insert_with(|| (p.clone(), name.clone()));
                }
            }

            out
        });
        let (pack, name) = index.get(&key.replace('\\', "/").to_lowercase())?;
        let game = attila()?;
        let mut pack = Pack::read_and_merge(std::slice::from_ref(pack), &game, true, false, false).ok()?;
        pack.files_mut().get_mut(name)?.encode(&None, false, false, true).ok().flatten()
    }
}
