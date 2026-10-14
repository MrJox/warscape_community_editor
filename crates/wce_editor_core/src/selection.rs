//! The selection: a set of `SourceRef`s (document + item), shared by the viewport, outliner, inspector and gizmo. It names document elements,
//! not scene entities; the scene entities that show them come from `RenderScene::entities_of`.

use wce_scene::render_scene::{DocumentId, EntityId, RenderScene, SourceRef};

#[derive(Clone, Debug, Default)]
pub struct Selection {
    items: Vec<SourceRef>,
    revision: u64,
}

impl Selection {
    pub fn items(&self) -> &[SourceRef] {
        &self.items
    }

    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    pub fn revision(&self) -> u64 {
        self.revision
    }

    pub fn contains(&self, r: SourceRef) -> bool {
        self.items.contains(&r)
    }

    pub fn clear(&mut self) {
        if !self.items.is_empty() {
            self.items.clear();
            self.revision += 1;
        }
    }

    pub fn set(&mut self, r: SourceRef) {
        if self.items.len() == 1 && self.items[0] == r {
            return;
        }

        self.items = vec![r];
        self.revision += 1;
    }

    pub fn add(&mut self, r: SourceRef) {
        if !self.contains(r) {
            self.items.push(r);
            self.revision += 1;
        }
    }

    pub fn toggle(&mut self, r: SourceRef) {
        match self.items.iter().position(|&s| s == r) {
            Some(i) => {
                self.items.remove(i);
            }

            None => self.items.push(r),
        }

        self.revision += 1;
    }

    pub fn remove_document(&mut self, doc: DocumentId) {
        let n = self.items.len();
        self.items.retain(|s| s.doc != doc);
        if self.items.len() != n {
            self.revision += 1;
        }
    }

    /// The scene entities that show the selected items.
    pub fn entities(&self, scene: &RenderScene) -> Vec<EntityId> {
        self.items.iter().flat_map(|&r| scene.entities_of(r)).collect()
    }
}
