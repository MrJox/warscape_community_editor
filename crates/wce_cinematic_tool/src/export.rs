//! File > Export > Movie pack (docs/plans/cinematic.md, step 5): a clip rendered frame by frame offscreen through the preview (the
//! same scene the viewport shows), the fades and cinematic bars laid over each picture, encoded as the game's VP8 movie; its sound
//! mixed down and given a `Play_Movie_<name>` event; the `videos` and `subtitle_timings` rows and loc; all in one mod pack. A live
//! export renders a few frames per editor frame (the editor stays responsive, with its progress); a scripted one all at once.

use std::path::PathBuf;
use std::sync::Arc;

use rpfm_lib::schema::Schema;
use wce_audio::library::SoundLibrary;
use wce_cinematic::eval::{SceneFrame, SpeedTable, evaluate};
use wce_cinematic::movie_export::{MovieEncoder, frame_count, mixdown, movie_files, subtitle_frames};
use wce_cinematic::scene::{Project, sanitize_key};
use wce_editor::editor::Editor;
use wce_render::render_target::RenderTarget;
use wce_scene::render_scene::DocumentId;
use wce_vfs::files::Files;
use wce_video::EncoderSettings;

use crate::file_export::{Format, Shot, Sink};
use crate::preview::{Preview, camera_of};

/// How a movie export writes: the format, one file per shot (with handle frames each side), the fade and the bars laid over.
#[derive(Clone, Debug)]
pub struct Output {
    pub format: Format,
    pub per_shot: bool,
    pub handles: usize,
    pub fade: bool,
    pub bars: bool,
}

impl Default for Output {
    fn default() -> Self {
        Self { format: Format::GamePack, per_shot: false, handles: 0, fade: true, bars: true }
    }
}

pub struct MovieJob {
    pub path: PathBuf,
    pub name: String,
    project: Project,
    clip: u32,
    doc: DocumentId,
    width: u32,
    height: u32,
    language: String,
    next: usize,
    total: usize,
    encoder: Option<MovieEncoder>,
    target: RenderTarget,
    out: Output,
    /// The runs written (the game's movie: the clip whole), the one being written, frames written so far.
    runs: Vec<Shot>,
    run: usize,
    written_frames: usize,
    sink: Option<Sink>,
    /// The clip's sound (files: each run's stretch of it beside its pictures), and the files written.
    sound: Option<wce_audio::decode::Pcm>,
    written: Vec<PathBuf>,
}

