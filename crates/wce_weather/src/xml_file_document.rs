//! A weather file as a document: .environment (battle), .lighting (campaign, porthole), weather_definitions.xml, vfx/*.xml. The lossless XmlDoc
//! is the model; every edit is a command on the document's stack, so undo and redo, merged drags and the modified marker come from the shared
//! core. Saving writes the original bytes with only the edited spans changed (`XmlDoc::save`), as a loose file under the output folder keeping
//! the in-pack path.

use std::any::Any;
use std::path::PathBuf;
use std::sync::Arc;

use wce_editor_core::command::{Command, CommandStack, History};
use wce_editor_core::document::{Document, DocumentCore, OutlinerNode, SaveTarget};
use wce_editor_core::inspector::Inspector;
use wce_formats::env_value::{ValueKind, format_value, parse_value};
use wce_formats::xml_doc::{XmlDoc, decode_entities, encode_entities};
use wce_scene::environment::{EnvironmentParams, read_environment};
use wce_scene::render_scene::{LayerId, RenderScene, SourceRef};

/// Where a weather file came from.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Source {
    /// In-pack path, lower case; empty for a file opened from disk.
    pub vfs_path: String,

    /// File it was read from.
    pub disk: PathBuf,

    /// Pack name or "loose".
    pub label: String,
}

struct XmlAttrCommand {
    name: String,
    elem: usize,
    attr: usize,
    before: Vec<u8>,
    after: Vec<u8>,
    gesture: u64,
}

impl Command<XmlDoc> for XmlAttrCommand {
    fn name(&self) -> String {
        self.name.clone()
    }

    fn apply(&mut self, xml: &mut XmlDoc) {
        xml.set_attr_raw(self.elem, self.attr, self.after.clone());
    }

    fn revert(&mut self, xml: &mut XmlDoc) {
        xml.set_attr_raw(self.elem, self.attr, self.before.clone());
    }

    fn merge_key(&self) -> u64 {
        if self.gesture != 0 { self.gesture ^ ((self.elem as u64) << 20) ^ ((self.attr as u64) << 48) } else { 0 }
    }

    fn merge_from(&mut self, next: &dyn Command<XmlDoc>) -> bool {
        match next.as_any().downcast_ref::<Self>() {
            Some(n) if n.elem == self.elem && n.attr == self.attr => {
                self.after = n.after.clone();
                true
            }

            _ => false,
        }
    }

