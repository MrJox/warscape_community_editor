//! Gate of the movie export (docs/plans/cinematic.md, step 5), without the 3D renderer: a 3 s clip with a sound event (the game's
//! SEGA sting) and two subtitles, its pictures drawn from the evaluated scene (a stand-in for the renderer). Checks: the CAMV movie
//! reads back with every frame and decodes; the overriding `global_movies.bnk` and the sound's WEM, mounted over the game's files,
//! make `Play_Movie_<name>` resolve to the new sound, which decodes to the clip's length; the `videos` and `subtitle_timings` rows
//! and the loc decode back; the export is the same twice. Writes a pack to `<out>` for a test in game.
//!
//!   movie_export_check <game folder> <out folder>

use std::collections::BTreeMap;
use std::io::Cursor;
use std::path::PathBuf;

use rpfm_lib::files::db::DB;
use rpfm_lib::files::{Decodeable, DecodeableExtraData};
use rpfm_lib::schema::Schema;
use wce_audio::decode::decode_wem;
use wce_audio::library::{MediaRef, SoundLibrary};
use wce_cinematic::eval::{SceneFrame, SpeedTable};
use wce_cinematic::movie_export::{frame_count, mixdown, movie_files, render_clip, subtitle_frames};
use wce_cinematic::movies::movie_audio_versions;
use wce_cinematic::scene::*;
use wce_vfs::files::Files;
use wce_vfs::{Config, GameProfile, Vfs};
use wce_video::{Decoder, EncoderSettings, Movie};

/// The game's files with some replaced or added (what a mod pack would do).
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

/// A stand-in picture: the sky's colour from the camera's height, a bar where the camera looks, white while a subtitle shows.
fn draw(frame: &SceneFrame, w: u32, h: u32) -> Vec<u8> {
    let mut rgba = vec![0u8; (w * h * 4) as usize];
    let cam = frame.camera.clone().map_or([0.0, 0.0, 0.0], |c| c.position);
    let x0 = ((cam[0] + 100.0) / 200.0 * w as f32).clamp(0.0, w as f32 - 1.0) as u32;
    for y in 0..h {
        for x in 0..w {
            let o = ((y * w + x) * 4) as usize;
            rgba[o] = (cam[1] * 2.0).clamp(0.0, 255.0) as u8;
            rgba[o + 1] = (y * 255 / h) as u8;
            rgba[o + 2] = if x.abs_diff(x0) < 8 { 240 } else { 60 };
            if frame.subtitle.is_some() && y > h - 20 {
                rgba[o..o + 3].copy_from_slice(&[255, 255, 255]);
            }
            rgba[o + 3] = 255;
        }
    }
    rgba
}

