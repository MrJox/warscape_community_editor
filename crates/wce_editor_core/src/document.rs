//! A document is one thing a tool edits (an environment file, a tile, a sample map). It owns the lossless model of that thing, accepts edits as
//! commands on its own stack, and publishes what it shows into the shared `RenderScene` (one layer per document). The editor core (selection,
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

/// Whether two paths name the same file, as the file dialogs and the joins spell them (separators and, on Windows, case aside).
pub fn same_path(a: &std::path::Path, b: &std::path::Path) -> bool {
    let norm = |p: &std::path::Path| {
        let s = p.display().to_string().replace('\\', "/");
        if cfg!(windows) { s.to_lowercase() } else { s }
    };
    norm(a) == norm(b)
}

static NEXT_DOCUMENT: AtomicU32 = AtomicU32::new(1);

/// What every document has: its id (unique in the process, from 1).
#[derive(Debug)]
pub struct DocumentCore {
    id: DocumentId,
}

impl Default for DocumentCore {
    fn default() -> Self {
        Self::new()
    }
}

impl DocumentCore {
    pub fn new() -> Self {
        Self { id: NEXT_DOCUMENT.fetch_add(1, Ordering::Relaxed) }
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

    /// Why File > Save as does not apply to the document (it is off, with this as its tooltip); `None`: it does. Save as writes a copy
    /// wherever the user picks, which is no use when the document is more than its files (a tile is its tile database entry too).
    fn save_as_blocked(&self) -> Option<String> {
        None
    }

    /// Writes the document as minimal edits of its source bytes. A save to where the document saves (`target` empty, or its own file)
    /// marks it saved; a copy elsewhere (Save as) does not, as its own file still lacks the edits.
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

    fn inspect(&mut self, item: SourceRef, inspector: &mut dyn Inspector) {
        let _ = (item, inspector);
    }

    /// Several items selected (of this document, the first one's): the properties they have in common, an edit setting them all. False when
    /// the document does not do that (the Inspector shows the first item).
    fn inspect_many(&mut self, items: &[SourceRef], inspector: &mut dyn Inspector) -> bool {
        let _ = (items, inspector);
        false
    }

    fn describe(&self, item: SourceRef) -> String {
        let _ = item;
        String::new()
    }

    /// A view filter laying a colour over some of the document's items in the viewport (the tile tool's freeform instances): a key that
    /// changes whenever the items or the colour do (0: no tint, `tint` is not asked), the items and the colour (RGBA, 0..1).
    fn tint_key(&self) -> u64 {
        0
    }

    fn tint(&self) -> (Vec<u64>, [f32; 4]) {
        (vec![], [0.0; 4])
    }

    /// The items a selected item is outlined (and framed) by in the viewport: itself, or what it holds when it draws nothing of its own (a
    /// group: the items below it).
    fn outline_items(&self, item: SourceRef) -> Vec<SourceRef> {
        vec![item]
    }

    /// Identifies the file a document came from (a normalized in-pack path); an open request for the same key shows the open document instead.
    fn source_key(&self) -> String {
        String::new()
    }

    /// The file on disk the document was opened from and saves to, when it is one file outside the packs (Save as starts there).
    fn file_path(&self) -> Option<std::path::PathBuf> {
        None
    }

    /// Whether opening `path` (a file or a folder on disk) would open this document again; the open request then shows it instead.
    fn opened_from(&self, _path: &str) -> bool {
        false
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

    /// The world frame of the item's parent (its group, prefab or layer), in the space of [`Self::item_transform`]: the axes of local
    /// coordinates. None: the world's.
    fn item_parent_transform(&self, item: SourceRef) -> Option<Float4x4> {
        let _ = item;
        None
    }

    /// An item the mouse must not move (TEd's draggable flag off): the gizmo is not shown for it, and a move of a selection leaves it.
    fn item_locked(&self, item: SourceRef) -> bool {
        let _ = item;
        false
    }

    /// Moves items by a world-space delta. `gesture` is constant during one drag so that the commands merge into one undo entry.
    fn transform_items(&mut self, items: &[SourceRef], delta: &Float4x4, gesture: u64) {
        let _ = (items, delta, gesture);
    }

    /// The Scale gizmo: `factor` is the scale on the item's own axes since the drag began (not since the last call), `delta` the same as a
    /// world-space delta for the documents that take it as a move.
    fn scale_items(&mut self, items: &[SourceRef], factor: [f32; 3], delta: &Float4x4, gesture: u64) {
        let _ = factor;
        self.transform_items(items, delta, gesture);
    }

    /// The document as `Any`, for tools to reach their own document types.
    fn as_any(&self) -> &dyn Any;
    fn as_any_mut(&mut self) -> &mut dyn Any;
}
