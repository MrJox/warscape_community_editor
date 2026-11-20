//! Editing the game's movies (docs/plans/cinematic.md, step 9): a movie's subtitles written as the game reads them, its sound
//! replaced, and new movies made from outside pictures (a PNG sequence, or any video through an ffmpeg the user has installed,
//! run as a separate process: nothing of it is linked into the editor).

use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use rpfm_lib::schema::Schema;
use wce_audio::decode::Pcm;
use wce_audio::edit::EditSet;
use wce_audio::encode::{DEFAULT_QUALITY, encode_wem};
use wce_audio::import::read_sound;
use wce_audio::library::{MediaRef, SoundLibrary};
use wce_vfs::files::Files;
use wce_video::{EncoderSettings, Movie};

use crate::movie_export::{MovieEncoder, movie_files, subtitle_files};
use crate::movies::{Subtitle, movie_audio_versions};
use crate::scene::sanitize_key;

/// A movie's subtitles as pack files (`subtitle_timings` rows and the `vo_fmv_texts` loc), tied to its `videos` row's `script_ref`.
///
/// # Errors
///
/// The movie has no `videos` row (no `script_ref`), or the tables can't be written.
pub fn subtitles_pack_files(
    movie: &str,
    script_ref: Option<i64>,
    language: &str,
    lines: &[Subtitle],
    files: &dyn Files,
    schema: &Schema,
) -> Result<Vec<(String, Vec<u8>)>, String> {
    let script_ref = script_ref.ok_or_else(|| format!("{movie} has no videos row: its subtitles can't be tied to it"))?;
    let mut sorted: Vec<(u32, u32, String)> = lines.iter().map(|l| (l.start, l.end.max(l.start), l.text.clone())).collect();
    sorted.sort_by_key(|l| l.0);
    subtitle_files(movie, script_ref, language, &sorted, files, schema)
}

/// Replaces the sound of a movie's `Play_Movie_<name>` event: every version it switches between (stereo and surround) plays
/// `pcm` (encoded once; the surround setup plays it as stereo). Returns the changed banks and loose files.
///
/// # Errors
///
/// The movie has no sound event, or a media can't be replaced.
pub fn replace_movie_sound(lib: &SoundLibrary, files: &dyn Files, movie: &str, pcm: &Pcm) -> Result<Vec<(String, Vec<u8>)>, String> {
    let versions = movie_audio_versions(lib, files, movie);
    if versions.is_empty() {
        return Err(format!("{movie} has no sound event (Play_Movie_{movie})"));
    }
    let wem = encode_wem(pcm, DEFAULT_QUALITY).map_err(|e| e.to_string())?;
    let mut edits = EditSet { id_seed: Some(wce_audio_format::fnv::id(movie)), ..EditSet::default() };
    let mut done: Vec<u32> = vec![];
    for (_, media) in versions {
        if matches!(media, MediaRef::Missing { .. }) || done.contains(&media.id()) {
            continue;
        }
        edits.replace_audio(lib, files, media.id(), &wem).map_err(|e| e.to_string())?;
        done.push(media.id());
    }
    let mut out: Vec<(String, Vec<u8>)> = edits.banks.iter().map(|(b, bank)| (lib.banks[*b].path.clone(), bank.write())).collect();
    out.extend(edits.loose);
    Ok(out)
}

/// Where ffmpeg is: `WCE_FFMPEG` (the program or its folder), else `ffmpeg` on the PATH when it answers.
pub fn find_ffmpeg() -> Option<PathBuf> {
    if let Ok(v) = std::env::var("WCE_FFMPEG") {
        let p = PathBuf::from(v);
        for c in [p.clone(), p.join("ffmpeg.exe"), p.join("bin").join("ffmpeg.exe"), p.join("ffmpeg"), p.join("bin").join("ffmpeg")] {
            if c.is_file() {
                return Some(c);
            }
        }
    }
    let ok = Command::new("ffmpeg").arg("-version").stdout(Stdio::null()).stderr(Stdio::null()).status().is_ok_and(|s| s.success());
    ok.then(|| PathBuf::from("ffmpeg"))
}

