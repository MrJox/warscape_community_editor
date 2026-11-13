//! LF edits of a map document: a stroke's undo record as a command, and the mirror that copies the model's samples into the view's LF.
//!
//! The model's field is the file's (`lf_heights.tif`: row 0 the map's top); the view's LF (`LfMap`, the map's own frame) holds the same
//! samples on the same lattice with the rows the other way up (its row j is the field's row h - 1 - j). An edit goes to the model and is
//! copied there, which moves the samples' revision: the renderer updates the edited texels, the views' picking its pyramid.

use std::any::Any;
use std::sync::Arc;

use wce_editor_core::command::Command;
use wce_scene::render_scene::LfSamples;
use wce_terrain::lf_edit::LfRecord;
use wce_terrain::paged::PagedU16;
use wce_terrain::{LfRole, MapModel};

/// The view's samples of a field: `w` x `h`, rows the other way up.
#[derive(Debug)]
pub struct LfMirror {
    pub samples: Arc<LfSamples>,
    pub w: u32,
    pub h: u32,

    /// The field's samples copied since the last `take_touched` (x0, y0, x1, y1; end exclusive): the views follow them (the picking, the
    /// objects standing on the LF), undo and redo included.
    touched: std::sync::Mutex<Option<[u32; 4]>>,
}

impl LfMirror {
    /// A mirror of `field` in `samples` when they are the same lattice (the map's own frame), none otherwise.
    pub fn of(field: &PagedU16, samples: Arc<LfSamples>, nx: i32, nz: i32) -> Option<Self> {
        (u32::try_from(nx).ok() == Some(field.width()) && u32::try_from(nz).ok() == Some(field.height())).then(|| Self {
            samples,
            w: field.width(),
            h: field.height(),
            touched: std::sync::Mutex::default(),
        })
    }

    /// The field's samples in `rect` (x0, y0, x1, y1; end exclusive) into the view's.
    pub fn copy(&self, field: &PagedU16, rect: [u32; 4]) {
        let [x0, y0, x1, y1] = rect;
        let (w, h) = (self.w, self.h);
        let (j0, j1) = (h - y1, h - y0);
        self.samples.edit([x0 as i32, j0 as i32, x1 as i32, j1 as i32], |v| {
            for y in y0..y1 {
                let row = ((h - 1 - y) * w) as usize;
                for x in x0..x1 {
                    v[row + x as usize] = field.at(x, y);
                }
            }
        });

        let mut t = self.touched.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        *t = Some(t.map_or(rect, |o| [o[0].min(x0), o[1].min(y0), o[2].max(x1), o[3].max(y1)]));
    }

    /// The samples copied since the last call (x0, y0, x1, y1; end exclusive).
    pub fn take_touched(&self) -> Option<[u32; 4]> {
        self.touched.lock().unwrap_or_else(std::sync::PoisonError::into_inner).take()
    }

    /// The view's rectangle (i0, j0, i1, j1; end exclusive) of a field rectangle.
    pub fn view_rect(&self, rect: [u32; 4]) -> [u32; 4] {
        [rect[0], self.h - rect[3], rect[2], self.h - rect[1]]
    }
}

/// A finished LF stroke on the history: applied again (redo) and reverted (undo) on the model's field, copied into the view.
pub struct LfStrokeCommand {
    pub role: LfRole,
    pub record: Arc<LfRecord>,
    pub mirror: Option<Arc<LfMirror>>,
}

impl LfStrokeCommand {
    fn put(&self, model: &mut Option<MapModel>, undo: bool) {
        let Some(item) = model.as_mut().and_then(|m| m.lf_mut(self.role)) else { return };
        let field = item.edit();
        if undo {
            self.record.revert(field)
        } else {
            self.record.apply(field)
        }
        if let Some(m) = &self.mirror {
            m.copy(field, self.record.rect);
        }
    }
}

impl Command<Option<MapModel>> for LfStrokeCommand {
    fn name(&self) -> String {
        match self.role {
            LfRole::Land => "LF stroke".to_owned(),
            LfRole::Sea => "sea LF stroke".to_owned(),
        }
    }

    fn apply(&mut self, model: &mut Option<MapModel>) {
        self.put(model, false);
    }

    fn revert(&mut self, model: &mut Option<MapModel>) {
        self.put(model, true);
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}
