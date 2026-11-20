//! The Cinematic tool: the Movies list and the Movie viewer (step 2 of docs/plans/cinematic.md).

use std::any::Any;
use std::sync::Arc;
use std::sync::mpsc::{Receiver, channel};

use wce_audio::library::SoundLibrary;
use wce_cinematic::movies::MovieCatalog;
use wce_editor::editor::Editor;
use wce_editor::gui::{self, v2};
use wce_editor::tool::Tool;
use wce_editor_core::command_registry::{CommandRegistry, CommandResult};
use wce_vfs::files::Files;

use crate::viewer::MovieViewer;

/// What a command asked for, done at the next frame (commands reach the tool between frames).
#[derive(Clone, Debug)]
enum Pending {
    Open(String),
    Seek(usize),
    Play,
    Pause,
    Language(String),
}

#[derive(Default)]
pub struct CinematicTool {
    files: Option<Arc<dyn Files>>,
    mount_id: u64,
    catalog: Option<Arc<MovieCatalog>>,
    catalog_loading: Option<Receiver<MovieCatalog>>,
    library: Option<Arc<SoundLibrary>>,
    library_loading: Option<Receiver<SoundLibrary>>,
    viewer: Option<MovieViewer>,
    filter: String,
    pending: Vec<Pending>,
}

impl CinematicTool {
    fn poll(&mut self, e: &mut Editor) {
        if let Some(files) = e.files()
            && files.mount_id() != self.mount_id
        {
            self.mount_id = files.mount_id();
            self.files = Some(files.clone());
            self.catalog = None;
            self.library = None;
            if let Some(mut v) = self.viewer.take() {
                v.close(e);
            }
            let (tx, rx) = channel();
            let f = files.clone();
            std::thread::spawn(move || {
                let _ = tx.send(MovieCatalog::load(f.as_ref()));
            });
            self.catalog_loading = Some(rx);
            self.library_loading = None;
        }
        // A scripted run waits for the list, so that its next line can open a movie.
        let scripted = e.scripted_input();
        if let Some(rx) = &self.catalog_loading
            && let Ok(c) = if scripted { rx.recv().map_err(|_| std::sync::mpsc::TryRecvError::Disconnected) } else { rx.try_recv() }
        {
            self.catalog = Some(Arc::new(c));
            self.catalog_loading = None;
        }
        if let Some(rx) = &self.library_loading
            && let Ok(l) = rx.try_recv()
        {
            for (path, why) in &l.failures {
                e.status(&format!("{path}: {why}"), true);
            }
            self.library = Some(Arc::new(l));
            self.library_loading = None;
        }

        // Commands wait for the catalog.
        if self.catalog.is_some() {
            for p in std::mem::take(&mut self.pending) {
                self.apply(e, p);
            }
        }

        // The sound library loads once a movie is open (not in scripted runs: they don't listen).
        if self.viewer.is_some() && self.library.is_none() && self.library_loading.is_none() && !e.scripted_input() {
            if let Some(files) = self.files.clone() {
                let (tx, rx) = channel();
                std::thread::spawn(move || {
                    let _ = tx.send(SoundLibrary::load(files.as_ref()));
                });
                self.library_loading = Some(rx);
            }
        }
        if let (Some(v), Some(lib), Some(files)) = (self.viewer.as_mut(), &self.library, &self.files) {
            v.attach_audio(lib, files);
        }
        if let Some(v) = self.viewer.as_mut() {
            v.update(e);
        }
    }

    fn apply(&mut self, e: &mut Editor, p: Pending) {
        match p {
            Pending::Open(what) => {
                if let Err(err) = self.open(e, &what) {
                    e.status(&err, true);
                }
            }
            Pending::Seek(frame) => {
                if let Some(v) = self.viewer.as_mut() {
                    v.pause();
                    v.seek_frame(frame);
                }
            }
            Pending::Play => {
                if let Some(v) = self.viewer.as_mut() {
                    v.play();
                }
            }
            Pending::Pause => {
                if let Some(v) = self.viewer.as_mut() {
                    v.pause();
                }
            }
            Pending::Language(l) => {
                if let (Some(v), Some(c)) = (self.viewer.as_mut(), &self.catalog) {
                    v.set_language(c, &l);
                }
            }
        }
    }

