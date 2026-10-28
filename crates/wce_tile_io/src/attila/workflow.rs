//! Opening an exported tile for editing and saving a project as an exported tile, over a game's folder: the exported tile (a folder on
//! disk, or the packs') is decompiled into a TEd project in a workspace (a kit-like `raw_data` tree with the tile's database entry), which
//! the editor opens like a kit tile; a project is compiled (stages A, B and C) into an exported tile folder, with the tile database in
//! effect plus the project's entry. docs/formats/tile_workflow.md.

use std::io::Cursor;
use std::path::{Path, PathBuf};

use rpfm_lib::files::tile_database::TileDatabase;
use rpfm_lib::files::{Decodeable, Encodeable};
use wce_tile::vegetation::{GenerationParams, grass::GrassParams};
use wce_tile_model::Blob;

use super::compile::{CompileContext, CompileNotes, compile};
use super::decompile::{Losses, decompile};
use super::game_data::GameData;
use super::stage_b::{PackIndex, StageB};
use super::stage_c::StageC;
use super::{TedProject, TileBinary};
use crate::{TileFiles, TileFormat};

/// A game's folder and what the round trip reads from it: the packs (tile databases, buildings, models) and the Assembly Kit
/// (`assembly_kit/raw_data` for BOB's inputs, `working_data` for the vegetation parameters).
pub struct Game {
    pub dir: PathBuf,
    pub data: GameData,
    pub packs: PackIndex,
    vegetation: Option<(GenerationParams, Option<GrassParams>)>,

    /// What did not load (no schema, no vegetation parameters ...).
    pub warnings: Vec<String>,
}

impl Game {
    /// Reads the game's packs (`data`) and the kit's vegetation parameters.
    ///
    /// # Errors
    ///
    /// The `data` folder can't be listed.
    pub fn load(dir: &Path) -> Result<Self, String> {
        let data = GameData::load(&dir.join("data"), super::game_data::default_schema_path().as_deref())?;
        let mut warnings = data.warnings.clone();
        let vegetation = match super::stage_c::kit_params(&dir.join("assembly_kit/raw_data")) {
            Ok(v) => Some(v),
            Err(e) => {
                warnings.push(format!("no vegetation parameters in the kit ({e}): tree and grass lists are carried only"));
                None
            }
        };

        Ok(Self { dir: dir.to_path_buf(), data, packs: PackIndex::new(&dir.join("data")), vegetation, warnings })
    }

    /// The Assembly Kit's `raw_data`.
    pub fn kit_raw_data(&self) -> PathBuf {
        self.dir.join("assembly_kit/raw_data")
    }

    /// An exported tile's files from the game's packs (`terrain/tiles/battle/.../aa`).
    ///
    /// # Errors
    ///
    /// No pack holds a file of the folder.
    pub fn exported_from_packs(&self, folder: &str) -> Result<TileFiles, String> {
        let files = self.packs.folder(folder);
        if files.is_empty() {
            return Err(format!("no pack holds {folder}"));
        }

        Ok(TileFiles { folder: files.into_iter().map(|(n, b)| (n, Blob::Bytes(b.into()))).collect(), entry: None, variation: None })
    }
}

/// The tile folder of a path (`terrain/tiles/battle/.../aa`, `/` separated): the part from `terrain/tiles/` on, as the packs name it.
pub fn tile_folder(path: &Path) -> Option<String> {
    let g = path.to_string_lossy().replace('\\', "/");
    let at = g.to_lowercase().find("terrain/tiles/")?;
    Some(g[at..].trim_end_matches('/').to_owned())
}

/// The kind of a tile folder (`battle`, `campaign`).
fn kind_of(folder: &str) -> String {
    folder.split('/').nth(2).unwrap_or("battle").to_lowercase()
}

