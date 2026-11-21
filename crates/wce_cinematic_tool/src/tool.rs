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

use wce_cinematic::scene::{Interp, PathKey, Project, TrackContent, sanitize_key};

use crate::create::{Create, CreateRequest};
use crate::export::MovieJob;
use crate::items::{self, How, SceneItem};
use crate::logic_ui::LogicUi;
use crate::movie_tools::MovieTools;
use crate::preview::Preview;
use crate::scene_audio::SceneAudio;
use crate::scene_doc::SceneDocument;
use crate::scene_ui::{SceneUi, add_track, edit, for_track, scene_doc};
use crate::view::SceneView;
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
    Sample,
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
    BattlePreview(bool),
    SubtitleSet(usize, u32, u32, String),
    ExportSubtitles(String),
    ReplaceSound(String, String),
    Import(String, String, bool),
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
    speeds_loading: Option<Receiver<wce_cinematic::eval::SpeedTable>>,
    logic: LogicUi,
    movie_tools: MovieTools,
    scene_audio: SceneAudio,
    lists_loading: Option<Receiver<crate::pickers::GameLists>>,
    /// The sound events are in the lists.
    events_listed: bool,
    scene_list_at: u64,
    start_shown: bool,
    create: Create,
    view: SceneView,
    /// The unit last mirrored between the editor's selection and the panels' selected unit.
    synced_unit: Option<u32>,
    /// The game started on the scene's battle (its launch pack removed when it exits), and a launch waiting for "launch anyway".
    launched: Option<wce_launch::launch::Running>,
    /// The battle's real ground under the scene.
    ground: crate::ground::Ground,
    /// The movie export's options (its window open).
    movie_options: Option<MovieOptions>,
    /// Listening to a pick: its rendering and the player.
    audition_rx: Option<Receiver<Option<wce_audio::decode::Pcm>>>,
    audition_player: wce_audio_tool::player::Player,
    launch_question: Option<String>,
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
            let (tx, rx) = channel();
            let f = files.clone();
            std::thread::spawn(move || {
                let _ = tx.send(wce_cinematic::speeds::speed_table(f.as_ref()));
            });
            self.speeds_loading = Some(rx);
            let (tx, rx) = channel();
            let f = files.clone();
            std::thread::spawn(move || {
                let _ = tx.send(crate::pickers::GameLists::load(f.as_ref()));
            });
            self.lists_loading = Some(rx);
            self.events_listed = false;
        }
        // A scripted run waits for the list, so that its next line can open a movie.
        let scripted = e.scripted_input();
        if let Some(rx) = &self.lists_loading
            && let Ok(l) = if scripted { rx.recv().map_err(|_| std::sync::mpsc::TryRecvError::Disconnected) } else { rx.try_recv() }
        {
            self.scene.lists = Arc::new(l);
            self.lists_loading = None;
        }
        if !self.events_listed
            && self.lists_loading.is_none()
            && let Some(lib) = &self.library
        {
            let mut l = (*self.scene.lists).clone();
            l.add_events(lib);
            self.scene.lists = Arc::new(l);
            self.events_listed = true;
        }
        if let Some(rx) = &self.speeds_loading
            && let Ok(t) = if scripted { rx.recv().map_err(|_| std::sync::mpsc::TryRecvError::Disconnected) } else { rx.try_recv() }
        {
            self.scene.speeds = t;
            self.speeds_loading = None;
        }
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
        if (self.viewer.is_some() || e.docs.of_tool("cinematic").is_some())
            && self.library.is_none()
            && self.library_loading.is_none()
            && !e.scripted_input()
            && let Some(files) = self.files.clone()
        {
            let (tx, rx) = channel();
            std::thread::spawn(move || {
                let _ = tx.send(SoundLibrary::load(files.as_ref()));
            });
            self.library_loading = Some(rx);
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
            Pending::Sample => self.sample_scene(e),
            Pending::SceneTime(t) => {
                self.scene.time = t.max(0.0);
                self.scene.playing = false;
            }
            Pending::ScenePlay(on) => self.scene.playing = on,
            Pending::UnitAdd(key, army, x, z) => {
                let lists = self.scene.lists.clone();
                self.scene.unit = Some(crate::scene_ui::add_unit(e, &lists, &key, army, Some([x, z])));
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
            Pending::BattlePreview(on) => self.scene.battle_preview = on,
            Pending::SubtitleSet(i, start, end, text) => {
                if let Some(v) = self.viewer.as_mut() {
                    let line = wce_cinematic::movies::Subtitle { start, end: end.max(start), text };
                    if i < v.subtitles.len() {
                        v.subtitles[i] = line;
                    } else {
                        v.subtitles.push(line);
                    }
                    v.subtitles.sort_by_key(|l| l.start);
                }
            }
            Pending::ExportSubtitles(path) => {
                if let (Some(v), Some(files)) = (self.viewer.as_ref(), self.files.clone()) {
                    self.movie_tools.export_subtitles(e, v, files, std::path::PathBuf::from(path));
                }
            }
            Pending::ReplaceSound(sound, path) => {
                if let (Some(v), Some(files)) = (self.viewer.as_ref(), self.files.clone()) {
                    self.movie_tools.replace_sound(
                        e,
                        v,
                        files,
                        self.library.clone(),
                        std::path::PathBuf::from(sound),
                        std::path::PathBuf::from(path),
                    );
                }
            }
            Pending::Import(source, path, event) => {
                if let Some(files) = self.files.clone() {
                    self.movie_tools.import(
                        e,
                        files,
                        self.library.clone(),
                        std::path::PathBuf::from(source),
                        event,
                        std::path::PathBuf::from(path),
                    );
                }
            }
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
                e.status(&format!("Cinematic: new scene {name}"), false);
                self.scene_opened(e);
            }
            Err(err) => e.status(&format!("Cinematic: {err}"), true),
        }
    }

    /// The sample scene: opened from the projects folder, made and saved there the first time.
    fn sample_scene(&mut self, e: &mut Editor) {
        let game = self.files.as_ref().and_then(|f| f.game().map(|g| g.key.to_owned())).unwrap_or_else(|| "attila".into());
        let p = wce_cinematic::sample::sample_project(&game);
        let path = Self::scenes_dir(e).join(&p.battle.key).join("project.json");
        if path.exists() {
            self.open_scene(e, &path.display().to_string());
            return;
        }
        let mut doc = SceneDocument::new(p, path.clone());
        if let Err(err) = wce_editor_core::document::Document::save(&mut doc, &Default::default()) {
            e.status(&format!("Cinematic: {err}"), true);
            return;
        }
        match e.replace_document("cinematic", Box::new(doc)) {
            Ok(_) => {
                e.status(&format!("Cinematic: the sample scene, saved as {}", path.display()), false);
                self.scene_opened(e);
            }
            Err(err) => e.status(&format!("Cinematic: {err}"), true),
        }
    }

    /// A scene was opened or made: the panels to fill it in front, the viewport looking at the player's army's zone from behind it.
    fn scene_opened(&mut self, e: &mut Editor) {
        self.scene.reset();
        self.create.stop();
        e.selection.clear();
        gui::select_dock_tab("Scene");
        gui::select_dock_tab("Timeline");
        gui::select_dock_tab("Viewport");
        self.frame_player_zone(e);
    }

    /// The viewport behind the player's army's zone, looking over it (on the ground when it is shown).
    fn frame_player_zone(&mut self, e: &mut Editor) {
        let Some(p) = scene_doc(e).map(|d| d.project().clone()) else { return };
        let Some(a) = p.armies.iter().find(|a| a.player).or(p.armies.first()) else { return };
        let units = e.scene.game_units.max(1e-3);
        let (s, c) = a.deployment_orientation.sin_cos();
        let at = a.deployment_centre;
        let mut cam = e.viewport.camera();
        let (tx, tz) = (at[0] + s * 60.0, at[1] + c * 60.0);
        let (ex, ez) = (at[0] - s * 140.0, at[1] - c * 140.0);
        cam.target = wce_foundation::math::Float3::new(tx / units, self.ground.height(tx, tz) / units, tz / units);
        cam.eye = wce_foundation::math::Float3::new(ex / units, (self.ground.height(ex, ez) + 70.0) / units, ez / units);
        e.viewport.controller().set_camera(&cam);
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
                    e.status(&format!("Cinematic: opened {}", path.display()), false);
                    self.scene_opened(e);
                }
                Err(err) => e.status(&format!("Cinematic: {err}"), true),
            },
            Err(err) => e.status(&format!("Cinematic: {err}"), true),
        }
    }

    /// The scene as a historical battle pack at `path`.
    fn export_battle(&mut self, e: &mut Editor, path: &std::path::Path) -> Result<String, String> {
        let p = scene_doc(e).map(|d| d.project().clone()).ok_or("no scene open")?;
        let (out, warnings) = self.battle_pack_files(e, true)?;
        wce_walls::pack::write_pack(path, &p.game, out, vec![])?;
        for w in &warnings {
            e.status(&format!("Cinematic: {w}"), true);
        }
        Ok(format!("Cinematic: wrote {} ({} warnings)", path.display(), warnings.len()))
    }

    /// Launch in game: the battle's files as the launch pack, the game started on the battle (a dry run writes the pack into the
    /// build folder and stops).
    fn launch_battle(&mut self, e: &mut Editor, dry_run: bool, ignore_movie_packs: bool) -> Result<String, String> {
        if self.launched.is_some() {
            return Err("the game launched earlier is still running: close it first".to_owned());
        }
        let (out, warnings) = self.battle_pack_files(e, false)?;
        let p = crate::launch::prepare(e, out, dry_run, ignore_movie_packs)?;
        for w in &warnings {
            e.status(&format!("Cinematic: {w}"), true);
        }
        if dry_run {
            return Ok(format!("dry run: the launch pack {}; command line: {}", p.pack.display(), p.command));
        }
        let (running, started) = crate::launch::start(e, &p)?;
        self.launched = Some(running);
        Ok(format!("launched {started} into the battle ({}); the launch pack is removed when the game exits", p.xml_path))
    }

    /// A pick's play button: the sound or the moods' music rendered on a worker, then played (live runs only).
    fn audition(&mut self, e: &mut Editor) {
        self.audition_player.tick();
        if let Some((event, moods)) = self.scene.audition.take()
            && !e.scripted_input()
            && let (Some(lib), Some(files)) = (self.library.clone(), self.files.clone())
        {
            let (tx, rx) = channel();
            std::thread::spawn(move || {
                let _ = tx.send(wce_cinematic::movie_export::audition(&lib, files.as_ref(), event.as_deref(), &moods, 20.0));
            });
            self.audition_rx = Some(rx);
            e.status("Cinematic: getting the sound ready...", false);
        }
        if let Some(rx) = &self.audition_rx
            && let Ok(pcm) = rx.try_recv()
        {
            self.audition_rx = None;
            match pcm {
                Some(p) => {
                    let _ = self.audition_player.play(Arc::new(p), "audition", None);
                }
                None => e.status("Cinematic: that plays nothing (a state, a stop...), or its audio is in no pack", true),
            }
        }
    }

    /// The movie export's window: the clip, the size, the frame rate, the quality, motion blur, the subtitles' language; Export picks
    /// the pack's file and starts.
    fn movie_options_window(&mut self, e: &mut Editor) {
        let Some(o) = &mut self.movie_options else { return };
        let Some(p) = scene_doc(e).map(|d| d.project().clone()) else {
            self.movie_options = None;
            return;
        };
        gui::set_next_window_size(v2(420.0, 0.0), gui::COND_APPEARING);
        let mut open = true;
        let mut go = false;
        if gui::begin("Export movie###cin_movie_export", Some(&mut open), gui::WINDOW_NO_COLLAPSE | gui::WINDOW_NO_DOCKING) {
            let clips: Vec<String> = p.clips.iter().map(|c| format!("{} ({:.1} s)", c.name, c.length)).collect();
            let refs: Vec<&str> = clips.iter().map(String::as_str).collect();
            let mut ci = p.clips.iter().position(|c| c.id == o.clip).unwrap_or(0) as i32;
            gui::row("Clip");
            gui::set_next_item_width(-1.0);
            if gui::combo("##mclip", &mut ci, &refs)
                && let Some(c) = p.clips.get(ci as usize)
            {
                o.clip = c.id;
            }
            gui::row("Size");
            gui::set_next_item_width(-1.0);
            let names: Vec<String> = RESOLUTIONS.iter().map(|r| r.0.to_owned()).chain(["Custom".to_owned()]).collect();
            let nrefs: Vec<&str> = names.iter().map(String::as_str).collect();
            let mut ri = RESOLUTIONS.iter().position(|r| (r.1, r.2) == (o.width, o.height)).unwrap_or(RESOLUTIONS.len()) as i32;
            if gui::combo("##mres", &mut ri, &nrefs)
                && let Some(r) = RESOLUTIONS.get(ri as usize)
            {
                (o.width, o.height) = (r.1, r.2);
            }
            if ri as usize == RESOLUTIONS.len() {
                let (mut w, mut h) = (o.width as i32, o.height as i32);
                gui::row("Width");
                gui::set_next_item_width(-1.0);
                if gui::drag_int_range("##mw", &mut w, 2.0, 16, 7680, "%d px") {
                    o.width = (w.max(16) as u32) & !1;
                }
                gui::row("Height");
                gui::set_next_item_width(-1.0);
                if gui::drag_int_range("##mh", &mut h, 2.0, 16, 4320, "%d px") {
                    o.height = (h.max(16) as u32) & !1;
                }
            }
            gui::row("Frame rate");
            gui::set_next_item_width(-1.0);
            let mut fi = FRAME_RATES.iter().position(|&f| f == o.fps).unwrap_or(2) as i32;
            let frefs: Vec<String> = FRAME_RATES.iter().map(|f| format!("{f} frames a second")).collect();
            let frefs: Vec<&str> = frefs.iter().map(String::as_str).collect();
            if gui::combo("##mfps", &mut fi, &frefs) {
                o.fps = FRAME_RATES[fi.clamp(0, FRAME_RATES.len() as i32 - 1) as usize];
            }
            gui::row("Quality");
            gui::set_next_item_width(-1.0);
            let qrefs: Vec<&str> = QUALITIES.iter().map(|q| q.0).collect();
            let mut qi = QUALITIES.iter().position(|q| q.1 == o.kbps).unwrap_or(2) as i32;
            if gui::combo("##mq", &mut qi, &qrefs) {
                o.kbps = QUALITIES[qi.clamp(0, QUALITIES.len() as i32 - 1) as usize].1;
            }
            if gui::is_item_hovered() {
                gui::set_tooltip("The video's rate: vanilla's movies run at about 5.5 Mbit/s, the community's at 20");
            }
            gui::row("Motion blur");
            gui::checkbox("##mblur", &mut o.blur);
            if gui::is_item_hovered() {
                gui::set_tooltip("Four moments of a 180 degree shutter each frame (slower to render)");
            }
            gui::row("Subtitles");
            gui::set_next_item_width(-1.0);
            let mut li = crate::scene_ui::LANGUAGES.iter().position(|l| *l == o.language).unwrap_or(0) as i32;
            if gui::combo("##mlang", &mut li, crate::scene_ui::LANGUAGES) {
                o.language = crate::scene_ui::LANGUAGES[li.max(0) as usize].to_owned();
            }
            let frames = p.clip(o.clip).map_or(0.0, |c| c.length * f64::from(o.fps));
            gui::text_disabled(&format!("{frames:.0} frames of {} x {}", o.width, o.height));
            gui::separator();
            if gui::button("Export...") {
                go = true;
            }
            gui::same_line();
            if gui::button("Cancel") {
                open = false;
            }
        }
        gui::end();
        if go {
            let o = self.movie_options.take().expect("open");
            let name = sanitize_key(&p.battle.key);
            let filters = [("Mod pack".to_owned(), "*.pack".to_owned())];
            if let Some(path) = e.pick_export_file("cinematic_movie_pack", &format!("{name}_movie.pack"), &filters) {
                let mut q = p.clone();
                if let Some(c) = q.clip_mut(o.clip) {
                    c.fps = o.fps;
                    c.motion_blur = o.blur;
                }
                let doc = e.docs.of_tool("cinematic").unwrap_or_default();
                match MovieJob::new(path, q, o.clip, doc, (o.width, o.height), &o.language, Some(o.kbps)) {
                    Ok(job) => {
                        e.status(&format!("Cinematic: rendering {}...", job.name), false);
                        self.movie_job = Some(job);
                    }
                    Err(err) => e.status(&format!("Cinematic: {err}"), true),
                }
            }
        } else if !open {
            self.movie_options = None;
        }
    }

    /// Asks for a launch from the UI: done, or the question about stray movie packs.
    fn launch_asked(&mut self, e: &mut Editor, ignore_movie_packs: bool) {
        match self.launch_battle(e, false, ignore_movie_packs) {
            Ok(m) => e.status(&format!("Cinematic: {m}"), false),
            Err(err) if err.contains("movie-type packs") => self.launch_question = Some(err),
            Err(err) => e.status(&format!("Cinematic: launch in game: {err}"), true),
        }
    }

    /// The historical battle's files (scripts, tables, loc; with `picture`, the menu's picture from the viewport) and the warnings.
    fn battle_pack_files(&mut self, e: &mut Editor, picture: bool) -> Result<(Vec<(String, Vec<u8>)>, Vec<String>), String> {
        let p = scene_doc(e).map(|d| d.project().clone()).ok_or("no scene open")?;
        let files = self.files.clone().ok_or("the game's files are not loaded")?;
        let schema = files
            .game()
            .and_then(wce_foundation::settings::rpfm_schema_path)
            .and_then(|s| rpfm_lib::schema::Schema::load(&s, None).ok())
            .ok_or("no RPFM schema for the game")?;
        let (mut out, warnings) = wce_cinematic::battle_tables::battle_files(&p, files.as_ref(), &schema, self.scene.lang())?;
        if !picture {
            return Ok((out, warnings));
        }
        // The Historical Battles menu's picture (vanilla's: 512 x 256 RGBA): the viewport's view as it is now.
        let shot = format!("{}/screenshot_small.png", wce_cinematic::battle_export::script_folder(&p));
        let camera = e.viewport.camera();
        let mut target = wce_render::render_target::RenderTarget::default();
        match wce_editor::viewport::render_offscreen(e, &mut target, 512, 256, camera, e.clock.time() as f32, true) {
            Some(rgba) => {
                let png = wce_formats::png_file::PngFile {
                    width: 512,
                    height: 256,
                    bit_depth: 8,
                    color_type: 6,
                    pixels: rgba,
                    ..Default::default()
                };
                out.push((shot, png.encode().map_err(|e| format!("the menu picture: {e:?}"))?));
            }
            None => e.status("Cinematic: no renderer: the battle has no menu picture", true),
        }
        Ok((out, warnings))
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
        match MovieJob::new(path, p, clip, doc, (w.max(16), h.max(16)), self.scene.lang(), None) {
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
        match job.step(e, &mut self.preview, &mut self.ground, &self.scene.speeds, budget) {
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
        let editor_h = if self.movie_tools.editing { (avail.y * 0.35).max(120.0) } else { 0.0 };
        let transport = gui::frame_height_with_spacing() * 3.0 + 8.0 + editor_h;
        v.draw_picture(e, v2(avail.x, (avail.y - transport).max(60.0)));
        let catalog = self.catalog.clone().unwrap_or_default();
        v.draw_transport(&catalog, self.library.as_ref(), self.files.as_ref());
        gui::checkbox("Edit subtitles", &mut self.movie_tools.editing);
        gui::same_line();
        let busy = self.movie_tools.busy();
        gui::begin_disabled(busy);
        let filters = [("Mod pack".to_owned(), "*.pack".to_owned())];
        if gui::button("Subtitles pack...")
            && let Some(files) = self.files.clone()
            && let Some(path) = e.pick_export_file("cinematic_subtitles_pack", &format!("{}_subtitles.pack", v.entry.name), &filters)
        {
            self.movie_tools.export_subtitles(e, v, files, path);
        }
        if gui::is_item_hovered() {
            gui::set_tooltip("These subtitles (the language shown) as a mod pack: subtitle_timings rows and the loc");
        }
        gui::same_line();
        if gui::button("Replace sound...")
            && let Some(files) = self.files.clone()
        {
            let sound_filters = [("Sound files".to_owned(), "*.wav *.ogg".to_owned()), ("All files".to_owned(), "*.*".to_owned())];
            if let Some(sound) = wce_vfs::steam_detect::file_dialog(false, std::path::Path::new(""), &sound_filters)
                && let Some(path) = e.pick_export_file("cinematic_sound_pack", &format!("{}_sound.pack", v.entry.name), &filters)
            {
                self.movie_tools.replace_sound(e, v, files, self.library.clone(), sound, path);
            }
        }
        if gui::is_item_hovered() {
            gui::set_tooltip("Every version of Play_Movie_ plays a sound file of yours, as a mod pack");
        }
        gui::end_disabled();
        if self.movie_tools.editing {
            if gui::begin_child("##subtitles", v2(-1.0, editor_h - 8.0), gui::CHILD_BORDERS, 0) {
                self.movie_tools.subtitle_editor(v);
            }
            gui::end_child();
        }
        gui::end();
    }
}

impl CinematicTool {
    /// The tool's keys (Settings > Shortcuts): placing turns and stops; over the viewport or the Scene panel, delete, duplicate and
    /// the quick-add popup.
    fn scene_keys(&mut self, e: &mut Editor) {
        use wce_editor::shortcuts::pressed;
        if gui::io().WantTextInput || scene_doc(e).is_none() {
            return;
        }
        if self.create.making.is_some() {
            if pressed("cinematic.turn", true) {
                self.create.turn_by(15.0);
            }
            if pressed("cinematic.turn_back", true) {
                self.create.turn_by(-15.0);
            }
            if pressed("cinematic.cancel", false) {
                self.create.stop();
            }
        }
        let here = e.viewport.hovered || self.scene.outliner_hovered;
        if here && pressed("cinematic.delete", false) {
            self.scene.request_delete = true;
        }
        if here && pressed("cinematic.duplicate", false) {
            self.scene.request_duplicate = true;
        }
        if e.viewport.hovered
            && !gui::is_mouse_down(1)
            && pressed("cinematic.quick_add", false)
            && let Some(g) = self.view.hover_ground
        {
            self.create.ask_quick_add(g);
        }
    }

    /// What the panels asked for: select what was placed, delete or duplicate the selection, look at a unit.
    fn scene_requests(&mut self, e: &mut Editor) {
        for r in std::mem::take(&mut self.create.requests) {
            match r {
                CreateRequest::Placed(item) => {
                    items::select(e, item, How::Set);
                    self.scene.key_focus = false;
                }
                CreateRequest::KeyView => {
                    self.scene.look_through = false;
                    self.scene.key_camera_view(e);
                }
                CreateRequest::Record => self.scene.toggle_recording(),
            }
        }
        if std::mem::take(&mut self.scene.request_delete) {
            let sel = items::selected(e);
            if sel.is_empty() && self.scene.key_focus && self.scene.key.is_some() {
                return;
            }
            edit(e, "delete", |p| delete_items(p, &sel));
            e.selection.clear();
            self.scene.unit = None;
        }
        if std::mem::take(&mut self.scene.request_duplicate) {
            let sel = items::selected(e);
            let mut made = vec![];
            edit(e, "duplicate", |p| made = duplicate_items(p, &sel));
            e.selection.clear();
            for i in made {
                items::select(e, i, How::Add);
            }
        }
        if let Some(id) = self.scene.look_at.take()
            && let Some(d) = scene_doc(e)
            && let Some(m) = d.transform_of(SceneItem::Unit(id))
        {
            let mut cam = e.viewport.camera();
            let target = wce_foundation::math::Float3::new(m.m[0][3], m.m[1][3], m.m[2][3]);
            let units = e.scene.game_units.max(1e-3);
            let back = cam.eye - cam.target;
            let len = (back.x * back.x + back.y * back.y + back.z * back.z).sqrt().max(1e-6);
            let dist = (80.0 / units).max(1e-3);
            cam.eye = target + wce_foundation::math::Float3::new(back.x / len * dist, back.y / len * dist, back.z / len * dist);
            cam.target = target;
            self.scene.look_through = false;
            e.viewport.controller().set_camera(&cam);
        }
    }
}

/// Removes the items: units with their tracks, effects, armies with their units (a scene keeps one army).
pub fn delete_items(p: &mut Project, sel: &[SceneItem]) {
    let mut gone: Vec<u32> = sel.iter().filter_map(|i| if let SceneItem::Unit(id) = i { Some(*id) } else { None }).collect();
    let mut armies: Vec<usize> = sel.iter().filter_map(|i| if let SceneItem::Army(a) = i { Some(*a) } else { None }).collect();
    armies.sort_unstable_by(|a, b| b.cmp(a));
    armies.dedup();
    for a in armies {
        if p.armies.len() <= 1 || a >= p.armies.len() {
            continue;
        }
        gone.extend(p.units.iter().filter(|u| u.army == a).map(|u| u.id));
        p.armies.remove(a);
        for u in &mut p.units {
            if u.army > a {
                u.army -= 1;
            }
        }
    }
    if !p.armies.iter().any(|a| a.player)
        && let Some(a) = p.armies.first_mut()
    {
        a.player = true;
    }
    p.units.retain(|u| !gone.contains(&u.id));
    let mut keys: Vec<(u32, usize)> = sel
        .iter()
        .filter_map(|i| match i {
            SceneItem::CameraKey { track, index } | SceneItem::CameraTarget { track, index } => Some((*track, *index)),
            _ => None,
        })
        .collect();
    keys.dedup();
    keys.sort_unstable_by_key(|k| std::cmp::Reverse(k.1));
    for c in &mut p.clips {
        for t in &mut c.tracks {
            if let TrackContent::Camera { keys: list } = &mut t.content {
                for (track, index) in &keys {
                    if *track == t.id && *index < list.len() {
                        list.remove(*index);
                    }
                }
            }
        }
    }
    let mut effects: Vec<(u32, usize)> =
        sel.iter().filter_map(|i| if let SceneItem::Vfx { track, index } = i { Some((*track, *index)) } else { None }).collect();
    effects.sort_unstable_by_key(|e| std::cmp::Reverse(e.1));
    for c in &mut p.clips {
        c.tracks.retain(|t| !matches!(&t.content, TrackContent::Unit { unit, .. } if gone.contains(unit)));
        for t in &mut c.tracks {
            if let TrackContent::Vfx { instances } = &mut t.content {
                for (track, index) in &effects {
                    if *track == t.id && *index < instances.len() {
                        instances.remove(*index);
                    }
                }
            }
        }
    }
}

/// Copies of the units and effects, beside them; the copies' items.
pub fn duplicate_items(p: &mut Project, sel: &[SceneItem]) -> Vec<SceneItem> {
    let mut made = vec![];
    for i in sel {
        match *i {
            SceneItem::Unit(id) => {
                let Some(u) = p.unit(id).cloned() else { continue };
                let nid = p.new_id();
                let (sin, cos) = u.facing.sin_cos();
                let step = u.width + 5.0;
                p.units.push(wce_cinematic::scene::SceneUnit {
                    id: nid,
                    seed: nid,
                    start: [u.start[0] + step * cos, u.start[1] - step * sin],
                    general: false,
                    ..u
                });
                made.push(SceneItem::Unit(nid));
            }
            SceneItem::Vfx { track, index } => {
                let Some(mut v) = crate::items::vfx_of(p, track, index).cloned() else { continue };
                v.position[0] += 5.0;
                for c in &mut p.clips {
                    for t in &mut c.tracks {
                        if t.id == track
                            && let TrackContent::Vfx { instances } = &mut t.content
                        {
                            instances.push(v.clone());
                            instances.sort_by(|a, b| a.start.total_cmp(&b.start));
                            if let Some(k) = instances.iter().rposition(|x| *x == v) {
                                made.push(SceneItem::Vfx { track, index: k });
                            }
                        }
                    }
                }
            }
            _ => {}
        }
    }
    made
}

/// The movie export's choices.
#[derive(Clone, Debug)]
struct MovieOptions {
    clip: u32,
    width: u32,
    height: u32,
    fps: u32,
    kbps: u32,
    blur: bool,
    language: String,
}

/// The sizes offered (the game's movies are 1920 x 1080).
const RESOLUTIONS: &[(&str, u32, u32)] = &[
    ("3840 x 2160 (4K)", 3840, 2160),
    ("2560 x 1440", 2560, 1440),
    ("1920 x 1080 (the game's)", 1920, 1080),
    ("1280 x 720", 1280, 720),
    ("960 x 540", 960, 540),
];
const FRAME_RATES: &[u32] = &[24, 25, 30, 50, 60];
/// The video's rate cap, kbit/s.
const QUALITIES: &[(&str, u32)] =
    &[("Low (5.5 Mbit/s, as vanilla)", 5500), ("Medium (10 Mbit/s)", 10000), ("High (20 Mbit/s)", 20000), ("Best (40 Mbit/s)", 40000)];

/// The Cinematic tool's keys (Settings > Shortcuts).
const SHORTCUTS: &[wce_editor::shortcuts::Action] = {
    use wce_editor::shortcuts::{Action, Scope};
    const fn a(id: &'static str, label: &'static str, defaults: &'static [&'static str]) -> Action {
        Action { id, scope: Scope::Tool("cinematic"), group: "Cinematic", label, defaults, held: false }
    }
    &[
        a("cinematic.delete", "Delete the selection", &["Delete"]),
        a("cinematic.duplicate", "Duplicate the selection", &["Ctrl+D"]),
        a("cinematic.quick_add", "Add here... (the quick-add popup)", &["Space"]),
        a("cinematic.turn", "Turn what is placed", &["R"]),
        a("cinematic.turn_back", "Turn what is placed back", &["Shift+R"]),
        a("cinematic.cancel", "Stop placing", &["Escape"]),
    ]
};

