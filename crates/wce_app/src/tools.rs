//! The registered tools: each one behind its Cargo feature (see Cargo.toml).

use wce_editor::Tool;

/// Every tool built in, in the switcher's order (Ctrl+1 is the first).
pub fn all_tools() -> Vec<Box<dyn Tool>> {
    #[allow(unused_mut, reason = "a build without any editor feature")]
    let mut tools: Vec<Box<dyn Tool>> = vec![];

    #[cfg(feature = "weather")]
    tools.push(Box::new(wce_weather_tool::WeatherTool::default()));

    #[cfg(feature = "prefab")]
    tools.push(Box::new(wce_tile_tool::PrefabTool::default()));

    #[cfg(feature = "tile")]
    tools.push(Box::new(wce_tile_tool::TileTool::default()));

    #[cfg(feature = "terrain")]
    tools.push(Box::new(wce_terrain_tool::TerrainTool::default()));

    #[cfg(feature = "ui")]
    tools.push(Box::new(wce_ui_tool::UiTool::default()));

    #[cfg(feature = "placeholders")]
    tools.extend(placeholders());

    tools
}

/// The future tools: no features yet (the Terrain tool is a feature of its own).
#[cfg(feature = "placeholders")]
fn placeholders() -> Vec<Box<dyn Tool>> {
    use wce_editor::icons;
    use wce_editor::tool::PlaceholderTool;

    vec![
        Box::new(PlaceholderTool::new(
            "unit", "Unit", icons::USER,
            "Unit editor: skinned models, animations, variant mesh definition (VMD) assembly. Needs the SkinnedInstance render kind, bone palette and the anim/skeleton formats.",
        )),
        Box::new(PlaceholderTool::new("vfx", "VFX", icons::SPARKLES, "Particle and effect editor on the existing particle system and vfx definitions: emitter graph, standalone preview scene, vfx XML export.")),
        Box::new(PlaceholderTool::new("cinematic", "Cinematic", icons::CLAPPERBOARD, "Keyframed scene composition and movie recording over PropertyPaths, with a camera rig, a second viewport and PNG sequence export.")),
    ]
}
