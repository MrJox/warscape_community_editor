//! The Terrain tool: its Map panel (which map, the rings, the view distance), the map documents and the view of the active one.
//!
//! Opening a map reads it on a worker (a kit map is filled as BOB does: the kit's campaign battle map takes about a minute; its folder is
//! read into the model too), then makes its view's frame on another (the map's LF), then streams it around the camera
//! (`wce_map_view::MapView`) in the map's own frame: origin at the map's centre, x along the columns, z up the rows, heights from the sea.

use std::any::Any;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::mpsc::{Receiver, channel};

use wce_editor::editor::Editor;
use wce_editor::tool::Tool;
use wce_editor::{gui, icons};
use wce_editor_core::command_registry::{CommandRegistry, CommandResult};
use wce_foundation::log;
use wce_foundation::math::Float3;
use wce_map_view::maps::{GAME_FAR_PLANE, PacksTree, game_map_folder, game_maps, load_map};
use wce_map_view::{MapView, Rings};
use wce_scene::render_scene::DocumentId;
use wce_terrain::MapKind;
use wce_terrain::pyramid::HeightPyramid;
use wce_terrain_editor::{MapDocument, MapSource};
use wce_tile::battle_map::context::{CELL, Spot};
use wce_tile::battle_map::kit_maps;
use wce_tile::battle_map::tile_db::TileDatabase;
use wce_tile_editor::terrain_bake::TerrainLook;
use wce_tile_editor::tile_document::context::{ContextRequest, Streamer, TileProvider, cache};
use wce_vfs::files::Files;

/// Scene units per metre (the tiles' editor units: the map context's frame is in them).
const UNIT: f64 = 2.0;

/// A map read on a worker: what its document is made of (the document itself stays on the UI thread).
struct Loaded {
    name: String,
    source: MapSource,
    model: Option<wce_terrain::MapModel>,
    placed: Arc<wce_tile::battle_map::context::PlacedMap>,
    locations: Option<wce_tile::battle_map::locations::BattleLocations>,
}

impl Loaded {
    fn document(self) -> Box<MapDocument> {
        Box::new(MapDocument::new(self.name, self.source, self.model, self.placed, self.locations))
    }
}

/// The shown map's LF and its pyramid: where a ray meets the LF (what the LF brushes paint).
struct LfPick {
    lf: Arc<wce_scene::render_scene::LfMap>,
    pyramid: HeightPyramid,
}

impl LfPick {
    fn new(lf: Arc<wce_scene::render_scene::LfMap>) -> Self {
        let (nx, nz) = (lf.nx.max(0) as u32, lf.nz.max(0) as u32);
        let pyramid = HeightPyramid::new(nx, nz, |x, y| lf.values[(y * nx + x) as usize]);
        Self { lf, pyramid }
    }

    /// Where the ray (frame, scene units) meets the LF: the point (scene units) and the sample's raw height there.
    fn hit(&self, o: Float3, d: Float3) -> Option<(Float3, f32)> {
        let lf = &self.lf;
        let (s, k) = (lf.step, 65535.0 / lf.scale.max(1e-6));
        let os = [(o.x - lf.x0) / s, (o.y + lf.offset) * k, (o.z - lf.z0) / s];
        let ds = [d.x / s, d.y * k, d.z / s];
        let nx = lf.nx as u32;
        let t = self.pyramid.intersect(os, ds, |x, y| lf.values[(y * nx + x) as usize])?;
        let p = o + d * t;
        Some((p, os[1] + ds[1] * t))
    }
}

enum Job {
    Opened(Result<Loaded, String>),
    Built(DocumentId, Box<Streamer>),
}

/// What opening a map needs, taken from the editor on the UI thread.
struct OpenInputs {
    name: String,
    game_dir: &'static str,
    raw: PathBuf,
    db: Arc<TileDatabase>,
    files: Option<Arc<dyn Files>>,
    packs: Option<Arc<PacksTree>>,
}

pub struct TerrainTool {
    maps: Vec<String>,
    map: i32,
    maps_for: Vec<PathBuf>,
    rings: Rings,
    overview: bool,

