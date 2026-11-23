//! A clip exported as files for editing (docs/plans/cinematic.md, user round 17): a PNG sequence with a WAV of its sound (built in),
//! or MP4 (H.264) / ProRes MOV made by the user's own ffmpeg (`movie_edit::find_ffmpeg`), the frames piped to it as they render.
//! Whole, or one file per shot of the Camera Cuts with handle frames (a shot's handles keep its camera, so the editor can trim).

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, Stdio};

use wce_audio::decode::Pcm;

/// What a movie export writes.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Format {
    /// The game's VP8 movie in a mod pack, with its sound event and subtitles' rows.
    #[default]
    GamePack,
    /// A PNG a frame in a folder, and a WAV of the sound.
    Png,
    /// H.264 in MP4 (through ffmpeg): for review and sharing.
    Mp4,
    /// Apple ProRes 422 HQ in MOV (through ffmpeg): for editing.
    ProRes,
}

impl Format {
    pub const ALL: [Format; 4] = [Format::GamePack, Format::Png, Format::Mp4, Format::ProRes];

    pub fn label(self) -> &'static str {
        match self {
            Format::GamePack => "Game movie (.pack)",
            Format::Png => "PNG sequence + WAV",
            Format::Mp4 => "MP4 (H.264)",
            Format::ProRes => "ProRes 422 HQ (.mov)",
        }
    }

    pub fn tip(self) -> &'static str {
        match self {
            Format::GamePack => "The game's movie format in a mod pack, with its sound event and subtitles (the game plays it)",
            Format::Png => "A PNG a frame and a WAV of the sound: any video editor takes them (nothing to install)",
            Format::Mp4 => "A compact video to watch or share; made by ffmpeg (installed by you)",
            Format::ProRes => "A video for editing (Premiere, Resolve) without loss; made by ffmpeg (installed by you)",
        }
    }

    /// The file's extension (the PNG sequence: its first frame's).
    pub fn extension(self) -> &'static str {
        match self {
            Format::GamePack => "pack",
            Format::Png => "png",
            Format::Mp4 => "mp4",
            Format::ProRes => "mov",
        }
    }

    pub fn needs_ffmpeg(self) -> bool {
        matches!(self, Format::Mp4 | Format::ProRes)
    }

    pub fn key(self) -> &'static str {
        match self {
            Format::GamePack => "pack",
            Format::Png => "png",
            Format::Mp4 => "mp4",
            Format::ProRes => "prores",
        }
    }

    pub fn from_key(k: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|f| f.key() == k)
    }
}

/// A run of frames written as one file: clip frames `from` to `to` (end exclusive), seen through `camera` (a shot's own, for its
/// handles; None: the live one), named `stem`.
#[derive(Clone, Debug, PartialEq)]
pub struct Shot {
    pub from: usize,
    pub to: usize,
    pub camera: Option<u32>,
    pub stem: String,
}

/// The runs a clip is written as: whole (`shots` None), or one per shot (`(from s, to s, camera, camera's name)`) with `handles`
/// frames more each side within the clip, named `<base>_010_<camera>`, `_020_`...
pub fn runs(base: &str, frames: usize, fps: f64, shots: Option<&[(f64, f64, u32, String)]>, handles: usize) -> Vec<Shot> {
    let Some(shots) = shots.filter(|s| s.len() > 1) else {
        return vec![Shot { from: 0, to: frames, camera: None, stem: base.to_owned() }];
    };
    shots
        .iter()
        .enumerate()
        .map(|(k, (a, b, cam, name))| {
            let (from, to) = (((a * fps).round() as usize).min(frames), ((b * fps).round() as usize).min(frames));
            Shot {
                from: from.saturating_sub(handles),
                to: (to + handles).min(frames),
                camera: Some(*cam),
                stem: format!("{base}_{:03}_{}", (k + 1) * 10, wce_cinematic::scene::sanitize_key(name)),
            }
        })
        .collect()
}

