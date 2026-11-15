//! The art resources (`wce_updater::art`): a game's git repository whose working tree is the projects' `raw_data/art` (Settings > Game > Art
//! resources). Help > Update Manager sets it up, tells what the team pushed and brings it in; File > Publish art resources..., the status
//! bar's badge and the Update Manager's button open the publish window (the files changed, a message). The git work runs on a thread, one job
//! at a time; a file changed both here and by the team is resolved by a choice per file (keep mine, take theirs) before anything is merged.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::mpsc::{Receiver, TryRecvError, channel};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use wce_foundation::log;
use wce_updater::art::{self, Change, ChangeKind, Choice, RepoInfo};

use crate::editor::Editor;
use crate::{gui, icons, theme};

/// How often the local changes are looked at again (no network: `git status`), for the badge and the menu.
const POLL: Duration = Duration::from_secs(10);

/// The publish window's title.
const PUBLISH_TITLE: &str = "Publish art resources";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Op {
    /// Local state only (the poll).
    Status,

    /// Asks the remote too.
    Check,
    Setup,
    Update,
    Publish,
}

impl Op {
    fn doing(self) -> &'static str {
        match self {
            Self::Status => "Looking at the local changes",
            Self::Check => "Asking the repository for new commits",
            Self::Setup => "Setting up the art folder",
            Self::Update => "Bringing the team's commits in",
            Self::Publish => "Publishing",
        }
    }
}

/// What a job ends with: its outcome (a message, or why not), and the repository's state after it.
struct Outcome {
    result: Result<Option<String>, art::Error>,
    info: Option<RepoInfo>,
}

struct Job {
    op: Op,
    rx: Receiver<Outcome>,

    /// git's last progress line.
    line: Arc<Mutex<String>>,

    /// The folder it works on (an outcome for another one, the game switched, is dropped).
    dir: PathBuf,
}

/// What to do again once the files changed on both sides have their choices.
#[derive(Clone)]
enum Pending {
    Update,
    Publish { files: Vec<String>, message: String },
}

/// The publish window.
#[derive(Default)]
struct PublishWindow {
    open: bool,

    /// The local changes ticked to publish (all of them at first).
    ticked: BTreeMap<String, bool>,
    message: String,

    /// Who the commit is by, asked for when git has no name or e-mail.
    name: String,
    email: String,
    needs_identity: bool,
}

#[derive(Default)]
pub(crate) struct ArtRepo {
    job: Option<Job>,

    /// Asked for while the poll was running: started when it ends.
    queued: Option<Op>,

    /// The state the last job found, and the folder it is of.
    info: Option<RepoInfo>,
    info_dir: PathBuf,

    /// When the remote was last asked.
    checked: Option<Instant>,
    polled: Option<Instant>,

    /// The last outcome to show (and whether it is an error).
    message: Option<(String, bool)>,

    /// Files changed both here and by the team, their choices, and what waits for them.
    conflicts: Vec<String>,
    choices: BTreeMap<String, Choice>,
    pending: Option<Pending>,

    /// The check was asked for at the start: the Update Manager opens only when the team pushed something.
    quiet: bool,

    publish: PublishWindow,

    /// The installed git (looked for once).
    git: Option<Result<String, String>>,
}

impl ArtRepo {
    /// The publish is a write: quitting waits for it.
    pub(crate) fn writing(&self) -> Option<&'static str> {
        self.job.as_ref().filter(|j| matches!(j.op, Op::Publish | Op::Update | Op::Setup)).map(|_| "the art resources' git work")
    }
}

impl Editor {
    /// The game's art resources repository (none set: empty).
    fn art_url(&self) -> String {
        self.settings_ref().art_repo().0.to_owned()
    }

    fn art_dir(&self) -> PathBuf {
        self.settings_ref().art_dir()
    }

