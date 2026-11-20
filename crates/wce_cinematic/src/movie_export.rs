//! A clip as a game-ready movie (docs/plans/cinematic.md, step 5): its pictures encoded as VP8 in the game's CAMV container, its
//! sound mixed down and played by a new `Play_Movie_<name>` event (an overriding `global_movies.bnk`, the sound streamed from
//! `audio/<id>.wem` as vanilla's movies are), the `videos` row, the subtitles' `subtitle_timings` rows (frames) and their
//! `vo_fmv_texts` loc entry. The pictures come from the caller (the editor's offline renderer); everything else is here.

use std::collections::BTreeMap;
use std::io::Cursor;

use rpfm_lib::files::db::DB;
use rpfm_lib::files::table::DecodedData;
use rpfm_lib::files::{Decodeable, DecodeableExtraData, Encodeable, EncodeableExtraData};
use rpfm_lib::schema::Schema;
use wce_audio::decode::{Pcm, decode_wem};
use wce_audio::edit::EditSet;
use wce_audio::encode::{DEFAULT_QUALITY, encode_wem};
use wce_audio::library::SoundLibrary;
use wce_vfs::files::Files;
use wce_video::{Encoder, EncoderSettings, Movie, yuv};

use crate::eval::{SpeedTable, evaluate};
use crate::movies::{audio_event, preferred_version};
use crate::scene::{Clip, Project, TrackContent, sanitize_key};

/// The movies' mixdown rate (vanilla's movie sounds are 48 kHz).
pub const MIX_RATE: u32 = 48_000;
/// The sound vanilla's `global_movies.bnk` plays for the SEGA sting: a stereo sound with no container, the template of a new
/// movie's sound (copied by `EditSet::new_sound_like`).
pub const TEMPLATE_SOUND: u32 = 32_672_080;
/// `subtitle_timings.subtitle_field` in every vanilla row.
pub const SUBTITLE_FIELD: &str = "1686093551";

/// Encodes RGBA pictures (top row first, `width * height * 4` bytes each) as a movie at `fps`.
pub struct MovieEncoder {
    encoder: Encoder,
    width: u32,
    height: u32,
    fps: u32,
    packets: Vec<wce_video::EncodedFrame>,
    count: usize,
}

impl MovieEncoder {
    pub fn new(settings: &EncoderSettings) -> Result<Self, String> {
        Ok(Self {
            encoder: Encoder::new(settings).map_err(|e| e.to_string())?,
            width: settings.width,
            height: settings.height,
            fps: settings.fps,
            packets: vec![],
            count: 0,
        })
    }

    pub fn push_rgba(&mut self, rgba: &[u8]) -> Result<(), String> {
        let picture = yuv::from_rgba(self.width, self.height, rgba);
        let key = self.count == 0;
        self.packets.extend(self.encoder.encode(&picture, key).map_err(|e| e.to_string())?);
        self.count += 1;
        Ok(())
    }

    pub fn frames(&self) -> usize {
        self.count
    }

    pub fn finish(mut self) -> Result<Movie, String> {
        self.packets.extend(self.encoder.finish().map_err(|e| e.to_string())?);
        Movie::from_frames(self.width, self.height, self.fps, &self.packets).map_err(|e| e.to_string())
    }
}

/// How many frames a clip has.
pub fn frame_count(clip: &Clip) -> usize {
    (clip.length * clip.fps.max(1) as f64).round() as usize
}

fn to_stereo_48k(pcm: &Pcm) -> Vec<[f32; 2]> {
    let ch = usize::from(pcm.channels.max(1));
    let frames = pcm.samples.len() / ch;
    let frame = |i: usize| -> [f32; 2] {
        let s = &pcm.samples[i * ch..i * ch + ch];
        match ch {
            1 => [s[0], s[0]],
            2 => [s[0], s[1]],
            // Wwise 5.1 (FL FR C LFE BL BR): centre and back folded in at -3 dB, LFE dropped.
            6 => {
                let k = std::f32::consts::FRAC_1_SQRT_2;
                [s[0] + k * s[2] + k * s[4], s[1] + k * s[2] + k * s[5]]
            }
            _ => [s[0], s[1.min(ch - 1)]],
        }
    };
    if pcm.sample_rate == MIX_RATE || pcm.sample_rate == 0 {
        return (0..frames).map(frame).collect();
    }
    // Linear resampling: plenty for a guide mix of the game's own sounds.
    let ratio = pcm.sample_rate as f64 / MIX_RATE as f64;
    let out = (frames as f64 / ratio).floor() as usize;
    (0..out)
        .map(|i| {
            let x = i as f64 * ratio;
            let (a, w) = (x.floor() as usize, (x - x.floor()) as f32);
            let (p, q) = (frame(a.min(frames - 1)), frame((a + 1).min(frames - 1)));
            [p[0] + (q[0] - p[0]) * w, p[1] + (q[1] - p[1]) * w]
        })
        .collect()
}

