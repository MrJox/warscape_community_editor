//! The Tile tool: battle and campaign tiles (tile.ted + height_map_0.png) on the shared editor. Port of Tile Studio: terrain brushes, select /
//! move / rotate / duplicate / delete / add instances, G-grab on the ground, layers, polygons and AI hints, the weather picker for the tile's
//! map, per-type visibility and distance culling. Everything goes through the shared selection, gizmo, undo stacks and overlay pass; every
//! action is a command too (`commands`), so scripts and menus use the same code.

pub mod actions;
pub mod areas_chip;
mod bridge_check;
mod building_effects_fix;
mod building_spline_ui;
mod camera_views;
pub mod commands;
pub mod context_panel;
pub mod create_panel;
pub mod entity_types;
pub mod extract_prompt;
mod falloff_editor;
pub mod flip_tool;
pub mod ground_views;
mod height_import_dialog;
mod image_brush;
pub mod kit_prompt;
pub mod launch;
pub mod launch_prompt;
mod layers_panel;
mod links_editor;
pub mod measure_tool;
pub mod minimap_panel;
pub mod new_tile_dialog;
pub mod placement_modes;
pub mod prefab_panel;
pub mod prefab_tool;
pub mod proton;
mod ref_units;
mod resize_tile;
mod save_tile_as;
pub mod scene_panel;
mod selection_tools;
pub mod settings;
mod share_prompt;
mod tech_chip;
mod thumbnails;
mod tile_database;
mod tile_properties;
mod tile_stats;
pub mod tool;
pub mod veg_brush;
pub mod veg_sets;
mod water_view;
pub use water_view::stage_b_of;
pub mod weather_panel;

pub use prefab_tool::PrefabTool;
pub use tool::TileTool;
