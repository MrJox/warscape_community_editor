//! The Terrain tool's model (docs/plans/terrain_editor.md): a map as data, independent of any game's files. wce_terrain_io reads the
//! game's files into it and writes them out of it.

pub mod bca;
pub mod campaign_props;
pub mod cell_edit;
pub mod cells;
pub mod deployment;
pub mod explicit;
pub mod lf_edit;
pub mod lf_patch;
pub mod locations;
pub mod map;
pub mod paged;
pub mod pyramid;
pub mod terry;

pub use map::{Item, LayerRole, LfRole, MapKind, MapModel};
