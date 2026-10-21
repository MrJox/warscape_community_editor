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
use wce_scene::render_scene::{DocumentId, EntityId, LayerId, Payload, RenderScene, SourceRef};
use wce_tile::battle_map::context::{CELL, PlacedMap, Spot};
use wce_tile::battle_map::locations::BattleLocations;
use wce_tile::battle_map::tile_db::TileDatabase;
use wce_tile::battle_map::{kit_maps, load_game_map, load_kit_map};
use wce_tile_editor::tile_document::TileDocument;
use wce_tile_editor::tile_document::context::{CHUNK, ChunkChange, ChunkKey, ContextRequest, Streamer, cache};
use wce_vfs::files::Files;

/// How long a frame spends putting finished chunks on the scene (one at least; the rest wait for the next frames).
const APPLY_BUDGET: std::time::Duration = std::time::Duration::from_millis(3);

/// The campaign battle map of the game's files.
pub const GAME_MAP: &str = "campaign (game files)";
const GAME_DIR: &str = "terrain/battles/main_attila_map";

pub enum Job {
    Map(String, Result<(Arc<PlacedMap>, Option<BattleLocations>), String>),
    Built(u64, Box<Streamer>),
}

pub struct ContextUi {
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
    pub trees: bool,

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

    /// Each chunk's entities on the layer (added in a row: a range of ids), and the sea's (with the focus it is around).
    chunks: HashMap<ChunkKey, std::ops::Range<EntityId>>,

    /// The streamer's chunks waiting for the renderer to have their terrains' GPU side, and the ones to take off.
    waiting: Vec<(ChunkKey, Vec<(Float4x4, Payload)>)>,
    gone: Vec<ChunkKey>,
    sea: Option<EntityId>,
    sea_at: (f64, f64),
    db: Option<(PathBuf, Arc<TileDatabase>)>,
    maps_for: PathBuf,
}

impl Default for ContextUi {
    fn default() -> Self {
        Self {
            shown: false, maps: vec![], map: 0, loaded: None, placements: vec![], choice: 0, manual: [0, 0, 0], near: 8, mid: 24, far: 64, trees: true,
            follow: true, status: String::new(), busy: String::new(), rx: None, streamer: None, layer: 0, doc: 0, build: 0, chunks: HashMap::new(), waiting: vec![],
            gone: vec![], sea: None,
            sea_at: (0.0, 0.0), db: None, maps_for: PathBuf::new(),
        }
    }
}

fn database(ui: &mut ContextUi, raw: &Path) -> Result<Arc<TileDatabase>, String> {
    if let Some((r, db)) = &ui.db
        && r == raw
    {
        return Ok(db.clone());
    }

    let db = Arc::new(TileDatabase::load(&raw.join("terrain/tiles/battle/_tile_database"))?);
    ui.db = Some((raw.to_path_buf(), db.clone()));
    Ok(db)
}

/// Reads a map (slow for the kit's campaign map: BOB's fill of 2032 x 1437 cells takes about a minute).
pub fn load_map(name: &str, raw: &Path, db: &TileDatabase, files: Option<&dyn Files>) -> Result<(Arc<PlacedMap>, Option<BattleLocations>), String> {
    if name == GAME_MAP {
        let files = files.ok_or("no game files")?;
        let read = |n: &str| files.read(&format!("{GAME_DIR}/{n}"));
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
        if files.is_some_and(|f| f.exists(&format!("{GAME_DIR}/tile_map.tiles"))) {
            self.maps.push(GAME_MAP.to_owned());
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
        self.busy = format!("reading {name}...");
        self.rx = Some(rx);
        std::thread::spawn(move || {
            let r = load_map(&name, &raw, &db, files.as_deref());
            let _ = tx.send(Job::Map(name, r));
        });
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

    /// The request for the open tile, `None` without a map or a kit.
    pub fn request(&self, d: &TileDocument) -> Option<ContextRequest> {
        let (_, map, _) = self.loaded.as_ref()?;
        let p = d.project();
        let raw = p.raw_root.clone()?;
        Some(ContextRequest {
            map: map.clone(), spot: self.spot()?, cells: (p.info.cells_x.max(1) as i32, p.info.cells_y.max(1) as i32), unit: p.unit_scale, near: self.near,
            mid: self.mid, far: self.far, raw_root: raw, look: d.tile().terrain_look().clone(), trees: self.trees, plain: d.tile().ground_material_base(),
            cache: Some(cache::cache_dir()),
        })
    }

    /// Starts streaming the context of the open tile (its frame is made on a thread first).
    pub fn start_build(&mut self, rq: ContextRequest, files: Option<Arc<dyn Files>>) {
        self.build += 1;
        let n = self.build;
        let (tx, rx) = channel();
        self.busy = "reading the map's ground...".to_owned();
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

            self.layer = e.scene.add_layer(id, "map context");
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

        // The sea is at 0 m: in the open tile's frame, below it by the mean LF under it.
        e.scene.sea_level = s.frame().sea_y();
        log::info(format!("map context: {}", s.frame().lf_note()));
        self.streamer = Some(s);
        let at = self.focus(e).unwrap_or_default();
        let sea = add_range(&mut e.scene, self.layer, id, vec![self.streamer.as_ref().expect("set").frame().sea(at, self.sea_reach())]);
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

        let cam = e.viewport.camera();
        let p = if e.viewport.is_fly() { cam.eye } else { cam.target };
        Some((f64::from(p.x), f64::from(p.z)))
    }

    /// The cells around the focus the sea covers: the far ring and some.
    fn sea_reach(&self) -> i32 {
        self.far.max(self.mid).max(self.near) + 4 * CHUNK
    }

    /// One update of the streamer: the chunks it finished replace theirs on the layer, the ones it dropped go (one pass over the scene); the
    /// sea follows the focus.
    pub fn stream(&mut self, e: &mut Editor) -> bool {
        wce_prof!("context.stream");
        let Some(focus) = self.focus(e) else { return false };
        let reach = self.sea_reach();
        let Some(s) = self.streamer.as_mut() else { return false };
        let changes = s.update(focus);
        self.status = s.report();
        let (dx, dz) = (focus.0 - self.sea_at.0, focus.1 - self.sea_at.1);
        let moved = (dx * dx + dz * dz).sqrt() > f64::from(2 * CHUNK) * CELL;
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

        let Some(d) = e.docs.find_as::<TileDocument>(id) else { return };
        if let Some(rq) = self.request(d) {
            self.start_build(rq, e.files());
        }
    }

    /// Takes the context off the scene (and stops its streamer).
    pub fn clear(&mut self, e: &mut Editor) {
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
        self.chunks.clear();
        self.waiting.clear();
        self.gone.clear();
        self.sea = None;
        self.build += 1;
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
        let Some(raw) = d.project().raw_root.clone() else {
            gui::text_disabled("the tile is not in an assembly kit (no neighbours to open)");
            return;
        };

        let files = e.files();
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
            s.set_rings(self.near, self.mid, self.far);
        }

        changed |= gui::checkbox("their trees", &mut self.trees);
        gui::checkbox("follow the camera", &mut self.follow);
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
