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
use wce_terrain::lf_patch;
use wce_terrain::pyramid::HeightPyramid;
use wce_terrain::LfRole;
use wce_terrain_editor::lf::{LfMirror, LfStrokeCommand};
use wce_terrain_editor::{MapDocument, MapSource};
use wce_tile::battle_map::context::{CELL, LfField, Spot};
use wce_tile::battle_map::{kit_maps, l16_dds_heights};
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

    /// BOB's messages of the fill (a kit map).
    messages: Vec<String>,
}

impl Loaded {
    fn document(self) -> Box<MapDocument> {
        let mut d = MapDocument::new(self.name, self.source, self.model, self.placed, self.locations, self.db);
        d.fill_messages = self.messages;
        d.terry_frame = if map_kind(&d.name).0 == MapKind::Campaign { wce_terrain_editor::document::TerryFrame::campaign(d.placed.width, d.placed.height) } else { wce_terrain_editor::document::TerryFrame::BATTLE };
        Box::new(d)
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

    /// The sea LF (`lf_sea_heights.tif`, under the sea tiles), not the land's.
    sea: bool,

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

    /// A dab every this share of the radius along the stroke's path (the stroke the same however fast the mouse goes).
    spacing: f32,
}

impl Default for LfPaint {
    fn default() -> Self {
        Self { on: false, sea: false, op: 0, radius_m: 300.0, amount_m: 2.0, share: 0.2, falloff: 1.0, aspect: 1.0, rotation_deg: 0.0, noise_scale_m: 400.0, spacing: 0.25 }
    }
}

const LF_OPS: [&str; 5] = ["raise", "lower", "smooth", "flatten", "noise"];

/// A stroke being painted: on which document and field, its record so far, the view's mirror and the flatten target (raw).
struct Painting {
    doc: DocumentId,
    role: LfRole,
    stroke: LfStroke,
    mirror: Option<Arc<LfMirror>>,
    target: f64,

    /// The last dab (field samples): the next ones go along the path from it.
    last: Option<(f64, f64)>,
}

enum Job {
    Opened(Result<Loaded, String>),
    Built(DocumentId, Box<Streamer>),
}

/// What opening a map needs, taken from the editor on the UI thread.
/// A kit campaign map's prefix in the map list (`campaigns/main_attila_map`).
pub(crate) const CAMPAIGN: &str = "campaigns/";

/// A map name's kind and folder name (`campaigns/<name>`: a kit campaign map).
pub(crate) fn map_kind(name: &str) -> (MapKind, &str) {
    match name.strip_prefix(CAMPAIGN) {
        Some(n) => (MapKind::Campaign, n),
        None => (MapKind::Battle, name),
    }
}

/// The folder of a kit map under a `raw_data` (`terrain/battles/<map>`, `terrain/campaigns/<map>`).
pub(crate) fn kit_map_dir(raw: &std::path::Path, name: &str) -> PathBuf {
    match map_kind(name) {
        (MapKind::Campaign, n) => raw.join("terrain/campaigns").join(n),
        (_, n) => raw.join("terrain/battles").join(n),
    }
}

/// The tile database folder of a kind of map.
fn database_dir(kind: MapKind) -> &'static str {
    if kind == MapKind::Campaign { "terrain/tiles/campaign/_tile_database" } else { "terrain/tiles/battle/_tile_database" }
}

/// A tile database's `RENDER_PARAMS` `lf_height` and `vertical_offset` (its `_settings.xml`).
fn render_lf(settings: &std::path::Path) -> Option<(f64, f64)> {
    let doc = wce_formats::xml_doc::XmlDoc::parse(std::fs::read(settings).ok()?).ok()?;
    let e = doc.elements().iter().position(|el| el.name == "RENDER_PARAMS")?;
    Some((doc.get(e, "lf_height")?.trim().parse().ok()?, doc.get(e, "vertical_offset")?.trim().parse().ok()?))
}

/// A kit's campaign maps (folders with a `tile_map.png`), as the map list names them.
fn kit_campaign_maps(raw: &std::path::Path) -> Vec<String> {
    let mut out: Vec<String> = std::fs::read_dir(raw.join("terrain/campaigns")).into_iter().flatten().flatten()
        .filter(|e| e.path().join("tile_map.png").is_file())
        .map(|e| format!("{CAMPAIGN}{}", e.file_name().to_string_lossy()))
        .collect();
    out.sort();
    out
}

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

    /// The cell layers' painting; the fill run again after an edit of the layers it reads (and one wanted once the stroke ends).
    cells: crate::cell_paint::CellPaint,
    explicit: crate::explicit_tiles::ExplicitPanel,

    /// The `.terry` scene drawn and its creation modes.
    terry: crate::terry_view::TerryView,
    layout: crate::layout_view::LayoutView,

    /// The last checks of the shown map.
    findings: Vec<crate::validate::Finding>,
    refill: crate::refill::Refill,
    refill_wanted: bool,
    painting: Option<Painting>,

    /// The shown map's fields mirrored into its view (a kit map's land and sea LF on the view's lattices), and the view's sea LF.
    mirror: Option<Arc<LfMirror>>,
    sea_mirror: Option<Arc<LfMirror>>,
    sea_lf: Option<Arc<wce_scene::render_scene::LfMap>>,
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
    db: Option<(Vec<PathBuf>, Arc<TileDatabase>)>,
    campaign_db: Option<(Vec<PathBuf>, Arc<TileDatabase>)>,

    /// The export on its way, and the last one done (what Launch in game ships).
    export: Option<crate::export::ExportJob>,
    exported: Option<crate::export::Exported>,

    /// A launch waiting for its export (the BCA, dry run, ignore movie packs), and the game launched.
    launch_after_export: Option<(crate::launch::Spot, bool, bool)>,
    launched: Option<wce_tile_tool::launch::Running>,
}

impl Default for TerrainTool {
    fn default() -> Self {
        Self {
            maps: vec![], map: 0, maps_for: vec![], rings: Rings::default(), overview: true, map_panel: crate::map_panel::MapPanel::new(), paint: LfPaint::default(), cells: crate::cell_paint::CellPaint::default(), explicit: Default::default(), terry: Default::default(), layout: Default::default(), findings: vec![], refill: Default::default(), refill_wanted: false, painting: None, mirror: None, sea_mirror: None, sea_lf: None, ring: (0, 0), pick: None, cursor: None, view: MapView::default(), shown: 0, building: 0, cameras: std::collections::HashMap::new(), rx: None, busy: String::new(), busy_since: None,
            status: String::new(), packs: None, provider: None, db: None, campaign_db: None, export: None, exported: None, launch_after_export: None, launched: None,
        }
    }
}

/// The `raw_data` folders a placement's tile and a kit map are looked for in, in order: the projects' kit layout, then the Assembly Kits in
/// the settings' order (`wce_tile::sources`).
pub(crate) fn kit_roots(e: &Editor) -> Vec<PathBuf> {
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
    Some(kit_map_dir(&e.settings_ref().kit_projects_dir().join("raw_data"), &d.name))
}

/// The kit with the battle tile database (its `raw_data`).
fn database_root(e: &Editor) -> Option<PathBuf> {
    // A whole kit first (battle and campaign databases: decompiling a pack's tile reads both); a project with only a battle database
    // copied into it would make every pack tile fail.
    let roots = kit_roots(e);
    let has = |r: &PathBuf, kind: &str| r.join(format!("terrain/tiles/{kind}/_tile_database")).is_dir();
    roots.iter().find(|r| has(r, "battle") && has(r, "campaign")).or_else(|| roots.iter().find(|r| has(r, "battle"))).cloned()
}