    /// The shown map's LF picking, and the cursor's point on it (frame, scene units) with its raw LF height.
    pick: Option<LfPick>,
    cursor: Option<(Float3, f32)>,

    /// The map on the scene and the document it shows, the one whose view is being made, the camera of each map last shown.
    view: MapView,
    shown: DocumentId,
    building: DocumentId,
    cameras: std::collections::HashMap<DocumentId, wce_scene::camera::Camera>,

    /// A job on its way (one at a time) and what it is doing.
    rx: Option<Receiver<Job>>,
    busy: String,
    status: String,

    /// The packs' tiles in the kit's layout (a placement no kit has; a map of the packs names the packs' tiles), and the kits' tile roots.
    packs: Option<Arc<PacksTree>>,
    provider: Option<TileProvider>,
    db: Option<(PathBuf, Arc<TileDatabase>)>,
}

impl Default for TerrainTool {
    fn default() -> Self {
        Self {
            maps: vec![], map: 0, maps_for: vec![], rings: Rings::default(), overview: true, pick: None, cursor: None, view: MapView::default(), shown: 0, building: 0, cameras: std::collections::HashMap::new(), rx: None, busy: String::new(),
            status: String::new(), packs: None, provider: None, db: None,
        }
    }
}

/// The `raw_data` folders a placement's tile and a kit map are looked for in, in order: the projects' kit layout, then the Assembly Kits in
/// the settings' order (`wce_tile::sources`).
fn kit_roots(e: &Editor) -> Vec<PathBuf> {
    let mut roots = e.settings_ref().assembly_kit_raw_dirs();
    roots.retain(|raw| raw.join("terrain").is_dir());
    let projects = e.settings_ref().kit_projects_dir().join("raw_data");
    if projects.join("terrain").is_dir() {
        roots.insert(0, projects);
    }

    roots
}

/// The kit with the battle tile database (its `raw_data`).
fn database_root(e: &Editor) -> Option<PathBuf> {
    kit_roots(e).into_iter().find(|r| r.join("terrain/tiles/battle/_tile_database").is_dir())
}

/// Reads a map (a worker's job).
fn open_map(i: &OpenInputs) -> Result<Loaded, String> {
    let packs_folder = game_map_folder(&i.name, i.game_dir);
    let db = match (&packs_folder, &i.packs) {
        // A map of the packs names the packs' tiles (a mod's among them): read with their database.
        (Some(_), Some(p)) => p.raw().ok().and_then(|r| TileDatabase::load(&r.join("terrain/tiles/battle/_tile_database")).ok()).map_or_else(|| i.db.clone(), Arc::new),
        _ => i.db.clone(),
    };

    let (placed, locations) = load_map(&i.name, i.game_dir, &i.raw, &db, i.files.as_deref())?;
    let (source, model) = match packs_folder {
        Some(folder) => (MapSource::Packs(folder), None),
        None => {
            let dir = i.raw.join("terrain/battles").join(&i.name);
            let files = wce_terrain_io::MapFiles::from_dir(&dir).map_err(|e| e.to_string())?;
            let model = wce_terrain_io::attila::read(&i.name, MapKind::Battle, &files).map_err(|e| e.to_string())?;
            (MapSource::Kit(dir), Some(model))
        }
    };

    Ok(Loaded { name: i.name.clone(), source, model, placed, locations })
}

impl TerrainTool {
    fn database(&mut self, raw: &std::path::Path) -> Result<Arc<TileDatabase>, String> {
        if let Some((r, db)) = &self.db
            && r == raw
        {
            return Ok(db.clone());
        }

        let db = Arc::new(TileDatabase::load(&raw.join("terrain/tiles/battle/_tile_database"))?);
        self.db = Some((raw.to_path_buf(), db.clone()));
        Ok(db)
    }

