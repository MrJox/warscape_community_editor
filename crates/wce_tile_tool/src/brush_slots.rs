//! Each terrain brush's own settings: Raise, Lower, Smooth, Paint and the rest each keep the radius, strength, falloff, curve, image and
//! the rest they were last used with, instead of sharing one brush. Kept in the settings (`[tools.tile] brushes`, by the mode's name);
//! scripted (fixed-step) runs neither read nor write them, so that a script's brushes start the same on every machine.

use std::path::Path;
use std::sync::Arc;

use wce_editor::editor::Editor;
use wce_editor::gui;
use wce_tile::brush_image::BrushImage;
use wce_tile::falloff::FalloffCurve;
use wce_tile_editor::tile_document::BrushSettings;

use crate::settings::{BrushCurve, ImageBrush, SETTINGS_ID, SavedBrush, TileSettings};
use crate::tool::{MODE_NAMES, TileMode, TileTool};

/// A brush's settings while the editor runs: the brush as a stroke takes it (its curve's table and its picture read), and the curve and
/// image settings it was made from (`None`: not read yet, the shared settings').
#[derive(Clone, Debug)]
pub(crate) struct BrushSlot {
    pub brush: BrushSettings,
    pub curve: Option<BrushCurve>,
    pub image: Option<ImageBrush>,
}

impl SavedBrush {
    fn of(b: &BrushSettings, curve: &BrushCurve, image: &ImageBrush) -> Self {
        Self {
            radius_m: b.radius_m,
            strength: b.strength,
            falloff: b.falloff,
            flatten_height: b.flatten_height,
            opacity: b.opacity,
            smooth_strength: b.smooth_strength,
            noise_scale_m: b.noise_scale_m,
            stamp_height_m: b.stamp_height_m,
            curve: curve.clone(),
            image: image.clone(),
        }
    }

    fn slot(&self) -> BrushSlot {
        let brush = BrushSettings {
            radius_m: self.radius_m,
            strength: self.strength,
            falloff: self.falloff,
            flatten_height: self.flatten_height,
            opacity: self.opacity,
            smooth_strength: self.smooth_strength,
            noise_scale_m: self.noise_scale_m,
            stamp_height_m: self.stamp_height_m,
            image_angle: self.image.angle,
            ..BrushSettings::default()
        };
        BrushSlot { brush, curve: Some(self.curve.clone()), image: Some(self.image.clone()) }
    }
}

fn name(m: TileMode) -> &'static str {
    MODE_NAMES[m.index()]
}

impl TileTool {
    /// Brings in the brush of mode `m` (a brush mode; Select keeps the last brush, which the commands then use), keeping the one in use for
    /// its mode first. A brush used for the first time starts as the one before it.
    pub(crate) fn switch_brush(&mut self, m: TileMode) {
        if m == TileMode::Select || m == self.brush_mode {
            return;
        }

        if self.brush_mode != TileMode::Select {
            let slot = BrushSlot { brush: self.brush.clone(), curve: self.curve.clone(), image: self.image_brush.clone() };
            self.brush_slots.insert(name(self.brush_mode), slot);
        }

        self.brush_mode = m;
        let Some(slot) = self.brush_slots.get(name(m)).cloned() else { return };

        // The settings a stroke reads that the slot does not hold (the paint brush's mode and filters are the Paint brush's alone).
        let (paint_mode, filter) = (self.brush.paint_mode, self.brush.filter);
        self.brush = BrushSettings { paint_mode, filter, ..slot.brush };
        if let Some(c) = &slot.curve {
            self.brush.curve = c.on.then(|| FalloffCurve::from_points(&c.points).lut());
            self.curve = Some(c.clone());
        }

        if let Some(i) = &slot.image {
            // A saved brush's picture is read when it is first brought in.
            let wanted = i.on && !i.path.is_empty();
            if !wanted {
                self.brush.image = None;
            } else if self.brush.image.as_ref().is_none_or(|have| have.path != i.path) {
                self.brush.image = BrushImage::read(Path::new(&i.path)).ok().map(Arc::new);
            }

            self.brush.image_angle = i.angle;
            self.image_brush = Some(i.clone());
        }
    }

    /// Per frame: the saved brushes read once, the brush of a mode set without `set_mode` brought in, and the brush in use saved when it
    /// changed and nothing is being dragged (a slider, the falloff curve, a stroke).
    pub(crate) fn sync_brush_slots(&mut self, e: &mut Editor) {
        if e.scripted_input() {
            self.switch_brush(self.mode);
            return;
        }

        if !self.brush_slots_read {
            self.brush_slots_read = true;
            let saved = e.tool_settings::<TileSettings>(SETTINGS_ID).brushes;
            for (mode, b) in &saved {
                if let Some(&mode) = MODE_NAMES.iter().find(|&&n| n == mode) {
                    self.brush_slots.entry(mode).or_insert_with(|| b.slot());
                }
            }

            self.brushes_saved = saved;
            // A brush mode set before they were read: its saved brush now.
            let m = std::mem::replace(&mut self.brush_mode, TileMode::Select);
            self.switch_brush(m);
        }

        self.switch_brush(self.mode);
        let busy = self.falloff_unsaved || self.brush_alt.is_some() || gui::is_any_item_active() || gui::is_mouse_down(0);
        if self.brush_mode == TileMode::Select || busy {
            return;
        }

        let (curve, image) = (self.brush_curve(e), self.image_brush(e));
        let now = SavedBrush::of(&self.brush, &curve, &image);
        let key = name(self.brush_mode);
        if self.brushes_saved.get(key) != Some(&now) {
            self.brushes_saved.insert(key.to_owned(), now.clone());
            e.update_tool_settings(SETTINGS_ID, |s: &mut TileSettings| {
                s.brushes.insert(key.to_owned(), now);
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_brush_keeps_its_own_settings() {
        let mut t = TileTool::default();
        t.set_mode(TileMode::Raise);
        t.brush.radius_m = 30.0;
        t.brush.strength = 12.0;

        // A brush used for the first time starts as the one before it, then goes its own way.
        t.set_mode(TileMode::Smooth);
        assert_eq!(t.brush.radius_m, 30.0);
        t.brush.radius_m = 50.0;
        t.brush.smooth_strength = 0.9;

        t.set_mode(TileMode::Raise);
        assert_eq!((t.brush.radius_m, t.brush.strength), (30.0, 12.0));

        // Select keeps the last brush (the commands use it), and the same brush again changes nothing.
        t.set_mode(TileMode::Select);
        t.brush.radius_m = 31.0;
        t.set_mode(TileMode::Raise);
        assert_eq!(t.brush.radius_m, 31.0);

        t.set_mode(TileMode::Smooth);
        assert_eq!((t.brush.radius_m, t.brush.smooth_strength), (50.0, 0.9));
    }

    #[test]
    fn a_saved_brush_comes_back_with_its_curve_and_the_paint_mode_stays() {
        let mut t = TileTool::default();
        let curve = BrushCurve { on: true, points: vec![[0.0, 1.0], [1.0, 0.0]] };
        let saved = SavedBrush { radius_m: 77.0, curve: curve.clone(), ..SavedBrush::default() };
        t.brush_slots.insert("flatten", saved.slot());
        t.brush.paint_mode = wce_tile::blend_field::PaintMode::Rock;
        t.set_mode(TileMode::Flatten);
        assert_eq!(t.brush.radius_m, 77.0);
        assert!(t.brush.curve.is_some());
        assert_eq!(t.curve, Some(curve));
        assert_eq!(t.brush.paint_mode, wce_tile::blend_field::PaintMode::Rock);
    }
}
