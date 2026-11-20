//! The Cinematic tool (docs/plans/cinematic.md). Step 2: the movie viewer (any `.ca_vp8` of the packs with its sound and subtitles).

mod export;
mod frames;
mod preview;
mod scene_doc;
mod scene_ui;
mod tool;
mod viewer;

pub use tool::CinematicTool;
