//! Autosave (docs/plans/autosave.md): every few minutes, a copy of each modified document (and of the tools' unsaved work, the weather
//! database) in `<projects>/autosave/`, never over the project's files; offered back when the editor next starts after a session that did
//! not end cleanly. The folder's layout is `wce_editor_core::autosave`'s; what a copy holds is each document's (`Document::autosave`).

use std::collections::HashMap;
use std::path::PathBuf;
use std::time::Instant;

use wce_editor_core::autosave::{self, AutosaveMeta, Found, Session};
use wce_foundation::log;
use wce_scene::render_scene::DocumentId;

use crate::editor::{Editor, PromptKind};
use crate::gui;

/// What autosave keeps between frames.
pub struct Autosaver {
    /// This editor's session folder, made at the first copy.
    session: Option<Session>,

    /// When copies were last made (or the editor started).
    last: Instant,

    /// Per document: the stamp and key of its last copy (`Document::autosave_stamp`, `AutosaveMeta::key`).
    copied: HashMap<DocumentId, (u64, String)>,

    /// Per tool: the key of its work's copy (`Tool::autosave_work`).
    work: HashMap<String, String>,

    /// The last copy failed and said so: the next failure is not said again until one works.
    failing: bool,

    recovery: Recovery,
}

impl Default for Autosaver {
    fn default() -> Self {
        Self {
            session: None,
            last: Instant::now(),
            copied: HashMap::new(),
            work: HashMap::new(),
            failing: false,
            recovery: Recovery::default(),
        }
    }
}

/// The Recover autosaved work dialog.
#[derive(Default)]
struct Recovery {
    /// The autosave folder looked at for a gone session's copies (once a folder, at start or a game switch).
    checked: Option<PathBuf>,
    found: Vec<Found>,

    /// Per document found: the copy chosen (an index of its copies; the newest by default).
    picks: Vec<usize>,
    open: bool,
    error: String,
}

/// How long ago a copy was written, said shortly.
fn ago(secs: u64) -> String {
    let d = autosave::now_secs().saturating_sub(secs);
    match d {
        0..60 => "just now".to_owned(),
        60..3600 => format!("{} min ago", d / 60),
        3600..86_400 => format!("{} h {} min ago", d / 3600, d % 3600 / 60),
        _ => format!("{} days ago", d / 86_400),
    }
}

impl Editor {
    /// The autosave folder of the current game's projects.
    pub fn autosave_root(&self) -> PathBuf {
        self.projects_dir().join(autosave::FOLDER)
    }

    /// Once a frame: drops the copies of documents saved or closed since, and makes new copies when the interval is up and nothing is
    /// under way (an interactive session only: scripts use `autosave_now`).
    pub(crate) fn autosave_tick(&mut self) {
        self.autosave_clean_up();
        let a = &self.settings_ref().autosave;
        if !a.enabled || !self.interactive() || self.autosaver.last.elapsed() < a.interval() || !self.autosave_can_run() {
            return;
        }

        self.autosaver.last = Instant::now();
        match self.autosave_run() {
            Ok(_) => self.autosaver.failing = false,
            Err(e) => {
                log::warn(format!("autosave: {e}"));
                if !self.autosaver.failing {
                    self.status(&format!("Autosave failed: {e}"), true);
                }

                self.autosaver.failing = true;
            }
        }
    }

    /// Nothing that a copy would catch half done: no gesture or mouse button held, no prompt open, nothing being written.
    fn autosave_can_run(&self) -> bool {
        self.open_gesture().is_none()
            && self.prompt_kind() == PromptKind::None
            && !(0..3).any(gui::is_mouse_down)
            && self.writes_in_progress().is_empty()
    }

