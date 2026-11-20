//! The Cinematic tool (docs/plans/cinematic.md): the Movies list and the Movie viewer (step 2), the scene editor (Scene, Timeline,
//! Key, Layout: steps 3 and 4) and the exports (a historical battle pack).

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

use wce_cinematic::scene::{Interp, PathKey, Project, SceneUnit, TrackContent, sanitize_key};

use crate::export::MovieJob;
use crate::preview::Preview;
use crate::scene_doc::SceneDocument;
use crate::scene_ui::{SceneUi, add_track, edit, for_track, scene_doc};
use crate::viewer::MovieViewer;

/// What a command asked for, done at the next frame (commands reach the tool between frames).
#[derive(Clone, Debug)]
enum Pending {
    Open(String),
    Seek(usize),
    Play,
    Pause,
    Language(String),
    SceneNew(String),
    SceneOpen(String),
    SceneTime(f64),
    ScenePlay(bool),
    UnitAdd(String, usize, f32, f32),
    UnitSelect(u32),
    TrackAdd(String),
    KeyAdd,
    PathKey(f32, f32),
    ExportBattle(String),
    ExportMovie(String, u32, u32),
    TrackSelect(u32),
    CameraKey([f32; 3], [f32; 3], f32),
    Show(String),
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
    scene: SceneUi,
    new_scene: Option<String>,
    preview: Preview,
    movie_job: Option<MovieJob>,
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
            self.preview.game_changed(e);
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
            Pending::SceneNew(name) => self.new_scene_project(e, &name),
            Pending::SceneOpen(what) => self.open_scene(e, &what),
            Pending::SceneTime(t) => {
                self.scene.time = t.max(0.0);
                self.scene.playing = false;
            }
            Pending::ScenePlay(on) => self.scene.playing = on,
            Pending::UnitAdd(key, army, x, z) => {
                let mut made = 0;
                edit(e, "add unit", |p| {
                    let id = p.new_id();
                    let facing = p.armies.get(army).map_or(0.0, |a| a.deployment_orientation);
                    p.units.push(SceneUnit {
                        id,
                        name: key.clone(),
                        unit: key.clone(),
                        army,
                        start: [x, z],
                        facing,
                        width: 30.0,
                        men: 160,
                        seed: id,
                        general: false,
                    });
                    made = id;
                });
                if made != 0 {
                    self.scene.unit = Some(made);
                }
            }
            Pending::UnitSelect(id) => self.scene.unit = Some(id),
            Pending::TrackAdd(kind) => {
                let Some(p) = scene_doc(e).map(|d| d.project().clone()) else { return };
                let Some(clip) = self.scene.current_clip(&p).map(|c| c.id) else { return };
                let (unit, env) = (self.scene.unit, p.battle.environment.clone());
                let mut made = None;
                edit(e, "add track", |p| made = add_track(p, clip, &kind, unit, &env));
                if made.is_some() {
                    self.scene.track = made;
                } else {
                    e.status(&format!("Cinematic: no {kind} track (a unit track needs a selected unit)"), true);
                }
            }
            Pending::KeyAdd => {
                if let Some(p) = scene_doc(e).map(|d| d.project().clone())
                    && let Some(clip) = self.scene.current_clip(&p).cloned()
                {
                    self.scene.add_key_public(e, &p, &clip);
                }
            }
            Pending::PathKey(x, z) => {
                let Some(p) = scene_doc(e).map(|d| d.project().clone()) else { return };
                let (Some(u), Some(clip)) = (self.scene.unit, self.scene.current_clip(&p).map(|c| c.id)) else { return };
                let t = self.scene.time;
                let mut track = None;
                edit(e, "path key", |p| {
                    let id = match p
                        .clip(clip)
                        .and_then(|c| c.tracks.iter().find(|tr| matches!(&tr.content, TrackContent::Unit { unit, .. } if *unit == u)))
                    {
                        Some(tr) => tr.id,
                        None => add_track(p, clip, "Unit", Some(u), "").unwrap_or(0),
                    };
                    for_track(p, clip, id, |tr| {
                        if let TrackContent::Unit { path, .. } = &mut tr.content {
                            path.retain(|k| (k.t - t).abs() > 1e-6);
                            path.push(PathKey { t, position: [x, z], interp: Interp::Linear, gait: None });
                        }
                    });
                    track = Some(id);
                });
                self.scene.track = track;
            }
            Pending::ExportBattle(path) => match self.export_battle(e, std::path::Path::new(&path)) {
                Ok(msg) => e.status(&msg, false),
                Err(err) => e.status(&format!("Cinematic: {err}"), true),
            },
            Pending::ExportMovie(path, w, h) => self.start_movie(e, std::path::PathBuf::from(path), w, h),
            Pending::TrackSelect(id) => self.scene.track = Some(id),
            Pending::Show(panel) => gui::select_dock_tab(&panel),
            Pending::CameraKey(position, target, fov) => {
                let Some(p) = scene_doc(e).map(|d| d.project().clone()) else { return };
                let Some(clip) = self.scene.current_clip(&p).map(|c| c.id) else { return };
                let t = self.scene.time;
                let track =
                    p.clip(clip).and_then(|c| c.tracks.iter().find(|tr| matches!(tr.content, TrackContent::Camera { .. })).map(|tr| tr.id));
                edit(e, "camera key", |p| {
                    let id = match track {
                        Some(id) => id,
                        None => add_track(p, clip, "Camera", None, "").unwrap_or(0),
                    };
                    for_track(p, clip, id, |tr| {
                        if let TrackContent::Camera { keys } = &mut tr.content {
                            keys.retain(|k| (k.t - t).abs() > 1e-6);
                            keys.push(wce_cinematic::scene::CameraKey { t, position, target, fov, interp: Interp::Smooth });
                        }
                    });
                });
            }
        }
    }

    fn scenes_dir(e: &Editor) -> std::path::PathBuf {
        e.projects_dir().join("cinematics")
    }

    /// A new scene in `<projects>/cinematics/<key>/project.json` (written when saved).
    fn new_scene_project(&mut self, e: &mut Editor, name: &str) {
        let game = self.files.as_ref().and_then(|f| f.game().map(|g| g.key.to_owned())).unwrap_or_else(|| "attila".into());
        let p = Project::new(name.trim(), &game);
        let path = Self::scenes_dir(e).join(sanitize_key(name)).join("project.json");
        if path.exists() {
            e.status(&format!("Cinematic: {} exists: opened instead", path.display()), true);
            self.open_scene(e, &path.display().to_string());
            return;
        }
        match e.replace_document("cinematic", Box::new(SceneDocument::new(p, path))) {
            Ok(_) => {
                self.scene = SceneUi::default();
                e.status(&format!("Cinematic: new scene {name}"), false);
                gui::select_dock_tab("Timeline");
                gui::select_dock_tab("Layout");
            }
            Err(err) => e.status(&format!("Cinematic: {err}"), true),
        }
    }

    /// Opens a scene by its project.json path or its folder name under `<projects>/cinematics`.
    fn open_scene(&mut self, e: &mut Editor, what: &str) {
        let mut path = std::path::PathBuf::from(what);
        if !path.is_absolute() {
            path = Self::scenes_dir(e).join(what);
        }
        if path.is_dir() {
            path = path.join("project.json");
        }
        match SceneDocument::open(path.clone()) {
            Ok(d) => match e.replace_document("cinematic", Box::new(d)) {
                Ok(_) => {
                    self.scene = SceneUi::default();
                    e.status(&format!("Cinematic: opened {}", path.display()), false);
                    gui::select_dock_tab("Timeline");
                    gui::select_dock_tab("Layout");
                }
                Err(err) => e.status(&format!("Cinematic: {err}"), true),
            },
            Err(err) => e.status(&format!("Cinematic: {err}"), true),
        }
    }

    /// The scene as a historical battle pack at `path`.
    fn export_battle(&mut self, e: &mut Editor, path: &std::path::Path) -> Result<String, String> {
        let p = scene_doc(e).map(|d| d.project().clone()).ok_or("no scene open")?;
        let files = self.files.clone().ok_or("the game's files are not loaded")?;
        let schema = files
            .game()
            .and_then(wce_foundation::settings::rpfm_schema_path)
            .and_then(|s| rpfm_lib::schema::Schema::load(&s, None).ok())
            .ok_or("no RPFM schema for the game")?;
        let warnings = wce_cinematic::battle_tables::write_battle_pack(path, &p, files.as_ref(), &schema, "en")?;
        for w in &warnings {
            e.status(&format!("Cinematic: {w}"), true);
        }
        Ok(format!("Cinematic: wrote {} ({} warnings)", path.display(), warnings.len()))
    }

    /// Starts rendering the timeline's clip as a movie pack.
    fn start_movie(&mut self, e: &mut Editor, path: std::path::PathBuf, w: u32, h: u32) {
        let (Some(doc), Some(p)) = (e.docs.of_tool("cinematic"), scene_doc(e).map(|d| d.project().clone())) else {
            e.status("Cinematic: no scene open", true);
            return;
        };
        let Some(clip) = self.scene.current_clip(&p).map(|c| c.id) else {
            e.status("Cinematic: the scene has no clip", true);
            return;
        };
        match MovieJob::new(path, p, clip, doc, w.max(16), h.max(16)) {
            Ok(job) => {
                e.status(&format!("Cinematic: rendering {}...", job.name), false);
                self.movie_job = Some(job);
            }
            Err(err) => e.status(&format!("Cinematic: {err}"), true),
        }
    }

    /// The movie export's frames: a few per editor frame live, all at once in a scripted run; then the pack.
    fn step_movie(&mut self, e: &mut Editor) {
        let Some(mut job) = self.movie_job.take() else { return };
        let budget = if e.scripted_input() { None } else { Some(3) };
        match job.step(e, &mut self.preview, &self.scene.speeds, budget) {
            Ok(false) => {
                let (n, total) = job.progress();
                e.status(&format!("Cinematic: rendering {} frame {n} of {total}", job.name), false);
                self.movie_job = Some(job);
            }
            Ok(true) => {
                let Some(files) = self.files.clone() else { return };
                match job.finish(&files, self.library.as_deref()) {
                    Ok((msg, warnings)) => {
                        for w in warnings {
                            e.status(&format!("Cinematic: {w}"), true);
                        }
                        e.status(&format!("Cinematic: {msg}"), false);
                    }
                    Err(err) => e.status(&format!("Cinematic: the movie: {err}"), true),
                }
            }
            Err(err) => e.status(&format!("Cinematic: the movie: {err}"), true),
        }
    }

    fn new_scene_popup(&mut self, e: &mut Editor) {
        let Some(mut name) = self.new_scene.take() else { return };
        gui::open_popup("New cinematic scene");
        let mut keep = true;
        if gui::begin_popup_modal("New cinematic scene", None, 0) {
            gui::set_next_item_width(260.0);
            let enter = gui::input_text("##scenename", Some("name"), &mut name, 64, gui::INPUT_ENTER_RETURNS_TRUE);
            if (gui::button("Create") || enter) && !name.trim().is_empty() {
                self.new_scene_project(e, &name);
                keep = false;
                gui::close_current_popup();
            }
            gui::same_line();
            if gui::button("Cancel") {
                keep = false;
                gui::close_current_popup();
            }
            gui::end_popup();
        }
        if keep {
            self.new_scene = Some(name);
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
        true
    }

    fn file_menu(&mut self, e: &mut Editor, part: wce_editor::tool::FileMenu) {
        if part != wce_editor::tool::FileMenu::Open {
            return;
        }
        if gui::menu_item("New cinematic scene...", None, false, true) {
            self.new_scene = Some(String::new());
        }
        if gui::begin_menu("Open cinematic scene") {
            let dir = Self::scenes_dir(e);
            let mut scenes: Vec<String> = std::fs::read_dir(&dir)
                .map(|r| {
                    r.flatten()
                        .filter(|d| d.path().join("project.json").is_file())
                        .map(|d| d.file_name().to_string_lossy().into_owned())
                        .collect()
                })
                .unwrap_or_default();
            scenes.sort();
            if scenes.is_empty() {
                gui::text_disabled("No scenes yet");
            }
            for s in scenes {
                if gui::menu_item(&s, None, false, true) {
                    self.open_scene(e, &s);
                }
            }
            gui::end_menu();
        }
    }

    fn export_menu(&mut self, e: &mut Editor) -> bool {
        let has = scene_doc(e).is_some();
        if gui::menu_item("Historical battle pack...", None, false, has) {
            let key = scene_doc(e).map(|d| sanitize_key(&d.project().battle.key)).unwrap_or_default();
            let filters = [("Mod pack".to_owned(), "*.pack".to_owned())];
            if let Some(path) = e.pick_export_file("cinematic_battle_pack", &format!("{key}.pack"), &filters) {
                match self.export_battle(e, &path) {
                    Ok(msg) => e.status(&msg, false),
                    Err(err) => e.status(&format!("Cinematic: {err}"), true),
                }
            }
        }
        if gui::is_item_hovered() {
            gui::set_tooltip("The battle XML, Lua, menu rows, subtitles and loc as one mod pack");
        }
        if gui::menu_item("Movie pack (1080p)...", None, false, has && self.movie_job.is_none()) {
            let name = scene_doc(e).map(|d| sanitize_key(&d.project().battle.key)).unwrap_or_default();
            let filters = [("Mod pack".to_owned(), "*.pack".to_owned())];
            if let Some(path) = e.pick_export_file("cinematic_movie_pack", &format!("{name}_movie.pack"), &filters) {
                self.start_movie(e, path, 1920, 1080);
            }
        }
        if gui::is_item_hovered() {
            gui::set_tooltip("The timeline's clip rendered as a game movie, with its sound event, videos row and subtitles");
        }
        true
    }

    fn default_layout(&self, dockspace: gui::ImGuiID) {
        let (left, center) = gui::dock_builder_split(dockspace, gui::DIR_LEFT, 0.22);
        let (right, center) = gui::dock_builder_split(center, gui::DIR_RIGHT, 0.24);
        let (bottom, center) = gui::dock_builder_split(center, gui::DIR_DOWN, 0.32);
        gui::dock_builder_dock_window("Movies", left);
        gui::dock_builder_dock_window("Scene", left);
        gui::dock_builder_dock_window("Movie", center);
        gui::dock_builder_dock_window("Layout", center);
        gui::dock_builder_dock_window("Viewport", center);
        gui::dock_builder_dock_window("Key", right);
        gui::dock_builder_dock_window("Assets", bottom);
        gui::dock_builder_dock_window("Log", bottom);
        gui::dock_builder_dock_window("History", bottom);
        gui::dock_builder_dock_window("Inspector", bottom);
        // Docked last: the bottom node's tab in front.
        gui::dock_builder_dock_window("Timeline", bottom);
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
        c.add("scene_new", "scene_new <name>", "A new cinematic scene (saved under <projects>/cinematics/<key>)", |e, a| {
            if a.is_empty() {
                return CommandResult::fail("usage: scene_new <name>");
            }
            queue(e, Pending::SceneNew(a.join(" ")))
        });
        c.add("scene_open", "scene_open <folder | project.json>", "Opens a cinematic scene", |e, a| {
            if a.is_empty() {
                return CommandResult::fail("usage: scene_open <folder | path>");
            }
            queue(e, Pending::SceneOpen(a.join(" ")))
        });
        c.add("scene_time", "scene_time <seconds>", "Moves the timeline's playhead", |e, a| match a.first().and_then(|x| x.parse().ok()) {
            Some(t) => queue(e, Pending::SceneTime(t)),
            None => CommandResult::fail("usage: scene_time <seconds>"),
        });
        c.add("scene_play", "scene_play [0|1]", "Plays (or stops) the clip", |e, a| {
            queue(e, Pending::ScenePlay(a.first().is_none_or(|x| x != "0")))
        });
        c.add("unit_add", "unit_add <main_units key> <army> <x> <z>", "Adds a unit to the scene and selects it", |e, a| {
            let n = |i: usize| a.get(i).and_then(|x| x.parse::<f32>().ok());
            match (a.first(), a.get(1).and_then(|x| x.parse().ok()), n(2), n(3)) {
                (Some(k), Some(army), Some(x), Some(z)) => queue(e, Pending::UnitAdd(k.clone(), army, x, z)),
                _ => CommandResult::fail("usage: unit_add <key> <army> <x> <z>"),
            }
        });
        c.add("unit_select", "unit_select <id>", "Selects a unit of the scene", |e, a| match a.first().and_then(|x| x.parse().ok()) {
            Some(id) => queue(e, Pending::UnitSelect(id)),
            None => CommandResult::fail("usage: unit_select <id>"),
        });
        c.add("track_add", "track_add <kind>", "Adds a track to the clip (a Unit track: the selected unit's)", |e, a| {
            if a.is_empty() {
                return CommandResult::fail("usage: track_add <kind>");
            }
            queue(e, Pending::TrackAdd(a.join(" ")))
        });
        c.add("key_add", "key_add", "Adds a key at the playhead on the selected track", |e, _| queue(e, Pending::KeyAdd));
        c.add("path_key", "path_key <x> <z>", "A path key for the selected unit at the playhead", |e, a| {
            match (a.first().and_then(|x| x.parse().ok()), a.get(1).and_then(|x| x.parse().ok())) {
                (Some(x), Some(z)) => queue(e, Pending::PathKey(x, z)),
                _ => CommandResult::fail("usage: path_key <x> <z>"),
            }
        });
        c.add("scene_export_battle", "scene_export_battle <file.pack>", "Writes the scene as a historical battle pack", |e, a| {
            if a.is_empty() {
                return CommandResult::fail("usage: scene_export_battle <file.pack>");
            }
            queue(e, Pending::ExportBattle(a.join(" ")))
        });
        c.add("track_select", "track_select <id>", "Selects a track of the clip", |e, a| match a.first().and_then(|x| x.parse().ok()) {
            Some(id) => queue(e, Pending::TrackSelect(id)),
            None => CommandResult::fail("usage: track_select <id>"),
        });
        c.add("camera_key", "camera_key <x y z> <tx ty tz> <fov>", "A camera key at the playhead (metres, degrees)", |e, a| {
            let v: Vec<f32> = a.iter().filter_map(|x| x.parse().ok()).collect();
            if v.len() != 7 {
                return CommandResult::fail("usage: camera_key x y z tx ty tz fov");
            }
            queue(e, Pending::CameraKey([v[0], v[1], v[2]], [v[3], v[4], v[5]], v[6]))
        });
        c.add("cinematic_show", "cinematic_show <panel>", "Brings a panel to the front (Viewport, Layout, Movie, Timeline...)", |e, a| {
            queue(e, Pending::Show(a.join(" ")))
        });
        c.add("scene_export_movie", "scene_export_movie <file.pack> [width height]", "Renders the clip as a movie pack", |e, a| {
            let n = |i: usize, d: u32| a.get(i).and_then(|x| x.parse().ok()).unwrap_or(d);
            match a.first() {
                Some(p) => queue(e, Pending::ExportMovie(p.clone(), n(1, 1920), n(2, 1080))),
                None => CommandResult::fail("usage: scene_export_movie <file.pack> [width height]"),
            }
        });
        c.add(
            "assert_unit",
            "assert_unit <id> <x> <z> [slot]",
            "Fails unless the unit is there (1 m) at the playhead, with that slot",
            |e, a| {
                let id: Option<u32> = a.first().and_then(|x| x.parse().ok());
                let (x, z) = (a.get(1).and_then(|v| v.parse::<f32>().ok()), a.get(2).and_then(|v| v.parse::<f32>().ok()));
                let slot = a.get(3).cloned();
                let (Some(id), Some(x), Some(z)) = (id, x, z) else { return CommandResult::fail("usage: assert_unit <id> <x> <z> [slot]") };
                let got = e.with_tool::<CinematicTool, _>(|t, e| {
                    let p = scene_doc(e).map(|d| d.project().clone())?;
                    let f = t.scene.frame(&p)?;
                    f.units.into_iter().find(|u| u.unit == id)
                });
                match got.flatten() {
                    Some(u)
                        if (u.position[0] - x).abs() <= 1.0
                            && (u.position[1] - z).abs() <= 1.0
                            && slot.as_ref().is_none_or(|s| *s == u.slot) =>
                    {
                        CommandResult::success(format!(
                            "unit {id} at {:.2} {:.2} {} since {:.2}",
                            u.position[0], u.position[1], u.slot, u.slot_start
                        ))
                    }
                    Some(u) => CommandResult::fail(format!(
                        "unit {id} at {:.2} {:.2} {}, expected {x} {z} {slot:?}",
                        u.position[0], u.position[1], u.slot
                    )),
                    None => CommandResult::fail(format!("no unit {id} in the scene")),
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
        self.scene.scene_panel(e);
        self.scene.timeline_panel(e);
        self.scene.key_panel(e);
        self.scene.layout_panel(e);
        self.new_scene_popup(e);
        // The scene in the viewport, at the playhead (the movie export poses it frame by frame meanwhile).
        if self.movie_job.is_some() {
            self.step_movie(e);
        } else if let (Some(doc), Some(p)) = (e.docs.of_tool("cinematic"), scene_doc(e).map(|d| d.project().clone())) {
            if let Some(frame) = self.scene.frame(&p) {
                self.preview.free_camera = self.scene.free_camera;
                self.preview.update(e, doc, &p, &frame);
            }
        } else {
            self.preview.clear(e);
        }
    }

    fn unsaved_work(&self) -> Vec<String> {
        vec![]
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}
