//! The Update Manager (Help > Update Manager, and a quiet check when an interactive session starts): the editor's updates and rpfm's schemas
//! in one window. The editor: the newest release above the running build (`wce_updater`), downloaded with a progress bar and installed over
//! the build, then a restart. The schemas: the newest commit of rpfm's schema repository against the one of the schemas in use, every
//! supported game's schema downloaded into the editor's own schemas folder (`Paths::schemas_dir`, read before rpfm's: `rpfm_schema_path`).
//! The network work runs on threads; the window shows their state each frame.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::sync::mpsc::{Receiver, TryRecvError, channel};

use wce_foundation::log;
use wce_foundation::settings::{db_schemas_dir, downloaded_schemas_dir, rpfm_schema_path};
use wce_updater::schemas;
use wce_updater::{Progress, Release};
use wce_vfs::GameProfile;

use crate::editor::Editor;
use crate::{gui, icons, theme};

/// The editor's own updates.
#[derive(Default)]
enum State {
    #[default]
    Idle,

    /// Asking GitHub.
    Checking { rx: Receiver<Result<Option<Release>, String>> },
    Available(Release),
    UpToDate,

    /// Downloading, then installing.
    Downloading { release: Release, progress: Arc<Progress>, rx: Receiver<Result<(), String>> },
    Installed(Release),
    Failed(String),
}

/// rpfm's schemas.
#[derive(Default)]
enum SchemaState {
    #[default]
    Idle,

    /// Asking GitHub for the repository's newest commit.
    Checking { rx: Receiver<Result<String, String>> },

    /// The newest commit is known.
    Known(String),

    /// Downloading the schemas of a commit (the progress counts files).
    Downloading { commit: String, progress: Arc<Progress>, rx: Receiver<Result<usize, String>> },

    /// Downloaded: the commit, the files written.
    Done(String, usize),
    Failed(String),
}

#[derive(Default)]
pub(crate) struct Updates {
    state: State,
    schemas: SchemaState,

    /// The window is shown.
    open: bool,

    /// The checks were asked for by the user (the window stays open with their outcome), not the quiet one at the start.
    manual: bool,

    /// The executable to start once the editor has quit ("Restart now").
    restart: Option<PathBuf>,
}

impl Updates {
    fn busy(&self) -> bool {
        matches!(self.state, State::Checking { .. } | State::Downloading { .. }) || matches!(self.schemas, SchemaState::Checking { .. } | SchemaState::Downloading { .. })
    }
}

/// Opens a web page in the system's browser.
fn open_url(url: &str) {
    let opener = if cfg!(windows) {
        "explorer"
    } else if cfg!(target_os = "macos") {
        "open"
    } else {
        "xdg-open"
    };

    if let Err(e) = std::process::Command::new(opener).arg(url).spawn() {
        log::error(format!("cannot open {url}: {e}"));
    }
}

fn megabytes(bytes: u64) -> f64 {
    bytes as f64 / (1024.0 * 1024.0)
}

/// Where the current game's schema is read from, and what the editor knows of it.
struct SchemaInUse {
    /// The schema file in use (none: no schema for the game anywhere).
    path: Option<PathBuf>,

    /// Its commit, when its folder tells (the editor's download, rpfm's git checkout).
    commit: Option<String>,

    /// It is not the editor's own folder's, so a download is not what the game reads (Settings > DB schemas, `$WCE_RPFM_SCHEMA`); rpfm's own
    /// folder comes after the editor's, so it does not count.
    overridden: bool,
}

fn schema_in_use(game: &GameProfile) -> SchemaInUse {
    let path = rpfm_schema_path(game);
    let own = downloaded_schemas_dir();
    let dir = path.as_deref().and_then(Path::parent);
    let commit = dir.and_then(schemas::installed);
    let from_settings = path.is_some() && (std::env::var_os("WCE_RPFM_SCHEMA").is_some() || dir.is_some_and(|d| db_schemas_dir().as_deref() == Some(d)));
    SchemaInUse { overridden: from_settings && dir != own.as_deref(), path, commit }
}

