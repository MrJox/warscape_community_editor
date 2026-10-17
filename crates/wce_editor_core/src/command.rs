//! Undo / redo. Every edit a document accepts is a [`Command`] pushed on the document's [`CommandStack`]: `apply` does it, `revert` takes it
//! back. Drag gestures (a gizmo drag, a slider drag) push many small commands with the same non-zero merge key; the stack folds them into one
//! entry until [`CommandStack::break_merge`] is called (the UI calls it when the gesture ends). The stack also tells whether the document
//! differs from its last saved state.
//!
//! The stack owns what its commands edit (the document's lossless model: an `XmlDoc`, a `TedScene`, ...), so a command gets it as an argument
//! instead of holding a reference to it; the stack knows nothing else about it. [`History`] is the stack without its model's type, which is what
//! the editor (history panel, save, the document manager) works with.

use std::any::Any;

/// One undoable edit of a `M`.
pub trait Command<M>: Any {
    fn name(&self) -> String;
    fn apply(&mut self, model: &mut M);
    fn revert(&mut self, model: &mut M);

    /// Non-zero: commands with equal keys that follow each other may merge (`merge_from` keeps this command's `before` and takes `next`'s `after`).
    fn merge_key(&self) -> u64 {
        0
    }

    fn merge_from(&mut self, next: &dyn Command<M>) -> bool {
        let _ = next;
        false
    }

    /// Items (document-defined ids) this command touched, so that only they are re-published. Empty = everything.
    fn dirty_items(&self) -> Vec<u64> {
        vec![]
    }

    /// The command as `Any`, for `merge_from` to check the type of the next command.
    fn as_any(&self) -> &dyn Any;
}

/// Items touched by commands (`all` set when unknown).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Dirty {
    pub all: bool,
    pub items: Vec<u64>,
}

impl Default for Dirty {
    fn default() -> Self {
        Self { all: true, items: vec![] }
    }
}

impl Dirty {
    const NONE: Dirty = Dirty { all: false, items: vec![] };
}

/// The commands of a document, with the model they edit.
pub struct CommandStack<M: 'static> {
    model: M,
    entries: Vec<Box<dyn Command<M>>>,
    position: usize,
    saved_position: usize,
    saved_valid: bool,
    merge_open: bool,
    version: u64,
    last_dirty: Dirty,
    pending: Dirty,
}

impl<M: Default + 'static> Default for CommandStack<M> {
    fn default() -> Self {
        Self::new(M::default())
    }
}

impl<M: 'static> CommandStack<M> {

    /// An empty history of `model`.
    pub fn new(model: M) -> Self {
        Self { model, entries: vec![], position: 0, saved_position: 0, saved_valid: false, merge_open: false, version: 0, last_dirty: Dirty::default(), pending: Dirty::NONE }
    }

    /// The model the commands edit.
    pub fn model(&self) -> &M {
        &self.model
    }

    /// The model, for changes that are not edits (caches, a reload). Edits go through `push`.
    pub fn model_mut(&mut self) -> &mut M {
        &mut self.model
    }

    /// Applies the command and records it. Drops the redo tail.
    pub fn push(&mut self, mut c: Box<dyn Command<M>>) {
        if self.position < self.entries.len() {
            // The redo tail goes; the saved state may have been in it.
            if self.saved_valid && self.saved_position > self.position {
                self.saved_valid = false;
            }

            self.entries.truncate(self.position);
            self.merge_open = false;
        }

        c.apply(&mut self.model);
        if self.merge_open && self.position > 0 && c.merge_key() != 0 && self.entries[self.position - 1].merge_key() == c.merge_key()
            && !(self.saved_valid && self.saved_position == self.position) && self.entries[self.position - 1].merge_from(c.as_ref())
        {
            let items = self.entries[self.position - 1].dirty_items();
            self.note(items);
            return;
        }

        self.note(c.dirty_items());
        self.merge_open = c.merge_key() != 0;
        self.entries.push(c);
        self.position = self.entries.len();
    }

    fn note(&mut self, items: Vec<u64>) {
        self.last_dirty.all = items.is_empty();
        if self.last_dirty.all {
            self.pending.all = true;
        } else if !self.pending.all {
            self.pending.items.extend_from_slice(&items);
        }

        self.last_dirty.items = items;
        self.version += 1;
    }
}

