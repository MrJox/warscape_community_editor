//! `.terry` edits of a map document: an attribute set (the Inspector, the gizmo: merged while one gesture lasts) and a structural edit
//! (entities added or removed: the text splices that did it). Both find their elements by entity id when they run, so they stay right
//! whatever the other edits did to the element table. Every one moves the document's `.terry` revision (the views draw the entities again).

use std::any::Any;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use wce_editor_core::command::Command;
use wce_formats::xml_doc::XmlSplice;
use wce_terrain::MapModel;

/// Moves with every `.terry` edit, undo and redo included.
pub type TerryRevision = Arc<AtomicU64>;

/// An attribute of an entity's component (`component` empty: of the `<entity>` element itself, its name).
pub struct TerryAttrCommand {
    pub id: u64,
    pub component: String,
    pub attr: String,
    pub before: String,
    pub after: String,

    /// Commands of one gesture merge (0: never).
    pub key: u64,
    pub revision: TerryRevision,
}

impl TerryAttrCommand {
    fn put(&self, model: &mut Option<MapModel>, value: &str) {
        let Some(item) = model.as_mut().and_then(|m| m.project.as_mut()) else { return };
        let p = item.edit();
        if self.component.is_empty() {
            if let Some(e) = p.entity(self.id).map(|e| e.elem) {
                p.doc.set(e, &self.attr, value);
                p.reindex();
            }
        } else {
            p.set_attr(self.id, &self.component, &self.attr, value);
        }

        self.revision.fetch_add(1, Ordering::Relaxed);
    }
}

impl Command<Option<MapModel>> for TerryAttrCommand {
    fn name(&self) -> String {
        format!("{} {}", if self.component.is_empty() { "entity" } else { &self.component }, self.attr)
    }

    fn apply(&mut self, model: &mut Option<MapModel>) {
        self.put(model, &self.after.clone());
    }

    fn revert(&mut self, model: &mut Option<MapModel>) {
        self.put(model, &self.before.clone());
    }

    fn merge_key(&self) -> u64 {
        self.key
    }

    fn merge_from(&mut self, next: &dyn Command<Option<MapModel>>) -> bool {
        let Some(n) = next.as_any().downcast_ref::<Self>() else { return false };
        if n.id != self.id || n.component != self.component || n.attr != self.attr {
            return false;
        }

        self.after.clone_from(&n.after);
        true
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

/// Entities added or removed: the splices that did it (made with `TerryProject::insert_entities` / `remove_entities`, already in the
/// document when pushed: applying them again is skipped the first time).
pub struct TerrySpliceCommand {
    pub name: String,
    pub splices: Vec<XmlSplice>,
    pub done: bool,
    pub revision: TerryRevision,
}

impl Command<Option<MapModel>> for TerrySpliceCommand {
    fn name(&self) -> String {
        self.name.clone()
    }

    fn apply(&mut self, model: &mut Option<MapModel>) {
        if self.done {
            self.done = false;
            return;
        }

        if let Some(item) = model.as_mut().and_then(|m| m.project.as_mut()) {
            item.edit().redo(&self.splices);
            self.revision.fetch_add(1, Ordering::Relaxed);
        }
    }

    fn revert(&mut self, model: &mut Option<MapModel>) {
        if let Some(item) = model.as_mut().and_then(|m| m.project.as_mut()) {
            item.edit().undo(&self.splices);
            self.revision.fetch_add(1, Ordering::Relaxed);
        }
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}