    /// The art folder is the repository's working tree.
    pub(crate) fn art_set_up(&self) -> bool {
        !self.art_url().is_empty() && art::is_repo(&self.art_dir())
    }

    /// The installed git, or why not (looked for once).
    fn art_git(&mut self) -> Result<String, String> {
        self.art.git.get_or_insert_with(art::git_version).clone()
    }

    /// The state known now, when it is the current game's folder's.
    fn art_info(&self) -> Option<&RepoInfo> {
        self.art.info.as_ref().filter(|_| self.art.info_dir == self.art_dir())
    }

    /// Starts `op` on a thread, unless one is running.
    fn art_start(&mut self, op: Op) {
        if self.art_url().is_empty() || self.art_git().is_err() {
            return;
        }

        if let Some(job) = &self.art.job {
            if job.op == Op::Status && op != Op::Status {
                self.art.queued = Some(op);
            }

            return;
        }

        let (url, branch) = self.settings_ref().art_repo();
        let (url, branch) = (url.to_owned(), branch.to_owned());
        let dir = self.art_dir();
        let backup = art::backup_dir(&self.settings_ref().projects_dir());
        // The choices go with the update or the publish they are for (the poll leaves them for the one asked).
        let choices = if matches!(op, Op::Update | Op::Publish) { std::mem::take(&mut self.art.choices) } else { BTreeMap::new() };
        let pending = self.art.pending.clone();
        let line = Arc::new(Mutex::new(String::new()));
        let (tx, rx) = channel();
        let (shared, at) = (line.clone(), dir.clone());
        std::thread::spawn(move || {
            let progress = move |l: &str| *shared.lock().unwrap_or_else(std::sync::PoisonError::into_inner) = l.to_owned();
            let result = match op {
                Op::Status => Ok(None),
                Op::Check => art::info(&at, true, &progress).map(|_| None).map_err(art::Error::Failed),
                Op::Setup => art::setup(&at, &url, Some(&branch), &backup, &progress).map(Some).map_err(art::Error::Failed),
                Op::Update => art::update(&at, &choices).map(Some),
                Op::Publish => match pending {
                    Some(Pending::Publish { files, message }) => art::publish(&at, &files, &message, &choices, &progress).map(Some),
                    _ => Ok(None),
                },
            };

            let info = art::info(&at, false, &|_| {}).ok();
            let _ = tx.send(Outcome { result, info });
        });

        if op == Op::Check {
            self.art.checked = Some(Instant::now());
        }

        self.art.polled = Some(Instant::now());
        self.art.job = Some(Job { op, rx, line, dir });
    }

    /// At the start (Settings > General > Check for updates at start): asks the repository for new commits; the Update Manager opens when
    /// there are some.
    pub(crate) fn art_at_start(&mut self) {
        if self.art_set_up() {
            self.art.quiet = true;
            self.art_start(Op::Check);
        }
    }

    /// The Update Manager's check (and Check again).
    pub(crate) fn art_check(&mut self) {
        self.art.quiet = false;
        if self.art_set_up() {
            self.art_start(Op::Check);
        }
    }

    /// The settings changed: a new repository is offered in the Update Manager, an outcome about the old one goes.
    pub(crate) fn art_settings_applied(&mut self, before_url: &str) {
        let url = self.art_url();
        if url == before_url {
            return;
        }

        self.art.info = None;
        self.art.message = None;
        self.art.conflicts.clear();
        self.art.pending = None;
        if !url.is_empty() {
            self.check_for_updates(true);
        }
    }

    /// Per frame: takes a job's outcome, and looks at the local changes again now and then.
    pub(crate) fn poll_art(&mut self) {
        if let Some(job) = &self.art.job {
            match job.rx.try_recv() {
                Ok(outcome) => {
                    let job = self.art.job.take().expect("a job");
                    self.art_finished(job, outcome);
                    if let Some(op) = self.art.queued.take() {
                        self.art_start(op);
                    }
                }

                Err(TryRecvError::Empty) => {}
                Err(TryRecvError::Disconnected) => {
                    self.art.job = None;
                    self.art.message = Some(("the git work stopped".to_owned(), true));
                }
            }

            return;
        }

        if self.art_set_up() && self.art.polled.is_none_or(|t| t.elapsed() >= POLL) {
            self.art_start(Op::Status);
        }
    }

