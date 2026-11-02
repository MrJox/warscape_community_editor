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
use wce_terrain::lf_edit::{LfBrush, LfOp, LfStroke};
use wce_terrain::pyramid::HeightPyramid;
use wce_terrain::LfRole;
use wce_terrain_editor::lf::{LfMirror, LfStrokeCommand};
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
    db: Arc<TileDatabase>,
}

impl Loaded {
    fn document(self) -> Box<MapDocument> {
        Box::new(MapDocument::new(self.name, self.source, self.model, self.placed, self.locations, self.db))
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
        let values = lf.values.read();
        let pyramid = HeightPyramid::new(nx, nz, |x, y| values[(y * nx + x) as usize]);
        drop(values);
        Self { lf, pyramid }
    }

    /// Where the ray (frame, scene units) meets the LF: the point (scene units) and the sample's raw height there.
    fn hit(&self, o: Float3, d: Float3) -> Option<(Float3, f32)> {
        let lf = &self.lf;
        let (s, k) = (lf.step, 65535.0 / lf.scale.max(1e-6));
        let os = [(o.x - lf.x0) / s, (o.y + lf.offset) * k, (o.z - lf.z0) / s];
        let ds = [d.x / s, d.y * k, d.z / s];
        let nx = lf.nx as u32;
        let values = lf.values.read();
        let t = self.pyramid.intersect(os, ds, |x, y| values[(y * nx + x) as usize])?;
        drop(values);
        let p = o + d * t;
        Some((p, os[1] + ds[1] * t))
    }
}

/// The LF brush's settings (the Terrain panel's "LF brush").
#[derive(Clone, Debug)]
struct LfPaint {
    on: bool,

    /// 0 raise, 1 lower, 2 smooth, 3 flatten (to the height where the stroke starts), 4 noise.
    op: i32,
    radius_m: f32,

    /// Metres a dab at full weight (raise, lower, noise); flatten and smooth go this share of the way (0 .. 1) per dab.
    amount_m: f32,
    share: f32,
    falloff: f32,
    aspect: f32,
    rotation_deg: f32,
    noise_scale_m: f32,
}

impl Default for LfPaint {
    fn default() -> Self {
        Self { on: false, op: 0, radius_m: 300.0, amount_m: 2.0, share: 0.2, falloff: 1.0, aspect: 1.0, rotation_deg: 0.0, noise_scale_m: 400.0 }
    }
}

const LF_OPS: [&str; 5] = ["raise", "lower", "smooth", "flatten", "noise"];

/// A stroke being painted: on which document, its record so far, the view's mirror and the flatten target (raw).
struct Painting {
    doc: DocumentId,
    stroke: LfStroke,
    mirror: Option<Arc<LfMirror>>,
    target: f64,
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

    /// The Map panel (the active map from above).
    map_panel: crate::map_panel::MapPanel,

    /// The LF brush, the stroke being painted, the brush ring's layer and entity.
    paint: LfPaint,
    painting: Option<Painting>,

    /// The shown map's field mirrored into its view (a kit map's land LF on the view's lattice).
    mirror: Option<Arc<LfMirror>>,
    ring: (wce_scene::render_scene::LayerId, wce_scene::render_scene::EntityId),

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

    /// When the job on its way started (the panel shows how long it has taken).
    busy_since: Option<std::time::Instant>,
    status: String,

    /// The packs' tiles in the kit's layout (a placement no kit has; a map of the packs names the packs' tiles), and the kits' tile roots.
    packs: Option<Arc<PacksTree>>,
    provider: Option<TileProvider>,
    db: Option<(PathBuf, Arc<TileDatabase>)>,
}

