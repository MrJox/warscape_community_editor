//! A UI layout as an editor document: the model on an undo stack, saved as the binary layout (or the XML).

use std::any::Any;
use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;

use wce_editor_core::command::{Command, CommandStack, History};
use wce_editor_core::document::{Document, DocumentCore, OutlinerNode, SaveTarget};
use wce_editor_core::inspector::Inspector;
use wce_scene::render_scene::{LayerId, RenderScene, SourceRef};
use wce_ui_model::{DrawItem, Entry, EntryKind, UiModel};

/// Where the layout came from.
#[derive(Clone, Debug, Default)]
pub struct Source {
    /// In-pack path, lower case; empty for a file opened from disk.
    pub vfs_path: String,
    /// The file it was read from.
    pub disk: PathBuf,
    /// Pack name or "loose".
    pub label: String,
}

/// Replaces the model. The model shares its tree, so a snapshot is a pointer; the edit copied the tree once.
struct SetModel {
    name: String,
    before: UiModel,
    after: UiModel,
    key: u64,
}

impl Command<UiModel> for SetModel {
    fn name(&self) -> String {
        self.name.clone()
    }

    fn apply(&mut self, m: &mut UiModel) {
        *m = self.after.clone();
    }

    fn revert(&mut self, m: &mut UiModel) {
        *m = self.before.clone();
    }

    fn merge_key(&self) -> u64 {
        self.key
    }

