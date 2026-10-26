//! Where a tile is saved and how it reaches the Assembly Kit (`wce_tile::project_files`). A tile is saved as `tile.wce` in the projects
//! folder (`<projects>/assembly_kit`, laid out like a kit's raw_data), never in the Assembly Kit: the first save of a tile opened from a kit
//! makes its project (the tile's folder and its database entry copied there) and the document goes on from there. "Save for Assembly Kit"
//! writes a tile TEd reads into the kit.

use std::path::{Path, PathBuf};


use super::TileDocument;

/// The tile folder of a path (`terrain/tiles/<kind>/.../<variation>`, `/` separated), if it has one.
pub fn tile_folder(path: &Path) -> Option<String> {
    let g = path.to_string_lossy().replace('\\', "/");
    let at = g.to_lowercase().find("terrain/tiles/")?;
    Some(g[at..].trim_end_matches('/').to_owned())
}

/// The kind of a tile folder (`battle`, `campaign`).
fn kind_of(folder: &str) -> String {
    folder.split('/').nth(2).unwrap_or("battle").to_lowercase()
}

/// Copies a kit's tile database entry (`<raw>/terrain/tiles/<kind>/_tile_database/TILES/<name>.xml`) into the raw_data `to_raw`, and the
/// kit's `_settings.xml` when that has none. A file it replaces gets a `.bak`.
///
/// # Errors
///
/// A file can't be read or written.
fn copy_entry(entry_xml: &Path, from_raw: &Path, to_raw: &Path, kind: &str) -> Result<PathBuf, String> {
    let database = |raw: &Path| raw.join("terrain/tiles").join(kind).join("_tile_database");
    let name = entry_xml.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let text = std::fs::read_to_string(entry_xml).map_err(|e| format!("{}: {e}", entry_xml.display()))?;

    let tiles = database(to_raw).join("TILES");
    std::fs::create_dir_all(&tiles).map_err(|e| format!("{}: {e}", tiles.display()))?;
    let target = tiles.join(&name);
    if target.exists() {
        if std::fs::read_to_string(&target).is_ok_and(|t| t == text) {
            return Ok(target);
        }

        let _ = std::fs::copy(&target, target.with_extension("xml.bak"));
    }

    std::fs::write(&target, text).map_err(|e| format!("{}: {e}", target.display()))?;
    let settings = database(to_raw).join("_settings.xml");
    let kit_settings = database(from_raw).join("_settings.xml");
    if !settings.exists() && kit_settings.is_file() {
        std::fs::copy(&kit_settings, &settings).map_err(|e| format!("{}: {e}", kit_settings.display()))?;
    }

    // The editor resolves tiles through the indices, which have another entry now.
    wce_formats::tile_index::TileIndex::forget_shared();
    Ok(target)
}

impl TileDocument {
    /// The projects folder in the kit's layout (`<projects>/assembly_kit`) the tile is saved into; none: saved where it is.
    pub fn set_projects_root(&mut self, root: Option<PathBuf>) {
        self.tile_mut().projects_root = root;
    }

    pub fn projects_root(&self) -> Option<&Path> {
        self.tile().projects_root.as_deref()
    }

    /// Whether the tile is a project (in the projects folder), not a kit's tile.
    pub fn is_project(&self) -> bool {
        self.projects_root().is_none_or(|root| self.project().dir.starts_with(root))
    }

    /// Where the tile's project is (or will be, at the first save): its folder in the projects, the kit's layout.
    pub fn project_folder(&self) -> Option<PathBuf> {
        let root = self.projects_root()?;
        if self.project().dir.starts_with(root) {
            return Some(self.project().dir.clone());
        }

        Some(root.join("raw_data").join(tile_folder(&self.project().dir)?))
    }

    /// Before the first save of a kit's tile: its database entry goes into the projects (the files go with the save), unless the project has
    /// it (another variation of the tile was saved before: its entry is the project's). Returns the project's folder.
    ///
    /// # Errors
    ///
    /// The tile is not in a kit's layout, or the entry can't be copied.
    pub(crate) fn make_project(&mut self) -> Result<PathBuf, String> {
        let root = self.projects_root().ok_or("no projects folder")?.to_path_buf();
        let target = self.project_folder().ok_or_else(|| format!("{} is not under terrain/tiles/", self.project().dir.display()))?;
        let folder = tile_folder(&target).unwrap_or_default();
        let to_raw = root.join("raw_data");
        let has_entry = self.project().entry_file_for(&target).is_some_and(|f| f.is_file());
        if let (Some(entry), Some(from_raw)) = (self.project().entry.as_ref(), self.project().raw_root.clone())
            && !has_entry
        {
            copy_entry(entry.file(), &from_raw, &to_raw, &kind_of(&folder))?;
        }

        Ok(target)
    }

    /// After the first save of a kit's tile: the document is the project's now.
    pub(crate) fn moved_to_project(&mut self, dir: PathBuf) {
        let raw = wce_tile::tile_project::kit_raw_root(&dir);
        let p = &mut self.tile_mut().project;
        // The entry is the project's from now on (the kit's numbers of the other variations may differ from the project's).
        let entry_file = p.entry_file_for(&dir);
        if let (Some(entry), Some(file)) = (p.entry.as_mut(), entry_file) {
            match wce_formats::tile_entry::TileEntry::load(&file) {
                Ok(own) => *entry = own,
                Err(_) => entry.set_file(&file),
            }
        }

        p.dir = dir;
        p.raw_root = raw;
    }