/// The picture sizes the game's movies come in.
pub const FULL_SCREEN: (u32, u32) = (1920, 1080);
pub const EVENT_PICTURE: (u32, u32) = (380, 214);

/// A video file made into a game movie through ffmpeg: scaled to `size`, at `fps`, from `start` to `end` seconds when given. The
/// sound comes too (48 kHz stereo), when the file has one.
///
/// # Errors
///
/// ffmpeg can't be run or fails, or the encoder fails.
pub fn movie_from_video(
    ffmpeg: &Path,
    video: &Path,
    size: (u32, u32),
    fps: u32,
    range: (Option<f64>, Option<f64>),
    mut progress: impl FnMut(usize),
) -> Result<(Movie, Option<Pcm>), String> {
    let clip = |c: &mut Command| {
        if let Some(s) = range.0 {
            c.args(["-ss", &format!("{s:.3}")]);
        }
        if let Some(e) = range.1 {
            c.args(["-to", &format!("{e:.3}")]);
        }
    };
    let mut cmd = Command::new(ffmpeg);
    cmd.args(["-v", "error", "-nostdin"]);
    clip(&mut cmd);
    cmd.arg("-i").arg(video).args([
        "-vf",
        &format!("scale={}:{}:flags=lanczos,fps={fps}", size.0, size.1),
        "-f",
        "rawvideo",
        "-pix_fmt",
        "rgba",
        "-",
    ]);
    let mut child = cmd.stdout(Stdio::piped()).stderr(Stdio::piped()).spawn().map_err(|e| format!("{}: {e}", ffmpeg.display()))?;
    let mut stdout = child.stdout.take().ok_or("ffmpeg gave no output")?;
    let mut encoder = MovieEncoder::new(&EncoderSettings::movie(size.0, size.1, fps))?;
    let mut frame = vec![0u8; (size.0 * size.1 * 4) as usize];
    let mut n = 0;
    loop {
        match read_full(&mut stdout, &mut frame) {
            Ok(true) => {
                encoder.push_rgba(&frame)?;
                n += 1;
                progress(n);
            }
            Ok(false) => break,
            Err(e) => return Err(format!("reading ffmpeg's pictures: {e}")),
        }
    }
    let status = child.wait().map_err(|e| e.to_string())?;
    if !status.success() || n == 0 {
        let mut err = String::new();
        if let Some(mut s) = child.stderr.take() {
            let _ = s.read_to_string(&mut err);
        }
        return Err(format!("ffmpeg failed on {}: {}", video.display(), err.trim()));
    }
    let movie = encoder.finish()?;

    // The sound: the file's, as 48 kHz stereo WAV (none when it has no audio stream).
    let mut cmd = Command::new(ffmpeg);
    cmd.args(["-v", "error", "-nostdin"]);
    clip(&mut cmd);
    cmd.arg("-i").arg(video).args(["-vn", "-ac", "2", "-ar", "48000", "-f", "wav", "-"]);
    let out = cmd.stdout(Stdio::piped()).stderr(Stdio::null()).output().map_err(|e| e.to_string())?;
    let sound = if out.status.success() && out.stdout.len() > 44 { read_sound(&patch_wav_sizes(out.stdout)).ok() } else { None };
    Ok((movie, sound.filter(|p| !p.samples.is_empty())))
}