    /// Copies of what changed since its last copy, now. Returns how many were written.
    ///
    /// # Errors
    ///
    /// The copies that failed, one line each (the others were written).
    pub fn autosave_run(&mut self) -> Result<usize, String> {
        let started = Instant::now();
        let root = self.autosave_root();
        let session = match self.autosaver.session.take() {
            Some(s) if s.root() == root => s,
            other => {
                // Another game's projects: its session ends when it has no copies left (its documents were closed by the switch).
                if let Some(old) = other {
                    if old.slots().is_empty() { old.end() } else { log::warn(format!("autosave: {} stays", old.dir().display())) }
                }

                self.autosaver.copied.clear();
                self.autosaver.work.clear();
                Session::start(&root)?
            }
        };

        let keep = self.settings_ref().autosave.kept();
        let (mut written, mut failed) = (0, vec![]);
        for id in self.docs.order().to_vec() {
            let tool = self.docs.tool_of(id);
            let Some(d) = self.docs.find_mut(id) else { continue };
            if !d.modified() {
                continue;
            }

            let Some(meta) = d.autosave_meta() else { continue };
            let meta = AutosaveMeta { tool, ..meta };
            let (stamp, key) = (d.autosave_stamp(), meta.key());
            let last = self.autosaver.copied.get(&id);
            if last == Some(&(stamp, key.clone())) {
                continue;
            }

            let old_key = last.map(|l| l.1.clone()).filter(|k| *k != key);
            match session.write(&meta, keep, |dir| d.autosave(dir)) {
                Ok(_) => {
                    written += 1;
                    self.autosaver.copied.insert(id, (stamp, key));
                    if let Some(k) = old_key {
                        session.remove(&k);
                    }
                }

                Err(e) => failed.push(format!("{}: {e}", meta.title)),
            }
        }

        for i in 0..self.tool_count() {
            let r = self.with_tool_at(i, |t, e| {
                let (meta, changed) = t.autosave_work(e)?;
                let meta = AutosaveMeta { tool: t.id().to_owned(), ..meta };
                Some((meta.key(), changed.then(|| session.write(&meta, keep, |dir| t.write_autosave_work(dir)).map(|_| meta.title))))
            });
            let Some(Some((key, done))) = r else { continue };
            match done {
                Some(Ok(_)) => written += 1,
                Some(Err(e)) => failed.push(e),
                None => {}
            }

            let tool = self.tool_ids()[i].clone();
            if let Some(old) = self.autosaver.work.insert(tool, key.clone()).filter(|k| *k != key) {
                session.remove(&old);
            }
        }

        if written > 0 {
            log::info(format!("autosave: {written} cop{} in {} ms", if written == 1 { "y" } else { "ies" }, started.elapsed().as_millis()));
        }

        self.autosaver.session = Some(session);
        if failed.is_empty() { Ok(written) } else { Err(failed.join("; ")) }
    }

    /// Copies of documents that no longer need them go: saved since (nothing unsaved), closed, or of the tools' work that was saved.
    fn autosave_clean_up(&mut self) {
        let Some(session) = self.autosaver.session.as_ref() else { return };
        let gone = self
            .autosaver
            .copied
            .iter()
            .filter(|(id, _)| self.docs.find(**id).is_none_or(|d| !d.modified()))
            .map(|(id, (_, key))| (*id, key.clone()))
            .collect::<Vec<_>>();
        for (id, key) in gone {
            session.remove(&key);
            self.autosaver.copied.remove(&id);
        }

        if self.autosaver.work.is_empty() {
            return;
        }

        let ids = self.tool_ids();
        let mut done = vec![];
        for (tool, key) in &self.autosaver.work {
            let i = ids.iter().position(|t| t == tool);
            let saved = i.and_then(|i| self.tool_ref(i)).is_none_or(|t| t.autosave_work(self).is_none());
            if saved {
                done.push((tool.clone(), key.clone()));
            }
        }

        let session = self.autosaver.session.as_ref().expect("checked above");
        for (tool, key) in done {
            session.remove(&key);
            self.autosaver.work.remove(&tool);
        }
    }

    /// The editor is closing cleanly (what was unsaved was saved or discarded): its copies go.
    pub fn finish_autosave(&mut self) {
        self.autosaver.copied.clear();
        self.autosaver.work.clear();
        if let Some(s) = self.autosaver.session.take() {
            s.end();
        }
    }

