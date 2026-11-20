//! The movie viewer (docs/plans/cinematic.md, step 2): a movie's pictures, its sound (`Play_Movie_<name>`, the stereo version by
//! default) and its subtitles, with play, pause, frame steps and scrubbing. Live, the sound drives the clock while it plays and a
//! worker decodes; in scripted runs the editor's fixed-step clock drives it and frames decode on the spot, so a shot is the same
//! every run.

use std::sync::Arc;
use std::sync::mpsc::{Receiver, channel};
use std::time::Instant;

use wce_audio::decode::{Pcm, decode_wem};
use wce_audio::library::{MediaRef, SoundLibrary};
use wce_audio_tool::player::Player;
use wce_cinematic::movies::{MovieCatalog, MovieEntry, Subtitle, movie_audio_versions, preferred_version};
use wce_editor::editor::Editor;
use wce_editor::gui::{self, v2};
use wce_editor::icons;
use wce_vfs::files::Files;
use wce_video::Movie;

use crate::frames::{FrameSource, FrameWorker, Picture};

const TEXTURE_KEY: &str = "cinematic/movie";

pub struct MovieViewer {
    pub entry: MovieEntry,
    pub movie: Arc<Movie>,
    source: FrameSource,
    worker: Option<FrameWorker>,
    /// The frame on the texture, and the texture's size.
    shown: Option<usize>,
    texture: Option<(u32, u32)>,
    requested: Option<usize>,
    /// Seconds into the movie.
    pub time: f64,
    pub playing: bool,
    pub languages: Vec<String>,
    pub language: Option<String>,
    pub subtitles: Vec<Subtitle>,
    pub show_subtitles: bool,
    /// The sound's versions (switch value, media), the one chosen, and its decoded samples.
    pub versions: Vec<(String, MediaRef)>,
    pub version: Option<usize>,
    pcm: Option<Arc<Pcm>>,
    decoding: Option<Receiver<Result<Pcm, String>>>,
    player: Player,
    /// The viewer keeps its own clock: the editor's ticks only while a viewport is drawn.
    last_update: Option<Instant>,
    pub error: Option<String>,
}

impl MovieViewer {
    pub fn open(entry: MovieEntry, bytes: &[u8], catalog: &MovieCatalog) -> Result<Self, String> {
        let movie = Arc::new(Movie::read(bytes).map_err(|e| e.to_string())?);
        let languages = catalog.languages_of(&entry.name);
        let language = catalog.best_language(&entry.name);
        let subtitles = language.as_deref().map(|l| catalog.subtitles(&entry.name, l)).unwrap_or_default();
        Ok(Self {
            source: FrameSource::new(movie.clone()),
            worker: None,
            movie,
            entry,
            shown: None,
            texture: None,
            requested: None,
            time: 0.0,
            playing: false,
            languages,
            language,
            subtitles,
            show_subtitles: true,
            versions: vec![],
            version: None,
            pcm: None,
            decoding: None,
            player: Player::default(),
            last_update: None,
            error: None,
        })
    }

    pub fn frame(&self) -> usize {
        ((self.time * self.movie.fps() as f64).floor().max(0.0) as usize).min(self.movie.frame_count().saturating_sub(1))
    }

    pub fn length(&self) -> f64 {
        self.movie.seconds()
    }

    pub fn set_language(&mut self, catalog: &MovieCatalog, language: &str) {
        self.language = Some(language.to_owned());
        self.subtitles = catalog.subtitles(&self.entry.name, language);
    }

    /// Finds the sound's versions once the sound library is there, and decodes the chosen one on a thread.
    pub fn attach_audio(&mut self, library: &SoundLibrary, files: &Arc<dyn Files>) {
        if !self.versions.is_empty() || self.version.is_some() {
            return;
        }
        self.versions = movie_audio_versions(library, files.as_ref(), &self.entry.name);
        self.version = preferred_version(&self.versions);
        // An empty list stays empty: Some(usize::MAX) marks "looked, nothing".
        if self.version.is_none() {
            self.version = Some(usize::MAX);
            return;
        }
        self.decode_version(library, files);
    }

    pub fn choose_version(&mut self, index: usize, library: &SoundLibrary, files: &Arc<dyn Files>) {
        if index < self.versions.len() && self.version != Some(index) {
            self.version = Some(index);
            self.decode_version(library, files);
        }
    }

    fn decode_version(&mut self, library: &SoundLibrary, files: &Arc<dyn Files>) {
        let Some((_, media)) = self.version.and_then(|v| self.versions.get(v)).cloned() else { return };
        self.player.stop();
        self.pcm = None;
        let bytes = library.media_bytes(&media, files.as_ref());
        let (tx, rx) = channel();
        std::thread::spawn(move || {
            let r = bytes.ok_or_else(|| "the sound's file is missing".to_owned()).and_then(|b| decode_wem(&b).map_err(|e| e.to_string()));
            let _ = tx.send(r);
        });
        self.decoding = Some(rx);
    }

