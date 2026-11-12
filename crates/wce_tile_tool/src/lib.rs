//! The Tile tool: battle and campaign tiles (tile.ted + height_map_0.png) on the shared editor. Port of Tile Studio: terrain brushes, select /
//! move / rotate / duplicate / delete / add instances, G-grab on the ground, layers, polygons and AI hints, the weather picker for the tile's
//! map, per-type visibility and distance culling. Everything goes through the shared selection, gizmo, undo stacks and overlay pass; every
//! action is a command too (`commands`), so scripts and menus use the same code.

pub mod areas_chip;
mod building_effects_fix;
pub mod flip_tool;
pub mod actions;
pub mod commands;
pub mod create_panel;
pub mod context_panel;
pub mod entity_types;
pub mod extract_prompt;
mod falloff_editor;
pub mod ground_views;
mod height_import_dialog;
mod image_brush;
mod layers_panel;
pub mod kit_prompt;
pub mod launch;
pub mod launch_prompt;
pub mod measure_tool;
mod bridge_check;
pub mod minimap_panel;
pub mod new_tile_dialog;
pub mod placement_modes;
pub mod prefab_panel;
pub mod prefab_tool;
pub mod proton;
pub mod scene_panel;
pub mod settings;
mod thumbnails;
mod camera_views;
mod selection_tools;
mod links_editor;
mod save_tile_as;
mod share_prompt;
mod tech_chip;
mod resize_tile;
mod tile_properties;
mod tile_database;
mod ref_units;
mod tile_stats;
pub mod tool;
pub mod veg_brush;
pub mod veg_sets;
mod water_view;
pub use water_view::stage_b_of;
pub mod weather_panel;

pub use prefab_tool::PrefabTool;
pub use tool::TileTool;