    /// Opens a movie by path (`movies/intro.ca_vp8`) or name (`intro`).
    fn open(&mut self, e: &mut Editor, what: &str) -> Result<(), String> {
        let (Some(catalog), Some(files)) = (self.catalog.clone(), self.files.clone()) else {
            return Err("the game's files are not loaded".into());
        };
        let lower = what.to_lowercase().replace('\\', "/");
        let entry = catalog
            .movies
            .iter()
            .find(|m| m.path == lower || m.name.to_lowercase() == lower)
            .cloned()
            .ok_or_else(|| format!("no movie {what}"))?;
        let bytes = files.read(&entry.path)?;
        let viewer = MovieViewer::open(entry, &bytes, &catalog)?;
        if let Some(mut old) = self.viewer.take() {
            old.close(e);
        }
        e.status(
            &format!(
                "{}: {} x {}, {} frames at {:.3} fps, {} subtitle lines",
                viewer.entry.path,
                viewer.movie.width(),
                viewer.movie.height(),
                viewer.movie.frame_count(),
                viewer.movie.fps(),
                viewer.subtitles.len()
            ),
            false,
        );
        self.viewer = Some(viewer);
        gui::select_dock_tab("Movie");
        Ok(())
    }

    fn movies_panel(&mut self, e: &mut Editor) {
        gui::set_next_window_size(v2(320.0, 500.0), gui::COND_FIRST_USE_EVER);
        if !gui::begin("Movies", None, 0) {
            gui::end();
            return;
        }
        let Some(catalog) = self.catalog.clone() else {
            gui::text_disabled(if self.files.is_some() { "Reading the movies..." } else { "No game files" });
            gui::end();
            return;
        };
        gui::set_next_item_width(-1.0);
        gui::input_text("##filter", Some("Filter"), &mut self.filter, 128, 0);
        let filter = self.filter.to_lowercase();
        let mut open = None;
        for (kind, list) in catalog.by_type() {
            let shown: Vec<_> = list.into_iter().filter(|m| filter.is_empty() || m.path.contains(&filter)).collect();
            if shown.is_empty() {
                continue;
            }
            let title = format!("{} ({})##{}", kind.as_deref().unwrap_or("No videos row"), shown.len(), kind.as_deref().unwrap_or("-"));
            let flags = if kind.as_deref() == Some("FULLSCREEN_INTRO") { gui::TREE_DEFAULT_OPEN } else { 0 };
            if gui::tree_node_ex(&title, flags) {
                gui::clipped(shown.len(), |i| {
                    let m = shown[i];
                    let selected = self.viewer.as_ref().is_some_and(|v| v.entry.path == m.path);
                    if gui::selectable(&m.name, selected, 0) {
                        open = Some(m.path.clone());
                    }
                    if gui::is_item_hovered() {
                        gui::set_tooltip(&m.path);
                    }
                });
                gui::tree_pop();
            }
        }
        gui::end();
        if let Some(path) = open
            && let Err(err) = self.open(e, &path)
        {
            e.status(&err, true);
        }
    }

    fn movie_panel(&mut self, e: &mut Editor) {
        gui::set_next_window_size(v2(900.0, 560.0), gui::COND_FIRST_USE_EVER);
        if !gui::begin("Movie", None, 0) {
            gui::end();
            return;
        }
        let Some(v) = self.viewer.as_mut() else {
            gui::text_disabled("Pick a movie in Movies");
            gui::end();
            return;
        };
        let avail = gui::content_region_avail();
        let transport = gui::frame_height_with_spacing() * 2.0 + 8.0;
        v.draw_picture(e, v2(avail.x, (avail.y - transport).max(60.0)));
        let catalog = self.catalog.clone().unwrap_or_default();
        v.draw_transport(&catalog, self.library.as_ref(), self.files.as_ref());
        gui::end();
    }
}

impl Tool for CinematicTool {
    fn id(&self) -> &str {
        "cinematic"
    }

    fn title(&self) -> &str {
        "Cinematic"
    }

    fn icon(&self) -> &str {
        wce_editor::icons::CLAPPERBOARD
    }

    fn opens(&self) -> Vec<String> {
        vec![".ca_vp8".into()]
    }