/// The schema files of every supported game.
fn schema_files() -> Vec<&'static str> {
    let mut files = GameProfile::ALL.iter().map(|g| g.rpfm_schema).collect::<Vec<_>>();
    files.dedup();
    files
}

impl Editor {
    /// Checks for a newer release and newer schemas on threads. `manual` (Help > Update Manager) shows the window with the outcome; otherwise
    /// (at the start) the window opens only when there is something to do, and a failure only goes to the log.
    pub fn check_for_updates(&mut self, manual: bool) {
        self.updates.open |= manual;
        if self.updates.busy() {
            return;
        }

        self.updates.manual = manual;
        self.check_app(manual || !cfg!(debug_assertions));
        self.check_schemas();
    }

    /// Looks for a newer release (unless an update is installed already and waits for the restart); `really` false leaves it alone (a
    /// debug build's quiet check: its folder is a Cargo target).
    fn check_app(&mut self, really: bool) {
        if !really || matches!(self.updates.state, State::Installed(_)) {
            return;
        }

        let prereleases = self.settings_ref().updates.prereleases;
        let (tx, rx) = channel();
        std::thread::spawn(move || {
            let _ = tx.send(wce_updater::check(&wce_updater::current_version(), prereleases));
        });

        self.updates.state = State::Checking { rx };
    }

    fn check_schemas(&mut self) {
        let (tx, rx) = channel();
        std::thread::spawn(move || {
            let _ = tx.send(schemas::latest());
        });

        self.updates.schemas = SchemaState::Checking { rx };
    }

    /// An interactive session's start: deletes what the last update left, then checks quietly (when the settings ask for it; not under the
    /// first-run dialog). Without the current game's schema the check is made whatever the settings say: features need it.
    pub fn updates_at_start(&mut self) {
        if let Ok(dir) = wce_updater::install_dir() {
            wce_updater::remove_leftovers(&dir);
        }

        if self.first_run {
            return;
        }

        if self.settings_ref().updates.check_on_start {
            self.check_for_updates(false);
        } else if rpfm_schema_path(self.game()).is_none() {
            self.check_schemas();
        }
    }

    /// The executable to start after quitting, when the user chose "Restart now" after an update.
    pub fn restart_after_quit(&self) -> Option<PathBuf> {
        self.updates.restart.clone()
    }

    /// The quit prompt was cancelled: no restart then.
    pub(crate) fn cancel_restart(&mut self) {
        self.updates.restart = None;
    }

    fn start_download(&mut self, release: Release) {
        let Some(asset) = release.asset.clone() else { return };
        let progress = Arc::new(Progress::default());
        let (tx, rx) = channel();
        let shared = progress.clone();
        std::thread::spawn(move || {
            let result = wce_updater::install_dir().and_then(|dir| {
                let zip = std::env::temp_dir().join(&asset.name);
                wce_updater::download(&asset, &zip, &shared)?;
                let installed = wce_updater::install(&zip, &dir);
                let _ = std::fs::remove_file(&zip);
                installed.map(|files| log::info(format!("Update: installed {} files into {}", files.len(), dir.display())))
            });

            let _ = tx.send(result);
        });

        self.updates.state = State::Downloading { release, progress, rx };
    }

    /// Downloads every supported game's schema of `commit` into the editor's schemas folder.
    fn start_schema_download(&mut self, commit: String) {
        let Some(dir) = downloaded_schemas_dir() else {
            self.updates.schemas = SchemaState::Failed("the editor's schemas folder is not known".to_owned());
            return;
        };

        let progress = Arc::new(Progress::default());
        let (tx, rx) = channel();
        let shared = progress.clone();
        let sha = commit.clone();
        std::thread::spawn(move || {
            let result = schemas::download(&dir, &sha, &schema_files(), &shared).map(|files| {
                log::info(format!("Schemas: {} files of rpfm-schemas {} in {}", files.len(), schemas::short(&sha), dir.display()));
                files.len()
            });

            let _ = tx.send(result);
        });

        self.updates.schemas = SchemaState::Downloading { commit, progress, rx };
    }

