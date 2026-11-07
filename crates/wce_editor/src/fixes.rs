//! The Fixes menu: one-time jobs that repair something of the game rather than edit a document. A fix says what it does in its own window
//! before anything is written, works on a thread (`Fix::apply` can take a minute), and is told apart from an edit by what it leaves: packs
//! in the game's data folder, which it can take out again. A tool offers them (`Tool::fixes`); this knows nothing of what they do.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::thread::JoinHandle;

use wce_foundation::game::GameProfile;

use crate::editor::Editor;
use crate::gui;

/// What a fix works from.
#[derive(Clone)]
pub struct FixSource {
    pub game_dir: PathBuf,
    pub profile: GameProfile,

    /// The chosen mod packs, first listed wins, without the fix's own packs.
    pub mods: Vec<String>,
}

pub trait Fix: Send + Sync {
    /// The name for the `fix` command.
    fn id(&self) -> &str;
    fn title(&self) -> &str;

    /// Whether it applies to a game (its key).
    fn for_game(&self, game: &str) -> bool;

    /// What it does, for the user who has not read any notes: paragraphs, with blank lines between them.
    fn explanation(&self) -> &str;

    /// The packs it writes that go on the mod list, ahead of the mods they were built from.
    fn packs(&self) -> Vec<String>;

    /// Whether its packs are in the data folder.
    fn installed(&self, data: &Path) -> bool;

    /// Does the work; what it did, for the window.
    ///
    /// # Errors
    ///
    /// The work failed: why, for the window.
    fn apply(&self, source: &FixSource) -> Result<String, String>;

    /// Takes out what `apply` wrote.
    ///
    /// # Errors
    ///
    /// It could not: why, for the window.
    fn remove(&self, data: &Path) -> Result<String, String>;
}

struct Job {
    fix: usize,
    applying: bool,
    worker: JoinHandle<Result<String, String>>,
}

#[derive(Default)]
pub(crate) struct Fixes {
    pub(crate) list: Vec<Arc<dyn Fix>>,

    /// The fix whose window shows, and whether the popup has still to open.
    open: Option<usize>,
    opening: bool,
    job: Option<Job>,

    /// How the last job ended: the text, and whether it failed.
    outcome: Option<(String, bool)>,
}

/// `selected` with the fix's packs out, and ahead of the rest if `applied`.
fn mod_list(selected: &[String], own: &[String], applied: bool) -> Vec<String> {
    let rest = selected.iter().filter(|n| !own.iter().any(|o| o.eq_ignore_ascii_case(n))).cloned();
    if applied { own.iter().cloned().chain(rest).collect() } else { rest.collect() }
}

impl Editor {
    fn fix_data_folder(&self) -> Option<PathBuf> {
        Some(self.settings_ref().game_dir()?.join(self.game().data_dir))
    }

    /// The fix's index, by its name.
    fn fix_named(&self, id: &str) -> Option<usize> {
        let game = self.game().key;
        self.fixes.list.iter().position(|f| f.id() == id && f.for_game(game))
    }

