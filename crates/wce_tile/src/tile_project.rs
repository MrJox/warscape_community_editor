//! One tile opened for editing. Its root is its tile database entry (_tile_database/TILES/*.xml, `tile_entry`): the entry says where the files
//! are (location + variation -> the folder with tile.ted, height_map_0.png, blend_map.tif), how big the tile is, its colour, the ground texture
//! groups, the height and normal map parameters of the variation and the tile links. The project holds the entry, tile.ted (`TedScene`) and the
//! height map; opening a bare folder or tile.ted looks its entry up from the path (indexed: `tile_index`) and warns when there is none. Reads and
//! writes the raw files only.

use std::path::{Path, PathBuf};

use wce_formats::png_file::PngFile;
use wce_formats::tile_entry::TileEntry;
use wce_formats::tile_index::TileIndex;
use wce_foundation::wce_prof;

use crate::height_field::HeightField;
use crate::ted_scene::TedScene;

pub type Result<T> = std::result::Result<T, String>;

/// What the tool needs from the entry every frame, copied out of it (the entry itself is `TileProject::entry`).
#[derive(Clone, Debug, PartialEq)]
pub struct TileInfo {
    /// terrain\tiles\battle\<set>\<tile> (TILE@location).
    pub location: String,

    /// Leaf folder, e.g. "medium", "aa".
    pub variation: String,

    /// TILE width / height in cells (128 m each).
    pub cells_x: u32,
    pub cells_y: u32,

    /// VARIATION min_height / scale (metres).
    pub min_height: f64,
    pub scale: f64,

    /// VARIATION values (overlap: cells of the height map's border).
    pub normal_strength: f64,
    pub overlap_border_size: f64,
    pub raw_data_tri_density: u32,

    /// The entry and the variation were found.
    pub found: bool,
}

impl Default for TileInfo {
    fn default() -> Self {
        Self {
            location: String::new(), variation: String::new(), cells_x: 0, cells_y: 0, min_height: 0.0, scale: 100.0, normal_strength: 0.0,
            overlap_border_size: 0.0, raw_data_tri_density: 0, found: false,
        }
    }
}

/// The raw_data folder an entry file or a tile path lives under (the part before terrain/tiles), if there is one.
pub fn kit_raw_root(path: &Path) -> Option<PathBuf> {
    let g = path.to_string_lossy().replace('\\', "/");
    let at = g.to_lowercase().find("/terrain/tiles/")?;
    Some(PathBuf::from(&g[..at]))
}

/// The numbers of TileInfo from the entry and one of its variations (`None`: the tile folder itself, no variation numbers).
fn fill_info(e: &TileEntry, variation: Option<usize>, info: &mut TileInfo) {
    info.location = e.location();
    info.cells_x = e.width();
    info.cells_y = e.height();
    let vars = e.variations();
    let Some(v) = variation.and_then(|v| vars.get(v)) else { return };
    info.variation = v.location.clone();
    info.min_height = v.min_height;
    info.scale = v.scale;
    info.normal_strength = v.normal_strength;
    info.overlap_border_size = v.overlap_border_size;
    info.raw_data_tri_density = v.raw_data_tri_density;
    info.found = true;
}

/// The tile database entry of a raw tile folder (<...>/raw_data/terrain/tiles/<battle|campaign>/<set>/.../<variation>), found through the
/// kit's index (no scan per call).
#[derive(Debug, Default)]
pub struct FoundTile {
    pub info: TileInfo,
    pub entry: Option<TileEntry>,
    pub variation: Option<usize>,
    pub raw_root: Option<PathBuf>,
}

pub fn find_tile_info(tile_dir: &Path) -> FoundTile {
    let Some(root) = kit_raw_root(tile_dir) else { return FoundTile::default() };
    let index = TileIndex::shared(&root);
    let r = index.resolve(tile_dir);
    let Some(indexed) = r.entry else { return FoundTile::default() };
    let Ok(entry) = TileEntry::load(&indexed.xml) else { return FoundTile::default() };
    let mut info = TileInfo::default();
    fill_info(&entry, r.variation, &mut info);

    // The folder is not one of the entry's variations: say which tile it is anyway.
    if info.variation.is_empty() {
        let g = tile_dir.to_string_lossy().replace('\\', "/");
        info.variation = g.rsplit('/').next().unwrap_or_default().to_owned();
    }

    FoundTile { info, entry: Some(entry), variation: r.variation, raw_root: Some(root) }
}

#[derive(Debug)]
pub struct TileProject {
    /// The variation folder (holds tile.ted).
    pub dir: PathBuf,
    pub info: TileInfo,

    /// The tile's database entry, `None` when there is none (a tile.ted outside a kit).
    pub entry: Option<TileEntry>,

    /// Index in the entry's variations, `None` when unknown.
    pub variation: Option<usize>,

