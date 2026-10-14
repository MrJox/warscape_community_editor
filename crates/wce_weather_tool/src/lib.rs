//! The Weather / Lighting tool: battle .environment, campaign .lighting, weather_definitions.xml and vfx XML as documents, a live preview of
//! the active environment (cross-fadable with a second one) on the sample map, the weather selection database picker and editor. Port of the
//! weather_editor application onto the shared shell; everything it did is a command here too (docs/editor.md).

pub mod commands;
pub mod picker;
pub mod tool;
pub mod variation_editor;

pub use tool::WeatherTool;
