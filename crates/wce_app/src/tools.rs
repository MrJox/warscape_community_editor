//! The registered tools: each one behind its Cargo feature (see Cargo.toml).

use wce_editor::Tool;

/// Every tool built in, in the switcher's order (Ctrl+1 is the first).
#[allow(clippy::vec_init_then_push, reason = "each push is behind its tool's feature: vec![] can't hold them")]
pub fn all_tools() -> Vec<Box<dyn Tool>> {
    #[allow(unused_mut, reason = "a build without any editor feature")]
    let mut tools: Vec<Box<dyn Tool>> = vec![];

    #[cfg(feature = "weather")]
    tools.push(Box::new(wce_weather_tool::WeatherTool::default()));

    #[cfg(feature = "vfx")]
    tools.push(Box::new(wce_vfx_tool::VfxTool::default()));

    #[cfg(feature = "prefab")]
    tools.push(Box::new(wce_tile_tool::PrefabTool::default()));

    #[cfg(feature = "tile")]
    tools.push(Box::new(wce_tile_tool::TileTool::default()));

    #[cfg(feature = "terrain")]
    tools.push(Box::new(wce_terrain_tool::TerrainTool::default()));

    #[cfg(feature = "ui")]
    tools.push(Box::new(wce_ui_tool::UiTool::default()));

    #[cfg(feature = "audio")]
    tools.push(Box::new(wce_audio_tool::AudioTool::default()));

    #[cfg(feature = "unit")]
    tools.push(Box::new(wce_unit_tool::UnitTool::default()));

    #[cfg(feature = "cinematic")]
    tools.push(Box::new(wce_cinematic_tool::CinematicTool::default()));

    #[cfg(feature = "placeholders")]
    tools.extend(placeholders());

    tools
}

/// The future tools: none at present (every editor is a feature of its own).
#[cfg(feature = "placeholders")]
fn placeholders() -> Vec<Box<dyn Tool>> {
    vec![]
}