    pub fn play(&mut self) {
        if self.time >= self.length() {
            self.time = 0.0;
        }
        self.playing = true;
        self.start_sound();
    }

    pub fn pause(&mut self) {
        self.playing = false;
        self.player.stop();
    }

    pub fn seek(&mut self, seconds: f64) {
        self.time = seconds.clamp(0.0, self.length());
        if self.playing {
            self.start_sound();
        }
    }

    pub fn seek_frame(&mut self, frame: usize) {
        self.seek((frame as f64 + 0.5) / self.movie.fps().max(1.0) as f64);
    }

    fn start_sound(&mut self) {
        let Some(pcm) = self.pcm.clone() else { return };
        let length = pcm.seconds().max(0.001);
        if self.player.play(pcm, &self.entry.name, None).is_ok() {
            self.player.seek((self.time / length) as f32);
        }
    }

    pub fn set_volume(&mut self, volume: f32) {
        self.player.set_volume(volume);
    }

    pub fn volume(&self) -> f32 {
        self.player.volume
    }

    /// Every frame: the clock, the sound's end, the decoded sound, and the picture for the current frame.
    pub fn update(&mut self, e: &mut Editor) {
        if let Some(rx) = &self.decoding
            && let Ok(r) = rx.try_recv()
        {
            self.decoding = None;
            match r {
                Ok(pcm) => {
                    self.pcm = Some(Arc::new(pcm));
                    if self.playing {
                        self.start_sound();
                    }
                }
                Err(err) => self.error = Some(format!("sound: {err}")),
            }
        }
        self.player.tick();
        let scripted = e.scripted_input();
        // Scripted runs advance one fixed step per frame (the same pictures every run); live, the wall clock.
        let now = Instant::now();
        let dt = if e.clock.fixed_step() { e.clock.step() } else { self.last_update.map_or(0.0, |t| now.duration_since(t).as_secs_f64()) };
        self.last_update = Some(now);
        if self.playing {
            if !scripted && self.player.playing() && self.player.loaded() {
                self.time = self.player.position() as f64 * self.player.length();
            } else {
                self.time += dt;
            }
            if self.time >= self.length() {
                self.time = self.length();
                self.playing = false;
                self.player.stop();
            }
        }

        let frame = self.frame();
        if self.shown == Some(frame) {
            return;
        }
        if scripted {
            match self.source.picture(frame) {
                Ok(p) => self.show(e, p),
                Err(err) => self.error = Some(err),
            }
            return;
        }
        let worker = self.worker.get_or_insert_with(|| FrameWorker::start(self.movie.clone()));
        if self.requested != Some(frame) {
            worker.request(frame);
            self.requested = Some(frame);
        }
        if let Some(r) = worker.poll() {
            match r {
                Ok(p) => self.show(e, p),
                Err(err) => self.error = Some(err),
            }
        }
    }

    fn show(&mut self, e: &mut Editor, p: Picture) {
        let (w, h) = (p.width, p.height);
        if self.texture != Some((w, h)) {
            e.images.forget(TEXTURE_KEY);
            e.images.submit_patchable(TEXTURE_KEY, w, h, p.rgba.as_ref().clone());
            self.texture = Some((w, h));
        } else {
            e.images.patch(TEXTURE_KEY, p.rgba, [0, 0, w, h]);
        }
        self.shown = Some(p.index);
    }

    /// Frees the texture (another movie, or the tool closes the viewer).
    pub fn close(&mut self, e: &mut Editor) {
        self.player.stop();
        e.images.forget(TEXTURE_KEY);
    }

    /// The picture with its subtitles, fitted to `size`.
    pub fn draw_picture(&self, e: &Editor, size: gui::Vec2) {
        let origin = gui::cursor_screen_pos();
        let (mw, mh) = (self.movie.width().max(1) as f32, self.movie.height().max(1) as f32);
        let scale = (size.x / mw).min(size.y / mh).max(0.01);
        let (w, h) = (mw * scale, mh * scale);
        let min = v2(origin.x + (size.x - w) * 0.5, origin.y + (size.y - h) * 0.5);
        let max = v2(min.x + w, min.y + h);
        let draw = gui::window_draw_list();
        draw.rect_filled(origin, v2(origin.x + size.x, origin.y + size.y), gui::col32(0, 0, 0, 255), 0.0);
        if let Some(image) = e.images.get(TEXTURE_KEY) {
            draw.image(image.id, min, max, v2(0.0, 0.0), v2(1.0, 1.0), gui::col32(255, 255, 255, 255));
        }
        if self.show_subtitles
            && let Some(line) = MovieCatalog::line_at(&self.subtitles, self.frame() as u32)
            && !line.text.is_empty()
        {
            // As the game: bottom centre, about 1/24 of the picture's height.
            let font = (h / 24.0).clamp(12.0, 48.0);
            let scale_text = font / gui::text_line_height().max(1.0);
            let tw = gui::calc_text_width(&line.text) * scale_text;
            let pos = v2(min.x + (w - tw) * 0.5, max.y - font * 2.2);
            draw.text_sized(font, v2(pos.x + 2.0, pos.y + 2.0), gui::col32(0, 0, 0, 220), &line.text);
            draw.text_sized(font, pos, gui::col32(240, 236, 220, 255), &line.text);
        }
        gui::dummy(size);
    }