fn main() {
    let mut args = std::env::args().skip(1);
    let (Some(game), Some(out)) = (args.next(), args.next()) else {
        eprintln!("movie_export_check <game folder> <out folder>");
        std::process::exit(2);
    };
    let out = PathBuf::from(out);
    let config = Config::new(PathBuf::from(game), GameProfile::from_env().clone());
    let vfs = Vfs::mount(&config).expect("the game's packs");
    let schema =
        Files::game(&vfs).and_then(wce_foundation::settings::rpfm_schema_path).and_then(|p| Schema::load(&p, None).ok()).expect("schema");
    let lib = SoundLibrary::load(&vfs);
    let mut failed: Vec<String> = vec![];

    let mut p = Project::new("WCE Test Movie", "attila");
    p.texts.insert("line_1".into(), BTreeMap::from([("en".into(), "The Cinematic tool's first movie.".into())]));
    p.texts.insert("line_2".into(), BTreeMap::from([("en".into(), "Rendered, encoded, subtitled.".into())]));
    let ids: Vec<u32> = (0..2).map(|_| p.new_id()).collect();
    let clip_id = p.clips[0].id;
    let c = p.clip_mut(clip_id).unwrap();
    c.length = 3.0;
    if let TrackContent::Camera { keys } = &mut c.tracks[0].content {
        keys.push(CameraKey { t: 0.0, position: [-80.0, 20.0, 0.0], target: [0.0, 0.0, 50.0], fov: 35.0, interp: Interp::Smooth });
        keys.push(CameraKey { t: 3.0, position: [80.0, 100.0, 0.0], target: [0.0, 0.0, 50.0], fov: 45.0, interp: Interp::Smooth });
    }
    c.tracks.push(Track {
        id: ids[0],
        name: "Sting".into(),
        muted: false,
        content: TrackContent::Sound {
            events: vec![SoundEvent { t: 0.25, event: "Play_Movie_SEGA_logo_sting_HD".into(), position: None, volume: 1.0 }],
        },
    });
    c.tracks.push(Track {
        id: ids[1],
        name: "Subtitles".into(),
        muted: false,
        content: TrackContent::Subtitles {
            lines: vec![SubtitleLine { t: 0.2, end: 1.4, text: "line_1".into() }, SubtitleLine { t: 1.6, end: 2.8, text: "line_2".into() }],
        },
    });
    let clip = p.clip(clip_id).unwrap().clone();
    let (w, h) = (320u32, 180u32);
    let mut settings = EncoderSettings::movie(w, h, 30);
    settings.target_kbps = 1500;

    let render = |p: &Project| render_clip(p, &clip, &SpeedTable::new(), &settings, |f| Ok(draw(f, w, h)), |_, _| {});
    let movie = render(&p).expect("render");
    let bytes = movie.to_ca_vp8().expect("ca_vp8");
    if render(&p).expect("render").to_ca_vp8().expect("ca_vp8") != bytes {
        failed.push("two renders give different movies".into());
    }
    let back = Movie::read(&bytes).expect("read back");
    println!("movie: {} x {}, {} frames, {:.3} fps, {} bytes", back.width(), back.height(), back.frame_count(), back.fps(), bytes.len());
    if back.frame_count() != frame_count(&clip) {
        failed.push(format!("{} frames, the clip has {}", back.frame_count(), frame_count(&clip)));
    }
    let mut dec = Decoder::new(1).unwrap();
    let shown = (0..back.frame_count()).filter(|&i| dec.decode(back.frame(i).unwrap()).ok().flatten().is_some()).count();
    if shown != back.frame_count() {
        failed.push(format!("{shown} pictures decoded of {}", back.frame_count()));
    }

    let mut missing = vec![];
    let pcm = mixdown(&p, &clip, &lib, &vfs, &mut missing);
    if !missing.is_empty() {
        failed.push(format!("missing sounds: {missing:?}"));
    }
    let peak = pcm.samples.iter().fold(0f32, |m, s| m.max(s.abs()));
    println!("mixdown: {:.2} s, peak {peak:.3}", pcm.seconds());
    if peak < 0.01 {
        failed.push("the mixdown is silent".into());
    }
    let subs = subtitle_frames(&p, &clip, "en");
    let mf = movie_files("wce_test_movie", Some(&pcm), &subs, "en", &lib, &vfs, &schema).expect("movie files");
    let mf2 = movie_files("wce_test_movie", Some(&pcm), &subs, "en", &lib, &vfs, &schema).expect("movie files");
    if mf.files != mf2.files {
        failed.push("two exports of the files differ".into());
    }
    for (path, b) in &mf.files {
        println!("{path}: {} bytes", b.len());
    }

    // Mounted over the game, the new event plays the new sound.
    let mut extra: BTreeMap<String, Vec<u8>> = mf.files.iter().cloned().collect();
    extra.insert("movies/wce_test_movie.ca_vp8".into(), bytes.clone());
    let overlay = Overlay { base: &vfs, extra };
    let lib2 = SoundLibrary::load(&overlay);
    let versions = movie_audio_versions(&lib2, &overlay, "wce_test_movie");
    match versions.first() {
        Some((_, m @ MediaRef::Loose { .. })) => match lib2.media_bytes(m, &overlay).and_then(|b| decode_wem(&b).ok()) {
            Some(s) => {
                println!("{}: {:?}, {} ch, {:.2} s", mf.event, m, s.channels, s.seconds());
                if (s.seconds() - clip.length).abs() > 0.1 {
                    failed.push(format!("the sound lasts {:.2} s", s.seconds()));
                }
            }
            None => failed.push("the new sound does not decode".into()),
        },
        other => failed.push(format!("{} resolves to {other:?}", mf.event)),
    }
    // The tables decode back.
    for (path, b) in mf.files.iter().filter(|(p, _)| p.starts_with("db/")) {
        let table = path.split('/').nth(1).unwrap();
        let mut ex = DecodeableExtraData::default();
        ex.set_schema(Some(&schema));
        ex.set_table_name(Some(table));
        match DB::decode(&mut Cursor::new(b), &Some(ex)) {
            Ok(db) => println!("{table}: {} rows", db.data().len()),
            Err(e) => failed.push(format!("{table}: {e}")),
        }
    }
    // And the catalog reads the movie with its subtitles.
    let catalog = wce_cinematic::movies::MovieCatalog::load(&overlay);
    let lines = catalog.subtitles("wce_test_movie", "en");
    println!("catalog: {} subtitle lines, first {:?}", lines.len(), lines.first().map(|l| &l.text));
    if lines.len() != 2 || lines[0].text != "The Cinematic tool's first movie." {
        failed.push("the catalog does not read the subtitles back".into());
    }

    std::fs::create_dir_all(&out).unwrap();
    let mut pack_files = mf.files.clone();
    pack_files.push(("movies/wce_test_movie.ca_vp8".into(), bytes));
    if let Err(e) = wce_walls::pack::write_pack(&out.join("wce_test_movie.pack"), "attila", pack_files, vec![]) {
        failed.push(format!("pack: {e}"));
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
