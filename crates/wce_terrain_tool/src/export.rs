//! Export (docs/plans/terrain_editor.md, Phase 7): a kit map's built files (`wce_terrain_io::attila::build`: what BOB_Terrain's
//! `Tilemap` and `Heights & Normals` write, and `tile_map.png`) made from the map as it is in the editor, on a worker, written two ways:
//!
//! - loose, under the output folder as the packs hold them: `<output>/terrain/battles/<map>/...` (the deployment files in `deployment/`), a
//!   campaign map's under `terrain/campaigns/<map>/`;
//! - a mod pack `<output>/terrain_<map>.pack` with the same paths.
//!
//! The game's LF normal chunks are not made: a map of the game's packs keeps its own, a map the game doesn't have has none.
//!
//! A built map (a pack map of a game without kit maps: Rome 2) exports the files of `wce_terrain_io::built::write` that differ from the
//! packs' (an LF edit: its DDS and CHMF).

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::mpsc::{Receiver, channel};
use std::time::Instant;

use wce_editor::editor::Editor;
use wce_scene::render_scene::DocumentId;
use wce_terrain_editor::MapDocument;
use wce_terrain_io::attila::build::{BuildInputs, build};
use wce_tile::battle_map::tile_db::TileDatabase;

/// An export done.
#[derive(Clone, Debug)]
pub struct Exported {
    pub map: String,

    /// The map document's history version it was made from.
    pub revision: u64,
    pub folder: PathBuf,
    pub pack: PathBuf,

    /// The files under the map's folder (relative, `/`), with their bytes.
    pub files: Vec<(String, Vec<u8>)>,
    pub messages: Vec<String>,
    pub secs: f64,
}

/// An export on its way.
pub struct ExportJob {
    pub map: String,
    pub started: Instant,
    rx: Receiver<Result<Exported, String>>,
}

impl ExportJob {
    /// The export, once done.
    pub fn poll(&self) -> Option<Result<Exported, String>> {
        self.rx.try_recv().ok()
    }
}

/// The first of the kits' files (`raw_data` relative), in the data source order.
fn kit_file(roots: &[PathBuf], rel: &str) -> Vec<u8> {
    roots.iter().find_map(|r| std::fs::read(r.join(rel)).ok()).unwrap_or_default()
}

/// A map's folder in the packs (`terrain/battles/<map>`, a campaign map's `terrain/campaigns/<map>`) and its bare name.
pub fn game_folder(map: &str) -> (String, &str) {
    let map = map.strip_suffix(wce_map_view::maps::GAME_FILES).unwrap_or(map);
    match crate::tool::map_kind(map) {
        (wce_terrain::MapKind::Campaign, n) => (format!("terrain/campaigns/{n}"), n),
        (_, n) => (format!("terrain/battles/{n}"), n),
    }
}

/// The pack's paths: the map's files under its folder in the packs.
pub fn pack_files(map: &str, files: &[(String, Vec<u8>)]) -> Vec<(String, Vec<u8>)> {
    let (folder, _) = game_folder(map);
    files.iter().map(|(rel, b)| (format!("{folder}/{rel}"), b.clone())).collect()
}

/// Writes an export's files loose and as a pack (`profile`: the game's, for the pack format).
fn write(out: &Path, map: &str, files: &[(String, Vec<u8>)], profile: &wce_vfs::GameProfile) -> Result<(PathBuf, PathBuf), String> {
    let (game, name) = game_folder(map);
    let folder = out.join(&game);
    for (rel, bytes) in files {
        let path = folder.join(rel);
        if let Some(p) = path.parent() {
            std::fs::create_dir_all(p).map_err(|err| format!("{}: {err}", p.display()))?;
        }

        std::fs::write(&path, bytes).map_err(|err| format!("{}: {err}", path.display()))?;
    }

    let pack = out.join(format!("terrain_{name}.pack"));
    wce_vfs::mod_pack::write_new(profile, &pack, &pack_files(map, files))?;
    Ok((folder, pack))
}

/// Starts the export of a map document (a kit map: its model) on a worker.
///
/// # Errors
///
/// No such document, a map of the packs (no model to build from).
pub fn start(e: &Editor, doc: DocumentId, db: Arc<TileDatabase>, kit_roots: &[PathBuf]) -> Result<ExportJob, String> {
    let d = e.docs.find_as::<MapDocument>(doc).ok_or("no map shown")?;
    let model = d.model().ok_or("a map of the game's packs can't be exported (open the kit's map)")?.clone();
    let map = d.name.clone();
    let revision = wce_editor_core::document::Document::history(d).version();
    let env = kit_file(kit_roots, "warscape_asset_variation_db/environment_map.xml");
    let upgrades = kit_file(kit_roots, "terrain/tiles/battle/tile_upgrades.xml");
    let out = e.output_dir();
    let profile = e.game();
    let packs = e.files();
    let (tx, rx) = channel();
    let name = map.clone();
    std::thread::spawn(move || {
        let t0 = Instant::now();
        let built = if model.built { Some(built_changes(&model, &name, packs.as_deref())) } else { None };
        let r = built.unwrap_or_else(|| build(&model, &BuildInputs { db: &db, environment_map: &env, tile_upgrades: &upgrades }).map_err(|err| err.to_string()))
            .and_then(|b| {
                let (folder, pack) = write(&out, &name, &b.files, profile)?;
                Ok(Exported { map: name.clone(), revision, folder, pack, files: b.files, messages: b.messages, secs: t0.elapsed().as_secs_f64() })
            });
        let _ = tx.send(r);
    });

    Ok(ExportJob { map, started: Instant::now(), rx })
}

/// A built map's files that differ from the packs' (or the packs lack), as a build's.
fn built_changes(model: &wce_terrain::MapModel, name: &str, packs: Option<&dyn wce_vfs::files::Files>) -> Result<wce_terrain_io::attila::build::Built, String> {
    let written = wce_terrain_io::built::write(model).map_err(|err| err.to_string())?;
    let (folder, _) = game_folder(name);
    let mut files = vec![];
    for (rel, blob) in &written.folder {
        let bytes = blob.bytes().map_err(|err| format!("{rel}: {err}"))?;
        let shipped = packs.and_then(|p| p.read(&format!("{folder}/{rel}")).ok());
        if shipped.as_deref() != Some(&bytes[..]) {
            files.push((rel.clone(), bytes.to_vec()));
        }
    }

    Ok(wce_terrain_io::attila::build::Built { files, messages: vec![] })
}

/// A line for the status bar.
pub fn summary(x: &Exported) -> String {
    let bytes: usize = x.files.iter().map(|f| f.1.len()).sum();
    let mut m = format!(
        "exported {}: {} files, {:.1} MB in {:.1} s, to {} and {}",
        x.map, x.files.len(), bytes as f64 / 1e6, x.secs, x.folder.display(), x.pack.display()
    );

    if !x.messages.is_empty() {
        m += &format!(" ({} fill messages: {})", x.messages.len(), x.messages.iter().take(3).cloned().collect::<Vec<_>>().join("; "));
    }

    m
}
