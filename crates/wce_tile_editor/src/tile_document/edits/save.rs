//! Saving the tile.

use std::path::{Path, PathBuf};

use super::*;
use crate::tile_document::kit;

impl TileDocument {
    // ---- save

    /// Saves tile.wce and the height map (in place, or into the folder of `target`); the message says what was written. A tile opened
    /// from a kit is saved into its project (kit.rs), which the document is from then on.
    pub(crate) fn save_tile(&mut self, target: &SaveTarget) -> Result<String, String> {
        let msg = self.write_tile(target)?;

        // What the game will not have, said at every save.
        Ok(match self.exclusion_warning() {
            Some(w) => {
                wce_foundation::log::warn(format!("{}: {w}", self.title()));
                format!("WARNING: {w}. {msg}")
            }

            None => msg,
        })
    }

    /// `save_tile`'s writing: the files, in place or into `target`'s folder; the message says what was written.
    fn write_tile(&mut self, target: &SaveTarget) -> Result<String, String> {
        self.end_stroke();
        let mut elsewhere = !target.path.as_os_str().is_empty();
        let mut into_project = None;
        if !elsewhere {
            // Restored from an autosave copy: every file into its target, as a kit tile's first save goes into its project.
            if let Some(to) = self.tile().restored_to.clone() {
                self.restored_entry(&to)?;
                into_project = Some(to);
                elsewhere = true;
            } else if !self.is_project() {
                into_project = Some(self.make_project()?);
                elsewhere = true;
            }
        }

        let t = self.tile();
        let mut out = into_project.clone().unwrap_or_else(|| if elsewhere { target.path.clone() } else { t.project.dir.clone() });

        // "Save as..." hands over a file name: the tile goes into its folder.
        if into_project.is_none() && elsewhere && out.extension().is_some() {
            out = out.parent().map(|p| p.to_path_buf()).unwrap_or_default();
        }

        // Or marked by what wrote to the maps outside the history (the roads' terrain, `take_road_terrain`; user 2026-11-08: recomputed
        // here from the history alone, the mark was lost and the height map never saved after the roads were made again).
        let height_dirty = t.project.height_dirty || t.height_modified() || t.png_on_disk_edited || (elsewhere && t.project.has_height());
        // A splice of attribute edits alone is baked into the scene's text without counting as a modification of it: the history says.
        let force_scene = t.save_scene_needed() || elsewhere || self.history.modified();
        let blend_dirty = t.project.blend_dirty || t.blend_modified() || t.blend_on_disk_edited;
        let ground_dirty = t.project.ground_dirty || t.ground_modified() || t.ground_on_disk_edited;
        let t = self.tile_mut();
        t.project.height_dirty = height_dirty;
        t.project.blend_dirty = blend_dirty;
        t.project.ground_dirty = ground_dirty;
        let (wrote, msg) = t.project.save(&out, force_scene, false)?;
        if let Some(dir) = into_project {
            self.moved_to_project(dir);
            self.tile_mut().restored_to = None;
            elsewhere = false;
        }

        let t = self.tile_mut();
        if !elsewhere {
            t.ted_on_disk_edited = t.tscene().modified();
            t.png_on_disk_edited = t.height_modified();
            t.saved_height_top = t.project.height.as_ref().map_or(0, |h| h.top_id());
            t.project.height_dirty = false;
            t.blend_on_disk_edited = t.blend_modified();
            t.saved_blend_top = t.project.blend.as_ref().map_or(0, |b| b.top_id());
            t.project.blend_dirty = false;
            t.ground_on_disk_edited = t.ground_modified();
            t.saved_ground_top = t.project.ground.as_ref().map_or(0, |g| g.top_id());
            t.project.ground_dirty = false;
            t.project.river_texts = t.project.current_river_texts();
            self.history.mark_saved();
        }

        Ok(if wrote { format!("{msg}({})", out.display()) } else { msg })
    }

    // ---- autosave (docs/plans/autosave.md)

    /// Where Save writes the tile: its folder; a kit tile's project (its first save makes it); a restored tile's target.
    pub fn save_target(&self) -> PathBuf {
        if let Some(to) = &self.tile().restored_to {
            return to.clone();
        }

        if self.is_project() { self.project().dir.clone() } else { self.project_folder().unwrap_or_else(|| self.project().dir.clone()) }
    }

