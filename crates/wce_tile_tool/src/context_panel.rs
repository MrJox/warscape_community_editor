//! The Map context section of the Tile panel: the open tile among its neighbours in a battle map (wce_tile_editor's `context`). The maps are
//! the game's campaign battle map (terrain.pack's `terrain/battles/main_attila_map`, as the game ships it) and the kit's battle maps (filled
//! as BOB does). Where the tile is placed more than once (the settlement tiles are reused across settlements), the user picks the placement;
//! where it is placed nowhere, a cell and a rotation. The context streams in chunks around the camera (or the tile): reading the map and its
//! frame runs on a thread, the chunks on the streamer's workers; every frame puts what they finished on the scene.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::mpsc::{Receiver, channel};

use wce_editor::editor::Editor;
use wce_editor::gui;
use wce_foundation::{log, wce_prof};
use wce_foundation::math::Float4x4;
use wce_scene::render_scene::{DocumentId, EntityId, LayerId, Payload, RenderScene, SnowRegions, SourceRef};
use wce_tile::battle_map::context::{CELL, PlacedMap, Spot};
use wce_tile::battle_map::locations::BattleLocations;
use wce_tile::battle_map::tile_db::TileDatabase;
use wce_tile::battle_map::{kit_maps, load_game_map, load_kit_map};
use wce_tile_editor::terrain_bake::TerrainLook;
use wce_tile_editor::tile_document::TileDocument;
use wce_tile_editor::tile_document::context::{CHUNK, ChunkChange, ChunkKey, ContextRequest, Streamer, TileProvider, cache};
use wce_vfs::files::Files;

/// How long a frame spends putting finished chunks on the scene (one at least; the rest wait for the next frames).
const APPLY_BUDGET: std::time::Duration = std::time::Duration::from_millis(3);

/// The campaign battle map of the game's files.
pub const GAME_MAP: &str = "campaign (game files)";

/// The game's battle camera's far plane (scene units; `Lens::default`): the view distance without the context.
pub const GAME_FAR_PLANE: f32 = 13_000.0;

/// The minimap's picture key in `Editor::images`, and its widest side (pixels of the picture; the overlay draws it smaller).
const MINIMAP_KEY: &str = "map_context/minimap";
const MINIMAP_SIDE: u32 = 1024;