    /// "Save for Assembly Kit": the tile as TEd reads it in the kit `kit_raw` (a raw_data folder), at its folder, with its database entry.
    /// Files it replaces get a `.bak`. The procedural walls need `walls`: they are
    /// written as their pieces (`wce_walls::export::walls_for_kit`), and a tile without walls takes its old pieces out of the pack.
    ///
    /// # Errors
    ///
    /// The tile is not in a kit's layout, it has walls and no `walls` sources, a wall can't be made, or a file can't be written.
    pub fn save_for_kit(&mut self, kit_raw: &Path, walls: Option<WallSources>) -> Result<String, String> {
        self.end_stroke();
        let folder = tile_folder(&self.project().dir).ok_or_else(|| format!("{} is not under terrain/tiles/", self.project().dir.display()))?;
        let target = kit_raw.join(&folder);
        let text = self.tile().tscene().save();
        let (msg, report) = match walls {
            Some(w) if self.tile().has_walls() => {
                let (scene, n, notes) = wce_walls::export::walls_for_kit(self.project(), text, w.files, w.schema, &folder, w.data)?;
                for line in &notes {
                    wce_foundation::log::info(format!("Save for Assembly Kit: {line}"));
                }

                let polyfill = format!("{n} procedural wall(s) written as their pieces in the layer '{}' ({})", wce_walls::export::KIT_LAYER, wce_walls::pack::PROC_PACK);
                self.project().save_for_kit_scene(&target, scene, vec![polyfill])?
            }

            None if self.tile().has_walls() => {
                return Err(format!("the procedural walls need the game's files and rpfm's schema to be written for TEd (as their pieces, in {})", wce_walls::pack::PROC_PACK));
            }

            w => {
                // No walls (any more): the tile's old pieces leave the pack.
                if let Some(w) = w {
                    let prefix = wce_walls::export::key_prefix(&folder);
                    wce_walls::pack::replace_in_proc_pack(&w.data.join(wce_walls::pack::PROC_PACK), "attila", &prefix, vec![], vec![])?;
                }

                self.project().save_for_kit_scene(&target, text, vec![])?
            }
        };

        for line in report.polyfilled.iter().chain(&report.dropped) {
            wce_foundation::log::info(format!("Save for Assembly Kit: {line}"));
        }

        if let (Some(entry), Some(from_raw)) = (self.project().entry.as_ref(), self.project().raw_root.clone()) {
            copy_entry(entry.file(), &from_raw, kit_raw, &kind_of(&folder))?;
        }

        Ok(msg)
    }

    /// The procedural walls of the tile as text, each with the layer path it is in (to put back with [`Self::restore_walls`]).
    pub fn wall_texts(&self) -> Vec<(String, Vec<u8>)> {
        let sc = self.tile().tscene();
        wce_walls::scene::walls(sc).into_iter().map(|w| (sc.layer_path(w), sc.element_text(w))).collect()
    }

    /// Import from Assembly Kit of a tile whose project has walls: the layer of their pieces (`wce_walls::export::KIT_LAYER`) goes and the
    /// walls are put back, each in its layer (the scene graph when it is no longer there); one undo entry. Returns how many were put back.
    ///
    /// # Errors
    ///
    /// The tile has no scene graph, or the text can't be spliced.
    pub fn restore_walls(&mut self, walls: &[(String, Vec<u8>)]) -> Result<usize, String> {
        let sc = self.tile().tscene();
        let graph = sc.scene_graph().ok_or("this tile has no scene graph")?;
        let pieces = sc.children(graph).into_iter().filter(|&c| sc.is_layer(c) && sc.name(c) == wce_walls::export::KIT_LAYER).collect::<Vec<_>>();
        if walls.is_empty() && pieces.is_empty() {
            return Ok(0);
        }

        let mark = self.begin()?;
        for &l in pieces.iter().rev() {
            self.tscene_mut().remove(l)?;
        }

        for (layer, text) in walls {
            let into = self.tile().find_layer(layer).unwrap_or(graph);
            self.tscene_mut().insert_element_text(into, text)?;
        }

        self.push_splice("Restore the procedural walls".to_owned(), mark, vec![], vec![]);
        Ok(walls.len())
    }
}

/// What the procedural walls need to be written for TEd: the game's files (the base pieces, their rows), rpfm's schema, and the game's
/// data folder (where `wce_walls::pack::PROC_PACK` goes).
#[derive(Clone, Copy)]
pub struct WallSources<'a> {
    pub files: &'a dyn wce_vfs::files::Files,
    pub schema: &'a rpfm_lib::schema::Schema,
    pub data: &'a Path,
}

impl TileDocument {
    /// The manual trees the game will not have: those outside every vegetation exclusion polygon, which BOB leaves out
    /// (`wce_tile::vegetation::trees_outside_exclusions`).
    pub fn trees_outside_exclusions(&self) -> Vec<usize> {
        wce_tile::vegetation::trees_outside_exclusions(self.tile().tscene())
    }

    /// The warning about them, if there are any.
    pub fn exclusion_warning(&self) -> Option<String> {
        let n = self.trees_outside_exclusions().len();
        (n != 0).then(|| format!("{n} manual tree(s) are outside every vegetation exclusion polygon: the game leaves them out (Tile panel > Problems)"))
    }
}