/// ffmpeg writing WAV to a pipe can't go back to fill in the sizes: set them from the data.
fn patch_wav_sizes(mut wav: Vec<u8>) -> Vec<u8> {
    if wav.len() >= 12 && &wav[0..4] == b"RIFF" {
        let riff = (wav.len() - 8) as u32;
        wav[4..8].copy_from_slice(&riff.to_le_bytes());
        // The `data` chunk: everything after its header.
        let mut i = 12;
        while i + 8 <= wav.len() {
            let id = &wav[i..i + 4];
            let size = u32::from_le_bytes([wav[i + 4], wav[i + 5], wav[i + 6], wav[i + 7]]) as usize;
            if id == b"data" {
                let real = (wav.len() - i - 8) as u32;
                wav[i + 4..i + 8].copy_from_slice(&real.to_le_bytes());
                break;
            }
            i += 8 + size + (size & 1);
        }
    }
    wav
}

fn read_full(r: &mut impl Read, buf: &mut [u8]) -> std::io::Result<bool> {
    let mut got = 0;
    while got < buf.len() {
        match r.read(&mut buf[got..])? {
            0 if got == 0 => return Ok(false),
            0 => return Err(std::io::Error::new(std::io::ErrorKind::UnexpectedEof, "a picture cut short")),
            k => got += k,
        }
    }
    Ok(true)
}

/// A PNG sequence (a folder of `.png` files, by name) made into a game movie at `fps`; every picture must have the first one's
/// size, 8-bit RGB or RGBA.
///
/// # Errors
///
/// No pictures, one that doesn't read or has another size, or the encoder fails.
pub fn movie_from_pngs(dir: &Path, fps: u32, mut progress: impl FnMut(usize, usize)) -> Result<Movie, String> {
    let mut paths: Vec<PathBuf> = std::fs::read_dir(dir)
        .map_err(|e| format!("{}: {e}", dir.display()))?
        .flatten()
        .map(|d| d.path())
        .filter(|p| p.extension().is_some_and(|x| x.eq_ignore_ascii_case("png")))
        .collect();
    paths.sort();
    if paths.is_empty() {
        return Err(format!("{} has no PNG pictures", dir.display()));
    }
    let mut encoder: Option<(MovieEncoder, u32, u32)> = None;
    for (i, p) in paths.iter().enumerate() {
        let bytes = std::fs::read(p).map_err(|e| format!("{}: {e}", p.display()))?;
        let png = wce_formats::png_file::PngFile::parse(&bytes).map_err(|e| format!("{}: {e:?}", p.display()))?;
        let rgba: Vec<u8> = match (png.bit_depth, png.color_type) {
            (8, 6) => png.pixels,
            (8, 2) => png.pixels.chunks_exact(3).flat_map(|c| [c[0], c[1], c[2], 255]).collect(),
            (d, t) => return Err(format!("{}: {d}-bit, colour type {t} (8-bit RGB or RGBA wanted)", p.display())),
        };
        let (enc, w, h) = match &mut encoder {
            Some(e) => e,
            None => encoder.insert((MovieEncoder::new(&EncoderSettings::movie(png.width, png.height, fps))?, png.width, png.height)),
        };
        if (png.width, png.height) != (*w, *h) {
            return Err(format!("{}: {} x {}, the first picture is {w} x {h}", p.display(), png.width, png.height));
        }
        enc.push_rgba(&rgba)?;
        progress(i + 1, paths.len());
    }
    encoder.ok_or("no pictures")?.0.finish()
}

/// A new movie's pack files: the movie, its sound as `Play_Movie_<name>` (when it has one), its `videos` row.
///
/// # Errors
///
/// As [`movie_files`].
pub fn new_movie_pack_files(
    name: &str,
    movie: &Movie,
    sound: Option<&Pcm>,
    lib: &SoundLibrary,
    files: &dyn Files,
    schema: &Schema,
) -> Result<Vec<(String, Vec<u8>)>, String> {
    let name = sanitize_key(name);
    let mf = movie_files(&name, sound, &[], "en", lib, files, schema)?;
    let mut out = mf.files;
    out.push((format!("movies/{name}.ca_vp8"), movie.to_ca_vp8().map_err(|e| e.to_string())?));
    Ok(out)
}
