//! A variant mesh definition as a document. The lossless [`XmlDoc`] is the model: a value edit changes its span only, a structural edit (a
//! slot, a mesh, a tint added; a node removed, copied or moved) swaps the document, which keeps the rest of the text as it was. Save writes it
//! to the project's loose folder under its path in the packs, mounted before them, so every reader of the file (the tile's references too)
//! sees it at once.

use std::any::Any;
use std::path::PathBuf;
use std::sync::Arc;

use wce_editor_core::command::{Command, CommandStack, History};
use wce_editor_core::document::{Document, DocumentCore, SaveTarget};
use wce_formats::variant_mesh::{VariantMeshFile, edit};
use wce_formats::xml_doc::{XmlDoc, XmlError, encode_entities};
use wce_scene::render_scene::{LayerId, Payload, RenderScene, SourceRef, Terrain};

/// Where a definition came from.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Source {
    /// In-pack path, lower case; empty for a file opened from disk.
    pub vfs_path: String,

    /// The file it was read from: the pack, or the file on disk.
    pub disk: PathBuf,

    /// Pack name, "project" or "loose".
    pub label: String,
}

struct AttrCommand {
    name: String,
    elem: usize,
    attr: usize,
    before: Vec<u8>,
    after: Vec<u8>,
    gesture: u64,
}

impl Command<XmlDoc> for AttrCommand {
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
                self.after.clone_from(&n.after);
                true
            }

            _ => false,
        }
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

/// A structural edit: the whole document before and after (its elements are numbered again).
struct SnapshotCommand {
    name: String,
    before: Arc<XmlDoc>,
    after: Arc<XmlDoc>,
}

impl Command<XmlDoc> for SnapshotCommand {
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

/// A structural edit of the tree.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Structural {
    /// A slot named so, the last of the mesh.
    AddSlot(String),

    /// A mesh of the model, the last alternative of the slot.
    AddMesh(String),

    /// A reference to the definition, the last alternative of the slot.
    AddReference(String),
    AddTint,
    Remove,
    Duplicate,
    MoveUp,
    MoveDown,
}

pub struct VmdDocument {
    core: DocumentCore,
    history: CommandStack<XmlDoc>,
    source: Source,

    /// The file wasn't XML: the document is the repaired text, and saving writes that.
    repaired: bool,

    /// The project's loose folder: where a file of the packs is saved, under its path in the packs.
    loose_dir: PathBuf,

    /// The tree of the document as of a history version.
    tree: Option<(u64, Arc<VariantMeshFile>)>,

    /// The stage's grass (the weather chips' climate and season).
    ground: String,
}

/// The stage's grass until the weather chips pick a climate's.
const GROUND: &str = "terrain/textures/battle/default/grass/0_su_diffuse.dds";

impl VmdDocument {
    /// The stage's grass: kept for the next publish, and the ground to draw now.
    pub fn set_ground(&mut self, texture: &str, game_units: f32) -> Terrain {
        texture.clone_into(&mut self.ground);
        let units = game_units.max(1e-3);
        Terrain::flat(400.0 / units, &self.ground, 8.0 / units)
    }

    /// # Errors
    ///
    /// The bytes are not a variant mesh definition, even repaired.
    pub fn new(source: Source, bytes: Vec<u8>, loose_dir: PathBuf) -> Result<Self, String> {
        let file = VariantMeshFile::parse(bytes).map_err(|e| e.to_string())?;
        let repaired = file.repaired;
        Ok(Self {
            core: DocumentCore::new(),
            history: CommandStack::new(file.doc),
            source,
            repaired,
            loose_dir,
            tree: None,
            ground: GROUND.to_owned(),
        })
    }

    pub fn xml(&self) -> &XmlDoc {
        self.history.model()
    }

    pub fn source(&self) -> &Source {
        &self.source
    }

    pub fn repaired(&self) -> bool {
        self.repaired
    }

    /// What changes when the document does: the history's position.
    pub fn version(&self) -> u64 {
        self.history.version()
    }

    /// The tree as the document is now.
    pub fn tree(&mut self) -> Arc<VariantMeshFile> {
        let version = self.history.version();
        if let Some((v, tree)) = &self.tree
            && *v == version
        {
            return tree.clone();
        }

        // The root was a VARIANT_MESH when it was read, and no edit here changes it.
        let tree = Arc::new(
            VariantMeshFile::from_doc(self.xml().clone(), self.repaired)
                .unwrap_or_else(|_| VariantMeshFile::parse(b"<VARIANT_MESH/>".to_vec()).expect("an empty definition parses")),
        );
        self.tree = Some((version, tree.clone()));
        tree
    }

    pub fn default_save_path(&self) -> PathBuf {
        if self.source.vfs_path.is_empty() { self.source.disk.clone() } else { self.loose_dir.join(&self.source.vfs_path) }
    }

    /// The last `n` edits as one undo entry named `name` (a node dragged several places: one move each).
    pub fn join_last(&mut self, n: usize, name: &str) {
        self.history.join_last(n, name);
    }