    fn fix_worker(&self, fix: usize, applying: bool) -> Result<impl FnOnce() -> Result<String, String> + Send + 'static, String> {
        let game_dir = self.settings_ref().game_dir().ok_or("the game folder is not set")?;
        let data = self.fix_data_folder().ok_or("the game folder is not set")?;
        let f = self.fixes.list[fix].clone();
        let source = FixSource { game_dir, profile: self.game().clone(), mods: mod_list(&self.selected_mod_packs(), &f.packs(), false) };
        Ok(move || if applying { f.apply(&source) } else { f.remove(&data) })
    }

    /// A job ended: its packs go on the mod list or off it, and the files are mounted again.
    fn fix_finished(&mut self, fix: usize, applying: bool, result: Result<String, String>) -> Result<String, String> {
        let done = result?;
        let names = mod_list(&self.selected_mod_packs(), &self.fixes.list[fix].packs(), applying);
        self.set_mod_packs(&names)?;
        self.activate_all_tools();
        Ok(done)
    }

    /// Does a fix's work here, for a script.
    ///
    /// # Errors
    ///
    /// No such fix for this game, or it failed.
    pub fn run_fix(&mut self, id: &str, applying: bool) -> Result<String, String> {
        let fix = self.fix_named(id).ok_or(format!("no fix {id} for this game"))?;
        let work = self.fix_worker(fix, applying)?;
        self.fix_finished(fix, applying, work())
    }

    /// Opens a fix's window.
    pub fn open_fix(&mut self, id: &str) -> bool {
        self.fixes.open = self.fix_named(id);
        self.fixes.opening = self.fixes.open.is_some();
        self.fixes.outcome = None;
        self.fixes.open.is_some()
    }

    /// The names of the fixes for this game.
    #[must_use]
    pub fn fix_names(&self) -> Vec<String> {
        let game = self.game().key;
        self.fixes.list.iter().filter(|f| f.for_game(game)).map(|f| f.id().to_owned()).collect()
    }

    fn start_fix(&mut self, fix: usize, applying: bool) {
        self.fixes.outcome = None;
        match self.fix_worker(fix, applying) {
            Ok(work) => self.fixes.job = Some(Job { fix, applying, worker: std::thread::spawn(work) }),
            Err(why) => self.fixes.outcome = Some((why, true)),
        }
    }

    fn poll_fix(&mut self) {
        if !self.fixes.job.as_ref().is_some_and(|j| j.worker.is_finished()) {
            return;
        }

        let Some(job) = self.fixes.job.take() else { return };
        let result = job.worker.join().unwrap_or_else(|_| Err("the work stopped: see the log".to_owned()));
        let done = self.fix_finished(job.fix, job.applying, result);
        self.fixes.outcome = Some(match done {
            Ok(text) => (text, false),
            Err(why) => (why, true),
        });
    }

    /// The menu, in the menu bar: this game's fixes.
    pub(crate) fn draw_fixes_menu(&mut self) {
        if self.fixes.list.is_empty() || !gui::begin_menu("Fixes") {
            return;
        }

        let game = self.game().key;
        let items = self.fixes.list.iter().filter(|f| f.for_game(game)).map(|f| (f.id().to_owned(), f.title().to_owned())).collect::<Vec<_>>();
        let data = self.fix_data_folder();
        for (id, title) in &items {
            let on = data.as_deref().zip(self.fix_named(id)).is_some_and(|(d, i)| self.fixes.list[i].installed(d));
            if gui::menu_item(&format!("{title}..."), None, on, data.is_some()) {
                self.open_fix(id);
            }
        }

        if items.is_empty() {
            gui::menu_item("Nothing for this game", None, false, false);
        }

        gui::end_menu();
    }

    /// The window of the open fix.
    pub(crate) fn draw_fix_window(&mut self) {
        self.poll_fix();
        let Some(fix) = self.fixes.open else { return };
        let f = self.fixes.list[fix].clone();
        let name = format!("{}##fix", f.title());
        if std::mem::take(&mut self.fixes.opening) {
            gui::open_popup(&name);
        }

        gui::set_next_window_size(gui::v2(620.0, 0.0), gui::COND_ALWAYS);
        if !gui::begin_popup_modal(&name, None, 0) {
            return;
        }

        for paragraph in f.explanation().split("\n\n") {
            gui::text_wrapped(paragraph);
            gui::spacing();
        }

        gui::separator();
        gui::spacing();
        let installed = self.fix_data_folder().is_some_and(|d| f.installed(&d));
        let working = self.fixes.job.is_some();
        match &self.fixes.outcome {
            _ if working => gui::text_disabled("Working..."),
            Some((text, true)) => {
                gui::push_style_color(gui::COL_TEXT, crate::theme::v4(crate::theme::error()));
                gui::text_wrapped(text);
                gui::pop_style_color(1);
            }
            Some((text, false)) => gui::text_wrapped(text),
            None => gui::text_disabled(if installed { "Installed." } else { "Not installed." }),
        }

        gui::spacing();
        gui::begin_disabled(working);
        if gui::button(if installed { "Build again" } else { "Build and install" }) {
            self.start_fix(fix, true);
        }

        gui::same_line();
        gui::begin_disabled(!installed);
        if gui::button("Remove") {
            self.start_fix(fix, false);
        }

        gui::end_disabled();
        gui::same_line();
        if gui::button("Close") {
            self.fixes.open = None;
            gui::close_current_popup();
        }

        gui::end_disabled();
        gui::end_popup();
    }
}

#[cfg(test)]
mod tests {
    use super::mod_list;

    #[test]
    fn its_packs_go_first_and_only_once() {
        let names = |v: &[&str]| v.iter().map(|s| (*s).to_owned()).collect::<Vec<_>>();
        let (selected, own) = (names(&["a.pack", "Fix.pack", "b.pack"]), names(&["fix.pack"]));
        assert_eq!(mod_list(&selected, &own, true), ["fix.pack", "a.pack", "b.pack"]);
        assert_eq!(mod_list(&selected, &own, false), ["a.pack", "b.pack"]);
    }
}