    /// The kit's raw_data folder, `None` outside a kit.
    pub raw_root: Option<PathBuf>,

    /// What is off about how it opened (no entry, ...), empty when nothing.
    pub warning: String,
    pub scene: TedScene,
    pub height_png: Option<PngFile>,
    pub height: Option<HeightField>,
    pub height_dirty: bool,

    /// Scene units per metre horizontally.
    pub unit_scale: f64,
}

/// What the parallel half of opening reads besides the scene.
struct Side {
    found: Option<FoundTile>,
    height: Option<(PngFile, HeightField)>,
}

impl TileProject {

    /// A variation folder, a tile.ted or a tile folder: the entry is resolved from the path (warning when there is none).
    ///
    /// # Errors
    ///
    /// No tile.ted, or a file that does not read.
    pub fn open(path: &Path) -> Result<Self> {
        let dir = if path.is_file() && path.file_name().is_some_and(|n| n == "tile.ted") { path.parent().unwrap_or(path).to_path_buf() } else { path.to_path_buf() };
        if !dir.join("tile.ted").exists() {
            // A tile folder that holds its variations: the first one (the entry lists them in order).
            if let Some(root) = kit_raw_root(&dir) {
                let index = TileIndex::shared(&root);
                let r = index.resolve(&dir);
                if let Some(indexed) = r.entry
                    && r.variation.is_none() && !indexed.variations.is_empty()
                {
                    let entry = TileEntry::load(&indexed.xml).map_err(|error| error.to_string())?;
                    return Self::open_entry_resolved(entry, 0, root);
                }
            }
        }

        Self::open_dir(dir, None)
    }

    /// The entry file (_tile_database/TILES/x.xml) and one of its variations (`None` = the first).
    ///
    /// # Errors
    ///
    /// The entry does not read, is not inside a kit, or has no such variation.
    pub fn open_entry(entry_xml: &Path, variation: Option<usize>) -> Result<Self> {
        let entry = TileEntry::load(entry_xml).map_err(|error| error.to_string())?;
        let root = entry.raw_root().ok_or_else(|| format!("{} is not inside <kit>/raw_data/terrain/tiles/<kind>/_tile_database/TILES", entry_xml.display()))?;
        Self::open_entry_resolved(entry, variation.unwrap_or(0), root)
    }

    fn open_entry_resolved(entry: TileEntry, v: usize, root: PathBuf) -> Result<Self> {
        let vars = entry.variations();
        if vars.is_empty() {
            return Err(format!("{} lists no variations", entry.file().display()));
        }

        if v >= vars.len() {
            let name = entry.file().file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
            return Err(format!("{name} has {} variations, no number {v}", vars.len()));
        }

        let mut info = TileInfo::default();
        fill_info(&entry, Some(v), &mut info);
        let dir = entry.variation_folder(&root, v);
        let found = FoundTile { info, entry: Some(entry), variation: Some(v), raw_root: Some(root) };
        Self::open_dir(dir, Some(found))
    }

    /// Reads the three parts of a tile side by side: the scene (the long one), the height map (needs the entry's min_height / scale) and, when
    /// the caller did not bring it, the entry.
    fn open_dir(dir: PathBuf, found: Option<FoundTile>) -> Result<Self> {
        wce_prof!("tile.open");
        let ted = dir.join("tile.ted");
        if !ted.exists() {
            return Err(format!("no tile.ted in {}", dir.display()));
        }

        let (scene, side) = rayon::join(
            || -> Result<TedScene> {
                let bytes = {
                    wce_prof!("tile.open.slurp_ted");
                    std::fs::read(&ted).map_err(|error| format!("{}: {error}", ted.display()))?
                };

                wce_prof!("tile.open.parse_ted");
                TedScene::parse(bytes)
            },
            || -> Result<Side> {
                let found = found.unwrap_or_else(|| {
                    wce_prof!("tile.open.tile_info");
                    find_tile_info(&dir)
                });

                let hm = dir.join("height_map_0.png");
                let height = if hm.exists() {
                    wce_prof!("tile.open.height");
                    let bytes = std::fs::read(&hm).map_err(|error| format!("{}: {error}", hm.display()))?;
                    let png = PngFile::parse(&bytes).map_err(|error| error.to_string())?;
                    let field = HeightField::from_png(&png, found.info.min_height, found.info.scale)?;
                    Some((png, field))
                } else {
                    None
                };

                Ok(Side { found: Some(found), height })
            },
        );

        let scene = scene?;
        let side = side?;
        let found = side.found.unwrap_or_default();
        let (height_png, height) = side.height.map_or((None, None), |(png, field)| (Some(png), Some(field)));
        let warning = match &found.entry {
            None => format!("no tile database entry for {}: the tile opens without its height scale, size, colour and links", dir.display()),
            Some(entry) if !found.info.found => format!(
                "the folder '{}' is not a variation of the tile database entry {}",
                dir.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default(),
                entry.file().file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()
            ),

            Some(_) => String::new(),
        };

        Ok(Self {
            dir, info: found.info, entry: found.entry, variation: found.variation, raw_root: found.raw_root, warning, scene, height_png, height,
            height_dirty: false, unit_scale: 2.0,
        })
    }

