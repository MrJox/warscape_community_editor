//! The Terrain tool (docs/plans/terrain_editor.md): Terry's replacement. Phase 2: a battle map opened as a document and shown whole.

mod cell_paint;
mod explicit_tiles;
mod export;
mod launch;
mod map_panel;
mod refill;
mod terry_view;
mod validate;
mod tool;

pub use tool::TerrainTool;