impl Default for TerrainTool {
    fn default() -> Self {
        Self {
            maps: vec![], map: 0, maps_for: vec![], rings: Rings::default(), overview: true, map_panel: crate::map_panel::MapPanel::new(), paint: LfPaint::default(), painting: None, mirror: None, ring: (0, 0), pick: None, cursor: None, view: MapView::default(), shown: 0, building: 0, cameras: std::collections::HashMap::new(), rx: None, busy: String::new(), busy_since: None,
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

/// Where a kit map saves: its folder in the projects' kit layout (a map of the packs: nowhere, it is read-only).
fn save_dir(e: &Editor, d: &MapDocument) -> Option<PathBuf> {
    d.model()?;
    Some(e.settings_ref().kit_projects_dir().join("raw_data/terrain/battles").join(&d.name))
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

    Ok(Loaded { name: i.name.clone(), source, model, placed, locations, db })
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
    fn start_open(&mut self, e: &mut Editor, name: &str) {
        match self.inputs(e, name) {
            Ok(i) => {
                let (tx, rx) = channel();
                // A kit map is filled as BOB does: the campaign map's 2.9 million cells take a while.
                let kit = game_map_folder(name, e.game().campaign_battle_map).is_none();
                self.busy = if kit {
                    format!("reading {name} and filling it with tiles as BOB does (the kit's campaign map: about a minute and a half)")
                } else {
                    format!("reading {name}")
                };

                self.busy_since = Some(std::time::Instant::now());
                e.status(&format!("Terrain: {}...", self.busy), false);
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
    fn adopt(&mut self, e: &mut Editor, mut d: Box<MapDocument>) {
        self.status = format!("{}: {}", d.name, d.summary());
        d.save_dir = save_dir(e, &d);
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
        self.busy = "reading the map's ground".to_owned();
        self.busy_since = Some(std::time::Instant::now());
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
        self.painting = None;
        self.mirror = e.docs.find_as::<MapDocument>(id).and_then(|d| d.lf_field(LfRole::Land))
            .zip(self.pick.as_ref())
            .and_then(|(field, p)| LfMirror::of(field, p.lf.values.clone(), p.lf.nx, p.lf.nz))
            .map(Arc::new);
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
        let took = self.busy_since.take().map_or(0.0, |t| t.elapsed().as_secs_f64());
        match job {
            Job::Opened(Ok(l)) => {
                e.status(&format!("Terrain: {} read in {took:.0} s", l.name), false);
                self.adopt(e, l.document());
            }

            Job::Opened(Err(err)) => {
                e.status(&format!("Terrain: {err}"), true);
                self.status = err;
            }

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
        let mut d = open_map(&i)?.document();
        d.save_dir = save_dir(e, &d);
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

        // What the workers are doing, and for how long (opening the kit's campaign map takes over a minute).
        if !self.busy.is_empty() {
            let secs = self.busy_since.map_or(0, |t| t.elapsed().as_secs());
            gui::text_colored([1.0, 0.8, 0.3, 1.0], &format!("{} ... {secs} s", self.busy));
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
        if !self.status.is_empty() {
            gui::text_wrapped(&self.status);
        }

        if !self.view.status.is_empty() {
            gui::text_wrapped(&self.view.status);
        }

        if let Some(line) = self.cursor_line() {
            gui::text_disabled(&line);
        }

        self.draw_paint_section(e);

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

impl TerrainTool {
    /// Metres between two LF samples of the shown map.
    fn sample_metres(&self) -> f64 {
        self.pick.as_ref().map_or(16.0, |p| f64::from(p.lf.step) / UNIT)
    }

    /// The field sample (x along the columns, y down the rows of the file) under a frame point.
    fn field_xy(&self, p: Float3) -> Option<(f64, f64)> {
        let lf = &self.pick.as_ref()?.lf;
        let (fi, fj) = (f64::from((p.x - lf.x0) / lf.step), f64::from((p.z - lf.z0) / lf.step));
        Some((fi, f64::from(lf.nz - 1) - fj))
    }

    /// The brush at a field sample.
    fn brush_at(&self, cx: f64, cy: f64) -> LfBrush {
        let r = f64::from(self.paint.radius_m) / self.sample_metres();
        let strength = if matches!(self.paint.op, 2 | 3) { f64::from(self.paint.share) } else { 1.0 };
        let brush = wce_tile::height_field::Brush { cx, cy, radius: r.max(1.0), strength, falloff: f64::from(self.paint.falloff), curve: None, image: None };
        LfBrush { brush, aspect: f64::from(self.paint.aspect).max(0.05), rotation: f64::from(self.paint.rotation_deg).to_radians() }
    }

    /// The operation of a dab (raw units).
    fn op(&self, target: f64) -> LfOp {
        let raw = f64::from(self.paint.amount_m) / wce_tile::battle_map::context::LF_HEIGHT * 65535.0;
        match self.paint.op {
            1 => LfOp::Raise(-raw),
            2 => LfOp::Smooth,
            3 => LfOp::Flatten(target),
            4 => LfOp::Noise { amount: raw, scale: f64::from(self.paint.noise_scale_m) / self.sample_metres() },
            _ => LfOp::Raise(raw),
        }
    }

    /// Starts a stroke on the shown map (a kit map: its model has the field); the flatten target is the height at `at`.
    fn begin_stroke(&mut self, e: &mut Editor, at: Option<(f64, f64)>) -> bool {
        let Some(d) = e.docs.find_as::<MapDocument>(self.shown) else { return false };
        let Some(field) = d.lf_field(LfRole::Land) else {
            e.status("this map is from the game's packs: open the kit's map to paint its LF", true);
            return false;
        };

        let mirror = self.mirror.clone();
        let target = at.map_or(0.0, |(x, y)| f64::from(field.at((x.round().max(0.0) as u32).min(field.width() - 1), (y.round().max(0.0) as u32).min(field.height() - 1))));
        self.painting = Some(Painting { doc: self.shown, stroke: LfStroke::default(), mirror, target });
        true
    }

    /// A dab at a field sample: the model's field, the view (mirror, picking).
    fn dab(&mut self, e: &mut Editor, cx: f64, cy: f64) {
        let b = self.brush_at(cx, cy);
        let Some(target) = self.painting.as_ref().map(|p| p.target) else { return };
        let op = self.op(target);
        let Some(paint) = self.painting.as_mut() else { return };
        let Some(d) = e.docs.find_as_mut::<MapDocument>(paint.doc) else { return };
        let Some(field) = d.lf_field_mut(LfRole::Land) else { return };
        let Some(rect) = paint.stroke.dab(field, &b, op) else { return };
        if let Some(m) = &paint.mirror {
            m.copy(field, rect);
            if let Some(pick) = self.pick.as_mut() {
                let v = m.view_rect(rect);
                let values = pick.lf.values.read();
                let nx = pick.lf.nx as u32;
                pick.pyramid.update((v[0], v[1], v[2], v[3]), |x, y| values[(y * nx + x) as usize]);
            }
        }
    }

    /// What the LF edits (strokes, undo, redo) moved, once no stroke is being painted: the picking there, and the chunks whose objects
    /// stand on it built again.
    fn follow_lf_edits(&mut self, e: &Editor) {
        if self.painting.is_some() {
            return;
        }

        let Some(m) = self.mirror.clone() else { return };
        let Some(rect) = m.take_touched() else { return };
        if let Some(pick) = self.pick.as_mut() {
            let v = m.view_rect(rect);
            let values = pick.lf.values.read();
            let nx = pick.lf.nx as u32;
            pick.pyramid.update((v[0], v[1], v[2], v[3]), |x, y| values[(y * nx + x) as usize]);
        }

        let Some(d) = e.docs.find_as::<MapDocument>(self.shown) else { return };
        let (sx, sy) = (f64::from(m.w) / f64::from(d.placed.width.max(1)), f64::from(m.h) / f64::from(d.placed.height.max(1)));
        let cells = [
            (f64::from(rect[0]) / sx).floor() as i32 - 1, (f64::from(rect[1]) / sy).floor() as i32 - 1,
            (f64::from(rect[2]) / sx).ceil() as i32 + 1, (f64::from(rect[3]) / sy).ceil() as i32 + 1,
        ];

        if let Some(s) = self.view.streamer.as_mut() {
            s.rebuild(cells);
        }
    }

    /// The stroke onto the document's history.
    fn end_stroke(&mut self, e: &mut Editor) -> Option<usize> {
        let paint = self.painting.take()?;
        let d = e.docs.find_as_mut::<MapDocument>(paint.doc)?;
        let record = paint.stroke.finish(d.lf_field(LfRole::Land)?)?;
        let bytes = record.bytes();
        d.push_stroke(LfStrokeCommand { role: LfRole::Land, record: Arc::new(record), mirror: paint.mirror });
        Some(bytes)
    }

    /// The brush's ring on the ground at the cursor (an overlay on a layer of the tool's), or none.
    fn update_ring(&mut self, e: &mut Editor) {
        let ring = (|| {
            if !self.paint.on {
                return None;
            }

            let (p, _) = self.cursor?;
            let (cx, cy) = self.field_xy(p)?;
            let lf = &self.pick.as_ref()?.lf;
            let b = self.brush_at(cx, cy);
            let (s, c) = b.rotation.sin_cos();
            let values = lf.values.read();
            let mut pts = vec![];
            for k in 0..=64 {
                let a = f64::from(k) / 64.0 * std::f64::consts::TAU;
                let (u, v) = (a.cos() * b.brush.radius * b.aspect, a.sin() * b.brush.radius);
                let (fx, fy) = (cx + c * u - s * v, cy + s * u + c * v);
                let (x, z) = (lf.x0 + fx as f32 * lf.step, lf.z0 + (f64::from(lf.nz - 1) - fy) as f32 * lf.step);
                pts.push(Float3::new(x, lf.height_in(&values, x, z) + 0.5, z));
            }

            let col = 0xFF40_D0FFu32;
            let lines = pts.windows(2).flat_map(|w| [w[0], w[1]]).map(|p| wce_scene::render_scene::OverlayVertex { p, rgba: col }).collect();
            Some(wce_scene::render_scene::Overlay { lines, depth_test: false, ..Default::default() })
        })();

        let (layer, id) = self.ring;
        match ring {
            Some(o) => {
                if layer == 0 || e.scene.layer(layer).is_none() {
                    self.ring.0 = e.scene.add_layer(self.shown, "terrain brush");
                    self.ring.1 = 0;
                }

                if self.ring.1 != 0 && e.scene.find(self.ring.1).is_some() {
                    e.scene.update_payload(self.ring.1, wce_scene::render_scene::Payload::Overlay(o));
                } else {
                    let src = wce_scene::render_scene::SourceRef { doc: self.shown, item: u64::MAX };
                    self.ring.1 = e.scene.add(self.ring.0, wce_foundation::math::Float4x4::IDENTITY, wce_scene::render_scene::Payload::Overlay(o), src);
                }
            }

            None => {
                if id != 0 {
                    e.scene.remove_layer(layer);
                    self.ring = (0, 0);
                }
            }
        }
    }

    fn draw_paint_section(&mut self, e: &mut Editor) {
        let can = e.docs.find_as::<MapDocument>(self.shown).is_some_and(|d| d.lf_field(LfRole::Land).is_some());
        if !gui::collapsing_header("LF brush", 0) {
            return;
        }

        if !can {
            gui::text_disabled("Open a kit map (not one of the game's packs) to paint its LF.");
            self.paint.on = false;
            return;
        }

        gui::checkbox("paint (left button)", &mut self.paint.on);
        gui::combo("operation", &mut self.paint.op, &LF_OPS);
        gui::slider_float("radius (m)", &mut self.paint.radius_m, 20.0, 5000.0, "%.0f");
        match self.paint.op {
            2 | 3 => {
                gui::slider_float("strength", &mut self.paint.share, 0.01, 1.0, "%.2f");
            }

            _ => {
                gui::slider_float("metres a dab", &mut self.paint.amount_m, 0.05, 50.0, "%.2f");
            }
        }

        if self.paint.op == 4 {
            gui::slider_float("noise size (m)", &mut self.paint.noise_scale_m, 32.0, 5000.0, "%.0f");
        }

        gui::slider_float("falloff", &mut self.paint.falloff, 0.0, 1.0, "%.2f");
        gui::slider_float("aspect", &mut self.paint.aspect, 0.1, 10.0, "%.2f");
        gui::slider_float("rotation (deg)", &mut self.paint.rotation_deg, -180.0, 180.0, "%.0f");
        gui::text_disabled(&format!("a sample every {:.1} m", self.sample_metres()));
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
        self.map_panel.draw(e, self.shown, &self.view);
    }

    fn default_layout(&self, dockspace: gui::ImGuiID) {
        let (left, center) = gui::dock_builder_split(dockspace, gui::DIR_LEFT, 0.24);
        let (right, center) = gui::dock_builder_split(center, gui::DIR_RIGHT, 0.25);
        let (bottom, center) = gui::dock_builder_split(center, gui::DIR_DOWN, 0.22);
        gui::dock_builder_dock_window("Viewport", center);
        gui::dock_builder_dock_window("Map", left);
        gui::dock_builder_dock_window("Outliner", left);
        gui::dock_builder_dock_window("Terrain", right);
        gui::dock_builder_dock_window("Inspector", right);
        gui::dock_builder_dock_window("Assets", bottom);
        gui::dock_builder_dock_window("Log", bottom);
        gui::dock_builder_dock_window("History", bottom);
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
        use wce_editor::tool::ViewportEventKind as K;
        self.cursor = self.pick_at(e, ev.x, ev.y);
        if !self.paint.on || ev.button != 0 {
            return false;
        }

        let at = self.cursor.and_then(|(p, _)| self.field_xy(p));
        match ev.kind {
            K::Press => {
                if self.painting.is_none() && !self.begin_stroke(e, at) {
                    return false;
                }

                if let Some((x, y)) = at {
                    self.dab(e, x, y);
                }

                true
            }

            K::Drag => {
                if let Some((x, y)) = at.filter(|_| self.painting.is_some()) {
                    self.dab(e, x, y);
                }

                true
            }

            K::Release => {
                self.end_stroke(e);
                true
            }

            _ => false,
        }
    }

    fn claims_left_button(&self, _: &Editor) -> bool {
        self.paint.on
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

        self.map_panel.apply_drape(e, self.shown, &self.view);
        self.update_ring(e);
        self.follow_lf_edits(e);
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

        c.add("terrain_start", "terrain_start <map | campaign>", "the Terrain tool: what its Open button does (the map read on a worker); `terrain_wait` for the view", |e, a| {
            let Some(name) = a.first() else { return CommandResult::fail("usage: terrain_start <map | campaign>") };
            let want = if name == "campaign" { wce_map_view::maps::GAME_MAP.to_owned() } else { name.clone() };
            with_terrain(e, |t, e| {
                t.refresh_maps(e);
                let want = if t.maps.contains(&want) { want } else { format!("{want}{}", wce_map_view::maps::GAME_FILES) };
                t.start_open(e, &want);
                CommandResult::success(format!("{}{}", t.busy, t.status))
            })
        });

        c.add("terrain_wait", "terrain_wait [seconds]", "the Terrain tool: waits until what Open started is shown (the workers polled as the frames would)", |e, a| {
            let limit = a.first().and_then(|v| v.parse::<f64>().ok()).unwrap_or(600.0);
            with_terrain(e, |t, e| {
                let t0 = std::time::Instant::now();
                while t.rx.is_some() && t0.elapsed().as_secs_f64() < limit {
                    t.poll(e);
                    std::thread::sleep(std::time::Duration::from_millis(50));
                }

                CommandResult::success(format!("after {:.1} s: shown {}, busy '{}', status '{}'", t0.elapsed().as_secs_f64(), t.shown, t.busy, t.status))
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

        c.add("terrain_map_layer", "terrain_map_layer <n> [ground|off] [opacity]", "the Terrain tool: the Map panel shows its n-th layer (0 the LF, 1 the placements, then the kit map's cell layers), on the ground or not", |e, a| {
            let Some(k) = a.first().and_then(|v| v.parse::<usize>().ok()) else { return CommandResult::fail("usage: terrain_map_layer <n>") };
            let drape = a.get(1).map(|v| v == "ground");
            let opacity = a.get(2).and_then(|v| v.parse::<f32>().ok());
            with_terrain(e, |t, _| {
                t.map_panel.set_layer(k);
                if let Some(on) = drape {
                    t.map_panel.drape = on;
                }

                if let Some(o) = opacity {
                    t.map_panel.opacity = o.clamp(0.05, 1.0);
                }

                CommandResult::success(format!("Map panel layer {k}{}", if t.map_panel.drape { ", on the ground" } else { "" }))
            })
        });

        c.add("terrain_paint", "terrain_paint <raise|lower|smooth|flatten|noise> <cell x> <cell y> <radius m> <amount m or strength> [dabs]", "the Terrain tool: one LF stroke of dabs at a map cell (flatten: to the height there), onto the history", |e, a| {
            let usage = "usage: terrain_paint <raise|lower|smooth|flatten|noise> <cell x> <cell y> <radius m> <amount m or strength> [dabs]";
            let Some(op) = a.first().and_then(|o| LF_OPS.iter().position(|n| n == o)) else { return CommandResult::fail(usage) };
            let nums: Vec<f64> = a[1..].iter().filter_map(|v| v.parse().ok()).collect();
            let [cx, cy, radius, amount, ..] = nums[..] else { return CommandResult::fail(usage) };
            let dabs = nums.get(4).map_or(1, |&n| n.max(1.0) as usize);
            with_terrain(e, |t, e| {
                let Some(d) = e.docs.find_as::<MapDocument>(t.shown) else { return CommandResult::fail("no map shown") };
                let Some(field) = d.lf_field(LfRole::Land) else { return CommandResult::fail("the shown map has no LF to paint (a map of the packs)") };
                let (ppc_x, ppc_y) = (f64::from(field.width()) / f64::from(d.placed.width.max(1)), f64::from(field.height()) / f64::from(d.placed.height.max(1)));
                let (x, y) = (cx * ppc_x, cy * ppc_y);
                let saved = t.paint.clone();
                t.paint.op = op as i32;
                t.paint.radius_m = radius as f32;
                if matches!(op, 2 | 3) { t.paint.share = amount as f32 } else { t.paint.amount_m = amount as f32 }
                if !t.begin_stroke(e, Some((x, y))) {
                    t.paint = saved;
                    return CommandResult::fail("no stroke");
                }

                for _ in 0..dabs {
                    t.dab(e, x, y);
                }

                let r = t.end_stroke(e);
                t.paint = saved;
                match r {
                    Some(bytes) => CommandResult::success(format!("{} at sample ({x:.0}, {y:.0}): {dabs} dabs, the record keeps {} KB", LF_OPS[op], bytes / 1024)),
                    None => CommandResult::fail("the stroke changed nothing"),
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
