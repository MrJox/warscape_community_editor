//! Export (docs/plans/terrain_editor.md, Phase 7): a kit map's built files (`wce_terrain_io::attila::build`: what BOB_Terrain's
//! `Tilemap` and `Heights & Normals` write, and `tile_map.png`) made from the map as it is in the editor, on a worker, written two ways:
//!
//! - loose, under the folder asked for (File > Export > Built files; Launch in game's own export: the project's build folder) as the packs
//!   hold them: `<folder>/terrain/battles/<map>/...` (the deployment files in `deployment/`), a campaign map's under `terrain/campaigns/<map>/`;
//! - a mod pack `<folder>/terrain_<map>.pack` with the same paths.
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
use wce_foundation::log;
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

/// The first of the kits' files (`raw_data` relative), in the data source order, else the packs' (`pack_path`); empty, with a warning in
/// the log, when none has it.
fn kit_file(roots: &[PathBuf], rel: &str, packs: Option<&dyn wce_vfs::files::Files>, pack_path: &str) -> Vec<u8> {
    wce_tile::sources::first_file(roots, rel)
        .and_then(|p| std::fs::read(p).ok())
        .or_else(|| packs.and_then(|f| f.read(pack_path).ok()))
        .unwrap_or_else(|| {
            log::warn(format!("terrain export: no kit or pack has {rel}: the export goes on without it"));
            vec![]
        })
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
fn write(
    out: &Path,
    map: &str,
    files: &[(String, Vec<u8>)],
    profile: &wce_vfs::GameProfile,
    model: &wce_terrain::MapModel,
    normals: &std::sync::Mutex<std::collections::BTreeMap<String, Arc<[u8]>>>,
    changes: &std::sync::Mutex<wce_foundation::disk_changes::Changes>,
    attempt: wce_foundation::disk_changes::Attempt,
) -> Result<(PathBuf, PathBuf), String> {
    use wce_foundation::publication::{Output, Source, Staged};
    let (game, name) = game_folder(map);
    let folder = out.join(&game);
    let pack = out.join(format!("terrain_{name}.pack"));
    let mut outputs = vec![];
    for (rel, bytes) in files {
        let mut output = Output::new(wce_foundation::publication::resource_path(&folder, rel)?, Source::Bytes(Arc::from(bytes.as_slice())));
        output.priority = wce_terrain_io::publication_priority(rel);
        outputs.push(output);
    }
    // Generate the pack before any loose output is published. Its reserved work file is disposable.
    let work = pack.with_added_extension("build.wce_tmp");
    struct Work(PathBuf);
    impl Drop for Work {
        fn drop(&mut self) {
            let _ = std::fs::remove_file(&self.0);
        }
    }
    let _work = Work(work.clone());
    if work.exists() {
        std::fs::remove_file(&work).map_err(|e| e.to_string())?;
    }
    wce_vfs::mod_pack::write_new(profile, &work, &pack_files(map, files))?;
    let mut pack_output = Output::new(pack.clone(), Source::File(work));
    pack_output.priority = 5;
    outputs.push(pack_output);
    let staged = Staged::prepare(outputs).map_err(|e| e.to_string())?;
    attempt.recheck()?;
    let mut inputs = normals.lock().map_err(|e| e.to_string())?;
    for (key, _) in files {
        if key.to_ascii_lowercase().starts_with("normals/")
            && !inputs.contains_key(key)
            && let Some(blob @ wce_tile_model::Blob::File(path)) = model.carried.get(key)
            && wce_foundation::save_path::key(path)? == wce_foundation::save_path::key(&folder.join(key))?
        {
            inputs.insert(key.clone(), blob.bytes().map_err(|e| e.to_string())?);
        }
    }
    drop(inputs);
    let lf_source = model.lf(wce_terrain::LfRole::Land).and_then(|i| match &i.origin {
        wce_tile_model::Blob::File(p) => wce_foundation::save_path::key(p).ok(),
        _ => None,
    });
    let attempt = std::cell::RefCell::new(attempt);
    staged
        .publish_checked(
            |p| attempt.borrow().check_target(p),
            |p| {
                attempt.borrow_mut().published(p)?;
                changes.lock().map_err(|e| e.to_string())?.note_owned(p)?;
                if lf_source.as_ref().is_some_and(|source| wce_foundation::save_path::key(p).as_ref() == Ok(source)) {
                    normals.lock().map_err(|e| e.to_string())?.clear();
                }
                Ok(())
            },
        )
        .map_err(|e| e.to_string())?;
    Ok((folder, pack))
}

/// Starts the export of a map document (a kit map: its model) on a worker.
///
/// # Errors
///
/// No such document, a map of the packs (no model to build from).
pub fn start(e: &Editor, doc: DocumentId, db: Arc<TileDatabase>, kit_roots: &[PathBuf], out: PathBuf) -> Result<ExportJob, String> {
    let d = e.docs.find_as::<MapDocument>(doc).ok_or("no map shown")?;
    let lease = d.begin_io()?;
    let model = d.publication_model()?;
    let changes = d.io_changes.clone();
    let normals = d.normal_inputs();
    let attempt = changes.lock().map_err(|e| e.to_string())?.begin_export(&out)?;
    let map = d.name.clone();
    let revision = wce_editor_core::document::Document::history(d).version();
    let env = kit_file(kit_roots, "warscape_asset_variation_db/environment_map.xml", None, "");
    let upgrades =
        kit_file(kit_roots, "terrain/tiles/battle/tile_upgrades.xml", e.files().as_deref(), "terrain/tiles/battle/tile_upgrades.xml");
    let profile = e.game();
    let packs = e.files();
    let (tx, rx) = channel();
    let name = map.clone();
    std::thread::spawn(move || {
        let _lease = lease;
        let t0 = Instant::now();
        let built = if model.built { Some(built_changes(&model, &db, &name, packs.as_deref())) } else { None };
        let r = built
            .unwrap_or_else(|| {
                build(&model, &BuildInputs { db: &db, environment_map: &env, tile_upgrades: &upgrades }).map_err(|err| err.to_string())
            })
            .and_then(|b| {
                let (folder, pack) = write(&out, &name, &b.files, profile, &model, &normals, &changes, attempt)?;
                Ok(Exported {
                    map: name.clone(),
                    revision,
                    folder,
                    pack,
                    files: b.files,
                    messages: b.messages,
                    secs: t0.elapsed().as_secs_f64(),
                })
            });
        let _ = tx.send(r);
    });

    Ok(ExportJob { map, started: Instant::now(), rx })
}

/// A built map's files that differ from the packs' (or the packs lack), as a build's.
fn built_changes(
    model: &wce_terrain::MapModel,
    db: &TileDatabase,
    name: &str,
    packs: Option<&dyn wce_vfs::files::Files>,
) -> Result<wce_terrain_io::attila::build::Built, String> {
    let written = wce_terrain_io::built::write(model, Some(db)).map_err(|err| err.to_string())?;
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
        x.map,
        x.files.len(),
        bytes as f64 / 1e6,
        x.secs,
        x.folder.display(),
        x.pack.display()
    );

    if !x.messages.is_empty() {
        m += &format!(" ({} fill messages: {})", x.messages.len(), x.messages.iter().take(3).cloned().collect::<Vec<_>>().join("; "));
    }

    m
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn step_c_export_stages_pack_and_loose_files_and_retains_exact_content() {
        let root = std::env::temp_dir().join(format!(
            "wce-terrain-export-{}-{}",
            std::process::id(),
            std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()
        ));
        std::fs::create_dir(&root).unwrap();
        let changes = std::sync::Mutex::new(wce_foundation::disk_changes::Changes::default());
        let normals = std::sync::Mutex::new(Default::default());
        let model = wce_terrain::MapModel::default();
        let files = vec![("opaque.bin".into(), b"native bytes".to_vec()), ("lf_height_map.dds".into(), b"LF baseline".to_vec())];
        let run = |files: &[(String, Vec<u8>)]| {
            let attempt = changes.lock().unwrap().begin_export(&root).unwrap();
            write(&root, "map", files, &wce_vfs::GameProfile::ROME_2, &model, &normals, &changes, attempt)
        };
        let (folder, pack) = run(&files).unwrap();
        let old = std::fs::read(folder.join("opaque.bin")).unwrap();
        let modified = std::fs::metadata(folder.join("opaque.bin")).unwrap().modified().unwrap();
        run(&files).unwrap();
        assert_eq!(std::fs::metadata(folder.join("opaque.bin")).unwrap().modified().unwrap(), modified);
        let original = std::fs::metadata(&pack).unwrap().permissions();
        let mut p = original.clone();
        p.set_readonly(true);
        std::fs::set_permissions(&pack, p).unwrap();
        let changed = vec![("opaque.bin".into(), b"changed bytes".to_vec())];
        assert!(run(&changed).is_err());
        assert_eq!(std::fs::read(folder.join("opaque.bin")).unwrap(), old);
        std::fs::set_permissions(&pack, original).unwrap();
        let canonical = root.canonicalize().unwrap();
        assert!(
            canonical.starts_with(std::env::temp_dir().canonicalize().unwrap())
                && canonical.file_name().unwrap().to_string_lossy().starts_with("wce-terrain-export-")
        );
        std::fs::remove_dir_all(canonical).unwrap();
    }
}