/// A command stack without the type of its model.
pub trait History {
    fn undo(&mut self) -> bool;
    fn redo(&mut self) -> bool;
    fn can_undo(&self) -> bool;
    fn can_redo(&self) -> bool;
    fn break_merge(&mut self);

    /// Forgets the entries that were undone (a cancelled gesture must not leave a redo behind).
    fn drop_redo(&mut self);

    /// The history, oldest first; entries `[0, position)` are applied.
    fn size(&self) -> usize;
    fn position(&self) -> usize;
    fn name(&self, i: usize) -> String;

    /// Undo or redo until `position` entries are applied.
    fn jump_to(&mut self, position: usize);

    /// Saved state tracking: `modified` is false right after `mark_saved` and again after undoing back to that point.
    fn mark_saved(&mut self);
    fn modified(&self) -> bool;

    /// Changes whenever an entry is pushed, merged, undone or redone (documents re-publish when it moves).
    fn version(&self) -> u64;

    /// Items touched by the most recent push / undo / redo.
    fn last_dirty(&self) -> &Dirty;

    /// Everything touched since the last call (several commands can move in one frame: a multi-step undo, `jump_to`): the union of their items,
    /// or all when any of them did not say. This is what a document re-publishes from.
    fn take_dirty(&mut self) -> Dirty;
    fn clear(&mut self);
}

impl<M: 'static> History for CommandStack<M> {
    fn undo(&mut self) -> bool {
        if !self.can_undo() {
            return false;
        }

        self.merge_open = false;
        let c = &mut self.entries[self.position - 1];
        c.revert(&mut self.model);
        let items = c.dirty_items();
        self.note(items);
        self.position -= 1;
        true
    }

    fn redo(&mut self) -> bool {
        if !self.can_redo() {
            return false;
        }

        self.merge_open = false;
        let c = &mut self.entries[self.position];
        c.apply(&mut self.model);
        let items = c.dirty_items();
        self.note(items);
        self.position += 1;
        true
    }

    fn can_undo(&self) -> bool {
        self.position > 0
    }

    fn can_redo(&self) -> bool {
        self.position < self.entries.len()
    }

    fn break_merge(&mut self) {
        self.merge_open = false;
    }

    fn drop_redo(&mut self) {
        if self.position >= self.entries.len() {
            return;
        }

        if self.saved_valid && self.saved_position > self.position {
            self.saved_valid = false;
        }

        self.entries.truncate(self.position);
        self.merge_open = false;
    }

    fn size(&self) -> usize {
        self.entries.len()
    }

    fn position(&self) -> usize {
        self.position
    }

    fn name(&self, i: usize) -> String {
        self.entries[i].name()
    }

    fn jump_to(&mut self, position: usize) {
        let position = position.min(self.entries.len());
        while self.position > position {
            self.undo();
        }

        while self.position < position {
            self.redo();
        }
    }

    fn mark_saved(&mut self) {
        self.saved_position = self.position;
        self.saved_valid = true;
    }

    fn modified(&self) -> bool {
        if self.saved_valid { self.position != self.saved_position } else { self.position != 0 }
    }

    fn version(&self) -> u64 {
        self.version
    }

    fn last_dirty(&self) -> &Dirty {
        &self.last_dirty
    }

    fn take_dirty(&mut self) -> Dirty {
        std::mem::replace(&mut self.pending, Dirty::NONE)
    }

    fn clear(&mut self) {
        self.entries.clear();
        self.position = 0;
        self.saved_position = 0;
        self.saved_valid = false;
        self.merge_open = false;
        self.last_dirty = Dirty::default();
        self.pending = Dirty::default();
        self.version += 1;
    }
}
