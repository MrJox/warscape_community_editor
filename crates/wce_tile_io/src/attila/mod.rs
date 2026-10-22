//! Total War: ATTILA.

pub mod binary;
pub mod building_list;
pub mod images;
pub mod schema;
pub mod compile;
pub mod decompile;
mod ted_project;
pub mod tile_binary;

pub use self::ted_project::TedProject;
pub use self::tile_binary::TileBinary;
