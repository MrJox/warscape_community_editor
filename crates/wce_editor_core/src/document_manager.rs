//! Owns the open documents, gives each a layer in the shared `RenderScene`, keeps the scene in step with the documents' edits, and routes save
//! and close. Each tool has at most one document (a tool's stand-in scene aside, `Document::stand_in`); documents of different tools live side
//! by side. The active document is the active tool's, else its stand-in (what the menus, the history panel and the default inspector target
//! follow).

use wce_scene::render_scene::{DocumentId, LayerId, RenderScene};

use crate::document::{Document, SaveTarget};
use std::collections::VecDeque;

struct Entry {
    doc: Box<dyn Document>,
    tool: String,
    layer: LayerId,
    published_version: u64,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum SavePolicy {
    Prompt,
    RefuseChanges,
    Overwrite,
}

#[derive(Clone)]
pub struct ChangedSave {
    pub id: DocumentId,
    pub target: SaveTarget,
    pub reason: String,
}

struct SaveBatch {
    remaining: VecDeque<DocumentId>,
    failures: Vec<String>,
}

pub struct DocumentManager {
    pending_save: Option<ChangedSave>,
    pending_save_all: Option<SaveBatch>,
    prompt_saves: bool,
    entries: Vec<Entry>,
    order: Vec<DocumentId>,

    /// The active tool's id (`set_active_tool`): its document is the active one.
    active_tool: String,

    /// `shows_scene` as of the last `apply_stand_ins`.
    real_scene: bool,

    /// The source keys of the documents saved since the last `take_saved` (the editor indexes loose copies again).
    saved: Vec<String>,
}

impl Default for DocumentManager {
    fn default() -> Self {
        Self {
            pending_save: None,
            pending_save_all: None,
            prompt_saves: true,
            entries: vec![],
            order: vec![],
            active_tool: String::new(),
            real_scene: false,
            saved: vec![],
        }
    }
}

impl DocumentManager {
    /// Takes the document, creates its layer and publishes it (the active one when it is the active tool's).
    ///
    /// # Errors
    ///
    /// The tool has a document already (a stand-in aside): the caller closes it first (`Editor` replaces it, asking about unsaved changes).
    pub fn add(&mut self, scene: &mut RenderScene, mut doc: Box<dyn Document>, tool_id: &str) -> Result<DocumentId, String> {
        if let Some(path) = doc.save_destination()
            && let Some(id) = self.destination_owner(&path, None)?
        {
            return Err(format!("Save destination is already owned by open document {id}; replace it explicitly"));
        }
        if !doc.stand_in()
            && let Some(old) = self.of_tool(tool_id).and_then(|id| self.find(id))
        {
            return Err(format!("{} is open already: close it first ({tool_id} has one document at a time)", old.title()));
        }

        let layer = scene.add_layer(doc.id(), &doc.title());
        doc.publish(scene, layer);
        let id = doc.id();
        let stand_in = doc.stand_in();
        self.entries.push(Entry { published_version: doc.history().version(), doc, tool: tool_id.to_owned(), layer });
        self.order.push(id);

        // Added while a tile is open.
        if stand_in && self.real_scene {
            scene.set_layer_visible(layer, false);
        }

        self.apply_stand_ins(scene);
        Ok(id)
    }

    /// Drops the document and its layer (the caller asked about unsaved changes).
    pub fn close(&mut self, scene: &mut RenderScene, id: DocumentId) -> bool {
        let Some(i) = self.entries.iter().position(|e| e.doc.id() == id) else { return false };
        scene.remove_layer(self.entries[i].layer);
        self.entries.remove(i);
        if self.pending_save.as_ref().is_some_and(|pending| pending.id == id) {
            self.pending_save = None;
            self.pending_save_all = None;
        }
        self.order.retain(|&d| d != id);
        self.apply_stand_ins(scene);
        true
    }

    fn entry(&self, id: DocumentId) -> Option<&Entry> {
        self.entries.iter().find(|e| e.doc.id() == id)
    }

    fn entry_mut(&mut self, id: DocumentId) -> Option<&mut Entry> {
        self.entries.iter_mut().find(|e| e.doc.id() == id)
    }

    pub fn find(&self, id: DocumentId) -> Option<&dyn Document> {
        self.entry(id).map(|e| e.doc.as_ref())
    }

    pub fn find_mut(&mut self, id: DocumentId) -> Option<&mut dyn Document> {
        Some(self.entry_mut(id)?.doc.as_mut())
    }

    /// A document as its own type.
    pub fn find_as<T: Document>(&self, id: DocumentId) -> Option<&T> {
        self.find(id)?.as_any().downcast_ref()
    }

    pub fn find_as_mut<T: Document>(&mut self, id: DocumentId) -> Option<&mut T> {
        self.find_mut(id)?.as_any_mut().downcast_mut()
    }

    /// The tool's document (not its stand-in), if it has one open.
    pub fn of_tool(&self, tool_id: &str) -> Option<DocumentId> {
        self.entries.iter().find(|e| e.tool == tool_id && !e.doc.stand_in()).map(|e| e.doc.id())
    }

