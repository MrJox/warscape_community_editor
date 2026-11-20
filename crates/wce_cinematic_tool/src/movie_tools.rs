//! Movie editing in the Movie panel and File > Export (docs/plans/cinematic.md, step 9): the open movie's subtitles edited (times in
//! frames, the text) and written as a pack, its sound replaced from a WAV or OGG file, and new movies made from a video file (an
//! ffmpeg the user has installed) or a PNG sequence. The slow parts run on a worker; scripted runs wait for them.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::mpsc::{Receiver, channel};

use rpfm_lib::schema::Schema;
use wce_audio::import::read_sound;
use wce_audio::library::SoundLibrary;
use wce_cinematic::movie_edit::{
    EVENT_PICTURE, FULL_SCREEN, find_ffmpeg, movie_from_pngs, movie_from_video, new_movie_pack_files, replace_movie_sound,
    subtitles_pack_files,
};
use wce_cinematic::movies::Subtitle;
use wce_editor::editor::Editor;
use wce_editor::gui;
use wce_vfs::files::Files;

use crate::viewer::MovieViewer;

/// What a worker made: the pack it wrote, or why it could not.
type Done = Result<String, String>;

#[derive(Default)]
pub struct MovieTools {
    pub editing: bool,
    job: Option<(String, Receiver<Done>)>,
}

fn schema_of(files: &dyn Files) -> Result<Schema, String> {
    files
        .game()
        .and_then(wce_foundation::settings::rpfm_schema_path)
        .and_then(|p| Schema::load(&p, None).ok())
        .ok_or_else(|| "no RPFM schema for the game".to_owned())
}

fn write_pack(path: &Path, game: &str, files: Vec<(String, Vec<u8>)>) -> Done {
    let n = files.len();
    wce_walls::pack::write_pack(path, game, files, vec![])?;
    Ok(format!("wrote {} ({n} files)", path.display()))
}

fn game_key(files: &dyn Files) -> String {
    files.game().map_or("attila", |g| g.key).to_owned()
}

impl MovieTools {
    pub fn busy(&self) -> bool {
        self.job.is_some()
    }

    /// The worker's end: its message in the status line (scripted runs wait for it).
    pub fn poll(&mut self, e: &mut Editor) {
        let Some((what, rx)) = &self.job else { return };
        let got = if e.scripted_input() { rx.recv().ok() } else { rx.try_recv().ok() };
        if let Some(r) = got {
            match r {
                Ok(msg) => e.status(&format!("Cinematic: {what}: {msg}"), false),
                Err(err) => e.status(&format!("Cinematic: {what}: {err}"), true),
            }
            self.job = None;
        }
    }

    fn start(&mut self, e: &mut Editor, what: &str, work: impl FnOnce() -> Done + Send + 'static) {
        if self.job.is_some() {
            e.status("Cinematic: wait for the movie work under way", true);
            return;
        }
        let (tx, rx) = channel();
        std::thread::spawn(move || {
            let _ = tx.send(work());
        });
        e.status(&format!("Cinematic: {what}..."), false);
        self.job = Some((what.to_owned(), rx));
    }

    /// The open movie's subtitles (as edited) written to a pack.
    pub fn export_subtitles(&mut self, e: &mut Editor, v: &MovieViewer, files: Arc<dyn Files>, path: PathBuf) {
        let (name, script_ref, lines) = (v.entry.name.clone(), v.entry.script_ref, v.subtitles.clone());
        let language = v.language.clone().unwrap_or_else(|| "en".into());
        self.start(e, "subtitles pack", move || {
            let schema = schema_of(files.as_ref())?;
            let out = subtitles_pack_files(&name, script_ref, &language, &lines, files.as_ref(), &schema)?;
            write_pack(&path, &game_key(files.as_ref()), out)
        });
    }

    /// The open movie's sound replaced by a sound file, written to a pack.
    pub fn replace_sound(
        &mut self,
        e: &mut Editor,
        v: &MovieViewer,
        files: Arc<dyn Files>,
        library: Option<Arc<SoundLibrary>>,
        sound: PathBuf,
        path: PathBuf,
    ) {
        let name = v.entry.name.clone();
        self.start(e, "sound pack", move || {
            let pcm = std::fs::read(&sound)
                .map_err(|e| format!("{}: {e}", sound.display()))
                .and_then(|b| read_sound(&b).map_err(|e| e.to_string()))?;
            let lib = library.unwrap_or_else(|| Arc::new(SoundLibrary::load(files.as_ref())));
            let out = replace_movie_sound(&lib, files.as_ref(), &name, &pcm)?;
            write_pack(&path, &game_key(files.as_ref()), out)
        });
    }