/// Decompiles an exported tile (its files and folder, `terrain/tiles/...`) into a TEd project under `workspace` (`<workspace>/raw_data`
/// gets the project at the same folder and its entry in `terrain/tiles/<kind>/_tile_database/TILES`, with the kit's `_settings.xml`).
/// Returns the project's folder and what the decompile lost or made up.
///
/// # Errors
///
/// The files don't read as an exported tile, the decompile fails or a file can't be written.
pub fn open_exported(game: &Game, files: &TileFiles, folder: &str, workspace: &Path) -> Result<(PathBuf, Losses), String> {
    let (binary, _) = TileBinary.read(files).map_err(|e| e.to_string())?;
    let (project, losses) = decompile(&binary, folder, &game.data.context(None))?;
    let (written, _) = TedProject.write(&project).map_err(|e| e.to_string())?;
    let raw = workspace.join("raw_data");
    let dir = raw.join(folder);
    let database = raw.join("terrain/tiles").join(kind_of(folder)).join("_tile_database");
    written.write_to(&dir, Some(&database.join("TILES"))).map_err(|e| e.to_string())?;
    let settings = database.join("_settings.xml");
    if !settings.is_file() {
        let kit = game.kit_raw_data().join("terrain/tiles").join(kind_of(folder)).join("_tile_database/_settings.xml");
        std::fs::copy(&kit, &settings).map_err(|e| format!("{}: {e}", kit.display()))?;
    }

    // The editor resolves the tile through the workspace's index, which has a new entry now.
    wce_formats::tile_index::TileIndex::forget_shared();
    Ok((dir, losses))
}

/// The tile database `base` with the entries of `entries` (TILES XML files, read with the kit's `_settings.xml`) in place of the tiles
/// of the same location, or added where BOB's order puts them (larger tiles first, then by location); a tile set they name that `base`
/// lacks is added.
///
/// # Errors
///
/// An entry or the settings don't read.
pub fn merge_entries(base: &TileDatabase, settings: &str, entries: &[&str]) -> Result<TileDatabase, String> {
    // Only the tiles and tile sets are taken: an attribute rpfm wants that the kit's settings lack (the campaign kit's has no
    // `campaign_sea_transparency_scale`) is given 0.
    let mut settings = settings.to_owned();
    let mut tries = 0;
    let built = loop {
        match TileDatabase::from_assembly_kit(&settings, entries, None, false) {
            Ok(db) => break db,
            Err(e) => {
                let text = e.to_string();
                let missing = text.split_once("missing attribute '").and_then(|(_, r)| r.split_once("' on <")).and_then(|(a, r)| Some((a.to_owned(), r.split_once('>')?.0.to_owned())));
                let Some((attr, element)) = missing.filter(|_| tries < 64) else { return Err(text) };
                tries += 1;
                settings = with_attribute(&settings, &element, &attr);
            }
        }
    };
    let mut out = base.clone();
    for set in built.tile_sets() {
        if !out.tile_sets().iter().any(|s| s.name().eq_ignore_ascii_case(set.name())) {
            out.tile_sets_mut().push(set.clone());
        }
    }

    for tile in built.tiles() {
        let tiles = out.tiles_mut();
        match tiles.iter().position(|t| t.location().eq_ignore_ascii_case(tile.location())) {
            Some(i) => tiles[i] = tile.clone(),
            None => {
                let size = tile.width() * tile.height();
                let at = tiles.iter().position(|t| (t.width() * t.height(), tile.location()) < (size, t.location())).unwrap_or(tiles.len());
                tiles.insert(at, tile.clone());
            }
        }
    }

    Ok(out)
}

/// Every `<element ...>` of an XML text that lacks `attr` gets `attr="0"`.
fn with_attribute(xml: &str, element: &str, attr: &str) -> String {
    let open = format!("<{element}");
    let mut out = String::with_capacity(xml.len());
    let mut rest = xml;
    while let Some(at) = rest.find(&open) {
        let after = &rest[at + open.len()..];
        out.push_str(&rest[..at + open.len()]);
        let is_element = after.starts_with([' ', '>', '/', '\t', '\r', '\n']);
        let tag_end = after.find('>').unwrap_or(after.len());
        if is_element && !after[..tag_end].contains(&format!("{attr}=")) {
            out.push_str(&format!(" {attr}=\"0\""));
        }

        rest = after;
    }

    out.push_str(rest);
    out
}