impl MovieJob {
    /// `kbps`: the encoder's rate cap (None: the default, 20 Mbit/s). `path`: the pack; for files, the folder and the base name
    /// (its stem). `sound`: the game's files and sound library, for files' sound (mixed at once).
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        path: PathBuf,
        project: Project,
        clip: u32,
        doc: DocumentId,
        size: (u32, u32),
        language: &str,
        kbps: Option<u32>,
        out: Output,
        sound: Option<(&Arc<dyn Files>, Option<&SoundLibrary>)>,
    ) -> Result<Self, String> {
        let (width, height) = size;
        let c = project.clip(clip).ok_or("no such clip")?;
        let name = format!("{}_{}", sanitize_key(&project.battle.key), sanitize_key(&c.name));
        let mut settings = EncoderSettings::movie(width, height, c.fps.max(1));
        if let Some(k) = kbps {
            settings.target_kbps = k;
        }
        let total = frame_count(c);
        let game = out.format == Format::GamePack;
        // Files: the runs (the clip whole, or its shots), named after the file picked.
        let base = path.file_stem().map_or(name.clone(), |s| s.to_string_lossy().into_owned());
        let shots: Vec<(f64, f64, u32, String)> = wce_cinematic::eval::shots(c)
            .into_iter()
            .map(|(a, b, cam)| (a, b, cam, c.tracks.iter().find(|t| t.id == cam).map_or(String::new(), |t| t.name.clone())))
            .collect();
        let runs = if game {
            vec![Shot { from: 0, to: total, camera: None, stem: name.clone() }]
        } else {
            crate::file_export::runs(&base, total, f64::from(c.fps.max(1)), out.per_shot.then_some(shots.as_slice()), out.handles)
        };
        let sound = match (game, sound) {
            (false, Some((files, lib))) => {
                let owned;
                let lib = match lib {
                    Some(l) => l,
                    None => {
                        owned = SoundLibrary::load(files.as_ref());
                        &owned
                    }
                };
                let pcm = mixdown(&project, c, lib, files.as_ref(), &mut vec![]);
                (!pcm.samples.iter().all(|s| s.abs() < 1e-6)).then_some(pcm)
            }
            _ => None,
        };
        Ok(Self {
            path,
            name,
            clip,
            doc,
            width,
            height,
            language: language.to_owned(),
            next: 0,
            encoder: if game { Some(MovieEncoder::new(&settings)?) } else { None },
            target: RenderTarget::default(),
            project,
            total: runs.iter().map(|r| r.to - r.from).sum(),
            runs,
            run: 0,
            written_frames: 0,
            sink: None,
            sound,
            written: vec![],
            out,
        })
    }

    pub fn progress(&self) -> (usize, usize) {
        (self.written_frames, self.total)
    }

    /// Renders up to `budget` frames (all with `None`). Ok(true) when every frame is in.
    pub fn step(
        &mut self,
        e: &mut Editor,
        preview: &mut Preview,
        ground: &mut crate::ground::Ground,
        speeds: &SpeedTable,
        budget: Option<usize>,
    ) -> Result<bool, String> {
        let clip = self.project.clip(self.clip).ok_or("the clip is gone")?.clone();
        let fps = clip.fps.max(1) as f64;
        let mut done = 0;
        while self.run < self.runs.len() && budget.is_none_or(|b| done < b) {
            let run = self.runs[self.run].clone();
            // A run starts: its first frame, its file (files).
            let first = self.sink.is_none() && (self.out.format != Format::GamePack || self.written_frames == 0);
            if first {
                self.next = run.from;
                if self.out.format != Format::GamePack {
                    let dir = self.path.parent().map(PathBuf::from).unwrap_or_default();
                    let stretch = self.sound.as_ref().map(|s| (s, run.from as f64 / fps, run.to as f64 / fps));
                    self.sink = Some(Sink::open(self.out.format, &dir, &run, (self.width, self.height), clip.fps.max(1), stretch)?);
                }
            }
            if self.next >= run.to {
                if let Some(s) = self.sink.take() {
                    self.written.push(s.finish()?);
                }
                self.run += 1;
                continue;
            }
            let t = self.next as f64 / fps;
            // A shot's run sees through its camera all along (its handles too, past its cuts).
            let seen = |f: wce_cinematic::eval::SceneFrame| match run.camera {
                Some(cam) => {
                    let mut f = f;
                    if let Some((_, c)) = f.cameras.iter().find(|(id, _)| *id == cam) {
                        f.camera = Some(c.clone());
                    }
                    f
                }
                None => f,
            };
            let frame = seen(evaluate(&self.project, &clip, speeds, t));
            // Pose the scene; wait for the units' models (a live export comes back next frame).
            let units = e.scene.game_units.max(1e-3);
            if let Some(c) = &frame.camera {
                e.viewport.controller().set_camera(&camera_of(c, units));
            }
            ground.follow(e);
            let heights = ground.heights();
            preview.update(e, self.doc, &self.project, &frame, heights.as_ref().map(|h| h as &dyn Fn(f32, f32) -> f32));
            if preview.busy() {
                if e.scripted_input() {
                    continue;
                }
                return Ok(false);
            }
            let units = e.scene.game_units.max(1e-3);
            let cam = match &frame.camera {
                Some(c) => camera_of(c, units),
                None => e.viewport.camera(),
            };
            let mut rgba =
                wce_editor::viewport::render_offscreen(e, &mut self.target, self.width, self.height, cam, t as f32, self.next == run.from)
                    .ok_or("the renderer could not draw the frame")?;
            // Motion blur: the frame and three more moments of its 180 degree shutter, averaged (the scene posed at each).
            if clip.motion_blur {
                const SUB: usize = 4;
                let mut sum: Vec<u32> = rgba.iter().map(|&v| u32::from(v)).collect();
                for k in 1..SUB {
                    let ts = t + k as f64 / SUB as f64 * 0.5 / fps;
                    let sub = seen(evaluate(&self.project, &clip, speeds, ts));
                    preview.update(e, self.doc, &self.project, &sub, heights.as_ref().map(|h| h as &dyn Fn(f32, f32) -> f32));
                    let cam = sub.camera.as_ref().map_or(cam, |c| camera_of(c, units));
                    let more = wce_editor::viewport::render_offscreen(e, &mut self.target, self.width, self.height, cam, ts as f32, false)
                        .ok_or("the renderer could not draw the frame")?;
                    sum.iter_mut().zip(&more).for_each(|(s, &v)| *s += u32::from(v));
                }
                rgba = sum.into_iter().map(|s| ((s + SUB as u32 / 2) / SUB as u32) as u8).collect();
            }
            overlay(&mut rgba, self.width, self.height, &frame, self.out.fade, self.out.bars);
            match &mut self.sink {
                Some(s) => s.push(&rgba, (self.width, self.height))?,
                None => self.encoder.as_mut().ok_or("finished")?.push_rgba(&rgba)?,
            }
            self.next += 1;
            self.written_frames += 1;
            done += 1;
        }
        Ok(self.run >= self.runs.len())
    }

    /// Encodes the rest, mixes the sound, writes the pack. Returns what was written and the warnings.
    pub fn finish(mut self, files: &Arc<dyn Files>, library: Option<&SoundLibrary>) -> Result<(String, Vec<String>), String> {
        if self.out.format != Format::GamePack {
            let dir = self.path.parent().map_or(String::new(), |d| d.display().to_string());
            let names: Vec<String> = self.written.iter().filter_map(|p| p.file_name().map(|n| n.to_string_lossy().into_owned())).collect();
            return Ok((
                format!(
                    "wrote {} in {dir} ({} frames, {} x {}{})",
                    names.join(", "),
                    self.total,
                    self.width,
                    self.height,
                    if self.sound.is_some() { ", with its sound" } else { ", silent" }
                ),
                vec![],
            ));
        }
        let movie = self.encoder.take().ok_or("finished already")?.finish()?;
        let bytes = movie.to_ca_vp8().map_err(|e| e.to_string())?;
        let clip = self.project.clip(self.clip).ok_or("the clip is gone")?.clone();
        let owned;
        let lib = match library {
            Some(l) => l,
            None => {
                owned = SoundLibrary::load(files.as_ref());
                &owned
            }
        };
        let mut missing = vec![];
        let sound = mixdown(&self.project, &clip, lib, files.as_ref(), &mut missing);
        let silent = sound.samples.iter().all(|s| s.abs() < 1e-6);
        let schema = files
            .game()
            .and_then(wce_foundation::settings::rpfm_schema_path)
            .and_then(|s| Schema::load(&s, None).ok())
            .ok_or("no RPFM schema for the game")?;
        let subs = subtitle_frames(&self.project, &clip, &self.language);
        let mf = movie_files(&self.name, if silent { None } else { Some(&sound) }, &subs, &self.language, lib, files.as_ref(), &schema)?;
        let mut out = mf.files;
        out.push((format!("movies/{}.ca_vp8", self.name), bytes));
        wce_walls::pack::write_pack(&self.path, &self.project.game, out, vec![])?;
        let mut warnings = mf.warnings;
        warnings.extend(missing.into_iter().map(|m| format!("no sound event {m}: left out of the mix")));
        Ok((
            format!("wrote {} ({} frames, {} x {}, event {})", self.path.display(), self.total, self.width, self.height, mf.event),
            warnings,
        ))
    }
}

/// The fade (towards black) and the cinematic bars (an eighth of the height at full strength, top and bottom) over a picture.
/// Never the subtitles: the game draws a movie's lines itself from its `subtitle_timings` rows and loc (the viewport's subtitle is
/// the editor's overlay, which the offscreen render does not draw).
fn overlay(rgba: &mut [u8], w: u32, h: u32, f: &SceneFrame, fade: bool, bars: bool) {
    let keep = if fade { (1.0 - f.fade.clamp(0.0, 1.0)) as f64 } else { 1.0 };
    let bar = if bars { (f.letterbox.clamp(0.0, 1.0) as f64 * h as f64 / 8.0).round() as u32 } else { 0 };
    for y in 0..h {
        let black = y < bar || y >= h - bar;
        for x in 0..w {
            let o = ((y * w + x) * 4) as usize;
            for c in 0..3 {
                rgba[o + c] = if black { 0 } else { (rgba[o + c] as f64 * keep).round() as u8 };
            }
            rgba[o + 3] = 255;
        }
    }
}
