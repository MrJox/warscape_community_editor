//! The Tile panel's Stats (Phase 6b of docs/plans/tile_ted_parity.md): what the tile holds and costs, from what the editor knows. TEd has a
//! `stats_view` module name in its scripts but no stats view in its executable, and the game's per-tile budgets are not known: no limits
//! are shown, only the numbers (instances by type, building keys and missing models, the files on disk, what the last frame drew).

use wce_editor::editor::Editor;
use wce_editor::gui;
use wce_scene::render_scene::DocumentId;
use wce_tile_editor::tile_document::TileDocument;

use crate::tool::TileTool;

/// A byte count in kB, MB or GB.
fn size(b: u64) -> String {
    match b {
        b if b >= 1 << 30 => format!("{:.2} GB", b as f64 / f64::from(1u32 << 30)),
        b if b >= 1 << 20 => format!("{:.1} MB", b as f64 / f64::from(1u32 << 20)),
        b => format!("{:.0} kB", b as f64 / 1024.0),
    }
}

impl TileTool {
    pub(crate) fn draw_stats_section(&mut self, e: &mut Editor, id: DocumentId) {
        if !gui::collapsing_header("Stats", 0) {
            return;
        }

        let frame = e.renderer().map(|r| *r.draw_stats());
        let Some(d) = e.docs.find_as::<TileDocument>(id) else { return };
        let t = d.tile();
        let mut counts = t.type_counts().iter().map(|(k, &v)| (k.clone(), v)).collect::<Vec<_>>();
        counts.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
        let total = counts.iter().map(|c| i64::from(c.1)).sum::<i64>();
        gui::text(&format!("{total} instance(s); {} building key(s), {} without a model", t.models().len(), t.models().values().filter(|m| !m.ok).count()));
        if gui::begin_table("stats_types", 2, gui::TABLE_SIZING_STRETCH_PROP | gui::TABLE_ROW_BG, gui::v2(0.0, 0.0)) {
            for (k, v) in &counts {
                gui::table_next_row();
                gui::table_next_column();
                gui::text(crate::entity_types::type_name(k));
                if gui::is_item_hovered() {
                    gui::set_tooltip(k);
                }

                gui::table_next_column();
                gui::text(&v.to_string());
            }

            gui::end_table();
        }

        // The files on disk.
        let dir = &d.project().dir;
        let mut files = std::fs::read_dir(dir).map(|r| r.flatten().filter_map(|f| Some((f.file_name().to_string_lossy().into_owned(), f.metadata().ok()?.len()))).collect::<Vec<_>>()).unwrap_or_default();
        files.retain(|f| !f.0.ends_with(".bak"));
        files.sort_by_key(|f| std::cmp::Reverse(f.1));
        let disk = files.iter().map(|f| f.1).sum::<u64>();
        if gui::tree_node(&format!("files on disk: {} in {}###stats_files", size(disk), files.len())) {
            for (n, b) in &files {
                gui::text(&format!("{n}  {}", size(*b)));
            }

            gui::tree_pop();
        }

        if let Some(s) = frame {
            gui::text_disabled(&format!(
                "last frame: {} instances, {} triangles seen by the camera; {} draw calls, {} triangles with the shadows", s.camera_instances, s.camera_triangles,
                s.draw_calls, s.triangles
            ));
        }

        gui::text_disabled("The game's per-tile budgets are not known: no limits are shown.");
    }
}