impl Tool for CinematicTool {
    fn id(&self) -> &str {
        "cinematic"
    }

    fn reloads(&self, e: &Editor) -> bool {
        e.docs.of_tool("cinematic").is_some_and(|id| e.docs.find_as::<SceneDocument>(id).is_some())
    }

    /// File > Reload: the scene closed and read again from its project.json (the editor asked about unsaved changes first).
    fn reload(&mut self, e: &mut Editor) -> Option<Result<String, String>> {
        let id = e.docs.of_tool("cinematic")?;
        let path = wce_editor_core::document::Document::file_path(e.docs.find_as::<SceneDocument>(id)?)?;
        if !path.is_file() {
            return Some(Err(format!("{} is not saved yet: there is nothing to read again", path.display())));
        }

        e.close_document(id);
        self.open_scene(e, &path.display().to_string());
        let opened = e.docs.of_tool("cinematic").is_some();
        Some(if opened { Ok(format!("reloaded {}", path.display())) } else { Err(e.status_text().to_owned()) })
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
        if gui::menu_item("Open the sample cinematic scene", None, false, true) {
            self.sample_scene(e);
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

    fn restore(
        &mut self,
        _e: &mut Editor,
        copy: &std::path::Path,
        meta: &wce_editor_core::autosave::AutosaveMeta,
    ) -> Result<Option<Box<dyn wce_editor_core::document::Document>>, String> {
        let doc = SceneDocument::restored(&copy.join(&meta.inner), meta.target.clone())?;
        self.scene.reset();
        Ok(Some(Box::new(doc)))
    }

    /// The tool's menu, as the Tile tool's: Launch in game.
    fn draw_menus(&mut self, e: &mut Editor) {
        if !gui::begin_menu("Cinematic") {
            return;
        }
        let label = if self.launched.is_some() { "Launch in game (running)" } else { "Launch in game" };
        if gui::menu_item(label, None, false, scene_doc(e).is_some() && self.launched.is_none()) {
            self.launch_asked(e, false);
        }
        if gui::is_item_hovered_allow_disabled() {
            gui::set_tooltip(
                "Writes the scene's historical battle into a launch pack (data\\wce_launch.pack, removed when the game exits) and starts the game straight into it, with the mod packs chosen in the editor; used_mods.txt is not touched.",
            );
        }
        gui::end_menu();
    }

    fn shortcuts(&self) -> &'static [wce_editor::shortcuts::Action] {
        SHORTCUTS
    }

    fn inspect(&mut self, e: &mut Editor, ins: &mut dyn wce_editor_core::inspector::Inspector) -> bool {
        crate::inspect::inspect(e, ins, &mut self.scene, &mut self.create)
    }

    fn viewport_input(&mut self, e: &mut Editor, ev: &wce_editor::tool::ViewportEvent) -> bool {
        if ev.kind == wce_editor::tool::ViewportEventKind::Hover {
            self.view.hover_ground = crate::view::ground_point(e, ev.x, ev.y);
        }
        self.view.input(e, &mut self.scene, &mut self.create, &self.ground, ev)
    }

    fn viewport_overlay(&mut self, e: &mut Editor, origin: gui::Vec2, size: gui::Vec2) {
        self.view.overlay(e, &mut self.scene, &mut self.create, &self.ground, origin, size);
    }

    fn claims_left_button(&self, _e: &Editor) -> bool {
        self.create.making.is_some()
    }

    fn left_drag_selects(&self, e: &Editor) -> bool {
        self.create.making.is_none() && e.viewport.gizmo == wce_editor::viewport::GizmoMode::Off && e.docs.of_tool("cinematic").is_some()
    }

    fn box_drag(&mut self, e: &mut Editor, from: (i32, i32), to: (i32, i32), done: bool) {
        self.view.box_drag(e, &self.ground, from, to, done);
    }

    fn edit_menu(&mut self, e: &mut Editor) {
        let some = !items::selected(e).is_empty();
        if gui::menu_item(
            &format!("{}  Duplicate", wce_editor::icons::COPY),
            wce_editor::shortcuts::label("cinematic.duplicate").as_deref(),
            false,
            some,
        ) {
            self.scene.request_duplicate = true;
        }
        if gui::menu_item(
            &format!("{}  Delete", wce_editor::icons::TRASH_2),
            wce_editor::shortcuts::label("cinematic.delete").as_deref(),
            false,
            some,
        ) {
            self.scene.request_delete = true;
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
        if gui::menu_item("Movie pack...", None, false, has && self.movie_job.is_none()) {
            let p = scene_doc(e).map(|d| d.project().clone());
            let clip = p.as_ref().and_then(|p| self.scene.current_clip(p).cloned());
            if let Some(c) = clip {
                self.movie_options = Some(MovieOptions {
                    clip: c.id,
                    width: 1920,
                    height: 1080,
                    fps: c.fps.max(1),
                    kbps: 20000,
                    blur: c.motion_blur,
                    language: self.scene.lang().to_owned(),
                });
            }
        }
        if gui::is_item_hovered() {
            gui::set_tooltip("The timeline's clip rendered as a game movie, with its sound event, videos row and subtitles");
        }
        gui::separator();
        let (files, library) = (self.files.clone(), self.library.clone());
        self.movie_tools.import_menu(e, files, library);
        true
    }

    fn default_layout(&self, dockspace: gui::ImGuiID) {
        let (left, center) = gui::dock_builder_split(dockspace, gui::DIR_LEFT, 0.22);
        let (right, center) = gui::dock_builder_split(center, gui::DIR_RIGHT, 0.24);
        let (bottom, center) = gui::dock_builder_split(center, gui::DIR_DOWN, 0.32);
        let (left_bottom, left_top) = gui::dock_builder_split(left, gui::DIR_DOWN, 0.5);
        gui::dock_builder_dock_window("Movies", left_top);
        gui::dock_builder_dock_window("Scene", left_top);
        gui::dock_builder_dock_window("Create", left_bottom);
        gui::dock_builder_dock_window("Movie", center);
        gui::dock_builder_dock_window("Layout", center);
        gui::dock_builder_dock_window("Logic", center);
        gui::dock_builder_dock_window("Viewport", center);
        gui::dock_builder_dock_window("Inspector", right);
        gui::dock_builder_dock_window("Assets", bottom);
        gui::dock_builder_dock_window("Log", bottom);
        gui::dock_builder_dock_window("History", bottom);
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
        c.add(
            "scene_launch",
            "scene_launch [--dry-run] [--ignore-movie-packs]",
            "Launches the scene's battle in the game (a dry run: the pack and the command line only)",
            |e, a| {
                let dry = a.iter().any(|x| x == "--dry-run");
                let ignore = a.iter().any(|x| x == "--ignore-movie-packs");
                match e.with_tool::<CinematicTool, _>(|t, e| t.launch_battle(e, dry, ignore)) {
                    Some(Ok(m)) => CommandResult::success(m),
                    Some(Err(err)) => CommandResult::fail(err),
                    None => CommandResult::fail("the Cinematic tool is not built in"),
                }
            },
        );
        c.add("scene_sample", "scene_sample", "Opens the sample scene (made in <projects>/cinematics the first time)", |e, _| {
            queue(e, Pending::Sample)
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
        // The panels' ways of filling a scene, as a click would do them (the Create panel, the viewport, the Inspector's buttons).
        c.add(
            "create_pick",
            "create_pick unit|vfx <key> | create_pick none",
            "Starts placing a unit or an effect, as a click in the Create panel",
            |e, a| {
                let m = match (a.first().map(String::as_str), a.get(1)) {
                    (Some("unit"), Some(k)) => Some(crate::create::Making::Unit(k.clone())),
                    (Some("vfx"), Some(k)) => Some(crate::create::Making::Vfx(k.clone())),
                    (Some("camera"), _) => Some(crate::create::Making::CameraPath),
                    (Some("none"), _) => None,
                    _ => return CommandResult::fail("usage: create_pick unit|vfx <key> | create_pick none"),
                };
                match e.with_tool::<CinematicTool, _>(|t, _| t.create.making = m) {
                    Some(()) => CommandResult::success("placing"),
                    None => CommandResult::fail("the Cinematic tool is not built in"),
                }
            },
        );
        c.add("scene_map_pick", "scene_map_pick", "Opens the window picking the battle's place on its map", |e, _| {
            let Some(at) = scene_doc(e).map(|d| d.project().map.position) else { return CommandResult::fail("no scene open") };
            e.with_tool::<CinematicTool, _>(|t, _| t.scene.map_pick.open(at));
            CommandResult::success("map picker open")
        });
        c.add(
            "scene_free_camera",
            "scene_free_camera 0|1",
            "Paused, the viewport's camera free (1) or looking through the camera keys (0)",
            |e, a| {
                let on = a.first().is_some_and(|x| x == "1");
                e.with_tool::<CinematicTool, _>(|t, _| t.scene.look_through = !on);
                CommandResult::success(if on { "free" } else { "following the keys" })
            },
        );
        c.add(
            "click_ground",
            "click_ground <x> <z> [right]",
            "Clicks the viewport (the scripted mouse) where a point of the ground is (metres)",
            |e, a| {
                let n = |i: usize| a.get(i).and_then(|x| x.parse::<f32>().ok());
                let (Some(x), Some(z)) = (n(0), n(1)) else { return CommandResult::fail("usage: click_ground <x> <z> [right]") };
                let units = e.scene.game_units.max(1e-3);
                let h = e.with_tool::<CinematicTool, _>(|t, _| t.ground.height(x, z)).unwrap_or(0.0);
                let Some((px, py)) = e.viewport.project(wce_foundation::math::Float3::new(x / units, h / units, z / units)) else {
                    return CommandResult::fail("that point is not in view");
                };
                if px < 0.0 || py < 0.0 || px >= e.viewport.width() as f32 || py >= e.viewport.height() as f32 {
                    return CommandResult::fail(format!("that point is outside the picture ({px:.0}, {py:.0})"));
                }
                let mut args = vec!["click".to_owned(), (px.round() as i32).to_string(), (py.round() as i32).to_string()];
                if a.get(2).is_some_and(|b| b == "right") {
                    args.push("right".to_owned());
                }
                CommandRegistry::run_args(e, "mouse", &args)
            },
        );
        c.add(
            "assert_unit_near",
            "assert_unit_near <x> <z> <metres>",
            "Fails unless a unit starts within that distance of the point",
            |e, a| {
                let n = |i: usize| a.get(i).and_then(|x| x.parse::<f32>().ok());
                let (Some(x), Some(z), Some(d)) = (n(0), n(1), n(2)) else {
                    return CommandResult::fail("usage: assert_unit_near <x> <z> <metres>");
                };
                let Some(p) = scene_doc(e).map(|d| d.project().clone()) else { return CommandResult::fail("no scene open") };
                match p.units.iter().map(|u| (u, (u.start[0] - x).hypot(u.start[1] - z))).min_by(|a, b| a.1.total_cmp(&b.1)) {
                    Some((u, dist)) if dist <= d => CommandResult::success(format!("unit {} at {:?}", u.id, u.start)),
                    Some((u, dist)) => CommandResult::fail(format!("the nearest, unit {}, is {dist:.2} m away at {:?}", u.id, u.start)),
                    None => CommandResult::fail("no units"),
                }
            },
        );
        c.add(
            "ground_height",
            "ground_height <x> <z>...",
            "The battle's ground height at points (metres; 0 on the flat stand-in)",
            |e, a| {
                let n: Vec<f32> = a.iter().filter_map(|x| x.parse().ok()).collect();
                let r = e.with_tool::<CinematicTool, _>(|t, _| {
                    (
                        t.ground.shown(),
                        n.chunks_exact(2).map(|p| format!("({}, {}): {:.1}", p[0], p[1], t.ground.height(p[0], p[1]))).collect::<Vec<_>>(),
                    )
                });
                match r {
                    Some((shown, v)) => CommandResult::success(format!("{} {}", if shown { "ground" } else { "flat" }, v.join(", "))),
                    None => CommandResult::fail("the Cinematic tool is not built in"),
                }
            },
        );
        c.add(
            "scene_ground",
            "scene_ground whole <map> | position <map> <x> <y> | tile <folder>",
            "Where the battle is fought, as Battle > Fought on sets it",
            |e, a| {
                let n = |i: usize| a.get(i).and_then(|x| x.parse::<f32>().ok());
                let (kind, what) = (a.first().map(String::as_str), a.get(1).cloned().unwrap_or_default());
                if what.is_empty() {
                    return CommandResult::fail("usage: scene_ground whole <map> | position <map> <x> <y> | tile <folder>");
                }
                let mut ok = true;
                edit(e, "battle ground", |p| match kind {
                    Some("whole") => (p.map.campaign_map, p.map.whole_map, p.map.tile) = (what.clone(), true, String::new()),
                    Some("position") => {
                        (p.map.campaign_map, p.map.whole_map, p.map.tile) = (what.clone(), false, String::new());
                        p.map.position = [n(2).unwrap_or(0.5), n(3).unwrap_or(0.5)];
                    }
                    Some("tile") => p.map.tile = what.clone(),
                    _ => ok = false,
                });
                if ok { CommandResult::success(what) } else { CommandResult::fail("whole, position or tile") }
            },
        );
        c.add(
            "assert_ground_height",
            "assert_ground_height <x> <z> <min> <max>",
            "Fails unless the real ground is shown and its height there is in the range (metres)",
            |e, a| {
                let n: Vec<f32> = a.iter().filter_map(|x| x.parse().ok()).collect();
                if n.len() != 4 {
                    return CommandResult::fail("usage: assert_ground_height <x> <z> <min> <max>");
                }
                match e.with_tool::<CinematicTool, _>(|t, _| (t.ground.shown(), t.ground.height(n[0], n[1]), t.ground.status.clone())) {
                    Some((true, h, _)) if h >= n[2] && h <= n[3] => CommandResult::success(format!("{h:.1} m")),
                    Some((true, h, _)) => CommandResult::fail(format!("the ground is at {h:.1} m there")),
                    Some((false, _, status)) => CommandResult::fail(format!("no real ground shown ({status})")),
                    None => CommandResult::fail("the Cinematic tool is not built in"),
                }
            },
        );
        c.add(
            "scene_record",
            "scene_record",
            "Starts or stops recording the camera (the clip plays; the viewport's camera keyed every half second)",
            |e, _| {
                e.with_tool::<CinematicTool, _>(|t, _| t.scene.toggle_recording());
                CommandResult::success("recording toggled")
            },
        );
        c.add("create_army", "create_army <n>", "The army the units placed join (the Create panel's army)", |e, a| {
            let Some(n) = a.first().and_then(|x| x.parse::<usize>().ok()) else { return CommandResult::fail("usage: create_army <n>") };
            e.with_tool::<CinematicTool, _>(|t, _| t.create.draft.army = n);
            CommandResult::success(format!("army {n}"))
        });
        c.add(
            "create_click",
            "create_click <x> <z>",
            "Places what is being placed at a point of the ground (metres), as a click there",
            |e, a| {
                let n = |i: usize| a.get(i).and_then(|x| x.parse::<f32>().ok());
                let (Some(x), Some(z)) = (n(0), n(1)) else { return CommandResult::fail("usage: create_click <x> <z>") };
                let Some(p) = scene_doc(e).map(|d| d.project().clone()) else { return CommandResult::fail("no scene open") };
                let r = e.with_tool::<CinematicTool, _>(|t, e| {
                    let m = t.create.making.clone()?;
                    let lists = t.scene.lists.clone();
                    let clip = t.scene.current_clip(&p).map(|c| c.id);
                    let item = t.create.place(e, &lists, &p, &m, [x, z], clip, t.scene.time)?;
                    items::select(e, item, How::Set);
                    Some(item)
                });
                match r.flatten() {
                    Some(item) => CommandResult::success(format!("{item:?}")),
                    None => CommandResult::fail("nothing is being placed (create_pick first), or no clip for an effect"),
                }
            },
        );
        c.add(
            "scene_select",
            "scene_select battle | army <n> | unit <id> | camera <track> <index> | none [add]",
            "Selects a scene item (add: adds it)",
            |e, a| {
                let item = match (a.first().map(String::as_str), a.get(1).and_then(|x| x.parse::<u32>().ok())) {
                    (Some("battle"), _) => Some(SceneItem::Battle),
                    (Some("army"), Some(n)) => Some(SceneItem::Army(n as usize)),
                    (Some("unit"), Some(id)) => Some(SceneItem::Unit(id)),
                    (Some("camera"), Some(track)) => {
                        Some(SceneItem::CameraKey { track, index: a.get(2).and_then(|x| x.parse().ok()).unwrap_or(0) })
                    }
                    (Some("none"), _) => None,
                    _ => return CommandResult::fail("usage: scene_select battle | army <n> | unit <id> | none [add]"),
                };
                let add = a.last().is_some_and(|x| x == "add");
                match item {
                    Some(i) => items::select(e, i, if add { How::Add } else { How::Set }),
                    None => e.selection.clear(),
                }
                CommandResult::success(format!("{item:?}"))
            },
        );
        c.add("scene_move", "scene_move <dx> <dz> [turn degrees]", "Moves and turns the selection as the gizmo does (metres)", |e, a| {
            let n = |i: usize| a.get(i).and_then(|x| x.parse::<f32>().ok());
            let (Some(dx), Some(dz)) = (n(0), n(1)) else { return CommandResult::fail("usage: scene_move <dx> <dz> [turn]") };
            let turn = n(2).unwrap_or(0.0).to_radians();
            let sel = items::selected(e);
            let units = e.scene.game_units.max(1e-3);
            let Some(d) = scene_doc(e) else { return CommandResult::fail("no scene open") };
            // About each item's own place: a turn alone keeps it where it is.
            for i in &sel {
                let Some(m) = d.transform_of(*i) else { continue };
                let at = wce_foundation::math::Float3::new(m.m[0][3], m.m[1][3], m.m[2][3]);
                let delta = wce_foundation::math::translation(at + wce_foundation::math::Float3::new(dx / units, 0.0, dz / units))
                    * wce_foundation::math::rotation_y(turn)
                    * wce_foundation::math::translation(wce_foundation::math::Float3::new(-at.x, -at.y, -at.z));
                d.end_gesture();
                d.move_items(&[*i], &delta, 0);
                d.end_gesture();
            }
            CommandResult::success(format!("{} moved", sel.len()))
        });
        c.add("scene_delete", "scene_delete", "Deletes the selection", |e, _| {
            let sel = items::selected(e);
            edit(e, "delete", |p| delete_items(p, &sel));
            e.selection.clear();
            CommandResult::success(format!("{} deleted", sel.len()))
        });
        c.add("scene_duplicate", "scene_duplicate", "Duplicates the selection (the copies selected)", |e, _| {
            let sel = items::selected(e);
            let mut made = vec![];
            edit(e, "duplicate", |p| made = duplicate_items(p, &sel));
            e.selection.clear();
            for i in &made {
                items::select(e, *i, How::Add);
            }
            CommandResult::success(format!("{} made", made.len()))
        });
        c.add("assert_count", "assert_count units|armies|effects|selected <n>", "Fails unless the scene has that many", |e, a| {
            let (Some(what), Some(want)) = (a.first(), a.get(1).and_then(|x| x.parse::<usize>().ok())) else {
                return CommandResult::fail("usage: assert_count units|armies|effects|selected <n>");
            };
            let Some(p) = scene_doc(e).map(|d| d.project().clone()) else { return CommandResult::fail("no scene open") };
            let got = match what.as_str() {
                "units" => p.units.len(),
                "armies" => p.armies.len(),
                "effects" => p
                    .clips
                    .iter()
                    .flat_map(|c| &c.tracks)
                    .map(|t| if let TrackContent::Vfx { instances } = &t.content { instances.len() } else { 0 })
                    .sum(),
                "selected" => items::selected(e).len(),
                "camera_keys" => p
                    .clips
                    .iter()
                    .flat_map(|c| &c.tracks)
                    .map(|t| if let TrackContent::Camera { keys } = &t.content { keys.len() } else { 0 })
                    .sum(),
                _ => return CommandResult::fail("units, armies, effects or selected"),
            };
            if got == want {
                CommandResult::success(format!("{what}: {got}"))
            } else {
                CommandResult::fail(format!("{what}: {got}, expected {want}"))
            }
        });
        c.add(
            "assert_camera_key",
            "assert_camera_key <track> <index> <x> <y> <z> <tx> <ty> <tz>",
            "Fails unless the camera key is there and looks there (0.05 m)",
            |e, a| {
                let n: Vec<f32> = a.iter().skip(2).filter_map(|x| x.parse().ok()).collect();
                let (Some(track), Some(index)) =
                    (a.first().and_then(|x| x.parse::<u32>().ok()), a.get(1).and_then(|x| x.parse::<usize>().ok()))
                else {
                    return CommandResult::fail("usage: assert_camera_key <track> <index> <x> <y> <z> <tx> <ty> <tz>");
                };
                if n.len() != 6 {
                    return CommandResult::fail("six numbers: the position and the target");
                }
                let Some(k) = scene_doc(e).and_then(|d| crate::items::camera_key_of(d.project(), track, index).cloned()) else {
                    return CommandResult::fail(format!("no camera key {track} {index}"));
                };
                let got = [k.position[0], k.position[1], k.position[2], k.target[0], k.target[1], k.target[2]];
                if got.iter().zip(&n).all(|(g, w)| (g - w).abs() < 0.05) {
                    CommandResult::success(format!("{got:?}"))
                } else {
                    CommandResult::fail(format!("camera key at {got:?}, expected {n:?}"))
                }
            },
        );
        c.add(
            "assert_start",
            "assert_start <unit id> <x> <z> [facing degrees] [army]",
            "Fails unless the unit starts there (0.05 m, 0.5 degrees)",
            |e, a| {
                let n = |i: usize| a.get(i).and_then(|x| x.parse::<f32>().ok());
                let (Some(id), Some(x), Some(z)) = (a.first().and_then(|x| x.parse::<u32>().ok()), n(1), n(2)) else {
                    return CommandResult::fail("usage: assert_start <unit id> <x> <z> [facing] [army]");
                };
                let Some(u) = scene_doc(e).and_then(|d| d.project().unit(id).cloned()) else {
                    return CommandResult::fail(format!("no unit {id}"));
                };
                let facing_ok = n(3).is_none_or(|f| (crate::scene_doc::wrap_angle(u.facing - f.to_radians())).abs() < 0.5f32.to_radians());
                let army_ok = a.get(4).and_then(|x| x.parse::<usize>().ok()).is_none_or(|k| k == u.army);
                if (u.start[0] - x).abs() < 0.05 && (u.start[1] - z).abs() < 0.05 && facing_ok && army_ok {
                    CommandResult::success(format!("unit {id} at {:?}", u.start))
                } else {
                    CommandResult::fail(format!(
                        "unit {id} at {:?} facing {:.1} in army {}, expected {x} {z} {a:?}",
                        u.start,
                        u.facing.to_degrees(),
                        u.army
                    ))
                }
            },
        );
        c.add("unit_add", "unit_add <main_units key> <army> <x> <z>", "Adds a unit to the scene and selects it", |e, a| {
            let n = |i: usize| a.get(i).and_then(|x| x.parse::<f32>().ok());
            match (a.first(), a.get(1).and_then(|x| x.parse().ok()), n(2), n(3)) {
                (Some(k), Some(army), Some(x), Some(z)) => queue(e, Pending::UnitAdd(k.clone(), army, x, z)),
                _ => CommandResult::fail("usage: unit_add <key> <army> <x> <z>"),
            }
        });
        c.add("scene_unit_select", "scene_unit_select <id>", "Selects a unit of the scene", |e, a| {
            match a.first().and_then(|x| x.parse().ok()) {
                Some(id) => queue(e, Pending::UnitSelect(id)),
                None => CommandResult::fail("usage: scene_unit_select <id>"),
            }
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
        c.add("clip_blur", "clip_blur 0|1", "Motion blur in the current clip's movie", |e, a| {
            let on = a.first().is_none_or(|x| x != "0");
            let r = e.with_tool::<CinematicTool, _>(|t, e| {
                let p = scene_doc(e).map(|d| d.project().clone())?;
                let id = t.scene.current_clip(&p)?.id;
                edit(e, "motion blur", |p| {
                    if let Some(c) = p.clip_mut(id) {
                        c.motion_blur = on;
                    }
                });
                Some(())
            });
            match r {
                Some(Some(())) => CommandResult::success(format!("motion blur {on}")),
                _ => CommandResult::fail("no scene or clip"),
            }
        });
        c.add("scene_battle_preview", "scene_battle_preview 0|1", "Plays the scene as a battle would (units under orders)", |e, a| {
            queue(e, Pending::BattlePreview(a.first().is_none_or(|x| x != "0")))
        });
        c.add("cinematic_show", "cinematic_show <panel>", "Brings a panel to the front (Viewport, Layout, Movie, Timeline...)", |e, a| {
            queue(e, Pending::Show(a.join(" ")))
        });
        c.add(
            "movie_subtitle",
            "movie_subtitle <line> <start> <end> <text...>",
            "Sets (or adds, past the last) a subtitle line of the open movie (frames)",
            |e, a| {
                let n = |i: usize| a.get(i).and_then(|x| x.parse::<u32>().ok());
                match (n(0), n(1), n(2)) {
                    (Some(i), Some(s), Some(en)) => queue(e, Pending::SubtitleSet(i as usize, s, en, a[3..].join(" "))),
                    _ => CommandResult::fail("usage: movie_subtitle <line> <start> <end> <text...>"),
                }
            },
        );
        c.add(
            "movie_export_subtitles",
            "movie_export_subtitles <file.pack>",
            "Writes the open movie's subtitles as a pack",
            |e, a| match a.first() {
                Some(p) => queue(e, Pending::ExportSubtitles(p.clone())),
                None => CommandResult::fail("usage: movie_export_subtitles <file.pack>"),
            },
        );
        c.add(
            "movie_replace_sound",
            "movie_replace_sound <sound.wav> <file.pack>",
            "Writes a pack where the open movie plays a sound file",
            |e, a| match (a.first(), a.get(1)) {
                (Some(s), Some(p)) => queue(e, Pending::ReplaceSound(s.clone(), p.clone())),
                _ => CommandResult::fail("usage: movie_replace_sound <sound> <file.pack>"),
            },
        );
        c.add(
            "movie_import",
            "movie_import <video | png folder> <file.pack> [event]",
            "A new movie from a video (ffmpeg) or PNGs, as a pack",
            |e, a| match (a.first(), a.get(1)) {
                (Some(s), Some(p)) => queue(e, Pending::Import(s.clone(), p.clone(), a.get(2).is_some_and(|x| x == "event"))),
                _ => CommandResult::fail("usage: movie_import <source> <file.pack> [event]"),
            },
        );
        c.add("logic_node", "logic_node <kind> [key=value...]", "Adds a battle logic node (event.* / action.*)", |e, a| {
            let Some(kind) = a.first().cloned() else { return CommandResult::fail("usage: logic_node <kind> [key=value...]") };
            if wce_cinematic::logic::node_type(&kind).is_none() {
                return CommandResult::fail(format!("no node kind {kind}"));
            }
            let values: std::collections::BTreeMap<String, String> =
                a[1..].iter().filter_map(|kv| kv.split_once('=')).map(|(k, v)| (k.to_owned(), v.to_owned())).collect();
            let mut made = 0;
            edit(e, "add logic node", |p| {
                let id = p.logic.nodes.iter().map(|n| n.id).max().unwrap_or(0) + 1;
                let x = 40.0 + 220.0 * ((id - 1) % 3) as f32;
                let y = 40.0 + 160.0 * ((id - 1) / 3) as f32;
                p.logic.nodes.push(wce_cinematic::scene::LogicNode { id, kind, values, position: [x, y] });
                made = id;
            });
            if made == 0 { CommandResult::fail("no scene open") } else { CommandResult::success(format!("node {made}")) }
        });
        c.add("logic_link", "logic_link <event node> <action node>", "Links an event node to an action node", |e, a| {
            match (a.first().and_then(|x| x.parse::<u32>().ok()), a.get(1).and_then(|x| x.parse::<u32>().ok())) {
                (Some(from), Some(to)) => {
                    edit(e, "logic link", |p| {
                        p.logic.links.push(wce_cinematic::scene::LogicLink { from, from_pin: "out".into(), to, to_pin: "in".into() })
                    });
                    CommandResult::success(format!("{from} -> {to}"))
                }
                _ => CommandResult::fail("usage: logic_link <from> <to>"),
            }
        });
        c.add("assert_logic", "assert_logic <text...>", "Fails unless the compiled battle logic contains the text", |e, a| {
            let want = a.join(" ");
            let Some(p) = scene_doc(e).map(|d| d.project().clone()) else { return CommandResult::fail("no scene open") };
            let lua = wce_cinematic::logic::compile(&p, &mut vec![]);
            // The script parser takes double quotes as grouping: compare without them.
            if lua.replace('"', "").contains(&want.replace('"', "")) {
                CommandResult::success(want)
            } else {
                CommandResult::fail(format!(
                    "the logic has no \"{want}\":
{lua}"
                ))
            }
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
        // No scene: the start buttons offer the projects folder's scenes; what they ask for is done here.
        if e.docs.of_tool("cinematic").is_none() && gui::time() as u64 != self.scene_list_at {
            self.scene_list_at = gui::time() as u64;
            let mut scenes: Vec<String> = std::fs::read_dir(Self::scenes_dir(e))
                .map(|r| {
                    r.flatten()
                        .filter(|d| d.path().join("project.json").is_file())
                        .map(|d| d.file_name().to_string_lossy().into_owned())
                        .collect()
                })
                .unwrap_or_default();
            scenes.sort();
            self.scene.scenes = scenes;
        }
        // Opening the tool without a scene: the panels with the start buttons in front, once.
        if e.docs.of_tool("cinematic").is_none() {
            if !self.start_shown {
                gui::select_dock_tab("Scene");
                gui::select_dock_tab("Layout");
                self.start_shown = true;
            }
        } else {
            self.start_shown = false;
        }
        match self.scene.request.take() {
            Some(crate::scene_ui::SceneRequest::New(name)) => self.new_scene_project(e, &name),
            Some(crate::scene_ui::SceneRequest::Open(name)) => self.open_scene(e, &name),
            Some(crate::scene_ui::SceneRequest::Sample) => self.sample_scene(e),
            None => {}
        }
        self.movie_tools.poll(e);
        self.create.set_files(self.files.as_ref(), self.mount_id as usize);
        self.create.sync_draft(&self.scene.lists);
        // The editor's selection and the panels' selected unit follow each other.
        let sel_unit = match items::primary(e) {
            Some(SceneItem::Unit(id)) => Some(id),
            _ => None,
        };
        if sel_unit != self.synced_unit {
            self.scene.unit = sel_unit;
            self.synced_unit = sel_unit;
        }
        self.movies_panel(e);
        self.movie_panel(e);
        let lists = self.scene.lists.clone();
        let project = scene_doc(e).map(|d| d.project().clone());
        // Recording the camera keys the view as the clip plays.
        self.scene.record(e);
        // The playhead moves whichever panels are shown.
        if let Some(p) = &project
            && let Some(len) = self.scene.current_clip(p).map(|c| c.length)
        {
            self.scene.tick(e, len);
        }
        crate::outliner::panel(e, &mut self.scene, &mut self.create);
        {
            let (create, scene) = (&mut self.create, &mut self.scene);
            create.panel(e, &lists, project.as_ref(), &mut || scene.start_buttons());
        }
        self.scene.timeline_panel(e);
        self.scene.layout_panel(e, &mut self.create);
        self.logic.panel(e, &lists);
        self.new_scene_popup(e);
        if let Some(p) = &project {
            let (clip, time) = (self.scene.current_clip(p).map(|c| c.id), self.scene.time);
            self.create.quick_add(e, &lists, p, clip, time);
        }
        self.scene_keys(e);
        self.scene_requests(e);
        if let Some(r) = &mut self.launched
            && let Some(m) = r.poll()
        {
            e.status(&format!("Cinematic: launch in game: {m}"), false);
            self.launched = None;
        }
        if self.launch_question.is_some() {
            gui::open_popup("Launch in game##cinematic");
        }
        if gui::begin_popup_modal("Launch in game##cinematic", None, gui::WINDOW_ALWAYS_AUTO_RESIZE) {
            gui::text_wrapped(self.launch_question.as_deref().unwrap_or(""));
            if gui::button("Launch anyway") {
                self.launch_question = None;
                gui::close_current_popup();
                self.launch_asked(e, true);
            }
            gui::same_line();
            if gui::button("Cancel") {
                self.launch_question = None;
                gui::close_current_popup();
            }
            gui::end_popup();
        }
        self.movie_options_window(e);
        if self.scene.launch_request.take().is_some() {
            self.launch_asked(e, false);
        }
        self.audition(e);
        if let Some(p) = &project
            && let Some(pos) = self.scene.map_pick.window(e, self.files.as_ref(), &p.map.campaign_map, p.map.position)
        {
            edit(e, "map position", |p| p.map.position = pos);
        }
        if self.scene.unit != self.synced_unit {
            match self.scene.unit {
                Some(id) => items::select(e, SceneItem::Unit(id), How::Set),
                None if matches!(items::primary(e), Some(SceneItem::Unit(_))) => e.selection.clear(),
                None => {}
            }
            self.synced_unit = self.scene.unit;
        }
        // The scene in the viewport, at the playhead (the movie export poses it frame by frame meanwhile).
        if self.movie_job.is_some() {
            self.step_movie(e);
        } else if let (Some(doc), Some(p)) = (e.docs.of_tool("cinematic"), scene_doc(e).map(|d| d.project().clone())) {
            // The scene's sound while it plays (live only): it leads the playhead.
            if !e.scripted_input()
                && let Some(clip) = self.scene.current_clip(&p).map(|c| c.id)
            {
                let version = e.docs.find(doc).map_or(0, |d| d.history().version());
                if let Some(t) = self.scene_audio.update(
                    version,
                    &p,
                    clip,
                    self.scene.playing,
                    self.scene.time,
                    self.library.as_ref(),
                    self.files.as_ref(),
                ) && self.scene.playing
                {
                    self.scene.time = t;
                }
            }
            if let Some(mut frame) = self.scene.frame(&p) {
                // Where things are at the playhead, for the gizmo and the viewport's clicks.
                let units = e.scene.game_units;
                if let Some(d) = scene_doc(e) {
                    d.view = crate::scene_doc::DocView {
                        clip: self.scene.clip,
                        time: self.scene.time,
                        units,
                        placed: frame.units.iter().map(|s| (s.unit, (s.position, s.facing))).collect(),
                        heights: Default::default(),
                    };
                }
                frame.units.retain(|s| !self.scene.hidden.contains(&s.unit));
                self.ground.update(e, doc, &p);
                if self.ground.take_arrived() {
                    self.frame_player_zone(e);
                }
                if let Some(d) = scene_doc(e) {
                    d.view.heights = d.view.placed.iter().map(|(&id, (at, _))| (id, self.ground.height(at[0], at[1]))).collect();
                }
                self.preview.free_camera = !self.scene.follows_keys();
                let heights = self.ground.heights();
                self.preview.update(e, doc, &p, &frame, heights.as_ref().map(|h| h as &dyn Fn(f32, f32) -> f32));
            }
        } else {
            self.preview.clear(e);
            self.ground.clear(e);
            self.scene_audio.stop();
        }
    }

    fn unsaved_work(&self) -> Vec<String> {
        vec![]
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}
