//! Saving the tile.

use super::*;

impl TileDocument {
    // ---- save

    /// Saves tile.wce and the height map (in place, or into the folder of `target`); the message says what was written. A tile opened
    /// from a kit is saved into its project (kit.rs), which the document is from then on.
    pub(crate) fn save_tile(&mut self, target: &SaveTarget) -> Result<String, String> {
        self.end_stroke();
        let mut elsewhere = !target.path.as_os_str().is_empty();
        let mut into_project = None;
        if !elsewhere && !self.is_project() {
            into_project = Some(self.make_project()?);
            elsewhere = true;
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

        let msg = if wrote { format!("{msg}({})", out.display()) } else { msg };

        // What the game will not have, said at every save.
        Ok(match self.exclusion_warning() {
            Some(w) => {
                wce_foundation::log::warn(format!("{}: {w}", self.title()));
                format!("WARNING: {w}. {msg}")
            }

            None => msg,
        })
    }
}
