//! The Cinematic tool's headless layer (docs/plans/cinematic.md).
//!
//! - [`movies`]: the game's movies, their sound events and subtitles.
//! - [`scene`]: the keyed scene (project, units, clips, tracks, keys) and its `project.json`.
//! - [`curves`], [`eval`]: a clip's scene at a time, a pure function of the keys.
//! - [`support`]: what a historical battle can reproduce of each track.
//! - [`battle_export`], [`logic`], [`battle_tables`]: the project as a historical battle (XML, Lua, DB rows, loc, a pack).

pub mod battle_export;
pub mod battle_tables;
pub mod curves;
pub mod eval;
pub mod logic;
pub mod movies;
pub mod scene;
pub mod support;
