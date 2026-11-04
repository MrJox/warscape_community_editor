//! Cell layer edits of a map document: a stroke's undo record as a command, and the watch that tells the views which cells changed (the
//! Map panel's picture, the fill run again over the tile map and the climates).

use std::any::Any;
use std::sync::{Arc, Mutex, PoisonError};

use wce_editor_core::command::Command;
use wce_terrain::MapModel;
use wce_terrain::cell_edit::CellRecord;

/// The cells changed since the views last looked: (layer index in the model, x0, y0, x1, y1; end exclusive), undo and redo included.
#[derive(Debug, Default)]
pub struct CellWatch {
    touched: Mutex<Vec<(usize, [u32; 4])>>,
}

impl CellWatch {
    pub fn note(&self, layer: usize, rect: [u32; 4]) {
        let mut t = self.touched.lock().unwrap_or_else(PoisonError::into_inner);
        match t.iter_mut().find(|(l, _)| *l == layer) {
            Some((_, r)) => *r = [r[0].min(rect[0]), r[1].min(rect[1]), r[2].max(rect[2]), r[3].max(rect[3])],
            None => t.push((layer, rect)),
        }
    }

    pub fn take(&self) -> Vec<(usize, [u32; 4])> {
        std::mem::take(&mut *self.touched.lock().unwrap_or_else(PoisonError::into_inner))
    }
}

/// A finished cell stroke on the history: applied again (redo) and reverted (undo) on the model's layer.
pub struct CellStrokeCommand {
    /// The layer's index in the model, and what the stroke did ("paint tile map", ...).
    pub layer: usize,
    pub name: String,
    pub record: Arc<CellRecord>,
    pub watch: Arc<CellWatch>,
}

impl CellStrokeCommand {
    fn put(&self, model: &mut Option<MapModel>, undo: bool) {
        let Some((_, item)) = model.as_mut().and_then(|m| m.layers.get_mut(self.layer)) else { return };
        let l = item.edit();
        if undo { self.record.revert(l) } else { self.record.apply(l) }
        self.watch.note(self.layer, self.record.rect);
    }
}

impl Command<Option<MapModel>> for CellStrokeCommand {
    fn name(&self) -> String {
        self.name.clone()
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

/// The watch's layer index for the explicit tiles (`explicit_tiles.txt`): an edit of them fills the map again too.
pub const EXPLICIT: usize = usize::MAX;

/// The watch's key for a built map's battle field (its placements, `terrain.tile_list`).
pub const TILE_LIST: usize = usize::MAX - 1;

/// An edit of a built map's battle field: the list before and after (Rome 2's maps: a few thousand placements, a few hundred KB).
pub struct TileListCommand {
    pub name: String,
    pub before: wce_tile::battle_map::tile_list::TileList,
    pub after: wce_tile::battle_map::tile_list::TileList,
    pub watch: Arc<CellWatch>,
}

impl TileListCommand {
    fn put(&self, model: &mut Option<MapModel>, to: &wce_tile::battle_map::tile_list::TileList) {
        if let Some(item) = model.as_mut().and_then(|m| m.tile_list.as_mut()) {
            *item.edit() = to.clone();
            self.watch.note(TILE_LIST, [0, 0, 0, 0]);
        }
    }
}

impl Command<Option<MapModel>> for TileListCommand {
    fn name(&self) -> String {
        self.name.clone()
    }

    fn apply(&mut self, model: &mut Option<MapModel>) {
        let to = self.after.clone();
        self.put(model, &to);
    }

    fn revert(&mut self, model: &mut Option<MapModel>) {
        let to = self.before.clone();
        self.put(model, &to);
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

/// An edit of the explicit tiles: the list before and after (a small file; none: the map has no `explicit_tiles.txt`).
pub struct ExplicitCommand {
    pub name: String,
    pub before: Option<wce_terrain::explicit::ExplicitTiles>,
    pub after: Option<wce_terrain::explicit::ExplicitTiles>,
    pub watch: Arc<CellWatch>,
}

impl ExplicitCommand {
    fn put(&self, model: &mut Option<MapModel>, to: Option<&wce_terrain::explicit::ExplicitTiles>) {
        let Some(m) = model.as_mut() else { return };
        match (to, m.explicit.as_mut()) {
            (Some(t), Some(item)) => *item.edit() = t.clone(),
            (Some(t), None) => {
                let mut item = wce_terrain::Item::new("explicit_tiles.txt", t.clone(), wce_tile_model::Blob::Bytes(Arc::from(&[][..])));
                item.edit();
                m.explicit = Some(item);
            }

            (None, _) => m.explicit = None,
        }

        self.watch.note(EXPLICIT, [0, 0, 0, 0]);
    }
}

impl Command<Option<MapModel>> for ExplicitCommand {
    fn name(&self) -> String {
        self.name.clone()
    }

    fn apply(&mut self, model: &mut Option<MapModel>) {
        self.put(model, self.after.as_ref());
    }

    fn revert(&mut self, model: &mut Option<MapModel>) {
        self.put(model, self.before.as_ref());
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}
