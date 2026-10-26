//! Opening an exported tile for editing and saving a project as an exported tile, over a game's folder: the exported tile (a folder on
//! disk, or the packs') is decompiled into a TEd project in a workspace (a kit-like `raw_data` tree with the tile's database entry), which
//! the editor opens like a kit tile; a project is compiled (stages A, B and C) into an exported tile folder, with the tile database in
//! effect plus the project's entry. docs/formats/tile_workflow.md.

use std::io::Cursor;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use rpfm_lib::files::tile_database::TileDatabase;
use rpfm_lib::files::{Decodeable, Encodeable};
use wce_tile::vegetation::{GenerationParams, grass::GrassParams};
use wce_tile_model::Blob;
use wce_foundation::game::GameProfile;
use wce_vfs::{Config, Vfs};

use super::compile::{CompileContext, CompileNotes, compile};
use super::decompile::{Losses, decompile};
use super::game_data::GameData;
use super::stage_b::StageB;
use super::stage_c::StageC;
use super::{TedProject, TileBinary};
use crate::{TileFiles, TileFormat};

/// A game and what the round trip reads from it: the mounted packs (tile databases, buildings, models) and the Assembly Kit, if the game
/// has one with its tiles (`raw_data` for BOB's inputs, `working_data` for the vegetation parameters).
pub struct Game {
    pub profile: GameProfile,

    /// The game's packs, as the editor mounted them.
    pub files: Arc<Vfs>,

    /// The kit's `raw_data`, when it has the tiles (`terrain/tiles`; not Rome 2's).
    pub kit_raw: Option<PathBuf>,
    pub data: GameData,
    vegetation: Option<(GenerationParams, Option<GrassParams>)>,

    /// What did not load (no schema, no vegetation parameters ...).
    pub warnings: Vec<String>,
}

impl Game {
    /// A game's folder as the checks see it (its own packs and every mod pack of its data folder) and the kit in it
    /// (`assembly_kit`).
    ///
    /// # Errors
    ///
    /// The packs can't be mounted.
    pub fn load(profile: &GameProfile, dir: &Path) -> Result<Self, String> {
        let mut config = Config::new(dir, profile.clone());
        config.use_mods = false;
        config.mod_packs = Some(profile.available_mod_packs(dir));
        let files = Arc::new(Vfs::mount(&config).map_err(|e| e.to_string())?);
        Self::from_vfs(files, Some(&dir.join("assembly_kit")))
    }

    /// The game of mounted packs (their profile) and its kit (the folder with `raw_data`, if any): the packs' tile data and the kit's
    /// vegetation parameters.
    ///
    /// # Errors
    ///
    /// None for now: what can't be read is a warning.
    pub fn from_vfs(files: Arc<Vfs>, kit: Option<&Path>) -> Result<Self, String> {
        let profile = files.profile().clone();
        let data = GameData::from_vfs(&files, super::game_data::default_schema_path(&profile).as_deref())?;
        let mut warnings = data.warnings.clone();
        let kit_raw = kit.map(|k| k.join("raw_data")).filter(|r| r.join("terrain/tiles").is_dir());
        let vegetation = match &kit_raw {
            Some(raw) => match super::stage_c::kit_params(raw) {
                Ok(v) => Some(v),
                Err(e) => {
                    warnings.push(format!("no vegetation parameters in the kit ({e}): tree and grass lists are carried only"));
                    None
                }
            },

            None => {
                warnings.push(format!("{}: no Assembly Kit with the tiles: the vegetation is carried only, BOB's inputs are the projects'", profile.display_name));
                None
            }
        };

        Ok(Self { profile, files, kit_raw, data, vegetation, warnings })
    }

    /// The `raw_data` BOB's stages read for a project under `project_raw` (a workspace's or a kit's `raw_data`): the kit's when the game
    /// has one with the tiles, else the project's own (the workspace has the database's settings).
    pub fn bob_raw_data(&self, project_raw: &Path) -> PathBuf {
        self.kit_raw.clone().unwrap_or_else(|| project_raw.to_path_buf())
    }

