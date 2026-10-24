//! The registered tools: each one behind its Cargo feature (see Cargo.toml).

use wce_editor::Tool;

/// Every tool built in, in the switcher's order (Ctrl+1 is the first).
pub fn all_tools() -> Vec<Box<dyn Tool>> {
    #[allow(unused_mut, reason = "a build without any editor feature")]
    let mut tools: Vec<Box<dyn Tool>> = vec![];

    #[cfg(feature = "weather")]
    tools.push(Box::new(wce_weather_tool::WeatherTool::default()));

    #[cfg(feature = "tile")]
    tools.push(Box::new(wce_tile_tool::TileTool::default()));

    #[cfg(feature = "ui")]
    tools.push(Box::new(wce_ui_tool::UiTool::default()));

    #[cfg(feature = "placeholders")]
    tools.extend(placeholders());

    tools
}

/// The future tools: no features yet.
#[cfg(feature = "placeholders")]
fn placeholders() -> Vec<Box<dyn Tool>> {
    use wce_editor::tool::PlaceholderTool;

    vec![
        Box::new(PlaceholderTool::new(
            "unit", "Unit",
            "Unit editor: skinned models, animations, variant mesh definition (VMD) assembly. Needs the SkinnedInstance render kind, bone palette and the anim/skeleton formats.",
        )),
        Box::new(PlaceholderTool::new("vfx", "VFX", "Particle and effect editor on the existing particle system and vfx definitions: emitter graph, standalone preview scene, vfx XML export.")),
        Box::new(PlaceholderTool::new(
            "terrain", "Terrain",
            "Battle map and Terry terrain: tilemap painting and the pixel-close terrain pass (see docs/re/terrain.md), tile_list/blm export, BOB terrain stages.",
        )),
        Box::new(PlaceholderTool::new("cinematic", "Cinematic", "Keyframed scene composition and movie recording over PropertyPaths, with a camera rig, a second viewport and PNG sequence export.")),
    ]
}