    fn dirty_items(&self) -> Vec<u64> {
        vec![self.elem as u64]
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

/// A structural edit swaps the whole XmlDoc (element indices change), so it keeps snapshots like the legacy editor did.
struct XmlSnapshotCommand {
    name: String,
    before: Arc<XmlDoc>,
    after: Arc<XmlDoc>,
}

impl Command<XmlDoc> for XmlSnapshotCommand {
    fn name(&self) -> String {
        self.name.clone()
    }

    fn apply(&mut self, xml: &mut XmlDoc) {
        *xml = (*self.after).clone();
    }

    fn revert(&mut self, xml: &mut XmlDoc) {
        *xml = (*self.before).clone();
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

/// A structural edit.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Op {
    Remove,
    Duplicate,
    MoveUp,
    MoveDown,
    AddChild,
    AddAttr,
    RemoveAttr,
}

fn element_label(doc: &XmlDoc, e: usize) -> String {
    let mut label = doc.elements()[e].name.clone();
    if let Some(n) = doc.get(e, "name").or_else(|| doc.get(e, "inst_name")) {
        label += &format!("  '{n}'");
    }

    label
}

fn text(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).into_owned()
}

pub struct XmlFileDocument {
    core: DocumentCore,
    history: CommandStack<XmlDoc>,
    source: Source,
    output_dir: PathBuf,
}

impl XmlFileDocument {

    /// # Errors
    ///
    /// The bytes are not an XML document.
    pub fn new(source: Source, bytes: Vec<u8>, output_dir: PathBuf) -> Result<Self, String> {
        let xml = XmlDoc::parse(bytes).map_err(|error| error.to_string())?;
        Ok(Self { core: DocumentCore::new(), history: CommandStack::new(xml), source, output_dir })
    }

    pub fn xml(&self) -> &XmlDoc {
        self.history.model()
    }

    pub fn source(&self) -> &Source {
        &self.source
    }

    pub fn set_output_dir(&mut self, dir: PathBuf) {
        self.output_dir = dir;
    }

    pub fn default_save_path(&self) -> PathBuf {
        if self.source.vfs_path.is_empty() { self.source.disk.clone() } else { self.output_dir.join(&self.source.vfs_path) }
    }

    /// .environment or .lighting: the kinds that light the scene.
    pub fn is_environment(&self) -> bool {
        matches!(self.kind().as_str(), "environment" | "lighting")
    }

    fn valid(&self, elem: usize, attr: usize) -> bool {
        self.xml().elements().get(elem).is_some_and(|e| attr < e.attrs.len())
    }

    /// Sets an attribute to `raw` (already entity-encoded, as it is stored). `gesture` merges the commands of one drag into one entry (0: never merge).
    pub fn set_attr(&mut self, elem: usize, attr: usize, raw: Vec<u8>, gesture: u64) {
        if !self.valid(elem, attr) {
            return;
        }

        let before = self.xml().attr_raw(elem, attr).to_vec();
        if before == raw {
            return;
        }

        let name = format!("Edit {} @{}", self.xml().path_of(elem), self.xml().elements()[elem].attrs[attr].name);
        self.history.push(Box::new(XmlAttrCommand { name, elem, attr, before, after: raw, gesture }));
    }

    /// Parses the displayed value of an attribute, replaces it, formats it back in the number style of the original text.
    pub fn set_attr_text(&mut self, elem: usize, attr: usize, value: &str, gesture: u64) {
        if !self.valid(elem, attr) {
            return;
        }

        let quote = self.xml().elements()[elem].attrs[attr].quote;
        let v = parse_value(&text(&decode_entities(self.xml().attr_raw(elem, attr))));

        // Same number style as the original text; plain strings are taken as typed.
        let mut n = parse_value(value);
        if n.kind == v.kind {
            n.style = v.style;
        }

        let out = if n.kind == ValueKind::String { value.to_owned() } else { format_value(&n) };
        self.set_attr(elem, attr, encode_entities(out.as_bytes(), quote), gesture);
    }

    /// Structural edit; returns the element to select afterwards (`None`: none).
    ///
    /// # Errors
    ///
    /// Why the edit can't be made.
    pub fn structural(&mut self, op: Op, elem: usize, attr: usize, name: &str, value: &str) -> Result<Option<usize>, String> {
        let xml = self.xml();
        if elem >= xml.elements().len() {
            return Err("no such element".to_owned());
        }

        let path = xml.path_of(elem);
        let e = |error: wce_formats::xml_doc::XmlError| error.to_string();
        let (next, sel, label) = match op {
            Op::Remove => (xml.removed_element(elem).map_err(e)?, xml.elements()[elem].parent, format!("Delete {path}")),
            Op::Duplicate => {
                let (next, sel) = xml.duplicated_element(elem).map_err(e)?;
                (next, Some(sel), format!("Duplicate {path}"))
            }

            Op::MoveUp => (xml.moved_element(elem, -1).map_err(e)?, None, format!("Move up {path}")),
            Op::MoveDown => (xml.moved_element(elem, 1).map_err(e)?, None, format!("Move down {path}")),
            Op::AddChild => {
                let (next, sel) = xml.with_child(elem, name).map_err(e)?;
                (next, Some(sel), format!("Add <{name}> to {path}"))
            }

            Op::AddAttr => (xml.with_attr_added(elem, name, value).map_err(e)?, Some(elem), format!("Add @{name} to {path}")),
            Op::RemoveAttr => {
                let attr_name = xml.elements()[elem].attrs.get(attr).ok_or("no such attribute")?.name.clone();
                (xml.without_attr(elem, attr).map_err(e)?, Some(elem), format!("Remove @{attr_name} of {path}"))
            }
        };

        let before = Arc::new(xml.clone());
        self.history.push(Box::new(XmlSnapshotCommand { name: label, before, after: Arc::new(next) }));
        Ok(sel)
    }

    /// Back to the file as first loaded (also undoes structural edits), clears the history.
    pub fn revert_all(&mut self) {
        let xml = self.history.model_mut();
        if xml.structural_edits() > 0 {
            if let Ok(original) = XmlDoc::parse(xml.origin_bytes().to_vec()) {
                *xml = original;
            }
        } else {
            xml.revert_all();
        }

        self.history.clear();
    }
}

impl Document for XmlFileDocument {
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

    /// The file name.
    fn title(&self) -> String {
        if self.source.vfs_path.is_empty() {
            self.source.disk.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()
        } else {
            self.source.vfs_path.rsplit('/').next().unwrap_or_default().to_owned()
        }
    }

    /// environment, lighting, weather_definitions, vfx, xml.
    fn kind(&self) -> String {
        let t = self.title().to_ascii_lowercase();
        let kind = if t.len() > 12 && t.ends_with(".environment") {
            "environment"
        } else if t.len() > 9 && t.ends_with(".lighting") {
            "lighting"
        } else if self.source.vfs_path.starts_with("vfx/") {
            "vfx"
        } else if self.source.vfs_path.starts_with("weather/") {
            "weather_definitions"
        } else {
            "xml"
        };

        kind.to_owned()
    }

    fn modified(&self) -> bool {
        self.xml().modified()
    }

    fn can_save(&self) -> bool {
        true
    }

    fn save(&mut self, target: &SaveTarget) -> Result<(), String> {
        let to = if target.path.as_os_str().is_empty() { self.default_save_path() } else { target.path.clone() };
        let fail = |error: String| format!("save failed: {error}");
        if let Some(parent) = to.parent() {
            let _ = std::fs::create_dir_all(parent);
        }

        std::fs::write(&to, self.xml().save()).map_err(|_| fail("write failed".to_owned()))?;

        // The saved file becomes the new baseline.
        let bytes = std::fs::read(&to).map_err(|error| fail(error.to_string()))?;
        *self.history.model_mut() = XmlDoc::parse(bytes).map_err(|error| fail(error.to_string()))?;
        self.history.clear();
        if self.source.vfs_path.is_empty() {
            self.source.disk = to;
        }

        Ok(())
    }

    /// Lights the scene through the weather tool, owns no entities.
    fn publish(&mut self, _: &mut RenderScene, _: LayerId) {}

    fn outline(&self) -> OutlinerNode {
        fn build(doc: &XmlFileDocument, e: usize) -> OutlinerNode {
            let xml = doc.xml();
            let children = xml.elements()[e].children.iter().map(|&c| build(doc, c)).collect();
            OutlinerNode { label: element_label(xml, e), source: SourceRef { doc: doc.id(), item: e as u64 }, selectable: true, children }
        }

        let children = self.xml().roots().iter().map(|&r| build(self, r)).collect();
        OutlinerNode { label: self.title(), source: SourceRef { doc: self.id(), item: 0xffff_ffff }, selectable: false, children }
    }

    fn describe(&self, item: SourceRef) -> String {
        if (item.item as usize) < self.xml().elements().len() { self.xml().path_of(item.item as usize) } else { String::new() }
    }

    fn inspect(&mut self, item: SourceRef, ins: &mut dyn Inspector) {
        let e = item.item as usize;
        let Some(el) = self.xml().elements().get(e).cloned() else { return };
        ins.section(&format!("Attributes of <{}>", el.name));
        if el.attrs.is_empty() {
            ins.text("none", "(no attributes)", "");
        }

        let gesture_base = (u64::from(self.id()) << 40) ^ ((e as u64) << 8) ^ 0x7;
        for (a, attr) in el.attrs.iter().enumerate() {
            let mut v = parse_value(&text(&decode_entities(self.xml().attr_raw(e, a))));
            let label = attr.name.as_str();
            let changed = match v.kind {
                ValueKind::Bool => ins.edit_bool(label, label, &mut v.b),
                ValueKind::Int => ins.edit_int(label, label, &mut v.i, 0.2),
                ValueKind::Float => {
                    let speed = (v.f[0].abs() * 0.005).max(0.001);
                    ins.edit_float(label, label, &mut v.f[0], speed, -1e9, 1e9)
                }

                ValueKind::Rgb => edit_array::<3>(&mut v.f, |f| ins.edit_color3(label, label, f)),
                ValueKind::Argb => edit_array::<4>(&mut v.f, |f| ins.edit_color4(label, label, f)),
                ValueKind::Variance => edit_array::<2>(&mut v.f, |f| ins.edit_float2(label, label, f, 0.01)),
                ValueKind::Euler => edit_array::<3>(&mut v.f, |f| ins.edit_float3(label, label, f, 0.1)),
                ValueKind::String => ins.edit_string(label, label, &mut v.s),
            };

            if changed {
                self.set_attr(e, a, encode_entities(format_value(&v).as_bytes(), attr.quote), gesture_base ^ ((a as u64) << 52));
            }

            if ins.gesture_ended() {
                self.history.break_merge();
            }
        }
    }

    fn source_key(&self) -> String {
        self.source.vfs_path.clone()
    }

    /// An .environment / .lighting file can light any scene.
    fn environment(&self) -> Option<EnvironmentParams> {
        self.is_environment().then(|| read_environment(self.xml()))
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}

/// Edits the first N components of a value through a fixed-size widget.
pub(crate) fn edit_array<const N: usize>(f: &mut [f32], edit: impl FnOnce(&mut [f32; N]) -> bool) -> bool {
    let mut a = [0.0f32; N];
    for (d, s) in a.iter_mut().zip(f.iter()) {
        *d = *s;
    }

    let changed = edit(&mut a);
    for (d, s) in f.iter_mut().zip(a) {
        *d = s;
    }

    changed
}