    /// Takes the threads' outcomes when they are there.
    fn poll_updates(&mut self) {
        let mut installed = None;
        let manual = self.updates.manual;
        let u = &mut self.updates;
        match &u.state {
            State::Checking { rx } => match rx.try_recv() {
                Ok(Ok(Some(release))) => {
                    log::info(format!("Update: {} is available ({})", release.version, release.page));
                    u.state = State::Available(release);
                    u.open = true;
                }

                Ok(Ok(None)) => u.state = State::UpToDate,
                Ok(Err(e)) => {
                    log::warn(format!("Update check: {e}"));
                    u.state = if manual { State::Failed(e) } else { State::Idle };
                }

                Err(TryRecvError::Empty) => {}
                Err(TryRecvError::Disconnected) => u.state = State::Idle,
            },

            State::Downloading { release, rx, .. } => match rx.try_recv() {
                Ok(Ok(())) => {
                    installed = Some(release.version.clone());
                    u.state = State::Installed(release.clone());
                    u.open = true;
                }

                Ok(Err(e)) => {
                    log::error(format!("Update: {e}"));
                    u.state = State::Failed(e);
                    u.open = true;
                }

                Err(TryRecvError::Empty) => {}
                Err(TryRecvError::Disconnected) => u.state = State::Failed("the update stopped".to_owned()),
            },

            _ => {}
        }

        let mut schemas_done = None;
        let mut schemas_known = None;
        match &u.schemas {
            SchemaState::Checking { rx } => match rx.try_recv() {
                Ok(Ok(commit)) => {
                    schemas_known = Some(commit.clone());
                    u.schemas = SchemaState::Known(commit);
                }

                Ok(Err(e)) => {
                    log::warn(format!("Schema check: {e}"));
                    u.schemas = SchemaState::Failed(e);
                }

                Err(TryRecvError::Empty) => {}
                Err(TryRecvError::Disconnected) => u.schemas = SchemaState::Idle,
            },

            SchemaState::Downloading { commit, rx, .. } => match rx.try_recv() {
                Ok(Ok(files)) => {
                    schemas_done = Some(commit.clone());
                    u.schemas = SchemaState::Done(commit.clone(), files);
                }

                Ok(Err(e)) => {
                    log::error(format!("Schemas: {e}"));
                    u.schemas = SchemaState::Failed(e);
                    u.open = true;
                }

                Err(TryRecvError::Empty) => {}
                Err(TryRecvError::Disconnected) => u.schemas = SchemaState::Failed("the download stopped".to_owned()),
            },

            _ => {}
        }

        // The quiet check opens the window for schemas that are missing (features need them) or, in a release build, older than rpfm's.
        if let Some(latest) = schemas_known.filter(|_| !manual) {
            let used = schema_in_use(self.game());
            let outdated = !used.overridden && used.commit.as_deref().is_some_and(|c| c != latest);
            if used.path.is_none() || (outdated && !cfg!(debug_assertions)) {
                log::info(format!("Schemas: {}", if used.path.is_none() { "none for this game" } else { "newer ones are available" }));
                self.updates.open = true;
            }
        }

        if let Some(version) = installed {
            self.status(&format!("Version {version} is installed: restart the editor to use it."), false);
        }

        if let Some(commit) = schemas_done {
            self.status(&format!("rpfm's schemas ({}) are downloaded.", schemas::short(&commit)), false);
        }
    }