    /// The tile's folder in an autosave copy: in the kit's layout (`raw_data/terrain/tiles/...`, its entry under `_tile_database` beside
    /// it), so that the copy opens with its entry; `tile` for a tile outside a kit or without an entry.
    pub(crate) fn autosave_inner(&self) -> String {
        let p = self.project();
        match (kit::tile_folder(&p.dir), p.entry.as_ref().and_then(|e| e.raw_root())) {
            (Some(folder), Some(_)) => format!("raw_data/{folder}"),
            _ => "tile".to_owned(),
        }
    }

    /// Writes an autosave copy into `dir`: the tile's files as a Save into another folder writes them, and its entry as it is now (with
    /// the kit's tile database settings), in the kit's layout (`autosave_inner`). The document is left as it was: nothing marked saved.
    ///
    /// # Errors
    ///
    /// A file can't be written.
    pub(crate) fn autosave_to(&mut self, dir: &Path) -> Result<(), String> {
        let inner = self.autosave_inner();
        if inner.starts_with("raw_data/")
            && let Some(entry) = self.project().entry.as_ref()
            && let Some(root) = entry.raw_root()
            && let Ok(rel) = entry.file().strip_prefix(&root)
        {
            let to = dir.join("raw_data").join(rel);
            if let Some(parent) = to.parent() {
                std::fs::create_dir_all(parent).map_err(|e| format!("{}: {e}", parent.display()))?;
            }

            std::fs::write(&to, entry.save()).map_err(|e| format!("{}: {e}", to.display()))?;

            // `_tile_database/_settings.xml`, two folders above the entry (`TILES/<name>.xml`).
            if let Some(database) = rel.parent().and_then(Path::parent) {
                let settings = root.join(database).join("_settings.xml");
                if settings.is_file() {
                    let _ = std::fs::copy(&settings, dir.join("raw_data").join(database).join("_settings.xml"));
                }
            }
        }

        // A save into another folder writes every file. What it notes on the way stays as it was, for the real Save: the maps' and the
        // terrain layers' unsaved marks, the entry and the range the heights are stored with (fitted into the copy's entry).
        let p = self.project();
        let marks = (p.height_dirty, p.blend_dirty, p.ground_dirty, p.layers.as_ref().map(|l| l.dirty));
        let (entry, info, notes) = (p.entry.clone(), p.info.clone(), p.prefab_notes.clone());
        let range = p.height.as_ref().map(|h| (h.min_height(), h.scale()));
        let written = self.write_tile(&SaveTarget { path: dir.join(&inner) });
        let p = &mut self.tile_mut().project;
        (p.height_dirty, p.blend_dirty, p.ground_dirty) = (marks.0, marks.1, marks.2);
        if let (Some(l), Some(dirty)) = (p.layers.as_mut(), marks.3) {
            l.dirty = dirty;
        }

        (p.entry, p.info, p.prefab_notes) = (entry, info, notes);
        if let (Some(h), Some((min, scale))) = (p.height.as_mut(), range) {
            h.set_range(min, scale);
        }

        wce_formats::tile_index::TileIndex::forget_shared();
        written.map(|_| ())
    }

    /// The document was opened from an autosave copy: from now on it saves into `to` (the tile's own folder, or the project a kit
    /// tile's first save makes), and is modified until then.
    pub fn set_restored(&mut self, to: PathBuf) {
        self.tile_mut().restored_to = Some(to);
    }

    /// Before a restored tile's Save: its entry into the target's kit layout when that has none yet (as a kit tile's first save copies
    /// the kit's).
    fn restored_entry(&self, to: &Path) -> Result<(), String> {
        if let (Some(entry), Some(file)) = (self.project().entry.as_ref(), self.project().entry_file_for(to))
            && !file.is_file()
        {
            if let Some(parent) = file.parent() {
                std::fs::create_dir_all(parent).map_err(|e| format!("{}: {e}", parent.display()))?;
            }

            wce_foundation::files::write_replacing(&file, entry.save()).map_err(|e| format!("{}: {e}", file.display()))?;

            // The tile database's settings with it, when that has none (`kit::copy_entry`).
            let database = |f: &Path| f.parent().and_then(Path::parent).map(|d| d.join("_settings.xml"));
            if let (Some(from), Some(to)) = (database(entry.file()), database(&file))
                && from.is_file()
                && !to.exists()
            {
                std::fs::copy(&from, &to).map_err(|e| format!("{}: {e}", from.display()))?;
            }

            wce_formats::tile_index::TileIndex::forget_shared();
        }

        Ok(())
    }
}
