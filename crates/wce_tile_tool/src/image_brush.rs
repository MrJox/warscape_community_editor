//! The image brush (TEd's ImageBrush, Load Brush): a picture's brightness as the height and paint brushes' weight
//! (`wce_tile::brush_image`), turned by an angle or at random at every step. Kept in the settings (`[tools.tile] brush_image`).

use std::path::Path;
use std::sync::Arc;

use wce_editor::editor::Editor;
use wce_tile::brush_image::BrushImage;

use crate::settings::{ImageBrush, SETTINGS_ID, TileSettings};
use crate::tool::TileTool;

impl TileTool {
    /// The image brush setting, read from the settings the first time.
    pub(crate) fn image_brush(&mut self, e: &Editor) -> ImageBrush {
        self.image_brush.get_or_insert_with(|| e.tool_settings::<TileSettings>(SETTINGS_ID).brush_image).clone()
    }

    /// A new image brush setting: its picture read (when it is another) and put into the brush, and the setting kept. The message says
    /// what is wrong with the picture.
    pub(crate) fn set_image_brush(&mut self, e: &mut Editor, b: ImageBrush) -> Result<(), String> {
        let mut result = Ok(());
        self.brush.image = if !b.on || b.path.is_empty() {
            None
        } else if let Some(same) = self.brush.image.clone().filter(|i| i.path == b.path) {
            Some(same)
        } else {
            BrushImage::read(Path::new(&b.path)).map(Arc::new).map_err(|why| result = Err(why)).ok()
        };

        self.brush.image_angle = b.angle;
        self.image_brush = Some(b.clone());
        e.update_tool_settings(SETTINGS_ID, |s: &mut TileSettings| s.brush_image = b);
        result
    }

    /// Puts the saved image brush into the brush once.
    pub(crate) fn sync_image_brush(&mut self, e: &Editor) {
        if self.image_brush.is_none() {
            let b = self.image_brush(e);
            self.brush.image_angle = b.angle;
            if b.on && !b.path.is_empty() {
                self.brush.image = BrushImage::read(Path::new(&b.path)).ok().map(Arc::new);
            }
        }
    }

    /// A step of a stroke with the image brush: a new turn when it turns at random.
    pub(crate) fn turn_image_brush(&mut self) {
        if self.brush.image.is_some() && self.image_brush.as_ref().is_some_and(|b| b.random_angle) {
            self.brush.image_angle = self.random.unit() * 360.0;
        }
    }
}
