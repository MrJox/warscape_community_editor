//! The battle maps the map views show (docs/re/battle_maps.md): the game's (its packs' `terrain/battles/<map>`, the campaign's and any
//! other a mod adds) and the kits' (filled as BOB does), how they are read, and the packs' tiles in the kit's layout for the placements no
//! kit has. Shared by the Tile tool's map context and the Terrain tool.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use wce_tile::battle_map::locations::BattleLocations;
use wce_tile::battle_map::tile_db::TileDatabase;
use wce_tile::battle_map::context::PlacedMap;
use wce_tile::battle_map::{load_game_map, load_kit_map};
use wce_vfs::files::Files;

/// The campaign battle map of the game's files.
pub const GAME_MAP: &str = "campaign (game files)";

/// The game's battle camera's far plane (scene units; `Lens::default`): the view distance without the context.
pub const GAME_FAR_PLANE: f32 = 13_000.0;

/// The minimap's picture key in `Editor::images`, and its widest side (pixels of the picture; the overlay draws it smaller).
pub const MINIMAP_KEY: &str = "map_context/minimap";
pub const MINIMAP_SIDE: u32 = 1024;

/// A map's LF as a picture (`side` pixels at most a side, rows from the map's top): the sea blue by depth, the land from green through
/// brown and grey to white by height, shaded by the slope from the north-west.
pub fn minimap_pixels(lf: &wce_tile::battle_map::context::LfField, side: u32) -> (u32, u32, Vec<u8>) {
    use wce_tile::battle_map::context::{LF_HEIGHT, VERTICAL_OFFSET};
    let k = lf.w.max(lf.h).div_ceil(side).max(1);
    let (w, h) = (lf.w.div_ceil(k), lf.h.div_ceil(k));
    let m = |x: u32, y: u32| f64::from(lf.values[(y.min(lf.h - 1) * lf.w + x.min(lf.w - 1)) as usize]) / 65535.0 * LF_HEIGHT - VERTICAL_OFFSET;
    let mut px = vec![0u8; (w * h * 4) as usize];
    for y in 0..h {
        for x in 0..w {
            let (sx, sy) = (x * k, y * k);
            let v = m(sx, sy);
            let shade = ((m(sx.saturating_sub(k), sy.saturating_sub(k)) - m(sx + k, sy + k)) / (f64::from(k) * 64.0)).clamp(-1.0, 1.0);
            let c: [f64; 3] = if v <= 0.5 {
                let t = (-v / 20.0).clamp(0.0, 1.0);
                [40.0 - 20.0 * t, 90.0 - 40.0 * t, 150.0 - 40.0 * t]
            } else {
                // Green to brown to grey to white, by height.
                let stops = [(0.0, [96.0, 130.0, 70.0]), (300.0, [150.0, 135.0, 90.0]), (1000.0, [130.0, 125.0, 120.0]), (2500.0, [240.0, 240.0, 240.0])];
                let i = stops.iter().rposition(|s| v >= s.0).unwrap_or(0).min(stops.len() - 2);
                let t = ((v - stops[i].0) / (stops[i + 1].0 - stops[i].0)).clamp(0.0, 1.0);
                std::array::from_fn(|c| (stops[i].1[c] * (1.0 - t) + stops[i + 1].1[c] * t) * (1.0 + 0.35 * shade))
            };

            let o = ((y * w + x) * 4) as usize;
            px[o..o + 4].copy_from_slice(&[c[0].clamp(0.0, 255.0) as u8, c[1].clamp(0.0, 255.0) as u8, c[2].clamp(0.0, 255.0) as u8, 255]);
        }
    }

    (w, h, px)
}

/// The label of the other battle maps in the game's packs: `<folder> (game files)` (the other campaigns', a mod's).
pub const GAME_FILES: &str = " (game files)";

/// The packs' tiles in the kit's layout (`workflow::neighbour_root`: every tile database entry of the mounted packs, mods included; the
/// tiles' folders decompiled on demand), made the first time something needs it (reading the game's tile data takes a while). Its folder
/// is keyed by the mounted packs, so another set of mods gets its own tree.
pub struct PacksTree {
    vfs: Arc<wce_vfs::Vfs>,
    kit: Option<PathBuf>,
    root: PathBuf,
    built: std::sync::OnceLock<Result<(Arc<wce_tile_io::attila::workflow::Game>, PathBuf), String>>,
}

impl PacksTree {
    /// `cache` is the folder the trees go in (`<cache>/neighbours`), `key` the game's.
    pub fn new(vfs: Arc<wce_vfs::Vfs>, kit: Option<PathBuf>, cache: &Path, key: &str) -> Self {
        let packs = vfs.packs().iter().fold(0xCBF2_9CE4_8422_2325u64, |h, p| {
            let len = std::fs::metadata(&p.path).map_or(0, |m| m.len());
            p.name.to_lowercase().bytes().chain(len.to_le_bytes()).fold(h, |h, b| (h ^ u64::from(b)).wrapping_mul(0x0100_0000_01B3))
        });

        Self { vfs, kit, root: cache.join(format!("{key}_{packs:016x}")), built: std::sync::OnceLock::new() }
    }