    fn art_finished(&mut self, job: Job, outcome: Outcome) {
        if job.dir != self.art_dir() {
            return;
        }

        let changed_files = matches!(job.op, Op::Setup | Op::Update | Op::Publish);
        if let Some(info) = outcome.info {
            self.art.info = Some(info);
            self.art.info_dir = job.dir;
        }

        match outcome.result {
            Ok(msg) => {
                if job.op != Op::Status {
                    self.art.conflicts.clear();
                    self.art.pending = None;
                }

                if let Some(m) = msg {
                    log::info(format!("Art resources: {m}"));
                    self.status(&format!("Art resources: {m}"), false);
                    self.art.message = Some((m, false));
                }

                if job.op == Op::Publish {
                    self.art.publish = PublishWindow::default();
                }

                let behind = self.art_info().map_or(0, |i| i.behind);
                if job.op == Op::Check && self.art.quiet && behind > 0 {
                    log::info(format!("Art resources: {behind} new commit(s) from the team"));
                    self.updates.show(true);
                }
            }

            Err(art::Error::Conflicts(files)) => {
                self.art.conflicts = files;
                self.art.pending =
                    Some(if job.op == Op::Publish { self.art.pending.clone().unwrap_or(Pending::Update) } else { Pending::Update });
                if !self.art.publish.open {
                    self.updates.show(true);
                }
            }

            Err(art::Error::Failed(e)) => {
                log::warn(format!("Art resources: {e}"));
                if !(job.op == Op::Check && self.art.quiet) {
                    self.art.message = Some((e, true));
                }

                if job.op == Op::Publish {
                    self.art.pending = None;
                }
            }
        }

        self.art.quiet = false;

        // New files in the art folder: the tools read their prefabs, vegetation sets and models again.
        if changed_files {
            for i in 0..self.tool_count() {
                self.with_tool_at(i, |t, e| t.art_updated(e));
            }
        }
    }

    // ---- the Update Manager's section

