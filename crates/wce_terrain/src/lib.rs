//! The Terrain tool's model (docs/plans/terrain_editor.md): a map as data, independent of any game's files. wce_terrain_io reads the
//! game's files into it and writes them out of it.

pub mod cells;
pub mod explicit;
pub mod map;
pub mod paged;
pub mod terry;

pub use map::{Item, LayerRole, LfRole, MapKind, MapModel};
