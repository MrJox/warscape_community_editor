//! A battle map on the scene for a tool (docs/plans/terrain_editor.md, Phase 2): `view::MapView` drives the map context's streamer
//! (wce_tile_editor's `tile_document::context`) on a scene layer, `maps` lists and reads the battle maps it shows.

pub mod maps;
pub mod view;

pub use view::{MapView, Rings};
