//! The Map context section of the Tile panel: the open tile among its neighbours in a battle map (wce_tile_editor's `context`). The maps are
//! the game's campaign battle map (terrain.pack's `terrain/battles/main_attila_map`, as the game ships it) and the kit's battle maps (filled
//! as BOB does). Where the tile is placed more than once (the settlement tiles are reused across settlements), the user picks the placement;
//! where it is placed nowhere, a cell and a rotation. The context streams in chunks around the camera (or the tile): reading the map and its
//! frame runs on a thread, the chunks on the streamer's workers; every frame puts what they finished on the scene.


use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::mpsc::{Receiver, channel};

use wce_editor::editor::Editor;
use wce_editor::gui;
use wce_foundation::log;
use wce_map_view::{MapView, Rings};
use wce_scene::render_scene::DocumentId;
use wce_tile::battle_map::context::{CELL, PlacedMap, Spot};
use wce_tile::battle_map::locations::BattleLocations;
use wce_tile::battle_map::tile_db::TileDatabase;
use wce_tile::battle_map::kit_maps;
use wce_tile_editor::terrain_bake::TerrainLook;
use wce_tile_editor::tile_document::TileDocument;
use wce_tile_editor::tile_document::context::{ContextRequest, Streamer, TileProvider, cache};
use wce_vfs::files::Files;

pub use wce_map_view::maps::{GAME_FAR_PLANE, GAME_FILES, GAME_MAP, PacksTree, game_map_folder, game_maps, load_map, location_names};

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

    /// The rings around the focus (cells): everything, the buildings, the tiles' coarse ground, the LF ground.
    pub near: i32,
    pub mid: i32,
    pub ground: i32,
    pub far: i32,

    pub trees: bool,

    /// The camera's view distance while the context shows (scene units; the game's battle camera sees 13 000), and whether the minimap
    /// shows over the viewport.
    pub view_distance: f32,
    pub minimap: bool,


    /// The rings follow the camera (else they stay around the tile).
    pub follow: bool,
    pub status: String,
    pub busy: String,
    rx: Option<Receiver<Job>>,

    /// The map on the scene (its streamer, layer and chunks), the build's number (a newer request wins).
    pub view: MapView,
    pub build: u64,

    /// The look the build on its way was asked with: a look that changes again meanwhile starts another, the same one waits for it.
    build_look: Option<TerrainLook>,

    db: Option<(Vec<PathBuf>, Arc<TileDatabase>)>,

    /// The variation of the open tile the map has, when it hasn't the open one (`find_placements`): looking for it goes over the whole map,
    /// too slow for each frame the section shows.
    placed_variation: Option<String>,
    maps_for: PathBuf,

    /// The season's snow where the map has it (its `snow_map_<season>.png`), how much.
    pub snow_on: bool,
    pub snow_amount: f32,
}

impl Default for ContextUi {
    fn default() -> Self {
        Self {
            game_dir: wce_vfs::GameProfile::ATTILA.campaign_battle_map, neighbours: None, packs_fallback: None, packs_tree: None, tile_roots: vec![], tile_roots_time: None, shown: false, maps: vec![], map: 0, loaded: None, placements: vec![], choice: 0, manual: [0, 0, 0], near: 8, mid: 24, ground: 24, far: 64, trees: true, view_distance: GAME_FAR_PLANE, minimap: true,
            follow: true, placed_variation: None, status: String::new(), busy: String::new(), rx: None, view: MapView::default(), build: 0, build_look: None, db: None, maps_for: PathBuf::new(), snow_on: true,
            snow_amount: 1.0,
        }
    }
}

