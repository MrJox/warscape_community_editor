//! A map (a battle map or a campaign map's terrain project) as the Terrain tool edits it: its cell layers, LF fields, explicit tiles,
//! `.terry` project and XML settings, decoded, and every other file of its folder carried as it is. Each decoded file keeps the bytes it
//! was read from: a file not edited is written back as those bytes, an edited one is made again from the model (wce_terrain_io).

use std::collections::BTreeMap;

use wce_formats::xml_doc::XmlDoc;
use wce_tile_model::Blob;

use crate::cells::CellLayer;
use crate::explicit::ExplicitTiles;
use crate::paged::PagedU16;
use crate::terry::TerryProject;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum MapKind {
    /// `terrain/battles/<map>`: the campaign battle map, a historical battle, a settlement map.
    #[default]
    Battle,

    /// `terrain/campaigns/<map>`: the campaign map's terrain project.
    Campaign,
}

/// What a cell layer holds.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LayerRole {
    /// Tile sets, tiles and variations by colour (`tile_map.png`).
    TileMap,
    Climate,
    Culture,

    /// Snow by season (`snow_map_<season>.png`; empty: `snow_map.png`).
    Snow(String),

    /// A campaign season mask (`season_mask_<season>_01.png`).
    SeasonMask(String),

    /// The campaign layout guide (`layout_map.png`).
    Layout,
}

/// What an LF field holds.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LfRole {
    /// `lf_heights.tif`: the land.
    Land,

    /// `lf_sea_heights.tif`: the sea bed.
    Sea,
}

/// A decoded file: its path in the map's folder, its value and the bytes it was read from.
#[derive(Clone, Debug)]
pub struct Item<T> {
    pub path: String,
    value: T,
    pub origin: Blob,
    edited: bool,
    original_version: Option<u32>,
}

impl<T> Item<T> {
    pub fn new(path: impl Into<String>, value: T, origin: Blob) -> Self {
        Self { path: path.into(), value, origin, edited: false, original_version: None }
    }

    pub fn get(&self) -> &T {
        &self.value
    }

    /// The value to change: the file is made again when written.
    pub fn edit(&mut self) -> &mut T {
        self.edited = true;
        &mut self.value
    }

    pub fn edited(&self) -> bool {
        self.edited
    }
}

impl Item<wce_tile::battle_map::tile_list::TileList> {
    /// A native list loaded from its original payload. Editing the value cannot change this format provenance.
    pub fn new_native_tile_list(path: impl Into<String>, value: wce_tile::battle_map::tile_list::TileList, origin: Blob) -> Self {
        let original_version = Some(value.version);
        Self { path: path.into(), value, origin, edited: false, original_version }
    }

    /// The loaded native format version; `None` for an item constructed without native provenance.
    pub fn original_version(&self) -> Option<u32> {
        self.original_version
    }
}

#[derive(Clone, Debug, Default)]
pub struct MapModel {
    pub name: String,
    pub kind: MapKind,

    pub layers: Vec<(LayerRole, Item<CellLayer>)>,
    pub lf: Vec<(LfRole, Item<PagedU16>)>,
    pub explicit: Option<Item<ExplicitTiles>>,
    pub project: Option<Item<TerryProject>>,

    /// The XML settings read losslessly (`config.xml`, `masks.xml`, `map_info.xml`, ...).
    pub xml: Vec<Item<XmlDoc>>,

    /// Every other file, by path in the map's folder (`/` separated).
    pub carried: BTreeMap<String, Blob>,

    /// A map in its built form (a game without kit maps: Rome 2), read and written by `wce_terrain_io::built`; else a kit map folder.
    pub built: bool,

    /// A built map's battle field (`terrain.tile_list`): its placements, edited as they are (no fill).
    pub tile_list: Option<Item<wce_tile::battle_map::tile_list::TileList>>,
}

impl MapModel {
    pub fn layer(&self, role: &LayerRole) -> Option<&Item<CellLayer>> {
        self.layers.iter().find(|(r, _)| r == role).map(|(_, l)| l)
    }

    pub fn layer_mut(&mut self, role: &LayerRole) -> Option<&mut Item<CellLayer>> {
        self.layers.iter_mut().find(|(r, _)| r == role).map(|(_, l)| l)
    }

    pub fn lf(&self, role: LfRole) -> Option<&Item<PagedU16>> {
        self.lf.iter().find(|(r, _)| *r == role).map(|(_, f)| f)
    }

    pub fn lf_mut(&mut self, role: LfRole) -> Option<&mut Item<PagedU16>> {
        self.lf.iter_mut().find(|(r, _)| *r == role).map(|(_, f)| f)
    }

    /// The map's size in cells: its tile map's.
    pub fn cells(&self) -> Option<(u32, u32)> {
        self.layer(&LayerRole::TileMap).map(|l| (l.get().width(), l.get().height()))
    }

    /// The memory the decoded samples take (bytes, about): the LF fields and the cell layers.
    pub fn decoded_bytes(&self) -> usize {
        self.lf.iter().map(|(_, f)| f.get().bytes()).sum::<usize>() + self.layers.iter().map(|(_, l)| l.get().cells().len() * 2).sum::<usize>()
    }
}
