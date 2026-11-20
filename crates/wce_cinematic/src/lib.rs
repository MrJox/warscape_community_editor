//! The Cinematic tool's headless layer (docs/plans/cinematic.md).
//!
//! - [`movies`]: the game's movies, their sound events and subtitles.
//! - [`scene`]: the keyed scene (project, units, clips, tracks, keys) and its `project.json`.
//! - [`curves`], [`eval`]: a clip's scene at a time, a pure function of the keys.
//! - [`support`]: what a historical battle can reproduce of each track.
//! - [`movie_export`]: a clip as a game-ready movie (pictures, sound event, videos and subtitle rows, loc).
//! - [`battle_export`], [`logic`], [`battle_tables`]: the project as a historical battle (XML, Lua, DB rows, loc, a pack).

pub mod battle_export;
pub mod battle_tables;
pub mod curves;
pub mod eval;
pub mod logic;
pub mod movie_export;
pub mod movies;
pub mod scene;
pub mod speeds;
pub mod support;

/// The largest number in a column of a game table (0 when none).
pub fn movies_table_max(schema: &rpfm_lib::schema::Schema, files: &dyn wce_vfs::files::Files, table: &str, column: &str) -> i64 {
    wce_audio::sound_db::table_rows(schema, files, table).iter().filter_map(|r| r.get(column)?.parse::<i64>().ok()).max().unwrap_or(0)
}