    /// Tests: this session's copies are left as a crash leaves them (its lock goes when the process ends), and a new session starts at
    /// the next copy.
    pub fn abandon_autosave(&mut self) {
        self.autosaver.copied.clear();
        self.autosaver.work.clear();
        if let Some(s) = self.autosaver.session.take() {
            std::mem::forget(s);
        }
    }

    /// What autosave has now, for the `autosave_status` command: its settings and the copies of this session.
    pub fn autosave_status(&self) -> String {
        let a = &self.settings_ref().autosave;
        let mut s = format!(
            "autosave {} every {} min, {} copies kept",
            if a.enabled { "on" } else { "off" },
            a.interval().as_secs() / 60,
            a.kept()
        );
        match &self.autosaver.session {
            Some(session) => {
                for m in session.slots() {
                    s += &format!("; {} ({}): {} cop{}", m.title, m.kind, m.copies.len(), if m.copies.len() == 1 { "y" } else { "ies" });
                }
            }

            None => s += "; no copies this session",
        }

        s
    }

    // ---- recovery

    /// Once per autosave folder (at start, after a game switch) in an interactive session: a gone session's copies open the dialog.
    pub(crate) fn check_autosaves(&mut self) {
        let root = self.autosave_root();
        if self.autosaver.recovery.checked.as_ref() == Some(&root) || !self.interactive() || self.vfs().is_none() {
            return;
        }

        self.autosaver.recovery.checked = Some(root);
        self.open_recovery(false);
    }

    /// Looks for the copies gone sessions left; the dialog opens when there are some (or always, `always`: File > Recover autosaves...).
    pub fn open_recovery(&mut self, always: bool) {
        let found = autosave::stale(&self.autosave_root());
        let r = &mut self.autosaver.recovery;
        r.picks = found.iter().map(|f| f.meta.copies.len().saturating_sub(1)).collect();
        r.found = found;
        r.error.clear();
        r.open = always || !r.found.is_empty();
    }

    /// The copies a gone session left (the dialog's list, for scripts: `autosave_found`).
    pub fn autosave_found(&mut self) -> Vec<Found> {
        autosave::stale(&self.autosave_root())
    }

    /// Takes a copy back into a document (opened modified, its Save writing where it did) and drops the others of that document.
    ///
    /// # Errors
    ///
    /// No tool takes it, the tool's document has unsaved changes, or the copy can't be taken back (it is offered again then).
    pub fn restore_autosave(&mut self, found: &Found, copy: usize) -> Result<String, String> {
        let tool = found.meta.tool.clone();
        let i = self.tool_ids().iter().position(|t| *t == tool).ok_or_else(|| format!("no tool '{tool}' in this editor"))?;
        if let Some(d) = self.docs.of_tool(&tool).and_then(|id| self.docs.find(id)).filter(|d| d.modified()) {
            return Err(format!("{} has unsaved changes: save or close it first", d.title()));
        }

        let root = self.autosave_root();
        let mut session = match self.autosaver.session.take() {
            Some(s) if s.root() == root => s,
            _ => Session::start(&root)?,
        };
        let taken = session.take(found, copy);
        self.autosaver.session = Some(session);
        let dir = taken?;
        let r = self.with_tool_at(i, |t, e| t.restore(e, &dir, &found.meta)).unwrap_or_else(|| Err(format!("{tool} is busy")));
        let r = r.and_then(|doc| match doc {
            Some(doc) => self.replace_document(&tool, doc).map(|_| ()),
            None => Ok(()),
        });
        if let Err(e) = r {
            autosave::give_back(found, copy, &dir);
            return Err(format!("{} not restored: {e}", found.meta.title));
        }

        autosave::discard(found);
        self.set_tool(&tool);
        let message = format!("{} restored from its autosave of {} (save to keep it)", found.meta.title, ago(found.meta.copies[copy].time));
        log::info(&message);
        Ok(message)
    }