    pub(crate) fn draw_art_updates(&mut self) {
        let url = self.art_url();
        let dir = self.art_dir();
        gui::push_text_wrap_pos(580.0);
        if url.is_empty() {
            gui::text_disabled(&format!(
                "No repository for {}. Settings > Game > Art resources sets one: a team's models, CS2 files and prefabs, shared in {}.",
                self.game().display_name,
                dir.display()
            ));
            gui::pop_text_wrap_pos();
            return;
        }

        if let Err(e) = self.art_git() {
            gui::text_colored(theme::error(), &format!("{}  {e}", icons::CIRCLE_X));
            gui::pop_text_wrap_pos();
            return;
        }

        let set_up = art::is_repo(&dir);
        let branch = self.art_info().map(|i| i.branch.clone()).unwrap_or_default();
        gui::text(&format!("{url}{}", if branch.is_empty() { String::new() } else { format!(" ({branch})") }));
        gui::text_disabled(&format!("in {}", dir.display()));
        let busy = self.art.job.as_ref().filter(|j| j.op != Op::Status).map(|j| {
            let line = j.line.lock().map(|l| l.clone()).unwrap_or_default();
            (j.op.doing(), line)
        });
        if let Some((doing, line)) = busy {
            gui::spinner(7.0, 2.0, theme::dim());
            gui::same_line();
            gui::text(&if line.is_empty() { format!("{doing}...") } else { format!("{doing}: {line}") });
        } else if !set_up {
            gui::text("Not set up: the art folder is not this repository's yet.");
            if gui::button(&format!("{}  Set up", icons::DOWNLOAD)) {
                self.art.message = None;
                self.art_start(Op::Setup);
            }

            if gui::is_item_hovered() {
                gui::set_tooltip(
                    "Downloads the repository into the art folder. The files there stay: those the repository lacks become your changes, to publish; those it has differently are backed up first (<projects>/art_backup), then replaced by the team's.",
                );
            }
        } else if !self.art.conflicts.is_empty() {
            self.draw_art_conflicts();
        } else if let Some(info) = self.art_info().cloned() {
            if info.behind > 0 {
                gui::text(&format!("{} new commit(s) from the team, {} file(s) changed.", info.behind, info.incoming.len()));
                if gui::button(&format!("{}  Update", icons::DOWNLOAD)) {
                    self.art.message = None;
                    self.art_start(Op::Update);
                }

                if gui::is_item_hovered() {
                    gui::set_tooltip(
                        "Brings the team's files into the art folder. Your changes stay; a file changed by both asks which one to keep.",
                    );
                }
            } else {
                let when = self.art.checked.map_or_else(String::new, |t| format!(" (checked {} s ago)", t.elapsed().as_secs()));
                gui::text(&format!("{}  Up to date{when}.", icons::CIRCLE_CHECK));
            }

            let local = info.changes.len();
            if local > 0 || info.ahead > 0 {
                if local > 0 {
                    gui::text(&format!("{local} local change(s) not published."));
                } else {
                    gui::text(&format!("{} commit(s) not published yet.", info.ahead));
                }

                if gui::button(&format!("{}  Publish...", icons::ARROW_UP)) {
                    self.updates.show(false);
                    self.open_art_publish();
                }
            }
        }

        if let Some((m, error)) = &self.art.message {
            if *error {
                gui::text_colored(theme::error(), m);
            } else {
                gui::text_colored(theme::dim(), m);
            }
        }

        gui::pop_text_wrap_pos();
    }

    /// The files changed both here and by the team: a choice for each, then the update (or publish) goes on.
    fn draw_art_conflicts(&mut self) {
        gui::text_colored(
            theme::warning(),
            &format!("{}  These files were changed both by you and by the team. Which version stays?", icons::TRIANGLE_ALERT),
        );
        for (i, path) in self.art.conflicts.clone().iter().enumerate() {
            gui::push_id_int(i as i32);
            let choice = self.art.choices.get(path).copied();
            if gui::radio_button("Keep mine", choice == Some(Choice::Mine)) {
                self.art.choices.insert(path.clone(), Choice::Mine);
            }

            if gui::is_item_hovered() {
                gui::set_tooltip("Your file stays, over the team's: publish it to share it.");
            }

            gui::same_line();
            if gui::radio_button("Take theirs", choice == Some(Choice::Theirs)) {
                self.art.choices.insert(path.clone(), Choice::Theirs);
            }

            if gui::is_item_hovered() {
                gui::set_tooltip("The team's file replaces yours: your change to it is lost.");
            }

            gui::same_line();
            gui::text(path);
            gui::pop_id();
        }

        let all = self.art.conflicts.iter().all(|p| self.art.choices.contains_key(p));
        gui::begin_disabled(!all);
        if gui::button("Continue") {
            let op = if matches!(self.art.pending, Some(Pending::Publish { .. })) { Op::Publish } else { Op::Update };
            self.art.conflicts.clear();
            self.art_start(op);
        }

        gui::end_disabled();
        gui::same_line();
        if gui::button("Cancel##conflicts") {
            self.art.conflicts.clear();
            self.art.choices.clear();
            self.art.pending = None;
        }
    }

    // ---- publishing