/// The decoded sound an event plays: its stereo version when it switches on the speakers, else its first media.
fn event_pcm(lib: &SoundLibrary, files: &dyn Files, event: &str) -> Option<Pcm> {
    let id = wce_audio_format::fnv::id(event);
    if !lib.has_object(id) {
        return None;
    }
    let mut versions = vec![];
    let tree = lib.resolve(id, files);
    fn walk(n: &wce_audio::library::PlayNode, v: &str, out: &mut Vec<(String, wce_audio::library::MediaRef)>) {
        let v = if n.kind == 0 && n.media.is_none() { n.label.as_str() } else { v };
        if let Some(m) = &n.media {
            out.push((v.to_owned(), m.clone()));
        }
        for c in &n.children {
            walk(c, v, out);
        }
    }
    walk(&tree, "", &mut versions);
    let pick = preferred_version(&versions)?;
    lib.media_bytes(&versions[pick].1, files).and_then(|b| decode_wem(&b).ok())
}

/// The clip's sound: every sound event at its time and volume, every music cue with its fades, mixed to 48 kHz stereo, as long as
/// the clip. A random container plays its first variation (the mix is the same every time). Missing events are reported.
pub fn mixdown(_project: &Project, clip: &Clip, lib: &SoundLibrary, files: &dyn Files, missing: &mut Vec<String>) -> Pcm {
    let total = (clip.length * MIX_RATE as f64).round() as usize;
    let mut mix = vec![[0f32; 2]; total];
    let mut cache: BTreeMap<String, Option<Vec<[f32; 2]>>> = BTreeMap::new();
    let mut get = |event: &str| -> Option<Vec<[f32; 2]>> {
        cache.entry(event.to_owned()).or_insert_with(|| event_pcm(lib, files, event).map(|p| to_stereo_48k(&p))).clone()
    };
    for track in clip.tracks.iter().filter(|t| !t.muted) {
        match &track.content {
            TrackContent::Sound { events } => {
                for e in events {
                    let Some(s) = get(&e.event) else {
                        missing.push(e.event.clone());
                        continue;
                    };
                    let start = (e.t * MIX_RATE as f64).round() as usize;
                    for (i, v) in s.iter().enumerate() {
                        let Some(m) = mix.get_mut(start + i) else { break };
                        m[0] += v[0] * e.volume;
                        m[1] += v[1] * e.volume;
                    }
                }
            }
            TrackContent::Music { cues } => {
                for c in cues {
                    let Some(s) = get(&c.event) else {
                        missing.push(c.event.clone());
                        continue;
                    };
                    let start = (c.t * MIX_RATE as f64).round() as usize;
                    let stop = c.stop.unwrap_or(clip.length);
                    for (i, v) in s.iter().enumerate() {
                        let t = c.t + i as f64 / MIX_RATE as f64;
                        if t >= stop {
                            break;
                        }
                        let fade_in = if c.fade_in > 0.0 { ((t - c.t) / c.fade_in as f64).min(1.0) } else { 1.0 };
                        let fade_out = if c.fade_out > 0.0 { ((stop - t) / c.fade_out as f64).min(1.0) } else { 1.0 };
                        let g = (fade_in * fade_out) as f32;
                        let Some(m) = mix.get_mut(start + i) else { break };
                        m[0] += v[0] * g;
                        m[1] += v[1] * g;
                    }
                }
            }
            _ => {}
        }
    }
    Pcm {
        channels: 2,
        sample_rate: MIX_RATE,
        samples: mix.into_iter().flat_map(|s| [s[0].clamp(-1.0, 1.0), s[1].clamp(-1.0, 1.0)]).collect(),
    }
}

