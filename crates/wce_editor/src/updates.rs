//! Updates (Help > Check for updates, and a quiet check when an interactive session starts): the newest release above the running build
//! (`wce_updater`), downloaded with a progress bar and installed over the build, then a restart. The network work runs on a thread; the
//! window shows its state each frame.

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::sync::mpsc::{Receiver, TryRecvError, channel};

use wce_foundation::log;
use wce_updater::{Progress, Release};

use crate::editor::Editor;
use crate::{gui, theme};

#[derive(Default)]
enum State {
    #[default]
    Idle,

    /// Asking GitHub; `manual`: the user asked (the window shows the outcome, even "no update").
    Checking { manual: bool, rx: Receiver<Result<Option<Release>, String>> },
    Available(Release),
    UpToDate,

    /// Downloading, then installing.
    Downloading { release: Release, progress: Arc<Progress>, rx: Receiver<Result<(), String>> },
    Installed(Release),
    Failed(String),
}

#[derive(Default)]
pub(crate) struct Updates {
    state: State,

    /// The window is shown.
    open: bool,

    /// The executable to start once the editor has quit ("Restart now").
    restart: Option<PathBuf>,
}

impl Updates {
    fn busy(&self) -> bool {
        matches!(self.state, State::Checking { .. } | State::Downloading { .. })
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

impl Editor {
    /// Looks for a newer release on a thread. `manual` (Help > Check for updates) shows the window with the outcome; otherwise (at the
    /// start) the window opens only when there is an update, and a failure only goes to the log.
    pub fn check_for_updates(&mut self, manual: bool) {
        if self.updates.busy() {
            self.updates.open |= manual;
            return;
        }

        let prereleases = self.settings_ref().updates.prereleases;
        let (tx, rx) = channel();
        std::thread::spawn(move || {
            let _ = tx.send(wce_updater::check(&wce_updater::current_version(), prereleases));
        });

        self.updates.state = State::Checking { manual, rx };
        self.updates.open = manual;
    }

    /// An interactive session's start: deletes what the last update left, then checks quietly (when the settings ask for it; not in a debug
    /// build, whose folder is a Cargo target, nor under the first-run dialog).
    pub fn updates_at_start(&mut self) {
        if let Ok(dir) = wce_updater::install_dir() {
            wce_updater::remove_leftovers(&dir);
        }

        if self.settings_ref().updates.check_on_start && !self.first_run && !cfg!(debug_assertions) {
            self.check_for_updates(false);
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

    /// Takes the thread's outcome when it is there.
    fn poll_updates(&mut self) {
        let mut installed = None;
        let u = &mut self.updates;
        match &u.state {
            State::Checking { manual, rx } => {
                let manual = *manual;
                match rx.try_recv() {
                    Ok(Ok(Some(release))) => {
                        log::info(format!("Update: {} is available ({})", release.version, release.page));
                        u.state = State::Available(release);
                        u.open = true;
                    }

                    Ok(Ok(None)) => u.state = if manual { State::UpToDate } else { State::Idle },
                    Ok(Err(e)) => {
                        log::warn(format!("Update check: {e}"));
                        u.state = if manual { State::Failed(e) } else { State::Idle };
                    }

                    Err(TryRecvError::Empty) => {}
                    Err(TryRecvError::Disconnected) => u.state = State::Idle,
                }
            }

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

        if let Some(version) = installed {
            self.status(&format!("Version {version} is installed: restart the editor to use it."), false);
        }
    }

    pub(crate) fn draw_updates(&mut self) {
        self.poll_updates();
        if !self.updates.open {
            return;
        }

        const TITLE: &str = "Updates";
        gui::open_popup(TITLE);
        let vp = gui::main_viewport();
        gui::set_next_window_pos(gui::v2(vp.Pos.x + vp.Size.x * 0.5, vp.Pos.y + vp.Size.y * 0.5), gui::COND_APPEARING, gui::v2(0.5, 0.5));
        gui::set_next_window_size(gui::v2(560.0, 0.0), 0);
        let mut open = true;
        if !gui::begin_popup_modal(TITLE, Some(&mut open), gui::WINDOW_ALWAYS_AUTO_RESIZE) {
            return;
        }

        let current = wce_updater::current_version();
        let mut close = !open;
        match std::mem::take(&mut self.updates.state) {
            state @ State::Checking { .. } => {
                gui::text("Looking for updates...");
                close |= gui::button("Hide");
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

                gui::spacing();
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

                gui::same_line();
                close |= gui::button("Later");
                self.updates.state = State::Available(release);
                if let Some(release) = next {
                    self.start_download(release);
                }
            }

            State::UpToDate => {
                gui::text(&format!("This is the newest version ({current})."));
                close |= gui::button("OK");
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
                if gui::button("Cancel") {
                    progress.cancel.store(true, Ordering::Relaxed);
                }

                gui::end_disabled();
                self.updates.state = State::Downloading { release, progress, rx };
            }

            State::Installed(release) => {
                gui::text_wrapped(&format!("Version {} is installed. Restart the editor to use it.", release.version));
                if gui::button("Restart now") {
                    close = true;
                    match std::env::current_exe() {
                        Ok(exe) => {
                            self.updates.restart = Some(exe);
                            self.ask_quit();
                        }

                        Err(e) => log::error(format!("cannot find the editor's executable to restart it: {e}")),
                    }
                }

                gui::same_line();
                close |= gui::button("Later");
                self.updates.state = State::Installed(release);
            }

            State::Failed(e) => {
                gui::push_text_wrap_pos(540.0);
                gui::text_colored(theme::error(), &format!("The update failed: {e}"));
                gui::pop_text_wrap_pos();
                close |= gui::button("OK");
                self.updates.state = State::Failed(e);
            }

            State::Idle => close = true,
        }

        // Closing the window while working hides it (the work goes on); an outcome that was seen is done with.
        if close {
            gui::close_current_popup();
            self.updates.open = false;
            if !self.updates.busy() && !matches!(self.updates.state, State::Installed(_)) {
                self.updates.state = State::Idle;
            }
        }

        gui::end_popup();
    }

    /// What Help's updates item says.
    pub(crate) fn updates_menu_label(&self) -> &'static str {
        match self.updates.state {
            State::Downloading { .. } => "Downloading the update...",
            State::Installed(_) => "Restart to finish the update...",
            _ => "Check for updates...",
        }
    }

    /// The menu item: shows the window of the work going on, or the installed update, or checks.
    pub(crate) fn updates_menu_clicked(&mut self) {
        if self.updates.busy() || matches!(self.updates.state, State::Installed(_)) {
            self.updates.open = true;
        } else {
            self.check_for_updates(true);
        }
    }
}
