//! The Tile tool: battle and campaign tiles (tile.ted + height_map_0.png) on the shared editor. Port of Tile Studio: terrain brushes, select /
//! move / rotate / duplicate / delete / add instances, G-grab on the ground, layers, polygons and AI hints, the weather picker for the tile's
//! map, per-type visibility and distance culling. Everything goes through the shared selection, gizmo, undo stacks and overlay pass; every
//! action is a command too (`commands`), so scripts and menus use the same code.

pub mod commands;
pub mod scene_panel;
pub mod tool;

pub use tool::TileTool;
