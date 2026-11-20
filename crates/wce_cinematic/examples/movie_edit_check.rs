//! Gate of movie editing (docs/plans/cinematic.md, step 9):
//! - a vanilla movie's subtitles edited and written: mounted over the game, the catalog reads the new text and timings;
//! - the SEGA sting's sound replaced by a tone: its event then resolves to a sound of the tone's length;
//! - a PNG sequence made into a movie, every picture back;
//! - with ffmpeg (`WCE_FFMPEG` or the PATH), a test video it makes (pictures and a tone) imported: the frame count and the sound's
//!   length; skipped, and said so, without ffmpeg.
//!
//!   movie_edit_check <game folder> <out folder>

use std::collections::BTreeMap;
use std::path::PathBuf;

use rpfm_lib::schema::Schema;
use wce_audio::decode::{Pcm, decode_wem};
use wce_audio::library::SoundLibrary;
use wce_cinematic::movie_edit::*;
use wce_cinematic::movies::{MovieCatalog, movie_audio_versions};
use wce_vfs::files::Files;
use wce_vfs::{Config, GameProfile, Vfs};
use wce_video::{Decoder, Movie};

struct Overlay<'a> {
    base: &'a Vfs,
    extra: BTreeMap<String, Vec<u8>>,
}

impl Files for Overlay<'_> {
    fn read(&self, path: &str) -> Result<Vec<u8>, String> {
        match self.extra.get(path) {
            Some(b) => Ok(b.clone()),
            None => Files::read(self.base, path),
        }
    }
    fn exists(&self, path: &str) -> bool {
        self.extra.contains_key(path) || Files::exists(self.base, path)
    }
    fn list(&self, prefix: &str, extension: &str) -> Vec<String> {
        let mut v = Files::list(self.base, prefix, extension);
        v.extend(self.extra.keys().filter(|k| k.starts_with(prefix) && k.ends_with(extension)).cloned());
        v.sort();
        v.dedup();
        v
    }
    fn game(&self) -> Option<&GameProfile> {
        Files::game(self.base)
    }
}

fn tone(seconds: f64) -> Pcm {
    let n = (seconds * 48_000.0) as usize;
    let samples = (0..n).flat_map(|i| {
        let v = (i as f32 * 440.0 * std::f32::consts::TAU / 48_000.0).sin() * 0.3;
        [v, v]
    });
    Pcm { channels: 2, sample_rate: 48_000, samples: samples.collect() }
}

fn decoded_pictures(m: &Movie) -> usize {
    let mut d = Decoder::new(1).unwrap();
    (0..m.frame_count()).filter(|&i| d.decode(m.frame(i).unwrap()).ok().flatten().is_some()).count()
}