/// Compiles a project (a variation folder under a `raw_data` tree with its entry, the workspace's or a kit's) into an exported tile under
/// `out` (`<out>/terrain/tiles/...`), stages A, B and C, and writes the tile database in effect with the project's entry
/// (`<out>/terrain/tiles/<kind>/tile_database.bin`). Returns the exported folder and the compile's notes; files already in the folder that
/// the compile did not write are noted, not removed.
///
/// # Errors
///
/// The project doesn't read or compile, or a file can't be written.
pub fn save_exported(game: &Game, project_dir: &Path, out: &Path) -> Result<(PathBuf, CompileNotes), String> {
    save_exported_scene(game, project_dir, None, out)
}

/// [`save_exported`] with the scene given as tile.ted's text (the editor's, prefab references baked: `project_files::to_ted`) instead of the
/// folder's.
///
/// # Errors
///
/// As `save_exported`.
pub fn save_exported_scene(game: &Game, project_dir: &Path, scene: Option<Vec<u8>>, out: &Path) -> Result<(PathBuf, CompileNotes), String> {
    save_exported_with(game, project_dir, scene, out, &|_| None)
}

/// [`save_exported_scene`] for a project with buildings the game's packs don't have yet (generated walls, docs/plans/procedural_walls.md):
/// `base_of` names the building a key was made from, whose destruction levels and thresholds it has.
///
/// # Errors
///
/// As `save_exported`.
pub fn save_exported_with(
    game: &Game, project_dir: &Path, scene: Option<Vec<u8>>, out: &Path, base_of: &(dyn Fn(&str) -> Option<String> + Sync),
) -> Result<(PathBuf, CompileNotes), String> {
    use wce_tile::project_files::{TED_FILE, WCE_FILE};

    wce_foundation::wce_prof!("export");
    let folder = tile_folder(project_dir).ok_or_else(|| format!("{} is not under terrain/tiles/", project_dir.display()))?;
    let mut files = TileFiles::from_kit_dir(project_dir).map_err(|e| e.to_string())?;
    if let Some(scene) = scene {
        files.folder.remove(WCE_FILE);
        files.folder.insert(TED_FILE.to_owned(), Blob::Bytes(scene.into()));
    }

    let entry_xml = files.entry.as_ref().map(|(_, b)| b.bytes().map(|b| String::from_utf8_lossy(&b).into_owned())).transpose().map_err(|e| e.to_string())?;
    let (raw, _) = {
        wce_foundation::wce_prof!("export.read_project");
        let _a = wce_foundation::activity::start("Reading the project (tile.ted, maps, entry)");
        TedProject.read(&files).map_err(|e| e.to_string())?
    };

    let kit_raw = game.kit_raw_data();
    let models = |k: &str| game.packs.read(k);
    let exists = |k: &str| game.packs.exists(k);
    let settings = kit_raw.join("terrain/tiles/battle/_tile_database/_settings.xml");
    let mut ctx: CompileContext = game.data.compile_context();
    let (levels, thresholds) = (ctx.building_levels, ctx.building_thresholds);
    let levels_or_base = |k: &str| levels.and_then(|f| f(k).or_else(|| base_of(k).and_then(|b| f(&b))));
    let thresholds_or_base = |k: &str| thresholds.and_then(|f| f(k).or_else(|| base_of(k).and_then(|b| f(&b))));
    ctx.building_levels = Some(&levels_or_base);
    ctx.building_thresholds = Some(&thresholds_or_base);
    ctx.stage_b = Some(StageB { raw_data: &kit_raw, models: &models });
    ctx.stage_c = game.vegetation.as_ref().map(|(params, grass)| StageC { params, grass: grass.as_ref(), settings: Some(&settings), model_exists: &exists });
    let (compiled, mut notes) = {
        wce_foundation::wce_prof!("export.compile");
        compile(&raw, &ctx)?
    };
    let (written, _) = {
        wce_foundation::wce_prof!("export.write_binary");
        let _a = wce_foundation::activity::start("Writing the tile's files");
        TileBinary.write(&compiled).map_err(|e| e.to_string())?
    };
    let dir = out.join(&folder);
    let stale = std::fs::read_dir(&dir)
        .into_iter()
        .flatten()
        .flatten()
        .filter(|e| e.path().is_file())
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|n| !written.folder.contains_key(n))
        .collect::<Vec<_>>();
    if !stale.is_empty() {
        notes.notes.push(format!("{} files already in {} were not written now (left as they are): {}", stale.len(), dir.display(), stale.join(", ")));
    }

    written.write_to(&dir, None).map_err(|e| e.to_string())?;

    // The tile database in effect, with the project's entry.
    let kind = kind_of(&folder);
    match (entry_xml, game.data.database(&kind)) {
        (Some(entry), Some(base)) => {
            let settings_path = kit_raw.join("terrain/tiles").join(&kind).join("_tile_database/_settings.xml");
            let settings = std::fs::read(&settings_path).map_err(|e| format!("{}: {e}", settings_path.display()))?;
            let mut db = merge_entries(base, &String::from_utf8_lossy(&settings), &[&entry])?;
            // A decompiled project's entry has the height range of its height map (and its density); while the terrain is as decompiled,
            // the row keeps the database's.
            if super::stage_b::unchanged(&raw, super::stage_b::Group::Terrain) {
                keep_height_range(&mut db, base, &folder);
            }

            let mut bytes = vec![];
            db.encode(&mut bytes, &None).map_err(|e| e.to_string())?;
            let path = out.join("terrain/tiles").join(&kind).join("tile_database.bin");
            std::fs::create_dir_all(path.parent().unwrap_or(out)).map_err(|e| e.to_string())?;
            std::fs::write(&path, bytes).map_err(|e| format!("{}: {e}", path.display()))?;
        }

        (None, _) => notes.notes.push("the project has no tile database entry: no tile_database.bin written".to_owned()),
        (_, None) => notes.notes.push(format!("the game's packs have no {kind} tile database: no tile_database.bin written")),
    }

    Ok((dir, notes))
}