    /// The packs' tiles in the kit's layout, for the current packs.
    fn prepare_packs(&mut self, e: &Editor) {
        let stale = self.packs.as_ref().is_some_and(|t| e.vfs().is_none_or(|v| !t.same_vfs(v)));
        if (self.packs.is_none() || stale)
            && let Some(vfs) = e.vfs().cloned()
        {
            let kit = database_root(e).and_then(|r| r.parent().map(std::path::Path::to_path_buf));
            let tree = Arc::new(PacksTree::new(vfs, kit, &e.settings_ref().paths().cache_dir().join("neighbours"), e.game().key));
            let t = tree.clone();
            self.provider = Some(Arc::new(move |location: &str, variation: &str| t.tile(location, variation)));
            self.packs = Some(tree);
        }
    }

    /// The maps to choose from: the packs' battle maps, then each kit's.
    fn refresh_maps(&mut self, e: &Editor) {
        let roots = kit_roots(e);
        if self.maps_for == roots && !self.maps.is_empty() {
            return;
        }

        self.maps.clear();
        if let Some(f) = e.files() {
            self.maps.extend(game_maps(f.as_ref(), e.game().campaign_battle_map));
        }

        for r in &roots {
            for m in kit_maps(r) {
                if !self.maps.contains(&m) {
                    self.maps.push(m);
                }
            }
        }

        self.maps_for = roots;
        self.map = self.map.clamp(0, (self.maps.len() as i32 - 1).max(0));
    }

    /// What opening `name` needs.
    fn inputs(&mut self, e: &Editor, name: &str) -> Result<OpenInputs, String> {
        if !e.game().features.kit_tiles {
            return Err(format!("{}: the Terrain tool reads its kit's battle maps and tile database (Attila and Thrones for now)", e.game().display_name));
        }

        self.prepare_packs(e);
        let roots = kit_roots(e);
        let raw = roots.iter().find(|r| r.join("terrain/battles").join(name).join("tile_map.png").is_file()).cloned()
            .or_else(|| database_root(e))
            .ok_or("no Assembly Kit with a battle tile database in the settings")?;
        let db_root = database_root(e).unwrap_or_else(|| raw.clone());
        let db = self.database(&db_root)?;
        Ok(OpenInputs { name: name.to_owned(), game_dir: e.game().campaign_battle_map, raw, db, files: e.files(), packs: self.packs.clone() })
    }

    /// Starts reading a map.
    fn start_open(&mut self, e: &Editor, name: &str) {
        match self.inputs(e, name) {
            Ok(i) => {
                let (tx, rx) = channel();
                self.busy = format!("reading {name}...");
                self.rx = Some(rx);
                std::thread::spawn(move || {
                    let _ = tx.send(Job::Opened(open_map(&i)));
                });
            }

            Err(err) => self.status = err,
        }
    }

    /// The view's request for a map document: the map's own frame (no open tile), the panel's rings.
    fn request(&self, e: &Editor, d: &MapDocument) -> Option<ContextRequest> {
        let m = &d.placed;
        let roots = kit_roots(e);
        Some(ContextRequest {
            map: m.clone(), spot: Spot { x: (m.width / 2) as i32, y: (m.height / 2) as i32, rotation: 0x10 }, cells: (0, 0), unit: UNIT,
            near: self.rings.near, mid: self.rings.mid, far: self.view.far_ring(self.rings.far), raw_root: database_root(e)?, tile_roots: roots,
            game_kit: database_root(e).and_then(|r| r.parent().map(std::path::Path::to_path_buf)), look: TerrainLook::default(), trees: true, plain: None,
            groups: None, cache: Some(cache::cache_dir()), provider: self.provider.clone(),
        })
    }

    /// A document read: on the editor's list (the active one: the view follows it).
    fn adopt(&mut self, e: &mut Editor, d: Box<MapDocument>) {
        self.status = format!("{}: {}", d.name, d.summary());
        let id = e.docs.add(&mut e.scene, d, "terrain");
        e.docs.set_active(id);
        self.start_view(e, id);
    }