    pub(crate) fn draw_updates(&mut self) {
        self.poll_updates();
        if !self.updates.open {
            return;
        }

        const TITLE: &str = "Update Manager";
        gui::open_popup(TITLE);
        let vp = gui::main_viewport();
        gui::set_next_window_pos(gui::v2(vp.Pos.x + vp.Size.x * 0.5, vp.Pos.y + vp.Size.y * 0.5), gui::COND_APPEARING, gui::v2(0.5, 0.5));
        gui::set_next_window_size(gui::v2(600.0, 0.0), 0);
        let mut open = true;
        if !gui::begin_popup_modal(TITLE, Some(&mut open), gui::WINDOW_ALWAYS_AUTO_RESIZE) {
            return;
        }

        gui::separator_text("Warscape Community Editor");
        self.draw_app_updates();
        gui::spacing();
        gui::separator_text("rpfm's schemas");
        self.draw_schema_updates();

        gui::spacing();
        gui::separator();
        gui::begin_disabled(self.updates.busy());
        if gui::button(&format!("{}  Check again", icons::REFRESH_CW)) {
            self.check_for_updates(true);
        }

        gui::end_disabled();
        gui::same_line();
        let close = gui::button("Close") || !open;

        // Closing the window while working hides it (the work goes on); an outcome that was seen is done with.
        if close {
            gui::close_current_popup();
            self.updates.open = false;
            if matches!(self.updates.state, State::Failed(_)) {
                self.updates.state = State::Idle;
            }

            if matches!(self.updates.schemas, SchemaState::Failed(_) | SchemaState::Done(..)) {
                self.updates.schemas = SchemaState::Idle;
            }
        }

        gui::end_popup();
    }

    /// The editor's part of the window.
    fn draw_app_updates(&mut self) {
        let current = wce_updater::current_version();
        match std::mem::take(&mut self.updates.state) {
            state @ State::Checking { .. } => {
                gui::text(&format!("This is version {current}. Looking for updates..."));
                self.updates.state = state;
            }

            State::Available(release) => {
                gui::text(&format!("Version {} is available (this is {current}).", release.version));
                if release.name != release.tag {
                    gui::text_disabled(&release.name);
                }

                if !release.notes.trim().is_empty() {
                    gui::begin_child("##notes", gui::v2(0.0, 220.0), gui::CHILD_BORDERS, 0);
                    gui::text_wrapped(&release.notes);
                    gui::end_child();
                }

                let mut next = None;
                if release.asset.is_some() {
                    if gui::button("Download and install") {
                        next = Some(release.clone());
                    }

                    gui::same_line();
                } else {
                    gui::text_wrapped("No build is published for this system: build this version from source.");
                }

                if gui::button("Release page") {
                    open_url(&release.page);
                }

                self.updates.state = State::Available(release);
                if let Some(release) = next {
                    self.start_download(release);
                }
            }

            State::UpToDate => {
                gui::text(&format!("This is the newest version ({current})."));
                self.updates.state = State::UpToDate;
            }

            State::Downloading { release, progress, rx } => {
                gui::text(&format!("Version {}", release.version));
                let done = progress.done.load(Ordering::Relaxed);
                let total = progress.total.load(Ordering::Relaxed);
                match progress.fraction() {
                    Some(f) if f >= 1.0 => gui::progress_bar(1.0, -1.0, "Installing..."),
                    Some(f) => gui::progress_bar(f, -1.0, &format!("{:.1} / {:.1} MB", megabytes(done), megabytes(total))),
                    None => gui::progress_bar(0.0, -1.0, &format!("{:.1} MB", megabytes(done))),
                }

                // Cancelling stops the download; once installing, it finishes.
                gui::begin_disabled(progress.fraction().is_some_and(|f| f >= 1.0));
                if gui::button("Cancel##app") {
                    progress.cancel.store(true, Ordering::Relaxed);
                }

                gui::end_disabled();
                self.updates.state = State::Downloading { release, progress, rx };
            }

            State::Installed(release) => {
                gui::text_wrapped(&format!("Version {} is installed. Restart the editor to use it.", release.version));
                if gui::button("Restart now") {
                    match std::env::current_exe() {
                        Ok(exe) => {
                            self.updates.restart = Some(exe);
                            self.updates.open = false;
                            gui::close_current_popup();
                            self.ask_quit();
                        }

                        Err(e) => log::error(format!("cannot find the editor's executable to restart it: {e}")),
                    }
                }

                self.updates.state = State::Installed(release);
            }

            State::Failed(e) => {
                gui::push_text_wrap_pos(580.0);
                gui::text_colored(theme::error(), &format!("The update failed: {e}"));
                gui::pop_text_wrap_pos();
                self.updates.state = State::Failed(e);
            }

            State::Idle => {
                gui::text(&format!("This is version {current}."));
                if cfg!(debug_assertions) {
                    gui::text_disabled("A debug build does not look for updates at start.");
                }
            }
        }
    }

