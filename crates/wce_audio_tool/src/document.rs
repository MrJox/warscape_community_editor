//! The Audio tool's edit session as an editor document (docs/plans/audio.md, step 10.10): the edited banks and loose files
//! (`EditSet`) in the editor's command stack, so Edit > Undo / Redo, the History panel, File > Save, the modified mark, autosave's
//! prompt on quitting and Revert work as for the other tools' documents. An edit is a snapshot command (the session before and
//! after, shared, not copied). Save writes the edited banks and files under the project's loose folder (read before the packs)
//! and the names given in the editor to `audio/names.txt`.

use std::any::Any;
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Arc;

use wce_audio::edit::EditSet;
use wce_audio::library::SoundLibrary;
use wce_audio_format::fnv;
use wce_editor_core::command::{Command, CommandStack, History};
use wce_editor_core::document::{Document, DocumentCore, SaveTarget};
use wce_scene::render_scene::{LayerId, RenderScene};

struct Snapshot {
    name: String,
    before: Arc<EditSet>,
    after: Arc<EditSet>,
}

impl Command<EditSet> for Snapshot {
    fn name(&self) -> String {
        self.name.clone()
    }

    fn apply(&mut self, edits: &mut EditSet) {
        *edits = (*self.after).clone();
    }

    fn revert(&mut self, edits: &mut EditSet) {
        *edits = (*self.before).clone();
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

pub struct AudioDocument {
    core: DocumentCore,
    history: CommandStack<EditSet>,
    lib: Arc<SoundLibrary>,
    /// The project's loose folder (files there are read before the packs) and its names file.
    loose_dir: PathBuf,
    names_file: PathBuf,
    /// The pack paths the last Save wrote (the tool tells the editor they changed), taken once.
    saved: Option<Vec<String>>,
}

impl AudioDocument {
    pub fn new(lib: Arc<SoundLibrary>, loose_dir: PathBuf, names_file: PathBuf) -> Self {
        Self { core: DocumentCore::new(), history: CommandStack::new(EditSet::default()), lib, loose_dir, names_file, saved: None }
    }

    pub fn edits(&self) -> &EditSet {
        self.history.model()
    }

    /// An edit: the session becomes `after` (undoable as `name`).
    pub fn push(&mut self, name: &str, after: EditSet) {
        let before = Arc::new(self.history.model().clone());
        self.history.push(Box::new(Snapshot { name: name.to_owned(), before, after: Arc::new(after) }));
    }

    pub fn take_saved(&mut self) -> Option<Vec<String>> {
        self.saved.take()
    }

    fn loose_path(&self, path: &str) -> PathBuf {
        path.split('/').filter(|p| !p.is_empty()).fold(self.loose_dir.clone(), |d, p| d.join(p))
    }

    /// The project's names file with the session's names: a name that hashes to its ID as a line of its own, any other (your
    /// label for an object) as `#<id> <name>`; a label given again replaces the old one.
    fn names_text(&self, old: &str) -> String {
        let mut labels: BTreeMap<u32, String> = BTreeMap::new();
        let mut plain: Vec<String> = vec![];
        for line in old.lines() {
            match parse_label(line) {
                Some((id, name)) => {
                    labels.insert(id, name.to_owned());
                }
                None if !line.trim().is_empty() => plain.push(line.trim().to_owned()),
                None => {}
            }
        }
        for (id, name) in &self.history.model().names {
            if fnv::id(name) == *id {
                if !plain.contains(name) {
                    plain.push(name.clone());
                }
            } else {
                labels.insert(*id, name.clone());
            }
        }
        let mut text = String::new();
        plain.iter().for_each(|n| text.push_str(&format!("{n}\n")));
        labels.iter().for_each(|(id, n)| text.push_str(&format!("#{id} {n}\n")));
        text
    }
}

/// A names file line naming an ID: `#<id> <name>`.
pub fn parse_label(line: &str) -> Option<(u32, &str)> {
    let rest = line.trim().strip_prefix('#')?;
    let (id, name) = rest.split_once(' ')?;
    Some((id.parse().ok()?, name.trim()))
}

impl Document for AudioDocument {
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
        "Audio edits".to_owned()
    }

    fn kind(&self) -> String {
        "audio".to_owned()
    }

    fn modified(&self) -> bool {
        !self.history.model().is_empty()
    }

    fn can_save(&self) -> bool {
        true
    }

    fn revert(&mut self) -> Result<String, String> {
        *self.history.model_mut() = EditSet::default();
        self.history.clear();
        Ok("the audio edits not saved are dropped".to_owned())
    }

    /// Writes the edited banks and files into the project's loose folder (the target is not used: they go where the game reads
    /// them), then starts a new session.
    fn save(&mut self, _target: &SaveTarget) -> Result<(), String> {
        let edits = self.history.model();
        let files: Vec<(String, Vec<u8>)> = edits
            .banks
            .iter()
            .map(|(b, bank)| (self.lib.banks[*b].path.clone(), bank.write()))
            .chain(edits.loose.iter().map(|(p, bytes)| (p.clone(), bytes.clone())))
            .collect();
        let mut written = vec![];
        for (path, bytes) in files {
            let disk = self.loose_path(&path);
            if let Some(dir) = disk.parent() {
                let _ = std::fs::create_dir_all(dir);
            }
            wce_foundation::files::write_replacing(&disk, &bytes).map_err(|err| format!("cannot write {}: {err}", disk.display()))?;
            written.push(path);
        }
        if !edits.names.is_empty() {
            let text = self.names_text(&std::fs::read_to_string(&self.names_file).unwrap_or_default());
            let _ = self.names_file.parent().map(std::fs::create_dir_all);
            wce_foundation::files::write_replacing(&self.names_file, text.as_bytes())
                .map_err(|err| format!("cannot keep the names in {}: {err}", self.names_file.display()))?;
        }
        *self.history.model_mut() = EditSet::default();
        self.history.clear();
        self.saved = Some(written);
        Ok(())
    }

    /// Nothing in the viewport (the Audio tool has none).
    fn publish(&mut self, _scene: &mut RenderScene, _layer: LayerId) {}

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn labels_by_id() {
        assert_eq!(parse_label("#123 my click"), Some((123, "my click")));
        assert_eq!(parse_label("battle_bird_wing_flaps"), None);
        let mut doc = AudioDocument::new(Arc::new(SoundLibrary::default()), PathBuf::new(), PathBuf::new());
        let mut edits = EditSet::default();
        edits.names.insert(fnv::id("my_event"), "my_event".to_owned());
        edits.names.insert(42, "my label".to_owned());
        doc.push("names", edits);
        let text = doc.names_text("old_event\n#42 old label\n#7 kept\n");
        assert_eq!(text, "old_event\nmy_event\n#7 kept\n#42 my label\n");
        assert!(doc.history().can_undo());
        doc.history_mut().undo();
        assert!(doc.edits().is_empty());
    }
}