    /// Starts making the view's frame of a map document on a worker.
    fn start_view(&mut self, e: &mut Editor, id: DocumentId) {
        let Some(rq) = e.docs.find_as::<MapDocument>(id).and_then(|d| self.request(e, d)) else { return };
        self.building = id;
        let (tx, rx) = channel();
        let files = e.files();
        self.busy = "reading the map's ground...".to_owned();
        self.rx = Some(rx);
        std::thread::spawn(move || {
            let _ = tx.send(Job::Built(id, Box::new(Streamer::new(rq, files))));
        });
    }

    /// The map lit by a weather of the climate at its centre: the first day, dry `.environment` of `weather/<climate>/` in the packs
    /// (ASSUMPTION: a stand-in for the Tile tool's weather choice through the kit's variation database, not shared yet).
    fn light(&mut self, e: &mut Editor, id: DocumentId) {
        let Some(d) = e.docs.find_as::<MapDocument>(id) else { return };
        let m = &d.placed;
        let (cx, cy) = ((m.width / 2) as i32, (m.height / 2) as i32);
        let climate = m.placements.iter().find(|p| { let r = p.rect(); r[0] <= cx && cx < r[2] && r[1] <= cy && cy < r[3] }).and_then(|p| m.climate_of(p)).map(str::to_lowercase);
        let Some(files) = e.files() else { return };
        let pick = |folder: &str| {
            let mut v: Vec<String> = files.list(&format!("weather/{folder}/"), ".environment");
            v.sort();
            v.iter().find(|f| f.contains("/day/") && f.contains("/dry/")).or_else(|| v.first()).cloned()
        };

        let Some(file) = climate.as_deref().and_then(pick).or_else(|| pick("rome_med")) else { return };
        match e.set_weather_preview(Some(&file)) {
            Ok(_) => log::info(format!("terrain: the map's weather {file}")),
            Err(err) => e.status(&format!("the map's weather: {err}"), true),
        }
    }

    /// A view's frame made: the view on the scene, the camera where it was on this map (else over its centre).
    fn show(&mut self, e: &mut Editor, id: DocumentId, s: Box<Streamer>) {
        if self.shown != 0 {
            self.cameras.insert(self.shown, e.viewport.camera());
        }

        self.light(e, id);
        let lift = s.frame().lift(0.0, 0.0);
        self.pick = s.frame().lf.clone().map(LfPick::new);
        self.cursor = None;
        self.view.begin(e, id, s, &self.rings, |_| vec![]);
        self.shown = id;
        self.building = 0;
        let cam = self.cameras.get(&id).copied().unwrap_or_else(|| {
            let target = Float3::new(0.0, lift, 0.0);
            wce_scene::camera::Camera { eye: target + Float3::new(0.0, 1800.0, -2600.0), target, ..e.viewport.camera() }
        });

        e.viewport.controller().set_camera(&cam);
        self.view.stream(e, &self.rings);
    }

    /// Takes in what the workers finished.
    fn poll(&mut self, e: &mut Editor) {
        let Some(rx) = &self.rx else { return };
        let Ok(job) = rx.try_recv() else { return };
        self.rx = None;
        self.busy.clear();
        match job {
            Job::Opened(Ok(l)) => self.adopt(e, l.document()),
            Job::Opened(Err(err)) => self.status = err,
            Job::Built(id, s) => {
                if e.docs.find(id).is_some() && e.docs.active_id() == id {
                    self.show(e, id, s);
                } else {
                    self.building = 0;
                }
            }
        }
    }

    /// Opens a map and waits for its view (scripts).
    fn open_now(&mut self, e: &mut Editor, name: &str) -> Result<String, String> {
        self.refresh_maps(e);
        let i = self.inputs(e, name)?;
        let d = open_map(&i)?.document();
        let summary = format!("{}: {}", d.name, d.summary());
        let rq = self.request(e, &d).ok_or("no kit with a tile database")?;
        let id = e.docs.add(&mut e.scene, d, "terrain");
        e.docs.set_active(id);
        let s = Box::new(Streamer::new(rq, e.files()));
        self.show(e, id, s);
        self.status.clone_from(&summary);
        Ok(summary)
    }