fn main() {
    let mut args = std::env::args().skip(1);
    let (Some(game), Some(out)) = (args.next(), args.next()) else {
        eprintln!("movie_edit_check <game folder> <out folder>");
        std::process::exit(2);
    };
    let out = PathBuf::from(out);
    std::fs::create_dir_all(&out).unwrap();
    let vfs = Vfs::mount(&Config::new(PathBuf::from(game), GameProfile::from_env().clone())).expect("the game's packs");
    let schema =
        Files::game(&vfs).and_then(wce_foundation::settings::rpfm_schema_path).and_then(|p| Schema::load(&p, None).ok()).expect("schema");
    let lib = SoundLibrary::load(&vfs);
    let catalog = MovieCatalog::load(&vfs);
    let mut failed: Vec<String> = vec![];

    // Subtitles: att_seal_3's English lines, the first one rewritten and moved a second later.
    let entry = catalog.movies.iter().find(|m| m.name == "att_seal_3").cloned().expect("att_seal_3");
    let mut lines = catalog.subtitles(&entry.name, "en");
    lines[0].text = "The Cinematic tool was here.".into();
    lines[0].start += 30;
    match subtitles_pack_files(&entry.name, entry.script_ref, "en", &lines, &vfs, &schema) {
        Ok(files) => {
            let overlay = Overlay { base: &vfs, extra: files.into_iter().collect() };
            let back = MovieCatalog::load(&overlay).subtitles(&entry.name, "en");
            println!("subtitles: {} lines back, first {:?} from frame {}", back.len(), back[0].text, back[0].start);
            if back.len() != lines.len() || back[0].text != lines[0].text || back[0].start != lines[0].start {
                failed.push("the edited subtitles don't read back".into());
            }
        }
        Err(e) => failed.push(format!("subtitles: {e}")),
    }

    // Sound: the SEGA sting plays a 3 s tone.
    match replace_movie_sound(&lib, &vfs, "sega_logo_sting_hd", &tone(3.0)) {
        Ok(files) => {
            println!("sound: {} files ({})", files.len(), files.iter().map(|f| f.0.as_str()).collect::<Vec<_>>().join(", "));
            let overlay = Overlay { base: &vfs, extra: files.into_iter().collect() };
            let lib2 = SoundLibrary::load(&overlay);
            let got = movie_audio_versions(&lib2, &overlay, "sega_logo_sting_hd")
                .first()
                .and_then(|(_, m)| lib2.media_bytes(m, &overlay))
                .and_then(|b| decode_wem(&b).ok());
            match got {
                Some(p) if (p.seconds() - 3.0).abs() < 0.05 => println!("sound: the sting now lasts {:.2} s", p.seconds()),
                Some(p) => failed.push(format!("the sting lasts {:.2} s, not 3", p.seconds())),
                None => failed.push("the sting's sound does not resolve".into()),
            }
        }
        Err(e) => failed.push(format!("sound: {e}")),
    }

    // A PNG sequence: 12 pictures of 64 x 48 with a moving bar.
    let pngs = out.join("pngs");
    let _ = std::fs::remove_dir_all(&pngs);
    std::fs::create_dir_all(&pngs).unwrap();
    for f in 0..12u32 {
        let pixels: Vec<u8> =
            (0..48u32).flat_map(|y| (0..64u32).flat_map(move |x| [((x + f * 5) % 64 * 4) as u8, (y * 5) as u8, 90])).collect();
        let png = wce_formats::png_file::PngFile { width: 64, height: 48, bit_depth: 8, color_type: 2, pixels, ..Default::default() };
        std::fs::write(pngs.join(format!("frame_{f:04}.png")), png.encode().unwrap()).unwrap();
    }
    match movie_from_pngs(&pngs, 30, |_, _| {}) {
        Ok(m) => {
            let shown = decoded_pictures(&m);
            println!("pngs: {} frames, {} pictures, {} x {}", m.frame_count(), shown, m.width(), m.height());
            if m.frame_count() != 12 || shown != 12 {
                failed.push("the PNG sequence did not give 12 pictures".into());
            }
        }
        Err(e) => failed.push(format!("pngs: {e}")),
    }

    // A video through ffmpeg: 2 s of its test pattern with a tone, imported as an event picture.
    match find_ffmpeg() {
        None => println!("ffmpeg: not found (WCE_FFMPEG or the PATH): the import from video is not checked"),
        Some(ff) => {
            let video = out.join("test.mp4");
            let made = std::process::Command::new(&ff)
                .args(["-v", "error", "-y", "-f", "lavfi", "-i", "testsrc2=size=640x360:rate=25:duration=2", "-f", "lavfi", "-i"])
                .args(["sine=frequency=330:duration=2", "-shortest", "-c:v", "libx264", "-pix_fmt", "yuv420p", "-c:a", "aac"])
                .arg(&video)
                .status();
            if !made.is_ok_and(|s| s.success()) {
                failed.push("ffmpeg could not make the test video".into());
            } else {
                match movie_from_video(&ff, &video, EVENT_PICTURE, 30, (None, None), |_| {}) {
                    Ok((m, sound)) => {
                        let shown = decoded_pictures(&m);
                        let secs = sound.as_ref().map_or(0.0, |p| p.seconds());
                        println!(
                            "ffmpeg: {} frames ({shown} pictures) at {} x {}, sound {secs:.2} s",
                            m.frame_count(),
                            m.width(),
                            m.height()
                        );
                        if !(59..=61).contains(&m.frame_count()) || shown != m.frame_count() || (m.width(), m.height()) != EVENT_PICTURE {
                            failed.push("the imported video's frames are off".into());
                        }
                        if (secs - 2.0).abs() > 0.1 {
                            failed.push(format!("the imported sound lasts {secs:.2} s"));
                        }
                        match new_movie_pack_files("wce_imported", &m, sound.as_ref(), &lib, &vfs, &schema) {
                            Ok(files) => {
                                let _ = wce_walls::pack::write_pack(&out.join("wce_imported.pack"), "attila", files, vec![]);
                            }
                            Err(e) => failed.push(format!("the imported movie's files: {e}")),
                        }
                    }
                    Err(e) => failed.push(format!("ffmpeg import: {e}")),
                }
            }
        }
    }

    if failed.is_empty() {
        println!("OK");
    } else {
        for f in &failed {
            println!("FAIL {f}");
        }
        std::process::exit(1);
    }
}
