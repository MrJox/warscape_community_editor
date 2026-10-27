//! The Tile tool: battle and campaign tiles (tile.ted + height_map_0.png) on the shared editor. Port of Tile Studio: terrain brushes, select /
//! move / rotate / duplicate / delete / add instances, G-grab on the ground, layers, polygons and AI hints, the weather picker for the tile's
//! map, per-type visibility and distance culling. Everything goes through the shared selection, gizmo, undo stacks and overlay pass; every
//! action is a command too (`commands`), so scripts and menus use the same code.

pub mod actions;
pub mod commands;
pub mod create_panel;
pub mod context_panel;
pub mod entity_types;
pub mod extract_prompt;
mod falloff_editor;
mod height_import_dialog;
mod image_brush;
pub mod kit_prompt;
pub mod launch;
pub mod launch_prompt;
pub mod new_tile_dialog;
pub mod placement_modes;
pub mod prefab_panel;
pub mod scene_panel;
pub mod settings;
pub mod tool;
pub mod veg_brush;
pub mod veg_sets;
mod water_view;
pub mod weather_panel;

pub use tool::TileTool;