/// A 16-bit PCM WAV of a stretch of the sound (seconds).
pub fn wav_bytes(pcm: &Pcm, from_s: f64, to_s: f64) -> Vec<u8> {
    let ch = usize::from(pcm.channels.max(1));
    let rate = pcm.sample_rate.max(1);
    let frames = pcm.samples.len() / ch;
    let a = ((from_s.max(0.0) * f64::from(rate)).round() as usize).min(frames);
    let b = ((to_s.max(0.0) * f64::from(rate)).round() as usize).clamp(a, frames);
    // The stretch past the sound's end is silence (the clip may be longer than its last sound).
    let want = ((to_s - from_s).max(0.0) * f64::from(rate)).round() as usize;
    let data: Vec<u8> = pcm.samples[a * ch..b * ch]
        .iter()
        .map(|s| (s.clamp(-1.0, 1.0) * 32767.0).round() as i16)
        .chain(std::iter::repeat_n(0i16, want.saturating_sub(b - a) * ch))
        .flat_map(i16::to_le_bytes)
        .collect();
    let mut out = Vec::with_capacity(44 + data.len());
    out.extend(b"RIFF");
    out.extend((36 + data.len() as u32).to_le_bytes());
    out.extend(b"WAVEfmt ");
    out.extend(16u32.to_le_bytes());
    out.extend(1u16.to_le_bytes());
    out.extend((ch as u16).to_le_bytes());
    out.extend(rate.to_le_bytes());
    out.extend((rate * ch as u32 * 2).to_le_bytes());
    out.extend((ch as u16 * 2).to_le_bytes());
    out.extend(16u16.to_le_bytes());
    out.extend(b"data");
    out.extend((data.len() as u32).to_le_bytes());
    out.extend(data);
    out
}

/// ffmpeg's arguments: raw RGBA frames on its input, the WAV beside it (when there is sound), the format's codec.
pub fn ffmpeg_args(format: Format, w: u32, h: u32, fps: u32, wav: Option<&Path>, out: &Path) -> Vec<String> {
    let mut a: Vec<String> = ["-y", "-hide_banner", "-loglevel", "error", "-f", "rawvideo", "-pix_fmt", "rgba"].map(str::to_owned).to_vec();
    a.extend(["-s".into(), format!("{w}x{h}"), "-framerate".into(), fps.to_string(), "-i".into(), "-".into()]);
    if let Some(wav) = wav {
        a.extend(["-i".into(), wav.display().to_string(), "-map".into(), "0:v".into(), "-map".into(), "1:a".into()]);
    }
    let codec: &[&str] = match format {
        Format::ProRes => &["-c:v", "prores_ks", "-profile:v", "3", "-pix_fmt", "yuv422p10le"],
        _ => &["-c:v", "libx264", "-preset", "medium", "-crf", "17", "-pix_fmt", "yuv420p", "-movflags", "+faststart"],
    };
    a.extend(codec.iter().map(|s| (*s).to_owned()));
    if wav.is_some() {
        let audio: &[&str] = if format == Format::ProRes { &["-c:a", "pcm_s16le"] } else { &["-c:a", "aac", "-b:a", "256k"] };
        a.extend(audio.iter().map(|s| (*s).to_owned()));
    }
    a.push(out.display().to_string());
    a
}

/// Where a run's frames go.
pub enum Sink {
    Png { dir: PathBuf, stem: String, count: usize },
    Ffmpeg { child: Child, stdin: Option<ChildStdin>, out: PathBuf, wav: Option<PathBuf>, log: PathBuf },
}

impl Sink {
    /// A run's sink in `dir`: its PNG folder, or ffmpeg started on its file (the run's sound written first, beside it).
    pub fn open(
        format: Format,
        dir: &Path,
        run: &Shot,
        size: (u32, u32),
        fps: u32,
        sound: Option<(&Pcm, f64, f64)>,
    ) -> Result<Self, String> {
        match format {
            Format::Png => {
                let folder = dir.join(&run.stem);
                std::fs::create_dir_all(&folder).map_err(|e| format!("{}: {e}", folder.display()))?;
                if let Some((pcm, a, b)) = sound {
                    let wav = dir.join(format!("{}.wav", run.stem));
                    std::fs::write(&wav, wav_bytes(pcm, a, b)).map_err(|e| format!("{}: {e}", wav.display()))?;
                }
                Ok(Sink::Png { dir: folder, stem: run.stem.clone(), count: 0 })
            }
            Format::Mp4 | Format::ProRes => {
                let ffmpeg = wce_cinematic::movie_edit::find_ffmpeg().ok_or("ffmpeg isn't installed (or WCE_FFMPEG doesn't name it)")?;
                let out = dir.join(format!("{}.{}", run.stem, format.extension()));
                let wav = match sound {
                    Some((pcm, a, b)) => {
                        let p = dir.join(format!("{}.wce_sound.wav", run.stem));
                        std::fs::write(&p, wav_bytes(pcm, a, b)).map_err(|e| format!("{}: {e}", p.display()))?;
                        Some(p)
                    }
                    None => None,
                };
                let log = dir.join(format!("{}.wce_ffmpeg.log", run.stem));
                let err = std::fs::File::create(&log).map_err(|e| format!("{}: {e}", log.display()))?;
                let mut child = Command::new(ffmpeg)
                    .args(ffmpeg_args(format, size.0, size.1, fps, wav.as_deref(), &out))
                    .stdin(Stdio::piped())
                    .stdout(Stdio::null())
                    .stderr(err)
                    .spawn()
                    .map_err(|e| format!("ffmpeg: {e}"))?;
                let stdin = child.stdin.take();
                Ok(Sink::Ffmpeg { child, stdin, out, wav, log })
            }
            Format::GamePack => Err("the game's movie has no file sink".into()),
        }
    }