    /// File > Publish art resources..., the badge, the Update Manager: the publish window with every local change ticked.
    pub(crate) fn open_art_publish(&mut self) {
        let dir = self.art_dir();
        let changes = self.art_info().map(|i| i.changes.clone()).unwrap_or_default();
        let identity = art::identity(&dir);
        let w = &mut self.art.publish;
        w.open = true;
        w.ticked = changes.iter().map(|c| (c.path.clone(), w.ticked.get(&c.path).copied().unwrap_or(true))).collect();
        w.needs_identity = identity.is_none();
        if let Some((name, email)) = identity {
            (w.name, w.email) = (name, email);
        }

        self.art.message = None;
    }

    /// Something to publish: local changes, or commits a publish left (it stopped before the push).
    pub(crate) fn art_publishable(&self) -> bool {
        self.art_set_up() && self.art_info().is_some_and(|i| !i.changes.is_empty() || i.ahead > 0)
    }

    pub(crate) fn draw_art_publish(&mut self) {
        if !self.art.publish.open {
            return;
        }

        gui::open_popup(PUBLISH_TITLE);
        let vp = gui::main_viewport();
        gui::set_next_window_pos(gui::v2(vp.Pos.x + vp.Size.x * 0.5, vp.Pos.y + vp.Size.y * 0.5), gui::COND_APPEARING, gui::v2(0.5, 0.5));
        gui::set_next_window_size(gui::v2(640.0, 0.0), 0);
        let mut open = true;
        if !gui::begin_popup_modal(PUBLISH_TITLE, Some(&mut open), gui::WINDOW_ALWAYS_AUTO_RESIZE) {
            return;
        }

        let info = self.art_info().cloned().unwrap_or_default();
        let busy = self
            .art
            .job
            .as_ref()
            .filter(|j| j.op != Op::Status)
            .map(|j| (j.op.doing(), j.line.lock().map(|l| l.clone()).unwrap_or_default()));
        gui::push_text_wrap_pos(620.0);
        gui::text_disabled(&format!(
            "To {} ({}). The team's newest files are brought in first.",
            info.remote.clone().unwrap_or_default(),
            info.branch
        ));

        // The files: kept in step with the local changes (a save while the window is open adds one, ticked).
        let changes = info.changes.clone();
        let w = &mut self.art.publish;
        w.ticked.retain(|p, _| changes.iter().any(|c| &c.path == p));
        for c in &changes {
            w.ticked.entry(c.path.clone()).or_insert(true);
        }

        if !self.art.conflicts.is_empty() {
            self.draw_art_conflicts();
        } else if let Some((doing, line)) = busy {
            gui::spinner(7.0, 2.0, theme::dim());
            gui::same_line();
            gui::text(&if line.is_empty() { format!("{doing}...") } else { format!("{doing}: {line}") });
        } else {
            self.draw_art_publish_form(&changes, info.ahead);
        }

        if let Some((m, true)) = &self.art.message {
            gui::text_colored(theme::error(), m);
        }

        gui::pop_text_wrap_pos();
        gui::separator();
        let ticked = self.art.publish.ticked.values().filter(|t| **t).count();
        let w = &self.art.publish;
        let ready = (ticked > 0 || info.ahead > 0)
            && !w.message.trim().is_empty()
            && (!w.needs_identity || (!w.name.trim().is_empty() && w.email.contains('@')))
            && self.art.job.as_ref().is_none_or(|j| j.op == Op::Status)
            && self.art.conflicts.is_empty();
        gui::begin_disabled(!ready);
        if gui::button(&format!("{}  Publish", icons::ARROW_UP)) {
            self.start_art_publish();
        }

        gui::end_disabled();
        gui::same_line();
        if gui::button("Close") || !open {
            // The work goes on (the status line tells its outcome); the message and the ticks are kept for the next time.
            self.art.publish.open = false;
            gui::close_current_popup();
        }

        gui::end_popup();
    }