    fn draw_map_panel(&mut self, e: &mut Editor) {
        if !gui::begin("Terrain", None, 0) {
            gui::end();
            return;
        }

        self.refresh_maps(e);
        let names: Vec<&str> = self.maps.iter().map(String::as_str).collect();
        gui::combo("map", &mut self.map, &names);
        let can = self.rx.is_none() && !names.is_empty();
        if gui::button("Open") && can {
            let name = self.maps[self.map as usize].clone();
            self.start_open(e, &name);
        }

        gui::separator();
        let mut rings = gui::slider_int("full detail (cells)", &mut self.rings.near, 1, 64);
        rings |= gui::slider_int("ground and buildings (cells)", &mut self.rings.mid, 1, 128);
        rings |= gui::slider_int("LF ground (cells)", &mut self.rings.far, 1, 512);
        if rings {
            self.view.set_rings(&self.rings);
        }

        if self.view.far_view() > self.rings.far {
            gui::text_disabled(&format!("LF ground to the camera's view: {} cells", self.view.far_view()));
        }

        let whole = e.docs.find_as::<MapDocument>(self.shown).map_or(GAME_FAR_PLANE * 4.0, |d| (d.placed.width.max(d.placed.height) as f32 * CELL as f32).max(GAME_FAR_PLANE));
        gui::slider_float("view distance", &mut self.rings.view_distance, GAME_FAR_PLANE, whole, "%.0f");
        gui::checkbox("follow the camera", &mut self.rings.follow);
        gui::checkbox("overview map", &mut self.overview);
        if !self.busy.is_empty() {
            gui::text_disabled(&self.busy);
        }

        if !self.status.is_empty() {
            gui::text_wrapped(&self.status);
        }

        if !self.view.status.is_empty() {
            gui::text_wrapped(&self.view.status);
        }

        if let Some(line) = self.cursor_line() {
            gui::text_disabled(&line);
        }

        gui::end();
    }
}

impl TerrainTool {
    /// Where a viewport pixel's ray meets the shown map's LF.
    fn pick_at(&self, e: &Editor, x: i32, y: i32) -> Option<(Float3, f32)> {
        let (o, d) = e.viewport.ray(x, y)?;
        self.pick.as_ref()?.hit(o, d)
    }

    /// The cursor's point on the LF for the panel: its map cell, metres above the sea and the raw sample.
    fn cursor_line(&self) -> Option<String> {
        let (p, raw) = self.cursor?;
        let f = self.view.frame()?;
        let (cx, cy) = f.cell_of(f64::from(p.x), f64::from(p.z));
        let metres = f64::from(raw) / 65535.0 * wce_tile::battle_map::context::LF_HEIGHT - wce_tile::battle_map::context::VERTICAL_OFFSET;
        Some(format!("LF under the cursor: cell {cx:.2}, {cy:.2}, {metres:.2} m (raw {raw:.1})"))
    }
}

fn with_terrain(e: &mut Editor, f: impl FnOnce(&mut TerrainTool, &mut Editor) -> CommandResult) -> CommandResult {
    e.with_tool::<TerrainTool, _>(f).unwrap_or_else(|| CommandResult::fail("the terrain tool is not available"))
}

impl Tool for TerrainTool {
    fn id(&self) -> &str {
        "terrain"
    }

    fn title(&self) -> &str {
        "Terrain"
    }

    fn icon(&self) -> &str {
        icons::MOUNTAIN
    }

    fn draw_panels(&mut self, e: &mut Editor) {
        self.draw_map_panel(e);
    }

    fn viewport_overlay(&mut self, e: &mut Editor, origin: gui::Vec2, size: gui::Vec2) {
        if !self.overview {
            return;
        }

        let Some(d) = e.docs.find_as::<MapDocument>(self.shown) else { return };
        let (name, placed) = (d.name.clone(), d.placed.clone());
        self.view.draw_overview(e, origin, size, &name, &placed, None);
    }

    fn viewport_input(&mut self, e: &mut Editor, ev: &wce_editor::tool::ViewportEvent) -> bool {
        if ev.kind == wce_editor::tool::ViewportEventKind::Hover {
            self.cursor = self.pick_at(e, ev.x, ev.y);
        }

        false
    }