/// The variation of `folder` (`terrain/tiles/<kind>/<location...>/<variation>`) in `db` gets `base`'s `min_height`, `scale` and
/// `raw_data_tri_density`.
fn keep_height_range(db: &mut TileDatabase, base: &TileDatabase, folder: &str) {
    let Some((location, variation)) = folder.rsplit_once('/') else { return };
    let location = location.replace('/', "\\").to_lowercase();
    let find = |d: &TileDatabase| d.tiles().iter().position(|t| t.location().to_lowercase() == location);
    let (Some(a), Some(b)) = (find(base), find(db)) else { return };
    let Some(old) = base.tiles()[a].variations().iter().find(|v| v.location().eq_ignore_ascii_case(variation)).cloned() else { return };
    if let Some(v) = db.tiles_mut()[b].variations_mut().iter_mut().find(|v| v.location().eq_ignore_ascii_case(variation)) {
        v.set_min_height(*old.min_height());
        v.set_scale(*old.scale());
        v.set_raw_data_tri_density(*old.raw_data_tri_density());
    }
}

/// A tile database read back (for checks).
///
/// # Errors
///
/// The bytes don't decode.
pub fn read_database(bytes: &[u8]) -> Result<TileDatabase, String> {
    TileDatabase::decode(&mut Cursor::new(bytes), &None).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_attributes_are_added_to_every_element() {
        let xml = "<A><B x=\"1\"/><B/><BC/><B y=\"0\">t</B></A>";
        assert_eq!(with_attribute(xml, "B", "y"), "<A><B y=\"0\" x=\"1\"/><B y=\"0\"/><BC/><B y=\"0\">t</B></A>");
    }

    #[test]
    fn tile_folders_from_paths() {
        assert_eq!(tile_folder(Path::new("/x/working_data/terrain/tiles/battle/a/b/aa/")).as_deref(), Some("terrain/tiles/battle/a/b/aa"));
        assert_eq!(tile_folder(Path::new("C:\\k\\Terrain\\Tiles\\battle\\a")).as_deref(), Some("Terrain/Tiles/battle/a"));
        assert_eq!(tile_folder(Path::new("/x/y")), None);
        assert_eq!(kind_of("terrain/tiles/Campaign/x"), "campaign");
    }
}