/// The tile database of `raw` over those of the tile roots (the projects' and the kits') and of the packs, the game's and the mods' (read
/// the first time): a project alone holds only its own few tiles.
pub(crate) fn database(ui: &mut ContextUi, raw: &Path) -> Result<Arc<TileDatabase>, String> {
    let mut roots = vec![raw.to_path_buf()];
    roots.extend(ui.tile_roots.iter().filter(|r| r.as_path() != raw).cloned());
    roots.extend(ui.packs_tree.as_ref().and_then(|p| p.raw().ok()).filter(|r| !roots.contains(r)));
    if let Some((r, db)) = &ui.db
        && *r == roots
    {
        return Ok(db.clone());
    }

    let folders = roots.iter().map(|r| r.join("terrain/tiles/battle/_tile_database")).collect::<Vec<_>>();
    let db = Arc::new(TileDatabase::load_layered(&folders.iter().map(PathBuf::as_path).collect::<Vec<_>>())?);
    ui.db = Some((roots, db.clone()));
    Ok(db)
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
            self.maps.extend(game_maps(f, self.game_dir, false));
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
            mid: self.mid, ground: self.ground, far: self.far_ring(), raw_root: raw, tile_roots: self.tile_roots.clone(), game_kit: p.game_kit.clone(), look: d.tile().terrain_look().clone(), trees: self.trees, plain: d.tile().ground_material_base(),
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
        self.placed_variation = None;
        if self.placements.is_empty() {
            // Another variation of the tile is placed: its spot (the variations share the tile's size and links); else the map's centre.
            let other = map.find_tile(&info.location).first().map(|&i| &map.placements[i]);
            self.placed_variation = other.map(|p| p.variation.clone());
            self.manual = match other {
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

        if self.view.streamer.is_some() && self.view.doc() == id {
            changed |= self.stream(e);
        }

        changed
    }


    /// The rings and camera settings of the panel.
    fn rings(&self) -> Rings {
        Rings { near: self.near, mid: self.mid, ground: self.ground, far: self.far, view_distance: self.view_distance, follow: self.follow }
    }

    /// A new streamer on the scene: the layer (emptied), the sea, the open tile clipped to its cells and drawn on the map's LF.
    pub fn begin(&mut self, e: &mut Editor, id: DocumentId, s: Box<Streamer>) {
        let lf = s.frame().lf.clone();
        let deepest = e.docs.find_as::<TileDocument>(id).and_then(|d| d.project().height.as_ref().map(|h| h.min_height())).unwrap_or(0.0);
        let rings = self.rings();
        self.view.begin(e, id, s, &rings, |frame| frame.open_floor(deepest).into_iter().collect());

        // The open tile's height map border would lie over the neighbours; its ground goes on the LF like theirs.
        if let Some(d) = e.docs.find_as_mut::<TileDocument>(id) {
            d.tile_mut().set_clip_to_tile(&mut e.scene, true);
            d.tile_mut().set_context_lf(&mut e.scene, lf);
            let sea = d.tile().display().show_sea;
            self.show_sea(e, sea);
        }

        self.stream(e);
    }

    /// The far ring: the slider's, or what the camera sees.
    fn far_ring(&self) -> i32 {
        self.view.far_ring(self.far)
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
        let Some(d) = e.docs.find_as::<TileDocument>(self.view.doc()) else { return };
        let look = d.tile().terrain_look().clone();
        let raw = self.raw_of(d);
        let Some(frame) = self.view.frame() else { return };
        if frame.look != look {
            // One build per look: asked again every frame, a build slower than a frame never landed and the threads piled up.
            if self.rx.is_none() || self.build_look.as_ref() != Some(&look) {
                let doc = self.view.doc();
                self.rebuild(e, doc);
            }

            return;
        }

        let dir = raw.as_deref().and_then(|r| self.map_dir(r));
        let (on, amount) = (self.snow_on, self.snow_amount);
        self.view.snow(e, dir, &look.season, on, amount);
    }

    /// One update of the streamer: the chunks it finished replace theirs on the layer, the ones it dropped go; the sea follows the focus.
    pub fn stream(&mut self, e: &mut Editor) -> bool {
        e.viewport.set_far_plane(self.view_distance.max(GAME_FAR_PLANE));
        self.follow_look(e);
        let rings = self.rings();
        let changed = self.view.stream(e, &rings);
        self.status.clone_from(&self.view.status);
        changed
    }

    /// Waits for every chunk around the focus and puts them on the scene (scripts).
    pub fn settle(&mut self, e: &mut Editor) {
        let rings = self.rings();
        self.view.settle(e, &rings);
        self.status.clone_from(&self.view.status);
    }

    /// Puts the chunks the renderer has ready on the scene (see `MapView::apply`). Whether anything changed.
    pub fn apply(&mut self, e: &mut Editor, budget: Option<std::time::Duration>) -> bool {
        self.view.apply(e, budget)
    }

    /// Where the scene sits in the map for the coordinates readout (cells, height above the sea), while a context is shown.
    pub fn map_frame(&self) -> Option<wce_editor::coords::MapFrame> {
        self.view.map_frame()
    }

    /// Where north is in the world for the compass: the map's while a context is shown, else the tile's own +z (its rows' top edge).
    pub fn north(&self) -> [f32; 2] {
        self.view.north()
    }

    /// Shows or hides the context's sea (the tile's sea toggle).
    pub fn show_sea(&self, e: &mut Editor, on: bool) {
        self.view.show_sea(e, on);
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
        if let Some(d) = e.docs.find_as_mut::<TileDocument>(self.view.doc()) {
            d.tile_mut().set_clip_to_tile(&mut e.scene, false);
            d.tile_mut().set_context_lf(&mut e.scene, None);
        }

        self.view.clear(e);
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
                gui::text_wrapped(&match &self.placed_variation {
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
        rings |= gui::slider_int("buildings (cells)", &mut self.mid, 1, 128);
        rings |= gui::slider_int("tile ground (cells)", &mut self.ground, 1, 1024);
        rings |= gui::slider_int("LF ground (cells)", &mut self.far, 1, 1024);
        if rings {
            let r = self.rings();
            self.view.set_rings(&r);
        }

        if self.view.far_view() > self.far {
            gui::text_disabled(&format!("LF ground to the camera's view: {} cells", self.view.far_view()));
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
            match self.view.snowy_cells(&season) {
                Some(0) => gui::text_disabled("no snow on the map this season"),
                Some(n) => gui::text_disabled(&format!("{n} snowy cells this season")),
                None => {}
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

    /// The overview map over the viewport (`MapView::draw_overview`), the open tile's placement highlighted.
    pub fn draw_minimap(&mut self, e: &mut Editor, origin: gui::Vec2, size: gui::Vec2) {
        if !self.minimap || !self.shown {
            return;
        }

        let Some((name, map, _)) = self.loaded.clone() else { return };
        let open = self.view.frame().map(|f| f.open);
        self.view.draw_overview(e, origin, size, &name, &map, open);
    }
}