/// The land nearest the map's centre (a sample above the sea, on a grid of about 256 a side), where a map's view starts: the campaign
/// map's centre is the Aegean, a view of fog and water. None for a map all under the sea.
fn land_near_centre(lf: &wce_scene::render_scene::LfMap) -> Option<Float3> {
    const ABOVE_SEA: f32 = 20.0;
    let v = lf.values.read();
    let stride = (lf.nx.max(lf.nz) / 256).max(1);
    let (ci, cj) = (-lf.x0 / lf.step, -lf.z0 / lf.step);
    (0..lf.nz).step_by(stride as usize)
        .flat_map(|j| (0..lf.nx).step_by(stride as usize).map(move |i| (i, j)))
        .map(|(i, j)| (lf.x0 + i as f32 * lf.step, lf.z0 + j as f32 * lf.step, (i as f32 - ci).powi(2) + (j as f32 - cj).powi(2)))
        .map(|(x, z, d)| (x, z, d, lf.height_in(&v, x, z)))
        .filter(|&(.., h)| h > ABOVE_SEA)
        .min_by(|a, b| a.2.total_cmp(&b.2))
        .map(|(x, z, _, h)| Float3::new(x, h, z))
}

/// A model's field as the map context's (the same rows).
fn lf_field(f: &wce_terrain::paged::PagedU16) -> LfField {
    LfField { w: f.width(), h: f.height(), values: f.to_le_rows().as_chunks::<2>().0.iter().map(|c| u16::from_le_bytes(*c)).collect() }
}

/// Reads a map (a worker's job).
fn open_map(i: &OpenInputs) -> Result<Loaded, String> {
    let packs_folder = game_map_folder(&i.name, i.game_dir);
    let db = match (&packs_folder, &i.packs) {
        // A map of the packs names the packs' tiles (a mod's among them): read with their database.
        (Some(_), Some(p)) => p.raw().ok().and_then(|r| TileDatabase::load(&r.join("terrain/tiles/battle/_tile_database")).ok()).map_or_else(|| i.db.clone(), Arc::new),
        _ => i.db.clone(),
    };

    let (mut placed, locations, source, model, sea, messages) = match packs_folder {
        Some(folder) => {
            let (placed, locations) = load_map(&i.name, i.game_dir, &i.raw, &db, i.files.as_deref())?;
            let sea = i.files.as_ref().and_then(|f| f.read(&format!("{folder}/lf_sea_height_map.dds")).ok()).and_then(|b| l16_dds_heights(&b));
            (placed, locations, MapSource::Packs(folder), None, sea, vec![])
        }

        // A kit map: its files into the model, the model filled as BOB fills the files (the same fill runs again after an edit).
        None => {
            let dir = kit_map_dir(&i.raw, &i.name);
            let files = wce_terrain_io::MapFiles::from_dir(&dir).map_err(|e| e.to_string())?;
            let model = wce_terrain_io::attila::read(map_kind(&i.name).1, map_kind(&i.name).0, &files).map_err(|e| e.to_string())?;
            let inputs = crate::refill::FillInputs::of(&model).ok_or_else(|| format!("{}: no tile_map.png", dir.display()))?;
            let (placed, messages) = inputs.fill(&db, &i.name);
            let sea = model.lf(LfRole::Sea).map(|f| lf_field(f.get()));
            (Arc::new(placed), None, MapSource::Kit(dir), Some(model), sea, messages)
        }
    };

    // The sea tiles stand on the map's sea LF (the Tile tool's map context keeps them on a flat -20 m).
    Arc::make_mut(&mut placed).sea_lf = sea;

    // The LF's metres: the tile database's RENDER_PARAMS (the campaign's 712 and 38.28; a battle map keeps 3000 and 20).
    let kind = map_kind(&i.name).0;
    if kind == MapKind::Campaign
        && let Some((h, o)) = render_lf(&i.raw.join(database_dir(kind)).join("_settings.xml"))
    {
        let m = Arc::make_mut(&mut placed);
        (m.lf_height, m.vertical_offset) = (h, o);
    }

    Ok(Loaded { name: i.name.clone(), source, model, placed, locations, db, messages })
}

impl TerrainTool {
    /// The kits' tile databases of a kind of map (battle, campaign) as one.
    fn databases_of(&mut self, roots: &[PathBuf], kind: MapKind) -> Result<Arc<TileDatabase>, String> {
        let slot = if kind == MapKind::Campaign { &mut self.campaign_db } else { &mut self.db };
        if let Some((r, db)) = slot.as_ref()
            && r == roots
        {
            return Ok(db.clone());
        }

        let folders: Vec<PathBuf> = roots.iter().map(|r| r.join(database_dir(kind))).filter(|f| f.is_dir()).collect();
        if folders.is_empty() {
            return Err(format!("no {} in the kits", database_dir(kind)));
        }

        let db = Arc::new(TileDatabase::load_layered(&folders.iter().map(PathBuf::as_path).collect::<Vec<_>>())?);
        *slot = Some((roots.to_vec(), db.clone()));
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
            self.maps.extend(game_maps(f.as_ref(), e.game().campaign_battle_map, !e.game().features.kit_tiles));
        }