    fn get(&self) -> Result<&(Arc<wce_tile_io::attila::workflow::Game>, PathBuf), String> {
        self.built.get_or_init(|| {
            let game = Arc::new(wce_tile_io::attila::workflow::Game::from_vfs(self.vfs.clone(), self.kit.as_deref())?);
            let raw = wce_tile_io::attila::workflow::neighbour_root(&game, &self.root)?;
            Ok((game, raw))
        }).as_ref().map_err(Clone::clone)
    }

    /// Whether it is the tree of these mounted packs.
    pub fn same_vfs(&self, vfs: &Arc<wce_vfs::Vfs>) -> bool {
        Arc::ptr_eq(&self.vfs, vfs)
    }

    /// Its `raw_data` folder (the packs' tile database in the kit's format).
    ///
    /// # Errors
    ///
    /// The game's tile data does not read, or the tree can't be written.
    pub fn raw(&self) -> Result<PathBuf, String> {
        self.get().map(|(_, raw)| raw.clone())
    }

    /// A tile's folder, decompiled from the packs the first time.
    ///
    /// # Errors
    ///
    /// As [`Self::raw`], or no pack has the tile, or it does not decompile.
    pub fn tile(&self, location: &str, variation: &str) -> Result<PathBuf, String> {
        let (game, raw) = self.get()?;
        wce_tile_io::attila::workflow::neighbour_tile(game, raw, location, variation)
    }
}

/// The folder in the packs of a map of the list (`GAME_MAP`: the game's own campaign, `game_dir`; `<folder> (game files)`), none for a kit
/// map.
pub fn game_map_folder(name: &str, game_dir: &str) -> Option<String> {
    if name == GAME_MAP {
        return Some(game_dir.to_owned());
    }

    name.strip_suffix(GAME_FILES).map(|f| format!("terrain/battles/{f}"))
}

/// The battle maps in the packs (base game and mods): the folders of `terrain/battles` with a `tile_map.tiles` (`lists`: or a
/// `terrain.tile_list`, a game without kit maps: Rome 2's 104), as the list names them (`game_dir`, the game's own campaign, first as
/// `GAME_MAP`).
pub fn game_maps(files: &dyn Files, game_dir: &str, lists: bool) -> Vec<String> {
    let mut folders: Vec<String> = files.list("terrain/battles/", "/tile_map.tiles").into_iter()
        .filter_map(|p| p.strip_suffix("/tile_map.tiles").map(str::to_owned))
        .collect();
    if lists {
        folders.extend(files.list("terrain/battles/", "/terrain.tile_list").into_iter().filter_map(|p| p.strip_suffix("/terrain.tile_list").map(str::to_owned)));
    }

    folders.sort();
    folders.dedup();
    let own = folders.iter().any(|f| f.eq_ignore_ascii_case(game_dir));
    own.then(|| GAME_MAP.to_owned()).into_iter()
        .chain(folders.into_iter().filter(|f| !f.eq_ignore_ascii_case(game_dir)).map(|f| format!("{}{GAME_FILES}", f.rsplit('/').next().unwrap_or_default())))
        .collect()
}

/// Reads a map (slow for the kit's campaign map: BOB's fill of 2032 x 1437 cells takes about a minute). `game_dir` is the folder of the
/// game's campaign battle map in the packs; a map of the packs (`game_map_folder`) is read from them.
pub fn load_map(name: &str, game_dir: &str, raw: &Path, db: &TileDatabase, files: Option<&dyn Files>) -> Result<(Arc<PlacedMap>, Option<BattleLocations>), String> {
    if let Some(folder) = game_map_folder(name, game_dir) {
        let files = files.ok_or("no game files")?;
        let read = |n: &str| files.read(&format!("{folder}/{n}"));
        let lf = read("lf_height_map.dds").ok();
        // A tile map, else (most of Rome 2's maps) the battle field.
        let map = match (read("tile_map.index"), read("tile_map.tiles")) {
            (Ok(index), Ok(tiles)) => load_game_map(name, db, &index, &tiles, lf.as_deref())?,
            _ => {
                let list = wce_tile::battle_map::tile_list::TileList::parse(&read("terrain.tile_list")?)?;
                PlacedMap::from_tile_list(name, &list, db, lf.as_deref().and_then(wce_tile::battle_map::l16_dds_heights))
            }
        };

        let locations = read("locations.blm").ok().and_then(|b| BattleLocations::parse(&b).ok());
        return Ok((Arc::new(map), locations));
    }

    Ok((Arc::new(load_kit_map(db, &raw.join("terrain/battles").join(name))?), None))
}

/// The battle locations that start at a placement's origin (a settlement's or a port's).
pub fn location_names(loc: Option<&BattleLocations>, x: i32, y: i32) -> String {
    let Some(loc) = loc else { return String::new() };
    let mut names: Vec<&str> = loc.types.iter().filter(|(_, l)| l.iter().any(|l| l.rect[0] == x && l.rect[1] == y)).map(|(t, _)| t.as_str()).collect();
    names.dedup();
    names.join(", ")
}