    fn draw_art_publish_form(&mut self, changes: &[Change], ahead: usize) {
        let w = &mut self.art.publish;
        if changes.is_empty() {
            gui::text(&if ahead > 0 {
                format!("No local changes; {ahead} commit(s) not published yet.")
            } else {
                "No local changes.".to_owned()
            });
        } else {
            gui::text(&format!("{} changed file(s):", changes.len()));
            gui::same_line();
            if gui::small_button("All") {
                w.ticked.values_mut().for_each(|t| *t = true);
            }

            gui::same_line();
            if gui::small_button("None") {
                w.ticked.values_mut().for_each(|t| *t = false);
            }

            let height = (changes.len() as f32 * gui::frame_height_with_spacing() + 8.0).min(260.0);
            if gui::begin_child("##art_changes", gui::v2(0.0, height), gui::CHILD_BORDERS, 0) {
                for (i, c) in changes.iter().enumerate() {
                    let (mark, colour) = match c.kind {
                        ChangeKind::Added => ("+", theme::success()),
                        ChangeKind::Modified => ("~", theme::warning()),
                        ChangeKind::Deleted => ("-", theme::error()),
                    };
                    let on = w.ticked.entry(c.path.clone()).or_insert(true);
                    gui::checkbox(&format!("##tick{i}"), on);
                    gui::same_line();
                    gui::text_colored(colour, mark);
                    gui::same_line();
                    gui::text(&c.path);
                }
            }

            gui::end_child();
        }

        gui::text("What changed:");
        gui::input_text_multiline("##art_message", &mut w.message, gui::v2(-1.0, gui::frame_height() * 3.0), 0);
        if w.needs_identity {
            gui::text_colored(
                theme::warning(),
                "git does not know who you are yet: your name and e-mail sign your commits (kept in this repository's settings).",
            );
            crate::settings_window::label("Name");
            gui::set_next_item_width(-1.0);
            gui::input_text("##art_name", None, &mut w.name, 256, 0);
            crate::settings_window::label("E-mail");
            gui::set_next_item_width(-1.0);
            gui::input_text("##art_email", None, &mut w.email, 256, 0);
        }
    }

    fn start_art_publish(&mut self) {
        let dir = self.art_dir();
        let w = &self.art.publish;
        if w.needs_identity {
            if let Err(e) = art::set_identity(&dir, &w.name, &w.email) {
                self.art.message = Some((e, true));
                return;
            }

            self.art.publish.needs_identity = false;
        }

        let w = &self.art.publish;
        let files = w.ticked.iter().filter(|(_, t)| **t).map(|(p, _)| p.clone()).collect::<Vec<_>>();
        self.art.pending = Some(Pending::Publish { files, message: w.message.clone() });
        self.art.message = None;
        self.art_start(Op::Publish);
    }

    // ---- the status bar and the menu

    /// The status bar's badge: the local changes and the team's new commits, when there are any; its width.
    pub(crate) fn art_badge(&self) -> Option<String> {
        if !self.art_set_up() {
            return None;
        }

        let info = self.art_info()?;
        let mut parts = vec![];
        if !self.art.conflicts.is_empty() {
            parts.push(format!("{} to resolve", self.art.conflicts.len()));
        }

        if info.behind > 0 {
            parts.push(format!("{} new", info.behind));
        }

        if !info.changes.is_empty() {
            parts.push(format!("{} changed", info.changes.len()));
        } else if info.ahead > 0 {
            parts.push(format!("{} unpublished", info.ahead));
        }

        (!parts.is_empty()).then(|| format!("{}  art: {}", icons::GIT_COMMIT_HORIZONTAL, parts.join(", ")))
    }

    /// The badge was clicked: the team's commits or a choice first (the Update Manager), else the publish window.
    pub(crate) fn art_badge_clicked(&mut self) {
        let behind = self.art_info().is_some_and(|i| i.behind > 0);
        let to_resolve = !self.art.conflicts.is_empty() && !matches!(self.art.pending, Some(Pending::Publish { .. }));
        if to_resolve || behind {
            self.check_for_updates(true);
        } else {
            self.open_art_publish();
        }
    }
}