    /// Ends a drag's merging: the next edit is an undo entry of its own.
    pub fn break_merge(&mut self) {
        self.history.break_merge();
    }

    /// Sets an attribute (added after the others when missing). `gesture` merges the edits of one drag or typing into one entry (0: never).
    pub fn set_value(&mut self, elem: usize, name: &str, value: &str, gesture: u64) {
        let xml = self.xml();
        if elem >= xml.elements().len() {
            return;
        }

        let label = format!("{} @{name}", xml.path_of(elem));
        match xml.find_attr(elem, name) {
            Some(attr) => {
                let quote = xml.elements()[elem].attrs[attr].quote;
                let after = encode_entities(value.as_bytes(), quote);
                let before = xml.attr_raw(elem, attr).to_vec();
                if before != after {
                    self.history.push(Box::new(AttrCommand { name: format!("Edit {label}"), elem, attr, before, after, gesture }));
                }
            }

            None => {
                if let Ok(next) = xml.with_attr_added(elem, name, value) {
                    self.push_snapshot(format!("Add {label}"), next);
                }
            }
        }
    }

    /// Takes an attribute away, if the element has it.
    pub fn remove_value(&mut self, elem: usize, name: &str) {
        let xml = self.xml();
        let Some(attr) = xml.find_attr(elem, name) else { return };
        if let Ok(next) = xml.without_attr(elem, attr) {
            self.push_snapshot(format!("Remove {} @{name}", xml.path_of(elem)), next);
        }
    }

    fn push_snapshot(&mut self, name: String, next: XmlDoc) {
        let before = Arc::new(self.xml().clone());
        self.history.push(Box::new(SnapshotCommand { name, before, after: Arc::new(next) }));
    }

    /// A structural edit of `elem`; returns the element to select afterwards (the new one, or the one moved), `None` for none.
    ///
    /// # Errors
    ///
    /// Why the document can't take it.
    pub fn structural(&mut self, op: &Structural, elem: usize) -> Result<Option<usize>, String> {
        let xml = self.xml();
        if elem >= xml.elements().len() {
            return Err("no such element".to_owned());
        }

        let path = xml.path_of(elem);
        let err = |e: XmlError| e.to_string();
        let siblings =
            |xml: &XmlDoc| xml.elements()[elem].parent.map_or_else(|| xml.roots().to_vec(), |p| xml.elements()[p].children.clone());
        let (next, select, label) = match op {
            Structural::AddSlot(name) => {
                let (next, e) = edit::add_slot(xml, elem, name).map_err(err)?;
                (next, Some(e), format!("Add slot {name} to {path}"))
            }

            Structural::AddMesh(model) => {
                let (next, e) = edit::add_mesh(xml, elem, model).map_err(err)?;
                (next, Some(e), format!("Add mesh to {path}"))
            }

            Structural::AddReference(definition) => {
                let (next, e) = edit::add_reference(xml, elem, definition).map_err(err)?;
                (next, Some(e), format!("Add reference to {path}"))
            }

            Structural::AddTint => {
                let (next, e) = edit::add_tint(xml, elem).map_err(err)?;
                (next, Some(e), format!("Add tint to {path}"))
            }

            Structural::Remove => (xml.removed_element(elem).map_err(err)?, xml.elements()[elem].parent, format!("Delete {path}")),
            Structural::Duplicate => {
                let (next, e) = xml.duplicated_element(elem).map_err(err)?;
                (next, Some(e), format!("Duplicate {path}"))
            }

            Structural::MoveUp | Structural::MoveDown => {
                let up = *op == Structural::MoveUp;
                let next = xml.moved_element(elem, if up { -1 } else { 1 }).map_err(err)?;

                // The moved element's index afterwards: where its sibling started (up), or after the sibling's subtree (down).
                let list = siblings(xml);
                let at = list.iter().position(|&s| s == elem).unwrap_or_default();
                let select = if up { list.get(at.wrapping_sub(1)).copied() } else { list.get(at + 1).map(|&s| elem + xml.subtree_size(s)) };
                (next, select, format!("Move {} {path}", if up { "up" } else { "down" }))
            }
        };

        self.push_snapshot(label, next);
        Ok(select)
    }

    /// Back to the file as first read (structural edits too); the history goes.
    pub fn revert_all(&mut self) {
        let xml = self.history.model_mut();
        if xml.structural_edits() > 0 {
            if let Ok(file) = VariantMeshFile::parse(xml.origin_bytes().to_vec()) {
                *xml = file.doc;
            }
        } else {
            xml.revert_all();
        }

        self.history.clear();
    }
}

