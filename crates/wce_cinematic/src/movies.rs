//! The game's movies with what plays alongside them (docs/plans/cinematic.md, "Movies"): the sound event `Play_Movie_<name>` and the
//! subtitles, timed in `subtitle_timings` (frames at the movie's rate) with their text in `vo_fmv_texts.loc` (one entry per movie,
//! sections separated by `|`).

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::io::Cursor;

use rpfm_lib::files::loc::Loc;
use rpfm_lib::files::{Decodeable, DecodeableExtraData};
use rpfm_lib::schema::Schema;
use wce_audio::library::{MediaRef, PlayNode, SoundLibrary};
use wce_audio::sound_db::table_rows;
use wce_vfs::files::Files;

/// The prefix of a movie's sound event (the engine's string `Play_Movie_`).
pub const AUDIO_EVENT_PREFIX: &str = "Play_Movie_";

/// A movie file of the packs.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MovieEntry {
    pub path: String,
    /// The file name without `.ca_vp8` (the key of `videos`, `subtitle_timings` and the sound event).
    pub name: String,
    /// Its `videos` row's type (FULLSCREEN_INTRO, CAMPAIGN_ESTABLISH...), when it has one.
    pub video_type: Option<String>,
}

/// One line of subtitles, shown from `start` to `end` (frames).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Subtitle {
    pub start: u32,
    pub end: u32,
    pub text: String,
}

/// Every movie and its subtitles.
#[derive(Clone, Debug, Default)]
pub struct MovieCatalog {
    pub movies: Vec<MovieEntry>,
    /// The languages `subtitle_timings` has rows for.
    pub languages: Vec<String>,
    /// (language, movie name lower-case) -> (section, start, end).
    timings: HashMap<(String, String), Vec<(u32, u32, u32)>>,
    /// Movie name lower-case -> the loc text's sections (the mounted language's).
    texts: HashMap<String, Vec<String>>,
}

/// The audio event's name for a movie.
pub fn audio_event(movie: &str) -> String {
    format!("{AUDIO_EVENT_PREFIX}{movie}")
}

fn file_stem(path: &str) -> String {
    let file = path.rsplit('/').next().unwrap_or(path);
    file.strip_suffix(".ca_vp8").unwrap_or(file).to_owned()
}

impl MovieCatalog {
    /// The movies of the files, the `videos` and `subtitle_timings` rows (with the game's schema) and the subtitles' loc text.
    pub fn load(files: &dyn Files) -> Self {
        let schema = files.game().and_then(wce_foundation::settings::rpfm_schema_path).and_then(|p| Schema::load(&p, None).ok());
        let mut catalog = Self::default();
        let mut types = HashMap::new();
        if let Some(schema) = &schema {
            for row in table_rows(schema, files, "videos_tables") {
                if let (Some(name), Some(kind)) = (row.get("video_name"), row.get("video_type")) {
                    types.insert(name.to_lowercase(), kind.clone());
                }
            }
            let mut languages = BTreeSet::new();
            for row in table_rows(schema, files, "subtitle_timings_tables") {
                let get = |c: &str| row.get(c).cloned().unwrap_or_default();
                let (Ok(section), Ok(start), Ok(end)) = (get("text_section").parse(), get("start").parse(), get("end").parse()) else {
                    continue;
                };
                let language = get("language");
                languages.insert(language.clone());
                catalog.timings.entry((language, get("foreign_key").to_lowercase())).or_default().push((section, start, end));
            }
            for list in catalog.timings.values_mut() {
                list.sort_unstable();
                list.dedup();
            }
            catalog.languages = languages.into_iter().collect();
        }
        for path in files.list("text/", ".loc") {
            let Ok(bytes) = files.read(&path) else { continue };
            if !path.contains("vo_fmv_texts") {
                continue;
            }
            let Ok(loc) = Loc::decode(&mut Cursor::new(&bytes), &Some(DecodeableExtraData::default())) else { continue };
            for row in loc.data().iter() {
                let (Some(key), Some(text)) = (row.first(), row.get(1)) else { continue };
                let key = key.data_to_string();
                if let Some(movie) = key.strip_prefix("vo_fmv_texts_text_") {
                    let sections = text.data_to_string().split('|').map(|s| s.trim().to_owned()).collect();
                    catalog.texts.insert(movie.to_lowercase(), sections);
                }
            }
        }
        catalog.movies = files
            .list("movies/", ".ca_vp8")
            .into_iter()
            .map(|path| {
                let name = file_stem(&path);
                let video_type = types.get(&name.to_lowercase()).cloned();
                MovieEntry { path, name, video_type }
            })
            .collect();
        catalog
    }