    fn project_folder(&self) -> &'static str {
        "cinematics"
    }

    fn shows_viewport(&self) -> bool {
        false
    }

    fn default_layout(&self, dockspace: gui::ImGuiID) {
        let (left, center) = gui::dock_builder_split(dockspace, gui::DIR_LEFT, 0.22);
        let (bottom, center) = gui::dock_builder_split(center, gui::DIR_DOWN, 0.2);
        gui::dock_builder_dock_window("Movies", left);
        gui::dock_builder_dock_window("Movie", center);
        gui::dock_builder_dock_window("Assets", bottom);
        gui::dock_builder_dock_window("Log", bottom);
        gui::dock_builder_dock_window("History", bottom);
        gui::dock_builder_dock_window("Inspector", bottom);
    }

    fn register_commands(&self, c: &mut CommandRegistry<Editor>) {
        fn queue(e: &mut Editor, p: Pending) -> CommandResult {
            match e.with_tool::<CinematicTool, _>(|t, _| t.pending.push(p.clone())) {
                Some(()) => CommandResult::success(format!("{p:?}")).wait(2),
                None => CommandResult::fail("the Cinematic tool is not built in"),
            }
        }
        c.add("movie_open", "movie_open <movies/x.ca_vp8 | name>", "Opens a movie in the Movie viewer", |e, a| {
            if a.is_empty() {
                return CommandResult::fail("usage: movie_open <path | name>");
            }
            queue(e, Pending::Open(a.join(" ")))
        });
        c.add("movie_seek", "movie_seek <frame>", "Pauses the movie at a frame", |e, a| match a.first().and_then(|x| x.parse().ok()) {
            Some(f) => queue(e, Pending::Seek(f)),
            None => CommandResult::fail("usage: movie_seek <frame>"),
        });
        c.add("movie_play", "movie_play", "Plays the movie", |e, _| queue(e, Pending::Play));
        c.add("movie_pause", "movie_pause", "Pauses the movie", |e, _| queue(e, Pending::Pause));
        c.add("movie_language", "movie_language <code>", "Shows the subtitles in a language's timings", |e, a| match a.first() {
            Some(l) => queue(e, Pending::Language(l.clone())),
            None => CommandResult::fail("usage: movie_language <code>"),
        });
        c.add(
            "assert_movie",
            "assert_movie <frame> [subtitle text...]",
            "Fails unless the open movie is at a frame (and shows a subtitle containing the text)",
            |e, a| {
                let Some(want) = a.first().and_then(|x| x.parse::<usize>().ok()) else {
                    return CommandResult::fail("usage: assert_movie <frame> [subtitle text...]");
                };
                let text = a[1..].join(" ");
                let got = e.with_tool::<CinematicTool, _>(|t, _| {
                    t.viewer.as_ref().map(|v| {
                        (v.frame(), MovieCatalog::line_at(&v.subtitles, v.frame() as u32).map(|s| s.text.clone()).unwrap_or_default())
                    })
                });
                match got {
                    Some(Some((frame, line))) if frame == want && line.contains(&text) => {
                        CommandResult::success(format!("frame {frame} \"{line}\""))
                    }
                    Some(Some((frame, line))) => {
                        CommandResult::fail(format!("frame {frame} \"{line}\", expected frame {want} with \"{text}\""))
                    }
                    _ => CommandResult::fail("no movie open"),
                }
            },
        );
        c.add("movie_info", "movie_info", "Logs the open movie's frame, time and subtitle line", |e, _| {
            let info = e.with_tool::<CinematicTool, _>(|t, _| {
                t.viewer.as_ref().map(|v| {
                    let line = MovieCatalog::line_at(&v.subtitles, v.frame() as u32).map(|s| s.text.clone()).unwrap_or_default();
                    format!("{} frame {} time {:.3} playing {} subtitle \"{line}\"", v.entry.name, v.frame(), v.time, v.playing)
                })
            });
            match info {
                Some(Some(s)) => CommandResult::success(s),
                Some(None) => CommandResult::fail("no movie open"),
                None => CommandResult::fail("the Cinematic tool is not built in"),
            }
        });
    }

    fn draw_panels(&mut self, e: &mut Editor) {
        self.poll(e);
        self.movies_panel(e);
        self.movie_panel(e);
    }

    fn unsaved_work(&self) -> Vec<String> {
        vec![]
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}
