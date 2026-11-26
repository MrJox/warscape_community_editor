//! The image brush (TEd's ImageBrush, Load Brush): a picture's brightness as the height and paint brushes' weight
//! (`wce_tile::brush_image`), turned by an angle or at random at every step. Kept in the settings (`[tools.tile] brush_image`).

use std::path::Path;
use std::sync::Arc;

use wce_editor::editor::Editor;
use wce_tile::brush_image::BrushImage;

use wce_editor::gui;
use wce_foundation::math::Float3;

use crate::create_panel::{ALT_TURN_DEG_PER_PX, alt_scale_factor};
use crate::settings::{ImageBrush, SETTINGS_ID, TileSettings};
use crate::tool::{TileMode, TileTool};

/// A terrain brush being turned or resized with the mouse while Alt is held: where its outline stays, the mouse's x, and the turn and the
/// radius when that was taken; `resize`: Ctrl too.
#[derive(Clone, Copy, Debug)]
pub(crate) struct BrushAlt {
    pub at: Float3,
    pub mouse_x: f32,
    pub angle: f32,
    pub radius: f32,
    pub resize: bool,
}

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

    /// Where the brush's outline is drawn: where Alt went down while it is turned or resized, else at the cursor.
    pub(crate) fn brush_centre(&self) -> Float3 {
        self.brush_alt.map_or(self.cursor, |b| b.at)
    }

    /// Per frame with a terrain brush: Alt held keeps its outline where it was when Alt went down and turns the image brush with the mouse
    /// (left / right, 0.5 degree a pixel; Shift: by the rotate step), as Alt turns what is placed; with Ctrl too it grows (right) or shrinks
    /// (left) the radius (x2 per 200 pixels; Shift: by the gizmo's scale snap). Letting Alt go keeps the change (the image brush's turn
    /// into the settings).
    pub(crate) fn alt_adjust_brush(&mut self, e: &mut Editor) {
        let io = gui::io();
        let active = self.mode != TileMode::Select && io.KeyAlt && (e.viewport.hovered || self.brush_alt.is_some());
        if !active {
            if self.brush_alt.take().is_some()
                && let Some(mut b) = self.image_brush.clone()
                && b.angle != self.brush.image_angle
            {
                b.angle = self.brush.image_angle;
                let _ = self.set_image_brush(e, b);
            }

            return;
        }

        // Alt with the mouse is not a trip to the menu bar.
        gui::cancel_alt_menu_toggle();
        let (x, resize) = (io.MousePos.x, io.KeyCtrl);
        let turning = self.brush.image.is_some() && !self.image_brush.as_ref().is_some_and(|b| b.random_angle);
        let state = match self.brush_alt {
            // Ctrl pressed or let go while Alt is held: the other change starts from here.
            Some(b) if b.resize == resize => b,
            Some(b) => BrushAlt { mouse_x: x, angle: self.brush.image_angle, radius: self.brush.radius_m, resize, ..b },
            None if self.terrain_hit => {
                BrushAlt { at: self.cursor, mouse_x: x, angle: self.brush.image_angle, radius: self.brush.radius_m, resize }
            }

            None => return,
        };

        self.brush_alt = Some(state);
        let density = io.DisplayFramebufferScale.x.max(1e-3);
        let dx = (x - state.mouse_x) / density;
        if resize {
            let f = alt_scale_factor(dx, io.KeyShift.then_some(e.viewport.snap_scale));
            self.brush.radius_m = (state.radius * f).clamp(1.0, 120.0);
            self.status = format!("radius {:.1} m (Ctrl + Alt + mouse; Shift: steps of {})", self.brush.radius_m, e.viewport.snap_scale);
        } else if turning {
            let mut turned = state.angle + dx * ALT_TURN_DEG_PER_PX;
            if io.KeyShift && self.angle_step > 0.0 {
                turned = (turned / self.angle_step).round() * self.angle_step;
            }

            self.brush.image_angle = turned.rem_euclid(360.0);
            if let Some(b) = &mut self.image_brush {
                b.angle = self.brush.image_angle;
            }

            self.status = format!("turn {:.0} deg (Alt + mouse; Shift: steps of {} deg)", self.brush.image_angle, self.angle_step);
        } else {
            self.status = "Alt + mouse turns an image brush with a fixed turn; Ctrl + Alt + mouse resizes the brush".to_owned();
        }
    }

    /// A step of a stroke with the image brush: a new turn when it turns at random.
    pub(crate) fn turn_image_brush(&mut self) {
        if self.brush.image.is_some() && self.image_brush.as_ref().is_some_and(|b| b.random_angle) {
            self.brush.image_angle = self.random.unit() * 360.0;
        }
    }
}