    /// The languages with timings for a movie.
    pub fn languages_of(&self, movie: &str) -> Vec<String> {
        let key = movie.to_lowercase();
        self.languages.iter().filter(|l| self.timings.contains_key(&((*l).clone(), key.clone()))).cloned().collect()
    }

    /// The language whose timings fit the loc text best: as many sections as the text, `en` on a tie or when none fits.
    pub fn best_language(&self, movie: &str) -> Option<String> {
        let key = movie.to_lowercase();
        let sections = self.texts.get(&key).map_or(0, Vec::len);
        let languages = self.languages_of(movie);
        let fits = |l: &String| self.timings.get(&(l.clone(), key.clone())).map_or(0, Vec::len) == sections;
        languages
            .iter()
            .find(|l| l.as_str() == "en" && fits(l))
            .or_else(|| languages.iter().find(|l| fits(l)))
            .or_else(|| languages.iter().find(|l| l.as_str() == "en"))
            .or_else(|| languages.first())
            .cloned()
    }

    /// A movie's subtitles in a language's timings with the loc's text; a section without text shows as empty.
    pub fn subtitles(&self, movie: &str, language: &str) -> Vec<Subtitle> {
        let key = movie.to_lowercase();
        let texts = self.texts.get(&key);
        self.timings
            .get(&(language.to_owned(), key.clone()))
            .map(|list| {
                list.iter()
                    .map(|&(section, start, end)| Subtitle {
                        start,
                        end,
                        text: texts.and_then(|t| t.get(section as usize)).cloned().unwrap_or_default(),
                    })
                    .collect()
            })
            .unwrap_or_default()
    }

    /// The line shown at a frame, if any.
    pub fn line_at(subtitles: &[Subtitle], frame: u32) -> Option<&Subtitle> {
        subtitles.iter().find(|s| s.start <= frame && frame < s.end)
    }

    /// Movies by `video_type` (None: no `videos` row).
    pub fn by_type(&self) -> BTreeMap<Option<String>, Vec<&MovieEntry>> {
        let mut out: BTreeMap<Option<String>, Vec<&MovieEntry>> = BTreeMap::new();
        for m in &self.movies {
            out.entry(m.video_type.clone()).or_default().push(m);
        }
        out
    }
}

/// The media a movie's sound event plays, in the tree's order, each once.
pub fn movie_media(library: &SoundLibrary, files: &dyn Files, movie: &str) -> Vec<MediaRef> {
    let mut out: Vec<MediaRef> = vec![];
    for (_, m) in movie_audio_versions(library, files, movie) {
        if !out.contains(&m) {
            out.push(m);
        }
    }
    out
}

/// The versions of a movie's sound: (the switch value above the media, empty when there is none; the media). Vanilla's movies
/// switch on the speaker setup: `stereo` (2 channels), `surround` (5.1) and `None` (the surround one again); a sting plays one sound.
pub fn movie_audio_versions(library: &SoundLibrary, files: &dyn Files, movie: &str) -> Vec<(String, MediaRef)> {
    let id = wce_audio_format::fnv::id(&audio_event(movie));
    if !library.has_object(id) {
        return vec![];
    }
    let mut out = vec![];
    collect_versions(&library.resolve(id, files), "", &mut out);
    out
}

/// The version to play by default: `stereo` when there is one, else the first.
pub fn preferred_version(versions: &[(String, MediaRef)]) -> Option<usize> {
    versions.iter().position(|(v, _)| v.eq_ignore_ascii_case("stereo")).or(if versions.is_empty() { None } else { Some(0) })
}

fn collect_versions(node: &PlayNode, version: &str, out: &mut Vec<(String, MediaRef)>) {
    // A switch value (kind 0 without media) names the branch below it.
    let version = if node.kind == 0 && node.media.is_none() { node.label.as_str() } else { version };
    if let Some(m) = &node.media {
        out.push((version.to_owned(), m.clone()));
    }
    for c in &node.children {
        collect_versions(c, version, out);
    }
}