    /// Writes tile.ted (when the scene is modified, or `force_scene`) and height_map_0.png (when `height_dirty`, or `force_height`); the
    /// message says what was written, `false` if nothing was. A file that is overwritten the first time gets a `.bak` copy.
    ///
    /// # Errors
    ///
    /// A file could not be written.
    pub fn save(&self, out_dir: &Path, force_scene: bool, force_height: bool) -> Result<(bool, String)> {
        let mut msg = String::new();
        let mut wrote = false;
        let write = |name: &str, bytes: &[u8]| -> Result<()> {
            let target = out_dir.join(name);
            let bak = out_dir.join(format!("{name}.bak"));
            if target.exists() && !bak.exists() {
                std::fs::copy(&target, &bak).map_err(|error| error.to_string())?;
            }

            std::fs::write(&target, bytes).map_err(|_| format!("could not write {}", target.display()))
        };

        std::fs::create_dir_all(out_dir).map_err(|error| error.to_string())?;
        if self.scene.modified() || force_scene {
            write("tile.ted", &self.scene.save())?;
            msg += "tile.ted saved. ";
            wrote = true;
        }

        if let (Some(field), Some(base)) = (&self.height, &self.height_png)
            && (self.height_dirty || force_height)
        {
            let png = field.to_png(base);
            let bytes = if png.has_ancillary { png.encode() } else { png.write() }.map_err(|error| error.to_string())?;
            write("height_map_0.png", &bytes)?;
            msg += if png.has_ancillary { "height_map_0.png saved (source had extra chunks: re-encoded). " } else { "height_map_0.png saved. " };
            wrote = true;
        }

        Ok((wrote, if wrote { msg } else { "nothing to save".to_owned() }))
    }

    pub fn has_entry(&self) -> bool {
        self.entry.is_some()
    }

    pub fn has_height(&self) -> bool {
        self.height.is_some()
    }

    pub fn scene_modified(&self) -> bool {
        self.scene.modified()
    }

    /// Tile side in metres along x.
    pub fn extent_m(&self) -> f64 {
        if self.info.found { f64::from(self.info.cells_x) * 128.0 } else { 256.0 }
    }

    pub fn extent_x(&self) -> f64 {
        self.extent_m()
    }

    /// Along z (the height map's rows).
    pub fn extent_z(&self) -> f64 {
        if self.info.found && self.info.cells_y != 0 { f64::from(self.info.cells_y) * 128.0 } else { self.extent_m() }
    }

    /// Where the height map sits. Measured on the shipped tiles (docs/formats/tile_ted.md): the map has a border of one cell (128 m) on every
    /// side, (cells + 2) * ppc pixels per axis, one pixel per 128 / ppc metres, pixel (0, 0) at (-128, -128) m, no flip; instance heights then
    /// sit on the terrain (mean error 0.7 m on western_city_rome, 1.9 m with the map stretched over the tile). Without a tile database entry (or
    /// when the sizes do not fit) the map is stretched over the tile.
    pub fn bordered(&self) -> bool {
        let Some(h) = &self.height else { return false };
        let (cx, cy) = (self.info.cells_x + 2, self.info.cells_y + 2);
        self.info.found && self.info.cells_x != 0 && self.info.cells_y != 0 && h.width() % cx == 0 && h.height() % cy == 0 && h.width() / cx == h.height() / cy
    }

    pub fn border_m(&self) -> f64 {
        if self.bordered() { 128.0 } else { 0.0 }
    }

    pub fn mpp_x(&self) -> f64 {
        match &self.height {
            None => 1.0,
            Some(h) if self.bordered() => 128.0 / f64::from(h.width() / (self.info.cells_x + 2)),
            Some(h) => self.extent_x() / f64::from(h.width() - 1),
        }
    }

    pub fn mpp_z(&self) -> f64 {
        match &self.height {
            None => 1.0,
            Some(h) if self.bordered() => 128.0 / f64::from(h.height() / (self.info.cells_y + 2)),
            Some(h) => self.extent_z() / f64::from(h.height() - 1),
        }
    }

    pub fn pixel_x(&self, x_m: f64) -> f64 {
        (x_m + self.border_m()) / self.mpp_x()
    }

    pub fn pixel_z(&self, z_m: f64) -> f64 {
        (z_m + self.border_m()) / self.mpp_z()
    }

    pub fn height_at(&self, x_m: f64, z_m: f64) -> f64 {
        self.height.as_ref().map_or(0.0, |h| h.sample(self.pixel_x(x_m), self.pixel_z(z_m)))
    }
}
