//! A document is one thing a tool edits (an environment file, a tile, a sample map). It owns the lossless model of that thing, accepts edits as
//! commands on its own stack, and publishes what it shows into the shared `RenderScene` (one layer per document). The editor core (outliner,
//! inspector, gizmo, save) only talks to this interface; what a click on item N means is the document's business.

use std::any::Any;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU32, Ordering};

use wce_foundation::math::Float4x4;
use wce_scene::environment::EnvironmentParams;
use wce_scene::render_scene::{DocumentId, LayerId, RenderScene, SourceRef};

use crate::command::{Dirty, History};
use crate::inspector::Inspector;

/// Where a document is saved to.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SaveTarget {
    /// Empty: back to where the document came from.
    pub path: PathBuf,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct OutlinerNode {
    pub label: String,

    /// What a click selects.
    pub source: SourceRef,
    pub selectable: bool,
    pub children: Vec<OutlinerNode>,
}

static NEXT_DOCUMENT: AtomicU32 = AtomicU32::new(1);

/// What every document has: its id (unique in the process, from 1) and the outline cache.
#[derive(Debug)]
pub struct DocumentCore {
    id: DocumentId,
    outline: Option<(u64, OutlinerNode)>,
}

impl Default for DocumentCore {
    fn default() -> Self {
        Self::new()
    }
}

impl DocumentCore {
    pub fn new() -> Self {
        Self { id: NEXT_DOCUMENT.fetch_add(1, Ordering::Relaxed), outline: None }
    }

    pub fn id(&self) -> DocumentId {
        self.id
    }
}

pub trait Document: Any {
    fn core(&self) -> &DocumentCore;
    fn core_mut(&mut self) -> &mut DocumentCore;
    fn history(&self) -> &dyn History;
    fn history_mut(&mut self) -> &mut dyn History;

    fn id(&self) -> DocumentId {
        self.core().id
    }

    fn title(&self) -> String;

    /// "sample_map", "environment", ...
    fn kind(&self) -> String;

    fn modified(&self) -> bool {
        self.history().modified()
    }

    fn can_save(&self) -> bool {
        false
    }

    /// What the last save said beyond "saved" (what it wrote, a warning), shown with it; none by default.
    fn save_message(&self) -> Option<String> {
        None
    }

    /// Save does something even without changes (a tile opened from the Assembly Kit: its first save makes its project).
    fn save_wanted(&self) -> bool {
        false
    }

    /// Writes the document as minimal edits of its source bytes.
    ///
    /// # Errors
    ///
    /// The reason it could not be saved.
    fn save(&mut self, target: &SaveTarget) -> Result<(), String> {
        let _ = target;
        Err(format!("{} cannot be saved", self.title()))
    }

    /// Puts everything the document shows into `layer` (called on open and when the whole document changed).
    fn publish(&mut self, scene: &mut RenderScene, layer: LayerId);

    /// Updates what the commands since the last publish touched.
    fn republish(&mut self, scene: &mut RenderScene, layer: LayerId, dirty: &Dirty) {
        let _ = dirty;
        self.publish(scene, layer);
    }

    fn outline(&self) -> OutlinerNode {
        OutlinerNode { label: self.title(), source: SourceRef { doc: self.id(), item: 0 }, selectable: false, children: vec![] }
    }

    /// The outline as of the last edit: rebuilt (through `outline`) only when the history changed, so a panel can ask every frame.
    fn outline_cached(&mut self) -> &OutlinerNode {
        let stamp = self.outline_stamp();
        if self.core().outline.as_ref().is_none_or(|(s, _)| *s != stamp) {
            let outline = self.outline();
            self.core_mut().outline = Some((stamp, outline));
        }

        &self.core().outline.as_ref().expect("the outline was just built").1
    }

    fn invalidate_outline(&mut self) {
        self.core_mut().outline = None;
    }

    /// Changes whenever the outline does (default: with every edit; a document whose edits mostly do not touch the tree overrides it).
    fn outline_stamp(&self) -> u64 {
        self.history().version()
    }

    fn inspect(&mut self, item: SourceRef, inspector: &mut dyn Inspector) {
        let _ = (item, inspector);
    }

    fn describe(&self, item: SourceRef) -> String {
        let _ = item;
        String::new()
    }

    /// Identifies the file a document came from (a normalized in-pack path); an open request for the same key activates the open document instead.
    fn source_key(&self) -> String {
        String::new()
    }

    /// Cross-tool composition. A document that can light the scene (an .environment or .lighting file) says what it would light it with; any
    /// tool's scene can then be previewed under it.
    fn environment(&self) -> Option<EnvironmentParams> {
        None
    }

    /// True for a document that brings its own ground and objects (a tile). A stand-in scene (the weather tool's sample map) is hidden while one
    /// is open and shown again when the last one closes; the user's own visibility toggles in between stay.
    fn shows_scene(&self) -> bool {
        false
    }

    fn stand_in(&self) -> bool {
        false
    }

    /// Gizmo support: the world transform of an item (the gizmo's pivot).
    fn item_transform(&self, item: SourceRef) -> Option<Float4x4> {
        let _ = item;
        None
    }

    /// Moves items by a world-space delta. `gesture` is constant during one drag so that the commands merge into one undo entry.
    fn transform_items(&mut self, items: &[SourceRef], delta: &Float4x4, gesture: u64) {
        let _ = (items, delta, gesture);
    }

    /// The document as `Any`, for tools to reach their own document types.
    fn as_any(&self) -> &dyn Any;
    fn as_any_mut(&mut self) -> &mut dyn Any;
}