    /// The active tool changed (the editor calls it): its document is the active one from now on.
    pub fn set_active_tool(&mut self, tool_id: &str) {
        if self.active_tool != tool_id {
            tool_id.clone_into(&mut self.active_tool);
        }
    }

    pub fn active_tool(&self) -> &str {
        &self.active_tool
    }

    /// The active tool's document.
    pub fn active(&self) -> Option<&dyn Document> {
        self.find(self.active_id())
    }

    pub fn active_mut(&mut self) -> Option<&mut dyn Document> {
        self.find_mut(self.active_id())
    }

    /// The active tool's document, else its stand-in (the Weather tool's sample map: its edits, History), 0 when it has neither.
    pub fn active_id(&self) -> DocumentId {
        let tool = self.active_tool.as_str();
        self.of_tool(tool).or_else(|| self.entries.iter().find(|e| e.tool == tool && e.doc.stand_in()).map(|e| e.doc.id())).unwrap_or(0)
    }

    pub fn layer_of(&self, id: DocumentId) -> LayerId {
        self.entry(id).map_or(0, |e| e.layer)
    }

    pub fn tool_of(&self, id: DocumentId) -> String {
        self.entry(id).map(|e| e.tool.clone()).unwrap_or_default()
    }

    /// The documents in opening order.
    pub fn order(&self) -> &[DocumentId] {
        &self.order
    }

    /// A document that brings its own ground (a tile) is open: stand-in scenes (the weather tool's sample map) are hidden meanwhile.
    pub fn shows_scene(&self) -> bool {
        self.entries.iter().any(|e| e.doc.shows_scene())
    }

    /// Stand-in scenes follow the first and last tile: hidden when one opens, shown when none is left.
    fn apply_stand_ins(&mut self, scene: &mut RenderScene) {
        let real = self.shows_scene();
        if real == self.real_scene {
            return;
        }

        self.real_scene = real;
        for e in self.entries.iter().filter(|e| e.doc.stand_in()) {
            scene.set_layer_visible(e.layer, !real);
        }
    }

    /// Re-publishes every document whose command stack moved since its last publish. Call once per frame, before drawing.
    pub fn update(&mut self, scene: &mut RenderScene) {
        for e in &mut self.entries {
            let v = e.doc.history().version();
            if v == e.published_version {
                continue;
            }

            let dirty = e.doc.history_mut().take_dirty();
            e.doc.republish(scene, e.layer, &dirty);
            e.published_version = v;
        }
    }

    pub fn any_modified(&self) -> bool {
        self.entries.iter().any(|e| e.doc.modified())
    }

    /// The modified documents, in opening order.
    pub fn unsaved(&self) -> Vec<DocumentId> {
        self.order.iter().copied().filter(|&id| self.entry(id).is_some_and(|e| e.doc.modified())).collect()
    }

    /// Saves one document in place and marks it saved.
    ///
    /// # Errors
    ///
    /// Why it could not be saved. On success the message says what was saved.
    pub fn save_one(&mut self, id: DocumentId) -> Result<String, String> {
        self.save_to(id, &SaveTarget::default())
    }

    pub fn destination_owner(&self, path: &std::path::Path, except: Option<DocumentId>) -> Result<Option<DocumentId>, String> {
        let key = wce_foundation::save_path::key(path)?;
        for e in &self.entries {
            if Some(e.doc.id()) != except
                && let Some(p) = e.doc.save_destination()
                && wce_foundation::save_path::key(&p)? == key
            {
                return Ok(Some(e.doc.id()));
            }
        }
        Ok(None)
    }