    fn merge_from(&mut self, next: &dyn Command<UiModel>) -> bool {
        match next.as_any().downcast_ref::<SetModel>() {
            Some(n) if n.key == self.key => {
                self.after = n.after.clone();
                true
            }
            _ => false,
        }
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

pub struct UiDocument {
    core: DocumentCore,
    history: CommandStack<UiModel>,
    pub source: Source,
    out_dir: PathBuf,
    entries: RefCell<Option<(u64, Rc<Vec<Entry>>)>>,
    draw: RefCell<Option<(u64, Rc<Vec<DrawItem>>)>>,
    xml: RefCell<Option<(u64, Rc<String>)>>,
    /// The layout as opened or last saved: what "reset all changes" goes back to.
    saved: UiModel,
}

/// Selection item of the entry at `index` of `entries()`.
pub fn item_of(index: usize) -> u64 {
    index as u64 + 1
}

/// The entry index of a selection item.
pub fn index_of(item: u64) -> Option<usize> {
    (item >= 1 && item < 0xffff_0000).then(|| item as usize - 1)
}

impl UiDocument {
    pub fn new(source: Source, model: UiModel, out_dir: PathBuf) -> Self {
        Self { core: DocumentCore::new(), saved: model.clone(), history: CommandStack::new(model), source, out_dir, entries: RefCell::new(None), draw: RefCell::new(None), xml: RefCell::new(None) }
    }

    pub fn model(&self) -> &UiModel {
        self.history.model()
    }

    pub fn entries(&self) -> Rc<Vec<Entry>> {
        let v = self.history.version();
        let mut c = self.entries.borrow_mut();
        if c.as_ref().is_none_or(|(cv, _)| *cv != v) {
            *c = Some((v, Rc::new(self.model().entries())));
        }
        c.as_ref().unwrap().1.clone()
    }

    pub fn draw_items(&self) -> Rc<Vec<DrawItem>> {
        let v = self.history.version();
        let mut c = self.draw.borrow_mut();
        if c.as_ref().is_none_or(|(cv, _)| *cv != v) {
            *c = Some((v, Rc::new(self.model().draw_list())));
        }
        c.as_ref().unwrap().1.clone()
    }

    pub fn xml(&self) -> Rc<String> {
        let v = self.history.version();
        let mut c = self.xml.borrow_mut();
        if c.as_ref().is_none_or(|(cv, _)| *cv != v) {
            *c = Some((v, Rc::new(self.model().to_xml())));
        }
        c.as_ref().unwrap().1.clone()
    }

    pub fn version(&self) -> u64 {
        self.history.version()
    }

    pub fn break_merge(&mut self) {
        self.history.break_merge();
    }

    /// Applies `op` to a copy of the model and records the result as one undo entry. Equal non-zero `key`s that follow each other merge
    /// (a drag); an `op` that fails changes nothing.
    pub fn edit<R>(&mut self, name: &str, key: u64, op: impl FnOnce(&mut UiModel) -> Result<R, String>) -> Result<R, String> {
        let before = self.history.model().clone();
        let mut after = before.clone();
        let r = op(&mut after)?;
        self.history.push(Box::new(SetModel { name: name.to_string(), before, after, key }));
        Ok(r)
    }

    /// Replaces the layout with the one on disk, as an undoable step, and marks it saved.
    pub fn reload(&mut self, model: UiModel) -> Result<(), String> {
        self.edit("Reload from disk", 0, |after| {
            *after = model;
            Ok(())
        })?;
        self.history.mark_saved();
        self.saved = self.history.model().clone();
        Ok(())
    }

    /// Puts the layout back as it was opened or last saved, as one undoable step.
    pub fn reset_all(&mut self) -> Result<(), String> {
        let saved = self.saved.clone();
        self.edit("Reset all changes", 0, |after| {
            *after = saved;
            Ok(())
        })?;
        self.history.break_merge();
        // the layout is now what the file on disk holds
        self.history.mark_saved();
        Ok(())
    }

    /// Replaces the whole layout with the XML, if it parses and converts to a binary layout.
    pub fn set_xml(&mut self, xml: &str, key: u64) -> Result<(), String> {
        let m = UiModel::from_xml(xml)?;
        m.clone().to_bytes().map_err(|e| format!("does not convert to a layout: {e}"))?;
        self.edit("Edit XML", key, |after| {
            *after = m;
            Ok(())
        })
    }

    /// Replaces one component (a `uientry` or `template` element) with the XML.
    pub fn set_entry_xml(&mut self, path: &[usize], xml: &str, key: u64) -> Result<(), String> {
        let frag_root = self.model().fragment_from_xml(xml)?;
        if frag_root.name != "uientry" && frag_root.name != "template" {
            return Err("the element must be a <component> (<uientry>) or <template>".into());
        }
        let path = path.to_vec();
        self.edit("Edit XML", key, |after| {
            *after.dom_mut().at_mut(&path).ok_or("the component is gone")? = frag_root;
            after.clone().to_bytes().map_err(|e| format!("does not convert to a layout: {e}"))?;
            Ok(())
        })
    }

    fn default_save_path(&self) -> PathBuf {
        if self.source.vfs_path.is_empty() { self.source.disk.clone() } else { self.out_dir.join(&self.source.vfs_path) }
    }

    /// The binary layout of the current state.
    pub fn bytes(&self) -> Result<Vec<u8>, String> {
        self.model().clone().to_bytes()
    }

    /// Writes the layout into a mod pack of `game` under its in-pack path (`inside`), creating the pack if needed.
    pub fn save_to_pack(&self, game: &wce_vfs::GameProfile, pack: &std::path::Path, inside: &str) -> Result<(), String> {
        wce_ui_model::pack::write_pack(game, pack, &wce_vfs::normalize(inside), &self.bytes()?)
    }

    fn label(e: &Entry) -> String {
        match e.kind {
            EntryKind::Template => format!("template {}", e.name),
            EntryKind::Entry if e.class.is_empty() => e.name.clone(),
            EntryKind::Entry => format!("{} ({})", e.name, e.class),
        }
    }
}

fn field_key(doc: u32, item: u64, field: u64) -> u64 {
    ((u64::from(doc) << 40) ^ (item << 8) ^ field) | 1
}

impl Document for UiDocument {
    fn core(&self) -> &DocumentCore {
        &self.core
    }

    fn core_mut(&mut self) -> &mut DocumentCore {
        &mut self.core
    }

    fn history(&self) -> &dyn History {
        &self.history
    }

    fn history_mut(&mut self) -> &mut dyn History {
        &mut self.history
    }

    fn title(&self) -> String {
        if self.source.vfs_path.is_empty() {
            self.source.disk.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()
        } else {
            self.source.vfs_path.rsplit('/').next().unwrap_or_default().to_owned()
        }
    }

    fn kind(&self) -> String {
        "ui_layout".to_owned()
    }

    fn can_save(&self) -> bool {
        true
    }

    /// An empty target saves where the file came from: a file on disk in place, a layout from the packs as a loose file under the output
    /// folder (never into the game's packs).
    fn save(&mut self, target: &SaveTarget) -> Result<(), String> {
        let to = if target.path.as_os_str().is_empty() { self.default_save_path() } else { target.path.clone() };
        let ext = to.extension().map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_default();
        let data = if ext == "xml" || ext == "uixml" { self.model().to_xml().into_bytes() } else { self.bytes()? };
        if let Some(parent) = to.parent() {
            let _ = std::fs::create_dir_all(parent);
        }

        std::fs::write(&to, data).map_err(|e| format!("cannot write {}: {e}", to.display()))?;
        self.history.mark_saved();
        self.saved = self.history.model().clone();
        if self.source.vfs_path.is_empty() {
            self.source.disk = to;
        }

        Ok(())
    }

    fn publish(&mut self, _: &mut RenderScene, _: LayerId) {}

    fn source_key(&self) -> String {
        self.source.vfs_path.clone()
    }

    fn outline(&self) -> OutlinerNode {
        let ents = self.entries();
        fn build(doc: &UiDocument, ents: &[Entry], i: usize) -> OutlinerNode {
            let children = ents.iter().enumerate().filter(|(_, e)| e.parent == Some(i)).map(|(k, _)| build(doc, ents, k)).collect();
            OutlinerNode { label: UiDocument::label(&ents[i]), source: SourceRef { doc: doc.id(), item: item_of(i) }, selectable: true, children }
        }

        let children = if ents.is_empty() { vec![] } else { vec![build(self, &ents, 0)] };
        OutlinerNode { label: self.title(), source: SourceRef { doc: self.id(), item: 0xffff_ffff }, selectable: false, children }
    }

    fn describe(&self, item: SourceRef) -> String {
        let ents = self.entries();
        index_of(item.item).and_then(|i| ents.get(i)).map(Self::label).unwrap_or_default()
    }

    fn inspect(&mut self, item: SourceRef, ins: &mut dyn Inspector) {
        let ents = self.entries();
        let Some(i) = index_of(item.item) else { return };
        let Some(e) = ents.get(i).cloned() else { return };
        let doc = self.id();
        ins.section(&Self::label(&e));
        ins.text("id", "id", &e.id.to_string());
        ins.text("screen", "on screen", &format!("{}, {}", e.ax, e.ay));
        if e.kind != EntryKind::Entry {
            return;
        }

        let path = e.path.clone();
        let mut name = e.name.clone();
        if ins.edit_string("name", "name", &mut name) {
            let p = path.clone();
            let _ = self.edit("Rename", field_key(doc, item.item, 1), |m| m.set_name(&p, &name));
        }

        let mut x = i64::from(e.x);
        let mut y = i64::from(e.y);
        let mut moved = ins.edit_int("x", "x offset", &mut x, 1.0);
        moved |= ins.edit_int("y", "y offset", &mut y, 1.0);
        if moved {
            let p = path.clone();
            let _ = self.edit("Move", field_key(doc, item.item, 2), |m| m.set_offset(&p, x as i32, y as i32));
        }

        let mut w = i64::from(e.w);
        let mut h = i64::from(e.h);
        let mut sized = ins.edit_int("w", "width", &mut w, 1.0);
        sized |= ins.edit_int("h", "height", &mut h, 1.0);
        if sized {
            let p = path.clone();
            let _ = self.edit("Resize", field_key(doc, item.item, 3), |m| m.set_size(&p, w.max(0) as i32, h.max(0) as i32));
        }

        let mut text = self.model().state_text(&path);
        if ins.edit_string("text", "text", &mut text) {
            let p = path.clone();
            let _ = self.edit("Set text", field_key(doc, item.item, 4), |m| m.set_text(&p, &text));
        }

        let c = self.model().state_color(&path);
        let mut col = [f32::from(c[0]) / 255.0, f32::from(c[1]) / 255.0, f32::from(c[2]) / 255.0, f32::from(c[3]) / 255.0];
        if ins.edit_color4("color", "text colour", &mut col) {
            let rgba = [0, 1, 2, 3].map(|k| (col[k].clamp(0.0, 1.0) * 255.0).round() as u8);
            let p = path.clone();
            let _ = self.edit("Set colour", field_key(doc, item.item, 5), |m| m.set_state_color(&p, rgba));
        }

        if ins.gesture_ended() {
            self.history.break_merge();
        }
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}
