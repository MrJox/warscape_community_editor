//! The Cinematic tool's document: a scene project (`<projects>/cinematics/<key>/project.json`) in the editor's command stack, so
//! Edit > Undo / Redo, History, File > Save and the unsaved prompts work as in the other tools. An edit is a snapshot (the project
//! before and after, shared).

use std::any::Any;
use std::path::PathBuf;
use std::sync::Arc;

use wce_cinematic::scene::Project;
use wce_editor_core::command::{Command, CommandStack, History};
use wce_editor_core::document::{Document, DocumentCore, SaveTarget};
use wce_scene::render_scene::{LayerId, RenderScene};

struct Snapshot {
    name: String,
    before: Arc<Project>,
    after: Arc<Project>,
}

impl Command<Project> for Snapshot {
    fn name(&self) -> String {
        self.name.clone()
    }
    fn apply(&mut self, p: &mut Project) {
        *p = (*self.after).clone();
    }
    fn revert(&mut self, p: &mut Project) {
        *p = (*self.before).clone();
    }
    fn as_any(&self) -> &dyn Any {
        self
    }
}

pub struct SceneDocument {
    core: DocumentCore,
    history: CommandStack<Project>,
    path: PathBuf,
}

impl SceneDocument {
    pub fn new(project: Project, path: PathBuf) -> Self {
        Self { core: DocumentCore::new(), history: CommandStack::new(project), path }
    }

    /// # Errors
    ///
    /// The file can't be read or is not a project.
    pub fn open(path: PathBuf) -> Result<Self, String> {
        let text = std::fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()))?;
        let p = Project::from_json(&text).map_err(|e| format!("{}: {e}", path.display()))?;
        let mut d = Self::new(p, path);
        d.history.mark_saved();
        Ok(d)
    }

    pub fn project(&self) -> &Project {
        self.history.model()
    }

    /// An edit: the project becomes `after` (sorted), undoable as `name`. Nothing when it is the same.
    pub fn push(&mut self, name: &str, mut after: Project) {
        after.normalize();
        if &after == self.history.model() {
            return;
        }
        let before = Arc::new(self.history.model().clone());
        self.history.push(Box::new(Snapshot { name: name.to_owned(), before, after: Arc::new(after) }));
    }
}

impl Document for SceneDocument {
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
        self.project().name.clone()
    }
    fn kind(&self) -> String {
        "cinematic".to_owned()
    }
    fn can_save(&self) -> bool {
        true
    }
    fn save(&mut self, target: &SaveTarget) -> Result<(), String> {
        let to = if target.path.as_os_str().is_empty() { self.path.clone() } else { target.path.clone() };
        if let Some(dir) = to.parent() {
            std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
        }
        wce_foundation::files::write_replacing(&to, self.project().to_json().as_bytes()).map_err(|e| format!("{}: {e}", to.display()))?;
        if wce_editor_core::document::same_path(&to, &self.path) {
            self.history.mark_saved();
        }
        Ok(())
    }
    fn file_path(&self) -> Option<PathBuf> {
        Some(self.path.clone())
    }
    fn source_key(&self) -> String {
        self.path.display().to_string().replace('\\', "/").to_lowercase()
    }
    fn opened_from(&self, path: &str) -> bool {
        wce_editor_core::document::same_path(std::path::Path::new(path), &self.path)
    }
    /// The 3D preview draws the scene itself (the tool), not through the document's layer.
    fn publish(&mut self, _scene: &mut RenderScene, _layer: LayerId) {}
    fn as_any(&self) -> &dyn Any {
        self
    }
    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}