    /// A new movie from a video file (through ffmpeg) or a folder of PNGs, written to a pack under `name`.
    pub fn import(
        &mut self,
        e: &mut Editor,
        files: Arc<dyn Files>,
        library: Option<Arc<SoundLibrary>>,
        source: PathBuf,
        event_picture: bool,
        path: PathBuf,
    ) {
        let name = path.file_stem().map_or_else(|| "wce_movie".into(), |s| s.to_string_lossy().into_owned());
        self.start(e, "movie import", move || {
            let size = if event_picture { EVENT_PICTURE } else { FULL_SCREEN };
            let (movie, sound) = if source.is_dir() {
                (movie_from_pngs(&source, 30, |_, _| {})?, None)
            } else {
                let ff = find_ffmpeg().ok_or("ffmpeg not found (set WCE_FFMPEG to it or its folder, or put it on the PATH)")?;
                movie_from_video(&ff, &source, size, 30, (None, None), |_| {})?
            };
            let lib = library.unwrap_or_else(|| Arc::new(SoundLibrary::load(files.as_ref())));
            let schema = schema_of(files.as_ref())?;
            let frames = movie.frame_count();
            let out = new_movie_pack_files(&name, &movie, sound.as_ref(), &lib, files.as_ref(), &schema)?;
            write_pack(&path, &game_key(files.as_ref()), out)
                .map(|m| format!("{m}: {frames} frames{}", if sound.is_some() { ", with sound" } else { "" }))
        });
    }

    /// The subtitle editor under the picture: each line's start and end frames and text; a line added at the current frame;
    /// a line removed. True when something changed.
    pub fn subtitle_editor(&mut self, v: &mut MovieViewer) -> bool {
        let mut changed = false;
        let frames = v.movie.frame_count().max(1) as i32;
        let mut remove = None;
        for (i, line) in v.subtitles.iter_mut().enumerate() {
            gui::push_id_int(i as i32);
            let (mut s, mut en) = (line.start as i32, line.end as i32);
            gui::set_next_item_width(80.0);
            changed |= gui::drag_int_range("##start", &mut s, 1.0, 0, frames, "%d");
            if gui::is_item_hovered() {
                gui::set_tooltip("First frame shown");
            }
            gui::same_line();
            gui::set_next_item_width(80.0);
            changed |= gui::drag_int_range("##end", &mut en, 1.0, 0, frames, "%d");
            if gui::is_item_hovered() {
                gui::set_tooltip("Frame it goes at");
            }
            gui::same_line();
            gui::set_next_item_width(-60.0);
            changed |= gui::input_text("##text", None, &mut line.text, 512, 0);
            gui::same_line();
            if gui::small_button("X") {
                remove = Some(i);
            }
            (line.start, line.end) = (s.max(0) as u32, en.max(s).max(0) as u32);
            gui::pop_id();
        }
        if let Some(i) = remove {
            v.subtitles.remove(i);
            changed = true;
        }
        if gui::small_button("Add line here") {
            let f = v.frame() as u32;
            v.subtitles.push(Subtitle { start: f, end: f + 90, text: String::new() });
            changed = true;
        }
        if gui::is_item_hovered() {
            gui::set_tooltip("A line from the current frame, 3 s long");
        }
        if changed {
            v.subtitles.sort_by_key(|l| l.start);
        }
        changed
    }

    /// The import entries of File > Export.
    pub fn import_menu(&mut self, e: &mut Editor, files: Option<Arc<dyn Files>>, library: Option<Arc<SoundLibrary>>) {
        let ffmpeg = find_ffmpeg().is_some();
        for (label, event, video) in [
            ("Movie from video (1080p)...", false, true),
            ("Event picture from video (380 x 214)...", true, true),
            ("Movie from PNG sequence...", false, false),
        ] {
            if gui::menu_item(label, None, false, files.is_some() && !self.busy() && (!video || ffmpeg)) {
                let source = if video {
                    let filters =
                        [("Videos".to_owned(), "*.mp4 *.mov *.mkv *.avi *.webm".to_owned()), ("All files".to_owned(), "*.*".to_owned())];
                    wce_vfs::steam_detect::file_dialog(false, Path::new(""), &filters)
                } else {
                    wce_vfs::steam_detect::pick_folder_at("A folder of PNG pictures", Path::new(""))
                };
                let filters = [("Mod pack".to_owned(), "*.pack".to_owned())];
                if let (Some(source), Some(files)) = (source, files.clone())
                    && let Some(path) = e.pick_export_file("cinematic_import_pack", "wce_movie.pack", &filters)
                {
                    self.import(e, files, library.clone(), source, event, path);
                }
            }
            if gui::is_item_hovered() {
                gui::set_tooltip(if video && !ffmpeg {
                    "Needs ffmpeg: set WCE_FFMPEG to it (or its folder), or put it on the PATH"
                } else {
                    "A new game movie with its sound event and videos row, in a pack named for the movie"
                });
            }
        }
    }
}