    /// Shared live Save/Save As boundary; ownership is checked before calling the document.
    pub fn pending_changed_save(&self) -> Option<&ChangedSave> {
        self.pending_save.as_ref()
    }
    /// Script execution refuses conflicts instead of creating choices with no answerer.
    pub fn set_save_prompts(&mut self, enabled: bool) -> bool {
        let previous = std::mem::replace(&mut self.prompt_saves, enabled);
        if !enabled {
            self.pending_save = None;
            self.pending_save_all = None;
        }
        previous
    }
    pub fn answer_changed_save(&mut self, overwrite: bool) -> Result<String, String> {
        let pending = self.pending_save.take().ok_or("no changed-file Save awaiting a choice")?;
        let batch = self.pending_save_all.take();
        let result = if overwrite {
            self.save_with_policy(pending.id, &pending.target, SavePolicy::Overwrite)
        } else {
            Ok("Save cancelled; document and files unchanged".into())
        };
        if overwrite && let Some(mut batch) = batch {
            let mut messages = batch.failures.clone();
            match result {
                Ok(message) => messages.push(message),
                Err(message) => {
                    messages.push(message.clone());
                    batch.failures.push(message);
                }
            }
            let all = self.resume_save_all(batch, &mut messages);
            let message = messages.join("; ");
            if all { Ok(message) } else { Err(message) }
        } else {
            result
        }
    }
    pub fn save_to(&mut self, id: DocumentId, target: &SaveTarget) -> Result<String, String> {
        let policy = if self.prompt_saves { SavePolicy::Prompt } else { SavePolicy::RefuseChanges };
        self.save_with_policy(id, target, policy)
    }
    fn save_with_policy(&mut self, id: DocumentId, target: &SaveTarget, policy: SavePolicy) -> Result<String, String> {
        let d = self.find(id).ok_or("no such document")?;
        let owned = target.path.as_os_str().is_empty()
            || d.save_destination()
                .map(|path| Ok::<_, String>(wce_foundation::save_path::key(&path)? == wce_foundation::save_path::key(&target.path)?))
                .transpose()?
                .unwrap_or(false);
        let destination =
            if target.path.as_os_str().is_empty() { d.save_destination() } else { d.save_destination().map(|_| target.path.clone()) };
        if let Some(path) = destination
            && let Some(owner) = self.destination_owner(&path, Some(id))?
        {
            return Err(format!("Cannot save: {} is already owned by open document {owner}", path.display()));
        }
        if policy != SavePolicy::Overwrite
            && let Some(reason) = d.save_changes(target)?
        {
            if policy == SavePolicy::Prompt {
                self.pending_save = Some(ChangedSave { id, target: target.clone(), reason: reason.clone() });
            }
            return Err(reason);
        }
        let Some(e) = self.entry_mut(id) else { return Err("no such document".to_owned()) };
        if !e.doc.can_save() {
            return Err(format!("{} cannot be saved", e.doc.title()));
        }

        if policy == SavePolicy::Overwrite {
            e.doc.save_overwrite(target)?;
        } else {
            e.doc.save(target)?;
        }
        if owned {
            e.doc.history_mut().mark_saved();
        }
        let key = e.doc.source_key();
        let message = if !owned && !target.path.as_os_str().is_empty() && e.doc.modified() {
            format!("a copy saved to {}; the document's own file does not have the edits yet (Save)", target.path.display())
        } else {
            match e.doc.save_message() {
                Some(m) if !m.is_empty() => format!("saved {}: {m}", e.doc.title()),
                _ => format!("saved {}", e.doc.title()),
            }
        };
        if owned {
            self.saved.push(key);
        }
        Ok(message)
    }

    /// The source keys of the documents saved since the last call (`source_key`: a game file's path, empty for others).
    pub fn take_saved(&mut self) -> Vec<String> {
        std::mem::take(&mut self.saved)
    }

    /// Saves every modified document that can be saved. The messages say what was saved and explain each failure; true when nothing failed.
    pub fn save_all(&mut self, messages: &mut Vec<String>) -> bool {
        if !self.prompt_saves {
            return self.save_all_without_prompt(messages);
        }
        if self.pending_save.is_some() {
            messages.push("Resolve the pending Save choice before starting Save All".into());
            return false;
        }
        self.resume_save_all(SaveBatch { remaining: self.order.iter().copied().collect(), failures: vec![] }, messages)
    }

    fn resume_save_all(&mut self, mut batch: SaveBatch, messages: &mut Vec<String>) -> bool {
        while let Some(id) = batch.remaining.pop_front() {
            let Some(entry) = self.entry(id).filter(|e| e.doc.modified() && e.doc.can_save()) else { continue };
            let title = entry.doc.title();
            match self.save_one(id) {
                Ok(message) => messages.push(message),
                Err(message) => {
                    let message = format!("cannot save {title}: {message}");
                    messages.push(message.clone());
                    if self.pending_save.as_ref().is_some_and(|pending| pending.id == id) {
                        self.pending_save_all = Some(batch);
                        return false;
                    }
                    batch.failures.push(message);
                }
            }
        }
        batch.failures.is_empty()
    }

    /// Best-effort Save when no UI can answer a conflict. Attempts every document,
    /// refuses unreviewed outside changes, and leaves failed edits dirty for recovery.
    pub fn save_all_without_prompt(&mut self, messages: &mut Vec<String>) -> bool {
        self.pending_save = None;
        self.pending_save_all = None;
        self.save_all_with_policy(messages, SavePolicy::RefuseChanges)
    }

    fn save_all_with_policy(&mut self, messages: &mut Vec<String>, policy: SavePolicy) -> bool {
        let mut ok = true;
        for id in self.order.clone() {
            let Some(e) = self.entry(id).filter(|e| e.doc.modified() && e.doc.can_save()) else { continue };
            let title = e.doc.title();
            match self.save_with_policy(id, &SaveTarget::default(), policy) {
                Ok(message) => messages.push(message),
                Err(message) => {
                    ok = false;
                    messages.push(format!("cannot save {title}: {message}"));
                    if policy == SavePolicy::Prompt && self.pending_save.is_some() {
                        break;
                    }
                }
            }
        }

        ok
    }

    /// Layer visibility per document (the tools' rules: the Prefab tool hides the tiles while its stage shows).
    pub fn set_visible(&self, scene: &mut RenderScene, id: DocumentId, visible: bool) {
        if let Some(e) = self.entry(id) {
            scene.set_layer_visible(e.layer, visible);
        }
    }

    pub fn visible(&self, scene: &RenderScene, id: DocumentId) -> bool {
        self.entry(id).and_then(|e| scene.layer(e.layer)).is_none_or(|l| l.visible)
    }
}