/// The clip's subtitles as (start frame, end frame, text), in `language`.
pub fn subtitle_frames(project: &Project, clip: &Clip, language: &str) -> Vec<(u32, u32, String)> {
    let fps = clip.fps.max(1) as f64;
    let mut out = vec![];
    for track in clip.tracks.iter().filter(|t| !t.muted) {
        if let TrackContent::Subtitles { lines } = &track.content {
            for l in lines {
                let text = project.text(&l.text, language).unwrap_or("").replace('|', "/");
                out.push(((l.t * fps).round() as u32, (l.end * fps).round() as u32, text));
            }
        }
    }
    out.sort_by_key(|s| s.0);
    out
}

fn template(schema: &Schema, files: &dyn Files, table: &str) -> Result<DB, String> {
    for path in files.list(&format!("db/{table}/"), "") {
        let Ok(bytes) = files.read(&path) else { continue };
        let mut extra = DecodeableExtraData::default();
        extra.set_schema(Some(schema));
        extra.set_table_name(Some(table));
        if let Ok(db) = DB::decode(&mut Cursor::new(&bytes), &Some(extra)) {
            return Ok(db);
        }
    }
    Err(format!("the game has no readable {table}"))
}

fn encode_db(mut db: DB, rows: Vec<Vec<DecodedData>>) -> Result<Vec<u8>, String> {
    db.set_data(&rows).map_err(|e| e.to_string())?;
    let mut extra = EncodeableExtraData::default();
    extra.set_table_has_guid(!db.guid().is_empty());
    let mut out = vec![];
    db.encode(&mut out, &Some(extra)).map_err(|e| e.to_string())?;
    Ok(out)
}

fn set(db: &DB, row: &mut [DecodedData], column: &str, value: &str) -> Result<(), String> {
    let i = db.definition().fields_processed().iter().position(|f| f.name() == column).ok_or_else(|| format!("no column {column}"))?;
    row[i].set_data(value).map_err(|e| format!("{column} = {value}: {e}"))
}

/// What a movie adds to the packs, besides its `.ca_vp8`.
pub struct MovieFiles {
    /// Pack path -> bytes: the bank, the sound's WEM, the tables, the loc.
    pub files: Vec<(String, Vec<u8>)>,
    pub event: String,
    pub warnings: Vec<String>,
}

/// The sound (as an event), the `videos` and `subtitle_timings` rows and the loc of a movie called `name`.
///
/// # Errors
///
/// The game has no `global_movies.bnk` with the template sound, a table is missing, or the sound can't be encoded.
pub fn movie_files(
    name: &str,
    sound: Option<&Pcm>,
    subtitles: &[(u32, u32, String)],
    language: &str,
    lib: &SoundLibrary,
    files: &dyn Files,
    schema: &Schema,
) -> Result<MovieFiles, String> {
    let name = sanitize_key(name);
    let mut out = vec![];
    let mut warnings = vec![];
    let event = audio_event(&name);

    if let Some(pcm) = sound {
        let bank =
            lib.banks.iter().position(|b| b.name.eq_ignore_ascii_case("global_movies")).ok_or("the game has no global_movies.bnk")?;
        // IDs seeded by the movie's name: the same export every time.
        let mut edits = EditSet { id_seed: Some(wce_audio_format::fnv::id(&name)), ..EditSet::default() };
        let wem = encode_wem(pcm, DEFAULT_QUALITY).map_err(|e| e.to_string())?;
        let sound_id = edits.new_sound_like(lib, TEMPLATE_SOUND, &wem).map_err(|e| e.to_string())?;
        edits.new_event(lib, bank, &event, sound_id).map_err(|e| e.to_string())?;
        for (b, bank) in &edits.banks {
            out.push((lib.banks[*b].path.clone(), bank.write()));
        }
        out.extend(edits.loose.into_iter());
    }

    // videos: a full-screen movie; its script_ref ties the subtitles to it (after every number the game uses).
    let videos = template(schema, files, "videos_tables")?;
    let names: Vec<String> = videos.definition().fields_processed().iter().map(|f| f.name().to_owned()).collect();
    let used = crate::movies_table_max(schema, files, "videos_tables", "script_ref");
    let script_ref = used + 1;
    let mut row = videos.new_row();
    set(&videos, &mut row, "video_name", &name)?;
    set(&videos, &mut row, "video_type", "FULLSCREEN_INTRO")?;
    if names.iter().any(|n| n == "audio_tracks") {
        set(&videos, &mut row, "audio_tracks", if sound.is_some() { "1" } else { "0" })?;
    }
    set(&videos, &mut row, "script_ref", &script_ref.to_string())?;
    let file = format!("wce_{name}");
    out.push((format!("db/videos_tables/{file}"), encode_db(videos, vec![row])?));

    if !subtitles.is_empty() {
        out.extend(subtitle_files(&name, script_ref, language, subtitles, files, schema)?);
        if language != "en" {
            warnings.push(format!("subtitle timings written for {language}: the game shows them only in that language"));
        }
    }
    Ok(MovieFiles { files: out, event, warnings })
}