    fn update(&mut self, e: &mut Editor) {
        self.poll(e);

        // The map closed: its view goes with it.
        if self.shown != 0 && e.docs.find(self.shown).is_none() {
            self.view.clear(e);
            self.cameras.remove(&self.shown);
            self.shown = 0;
        }

        // The view follows the active map document.
        let active = e.docs.active_id();
        if active != self.shown && active != self.building && self.rx.is_none() && e.docs.find_as::<MapDocument>(active).is_some() {
            self.start_view(e, active);
        }

        if self.shown != 0 {
            self.view.stream(e, &self.rings);
        }
    }

    fn register_commands(&self, c: &mut CommandRegistry<Editor>) {
        c.add("terrain_open", "terrain_open <map | campaign>", "the Terrain tool: open a battle map (the map list's name; `campaign`: the game's campaign battle map) and show it", |e, a| {
            let Some(name) = a.first() else { return CommandResult::fail("usage: terrain_open <map | campaign>") };
            let want = if name == "campaign" { wce_map_view::maps::GAME_MAP.to_owned() } else { name.clone() };
            with_terrain(e, |t, e| {
                t.refresh_maps(e);
                let want = if t.maps.contains(&want) { want } else { format!("{want}{}", wce_map_view::maps::GAME_FILES) };
                match t.open_now(e, &want) {
                    Ok(s) => CommandResult::success(s),
                    Err(err) => CommandResult::fail(err),
                }
            })
        });

        c.add("terrain_settle", "terrain_settle", "the Terrain tool: wait until the map's chunks around the camera are on the scene; says what it has", |e, _| {
            with_terrain(e, |t, e| {
                if t.view.streamer.is_none() {
                    return CommandResult::fail("no map shown");
                }

                t.view.settle(e, &t.rings);
                CommandResult::success(t.view.status.clone())
            })
        });

        c.add("terrain_pick", "terrain_pick <x> <y>", "the Terrain tool: where the ray of a viewport pixel meets the shown map's LF", |e, a| {
            let (Some(x), Some(y)) = (a.first().and_then(|v| v.parse().ok()), a.get(1).and_then(|v| v.parse().ok())) else {
                return CommandResult::fail("usage: terrain_pick <x> <y>");
            };

            with_terrain(e, |t, e| {
                t.cursor = t.pick_at(e, x, y);
                match (t.cursor, t.cursor_line()) {
                    (Some((p, _)), Some(line)) => CommandResult::success(format!("{line}; at {:.2} {:.2} {:.2}", p.x, p.y, p.z)),
                    _ => {
                        // What a march along the ray over the LF finds (to tell a miss of the pyramid from a ray off the map).
                        let ray = e.viewport.ray(x, y);
                        let march = ray.zip(t.pick.as_ref()).and_then(|((o, d), p)| {
                            (0..200_000).map(|k| k as f32 * 0.5).find(|&k| {
                                let q = o + d * k;
                                q.y <= p.lf.height_at(q.x, q.z)
                            }).map(|k| o + d * k)
                        });
                        CommandResult::fail(format!("no LF under that pixel (ray {ray:?}; a march finds {march:?})"))
                    }
                }
            })
        });

        c.add("terrain_view", "terrain_view <distance> [near mid far]", "the Terrain tool: the camera's view distance (scene units) and the rings (cells)", |e, a| {
            let Some(d) = a.first().and_then(|v| v.parse::<f32>().ok()) else { return CommandResult::fail("usage: terrain_view <distance> [near mid far]") };
            let rings: Vec<i32> = a[1..].iter().filter_map(|v| v.parse().ok()).collect();
            with_terrain(e, |t, _| {
                t.rings.view_distance = d.max(GAME_FAR_PLANE);
                if let [near, mid, far] = rings[..] {
                    (t.rings.near, t.rings.mid, t.rings.far) = (near, mid, far);
                }

                t.view.set_rings(&t.rings);
                CommandResult::success(format!("view distance {}, rings {} / {} / {}", t.rings.view_distance, t.rings.near, t.rings.mid, t.rings.far))
            })
        });
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}
