//! A tile as data, independent of any game's files (docs/formats/tile_model.md).
//!
//! - [`Node`]: typed element trees. The scene, the river splines and the tile database entry are trees of classes with typed properties
//!   ([`Value`]), in the shape of TEd's files (wrappers like `children` are nodes too).
//! - [`Raster`]: decoded maps (height, blend, ground type, nogo, LF influence).
//! - [`Tile`]: one variation of a tile: the trees, the maps, the files carried along untouched, and the trivia of the format it was read
//!   from, so that format writes the same bytes back.
//!
//! Formats live in `wce_tile_io`.

pub mod node;
pub mod raster;
pub mod sym;
pub mod tile;
pub mod value;

pub use self::node::{Node, Prop, Trivia};
pub use self::raster::{Raster, Samples};
pub use self::sym::{Sym, SymCache};
pub use self::tile::{Blob, Entry, MapKind, OpaqueFile, RiverSplines, SourceTrivia, Tile, TileMap};
pub use self::value::Value;
