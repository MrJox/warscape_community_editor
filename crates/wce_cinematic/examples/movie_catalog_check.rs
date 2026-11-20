//! Gate of the movie catalog (docs/plans/cinematic.md, step 2): every movie of the game with its `videos` type, the media its sound
//! event (`Play_Movie_<name>`) plays and its subtitles (timings and text). Fails when a movie with subtitle timings has none that
//! fit its frame count, or when the catalog finds no movie.
//!
//!   movie_catalog_check <game folder> [mod pack...]

use std::path::PathBuf;

use wce_audio::library::{MediaRef, SoundLibrary};
use wce_cinematic::movies::{MovieCatalog, movie_media};
use wce_vfs::files::Files;
use wce_vfs::{Config, GameProfile, Vfs};
use wce_video::Movie;

fn main() {
    let mut args = std::env::args().skip(1);
    let Some(game) = args.next() else {
        eprintln!("movie_catalog_check <game folder> [mod pack...]");
        std::process::exit(2);
    };
    let mods: Vec<String> = args.collect();
    let mut config = Config::new(PathBuf::from(game), GameProfile::from_env().clone());
    config.use_mods = !mods.is_empty();
    config.mod_packs = Some(mods);
    let vfs = Vfs::mount(&config).expect("the game's packs");
    let files: &dyn Files = &vfs;

    let catalog = MovieCatalog::load(files);
    let library = SoundLibrary::load(files);
    let mut failed = vec![];
    let (mut with_audio, mut with_subs) = (0, 0);
    println!("languages: {}", catalog.languages.join(" "));
    for m in &catalog.movies {
        let media = movie_media(&library, files, &m.name);
        let found = media.iter().filter(|r| !matches!(r, MediaRef::Missing { .. })).count();
        if found > 0 {
            with_audio += 1;
        }
        let language = catalog.best_language(&m.name);
        let subs = language.as_deref().map(|l| catalog.subtitles(&m.name, l)).unwrap_or_default();
        if subs.is_empty() && m.video_type.as_deref() == Some("CAMPAIGN_ESTABLISH") && found == 0 {
            continue; // the event pictures: silent, no subtitles
        }
        let frames = vfs.read(&m.path).ok().and_then(|b| Movie::read(&b).ok()).map_or(0, |mv| mv.frame_count());
        if !subs.is_empty() {
            with_subs += 1;
            if subs.iter().any(|s| s.end as usize > frames || s.start > s.end) {
                failed.push(format!("{}: subtitles past its {frames} frames", m.name));
            }
        }
        println!(
            "{:<40} {:<20} {:>5} frames  audio {} of {}  subtitles {} ({})  first: {}",
            m.name,
            m.video_type.as_deref().unwrap_or("-"),
            frames,
            found,
            media.len(),
            subs.len(),
            language.unwrap_or_default(),
            subs.first().map(|s| s.text.chars().take(50).collect::<String>()).unwrap_or_default()
        );
    }
    println!("movies {}, with audio {with_audio}, with subtitles {with_subs}", catalog.movies.len());
    if catalog.movies.is_empty() {
        failed.push("no movie".into());
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
