//! Camera bookmarks (Phase 5g of docs/plans/tile_ted_parity.md; wce's): views of a tile saved by name and gone back to from the toolbar's
//! popup. Kept in the settings by the tile's folder (`[tools.tile] camera_views`), not in the tile.

use wce_editor::editor::Editor;
use wce_editor::{gui, icons};
use wce_editor_core::document::Document;
use wce_foundation::math::Float3;
use wce_scene::camera::Camera;

use crate::settings::{CameraView, SETTINGS_ID, TileSettings};
use crate::tool::{TileTool, document};

impl TileTool {
    /// The open tile's key in the settings and its saved views.
    pub(crate) fn camera_views(&self, e: &mut Editor) -> Option<(String, Vec<CameraView>)> {
        let key = document(e)?.source_key();
        let views = e.tool_settings::<TileSettings>(SETTINGS_ID).camera_views.get(&key).cloned().unwrap_or_default();
        Some((key, views))
    }

    /// Saves the camera as a view of the open tile (a view of that name is replaced); its index.
    pub(crate) fn save_camera_view(&mut self, e: &mut Editor, name: &str) -> Option<usize> {
        let (key, mut views) = self.camera_views(e)?;
        let c = e.viewport.camera();
        let view = CameraView { name: name.to_owned(), eye: [c.eye.x, c.eye.y, c.eye.z], target: [c.target.x, c.target.y, c.target.z] };
        let at = match views.iter().position(|v| v.name == name) {
            Some(i) => {
                views[i] = view;
                i
            }

            None => {
                views.push(view);
                views.len() - 1
            }
        };

        e.update_tool_settings(SETTINGS_ID, |s: &mut TileSettings| {
            s.camera_views.insert(key, views);
        });
        Some(at)
    }

    /// Puts the camera on a saved view of the open tile, by its name; false when there is none.
    pub(crate) fn go_to_camera_view(&mut self, e: &mut Editor, name: &str) -> bool {
        let Some(v) = self.camera_views(e).and_then(|(_, views)| views.into_iter().find(|v| v.name == name)) else { return false };
        let cam = e.viewport.controller().handoff();
        let cam = Camera { eye: Float3::new(v.eye[0], v.eye[1], v.eye[2]), target: Float3::new(v.target[0], v.target[1], v.target[2]), ..cam };
        e.viewport.controller().set_camera(&cam);
        true
    }

    fn delete_camera_view(&mut self, e: &mut Editor, name: &str) {
        let Some((key, mut views)) = self.camera_views(e) else { return };
        views.retain(|v| v.name != name);
        e.update_tool_settings(SETTINGS_ID, |s: &mut TileSettings| {
            if views.is_empty() {
                s.camera_views.remove(&key);
            } else {
                s.camera_views.insert(key, views);
            }
        });
    }

    /// The toolbar's button and its popup: the saved views (a click goes there, x forgets it) and the current view saved by a name.
    pub(crate) fn draw_camera_views_button(&mut self, e: &mut Editor) {
        if gui::small_button(&format!("{}##views", icons::CAMERA)) {
            gui::open_popup("tile_views");
        }

        if gui::is_item_hovered() {
            gui::set_tooltip("Camera views: the views of this tile you saved; a click goes back to one");
        }

        if !gui::begin_popup("tile_views") {
            return;
        }

        let views = self.camera_views(e).map(|(_, v)| v).unwrap_or_default();
        if views.is_empty() {
            gui::text_disabled("No saved views for this tile.");
        }

        let (mut go, mut forget) = (None, None);
        for (i, v) in views.iter().enumerate() {
            if gui::small_button(&format!("{}##forget{i}", icons::X)) {
                forget = Some(v.name.clone());
            }

            gui::same_line();
            if gui::selectable(&format!("{}##view{i}", v.name), false, 0) {
                go = Some(v.name.clone());
            }
        }

        gui::separator();
        gui::set_next_item_width(160.0);
        let enter = gui::input_text("##viewname", Some("name"), &mut self.view_name, 64, gui::INPUT_ENTER_RETURNS_TRUE);
        gui::same_line();
        if (gui::small_button("Save the current view") || enter) && !self.view_name.trim().is_empty() {
            let name = self.view_name.trim().to_owned();
            self.save_camera_view(e, &name);
            self.view_name.clear();
        }

        gui::end_popup();
        if let Some(n) = go {
            self.go_to_camera_view(e, &n);
        }

        if let Some(n) = forget {
            self.delete_camera_view(e, &n);
        }
    }
}
