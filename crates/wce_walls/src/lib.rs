//! Procedural walls (docs/plans/procedural_walls.md): new buildings made from a base wall piece of the game's packs.
//!
//! - [`building`]: a building's folder (`rigidmodels/buildings/<key>/`) read from the packs and renamed to a new key;
//! - [`warp`]: the map from a base piece's frame to the generated piece's, and its use on the piece's files ([`mesh`], [`logic`],
//!   [`zones`]);
//! - [`db`]: the base's database rows copied under the new key, with [`lines`]: the firing and docking lines the game keeps in them;
//! - [`pack`]: the pack the generated buildings are written into.

pub mod building;
pub mod db;
pub mod lines;
pub mod logic;
pub mod mesh;
pub mod pack;
pub mod warp;
pub mod zones;