    pub fn push(&mut self, rgba: &[u8], size: (u32, u32)) -> Result<(), String> {
        match self {
            Sink::Png { dir, stem, count } => {
                let png = wce_formats::png_file::PngFile {
                    width: size.0,
                    height: size.1,
                    bit_depth: 8,
                    color_type: 6,
                    pixels: rgba.to_vec(),
                    ..Default::default()
                };
                let path = dir.join(format!("{stem}_{:05}.png", *count));
                std::fs::write(&path, png.encode().map_err(|e| format!("{e:?}"))?).map_err(|e| format!("{}: {e}", path.display()))?;
                *count += 1;
                Ok(())
            }
            Sink::Ffmpeg { stdin, .. } => {
                stdin.as_mut().ok_or("ffmpeg's input is closed")?.write_all(rgba).map_err(|e| format!("ffmpeg stopped taking frames: {e}"))
            }
        }
    }

    /// Ends the run: ffmpeg finishes its file (its sound's WAV and its log removed when it succeeds). Returns what was written.
    pub fn finish(self) -> Result<PathBuf, String> {
        match self {
            Sink::Png { dir, .. } => Ok(dir),
            Sink::Ffmpeg { mut child, stdin, out, wav, log } => {
                drop(stdin);
                let status = child.wait().map_err(|e| format!("ffmpeg: {e}"))?;
                if let Some(w) = &wav {
                    let _ = std::fs::remove_file(w);
                }
                if !status.success() {
                    let said = std::fs::read_to_string(&log).unwrap_or_default();
                    return Err(format!("ffmpeg failed ({status}): {}", said.lines().last().unwrap_or("see its log")));
                }
                let _ = std::fs::remove_file(&log);
                Ok(out)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn runs_cover_the_clip_or_its_shots() {
        assert_eq!(runs("m", 300, 30.0, None, 5), [Shot { from: 0, to: 300, camera: None, stem: "m".into() }]);
        let shots = vec![(0.0, 4.0, 7, "Wide".to_owned()), (4.0, 10.0, 9, "Close up".to_owned())];
        let r = runs("m", 300, 30.0, Some(&shots), 6);
        assert_eq!(r[0], Shot { from: 0, to: 126, camera: Some(7), stem: "m_010_wide".into() });
        assert_eq!(r[1], Shot { from: 114, to: 300, camera: Some(9), stem: "m_020_close_up".into() });
        // One shot: the clip whole.
        assert_eq!(runs("m", 300, 30.0, Some(&shots[..1]), 6).len(), 1);
    }

    #[test]
    fn a_wav_of_a_stretch() {
        let pcm = Pcm { channels: 2, sample_rate: 48_000, samples: vec![0.5; 48_000 * 2] };
        let w = wav_bytes(&pcm, 0.5, 1.5);
        assert_eq!(&w[..4], b"RIFF");
        assert_eq!(u32::from_le_bytes(w[40..44].try_into().unwrap()), 48_000 * 2 * 2, "a second: half sound, half silence");
        assert_eq!(i16::from_le_bytes([w[44], w[45]]), 16384);
        assert_eq!(i16::from_le_bytes([w[w.len() - 2], w[w.len() - 1]]), 0);
    }

    #[test]
    fn ffmpeg_gets_the_frames_and_the_codec() {
        let a = ffmpeg_args(Format::ProRes, 1920, 1080, 30, Some(Path::new("s.wav")), Path::new("o.mov"));
        let s = a.join(" ");
        assert!(s.contains("-f rawvideo -pix_fmt rgba -s 1920x1080 -framerate 30 -i -"), "{s}");
        assert!(s.contains("-c:v prores_ks -profile:v 3") && s.contains("-c:a pcm_s16le") && s.ends_with("o.mov"), "{s}");
        let s = ffmpeg_args(Format::Mp4, 640, 360, 24, None, Path::new("o.mp4")).join(" ");
        assert!(s.contains("libx264") && !s.contains("-c:a") && !s.contains("-map"), "{s}");
    }
}
