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

    /// The raw_data folders whose `battlefield_buildings` count after the tile's own kit's (the projects' kit layout, then the Assembly Kits in
    /// the settings' order): a project tile has no kit of its own.
    pub fn set_building_tables(&mut self, raw_dirs: Vec<PathBuf>) {
        let tile = self.tile_mut();
        tile.building_tables = raw_dirs;
        tile.building_db = std::cell::OnceCell::new();
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

    /// TEd's "Save tile as" (Phase 6b of docs/plans/tile_ted_parity.md): the tile as a battle tile of another tile set, name and variation,
    /// in the projects folder (`terrain/tiles/battle/<set>/<name>/<variation>`), and the document goes on from there. Its database entry
    /// (`TILES/<set>_<name>.xml`) is the open tile's with the new location, its tile set and a colour of its own (TEd's rule, `new_tile::
    /// colour_of`), and this variation alone, renamed; when that tile exists already (another variation was saved as it before), the
    /// variation is added to its entry and the rest of it kept (TEd: "save as a variation of existing tile and inherit its properties"),
    /// and a tile of another size is refused, as TEd does. `kit`: the Assembly Kit, whose tiles' colours the new one must differ from.
    ///
    /// # Errors
    ///
    /// No projects folder, a bad name, no database entry, a tile of another size or a variation that exists already, or a file can't be
    /// written.
    pub fn save_tile_as(&mut self, set: &str, name: &str, variation: &str, kit: Option<&Path>) -> Result<String, String> {
        use wce_formats::tile_entry::TileEntry;
        use wce_tile::new_tile::{check_name, colour_of, colour_sources, colours_in};

        let (set, name, variation) = (check_name(set)?, check_name(name)?, check_name(variation)?);
        let raw = self.projects_root().ok_or("no projects folder")?.join("raw_data");
        let folder = raw.join("terrain/tiles/battle").join(&set).join(&name).join(&variation);
        if wce_tile::tile_project::has_tile(&folder) && folder != self.project().dir {
            return Err(format!("{set}/{name}/{variation} exists already"));
        }

        let entry = self.project().entry.as_ref().ok_or("the tile has no database entry")?;
        let v = self.project().variation.unwrap_or(0);
        let text = entry.variation_text(v).ok_or("the tile's variation is not in its entry")?;
        let entry_path = raw.join("terrain/tiles/battle/_tile_database/TILES").join(format!("{set}_{name}.xml"));
        let (mut there, index) = if entry_path.is_file() {
            let mut there = TileEntry::load(&entry_path).map_err(|e| e.to_string())?;
            if (there.width(), there.height()) != (entry.width(), entry.height()) {
                return Err(format!("{set}/{name} is a {} x {} tile: a {} x {} tile can't be saved as a variation of it", there.width(), there.height(), entry.width(), entry.height()));
            }

            if there.find_variation(&variation).is_some() {
                return Err(format!("{set}/{name} has a variation {variation} already"));
            }

            let mut all = (0..there.variations().len()).filter_map(|i| there.variation_text(i)).map(|t| (t, None)).collect::<Vec<_>>();
            all.push((text, Some(variation.clone())));
            let n = all.len() - 1;
            there.set_variations(&all).map_err(|e| e.to_string())?;
            (there, n)
        } else {
            let mut there = entry.clone();
            there.set_file(&entry_path);
            there.set_tile_attr("location", &format!("terrain\\tiles\\battle\\{set}\\{name}"));
            there.set_tile_attr("tile_set", &set);
            let taken = colours_in(&colour_sources(&raw, kit));
            let c = colour_of(&format!("{set}/{name}/{variation}"), kit, &taken);
            for (attr, value) in ["red", "green", "blue"].into_iter().zip(c) {
                there.set_tile_attr(attr, &format!("{:.6}", f64::from(value)));
            }

            there.set_variations(&[(text, Some(variation.clone()))]).map_err(|e| e.to_string())?;
            (there, 0)
        };

        if let Some(dir) = entry_path.parent() {
            std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
        }

        std::fs::write(&entry_path, there.save()).map_err(|_| format!("could not write {}", entry_path.display()))?;
        there = TileEntry::load(&entry_path).map_err(|e| e.to_string())?;
        wce_formats::tile_index::TileIndex::forget_shared();

        // The document is the new tile's: its entry, variation and names; every file written there, then saved in place (the history's
        // saved mark).
        self.end_stroke();
        let p = &mut self.tile_mut().project;
        p.info.location = there.location();
        p.info.variation = variation.clone();
        p.entry = Some(there);
        p.variation = Some(index);
        p.raw_root = Some(raw);
        let msg = self.save_tile(&wce_editor_core::document::SaveTarget { path: folder.clone() })?;
        self.tile_mut().project.dir = folder;
        self.save_tile(&wce_editor_core::document::SaveTarget::default())?;
        Ok(format!("saved as {set}/{name}/{variation}: {msg}"))
    }

    /// TEd's Resize Tile (Phase 6b of docs/plans/tile_ted_parity.md): the scene moved by the resize's offset (whole cells; one undo entry,
    /// saved), the entry's size set, and the maps resized on disk (`wce_tile::resize::resize_maps`, `.resize.bak` beside each). The caller
    /// opens the tile again (the maps in memory are the old ones). Returns the tile's folder and what was done.
    ///
    /// # Errors
    ///
    /// No database entry, a size out of 1..64, or a file can't be read or written.
    pub fn resize_tile(&mut self, r: &wce_tile::resize::Resize) -> Result<(PathBuf, String), String> {
        use wce_tile::world_transform::{set_world_position, world_transform};

        if !(1..=64).contains(&r.cells.0) || !(1..=64).contains(&r.cells.1) {
            return Err("a tile is 1 to 64 cells a side".to_owned());
        }

        let entry = self.project().entry.as_ref().ok_or("the tile has no database entry")?;
        let old = (entry.width(), entry.height());
        let px = self.project().height.as_ref().map(|h| h.width()).ok_or("the tile has no height map")?;
        let density = px / (old.0 + 2);

        // The scene: every instance not inside another one, by the offset (rows of the maps go along +z).
        self.end_stroke();
        let t = self.tile();
        let u = t.unit();
        let delta = wce_foundation::math::Double3::new(f64::from(r.offset.0) * 128.0 * u, 0.0, f64::from(r.offset.1) * 128.0 * u);
        let all = t.instances.clone();
        let set = all.iter().copied().collect::<std::collections::HashSet<_>>();
        let sc = t.tscene();
        let top = all.into_iter().filter(|&e| {
            let mut p = sc.doc().elements()[e].parent;
            while let Some(q) = p {
                if set.contains(&q) {
                    return false;
                }

                p = sc.doc().elements()[q].parent;
            }

            true
        }).collect::<Vec<_>>();

        if delta != wce_foundation::math::Double3::ZERO && !top.is_empty() {
            let mark = self.begin()?;
            for &e in &top {
                let before = self.tile().tscene().position(e);
                let w = world_transform(self.tile().tscene(), e);
                set_world_position(self.tscene_mut(), e, w.position + delta);
                self.carry_points(e, before);
            }

            self.push_splice("Resize tile: move the scene".to_owned(), mark, vec![], vec![]);
        }

        self.save_tile(&wce_editor_core::document::SaveTarget::default())?;
        let dir = self.project().dir.clone();

        // The entry's size, in the project's entry file and in memory.
        let file = self.project().entry_file_for(&dir).ok_or("the tile's entry is not beside it")?;
        let mut there = wce_formats::tile_entry::TileEntry::load(&file).map_err(|e| e.to_string())?;
        there.set_tile_attr("width", &r.cells.0.to_string());
        there.set_tile_attr("height", &r.cells.1.to_string());
        std::fs::write(&file, there.save()).map_err(|_| format!("could not write {}", file.display()))?;
        wce_formats::tile_index::TileIndex::forget_shared();

        let msg = wce_tile::resize::resize_maps(&dir, old, density, r)?;
        Ok((dir, msg))
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
                    let game = w.files.game().map_or("attila", |g| g.key);
                    wce_walls::pack::replace_in_proc_pack(&w.data.join(wce_walls::pack::PROC_PACK), game, &prefix, vec![], vec![])?;
                }

                self.project().save_for_kit_scene(&target, text, vec![])?
            }
        };

        for line in report.polyfilled.iter().chain(&report.dropped) {
            wce_foundation::log::info(format!("Save for Assembly Kit: {line}"));
        }

        let mut msg = msg;
        if let (Some(entry), Some(from_raw)) = (self.project().entry.as_ref(), self.project().raw_root.clone()) {
            copy_entry(entry.file(), &from_raw, kit_raw, &kind_of(&folder))?;

            // The tile sets it uses (its own, its links') that the kit's database lacks (made in the projects: Tile database > New tile set).
            let mut names = vec![entry.tile_set()];
            names.extend(entry.link_targets().into_iter().map(|t| t.tile_set));
            names.extend(entry.links().into_iter().map(|l| l.link_set));
            names.sort();
            names.dedup();
            let (ours, kits) = (wce_tile::tile_sets::settings_file(&from_raw), wce_tile::tile_sets::settings_file(kit_raw));
            if ours != kits && ours.is_file() && kits.is_file() {
                let added = wce_tile::tile_sets::merge_into(&ours, &kits, &names)?;
                if !added.is_empty() {
                    msg += &format!("; the kit's tile database takes the tile set(s) {} (_settings.xml, a .bak beside it)", added.join(", "));
                }
            }
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