    /// Play / pause, frame steps, the scrub bar, time, language, sound version and volume.
    pub fn draw_transport(&mut self, catalog: &MovieCatalog, library: Option<&Arc<SoundLibrary>>, files: Option<&Arc<dyn Files>>) {
        let count = self.movie.frame_count();
        let frame = self.frame();
        if gui::button(if self.playing { icons::PAUSE } else { icons::PLAY }) {
            if self.playing { self.pause() } else { self.play() }
        }
        if gui::is_item_hovered() {
            gui::set_tooltip(if self.playing { "Pause" } else { "Play" });
        }
        gui::same_line();
        if gui::button("<") {
            self.pause();
            self.seek_frame(frame.saturating_sub(1));
        }
        if gui::is_item_hovered() {
            gui::set_tooltip("Previous frame");
        }
        gui::same_line();
        if gui::button(">") {
            self.pause();
            self.seek_frame((frame + 1).min(count.saturating_sub(1)));
        }
        if gui::is_item_hovered() {
            gui::set_tooltip("Next frame");
        }
        gui::same_line();
        let fps = self.movie.fps().max(1.0) as f64;
        let label = format!("{} / {}  frame {} / {}", clock(self.time), clock(self.length()), frame, count.saturating_sub(1));
        gui::set_next_item_width((gui::content_region_avail().x - gui::calc_text_width(&label) - 16.0).max(80.0));
        let mut f = frame as i32;
        if gui::slider_int("##scrub", &mut f, 0, count.saturating_sub(1) as i32) {
            self.seek(f as f64 / fps + 0.5 / fps);
        }
        gui::same_line();
        gui::text(&label);

        if !self.languages.is_empty() {
            let names: Vec<&str> = self.languages.iter().map(String::as_str).collect();
            let mut current = self.language.as_ref().and_then(|l| self.languages.iter().position(|x| x == l)).map_or(0, |i| i as i32);
            gui::set_next_item_width(70.0);
            if gui::combo("Subtitles##lang", &mut current, &names) {
                let lang = self.languages[current as usize].clone();
                self.set_language(catalog, &lang);
            }
            if gui::is_item_hovered() {
                gui::set_tooltip("The subtitle timings' language; the text is the installed language's");
            }
            gui::same_line();
            gui::checkbox("Show##subs", &mut self.show_subtitles);
            gui::same_line();
        }
        if self.versions.len() > 1
            && let (Some(library), Some(files)) = (library, files)
        {
            let labels: Vec<String> =
                self.versions.iter().map(|(v, m)| if v.is_empty() { format!("#{}", m.id()) } else { v.clone() }).collect();
            let names: Vec<&str> = labels.iter().map(String::as_str).collect();
            let mut current = self.version.filter(|v| *v < self.versions.len()).map_or(0, |v| v as i32);
            gui::set_next_item_width(100.0);
            if gui::combo("Sound##version", &mut current, &names) {
                self.choose_version(current as usize, library, files);
            }
            if gui::is_item_hovered() {
                gui::set_tooltip("The sound event's versions (the game picks one by the speaker setup)");
            }
            gui::same_line();
        }
        let mut volume = self.volume();
        gui::set_next_item_width(90.0);
        if gui::slider_float("Volume", &mut volume, 0.0, 2.0, "%.2f") {
            self.set_volume(volume);
        }
        if self.decoding.is_some() {
            gui::same_line();
            gui::text_disabled("Decoding the sound...");
        } else if self.version == Some(usize::MAX) {
            gui::same_line();
            gui::text_disabled("No sound");
        }
        if let Some(err) = &self.error {
            gui::text_colored([1.0, 0.45, 0.4, 1.0], err);
        }
    }
}

fn clock(seconds: f64) -> String {
    let s = seconds.max(0.0);
    format!("{:02}:{:05.2}", (s / 60.0).floor() as u64, s % 60.0)
}
