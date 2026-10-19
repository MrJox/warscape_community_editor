//! The Map context section of the Tile panel: the open tile among its neighbours in a battle map (wce_tile_editor's `context`). The maps are
//! the game's campaign battle map (terrain.pack's `terrain/battles/main_attila_map`, as the game ships it) and the kit's battle maps (filled
//! as BOB does). Where the tile is placed more than once (the settlement tiles are reused across settlements), the user picks the placement;
//! where it is placed nowhere, a cell and a rotation. Loading and building run on a worker thread.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::mpsc::{Receiver, channel};

use wce_editor::editor::Editor;
use wce_editor::gui;
use wce_foundation::log;
use wce_scene::render_scene::{DocumentId, LayerId, Payload};
use wce_tile::battle_map::context::{PlacedMap, Spot};
use wce_tile::battle_map::locations::BattleLocations;
use wce_tile::battle_map::tile_db::TileDatabase;
use wce_tile::battle_map::{kit_maps, load_game_map, load_kit_map};
use wce_tile_editor::tile_document::TileDocument;
use wce_tile_editor::tile_document::context::{ContextRequest, ContextScene, build_context, publish_context};
use wce_vfs::files::Files;

/// The campaign battle map of the game's files.
pub const GAME_MAP: &str = "campaign (game files)";
const GAME_DIR: &str = "terrain/battles/main_attila_map";

pub enum Job {
    Map(String, Result<(Arc<PlacedMap>, Option<BattleLocations>), String>),
    Built(u64, Box<ContextScene>),
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
    pub radius: i32,
    pub far: i32,
    pub trees: bool,
    pub status: String,
    pub busy: String,
    rx: Option<Receiver<Job>>,

    /// The scene layer of the context, the tile it was built for and the build's number (a newer request wins).
    layer: LayerId,
    doc: DocumentId,
    pub build: u64,
    db: Option<(PathBuf, Arc<TileDatabase>)>,
    maps_for: PathBuf,
}

impl Default for ContextUi {
    fn default() -> Self {
        Self {
            shown: false, maps: vec![], map: 0, loaded: None, placements: vec![], choice: 0, manual: [0, 0, 0], radius: 8, far: 48, trees: true,
            status: String::new(), busy: String::new(), rx: None, layer: 0, doc: 0, build: 0, db: None, maps_for: PathBuf::new(),
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
            map: map.clone(), spot: self.spot()?, cells: (p.info.cells_x.max(1) as i32, p.info.cells_y.max(1) as i32), unit: p.unit_scale, radius: self.radius,
            far: self.far, raw_root: raw, look: d.tile().terrain_look().clone(), trees: self.trees,
        })
    }

    /// Starts building the context of the open tile.
    pub fn start_build(&mut self, rq: ContextRequest, files: Option<Arc<dyn Files>>) {
        self.build += 1;
        let n = self.build;
        let (tx, rx) = channel();
        self.busy = "building the context...".to_owned();
        self.rx = Some(rx);
        std::thread::spawn(move || {
            let built = build_context(&rq, files);
            let _ = tx.send(Job::Built(n, Box::new(built)));
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

    /// Puts a finished job's result in place.
    pub fn poll(&mut self, e: &mut Editor, id: DocumentId) -> bool {
        let Some(rx) = &self.rx else { return false };
        let Ok(job) = rx.try_recv() else { return false };
        self.rx = None;
        self.busy.clear();
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
            Job::Built(n, mut built) => {
                if n == self.build && self.shown {
                    self.publish(e, id, &mut built);
                }
            }
        }

        true
    }

    pub fn publish(&mut self, e: &mut Editor, id: DocumentId, built: &mut ContextScene) {
        if self.layer == 0 || self.doc != id || e.scene.layer(self.layer).is_none() {
            if self.layer != 0 {
                e.scene.remove_layer(self.layer);
            }

            self.layer = e.scene.add_layer(id, "map context");
            self.doc = id;
        }

        publish_context(&mut e.scene, self.layer, built, id);

        // The open tile's height map border would lie over the neighbours.
        if let Some(d) = e.docs.find_as_mut::<TileDocument>(id) {
            d.tile_mut().set_clip_to_tile(&mut e.scene, true);
            let sea = d.tile().display().show_sea;
            self.show_sea(e, sea);
        }

        self.status = built.report.clone();
        log::info(format!("map context: {}", built.report));
        for m in built.missing.iter().take(10) {
            log::info(format!("map context: not opened: {m}"));
        }
    }

    /// Shows or hides the context's sea (the tile's sea toggle).
    pub fn show_sea(&self, e: &mut Editor, on: bool) {
        if self.layer == 0 {
            return;
        }

        let seas = e.scene.entities().iter()
            .filter(|en| en.layer == self.layer && matches!(&en.payload, Payload::Water(w) if !w.river) && en.visible != on)
            .map(|en| en.id)
            .collect::<Vec<_>>();
        for id in &seas {
            if let Some(en) = e.scene.find_mut(*id) {
                en.visible = on;
            }
        }

        if !seas.is_empty() {
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

    /// Takes the context off the scene.
    pub fn clear(&mut self, e: &mut Editor) {
        if self.layer != 0 {
            e.scene.remove_layer(self.layer);
            self.layer = 0;
        }

        if let Some(d) = e.docs.find_as_mut::<TileDocument>(self.doc) {
            d.tile_mut().set_clip_to_tile(&mut e.scene, false);
        }

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

        changed |= gui::slider_int("neighbour cells", &mut self.radius, 1, 64);
        changed |= gui::slider_int("ground beyond (cells)", &mut self.far, 0, 256);
        changed |= gui::checkbox("their trees", &mut self.trees);
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
