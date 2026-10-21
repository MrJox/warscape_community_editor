//! A tile (one variation of a tile database entry) as data.

use std::any::Any;
use std::collections::BTreeMap;
use std::fmt;
use std::path::PathBuf;
use std::sync::Arc;

use crate::node::Node;
use crate::raster::Raster;

/// What a map is for.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum MapKind {
    /// 16-bit heights over the tile and a border (TEd: `height_map_<n>.png`).
    Height,

    /// The weights of the blend layers (TEd: `blend_map.tif`, 16 channels of 16 bits).
    Blend,

    /// TEd: `ground_type_map.png`.
    GroundType,

    /// TEd: `nogo.png`.
    Nogo,

    /// TEd: `lf_influence_map.png`.
    LfInfluence,
}

#[derive(Clone, Debug, PartialEq)]
pub struct TileMap {
    pub kind: MapKind,

    /// Which one of its kind (`height_map_1.png`: 1); 0 for kinds there is one of.
    pub index: u32,
    pub raster: Raster,
}

/// A river spline file (TEd: `river_spline<n>.xml`, the export of a `SplineRiverInstance`).
#[derive(Clone, Debug, PartialEq)]
pub struct RiverSplines {
    /// `None` for the unnumbered file (`river_spline.xml`).
    pub index: Option<u32>,
    pub root: Node,
}

/// The tile database entry the tile is a variation of (TEd: `_tile_database/TILES/<name>.xml`). One entry is shared by every
/// variation of the tile.
#[derive(Clone, Debug, PartialEq)]
pub struct Entry {
    /// The entry's file name, which names it in the database (`plains_3x1.xml`).
    pub file_name: String,
    pub root: Node,
}

/// Bytes of a file the model carries without reading them.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Blob {
    Bytes(Arc<[u8]>),

    /// A file on disk, read when it is written out.
    File(PathBuf),
}

impl Blob {

    /// # Errors
    ///
    /// The file could not be read.
    pub fn bytes(&self) -> std::io::Result<Arc<[u8]>> {
        match self {
            Self::Bytes(b) => Ok(b.clone()),
            Self::File(p) => Ok(std::fs::read(p)?.into()),
        }
    }
}

/// A file of the tile's folder that no part of the model holds (a BOB output, an artist's working file, a file that did not decode).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OpaqueFile {
    /// Relative to the tile's folder, `/` separated.
    pub path: String,
    pub data: Blob,
}

/// What a reader keeps so its own writer gives the same bytes back, keyed by part (`tile.ted`, `height_map_0.png` ...). Other formats
/// ignore it. Shared, so cloning a tile is cheap.
pub type SourceTrivia = BTreeMap<String, Arc<dyn Any + Send + Sync>>;

#[derive(Clone, Default)]
pub struct Tile {
    /// The scene (TEd: the `project` element of tile.ted).
    pub scene: Option<Node>,
    pub rivers: Vec<RiverSplines>,
    pub maps: Vec<TileMap>,
    pub entry: Option<Entry>,

    /// The variation's name in the entry (`aa`, `medium`), when known.
    pub variation: Option<String>,
    pub files: Vec<OpaqueFile>,

    /// The format the tile was read from, and that format's trivia.
    pub source_format: Option<&'static str>,
    pub source: SourceTrivia,
}

impl Tile {
    pub fn map(&self, kind: MapKind, index: u32) -> Option<&TileMap> {
        self.maps.iter().find(|m| m.kind == kind && m.index == index)
    }

    pub fn map_mut(&mut self, kind: MapKind, index: u32) -> Option<&mut TileMap> {
        self.maps.iter_mut().find(|m| m.kind == kind && m.index == index)
    }

    /// A format's trivia for a part, when the tile was read by that format.
    pub fn trivia<T: Any + Send + Sync>(&self, part: &str) -> Option<&T> {
        self.source.get(part)?.downcast_ref::<T>()
    }

    /// The tile as data only: no trivia in the trees, no source trivia. Two tiles that mean the same compare equal this way.
    pub fn semantic(&self) -> SemanticTile<'_> {
        SemanticTile(self)
    }
}

impl fmt::Debug for Tile {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Tile")
            .field("scene", &self.scene.as_ref().map(Node::count))
            .field("rivers", &self.rivers.len())
            .field("maps", &self.maps.iter().map(|m| (m.kind, m.index, m.raster.width, m.raster.height, m.raster.channels)).collect::<Vec<_>>())
            .field("entry", &self.entry.as_ref().map(|e| &e.file_name))
            .field("variation", &self.variation)
            .field("files", &self.files.iter().map(|f| &f.path).collect::<Vec<_>>())
            .field("source_format", &self.source_format)
            .field("source", &self.source.keys().collect::<Vec<_>>())
            .finish()
    }
}

/// See [`Tile::semantic`].
pub struct SemanticTile<'a>(&'a Tile);

impl PartialEq for SemanticTile<'_> {
    fn eq(&self, other: &Self) -> bool {
        let (a, b) = (self.0, other.0);
        let tree = |x: &Option<Node>, y: &Option<Node>| match (x, y) {
            (Some(x), Some(y)) => x.without_trivia() == y.without_trivia(),
            (None, None) => true,
            _ => false,
        };

        tree(&a.scene, &b.scene)
            && a.rivers.len() == b.rivers.len()
            && a.rivers.iter().zip(&b.rivers).all(|(x, y)| x.index == y.index && x.root.without_trivia() == y.root.without_trivia())
            && a.maps == b.maps
            && match (&a.entry, &b.entry) {
                (Some(x), Some(y)) => x.file_name == y.file_name && x.root.without_trivia() == y.root.without_trivia(),
                (None, None) => true,
                _ => false,
            }
            && a.variation == b.variation
            && a.files.iter().map(|f| &f.path).eq(b.files.iter().map(|f| &f.path))
    }
}