    /// An exported tile's files from the game's packs (`terrain/tiles/battle/.../aa`).
    ///
    /// # Errors
    ///
    /// No pack holds a file of the folder.
    pub fn exported_from_packs(&self, folder: &str) -> Result<TileFiles, String> {
        let prefix = format!("{}/", wce_vfs::normalize(folder).trim_end_matches('/'));
        let mut out = TileFiles::default();
        for path in self.files.list(&prefix, "") {
            let bytes = self.files.read(path).map_err(|e| e.to_string())?;
            out.folder.insert(path[prefix.len()..].to_owned(), Blob::Bytes(bytes.into()));
        }

        if out.folder.is_empty() {
            return Err(format!("no pack holds {folder}"));
        }

        Ok(out)
    }

    /// The tile database settings of a kind (`battle`, `campaign`) in the kit's format (`_settings.xml` of `_tile_database`): the kit's
    /// file, else (Rome 2) the packs' database written so.
    ///
    /// # Errors
    ///
    /// Neither has it.
    pub fn kit_settings(&self, kind: &str) -> Result<Vec<u8>, String> {
        if let Some(raw) = &self.kit_raw {
            let path = raw.join("terrain/tiles").join(kind).join("_tile_database/_settings.xml");
            return std::fs::read(&path).map_err(|e| format!("{}: {e}", path.display()));
        }

        let db = self.data.database(kind).ok_or_else(|| format!("the game has no {kind} tile database"))?;
        Ok(crate::rome2::kit_settings::kit_settings_xml(db).into_bytes())
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
        std::fs::write(&settings, game.kit_settings(&kind_of(folder))?).map_err(|e| format!("{}: {e}", settings.display()))?;
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

    let project_raw = project_dir.ancestors().find(|a| a.file_name().is_some_and(|n| n.eq_ignore_ascii_case("raw_data"))).unwrap_or(project_dir);
    let kit_raw = game.bob_raw_data(project_raw);
    let mut early_notes = vec![];
    if game.kit_raw.is_none()
        && let Err(e) = provide_linking_tiles(game, &raw, &kit_raw)
    {
        early_notes.push(format!("the linking tiles for stage B: {e}"));
    }

    let models = |k: &str| game.files.read(k).ok();
    let exists = |k: &str| game.files.owner(k).is_some();
    let settings = kit_raw.join("terrain/tiles/battle/_tile_database/_settings.xml");
    let mut ctx: CompileContext = game.data.compile_context();
    let (levels, thresholds) = (ctx.building_levels, ctx.building_thresholds);
    let levels_or_base = |k: &str| levels.and_then(|f| f(k).or_else(|| base_of(k).and_then(|b| f(&b))));
    let thresholds_or_base = |k: &str| thresholds.and_then(|f| f(k).or_else(|| base_of(k).and_then(|b| f(&b))));
    ctx.building_levels = Some(&levels_or_base);
    ctx.building_thresholds = Some(&thresholds_or_base);
    ctx.stage_b = Some(StageB { raw_data: &kit_raw, models: &models });
    ctx.stage_c = game.vegetation.as_ref().map(|(params, grass)| StageC { params, grass: grass.as_ref(), settings: Some(&settings), model_exists: &exists });
    ctx.model_exists = Some(&exists);
    let (compiled, mut notes) = {
        wce_foundation::wce_prof!("export.compile");
        compile(&raw, &ctx)?
    };
    notes.notes.splice(0..0, early_notes);
    let (mut written, _) = {
        wce_foundation::wce_prof!("export.write_binary");
        let _a = wce_foundation::activity::start("Writing the tile's files");
        TileBinary.write(&compiled).map_err(|e| e.to_string())?
    };
    for name in game.data.conventions.not_shipped {
        written.folder.remove(*name);
    }

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
            let settings = game.kit_settings(&kind)?;
            let mut db = merge_entries(base, &String::from_utf8_lossy(&settings), &[&entry])?;
            // A decompiled project's entry has the height range of its height map (and its density); while the terrain is as decompiled,
            // the row keeps the database's.
            if super::stage_b::unchanged(&raw, super::stage_b::Group::Terrain) {
                keep_height_range(&mut db, base, &folder);
            }

            let written = match game.data.xml_databases.get(&kind) {
                // Rome 2: the tile's set file (and the settings, for a new texture group).
                Some(xml) => {
                    let (location, _) = super::decompile::split_location(&folder);
                    let tile = db.tiles().iter().find(|t| t.location().eq_ignore_ascii_case(&location)).ok_or("the merged database lacks the tile")?;
                    xml.with_tile(tile)?.into_iter().map(|(name, bytes)| (format!("{}{name}", crate::rome2::tile_database::Rome2TileDatabase::folder(&kind)), bytes)).collect()
                }

                None => {
                    let mut bytes = vec![];
                    db.encode(&mut bytes, &None).map_err(|e| e.to_string())?;
                    vec![(format!("terrain/tiles/{kind}/tile_database.bin"), bytes)]
                }
            };

            for (name, bytes) in written {
                let path = out.join(&name);
                std::fs::create_dir_all(path.parent().unwrap_or(out)).map_err(|e| e.to_string())?;
                std::fs::write(&path, bytes).map_err(|e| format!("{}: {e}", path.display()))?;
            }
        }

