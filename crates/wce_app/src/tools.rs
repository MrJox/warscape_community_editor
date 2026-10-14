//! The registered tools.

use wce_editor::Tool;
use wce_editor::tool::PlaceholderTool;

/// Every tool, in the switcher's order (Ctrl+1 is the first).
pub fn all_tools() -> Vec<Box<dyn Tool>> {
    vec![
        Box::new(wce_weather_tool::WeatherTool::default()),
        Box::new(PlaceholderTool::new("tile", "Tile", "Tile editor: being ported to Rust (the document is; the panels are next).")),
        // Placeholders for the future tools: no features.
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