        for r in &roots {
            for m in kit_maps(r).into_iter().chain(kit_campaign_maps(r)) {
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
        self.prepare_packs(e);
        // A game without kit maps (Rome 2): the maps of its packs, with the packs' tile database (`PacksTree::raw`).
        if !e.game().features.kit_tiles {
            if game_map_folder(name, e.game().campaign_battle_map).is_none() {
                return Err(format!("{}: no kit maps; open one of the packs' (game files)", e.game().display_name));
            }

            let raw = self.packs.as_ref().ok_or("no game files")?.raw()?;
            let db = self.databases_of(std::slice::from_ref(&raw), map_kind(name).0)?;
            return Ok(OpenInputs { name: name.to_owned(), game_dir: e.game().campaign_battle_map, raw, db, files: e.files(), packs: self.packs.clone() });
        }

        let roots = kit_roots(e);
        let raw = roots.iter().find(|r| kit_map_dir(r, name).join("tile_map.png").is_file()).cloned()
            .or_else(|| database_root(e))
            .ok_or("no Assembly Kit with a battle tile database in the settings")?;
        // Every kit's tiles (a project's few and the Assembly Kit's): the project's database alone misses the map's tiles.
        let db = self.databases_of(&roots, map_kind(name).0)?;
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
            near: self.rings.near, mid: self.rings.mid, far: self.view.far_ring(self.rings.far),
            raw_root: database_root(e).or_else(|| self.packs.as_ref().and_then(|p| p.raw().ok()))?, tile_roots: roots,
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

    /// A view's frame made: the view on the scene, the camera where it was on this map (else over the land nearest its centre).
    fn show(&mut self, e: &mut Editor, id: DocumentId, s: Box<Streamer>) {
        if self.shown != 0 {
            self.cameras.insert(self.shown, e.viewport.camera());
        }

        self.light(e, id);
        let lift = s.frame().lift(0.0, 0.0);
        let start = s.frame().lf.as_ref().and_then(|lf| land_near_centre(lf));
        self.pick = s.frame().lf.clone().map(LfPick::new);
        self.cursor = None;
        self.painting = None;
        self.sea_lf = s.frame().sea_lf.clone();
        let doc = e.docs.find_as::<MapDocument>(id);
        let mirror = |role: LfRole, lf: Option<&Arc<wce_scene::render_scene::LfMap>>| doc.and_then(|d| d.lf_field(role))
            .zip(lf)
            .and_then(|(field, lf)| LfMirror::of(field, lf.values.clone(), lf.nx, lf.nz))
            .map(Arc::new);
        self.mirror = mirror(LfRole::Land, self.pick.as_ref().map(|p| &p.lf));
        self.sea_mirror = mirror(LfRole::Sea, self.sea_lf.as_ref());
        self.view.begin(e, id, s, &self.rings, |_| vec![]);
        self.shown = id;
        self.building = 0;
        let cam = self.cameras.get(&id).copied().unwrap_or_else(|| {
            let target = start.unwrap_or(Float3::new(0.0, lift, 0.0));
            wce_scene::camera::Camera { eye: target + Float3::new(0.0, 1200.0, -900.0), target, ..e.viewport.camera() }
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

        if let Some(t) = self.refill.elapsed() {
            gui::text_colored([1.0, 0.8, 0.3, 1.0], &format!("filling the map with tiles again... {t:.0} s"));
        }

        let lf_was = self.paint.on;
        self.draw_paint_section(e);
        let cells_was = self.cells.on;
        self.cells.draw_section(e, self.shown);
        let placing_was = self.explicit.placing;
        self.explicit.draw_section(e, self.shown);
        let terry_was = self.terry.claims_left_button();
        self.terry.draw_section(e, self.shown);
        self.layout.draw_controls(e, self.shown);
        self.draw_validation(e);
        self.draw_export(e);

        // One left-button mode at a time: the one just turned on.
        if self.paint.on && !lf_was {
            (self.cells.on, self.explicit.placing) = (false, false);
        } else if self.cells.on && !cells_was {
            (self.paint.on, self.explicit.placing) = (false, false);
        } else if self.explicit.placing && !placing_was {
            (self.paint.on, self.cells.on) = (false, false);
        }

        if self.terry.claims_left_button() && !terry_was {
            (self.paint.on, self.cells.on, self.explicit.placing) = (false, false, false);
        } else if self.paint.on || self.cells.on || self.explicit.placing {
            self.terry.mode = crate::terry_view::Mode::Select;
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
        let metres = f64::from(raw) / 65535.0 * f.map.lf_height - f.map.vertical_offset;
        Some(format!("LF under the cursor: cell {cx:.2}, {cy:.2}, {metres:.2} m (raw {raw:.1})"))
    }
}

impl TerrainTool {
    /// The field the brush paints.
    fn role(&self) -> LfRole {
        if self.paint.sea { LfRole::Sea } else { LfRole::Land }
    }

    /// The view's LF of the brush's field, and its mirror.
    fn target_lf(&self) -> Option<&Arc<wce_scene::render_scene::LfMap>> {
        if self.paint.sea { self.sea_lf.as_ref() } else { self.pick.as_ref().map(|p| &p.lf) }
    }

    fn target_mirror(&self) -> Option<Arc<LfMirror>> {
        if self.paint.sea { self.sea_mirror.clone() } else { self.mirror.clone() }
    }

    /// Metres between two samples of the brush's field.
    fn sample_metres(&self) -> f64 {
        self.target_lf().map_or(16.0, |lf| f64::from(lf.step) / UNIT)
    }

    /// The sample of the brush's field (x along the columns, y down the rows of the file) under a frame point.
    fn field_xy(&self, p: Float3) -> Option<(f64, f64)> {
        let lf = self.target_lf()?;
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

    /// The operation of a dab (raw units of the brush's field: the sea LF spans 400 m, the land's its map's: 3000 m, 712 on the campaign).
    fn op(&self, target: f64) -> LfOp {
        use wce_tile::battle_map::context::{LF_HEIGHT, SEA_LF_HEIGHT};
        let land = self.view.frame().map_or(LF_HEIGHT, |f| f.map.lf_height);
        let raw = f64::from(self.paint.amount_m) / if self.paint.sea { SEA_LF_HEIGHT } else { land } * 65535.0;
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
        let role = self.role();
        let Some(field) = d.lf_field(role) else {
            let why = if d.model().is_none() { "this map is from the game's packs: open the kit's map to paint its LF" } else { "this map has no lf_sea_heights.tif" };
            e.status(why, true);
            return false;
        };

        let mirror = self.target_mirror();
        let target = at.map_or(0.0, |(x, y)| f64::from(field.at((x.round().max(0.0) as u32).min(field.width() - 1), (y.round().max(0.0) as u32).min(field.height() - 1))));
        self.painting = Some(Painting { doc: self.shown, role, stroke: LfStroke::default(), mirror, target, last: None });
        true
    }

    /// A dab at a field sample: the model's field, the view (mirror, picking).
    fn dab(&mut self, e: &mut Editor, cx: f64, cy: f64) {
        let b = self.brush_at(cx, cy);
        let Some(target) = self.painting.as_ref().map(|p| p.target) else { return };
        let op = self.op(target);
        let Some(paint) = self.painting.as_mut() else { return };
        let Some(d) = e.docs.find_as_mut::<MapDocument>(paint.doc) else { return };
        let Some(field) = d.lf_field_mut(paint.role) else { return };
        let Some(rect) = paint.stroke.dab(field, &b, op) else { return };
        if let Some(m) = &paint.mirror {
            m.copy(field, rect);
            if paint.role == LfRole::Land
                && let Some(pick) = self.pick.as_mut()
            {
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

        for (m, land) in [(self.mirror.clone(), true), (self.sea_mirror.clone(), false)] {
            let Some(m) = m else { continue };
            let Some(rect) = m.take_touched() else { continue };
            if land && let Some(pick) = self.pick.as_mut() {
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
    }

    /// Dabs from the last one towards a field sample, `spacing` of the radius apart (the narrow side of an elongated brush); the first
    /// of a stroke where it is.
    fn dab_to(&mut self, e: &mut Editor, x: f64, y: f64) {
        let Some(last) = self.painting.as_ref().map(|p| p.last) else { return };
        let Some((mut lx, mut ly)) = last else {
            self.dab(e, x, y);
            if let Some(p) = self.painting.as_mut() {
                p.last = Some((x, y));
            }

            return;
        };

        let b = self.brush_at(x, y);
        let step = (b.brush.radius * b.aspect.min(1.0) * f64::from(self.paint.spacing)).max(0.5);
        let (dx, dy) = (x - lx, y - ly);
        let len = dx.hypot(dy);
        let n = (len / step).floor() as usize;
        for _ in 0..n {
            (lx, ly) = (lx + dx / len * step, ly + dy / len * step);
            self.dab(e, lx, ly);
        }

        if let Some(p) = self.painting.as_mut() {
            p.last = Some((lx, ly));
        }
    }

    /// Terry's "Apply Per-Tile LF Patches" on the shown map (`wce_terrain::lf_patch`): every placement whose variation folder has an
    /// `lf_mask.tif` (the custom-mesh cliffs and the `sea` set, whose masks are all 0; looked for in the kits' order) lays it into the land
    /// LF, one step on the history. No kit map has them applied (Attila's 113: every low sample changes).
    fn apply_lf_patches(&mut self, e: &mut Editor) -> Result<String, String> {
        let roots = kit_roots(e);
        let mirror = self.mirror.clone();
        let d = e.docs.find_as_mut::<MapDocument>(self.shown).ok_or("no map shown")?;
        let placed = d.placed.clone();
        let field = d.lf_field_mut(LfRole::Land).ok_or("this map is from the game's packs: open the kit's map to patch its LF")?;
        let per_cell = field.width() / placed.width.max(1);
        let mut masks: std::collections::HashMap<(String, String), Option<lf_patch::LfPatch>> = std::collections::HashMap::new();
        let mut stroke = LfStroke::default();
        let (mut laid, mut outside, mut written, mut changed) = (0usize, 0usize, 0usize, 0usize);
        for p in &placed.placements {
            let mask = masks.entry((p.location.clone(), p.variation.clone())).or_insert_with(|| {
                let rel = format!("{}/{}/lf_mask.tif", p.location.replace('\\', "/"), p.variation);
                roots.iter().find_map(|r| std::fs::read(r.join(&rel)).ok())
                    .and_then(|b| wce_formats::image_containers::TiffFile::parse(&b).ok())
                    .filter(|t| t.bits_per_sample == 16 && t.samples_per_pixel == 1)
                    .map(|t| lf_patch::LfPatch { w: t.width, h: t.height, values: t.pixels.as_chunks::<2>().0.iter().map(|c| u16::from_le_bytes(*c)).collect() })
            });

            let Some(mask) = mask else { continue };
            match lf_patch::apply(&mut stroke, field, mask, (p.x, p.y), p.rotation, per_cell) {
                Some((w, c)) => (laid, written, changed) = (laid + 1, written + w, changed + c),
                None => outside += 1,
            }
        }

        let tiles = masks.values().filter(|m| m.is_some()).count();
        let Some(record) = stroke.finish(field) else {
            return Ok(format!("{laid} placements of {tiles} tiles with an lf_mask.tif ({outside} off the map): the LF already had them"));
        };

        if let Some(m) = &mirror {
            m.copy(field, record.rect);
        }

        let kb = record.bytes() / 1024;
        d.push_stroke(LfStrokeCommand { role: LfRole::Land, record: Arc::new(record), mirror });
        Ok(format!("{laid} placements of {tiles} tiles with an lf_mask.tif patched ({outside} off the map): {changed} of their {written} low samples changed, the record keeps {kb} KB"))
    }

    /// The checks of the shown map, again.
    fn validate(&mut self, e: &Editor) -> String {
        let roots = kit_roots(e);
        self.findings = e.docs.find_as::<MapDocument>(self.shown).map(|d| crate::validate::check(d, &roots)).unwrap_or_default();
        let errors = self.findings.iter().filter(|f| f.severity == crate::validate::Severity::Error).count();
        format!("{errors} errors, {} warnings", self.findings.len() - errors)
    }

    /// The camera over a map cell.
    fn look_at_cell(&self, e: &mut Editor, at: (i32, i32)) {
        let Some(f) = self.view.frame() else { return };
        let (fx, fz) = f.from_map.apply(f64::from(at.0) + 0.5, f64::from(at.1) + 0.5);
        let cam = e.viewport.camera();
        let target = Float3::new(fx as f32, f.lift(fx, fz), fz as f32);
        e.viewport.controller().set_camera(&wce_scene::camera::Camera { eye: target + Float3::new(0.0, 400.0, -300.0), target, ..cam });
    }

    fn draw_validation(&mut self, e: &mut Editor) {
        if !gui::collapsing_header("Validation", 0) {
            return;
        }

        if gui::button("Check the map") {
            let m = self.validate(e);
            e.status(&format!("Terrain: {m}"), false);
        }

        gui::same_line();
        gui::text_disabled("colours in no database, cells no tile fits, explicit tiles, tiles in no kit");
        let mut go = None;
        for (i, f) in self.findings.iter().enumerate() {
            let col = if f.severity == crate::validate::Severity::Error { [1.0, 0.45, 0.4, 1.0] } else { [1.0, 0.8, 0.3, 1.0] };
            gui::push_id_int(i as i32);
            gui::push_style_color(gui::COL_TEXT as i32, gui::v4(col[0], col[1], col[2], col[3]));
            if gui::selectable(&f.text, false, 0) {
                go = f.cell;
            }

            gui::pop_style_color(1);
            gui::pop_id();
        }

        if let Some(at) = go {
            self.look_at_cell(e, at);
        }
    }

    /// The cell paint on the 3D view's ground: the cell under the cursor.
    fn cell_input(&mut self, e: &mut Editor, kind: wce_editor::tool::ViewportEventKind) -> bool {
        use wce_editor::tool::ViewportEventKind as K;
        let cell = self.cursor.zip(self.view.frame()).map(|((p, _), f)| {
            let (x, y) = f.cell_of(f64::from(p.x), f64::from(p.z));
            (x.floor() as i64, y.floor() as i64)
        });

        match kind {
            K::Press => cell.is_some_and(|c| self.cells.press(e, self.shown, c)),
            K::Drag => {
                if let Some(c) = cell {
                    self.cells.drag(e, c);
                }

                true
            }

            K::Release => {
                if let Some(m) = self.cells.release(e) {
                    e.status(&m, false);
                }

                true
            }

            _ => false,
        }
    }

    /// What the cell edits (strokes, undo, redo) changed: the Map panel's picture (and its copy on the ground once no stroke is painted).
    fn follow_cell_edits(&mut self, e: &mut Editor) {
        let Some(d) = e.docs.find_as::<MapDocument>(self.shown) else { return };
        let touched = d.cells.take();
        let fill_reads = |k: usize| k == wce_terrain_editor::cells::EXPLICIT
            || d.model().and_then(|m| m.layers.get(k)).is_some_and(|(r, _)| matches!(r, wce_terrain::LayerRole::TileMap | wce_terrain::LayerRole::Climate));
        self.refill_wanted |= touched.iter().any(|(k, _)| fill_reads(*k));
        let drape = !self.cells.painting();
        for (k, rect) in touched {
            self.map_panel.cells_changed(e, self.shown, k, rect, drape);
        }

        // The fill again once the stroke is done.
        if self.refill_wanted && !self.cells.painting()
            && let Some(frame) = self.view.frame()
            && let Some(d) = e.docs.find_as::<MapDocument>(self.shown)
        {
            self.refill_wanted = false;
            if self.refill.request(d, self.shown, frame) {
                e.status("Terrain: filling the map with tiles again (BOB's fill)...", false);
            }
        }
    }

    /// A fill done: the map's placements replaced, the chunks whose placements changed built again.
    fn follow_refill(&mut self, e: &mut Editor) {
        let Some(done) = self.refill.poll() else { return };
        if done.doc == self.shown
            && let Some(d) = e.docs.find_as_mut::<MapDocument>(done.doc)
        {
            d.placed = done.map.clone();
            d.fill_messages.clone_from(&done.messages);
            if let Some(s) = self.view.streamer.as_mut() {
                s.swap_frame(done.frame.clone(), &done.changed);
            }

            self.map_panel.placements_changed();
            let failed = if done.messages.is_empty() { String::new() } else { format!(", {} cells with no tile that fits", done.messages.len()) };
            let m = format!("Terrain: filled again in {:.1} s: {} placements, {} chunks changed{failed}", done.secs, done.map.placements.len(), done.changed.len());
            self.status = m.clone();
            e.status(&m, false);
        }

        if self.refill.again
            && let Some(frame) = self.view.frame()
            && let Some(d) = e.docs.find_as::<MapDocument>(self.shown)
        {
            self.refill.again = false;
            self.refill.request(d, self.shown, frame);
        }
    }

    /// The stroke onto the document's history.
    fn end_stroke(&mut self, e: &mut Editor) -> Option<usize> {
        let paint = self.painting.take()?;
        let d = e.docs.find_as_mut::<MapDocument>(paint.doc)?;
        let record = paint.stroke.finish(d.lf_field(paint.role)?)?;
        let bytes = record.bytes();
        d.push_stroke(LfStrokeCommand { role: paint.role, record: Arc::new(record), mirror: paint.mirror });
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
            let lf = self.target_lf()?;
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

    /// Export starts a build of the shown map on a worker (one at a time).
    pub(crate) fn start_export(&mut self, e: &mut Editor) -> Result<String, String> {
        if self.export.is_some() {
            return Err("an export is on its way".to_owned());
        }

        let d = e.docs.find_as::<MapDocument>(self.shown).ok_or("no map shown")?;
        let db = d.db.clone();
        let job = crate::export::start(e, self.shown, db, &kit_roots(e))?;
        let m = format!("exporting {}...", job.map);
        self.export = Some(job);
        Ok(m)
    }

    /// Launch in game at a BCA (`id`, else the selected one, else the first): with the shown map's last export when it is of this map and
    /// the map has not changed since, else after an export.
    pub(crate) fn launch(&mut self, e: &mut Editor, id: Option<u64>, dry_run: bool, ignore_movie_packs: bool) -> Result<String, String> {
        if self.launched.is_some() && !dry_run {
            return Err("the game launched before is still running".to_owned());
        }

        let name = e.docs.find_as::<MapDocument>(self.shown).ok_or("no map shown")?.name.clone();
        if map_kind(&name).0 == MapKind::Campaign {
            return Err("Launch in game starts a battle: a campaign map's export is a mod pack to load in the launcher".to_owned());
        }

        let spot = crate::launch::spot(e, self.shown, id)?;
        let d = e.docs.find_as::<MapDocument>(self.shown).ok_or("no map shown")?;
        let fresh = self.exported.as_ref().is_some_and(|x| x.map == d.name && x.revision == wce_editor_core::document::Document::history(d).version());
        if !fresh {
            let m = self.start_export(e)?;
            self.launch_after_export = Some((spot, dry_run, ignore_movie_packs));
            return Ok(format!("{m} (then the launch)"));
        }

        self.launch_now(e, &spot, dry_run, ignore_movie_packs)
    }

    fn launch_now(&mut self, e: &mut Editor, spot: &crate::launch::Spot, dry_run: bool, ignore_movie_packs: bool) -> Result<String, String> {
        let x = self.exported.clone().ok_or("no export")?;
        let d = e.docs.find_as::<MapDocument>(self.shown).ok_or("no map shown")?;
        let (w, h) = (d.placed.width, d.placed.height);
        let p = crate::launch::prepare(e, &x, w, h, spot, dry_run, ignore_movie_packs)?;
        let at = format!("{} ({}) at cell {:.0}, {:.0}", spot.name, spot.kind, spot.cell.0, spot.cell.1);
        if dry_run {
            let xml = e.output_dir().join("dry_wce_launch.xml");
            let _ = std::fs::write(&xml, &p.xml);
            return Ok(format!("dry run: {} at {at}: the pack {}, the battle {}; command line: {}", x.map, p.pack.display(), xml.display(), p.command));
        }

        let (running, started) = crate::launch::start(e, &p)?;
        self.launched = Some(running);
        Ok(format!("launched {started} into a battle on {} at {at}; the launch pack is removed when the game exits", x.map))
    }

    /// The export, when it is done: its result for the status bar (and the launch waiting for it).
    fn follow_export(&mut self, e: &mut Editor) -> Option<Result<String, String>> {
        let r = self.export.as_ref()?.poll()?;
        self.export = None;
        let r = r.map(|x| {
            let m = crate::export::summary(&x);
            self.exported = Some(x);
            m
        });

        if let Some((spot, dry, ignore)) = self.launch_after_export.take() {
            let r = r.and_then(|m| self.launch_now(e, &spot, dry, ignore).map(|l| format!("{m}; {l}")));
            match &r {
                Ok(m) => e.status(m, false),
                Err(err) => e.status(&format!("launch in game: {err}"), true),
            }

            return Some(r);
        }

        match &r {
            Ok(m) => e.status(m, false),
            Err(err) => e.status(&format!("export: {err}"), true),
        }

        Some(r)
    }

    fn draw_export(&mut self, e: &mut Editor) {
        if !gui::collapsing_header("Export", 0) {
            return;
        }

        let can = e.docs.find_as::<MapDocument>(self.shown).is_some_and(|d| d.model().is_some());
        if let Some(j) = &self.export {
            gui::text_colored([1.0, 0.8, 0.3, 1.0], &format!("exporting {}... {:.0} s", j.map, j.started.elapsed().as_secs_f64()));
        }

        gui::begin_disabled(!can || self.export.is_some());
        if gui::button("Export built files") {
            match self.start_export(e) {
                Ok(m) => e.status(&m, false),
                Err(err) => e.status(&err, true),
            }
        }

        gui::end_disabled();
        if gui::is_item_hovered_allow_disabled() {
            gui::set_tooltip("What BOB_Terrain builds (tile map, battle field, LF heights, locations.blm and its masks, deployment files), made from the map as it is, loose under the output folder and as a mod pack");
        }

        gui::same_line();
        gui::begin_disabled(!can || self.export.is_some() || self.launched.is_some());
        if gui::button("Launch in game") {
            match self.launch(e, None, false, false) {
                Ok(m) => e.status(&m, false),
                Err(err) => e.status(&format!("launch in game: {err}"), true),
            }
        }

        gui::end_disabled();
        if gui::is_item_hovered_allow_disabled() {
            gui::set_tooltip("The export in the launch pack, a battle at the selected battle catchment area (else the first): one unit a side");
        }

        if !can {
            gui::text_disabled("Open a kit map (not one of the game's packs) to export it.");
        } else if let Some(x) = &self.exported {
            gui::text_disabled(&format!("last: {} files, {}", x.files.len(), x.pack.display()));
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

        if gui::button("Apply per-tile LF patches") {
            match self.apply_lf_patches(e) {
                Ok(m) => e.status(&format!("LF patches: {m}"), false),
                Err(err) => e.status(&err, true),
            }
        }

        gui::same_line();
        gui::text_disabled("the cliff and sea tiles' lf_mask.tif into the land LF");
        gui::checkbox("paint (left button)", &mut self.paint.on);
        let mut field = i32::from(self.paint.sea);
        if gui::combo("field", &mut field, &["land LF", "sea LF (under the sea tiles)"]) {
            self.paint.sea = field == 1;
        }

        if self.paint.sea && self.sea_mirror.is_none() {
            gui::text_disabled("This map has no sea LF to paint (no lf_sea_heights.tif).");
        }

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
        gui::slider_float("spacing (of the radius)", &mut self.paint.spacing, 0.05, 1.0, "%.2f");
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
        self.map_panel.paint = self.cells.on || self.explicit.placing || self.terry.claims_left_button();
        self.map_panel.outline = self.cells.dragging;
        self.map_panel.marks = e.docs.find_as::<MapDocument>(self.shown).map(|d| self.explicit.marks(d)).unwrap_or_default();
        self.map_panel.draw(e, self.shown, &self.view);

        // A `.terry` creation click in the Map panel (the cell's centre).
        if self.terry.claims_left_button()
            && let Some(i) = self.map_panel.input.filter(|i| i.pressed)
            && let Some(d) = e.docs.find_as::<MapDocument>(self.shown)
        {
            let (x, z) = d.terry_frame.of_cell(i.cell.0 as f64 + 0.5, i.cell.1 as f64 + 0.5, d.placed.width, d.placed.height);
            let at = [x, z];
            if let Some(m) = self.terry.click(e, self.shown, at) {
                e.status(&m, false);
            }
        }

        // Placing an explicit tile in the Map panel.
        if self.explicit.placing
            && let Some(i) = self.map_panel.input
        {
            self.explicit.preview = e.docs.find_as::<MapDocument>(self.shown).and_then(|d| self.explicit.fits(d, i.cell));
            if i.pressed {
                match self.explicit.place(e, self.shown, i.cell) {
                    Ok(m) => e.status(&m, false),
                    Err(err) => e.status(&err, true),
                }
            }
        }

        // The cell paint in the Map panel.
        if self.cells.on
            && let Some(i) = self.map_panel.input
        {
            if i.pressed {
                self.cells.press(e, self.shown, i.cell);
            } else if i.down && self.cells.painting() {
                self.cells.drag(e, i.cell);
            } else if i.released && let Some(m) = self.cells.release(e) {
                e.status(&m, false);
            }
        }
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
        if self.cells.on && ev.button == 0 {
            return self.cell_input(e, ev.kind);
        }

        // A `.terry` creation click on the ground.
        if self.terry.claims_left_button() && ev.button == 0 {
            if ev.kind == wce_editor::tool::ViewportEventKind::Press {
                if let Some((p, _)) = self.cursor
                    && let Some(d) = e.docs.find_as::<MapDocument>(self.shown)
                {
                    let (x, z) = d.terry_frame.from_frame(f64::from(p.x), f64::from(p.z));
                    let at = [x, z];
                    if let Some(m) = self.terry.click(e, self.shown, at) {
                        e.status(&m, false);
                    }
                }

                return true;
            }

            return matches!(ev.kind, wce_editor::tool::ViewportEventKind::Drag | wce_editor::tool::ViewportEventKind::Release);
        }

        if self.explicit.placing {
            let cell = self.cursor.zip(self.view.frame()).map(|((p, _), f)| {
                let (x, y) = f.cell_of(f64::from(p.x), f64::from(p.z));
                (x.floor() as i64, y.floor() as i64)
            });

            self.explicit.preview = cell.and_then(|c| e.docs.find_as::<MapDocument>(self.shown).and_then(|d| self.explicit.fits(d, c)));
            if ev.button == 0 && ev.kind == wce_editor::tool::ViewportEventKind::Press {
                if let Some(c) = cell {
                    match self.explicit.place(e, self.shown, c) {
                        Ok(m) => e.status(&m, false),
                        Err(err) => e.status(&err, true),
                    }
                }

                return true;
            }

            return false;
        }

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
                    self.dab_to(e, x, y);
                }

                true
            }

            K::Drag => {
                if let Some((x, y)) = at.filter(|_| self.painting.is_some()) {
                    self.dab_to(e, x, y);
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
        self.paint.on || self.cells.on || self.explicit.placing || self.terry.claims_left_button()
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
        self.follow_cell_edits(e);
        self.follow_refill(e);
        self.follow_export(e);
        if let Some(m) = self.launched.as_mut().and_then(wce_tile_tool::launch::Running::poll) {
            self.launched = None;
            e.status(&format!("Launch in game: {m}"), false);
        }

        let frame = self.view.frame().filter(|_| self.shown != 0);
        self.terry.update(e, self.shown, frame.clone());
        self.layout.update(e, self.shown, frame);
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
                let failed = t.view.streamer.as_ref().map(|s| s.failed()).unwrap_or_default();
                let note = if failed.is_empty() { String::new() } else { format!("; could not open: {}", failed.iter().take(4).cloned().collect::<Vec<_>>().join("; ")) };
                CommandResult::success(format!("{}{note}", t.view.status))
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

        c.add("terrain_paint", "terrain_paint [sea] <raise|lower|smooth|flatten|noise> <cell x> <cell y> <radius m> <amount m or strength> [dabs]", "the Terrain tool: one LF stroke of dabs at a map cell (flatten: to the height there), onto the history; `sea`: on the sea LF", |e, a| {
            let usage = "usage: terrain_paint [sea] <raise|lower|smooth|flatten|noise> <cell x> <cell y> <radius m> <amount m or strength> [dabs]";
            let sea = a.first().is_some_and(|v| v == "sea");
            let a = if sea { &a[1..] } else { a };
            let Some(op) = a.first().and_then(|o| LF_OPS.iter().position(|n| n == o)) else { return CommandResult::fail(usage) };
            let nums: Vec<f64> = a[1..].iter().filter_map(|v| v.parse().ok()).collect();
            let [cx, cy, radius, amount, ..] = nums[..] else { return CommandResult::fail(usage) };
            let dabs = nums.get(4).map_or(1, |&n| n.max(1.0) as usize);
            with_terrain(e, |t, e| {
                let Some(d) = e.docs.find_as::<MapDocument>(t.shown) else { return CommandResult::fail("no map shown") };
                let Some(field) = d.lf_field(if sea { LfRole::Sea } else { LfRole::Land }) else { return CommandResult::fail("the shown map has no such LF to paint (a map of the packs, or no sea LF)") };
                let (ppc_x, ppc_y) = (f64::from(field.width()) / f64::from(d.placed.width.max(1)), f64::from(field.height()) / f64::from(d.placed.height.max(1)));
                let (x, y) = (cx * ppc_x, cy * ppc_y);
                let saved = t.paint.clone();
                t.paint.op = op as i32;
                t.paint.sea = sea;
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

        c.add("terrain_stroke", "terrain_stroke <raise|lower|smooth|flatten|noise> <x0> <y0> <x1> <y1> <radius m> <amount m or strength> [events]", "the Terrain tool: a land LF stroke along a line of map cells as the mouse paints it, its moves `events` (default 2) along the line; reports the field's sum", |e, a| {
            let usage = "usage: terrain_stroke <raise|lower|smooth|flatten|noise> <x0> <y0> <x1> <y1> <radius m> <amount m or strength> [events]";
            let Some(op) = a.first().and_then(|o| LF_OPS.iter().position(|n| n == o)) else { return CommandResult::fail(usage) };
            let nums: Vec<f64> = a[1..].iter().filter_map(|v| v.parse().ok()).collect();
            let [x0, y0, x1, y1, radius, amount, ..] = nums[..] else { return CommandResult::fail(usage) };
            let events = nums.get(6).map_or(2, |&n| n.max(2.0) as usize);
            with_terrain(e, |t, e| {
                let Some(d) = e.docs.find_as::<MapDocument>(t.shown) else { return CommandResult::fail("no map shown") };
                let Some(field) = d.lf_field(LfRole::Land) else { return CommandResult::fail("the shown map has no LF to paint (a map of the packs)") };
                let (ppc_x, ppc_y) = (f64::from(field.width()) / f64::from(d.placed.width.max(1)), f64::from(field.height()) / f64::from(d.placed.height.max(1)));
                let saved = t.paint.clone();
                t.paint.op = op as i32;
                t.paint.sea = false;
                t.paint.radius_m = radius as f32;
                if matches!(op, 2 | 3) { t.paint.share = amount as f32 } else { t.paint.amount_m = amount as f32 }
                if !t.begin_stroke(e, Some((x0 * ppc_x, y0 * ppc_y))) {
                    t.paint = saved;
                    return CommandResult::fail("no stroke");
                }

                for k in 0..events {
                    let f = k as f64 / (events - 1) as f64;
                    t.dab_to(e, (x0 + (x1 - x0) * f) * ppc_x, (y0 + (y1 - y0) * f) * ppc_y);
                }

                let r = t.end_stroke(e);
                t.paint = saved;
                let sum: u64 = e.docs.find_as::<MapDocument>(t.shown).and_then(|d| d.lf_field(LfRole::Land)).map_or(0, |f| f.to_le_rows().as_chunks::<2>().0.iter().map(|c| u64::from(u16::from_le_bytes(*c))).sum());
                match r {
                    Some(bytes) => CommandResult::success(format!("{} along ({x0}, {y0}) .. ({x1}, {y1}), {events} moves: the record keeps {} KB, the field's sum {sum}", LF_OPS[op], bytes / 1024)),
                    None => CommandResult::fail("the stroke changed nothing"),
                }
            })
        });

        c.add("terrain_lf_patches", "terrain_lf_patches", "the Terrain tool: Terry's Apply Per-Tile LF Patches on the shown map (the custom-mesh tiles' lf_mask.tif into the land LF)", |e, _| {
            with_terrain(e, |t, e| match t.apply_lf_patches(e) {
                Ok(m) => CommandResult::success(m),
                Err(err) => CommandResult::fail(err),
            })
        });

        c.add("terrain_cells", "terrain_cells <layer index | tile_map | climate> brush <x> <y> <radius> <r> <g> <b> | rect <x0> <y0> <x1> <y1> <r> <g> <b> | fill <x> <y> <r> <g> <b>", "the Terrain tool: one cell stroke on a kit map's layer (its index in the Cell layers list; rect corners inclusive), onto the history", |e, a| {
            let usage = "usage: terrain_cells <layer> brush <x> <y> <radius> <r> <g> <b> | rect <x0> <y0> <x1> <y1> <r> <g> <b> | fill <x> <y> <r> <g> <b>";
            let (Some(layer), Some(mode)) = (a.first().cloned(), a.get(1)) else { return CommandResult::fail(usage) };
            let n: Vec<i64> = a[2..].iter().filter_map(|v| v.parse().ok()).collect();
            let (mode, from, to, radius, colour) = match (mode.as_str(), &n[..]) {
                ("brush", [x, y, rad, r, g, b]) => (0, (*x, *y), (*x, *y), *rad, [*r, *g, *b]),
                ("rect", [x0, y0, x1, y1, r, g, b]) => (1, (*x0, *y0), (*x1, *y1), 0, [*r, *g, *b]),
                ("fill", [x, y, r, g, b]) => (2, (*x, *y), (*x, *y), 0, [*r, *g, *b]),
                _ => return CommandResult::fail(usage),
            };

            with_terrain(e, |t, e| {
                // A layer by index or role (tile_map, climate).
                let role = match layer.as_str() {
                    "tile_map" => Some(wce_terrain::LayerRole::TileMap),
                    "climate" => Some(wce_terrain::LayerRole::Climate),
                    _ => None,
                };

                let model = e.docs.find_as::<MapDocument>(t.shown).and_then(|d| d.model());
                let Some(layer) = layer.parse::<usize>().ok().or_else(|| model.and_then(|m| m.layers.iter().position(|(r, _)| Some(r) == role.as_ref()))) else {
                    return CommandResult::fail("no such layer on the shown map");
                };

                let saved = (t.cells.layer, t.cells.mode, t.cells.radius, t.cells.colour);
                (t.cells.layer, t.cells.mode, t.cells.radius) = (layer, mode, radius as i32);
                t.cells.colour = [colour[0] as u8, colour[1] as u8, colour[2] as u8, 255];
                let shown = t.shown;
                let r = if t.cells.press(e, shown, from) {
                    t.cells.drag(e, to);
                    t.cells.release(e)
                } else {
                    None
                };

                (t.cells.layer, t.cells.mode, t.cells.radius, t.cells.colour) = saved;
                match r {
                    Some(m) => CommandResult::success(m),
                    None => CommandResult::fail("no cells changed (no kit map, no such layer, or outside it)"),
                }
            })
        });

        c.add("terrain_export", "terrain_export [--wait]", "the Terrain tool: export the shown kit map's built files (loose under the output folder and as a pack); --wait: until done", |e, a| {
            let started = with_terrain(e, |t, e| match t.start_export(e) {
                Ok(m) => CommandResult::success(m),
                Err(err) => CommandResult::fail(err),
            });

            if !started.ok || !a.iter().any(|v| v == "--wait") {
                return started;
            }

            loop {
                let r = with_terrain(e, |t, e| match t.follow_export(e) {
                    Some(Ok(m)) => CommandResult::success(m),
                    Some(Err(err)) => CommandResult::fail(err),
                    None => CommandResult::success("busy"),
                });

                if !(r.ok && r.message == "busy") {
                    return r;
                }

                std::thread::sleep(std::time::Duration::from_millis(50));
            }
        });

        c.add("terrain_launch", "terrain_launch [<BCA id>] [--dry-run] [--ignore-movie-packs]", "the Terrain tool: Launch in game on the shown kit map at a battle catchment area (its id in hex; else the selected one, else the first), after an export when needed (waits for it)", |e, a| {
            let id = a.iter().find(|v| !v.starts_with("--")).and_then(|v| u64::from_str_radix(v, 16).ok());
            let (dry, ignore) = (a.iter().any(|v| v == "--dry-run"), a.iter().any(|v| v == "--ignore-movie-packs"));
            let r = with_terrain(e, |t, e| match t.launch(e, id, dry, ignore) {
                Ok(m) => CommandResult::success(m),
                Err(err) => CommandResult::fail(err),
            });

            if !r.ok || !r.message.ends_with("(then the launch)") {
                return r;
            }

            loop {
                let r = with_terrain(e, |t, e| match t.follow_export(e) {
                    Some(Ok(m)) => CommandResult::success(m),
                    Some(Err(err)) => CommandResult::fail(err),
                    None => CommandResult::success("busy"),
                });

                if !(r.ok && r.message == "busy") {
                    return r;
                }

                std::thread::sleep(std::time::Duration::from_millis(50));
            }
        });

        c.add("terrain_refill_wait", "terrain_refill_wait [seconds]", "the Terrain tool: waits for the fill run again after a cell edit (scripts)", |e, a| {
            let limit = a.first().and_then(|v| v.parse::<f64>().ok()).unwrap_or(600.0);
            let started = std::time::Instant::now();
            loop {
                let busy = with_terrain(e, |t, e| {
                    t.follow_cell_edits(e);
                    t.follow_refill(e);
                    CommandResult::success(if t.refill.busy() || t.refill_wanted { "busy" } else { "idle" })
                });

                if busy.message != "busy" {
                    let status = with_terrain(e, |t, _| CommandResult::success(t.status.clone()));
                    return CommandResult::success(format!("after {:.1} s: {}", started.elapsed().as_secs_f64(), status.message));
                }

                if started.elapsed().as_secs_f64() > limit {
                    return CommandResult::fail("the fill is still running");
                }

                std::thread::sleep(std::time::Duration::from_millis(50));
            }
        });

        c.add("terrain_explicit", "terrain_explicit add <x> <y> <tile or variation path> <degrees> | remove <i> | list", "the Terrain tool: the shown kit map's explicit tiles (one step on the history each)", |e, a| {
            let usage = "usage: terrain_explicit add <x> <y> <path> <degrees> | remove <i> | list";
            with_terrain(e, |t, e| {
                let shown = t.shown;
                match (a.first().map(String::as_str), &a.get(1..).unwrap_or_default()) {
                    (Some("add"), [x, y, path, deg]) => {
                        let (Ok(x), Ok(y), Ok(deg)) = (x.parse::<i64>(), y.parse::<i64>(), deg.parse::<i32>()) else { return CommandResult::fail(usage) };
                        t.explicit.choose(path, deg);
                        match t.explicit.place(e, shown, (x, y)) {
                            Ok(m) => CommandResult::success(m),
                            Err(err) => CommandResult::fail(err),
                        }
                    }

                    (Some("remove"), [i]) => match i.parse::<usize>() {
                        Ok(i) if crate::explicit_tiles::ExplicitPanel::remove(e, shown, i) => CommandResult::success(format!("explicit tile {i} removed")),
                        _ => CommandResult::fail("no such explicit tile"),
                    },

                    (Some("list"), []) => {
                        let d = e.docs.find_as::<MapDocument>(shown);
                        let lines: Vec<String> = d.and_then(|d| d.model()).and_then(|m| m.explicit.as_ref())
                            .map(|x| x.get().tiles().map(|t| format!("{},{},{},{}", t.x, t.y, t.path, t.rotation)).collect())
                            .unwrap_or_default();
                        CommandResult::success(format!("{} explicit tiles: {}", lines.len(), lines.join("; ")))
                    }

                    _ => CommandResult::fail(usage),
                }
            })
        });

        c.add("terrain_layout", "terrain_layout [on|off] [radius cells]", "the Terrain tool: the campaign layout guide (layout_map.png's legend colours outlined near the camera): set, then how many cells it drew", |e, a| {
            with_terrain(e, |t, _| {
                for v in a {
                    match v.as_str() {
                        "on" => t.layout.on = true,
                        "off" => t.layout.on = false,
                        n => {
                            if let Ok(r) = n.parse() {
                                t.layout.radius_cells = r;
                            }
                        }
                    }
                }

                CommandResult::success(format!("layout guide {}: {} cells within {} cells (as of the last frame)", if t.layout.on { "on" } else { "off" }, t.layout.cells_drawn, t.layout.radius_cells))
            })
        });

        c.add("terrain_validate", "terrain_validate", "the Terrain tool: the shown map's checks (the Validation section): a count, then each finding", |e, _| {
            with_terrain(e, |t, e| {
                let m = t.validate(e);
                let lines: Vec<String> = t.findings.iter().map(|f| format!("{:?}: {}", f.severity, f.text)).collect();
                CommandResult::success(format!("{m}{}{}", if lines.is_empty() { "" } else { "\n" }, lines.join("\n")))
            })
        });

        c.add("terrain_terry", "terrain_terry count | bca <x> <z> | prop <x> <z> <model> [scale] | vfx <x> <z> <effect> | ev <x> <z> <lighting> <inner> <outer> | generate | points <id> <x> <z> ... | zone <x> <z> <x> <z> <x> <z> ... | delete <id> | duplicate <id> | set <id> <component> <attr> <value> | move <id> <dx> <dz> | get <id>", "the Terrain tool: the shown kit map's .terry entities (metres from the map's centre, ids in hex; one step on the history each)", |e, a| {
            let usage = "usage: terrain_terry count | bca <x> <z> | prop <x> <z> <model> [scale] | vfx <x> <z> <effect> | ev <x> <z> <lighting> <inner> <outer> | generate | points <id> <x> <z> ... | zone <x> <z> ... | delete <id> | duplicate <id> | set <id> <component> <attr> <value> | move <id> <dx> <dz> | get <id>";
            let hex = |v: &String| u64::from_str_radix(v, 16).ok();
            let nums: Vec<f64> = a.iter().skip(1).filter_map(|v| v.parse().ok()).collect();
            with_terrain(e, |t, e| {
                let doc = t.shown;
                let r = match (a.first().map(String::as_str), a.len()) {
                    (Some("count"), 1) => {
                        let d = e.docs.find_as::<MapDocument>(doc);
                        Ok(d.and_then(|d| d.project()).map_or_else(|| "no .terry project".to_owned(), |p| {
                            let mut kinds: std::collections::BTreeMap<String, usize> = std::collections::BTreeMap::new();
                            for x in &p.entities {
                                *kinds.entry(p.kind(x).name()).or_default() += 1;
                            }

                            format!("{} entities: {kinds:?}", p.entities.len())
                        }))
                    }

                    (Some("bca"), 3) if nums.len() == 2 => crate::terry_view::new_bca(e, doc, [nums[0], nums[1]]),
                    (Some("prop"), 4 | 5) if a[1].parse::<f64>().is_ok() && a[2].parse::<f64>().is_ok() => {
                        let scale = a.get(4).and_then(|v| v.parse().ok()).unwrap_or(1.0);
                        crate::terry_view::new_prop(e, doc, [nums[0], nums[1]], &a[3], scale)
                    }

                    (Some("vfx"), 4) if a[1].parse::<f64>().is_ok() && a[2].parse::<f64>().is_ok() => crate::terry_view::new_vfx(e, doc, [nums[0], nums[1]], &a[3]),
                    (Some("ev"), 6) if a[1].parse::<f64>().is_ok() && a[2].parse::<f64>().is_ok() => match (a[4].parse(), a[5].parse()) {
                        (Ok(i), Ok(o)) => crate::terry_view::new_environment_volume(e, doc, [nums[0], nums[1]], &a[3], [i, o]),
                        _ => Err(usage.to_owned()),
                    },
                    (Some("generate"), 1) => crate::terry_view::generate_bcas(e, doc),
                    (Some("points"), n) if n >= 8 && n % 2 == 0 => match hex(&a[1]) {
                        Some(id) => {
                            let v: Vec<f64> = a[2..].iter().filter_map(|v| v.parse().ok()).collect();
                            let pts: Vec<[f64; 2]> = v.chunks(2).map(|c| [c[0], c[1]]).collect();
                            crate::terry_view::set_points(e, doc, id, &pts).map(|()| format!("{} points", pts.len()))
                        }

                        None => Err(usage.to_owned()),
                    },
                    (Some("zone"), n) if n >= 7 && nums.len() == n - 1 && nums.len() % 2 == 0 => {
                        let pts: Vec<[f64; 2]> = nums.chunks(2).map(|c| [c[0], c[1]]).collect();
                        crate::terry_view::new_zone(e, doc, &pts)
                    }

                    (Some("delete"), 2) => hex(&a[1]).ok_or_else(|| usage.to_owned()).and_then(|id| crate::terry_view::delete(e, doc, &[id])),
                    (Some("duplicate"), 2) => hex(&a[1]).ok_or_else(|| usage.to_owned()).and_then(|id| crate::terry_view::duplicate(e, doc, &[id])),
                    (Some("set"), 5) => match (hex(&a[1]), e.docs.find_as_mut::<MapDocument>(doc)) {
                        (Some(id), Some(d)) => {
                            let before = d.project().and_then(|p| p.entity(id).and_then(|x| p.component(x, &a[2])).and_then(|c| p.doc.get(c, &a[3])));
                            match before {
                                Some(before) => {
                                    let revision = d.terry_revision.clone();
                                    d.push_terry(Box::new(wce_terrain_editor::terry::TerryAttrCommand { id, component: a[2].clone(), attr: a[3].clone(), before: before.clone(), after: a[4].clone(), key: 0, revision }));
                                    Ok(format!("{}.{}: {before} -> {}", a[2], a[3], a[4]))
                                }

                                None => Err("no such entity, component or attribute".to_owned()),
                            }
                        }

                        _ => Err(usage.to_owned()),
                    },

                    (Some("move"), 4) => match (hex(&a[1]), a[2].parse::<f64>(), a[3].parse::<f64>(), e.docs.find_as_mut::<MapDocument>(doc)) {
                        (Some(id), Ok(dx), Ok(dz), Some(d)) => {
                            let mut m = wce_foundation::math::Float4x4::IDENTITY;
                            let u = d.terry_frame.unit;
                            (m.m[0][3], m.m[2][3]) = ((dx / u) as f32, (dz / u) as f32);
                            let src = wce_scene::render_scene::SourceRef { doc, item: id };
                            wce_editor_core::document::Document::transform_items(d, &[src], &m, 0x7e57_0001 ^ id);
                            let p = d.project().and_then(|p| p.entity(id).and_then(|x| p.local_position(x)));
                            Ok(format!("moved: {p:?}"))
                        }

                        _ => Err(usage.to_owned()),
                    },

                    (Some("get"), 2) => match (hex(&a[1]), e.docs.find_as::<MapDocument>(doc)) {
                        (Some(id), Some(d)) => d.project().and_then(|p| p.entity(id).map(|x| {
                            let w = p.world_position(id);
                            let line = p.polyline(x).map(|(pts, closed)| format!(" polyline {pts:?}{}", if closed { " closed" } else { "" })).unwrap_or_default();
                            format!("{} ({}) local {:?} world {w:?} children {:x?}{line}", x.name, p.kind(x).name(), p.local_position(x), x.children)
                        })).ok_or_else(|| "no such entity".to_owned()),

                        _ => Err(usage.to_owned()),
                    },

                    _ => Err(usage.to_owned()),
                };

                match r {
                    Ok(m) => CommandResult::success(m),
                    Err(err) => CommandResult::fail(err),
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