        (None, _) => notes.notes.push("the project has no tile database entry: no tile database written".to_owned()),
        (_, None) => notes.notes.push(format!("the game's packs have no {kind} tile database: no tile database written")),
    }

    Ok((dir, notes))
}

/// For a game without a kit (Rome 2; BOB's inputs are the workspace's `raw_data`), the linking tiles the project's links blend with: each
/// one the workspace lacks is decompiled from the packs into it (unmarked: where BOB finds a kit's) with its `final_heights.dds` (its
/// height map without links, as BOB converts a linking tile first). Once per linking tile.
///
/// # Errors
///
/// A linking tile is in no pack, or does not decompile.
fn provide_linking_tiles(game: &Game, raw: &wce_tile_model::Tile, raw_data: &Path) -> Result<(), String> {
    use super::bob_tile::{HeightNumbers, InfluenceLinks, dds_r32f, final_heights};
    let Some(entry) = raw.entry.as_ref() else { return Ok(()) };
    let linking = super::terrain_mesh::linking_tiles(raw_data);
    let own = entry.root.get("tile_set").and_then(|v| v.as_str()).unwrap_or_default().to_owned();
    let mut wanted = std::collections::BTreeSet::new();
    let sets = entry.root.wrapped("TILE_LINKS").iter().filter_map(|l| l.get("link_set").and_then(|v| v.as_str()).map(str::to_owned)).chain([own, "sea".to_owned()]);
    for set in sets {
        if let Some(tile) = linking.get(&set).filter(|t| !t.is_empty()) {
            wanted.insert(tile.replace('\\', "/").to_lowercase());
        }
    }

    let workspace = raw_data.parent().unwrap_or(raw_data);
    for folder in wanted {
        let dir = raw_data.join(&folder);
        if dir.join("final_heights.dds").is_file() {
            continue;
        }

        let files = game.exported_from_packs(&folder)?;
        let (dir, _) = open_exported(game, &files, &folder, workspace)?;
        let tile = TileFiles::from_kit_dir(&dir).map_err(|e| e.to_string())?;
        let (link, _) = TedProject.read(&tile).map_err(|e| e.to_string())?;
        let hm = link.map(wce_tile_model::MapKind::Height, 0).ok_or_else(|| format!("{folder}: no height map"))?;
        let wce_tile_model::Samples::U16(hs) = &hm.raster.samples else { return Err(format!("{folder}: the height map is not 16 bit")) };
        let (f, w, h) = final_heights(hs, hm.raster.width as usize, hm.raster.height as usize, &HeightNumbers::of(&link), &InfluenceLinks::default())?;
        std::fs::write(dir.join("final_heights.dds"), dds_r32f(&f, w, h)).map_err(|e| format!("{}: {e}", dir.display()))?;
    }

    Ok(())
}