    pub(crate) fn draw_recovery(&mut self) {
        const TITLE: &str = "Recover autosaved work";
        if std::mem::take(&mut self.autosaver.recovery.open) {
            gui::open_popup(TITLE);
        }

        if !gui::begin_popup_modal(TITLE, None, gui::WINDOW_ALWAYS_AUTO_RESIZE) {
            return;
        }

        if self.autosaver.recovery.found.is_empty() {
            gui::text("No autosaved work to recover.");
        } else {
            gui::text_wrapped(
                "The editor did not close cleanly last time. These have autosaved copies of their unsaved changes. Restore opens one with \
                 the copy's changes, unsaved: Save writes them into the project.",
            );
        }

        gui::spacing();
        let mut act: Option<(usize, bool)> = None;
        let found = self.autosaver.recovery.found.clone();
        for (n, f) in found.iter().enumerate() {
            gui::push_id_int(n as i32);
            gui::separator();
            gui::text(&format!("{}  ({})", f.meta.title, f.meta.kind));
            gui::text_disabled(&f.meta.target.display().to_string());
            if f.saved_after() {
                gui::text_colored(crate::theme::warning(), "Its file was saved after this copy: the copy may be older than the file.");
            }

            let pick = &mut self.autosaver.recovery.picks[n];
            let label = |c: usize| {
                let newest = if c + 1 == f.meta.copies.len() { ", newest" } else { "" };
                format!("{}{newest}", ago(f.meta.copies[c].time))
            };
            gui::set_next_item_width(220.0);
            if gui::begin_combo("##copy", &label(*pick)) {
                for c in (0..f.meta.copies.len()).rev() {
                    if gui::selectable(&label(c), c == *pick, 0) {
                        *pick = c;
                    }
                }

                gui::end_combo();
            }

            gui::same_line();
            if gui::button("Restore") {
                act = Some((n, true));
            }

            gui::same_line();
            if gui::button("Discard") {
                act = Some((n, false));
            }

            gui::pop_id();
        }

        if let Some((n, restore)) = act {
            let f = found[n].clone();
            let r = if restore {
                self.restore_autosave(&f, self.autosaver.recovery.picks[n])
            } else {
                autosave::discard(&f);
                Ok(format!("{}: autosaved copies discarded", f.meta.title))
            };

            match r {
                Ok(m) => {
                    self.status(&m, false);
                    self.autosaver.recovery.found.remove(n);
                    self.autosaver.recovery.picks.remove(n);
                    self.autosaver.recovery.error.clear();
                }

                Err(e) => self.autosaver.recovery.error = e,
            }
        }

        if !self.autosaver.recovery.error.is_empty() {
            gui::text_colored(crate::theme::warning(), &self.autosaver.recovery.error.clone());
        }

        gui::separator();
        gui::spacing();
        let any = !self.autosaver.recovery.found.is_empty();
        if any && gui::button("Discard all") {
            for f in std::mem::take(&mut self.autosaver.recovery.found) {
                autosave::discard(&f);
            }

            self.autosaver.recovery.picks.clear();
            self.status("autosaved copies discarded", false);
        }

        if any {
            gui::same_line();
        }

        if gui::button(if any { "Later" } else { "Close" }) {
            if any {
                self.status("autosaved copies kept: File > Recover autosaves... offers them again", false);
            }

            gui::close_current_popup();
        }

        gui::end_popup();
    }
}

/// The settings' "Autosave" fields (Settings > General).
pub(crate) fn draw_settings(a: &mut wce_foundation::settings::AutosaveSettings) {
    use wce_foundation::settings::AutosaveSettings as A;
    gui::checkbox("Autosave", &mut a.enabled);
    gui::begin_disabled(!a.enabled);
    let mut minutes = a.interval_minutes as i32;
    gui::set_next_item_width(200.0);
    if gui::slider_int("every (minutes)", &mut minutes, *A::MINUTES.start() as i32, *A::MINUTES.end() as i32) {
        a.interval_minutes = minutes.clamp(*A::MINUTES.start() as i32, *A::MINUTES.end() as i32) as u32;
    }

    let mut keep = a.keep as i32;
    gui::set_next_item_width(200.0);
    if gui::slider_int("copies kept per document", &mut keep, *A::KEEP.start() as i32, *A::KEEP.end() as i32) {
        a.keep = keep.clamp(*A::KEEP.start() as i32, *A::KEEP.end() as i32) as u32;
    }

    gui::end_disabled();
}
