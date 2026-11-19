//! The game's sound files, read and written byte for byte (docs/plans/audio.md, docs/formats/wwise_bank.md and its neighbours).
//!
//! - [`bank`]: Wwise sound banks (`.bnk`): their chunks, and the hierarchy's objects (`HIRC`).
//! - [`wem`]: Wwise sound files (`.wem`): the RIFF chunks and the Wwise Vorbis header.
//! - [`event_data`]: `audio/event_data.dat`, the game's list of events, states, dialogue events and game parameters.
//! - [`amb`]: campaign map ambience emitters (`audio/<map>.amb`).
//! - [`triggers`]: `audio/sound_animation_triggers.csv`, what a `.snd.meta`'s `SOUND_TRIGGER` (read by `wce_formats::anim_meta`) fires.
//! - [`hirc`]: typed hierarchy objects (bank version 112).
//! - [`fnv`]: Wwise's IDs, the FNV-1 hash of a lower-case name.
//!
//! Every reader keeps what it does not interpret, so writing what it read gives back the same bytes.

pub mod amb;
pub mod bank;
pub mod event_data;
pub mod fnv;
pub mod hirc;
mod io;
pub mod triggers;
pub mod wem;

pub use io::FormatError;

/// The result of reading a sound file.
pub type Result<T> = std::result::Result<T, FormatError>;