/// A map's LF as a picture (`side` pixels at most a side, rows from the map's top): the sea blue by depth, the land from green through
/// brown and grey to white by height, shaded by the slope from the north-west.
fn minimap_pixels(lf: &wce_tile::battle_map::context::LfField, side: u32) -> (u32, u32, Vec<u8>) {
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

pub enum Job {
    Map(String, Result<(Arc<PlacedMap>, Option<BattleLocations>), String>),
    Built(u64, Box<Streamer>),
}

pub struct ContextUi {
    /// The folder of the game's campaign battle map in the packs (the current game's).
    pub game_dir: &'static str,

    /// For a game whose kit has no tiles (Rome 2): the neighbours' tree (a `raw_data` with the whole database's entries) and what makes a
    /// tile's folder in it (`workflow::neighbour_tile`). See [`Self::raw_of`].
    pub neighbours: Option<(PathBuf, TileProvider)>,

    /// For a game whose kit has tiles: the packs' tiles, for a placement whose tile no kit has (`TileTool::prepare_neighbours`), and their
    /// tree (whose tile database reads the game's battle maps: a mod's map names the mod's tiles).
    pub packs_fallback: Option<TileProvider>,
    pub packs_tree: Option<Arc<PacksTree>>,

    /// The `raw_data` folders a placement's tile is looked for in, in order (`TileTool::kit_roots`: the projects, then the kits), and
    /// when they were looked up (`gui::time`).
    pub tile_roots: Vec<PathBuf>,
    pub tile_roots_time: Option<f64>,
    pub shown: bool,
    pub maps: Vec<String>,
    pub map: i32,

    /// The loaded map (its name) and its battle locations (the game's map only).
    pub loaded: Option<(String, Arc<PlacedMap>, Option<BattleLocations>)>,

    /// The open tile's placements in the loaded map, and the chosen one (`placements.len()`: the manual spot).
    pub placements: Vec<usize>,
    pub choice: usize,
    pub manual: [i32; 3],

    /// The rings around the focus (cells): everything, the tiles' coarse ground and buildings, the LF ground.
    pub near: i32,
    pub mid: i32,
    pub far: i32,

    /// The LF ground the camera sees (cells around the focus, whole chunks): the far ring is at least that, so that no chunk comes and
    /// goes at the horizon.
    far_view: i32,
    pub trees: bool,

    /// The camera's view distance while the context shows (scene units; the game's battle camera sees 13 000), and whether the minimap
    /// shows over the viewport.
    pub view_distance: f32,
    pub minimap: bool,

    /// The minimap's picture: the map it is of (its name) and the image's key in `Editor::images`.
    minimap_of: String,

    /// The rings follow the camera (else they stay around the tile).
    pub follow: bool,
    pub status: String,
    pub busy: String,
    rx: Option<Receiver<Job>>,

    /// The streamer, the scene layer of the context and the tile it was built for, the build's number (a newer request wins).
    pub streamer: Option<Box<Streamer>>,
    layer: LayerId,
    doc: DocumentId,
    pub build: u64,

    /// The look the build on its way was asked with: a look that changes again meanwhile starts another, the same one waits for it.
    build_look: Option<TerrainLook>,

    /// Each chunk's entities on the layer (added in a row: a range of ids), and the sea's (with the focus it is around).
    chunks: HashMap<ChunkKey, std::ops::Range<EntityId>>,

    /// The streamer's chunks waiting for the renderer to have their terrains' GPU side, and the ones to take off.
    waiting: Vec<(ChunkKey, Vec<(Float4x4, Payload)>)>,
    gone: Vec<ChunkKey>,
    sea: Option<EntityId>,
    sea_at: (f64, f64),
    db: Option<(PathBuf, Arc<TileDatabase>)>,
    maps_for: PathBuf,

    /// The season's snow where the map has it (its `snow_map_<season>.png`), how much; the snow map loaded (map folder, season) and the
    /// regions' key the scene has (season, on, amount).
    pub snow_on: bool,
    pub snow_amount: f32,
    snow_map: Option<(PathBuf, String, Option<Arc<SnowMap>>)>,
    snow_key: Option<(String, bool, u32)>,
}

impl Default for ContextUi {
    fn default() -> Self {
        Self {
            game_dir: wce_vfs::GameProfile::ATTILA.campaign_battle_map, neighbours: None, packs_fallback: None, packs_tree: None, tile_roots: vec![], tile_roots_time: None, shown: false, maps: vec![], map: 0, loaded: None, placements: vec![], choice: 0, manual: [0, 0, 0], near: 8, mid: 24, far: 64, far_view: 0, trees: true, view_distance: GAME_FAR_PLANE, minimap: true, minimap_of: String::new(),
            follow: true, status: String::new(), busy: String::new(), rx: None, streamer: None, layer: 0, doc: 0, build: 0, build_look: None, chunks: HashMap::new(), waiting: vec![],
            gone: vec![], sea: None,
            sea_at: (0.0, 0.0), db: None, maps_for: PathBuf::new(), snow_on: true, snow_amount: 1.0, snow_map: None, snow_key: None,
        }
    }
}

/// A season's snow map: its size and a byte per cell.
type SnowMap = (u32, u32, Vec<u8>);

pub(crate) fn database(ui: &mut ContextUi, raw: &Path) -> Result<Arc<TileDatabase>, String> {
    if let Some((r, db)) = &ui.db
        && r == raw
    {
        return Ok(db.clone());
    }

    let db = Arc::new(TileDatabase::load(&raw.join("terrain/tiles/battle/_tile_database"))?);
    ui.db = Some((raw.to_path_buf(), db.clone()));
    Ok(db)
}

/// The folder in the packs of a map of the list (`GAME_MAP`: the game's own campaign, `game_dir`; `<folder> (game files)`), none for a kit
/// map.
pub fn game_map_folder(name: &str, game_dir: &str) -> Option<String> {
    if name == GAME_MAP {
        return Some(game_dir.to_owned());
    }

    name.strip_suffix(GAME_FILES).map(|f| format!("terrain/battles/{f}"))
}

/// The battle maps in the packs (base game and mods): the folders of `terrain/battles` with a `tile_map.tiles`, as the list names them
/// (`game_dir`, the game's own campaign, first as `GAME_MAP`).
pub fn game_maps(files: &dyn Files, game_dir: &str) -> Vec<String> {
    let mut folders: Vec<String> = files.list("terrain/battles/", "/tile_map.tiles").into_iter()
        .filter_map(|p| p.strip_suffix("/tile_map.tiles").map(str::to_owned))
        .collect();
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
        let index = read("tile_map.index")?;
        let tiles = read("tile_map.tiles")?;
        let lf = read("lf_height_map.dds").ok();
        let map = load_game_map(name, db, &index, &tiles, lf.as_deref())?;
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

/// Adds entities to a layer in a row (they can't be picked): their ids.
fn add_range(scene: &mut RenderScene, layer: LayerId, doc: DocumentId, entities: Vec<(Float4x4, Payload)>) -> std::ops::Range<EntityId> {
    let first = scene.next_id();
    for (world, payload) in entities {
        let id = scene.add(layer, world, payload, SourceRef { doc, item: u64::MAX });
        if let Some(en) = scene.find_mut(id) {
            en.locked = true;
        }
    }

    first..scene.next_id()
}

impl ContextUi {

    /// The maps to choose from for a kit.
    pub fn refresh_maps(&mut self, raw: &Path, files: Option<&dyn Files>) {
        if self.maps_for == raw && !self.maps.is_empty() {
            return;
        }

        self.maps_for = raw.to_path_buf();
        self.maps.clear();
        if let Some(f) = files {
            self.maps.extend(game_maps(f, self.game_dir));
        }

        self.maps.extend(kit_maps(raw));
        self.map = self.map.clamp(0, (self.maps.len() as i32 - 1).max(0));
    }

    /// Starts reading the selected map.
    pub fn start_map(&mut self, raw: &Path, files: Option<Arc<dyn Files>>) {
        let Some(name) = self.maps.get(self.map as usize).cloned() else { return };
        let db = match database(self, raw) {
            Ok(db) => db,
            Err(e) => {
                self.status = e;
                return;
            }
        };

        let (tx, rx) = channel();
        let raw = raw.to_path_buf();
        let game_dir = self.game_dir;
        let packs = self.packs_tree.clone().filter(|_| game_map_folder(&name, game_dir).is_some());
        self.busy = format!("reading {name}...");
        self.rx = Some(rx);
        self.build_look = None;
        std::thread::spawn(move || {
            // A map of the packs names the packs' tiles (a mod's among them): read with their database.
            let db = packs.and_then(|p| p.raw().ok()).and_then(|r| TileDatabase::load(&r.join("terrain/tiles/battle/_tile_database")).ok()).map_or(db, Arc::new);
            let r = load_map(&name, game_dir, &raw, &db, files.as_deref());
            let _ = tx.send(Job::Map(name, r));
        });
    }

    /// Shows a map made for the moment (a link candidate next to the open tile, `links_editor`) in the context, the open tile at
    /// placement `open`.
    pub fn show_map(&mut self, e: &mut Editor, id: DocumentId, map: PlacedMap, open: usize) {
        self.shown = true;
        self.status = format!("{}: {} placements", map.name, map.placements.len());
        self.loaded = Some((map.name.clone(), Arc::new(map), None));
        self.placements = vec![open];
        self.choice = 0;
        self.clear(e);
        self.rebuild(e, id);
    }

    /// Where the open tile goes: the chosen placement, else the manual cell.
    pub fn spot(&self) -> Option<Spot> {
        let (_, map, _) = self.loaded.as_ref()?;
        match self.placements.get(self.choice) {
            Some(&i) => {
                let p = &map.placements[i];
                Some(Spot { x: p.x, y: p.y, rotation: p.rotation })
            }

            None => Some(Spot { x: self.manual[0], y: self.manual[1], rotation: [0x10, 0x20, 0x40, 0x80][self.manual[2].clamp(0, 3) as usize] }),
        }
    }

    /// The climate of the chosen placement in the map (the region's: what the game draws the tile with); none for a cell entered by hand.
    pub fn spot_climate(&self) -> Option<String> {
        let (_, map, _) = self.loaded.as_ref()?;
        let &i = self.placements.get(self.choice)?;
        map.climate_of(&map.placements[i]).map(str::to_owned)
    }

    /// The open tile takes its placement's climate (when the tile's climates have it): the ground and the context's plain tiles then
    /// match the map's region instead of the editor's default.
    pub fn adopt_climate(&self, e: &mut Editor, id: DocumentId) {
        let Some(climate) = self.spot_climate() else { return };
        let Some(d) = e.docs.find_as_mut::<TileDocument>(id) else { return };
        let mut look = d.tile().terrain_look().clone();
        if look.climate == climate || d.tile().climates().find(&climate).is_none() {
            return;
        }

        look.climate = climate.clone();
        d.tile_mut().set_terrain_look(&mut e.scene, look);
        log::info(format!("map context: the tile's ground is its placement's climate, {climate}"));
    }

    /// The `raw_data` the context reads (the tile database, the kit's battle maps, the neighbours' folders): the neighbours' tree when
    /// there is one, else the open tile's kit.
    pub fn raw_of(&self, d: &TileDocument) -> Option<PathBuf> {
        self.neighbours.as_ref().map(|(raw, _)| raw.clone()).or_else(|| d.project().raw_root.clone())
    }

    /// The request for the open tile, `None` without a map or a kit.
    pub fn request(&self, d: &TileDocument) -> Option<ContextRequest> {
        let (_, map, _) = self.loaded.as_ref()?;
        let p = d.project();
        let raw = self.raw_of(d)?;
        Some(ContextRequest {
            map: map.clone(), spot: self.spot()?, cells: (p.info.cells_x.max(1) as i32, p.info.cells_y.max(1) as i32), unit: p.unit_scale, near: self.near,
            mid: self.mid, far: self.far_ring(), raw_root: raw, tile_roots: self.tile_roots.clone(), game_kit: p.game_kit.clone(), look: d.tile().terrain_look().clone(), trees: self.trees, plain: d.tile().ground_material_base(),
            groups: p.entry.as_ref().map(|e| e.texture_group()),
            cache: Some(cache::cache_dir()),
            provider: self.neighbours.as_ref().map(|(_, provide)| provide.clone()).or_else(|| self.packs_fallback.clone()),
        })
    }

    /// Starts streaming the context of the open tile (its frame is made on a thread first).
    pub fn start_build(&mut self, rq: ContextRequest, files: Option<Arc<dyn Files>>) {
        self.build += 1;
        let n = self.build;
        let (tx, rx) = channel();
        self.busy = "reading the map's ground...".to_owned();
        self.build_look = Some(rq.look.clone());
        self.rx = Some(rx);
        std::thread::spawn(move || {
            let s = Streamer::new(rq, files);
            let _ = tx.send(Job::Built(n, Box::new(s)));
        });
    }

    /// The placements of the open tile in the loaded map.
    pub fn find_placements(&mut self, d: &TileDocument) {
        let Some((_, map, _)) = &self.loaded else { return };
        let info = &d.project().info;
        self.placements = map.find(&info.location, &info.variation);
        self.choice = 0;
        if self.placements.is_empty() {
            // Another variation of the tile is placed: its spot (the variations share the tile's size and links); else the map's centre.
            self.manual = match map.find_tile(&info.location).first().map(|&i| &map.placements[i]) {
                Some(p) => [p.x, p.y, match p.rotation { 0x20 => 1, 0x40 => 2, 0x80 => 3, _ => 0 }],
                None => [(map.width / 2) as i32, (map.height / 2) as i32, 0],
            };
        }
    }

    /// Puts a finished job's result in place, and what the streamer finished on the scene.
    pub fn poll(&mut self, e: &mut Editor, id: DocumentId) -> bool {
        let mut changed = false;
        if let Some(rx) = &self.rx
            && let Ok(job) = rx.try_recv()
        {
            self.rx = None;
            self.build_look = None;
            self.busy.clear();
            changed = true;
            match job {
                Job::Map(name, Ok((map, locations))) => {
                    self.status = format!("{name}: {} x {} cells, {} placements", map.width, map.height, map.placements.len());
                    self.loaded = Some((name, map, locations));
                    if let Some(d) = e.docs.find_as::<TileDocument>(id) {
                        self.find_placements(d);
                    }

                    self.rebuild(e, id);
                }

                Job::Map(name, Err(err)) => self.status = format!("{name}: {err}"),
                Job::Built(n, s) => {
                    if n == self.build && self.shown {
                        self.begin(e, id, s);
                    }
                }
            }
        }

        if self.streamer.is_some() && self.doc == id {
            changed |= self.stream(e);
        }

        changed
    }

    /// A new streamer on the scene: the layer (emptied), the sea, the open tile clipped to its cells and drawn on the map's LF.
    pub fn begin(&mut self, e: &mut Editor, id: DocumentId, s: Box<Streamer>) {
        if self.layer == 0 || self.doc != id || e.scene.layer(self.layer).is_none() {
            if self.layer != 0 {
                e.scene.remove_layer(self.layer);
            }

            self.layer = e.scene.add_layer(id, wce_scene::render_scene::CONTEXT_LAYER);
            self.doc = id;
        } else {
            e.scene.clear_layer(self.layer);
        }

        if let Some(l) = e.scene.layer_mut(self.layer) {
            l.locked = true;
        }

        self.chunks.clear();
        self.waiting.clear();
        self.gone.clear();
        let lf = s.frame().lf.clone();

        // The sea is at 0 m: in the open tile's frame, below it by the median LF under it (the shaders measure from a little above it).
        e.scene.sea_level = s.frame().bias_y();
        log::info(format!("map context: {}", s.frame().lf_note()));
        self.streamer = Some(s);
        let at = self.focus(e).unwrap_or_default();
        let frame = self.streamer.as_ref().expect("set").frame().clone();
        let deepest = e.docs.find_as::<TileDocument>(id).and_then(|d| d.project().height.as_ref().map(|h| h.min_height())).unwrap_or(0.0);
        let sea = add_range(&mut e.scene, self.layer, id, std::iter::once(frame.sea(at, self.sea_reach())).chain(frame.open_floor(deepest)).collect());
        (self.sea, self.sea_at) = (Some(sea.start), at);

        // The open tile's height map border would lie over the neighbours; its ground goes on the LF like theirs.
        if let Some(d) = e.docs.find_as_mut::<TileDocument>(id) {
            d.tile_mut().set_clip_to_tile(&mut e.scene, true);
            d.tile_mut().set_context_lf(&mut e.scene, lf);
            let sea = d.tile().display().show_sea;
            self.show_sea(e, sea);
        }

        self.stream(e);
    }

    /// The focus of the rings: what the camera looks at (the orbit's target, the fly camera's eye), or the tile.
    fn focus(&self, e: &Editor) -> Option<(f64, f64)> {
        let s = self.streamer.as_ref()?;
        if !self.follow {
            return Some(s.open_centre());
        }

        let p = e.viewport.pivot();
        Some((f64::from(p.x), f64::from(p.z)))
    }

    /// The far ring: the slider's, or what the camera sees.
    fn far_ring(&self) -> i32 {
        self.far.max(self.far_view)
    }

    /// The cells around the focus the sea covers: the far ring and some.
    fn sea_reach(&self) -> i32 {
        self.far_ring().max(self.mid).max(self.near) + 4 * CHUNK
    }

    /// The cells around the focus the camera sees to: its far plane from its eye, which is off the focus (the orbit's target) by its
    /// distance, in whole chunks.
    fn view_cells(e: &Editor, focus: (f64, f64)) -> i32 {
        let cam = e.viewport.camera();
        let off = (f64::from(cam.eye.x) - focus.0).hypot(f64::from(cam.eye.z) - focus.1);
        let cells = ((f64::from(cam.zfar) + off) / CELL).ceil() as i32;
        (cells + CHUNK - 1) / CHUNK * CHUNK
    }

    /// The far ring follows what the camera sees from the focus; whether it changed.
    fn follow_view(&mut self, e: &Editor, focus: (f64, f64)) -> bool {
        let view = Self::view_cells(e, focus);
        if view == self.far_view {
            return false;
        }

        self.far_view = view;
        let (near, mid, far) = (self.near, self.mid, self.far_ring());
        if let Some(s) = self.streamer.as_mut() {
            s.set_rings(near, mid, far);
        }

        true
    }

    /// The map's folder in the kit (its season maps).
    fn map_dir(&self, raw: &Path) -> Option<PathBuf> {
        let (name, _, _) = self.loaded.as_ref()?;
        let folder = game_map_folder(name, self.game_dir).map_or_else(|| name.clone(), |f| f.rsplit('/').next().unwrap_or_default().to_owned());
        Some(raw.join("terrain/battles").join(folder))
    }

    /// The scene's snow regions for the open tile's season (the map's snow map of it), made again when the season, the switch or the
    /// amount change; and a new context when the tile's look is no longer the one the context was made with (its tiles' textures).
    fn follow_look(&mut self, e: &mut Editor) {
        let Some(d) = e.docs.find_as::<TileDocument>(self.doc) else { return };
        let look = d.tile().terrain_look().clone();
        let raw = self.raw_of(d);
        let Some(s) = self.streamer.as_ref() else { return };
        if s.frame().look != look {
            // One build per look: asked again every frame, a build slower than a frame never landed and the threads piled up.
            if self.rx.is_none() || self.build_look.as_ref() != Some(&look) {
                let doc = self.doc;
                self.rebuild(e, doc);
            }

            return;
        }

        let key = (look.season.clone(), self.snow_on, self.snow_amount.to_bits());
        if self.snow_key.as_ref() == Some(&key) {
            return;
        }

        self.snow_key = Some(key);
        let dir = raw.as_deref().and_then(|r| self.map_dir(r));
        let regions = match (self.snow_on, dir) {
            (true, Some(dir)) => {
                if self.snow_map.as_ref().is_none_or(|(d, season, _)| *d != dir || *season != look.season) {
                    let cells = wce_tile::battle_map::load_snow_map(&dir, &look.season).map(Arc::new);
                    self.snow_map = Some((dir, look.season.clone(), cells));
                }

                let to_cell = s.frame().to_map.m.map(|row| row.map(|v| v as f32));
                self.snow_map.as_ref().and_then(|(_, _, c)| c.clone()).map(|c| SnowRegions {
                    to_cell, w: c.0, h: c.1, cells: Arc::new(c.2.clone()), amount: self.snow_amount,
                })
            }

            _ => None,
        };

        e.scene.set_snow(regions);
    }

    /// One update of the streamer: the chunks it finished replace theirs on the layer, the ones it dropped go (one pass over the scene); the
    /// sea follows the focus.
    pub fn stream(&mut self, e: &mut Editor) -> bool {
        wce_prof!("context.stream");
        e.viewport.set_far_plane(self.view_distance.max(GAME_FAR_PLANE));
        self.follow_look(e);
        let Some(focus) = self.focus(e) else { return false };
        let widened = self.follow_view(e, focus);
        let reach = self.sea_reach();
        let Some(s) = self.streamer.as_mut() else { return false };
        let changes = s.update(focus);
        self.status = s.report();
        let (dx, dz) = (focus.0 - self.sea_at.0, focus.1 - self.sea_at.1);
        let moved = (dx * dx + dz * dz).sqrt() > f64::from(2 * CHUNK) * CELL || widened;
        if let (Some(id), true) = (self.sea, moved) {
            let (_, sea) = s.frame().sea(focus, reach);
            e.scene.update_payload(id, sea);
            self.sea_at = focus;
        }

        self.take(changes);
        self.apply(e, Some(APPLY_BUDGET)) || moved
    }

    /// Waits for every chunk around the focus and puts them on the scene (scripts).
    pub fn settle(&mut self, e: &mut Editor) {
        let Some(focus) = self.focus(e) else { return };
        self.follow_view(e, focus);
        let Some(s) = self.streamer.as_mut() else { return };
        let changes = s.run_to_idle(focus);
        self.status = s.report();
        self.take(changes);

        // The renderer makes their terrains' GPU side on its loading threads.
        let t0 = std::time::Instant::now();
        while !self.waiting.is_empty() && t0.elapsed().as_secs() < 60 {
            if !self.apply(e, None) {
                std::thread::sleep(std::time::Duration::from_millis(5));
            }
        }
    }

    /// The streamer's changes: the chunks it removed go now (with what was waiting of them), the ones it built wait for their terrains.
    fn take(&mut self, changes: Vec<ChunkChange>) {
        for c in changes {
            match c {
                ChunkChange::Set(k, _, entities) => {
                    self.waiting.retain(|(w, _)| *w != k);
                    self.waiting.push((k, entities));
                }

                ChunkChange::Remove(k) => {
                    self.waiting.retain(|(w, _)| *w != k);
                    self.gone.push(k);
                }
            }
        }
    }

    /// Puts on the layer the waiting chunks whose terrains the renderer has ready (the version each replaces stays until then), in the
    /// order they came, until `budget` is spent (one at least), and takes off the removed ones. Whether anything changed.
    fn apply(&mut self, e: &mut Editor, budget: Option<std::time::Duration>) -> bool {
        wce_prof!("context.apply");
        let t0 = std::time::Instant::now();
        let mut gone: Vec<std::ops::Range<EntityId>> = std::mem::take(&mut self.gone).into_iter().filter_map(|k| self.chunks.remove(&k)).collect();
        let mut ready = vec![];
        let mut k = 0;
        while k < self.waiting.len() {
            let terrains_ready = e.renderer.as_mut().is_none_or(|r| self.waiting[k].1.iter().all(|(_, p)| match p {
                Payload::Terrain(t) => r.terrain_ready(t),
                _ => true,
            }));

            if terrains_ready {
                let (key, entities) = self.waiting.remove(k);
                gone.extend(self.chunks.remove(&key));
                ready.push((key, entities));
                if budget.is_some_and(|b| t0.elapsed() > b) {
                    break;
                }
            } else {
                k += 1;
            }
        }

        if gone.is_empty() && ready.is_empty() {
            return false;
        }

        let scene = &mut e.scene;
        scene.remove_ranges(&gone);
        scene.reserve(ready.iter().map(|(_, v)| v.len()).sum());
        for (key, entities) in ready {
            let ids = add_range(scene, self.layer, self.doc, entities);
            self.chunks.insert(key, ids);
        }

        true
    }

    /// Where the scene sits in the map for the coordinates readout (cells, height above the sea), while a context is shown.
    pub fn map_frame(&self) -> Option<wce_editor::coords::MapFrame> {
        let f = self.streamer.as_ref()?.frame();
        Some(wce_editor::coords::MapFrame { to_map: f.to_map.m, sea_y: f.sea_y(), metres: f.unit })
    }

    /// Where north is in the world for the compass: the map's while a context is shown, else the tile's own +z (its rows' top edge).
    pub fn north(&self) -> [f32; 2] {
        self.streamer.as_ref().map_or([0.0, 1.0], |s| s.frame().north())
    }

    /// Shows or hides the context's sea (the tile's sea toggle).
    pub fn show_sea(&self, e: &mut Editor, on: bool) {
        let Some(id) = self.sea else { return };
        if let Some(en) = e.scene.find_mut(id)
            && en.visible != on
        {
            en.visible = on;
            e.scene.touch();
        }
    }

    /// Rebuilds for the current choices (when shown).
    pub fn rebuild(&mut self, e: &mut Editor, id: DocumentId) {
        if !self.shown {
            return;
        }

        self.adopt_climate(e, id);
        let Some(d) = e.docs.find_as::<TileDocument>(id) else { return };
        if let Some(rq) = self.request(d) {
            self.start_build(rq, e.files());
        }
    }

    /// Takes the context off the scene (and stops its streamer).
    pub fn clear(&mut self, e: &mut Editor) {
        e.viewport.set_far_plane(GAME_FAR_PLANE);
        if self.layer != 0 {
            e.scene.remove_layer(self.layer);
            self.layer = 0;
        }

        if let Some(d) = e.docs.find_as_mut::<TileDocument>(self.doc) {
            d.tile_mut().set_clip_to_tile(&mut e.scene, false);
            d.tile_mut().set_context_lf(&mut e.scene, None);
        }

        self.streamer = None;
        e.scene.sea_level = 0.0;
        e.scene.set_snow(None);
        self.snow_key = None;
        self.chunks.clear();
        self.waiting.clear();
        self.gone.clear();
        self.sea = None;
        self.build += 1;
        self.build_look = None;
    }

    /// A new tile was opened: drop the old tile's context, find the new one in the loaded map.
    pub fn tile_changed(&mut self, e: &mut Editor, id: DocumentId) {
        self.clear(e);
        if let Some(d) = e.docs.find_as::<TileDocument>(id) {
            self.find_placements(d);
        }

        self.rebuild(e, id);
    }

    pub fn draw(&mut self, e: &mut Editor, id: DocumentId) {
        if !gui::collapsing_header("Map context", 0) {
            return;
        }

        // Its own id scope: the Tile panel's weather section has a "map" combo too (ImGui reported the id twice).
        gui::push_id("map context");
        self.draw_section(e, id);
        gui::pop_id();
    }

    fn draw_section(&mut self, e: &mut Editor, id: DocumentId) {
        let Some(d) = e.docs.find_as::<TileDocument>(id) else { return };
        let Some(raw) = self.raw_of(d) else {
            gui::text_disabled("the tile is not in an assembly kit (no neighbours to open)");
            return;
        };

        let files = e.files();
        self.game_dir = e.game().campaign_battle_map;
        self.refresh_maps(&raw, files.as_deref());
        let mut changed = false;
        if gui::checkbox("show the tiles around it", &mut self.shown) {
            if self.shown { changed = true } else { self.clear(e) }
        }

        let names: Vec<&str> = self.maps.iter().map(String::as_str).collect();
        if gui::combo("map", &mut self.map, &names) || (self.shown && self.loaded.is_none() && self.rx.is_none() && !names.is_empty()) {
            self.loaded = None;
            self.start_map(&raw, files.clone());
        }

        if let Some((name, map, locations)) = &self.loaded {
            let d = e.docs.find_as::<TileDocument>(id).expect("a tile");
            let info = &d.project().info;
            gui::text_disabled(&format!("{name}: {} x {} cells", map.width, map.height));
            let label = |i: usize| {
                let p = &map.placements[i];
                let at = location_names(locations.as_ref(), p.x, p.y);
                format!("cell ({}, {}), {} deg{}", p.x, p.y, match p.rotation { 0x20 => 90, 0x40 => 180, 0x80 => 270, _ => 0 }, if at.is_empty() { String::new() } else { format!(" - {at}") })
            };

            let preview = if self.choice < self.placements.len() { label(self.placements[self.choice]) } else { "a cell of my choice".to_owned() };
            if self.placements.is_empty() {
                let other = map.find_tile(&info.location).first().map(|&i| map.placements[i].variation.clone());
                gui::text_wrapped(&match other {
                    Some(v) => format!("{}\\{} is not placed in this map: shown where its variation {v} is (or put it at another cell).", info.location, info.variation),
                    None => format!("{}\\{} is not placed in this map: put it at a cell.", info.location, info.variation),
                });
            }

            if gui::begin_combo("placement", &preview) {
                for k in 0..=self.placements.len() {
                    let text = if k < self.placements.len() { label(self.placements[k]) } else { "a cell of my choice".to_owned() };
                    if gui::selectable(&format!("{text}##{k}"), k == self.choice, 0) && k != self.choice {
                        self.choice = k;
                        changed = true;
                    }
                }

                gui::end_combo();
            }

            if self.choice >= self.placements.len() {
                changed |= gui::input_int("cell x", &mut self.manual[0]);
                changed |= gui::input_int("cell y", &mut self.manual[1]);
                changed |= gui::combo("rotation", &mut self.manual[2], &["0", "90", "180", "270"]);
            }
        }

        // The rings change the streamer's levels; the rest builds a new one.
        let mut rings = gui::slider_int("full detail (cells)", &mut self.near, 1, 64);
        rings |= gui::slider_int("ground and buildings (cells)", &mut self.mid, 1, 128);
        rings |= gui::slider_int("LF ground (cells)", &mut self.far, 1, 512);
        if let Some(s) = self.streamer.as_mut().filter(|_| rings) {
            s.set_rings(self.near, self.mid, self.far.max(self.far_view));
        }

        if self.far_view > self.far {
            gui::text_disabled(&format!("LF ground to the camera's view: {} cells", self.far_view));
        }

        // How far the camera sees while the context shows (the far ring follows it); the whole map at most.
        let whole = self.loaded.as_ref().map_or(GAME_FAR_PLANE * 4.0, |(_, m, _)| (m.width.max(m.height) as f32 * CELL as f32).max(GAME_FAR_PLANE));
        gui::slider_float("view distance", &mut self.view_distance, GAME_FAR_PLANE, whole, "%.0f");
        gui::checkbox("overview map", &mut self.minimap);
        if self.view_distance > GAME_FAR_PLANE * 2.0 {
            gui::text_disabled("far views draw many chunks: slower frames");
        }

        changed |= gui::checkbox("their trees", &mut self.trees);
        gui::checkbox("follow the camera", &mut self.follow);

        // The tiles around are drawn in the climates the map gives them; the open tile in the one its panel has.
        let map_climate = self.loaded.as_ref().zip(self.placements.get(self.choice)).and_then(|((_, map, _), &i)| map.climate_of(&map.placements[i]).map(str::to_owned));
        if let (Some(climate), Some(d)) = (map_climate, e.docs.find_as_mut::<TileDocument>(id)) {
            let look = d.tile().terrain_look().clone();
            if look.climate.eq_ignore_ascii_case(&climate) {
                gui::text_disabled(&format!("the map's climate here: {climate}"));
            } else {
                gui::text_disabled(&format!("the map's climate here: {climate} (the tile: {})", look.climate));
                gui::same_line();
                if gui::small_button("use it") {
                    d.tile_mut().set_terrain_look(&mut e.scene, TerrainLook { climate, ..look });
                    changed = true;
                }
            }
        }

        // The season's snow where the map has it (winter on the campaign map): the ground, buildings, decals and trees of those cells.
        gui::checkbox("snow where the map says", &mut self.snow_on);
        if self.snow_on {
            gui::slider_float("snow amount", &mut self.snow_amount, 0.05, 1.0, "%.2f");
            let season = e.docs.find_as::<TileDocument>(id).map(|d| d.tile().terrain_look().season.clone()).unwrap_or_default();
            match &self.snow_map {
                Some((_, s, Some(c))) if *s == season => gui::text_disabled(&format!("{} snowy cells this season", c.2.iter().filter(|&&v| v != 0).count())),
                Some((_, s, None)) if *s == season => gui::text_disabled("no snow on the map this season"),
                _ => {}
            }
        }

        if changed && self.loaded.is_some() {
            self.rebuild(e, id);
        }

        if !self.busy.is_empty() {
            gui::text_disabled(&self.busy);
        }

        if !self.status.is_empty() {
            gui::text_wrapped(&self.status);
        }
    }
}

impl ContextUi {

    /// The overview map over the viewport (top right, under the camera chips): the loaded map's LF, the open tile's placement, the camera and where it looks; a click
    /// (or a drag) moves the camera there, keeping its height and angle.
    pub fn draw_minimap(&mut self, e: &mut Editor, origin: gui::Vec2, size: gui::Vec2) {
        if !self.minimap || !self.shown {
            return;
        }

        let Some((name, map, _)) = self.loaded.clone() else { return };
        let Some(frame) = self.streamer.as_ref().map(|s| s.frame().clone()) else { return };
        let Some(lf) = map.lf.as_ref().filter(|f| f.w > 1 && f.h > 1) else { return };
        if self.minimap_of != name {
            let (w, h, px) = minimap_pixels(lf, MINIMAP_SIDE);
            e.images.forget(MINIMAP_KEY);
            e.images.submit(MINIMAP_KEY, w, h, px);
            self.minimap_of = name;
        }

        let Some(img) = e.images.get(MINIMAP_KEY) else { return };

        // About a third of the image's width (the whole map's shape), under the camera chips (the coordinates are bottom left).
        let mw = (size.x * 0.3).clamp(160.0, 420.0);
        let mh = mw * img.h as f32 / img.w.max(1) as f32;
        let top = e.viewport.chips_top + 44.0;
        if mh > size.y - top - 120.0 || size.x < 400.0 {
            return;
        }

        let pos = gui::v2(origin.x + size.x - mw - 10.0, origin.y + top);
        gui::set_cursor_screen_pos(pos);
        gui::push_style_color(gui::COL_CHILD_BG, gui::v4(0.0, 0.0, 0.0, 0.6));
        if gui::begin_child("##minimap", gui::v2(mw, mh), 0, gui::WINDOW_NO_SCROLLBAR) {
            let dl = gui::window_draw_list();
            let (x0, y0) = (pos.x, pos.y);
            dl.image(img.id, gui::v2(x0, y0), gui::v2(x0 + mw, y0 + mh), gui::v2(0.0, 0.0), gui::v2(1.0, 1.0), gui::col32(255, 255, 255, 255));
            dl.rect(gui::v2(x0, y0), gui::v2(x0 + mw, y0 + mh), gui::col32(0, 0, 0, 200), 0.0, 1.0);

            // Map cells to the picture.
            let (cw, ch) = (map.width.max(1) as f32, map.height.max(1) as f32);
            let at = |cx: f64, cy: f64| gui::v2(x0 + cx as f32 / cw * mw, y0 + cy as f32 / ch * mh);

            // The open tile's placement.
            let o = frame.open;
            let (a, b) = (at(f64::from(o[0]), f64::from(o[1])), at(f64::from(o[2]), f64::from(o[3])));
            dl.rect(gui::v2(a.x - 1.0, a.y - 1.0), gui::v2(b.x.max(a.x + 2.0) + 1.0, b.y.max(a.y + 2.0) + 1.0), gui::col32(255, 210, 60, 255), 0.0, 2.0);

            // The camera and where it looks (the view distance's reach).
            let cam = e.viewport.camera();
            let (ex, ey) = frame.cell_of(f64::from(cam.eye.x), f64::from(cam.eye.z));
            let (tx, ty) = frame.cell_of(f64::from(cam.target.x), f64::from(cam.target.z));
            let (dx, dy) = (tx - ex, ty - ey);
            let l = dx.hypot(dy).max(1e-9);
            let reach = f64::from(cam.zfar) / CELL;
            let eye = at(ex, ey);
            let half = (f64::from(cam.fov_y) * 0.75).min(1.2);
            let side = |s: f64| {
                let (c, n) = (half.cos(), s * half.sin());
                let (rx, ry) = ((dx * c - dy * n) / l, (dx * n + dy * c) / l);
                at(ex + rx * reach, ey + ry * reach)
            };

            dl.triangle_filled(eye, side(-1.0), side(1.0), gui::col32(255, 255, 255, 40));
            dl.line(eye, side(-1.0), gui::col32(255, 255, 255, 140), 1.0);
            dl.line(eye, side(1.0), gui::col32(255, 255, 255, 140), 1.0);
            dl.circle_filled(eye, 4.0, gui::col32(230, 60, 50, 255));

            // A click or a drag: the camera over that cell.
            gui::set_cursor_screen_pos(pos);
            gui::invisible_button("##minimap_click", gui::v2(mw, mh), 0);
            if gui::is_item_active() {
                let m = gui::mouse_pos();
                let (cx, cy) = (f64::from((m.x - x0) / mw * cw), f64::from((m.y - y0) / mh * ch));
                let (fx, fz) = frame.from_map.apply(cx, cy);
                let (fx, fz) = (fx as f32, fz as f32);
                let d = wce_foundation::math::Float3::new(fx - cam.target.x, 0.0, fz - cam.target.z);
                let to = wce_scene::camera::Camera { eye: cam.eye + d, target: cam.target + d, ..cam };
                e.viewport.controller().set_camera(&to);
            }

            if gui::is_item_hovered() {
                gui::set_tooltip(&format!("{}: click to move the camera there", self.minimap_of));
            }
        }

        gui::end_child();
        gui::pop_style_color(1);
    }
}