/// A tile tree for the map context's neighbours of a game without kit tiles (Rome 2): `<root>/raw_data` with each kind's database
/// settings and every tile's entry in the kit's format, written once; the tiles' folders are decompiled into it when the context first
/// needs them ([`neighbour_tile`]). Returns the `raw_data` folder.
///
/// # Errors
///
/// A file can't be written.
pub fn neighbour_root(game: &Game, root: &Path) -> Result<PathBuf, String> {
    let raw = root.join("raw_data");
    for kind in ["battle", "campaign"] {
        let Some(db) = game.data.database(kind) else { continue };
        let dir = raw.join("terrain/tiles").join(kind).join("_tile_database");
        let settings = dir.join("_settings.xml");
        if settings.is_file() {
            continue;
        }

        let tiles = dir.join("TILES");
        std::fs::create_dir_all(&tiles).map_err(|e| format!("{}: {e}", tiles.display()))?;
        for (i, t) in db.tiles().iter().enumerate() {
            let path = tiles.join(super::decompile::entry_file_name(t.location()));
            std::fs::write(&path, super::decompile::entry_xml(db, i)).map_err(|e| format!("{}: {e}", path.display()))?;
        }

        // Last: the folder is complete once the settings are there.
        std::fs::write(&settings, game.kit_settings(kind)?).map_err(|e| format!("{}: {e}", settings.display()))?;
    }

    Ok(raw)
}

/// A tile's folder under a [`neighbour_root`] (`location` as the map names it, `terrain\tiles\...\<tile>`, and the variation), decompiled
/// from the packs the first time.
///
/// # Errors
///
/// No pack has the tile, or it does not decompile.
pub fn neighbour_tile(game: &Game, raw: &Path, location: &str, variation: &str) -> Result<PathBuf, String> {
    let folder = format!("{}/{variation}", location.replace('\\', "/").trim_end_matches('/')).to_lowercase();
    let dir = raw.join(&folder);
    if wce_tile::tile_project::has_tile(&dir) {
        return Ok(dir);
    }

    let files = game.exported_from_packs(&folder)?;
    let (dir, _) = open_exported(game, &files, &folder, raw.parent().unwrap_or(raw))?;
    Ok(dir)
}

/// [`save_exported_scene`] into a mod pack (created if missing): the exported tile's folder and the tile database's files, the pack's
/// files below the tile's folder replaced. Returns the tile's folder in the pack and the compile's notes.
///
/// # Errors
///
/// As `save_exported`, or the pack can't be read or written.
pub fn save_exported_to_pack(game: &Game, project_dir: &Path, scene: Option<Vec<u8>>, pack: &Path) -> Result<(String, CompileNotes), String> {
    let scratch = std::env::temp_dir().join(format!("wce_pack_{}_{}", std::process::id(), std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_nanos())));
    let result = (|| {
        let (dir, notes) = save_exported_scene(game, project_dir, scene, &scratch)?;
        let folder = dir.strip_prefix(&scratch).map_err(|e| e.to_string())?.to_string_lossy().replace('\\', "/");
        let mut files = vec![];
        let mut pending = vec![scratch.clone()];
        while let Some(d) = pending.pop() {
            for entry in std::fs::read_dir(&d).map_err(|e| format!("{}: {e}", d.display()))?.flatten() {
                let path = entry.path();
                if path.is_dir() {
                    pending.push(path);
                } else {
                    let inside = path.strip_prefix(&scratch).map_err(|e| e.to_string())?.to_string_lossy().replace('\\', "/");
                    files.push((inside, std::fs::read(&path).map_err(|e| format!("{}: {e}", path.display()))?));
                }
            }
        }

        files.sort();
        wce_vfs::mod_pack::write_files(&game.profile, pack, &[&folder], &files)?;
        Ok((folder, notes))
    })();
    let _ = std::fs::remove_dir_all(&scratch);
    result
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
