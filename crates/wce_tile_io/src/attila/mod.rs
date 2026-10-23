//! Total War: ATTILA.

pub mod ati_dxt;
pub mod binary;
pub mod bob_tile;
pub mod building_list;
pub mod carry;
pub mod images;
pub mod schema;
pub mod terrain_mesh;
pub mod river;
pub mod stage_b;
pub mod stage_c;
pub mod terrain_outlines;
pub mod tile_links;
pub mod compile;
pub mod decompile;
pub mod game_data;
mod ted_project;
pub mod tile_binary;
pub mod workflow;

pub use self::ted_project::TedProject;
pub use self::tile_binary::TileBinary;
