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

    wce_foundation::files::write_replacing(&target, text).map_err(|e| format!("{}: {e}", target.display()))?;
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

    /// The buildings' categories in the game's packs (`wce_tile_io::attila::game_data::building_categories_in_packs`): the last source
    /// of the table, after the kits.
    pub fn set_pack_categories(&mut self, categories: std::sync::Arc<std::collections::HashMap<String, String>>) {
        let tile = self.tile_mut();
        tile.pack_categories = Some(categories);
        tile.building_db = std::cell::OnceCell::new();
    }

    /// The Assembly Kit whose battle tiles' colours a tile made by Save as must differ from (`save_as_new_tile`).
    pub fn set_colour_kit(&mut self, kits: Vec<PathBuf>) {
        self.tile_mut().colour_kits = kits;
    }

    /// The tile set, name and variation a Save as into `target` makes, when it makes a new tile: a battle tile's variation folder in the
    /// projects (`<projects>/raw_data/terrain/tiles/battle/<set>/<name>/<variation>`, or a file in one) that is not the open tile's own, the
    /// open tile having a database entry to make the new one's from. Saved there as is, the folder would hold a tile no entry points to:
    /// it would open again without its height range, size and colour.
    pub fn save_as_new_tile(&self, target: &Path) -> Option<(String, String, String)> {
        let folder = if target.extension().is_some() { target.parent()? } else { target };
        let raw = self.projects_root()?.join("raw_data");
        let rel = folder.strip_prefix(&raw).ok()?.to_string_lossy().replace('\\', "/");
        let parts = rel.trim_matches('/').split('/').collect::<Vec<_>>();
        let [terrain, tiles, kind, set, name, variation] = parts[..] else { return None };
        if !terrain.eq_ignore_ascii_case("terrain") || !tiles.eq_ignore_ascii_case("tiles") || !kind.eq_ignore_ascii_case("battle") {
            return None;
        }

        let p = self.project();
        let entry = p.entry.as_ref()?;
        let own = format!("{}/{}", entry.location().replace('\\', "/"), entry.variations().get(p.variation?)?.location);
        if own.eq_ignore_ascii_case(rel.trim_matches('/')) {
            return None;
        }

        Some((set.to_owned(), name.to_owned(), variation.to_owned()))
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
    pub fn save_tile_as(&mut self, set: &str, name: &str, variation: &str, kits: &[PathBuf]) -> Result<String, String> {
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
                return Err(format!(
                    "{set}/{name} is a {} x {} tile: a {} x {} tile can't be saved as a variation of it",
                    there.width(),
                    there.height(),
                    entry.width(),
                    entry.height()
                ));
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
            let taken = colours_in(&colour_sources(&raw, kits));
            let c = colour_of(&format!("{set}/{name}/{variation}"), kits.first().map(PathBuf::as_path), &taken);
            for (attr, value) in ["red", "green", "blue"].into_iter().zip(c) {
                there.set_tile_attr(attr, &format!("{:.6}", f64::from(value)));
            }

            there.set_variations(&[(text, Some(variation.clone()))]).map_err(|e| e.to_string())?;
            (there, 0)
        };

        if let Some(dir) = entry_path.parent() {
            std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
        }

        wce_foundation::files::write_replacing(&entry_path, there.save())
            .map_err(|_| format!("could not write {}", entry_path.display()))?;
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
        let top = all
            .into_iter()
            .filter(|&e| {
                let mut p = sc.doc().elements()[e].parent;
                while let Some(q) = p {
                    if set.contains(&q) {
                        return false;
                    }

                    p = sc.doc().elements()[q].parent;
                }

                true
            })
            .collect::<Vec<_>>();

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
        wce_foundation::files::write_replacing(&file, there.save()).map_err(|_| format!("could not write {}", file.display()))?;
        wce_formats::tile_index::TileIndex::forget_shared();

        let msg = wce_tile::resize::resize_maps(&dir, old, density, r)?;
        Ok((dir, msg))
    }

    /// The scene as an export reads it: the editor's text with the buildings marked Export as prop as their models
    /// (`wce_tile::export_as_prop`), what that did in the log. `for_kit`: TEd's export comes next, and what it would lose is said too.
    ///
    /// # Errors
    ///
    /// The text does not read.
    pub fn text_for_export(&mut self, for_kit: bool) -> Result<Vec<u8>, String> {
        // The roads' pieces and bridges as they are now, not as a job left them.
        self.tile_mut().finish_road_jobs();
        let files = self.tile().files.as_deref();
        let models_in = |folder: &str| files.map_or_else(Vec::new, |f| f.list(folder, ".rigid_model_v2"));
        let (text, report) = wce_tile::export_as_prop::apply(
            self.tile().tscene().save(),
            files.is_some().then_some(&models_in as wce_tile::export_as_prop::ModelsIn),
        )?;
        for line in if for_kit { report.kit_notes() } else { report.notes() } {
            wce_foundation::log::info(line);
        }

        let text = self.tile().with_road_items(text)?;
        let text = self.tile().with_biome_items(text)?;
        let text = self.tile().with_decal_items(text)?;
        let text = self.tile().with_field_items(text)?;
        self.tile().with_building_spline_items(text)
    }

    /// "Save for Assembly Kit": the tile as TEd reads it in the kit `kit_raw` (a raw_data folder), at its folder, with its database entry.
    /// Files it replaces get a `.bak`. Buildings marked Export as prop go as their models (`text_for_export`). The procedural walls need `walls`: they are
    /// written as their pieces (`wce_walls::export::walls_for_kit`), and a tile without walls takes its old pieces out of the pack.
    ///
    /// # Errors
    ///
    /// The tile is not in a kit's layout, it has walls and no `walls` sources, a wall can't be made, or a file can't be written.
    pub fn save_for_kit(&mut self, kit_raw: &Path, walls: Option<WallSources>) -> Result<String, String> {
        if self.project().is_combined() {
            return self.save_variations_for_kit(kit_raw, walls, &[]);
        }

        self.end_stroke();
        let folder =
            tile_folder(&self.project().dir).ok_or_else(|| format!("{} is not under terrain/tiles/", self.project().dir.display()))?;
        let target = kit_raw.join(&folder);
        let text = self.text_for_export(true)?;
        let (msg, report) = match walls {
            Some(w) if self.tile().has_generated_buildings() => {
                let kit_walls = self.tile_mut().kit_walls_for_export();
                let (scene, n, notes) =
                    wce_walls::export::walls_for_kit(self.project(), text, w.files, w.schema, &folder, w.data, &kit_walls)?;
                for line in &notes {
                    wce_foundation::log::info(format!("Save for Assembly Kit: {line}"));
                }

                let polyfill = format!(
                    "{n} wall(s) written as their pieces in the layer '{}' ({})",
                    wce_walls::export::KIT_LAYER,
                    wce_walls::pack::PROC_PACK
                );
                let maps = self.tile_mut().project.export_maps()?;
                self.project().save_for_kit_scene(&target, scene, vec![polyfill], &maps)?
            }

            None if self.tile().has_generated_buildings() => {
                return Err(format!(
                    "the procedural walls need the game's files and rpfm's schema to be written for TEd (as their pieces, in {})",
                    wce_walls::pack::PROC_PACK
                ));
            }

            w => {
                // No walls (any more): the tile's old pieces leave the pack.
                if let Some(w) = w {
                    let prefix = wce_walls::export::key_prefix(&folder);
                    let game = w.files.game().map_or("attila", |g| g.key);
                    wce_walls::pack::replace_in_proc_pack(&w.data.join(wce_walls::pack::PROC_PACK), game, &prefix, vec![], vec![])?;
                }

                let maps = self.tile_mut().project.export_maps()?;
                self.project().save_for_kit_scene(&target, text, vec![], &maps)?
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
                    msg +=
                        &format!("; the kit's tile database takes the tile set(s) {} (_settings.xml, a .bak beside it)", added.join(", "));
                }
            }
        }

        Ok(msg)
    }

    /// Where a combined project's variations are written as projects of their own for the exports (`TileProject::materialize`):
    /// beside its kit layout, where the loaders write them too (`variations::materialize::build_root`: `<projects>/build/variations/
    /// raw_data`); without a kit layout, the temporary folder's.
    #[must_use]
    pub fn variations_build_root(&self) -> PathBuf {
        match &self.project().raw_root {
            Some(raw) => wce_tile::variations::materialize::build_root(raw),
            None => std::env::temp_dir().join("wce_variations").join("raw_data"),
        }
    }

    /// `f` of the tile with a combined project's variation shown alone (its scene, its terrain), the variations shown again after: what
    /// reads one variation's tile from the document (Launch in game's deployment zones).
    ///
    /// # Errors
    ///
    /// Not a variation of the project.
    pub fn with_variation_alone<R>(&mut self, variation: &str, f: impl FnOnce(&super::Tile) -> R) -> Result<R, String> {
        let shown = self.project().shown_variations().to_vec();
        if shown == [variation.to_owned()] {
            return Ok(f(self.tile()));
        }

        self.tile_mut().export_view(variation)?;
        let out = f(self.tile());
        self.tile_mut().end_export_view(&shown)?;
        Ok(out)
    }

    /// A combined project's variations (`list`, empty: all) written as projects of their own under [`Self::variations_build_root`], each
    /// from the scene as an export reads it ([`Self::text_for_export`]) with that variation viewed (`Tile::export_view`): what the editor
    /// makes over the terrain (the procedural roads' layers and pieces, the building splines' items, the kit walls' height layers and
    /// layouts) is made for each on its own ground; each with its kit walls as its export lowers them. The variation viewed is viewed
    /// again after.
    ///
    /// # Errors
    ///
    /// Not a combined project, a variation it does not have, or what `materialize` refuses.
    pub fn materialize_variations(&mut self, list: &[String], for_kit: bool) -> Result<Vec<VariationExport>, String> {
        self.materialize_variations_with(list, for_kit, &mut |_, _| Ok(()))
    }

    /// [`Self::materialize_variations`], `each` called with the tile and the folder of each variation once it is written, while that
    /// variation is viewed alone (what an export adds to it from the tile as it is: its radar).
    ///
    /// # Errors
    ///
    /// As `materialize_variations`, or what `each` returns.
    pub fn materialize_variations_with(
        &mut self,
        list: &[String],
        for_kit: bool,
        each: &mut dyn FnMut(&mut super::Tile, &Path) -> Result<(), String>,
    ) -> Result<Vec<VariationExport>, String> {
        if !self.project().is_combined() {
            return Err("the tile has one variation: it is not a combined project".to_owned());
        }

        self.end_stroke();
        let raw = self.variations_build_root();
        let shown = self.project().shown_variations().to_vec();
        let list = if list.is_empty() { self.project().variations.clone() } else { list.to_vec() };

        let mut made = vec![];
        let mut result = Ok(());
        let writing = wce_tile::variations::materialize::writing();
        for v in &list {
            let step = (|| {
                // Each alone (its roads, walls and terrain are its own), unless it is the only one shown already.
                if shown != [v.clone()] {
                    self.tile_mut().export_view(v)?;
                }

                let text = self.text_for_export(for_kit)?;
                // Its kit walls' height layers made on its ground (over its roads') before its maps are composed.
                let kit_walls = self.tile_mut().kit_walls_laid_for_export();
                let generated = self.tile().has_generated_buildings();
                let made = self.project().materialize(v, &text, &raw)?;
                each(self.tile_mut(), &made.dir)?;
                Ok::<_, String>(VariationExport { made, kit_walls, generated })
            })();
            match step {
                Ok(m) => made.push(m),
                Err(e) => {
                    result = Err(format!("{v}: {e}"));
                    break;
                }
            }
        }

        drop(writing);
        if self.project().shown_variations() != shown.as_slice() {
            self.tile_mut().end_export_view(&shown)?;
        }

        result?;
        for m in made.iter().map(|x| &x.made).filter(|m| m.refit) {
            wce_foundation::log::info(format!(
                "{}: the heights left the row's range; exported with min_height {:.3}, scale {:.3}",
                m.variation, m.range.0, m.range.1
            ));
        }

        Ok(made)
    }

    /// [`Self::save_for_kit`] of a combined project: each variation of `list` (empty: all) as TEd reads it, in its folder of the kit
    /// (each written as a project of its own first, `materialize_variations`), and the entry with every row.
    ///
    /// # Errors
    ///
    /// As `save_for_kit`.
    pub fn save_variations_for_kit(&mut self, kit_raw: &Path, walls: Option<WallSources>, list: &[String]) -> Result<String, String> {
        let made = self.materialize_variations(list, true)?;
        if walls.is_none() && made.iter().any(|x| x.generated) {
            return Err(format!(
                "the procedural walls need the game's files and rpfm's schema to be written for TEd (as their pieces, in {})",
                wce_walls::pack::PROC_PACK
            ));
        }

        let mut said = vec![];
        for VariationExport { made: m, kit_walls, generated } in &made {
            let generated = *generated;
            let folder = tile_folder(&m.dir).ok_or_else(|| format!("{} is not under terrain/tiles/", m.dir.display()))?;
            let target = kit_raw.join(&folder);
            let mut p = wce_tile::tile_project::TileProject::open(&m.dir)?;
            let scene = std::fs::read(m.dir.join(wce_tile::project_files::WCE_FILE)).map_err(|e| format!("{}: {e}", m.dir.display()))?;
            let maps = p.export_maps()?;
            let (msg, report) = match &walls {
                Some(w) if generated => {
                    let (scene, n, notes) = wce_walls::export::walls_for_kit(&p, scene, w.files, w.schema, &folder, w.data, kit_walls)?;
                    for line in &notes {
                        wce_foundation::log::info(format!("Save for Assembly Kit, {}: {line}", m.variation));
                    }

                    let polyfill = format!(
                        "{n} wall(s) written as their pieces in the layer '{}' ({})",
                        wce_walls::export::KIT_LAYER,
                        wce_walls::pack::PROC_PACK
                    );
                    p.save_for_kit_scene(&target, scene, vec![polyfill], &maps)?
                }

                w => {
                    if let Some(w) = w {
                        let prefix = wce_walls::export::key_prefix(&folder);
                        let game = w.files.game().map_or("attila", |g| g.key);
                        wce_walls::pack::replace_in_proc_pack(&w.data.join(wce_walls::pack::PROC_PACK), game, &prefix, vec![], vec![])?;
                    }

                    p.save_for_kit_scene(&target, scene, vec![], &maps)?
                }
            };

            for line in report.polyfilled.iter().chain(&report.dropped) {
                wce_foundation::log::info(format!("Save for Assembly Kit, {}: {line}", m.variation));
            }

            wce_foundation::log::info(format!("Save for Assembly Kit, {}: {msg}", m.variation));
            said.push(format!("{} ({})", m.variation, report.summary()));
        }

        let made = made.into_iter().map(|x| x.made).collect::<Vec<_>>();

        // The entry with each row's height range as its variation was written (`materialize`), and the tile sets it uses.
        let (Some(entry), Some(from_raw)) = (self.project().entry.as_ref(), self.project().raw_root.clone()) else {
            return Err("a combined project needs its tile database entry".to_owned());
        };

        let folder = tile_folder(&self.project().dir).unwrap_or_default();
        let kind = kind_of(&folder);
        let name = entry.file().file_name().map(|n| n.to_owned()).unwrap_or_default();
        let built = self.variations_build_root().join("terrain/tiles").join(&kind).join("_tile_database/TILES").join(name);
        copy_entry(&built, &from_raw, kit_raw, &kind)?;
        let mut msg = format!(
            "saved for the Assembly Kit: {} variation(s) of {folder} in {} ({}; see the log)",
            made.len(),
            kit_raw.display(),
            said.join(", ")
        );
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

        Ok(msg)
    }

    /// The walls of the tile (both kinds) and its building splines as text, each with the layer path it is in (to put back with
    /// [`Self::restore_walls`]).
    pub fn wall_texts(&self) -> Vec<(String, Vec<u8>)> {
        let sc = self.tile().tscene();
        let splines = (0..sc.doc().elements().len()).filter(|&e| sc.type_of(e) == super::BUILDING_SPLINE && sc.in_variation(e));
        wce_walls::scene::all_walls(sc).into_iter().chain(splines).map(|w| (sc.layer_path(w), sc.element_text(w))).collect()
    }

    /// Import from Assembly Kit of a tile whose project has walls or building splines: the layers of their pieces (`wce_walls::export::KIT_LAYER`,
    /// `building_splines::KIT_LAYER`) go and the
    /// walls are put back, each in its layer (the scene graph when it is no longer there); one undo entry. Returns how many were put back.
    ///
    /// # Errors
    ///
    /// The tile has no scene graph, or the text can't be spliced.
    pub fn restore_walls(&mut self, walls: &[(String, Vec<u8>)]) -> Result<usize, String> {
        let sc = self.tile().tscene();
        let graph = sc.scene_graph().ok_or("this tile has no scene graph")?;
        let pieces = sc
            .children(graph)
            .into_iter()
            .filter(|&c| {
                sc.is_layer(c) && [wce_walls::export::KIT_LAYER, super::building_splines::KIT_LAYER].iter().any(|n| sc.name(c) == *n)
            })
            .collect::<Vec<_>>();
        if walls.is_empty() && pieces.is_empty() {
            return Ok(0);
        }

        let mark = self.begin()?;
        for &l in pieces.iter().rev() {
            if let Err(e) = self.tscene_mut().remove(l) {
                return self.rolled_back(mark, e);
            }
        }

        for (layer, text) in walls {
            let into = self.tile().find_layer(layer).unwrap_or(graph);
            if let Err(e) = self.tscene_mut().insert_element_text(into, text) {
                // Taken back: the pieces too, not the walls lost.
                return self.rolled_back(mark, e);
            }
        }

        self.push_splice("Restore the procedural walls".to_owned(), mark, vec![], vec![]);
        Ok(walls.len())
    }
}

/// A variation of a combined project written for an export (`TileDocument::materialize_variations`): its folder, its kit walls as its
/// export lowers them, and whether its export makes buildings (walls, road bridges: `Tile::has_generated_buildings` while it was viewed).
pub struct VariationExport {
    pub made: wce_tile::variations::materialize::Materialized,
    pub kit_walls: Vec<wce_walls::export::KitWall>,
    pub generated: bool,
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
        (n != 0).then(|| {
            format!("{n} manual tree(s) are outside every vegetation exclusion polygon: the game leaves them out (Tile panel > Problems)")
        })
    }
}