impl Document for VmdDocument {
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
        "variant_mesh_definition".to_owned()
    }

    fn modified(&self) -> bool {
        self.xml().modified()
    }

    fn can_save(&self) -> bool {
        true
    }

    /// The file as saved (or as opened); not undoable, as a structural edit's snapshot could not be replayed over it.
    fn revert(&mut self) -> Result<String, String> {
        self.revert_all();
        Ok(format!("{} as saved", self.title()))
    }

    fn save(&mut self, target: &SaveTarget) -> Result<(), String> {
        let to = if target.path.as_os_str().is_empty() { self.default_save_path() } else { target.path.clone() };
        let fail = |error: String| format!("save failed: {error}");
        if let Some(parent) = to.parent() {
            let _ = std::fs::create_dir_all(parent);
        }

        wce_foundation::files::write_replacing(&to, self.xml().save()).map_err(|e| fail(format!("cannot write {}: {e}", to.display())))?;

        // A game file's copy elsewhere (Save as): the project's file still lacks the edits, so the document stays unsaved.
        if !self.source.vfs_path.is_empty() && !wce_editor_core::document::same_path(&to, &self.default_save_path()) {
            return Ok(());
        }

        // The saved file becomes the new baseline: XML now, whatever it was.
        let bytes = std::fs::read(&to).map_err(|error| fail(error.to_string()))?;
        *self.history.model_mut() = XmlDoc::parse(bytes).map_err(|error| fail(error.to_string()))?;
        self.history.clear();
        self.repaired = false;
        self.tree = None;
        if self.source.vfs_path.is_empty() {
            self.source.disk = to;
        } else {
            "project".clone_into(&mut self.source.label);
        }

        Ok(())
    }

    /// A ground to stand on (the soldiers are the tool's, posed every frame).
    fn publish(&mut self, scene: &mut RenderScene, layer: LayerId) {
        scene.clear_layer(layer);
        let units = scene.game_units.max(1e-3);
        let ground = Terrain::flat(400.0 / units, &self.ground, 8.0 / units);
        scene.add(layer, wce_foundation::math::Float4x4::IDENTITY, Payload::Terrain(ground), SourceRef { doc: self.id(), item: 0 });
    }

    fn describe(&self, item: SourceRef) -> String {
        if item.item == 0 { "Ground".to_owned() } else { self.xml().path_of(item.item as usize) }
    }

    fn source_key(&self) -> String {
        self.source.vfs_path.clone()
    }

    fn file_path(&self) -> Option<PathBuf> {
        self.source.vfs_path.is_empty().then(|| self.source.disk.clone())
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use wce_editor_core::document::Document as _;
    use wce_formats::variant_mesh::Node;

    use super::{Source, Structural, VmdDocument};

    const FILE: &str = "<VARIANT_MESH>\r\n\t<SLOT name=\"a\">\r\n\t\t<VARIANT_MESH model=\"x\"/>\r\n\t</SLOT>\r\n\t<SLOT name=\"b\">\r\n\t\t<VARIANT_MESH model=\"y\"/>\r\n\t</SLOT>\r\n</VARIANT_MESH>\r\n";

    fn doc() -> VmdDocument {
        VmdDocument::new(Source::default(), FILE.as_bytes().to_vec(), PathBuf::new()).expect("reads")
    }

    #[test]
    fn edits_undo_and_the_tree_follows() {
        let mut d = doc();
        let mesh = d.tree().node(&[0, 0]).expect("a mesh").element();
        d.set_value(mesh, "model", "z", 1);
        d.set_value(mesh, "model", "zz", 1);
        assert_eq!(d.xml().save(), FILE.replace("\"x\"", "\"zz\"").as_bytes());
        assert!(matches!(d.tree().node(&[0, 0]), Some(Node::Mesh(m)) if m.model.as_deref() == Some("zz")));
        d.history_mut().undo();
        assert_eq!(d.xml().save(), FILE.as_bytes(), "one typing, one undo");

        d.set_value(mesh, "mask0", "2", 0);
        assert!(matches!(d.tree().node(&[0, 0]), Some(Node::Mesh(m)) if m.masks[0] == Some(2)));
        let mesh = d.tree().node(&[0, 0]).expect("a mesh").element();
        d.remove_value(mesh, "mask0");
        assert_eq!(d.xml().save(), FILE.as_bytes());
    }

    #[test]
    fn moves_select_what_moved() {
        let mut d = doc();
        let b = d.tree().node(&[1]).expect("slot b").element();
        let moved = d.structural(&Structural::MoveUp, b).expect("moves").expect("a selection");
        let tree = d.tree();
        assert_eq!(tree.path_of(moved), Some(vec![0]));
        assert!(matches!(tree.node(&[0]), Some(Node::Slot(s)) if s.name == "b"));
        let down = d.structural(&Structural::MoveDown, moved).expect("moves").expect("a selection");
        assert_eq!(d.tree().path_of(down), Some(vec![1]));
        assert_eq!(d.xml().save(), FILE.as_bytes());

        let a = d.tree().node(&[0]).expect("slot a").element();
        let copy = d.structural(&Structural::Duplicate, a).expect("copies").expect("a selection");
        assert_eq!(d.tree().path_of(copy), Some(vec![1]));
    }
}