    /// The schemas' part of the window: the current game's schema in use, the newest one, and the download.
    fn draw_schema_updates(&mut self) {
        let game = self.game();
        let used = schema_in_use(game);
        gui::push_text_wrap_pos(580.0);
        match &used.path {
            Some(path) => {
                let commit = used.commit.as_deref().map_or_else(|| "version unknown".to_owned(), |c| schemas::short(c).to_owned());
                gui::text(&format!("{}: {} ({commit})", game.display_name, path.display()));
            }

            None => gui::text_colored(
                theme::warning(),
                &format!("{}  No {} schema ({}): the buildings' data, the weather's climate phases and the procedural walls need it. Download the schemas.", icons::TRIANGLE_ALERT, game.display_name, game.rpfm_schema),
            ),
        }

        if used.overridden {
            gui::text_disabled("The schema in use comes from Settings > General > DB schemas (or $WCE_RPFM_SCHEMA): a download goes to the editor's own folder, read only for the games missing there.");
        }

        let mut download = None;
        match std::mem::take(&mut self.updates.schemas) {
            state @ SchemaState::Checking { .. } => {
                gui::text("Looking for newer schemas...");
                self.updates.schemas = state;
            }

            SchemaState::Known(latest) => {
                let current = used.commit.as_deref() == Some(latest.as_str());
                if current && used.path.is_some() {
                    gui::text(&format!("These are the newest schemas ({}).", schemas::short(&latest)));
                } else {
                    gui::text(&format!("The newest schemas are {}.", schemas::short(&latest)));
                    if gui::button(&format!("{}  {}", icons::DOWNLOAD, if used.path.is_some() { "Update the schemas" } else { "Download the schemas" })) {
                        download = Some(latest.clone());
                    }

                    if gui::is_item_hovered() {
                        let dir = downloaded_schemas_dir().map_or_else(String::new, |d| d.display().to_string());
                        gui::set_tooltip(&format!("Every supported game's schema ({}), from github.com/{}, into {dir}", schema_files().join(", "), schemas::REPO));
                    }
                }

                self.updates.schemas = SchemaState::Known(latest);
            }

            SchemaState::Downloading { commit, progress, rx } => {
                gui::text(&format!("Schemas {}", schemas::short(&commit)));
                let done = progress.done.load(Ordering::Relaxed);
                let total = progress.total.load(Ordering::Relaxed);
                gui::progress_bar(progress.fraction().unwrap_or(0.0), -1.0, &format!("{done} / {total} files"));
                if gui::button("Cancel##schemas") {
                    progress.cancel.store(true, Ordering::Relaxed);
                }

                self.updates.schemas = SchemaState::Downloading { commit, progress, rx };
            }

            SchemaState::Done(commit, files) => {
                gui::text_wrapped(&format!(
                    "{files} schemas of {} are downloaded. What has read the game's tables already (an open tile's buildings) reads them again when reopened.",
                    schemas::short(&commit)
                ));
                self.updates.schemas = SchemaState::Done(commit, files);
            }

            SchemaState::Failed(e) => {
                gui::text_colored(theme::error(), &format!("The schemas could not be checked or downloaded: {e}"));
                self.updates.schemas = SchemaState::Failed(e);
            }

            SchemaState::Idle => {}
        }

        gui::pop_text_wrap_pos();
        if let Some(commit) = download {
            self.start_schema_download(commit);
        }
    }

    /// What Help's Update Manager item says.
    pub(crate) fn updates_menu_label(&self) -> String {
        let what = match self.updates.state {
            State::Downloading { .. } => "  (downloading)",
            State::Installed(_) => "  (restart to finish)",
            _ => "",
        };

        format!("{}  Update Manager...{what}", icons::DOWNLOAD)
    }

    /// The menu item: shows the window, checking again unless something is going on or an update waits for the restart.
    pub(crate) fn updates_menu_clicked(&mut self) {
        if self.updates.busy() || matches!(self.updates.state, State::Installed(_)) {
            self.updates.open = true;
            self.updates.manual = true;
        } else {
            self.check_for_updates(true);
        }
    }
}
