//! The Weather / Lighting tool: battle .environment, campaign .lighting, weather_definitions.xml and vfx XML as documents, a live preview of
//! the active environment (cross-fadable with a second one) on the sample map, the weather selection database picker and editor. Port of the
//! weather_editor application onto the shared shell; everything it did is a command here too (docs/editor.md).

pub mod commands;
mod gallery;
mod mix;
pub mod picker;
mod preview;
mod sky_strip;
pub mod tool;
pub mod variation_editor;
mod visual;

pub use tool::WeatherTool;
