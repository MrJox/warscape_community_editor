//! The Audio tool (docs/plans/audio.md): browse the game's sound banks with names instead of IDs, search every bank, see what an
//! event plays, and listen. Editing (replace, add, remove) comes with the later steps of the plan.

mod document;
mod player;
mod tool;

pub use tool::AudioTool;