/// A movie's subtitles as the game reads them: `subtitle_timings` rows (frames, in `language`, tied to the `videos` row by its
/// `script_ref`) and the `vo_fmv_texts` loc entry, its sections joined by `|`.
///
/// # Errors
///
/// The game has no `subtitle_timings` table, or a value doesn't fit.
pub fn subtitle_files(
    name: &str,
    script_ref: i64,
    language: &str,
    subtitles: &[(u32, u32, String)],
    files: &dyn Files,
    schema: &Schema,
) -> Result<Vec<(String, Vec<u8>)>, String> {
    let file = format!("wce_{}", sanitize_key(name));
    let timings = template(schema, files, "subtitle_timings_tables")?;
    let mut rows = vec![];
    for (i, (start, end, _)) in subtitles.iter().enumerate() {
        let mut row = timings.new_row();
        set(&timings, &mut row, "language", language)?;
        set(&timings, &mut row, "subtitle_field", SUBTITLE_FIELD)?;
        set(&timings, &mut row, "text_section", &i.to_string())?;
        set(&timings, &mut row, "foreign_key", name)?;
        set(&timings, &mut row, "start", &start.to_string())?;
        set(&timings, &mut row, "end", &end.to_string())?;
        set(&timings, &mut row, "script_id", &script_ref.to_string())?;
        set(&timings, &mut row, "text_id", &format!("vo_fmv_texts_text_{name}"))?;
        rows.push(row);
    }
    let text = subtitles.iter().map(|s| s.2.replace('|', "/")).collect::<Vec<_>>().join("|");
    let loc = crate::battle_tables::loc_file(&BTreeMap::from([(format!("vo_fmv_texts_text_{name}"), text)]))?;
    // Named for what it holds: the movie catalog reads the `vo_fmv_texts` loc files.
    Ok(vec![(format!("db/subtitle_timings_tables/{file}"), encode_db(timings, rows)?), (format!("text/db/vo_fmv_texts_{file}.loc"), loc)])
}

/// Renders a clip through `render` (the scene at a time to RGBA pictures) and encodes it.
///
/// # Errors
///
/// When the encoder fails or a picture has the wrong size.
pub fn render_clip(
    project: &Project,
    clip: &Clip,
    speeds: &SpeedTable,
    settings: &EncoderSettings,
    mut render: impl FnMut(&crate::eval::SceneFrame) -> Result<Vec<u8>, String>,
    mut progress: impl FnMut(usize, usize),
) -> Result<Movie, String> {
    let mut enc = MovieEncoder::new(settings)?;
    let n = frame_count(clip);
    for i in 0..n {
        let frame = evaluate(project, clip, speeds, i as f64 / clip.fps.max(1) as f64);
        let rgba = render(&frame)?;
        if rgba.len() != (settings.width * settings.height * 4) as usize {
            return Err(format!("frame {i}: {} bytes, expected {}", rgba.len(), settings.width * settings.height * 4));
        }
        enc.push_rgba(&rgba)?;
        progress(i + 1, n);
    }
    enc.finish()
}
