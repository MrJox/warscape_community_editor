//! The Terrain tool: its Map panel (which map, the rings, the view distance), the map documents and the view of the active one.
//!
//! Opening a map reads it on a worker (a kit map is filled as BOB does: the kit's campaign battle map takes about a minute; its folder is
//! read into the model too), then makes its view's frame on another (the map's LF), then streams it around the camera
//! (`wce_map_view::MapView`) in the map's own frame: origin at the map's centre, x along the columns, z up the rows, heights from the sea.

use std::any::Any;
use std::path::{Path, PathBuf};
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
use wce_scene::lf_derive::LfDerive;
use wce_scene::render_scene::DocumentId;
use wce_terrain::LfRole;
use wce_terrain::MapKind;
use wce_terrain::lf_edit::{LfBrush, LfOp, LfStroke};
use wce_terrain::lf_patch;
use wce_terrain::pyramid::HeightPyramid;
use wce_terrain_editor::lf::{LfMirror, LfStrokeCommand};
use wce_terrain_editor::{MapDocument, MapSource};
use wce_tile::battle_map::context::{CELL, LfField, Spot};
use wce_tile::battle_map::tile_db::TileDatabase;
use wce_tile::battle_map::{kit_maps, l16_dds_heights};
use wce_tile_editor::terrain_bake::TerrainLook;
use wce_tile_editor::tile_document::context::{ContextRequest, Streamer, TileProvider, cache};
use wce_vfs::files::Files;

/// Scene units per metre (the tiles' editor units: the map context's frame is in them).
/// The File > Export > Built files entry's key for its last destination (`Editor::export_dir`).
const EXPORT_KEY: &str = "terrain";

const EXPORT_TIP: &str = "What BOB_Terrain builds (tile map, battle field, LF heights, locations.blm and its masks, deployment files), made from the map as it is, loose under a folder you choose and as a mod pack there";

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

    /// A campaign's roads at its start (none for a battle map, or when its files can't be read).
    roads: Option<Arc<wce_terrain_io::attila::roads::CampaignRoads>>,

    /// Read from an autosave copy (`OpenInputs::restored_from`).
    restored_from: Option<PathBuf>,

    /// A campaign's world bounds (`campaign_maps/<map>/map_data.esf`), the frame of its `.terry` and markers.
    world: Option<[f64; 4]>,
}

impl Loaded {
    fn document(self) -> Box<MapDocument> {
        let mut d = MapDocument::new(self.name, self.source, self.model, self.placed, self.locations, self.db);
        d.fill_messages = self.messages;
        d.view_roads = self.roads;
        d.restored_from = self.restored_from;
        d.terry_frame = if map_kind(&d.name).0 == MapKind::Campaign {
            wce_terrain_editor::document::TerryFrame::campaign(d.placed.width, d.placed.height, self.world)
        } else {
            wce_terrain_editor::document::TerryFrame::BATTLE
        };
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
        let (s, sz, k) = (lf.step, lf.dz(), 65535.0 / lf.scale.max(1e-6));
        let os = [(o.x - lf.x0) / s, (o.y + lf.offset) * k, (o.z - lf.z0) / sz];
        let ds = [d.x / s, d.y * k, d.z / sz];
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
        Self {
            on: false,
            sea: false,
            op: 0,
            radius_m: 300.0,
            amount_m: 2.0,
            share: 0.2,
            falloff: 1.0,
            aspect: 1.0,
            rotation_deg: 0.0,
            noise_scale_m: 400.0,
            spacing: 0.25,
        }
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

// Sent once per map opened, moved not copied: boxing the large variant would buy nothing.
#[allow(clippy::large_enum_variant)]
enum Job {
    Opened(Result<Loaded, String>),
    Built(DocumentId, Box<Streamer>),
}

/// What opening a map needs, taken from the editor on the UI thread.
/// A kit campaign map's prefix in the map list (`campaigns/main_attila_map`).
pub(crate) const CAMPAIGN: &str = "campaigns/";

/// A map name's kind and folder name (`campaigns/<name>`: a kit campaign map).
/// A campaign map's sea colour (`terrain/campaigns/<map>/lf_sea_colour.dds` in the packs) over the map's cells in the view's frame: the
/// game's campaign water shader colours its sea from it (`waterplane.hlsl` campaign: `s_lf_colour` at the world position over
/// `g_wp_world_size`, v from the north). None for a battle map, or without the file.
fn campaign_sea(e: &Editor, id: DocumentId, s: &Streamer) -> Option<wce_scene::render_scene::CampaignSea> {
    let d = e.docs.find_as::<MapDocument>(id)?;
    let (MapKind::Campaign, name) = map_kind(&d.name) else { return None };
    let colour = format!("terrain/campaigns/{name}/lf_sea_colour.dds");
    e.files().filter(|f| f.exists(&colour))?;
    let f = s.frame();
    let (w, h) = (f64::from(f.map.width), f64::from(f.map.height));
    let (sw, ne) = (f.from_map.apply(0.0, h), f.from_map.apply(w, 0.0));
    let size = [(ne.0 - sw.0) as f32, (ne.1 - sw.1) as f32];
    (size[0] > 0.0 && size[1] > 0.0).then_some(wce_scene::render_scene::CampaignSea { colour, corner: [sw.0 as f32, sw.1 as f32], size })
}

/// A campaign map's season snow (`terrain/campaigns/<map>/snow_mask_<season>.dds` at climate stage 0, `snow_mask_<season><stage>.dds`
/// after, in the packs: Attila ships spring's 1 to 4, autumn's 3 and 4, winter's 1 and 3 and plain) over the map's
/// cells in the view's frame, with the campaign climates' `snow` group (`terrain/textures/campaign/default/snow`) at the kit's
/// `blend_pixel_scale` for it (620 without the kit's `_settings.xml`). None for a battle map, or a season without a mask.
fn campaign_snow(e: &Editor, id: DocumentId, s: &Streamer, season: &str) -> Option<wce_scene::render_scene::CampaignSnow> {
    let d = e.docs.find_as::<MapDocument>(id)?;
    let (MapKind::Campaign, name) = map_kind(&d.name) else { return None };
    // The climate stage's mask as the weather node names them (fn 0x118f6360, code 0x1191d1e0 - 0x1191d4a0): stage 0 the season's own
    // (`snow_mask_wi`), stage n after it `snow_mask_<season><n>`. RenderDoc (the user's captures, 2026-11-18): the game's mask texture
    // (`s_mask_map`, R8G8 2032 x 1441) is `snow_mask_wi` exactly in a winter turn at stage 0, and empty on turn 1 in spring (Attila
    // ships no `snow_mask_sp`): the snowy peaks of its spring are the terrain's own textures. (Before: stage + 1, `snow_mask_sp1` on
    // turn 1.) ASSUMPTION: a stage's missing mask falls back to the season's own.
    let stage = d.view_roads.as_ref().and_then(|r| r.climate_stage).unwrap_or(0);
    let files = e.files()?;
    let own = format!("terrain/campaigns/{name}/snow_mask_{season}.dds");
    let names = if stage == 0 { vec![own] } else { vec![format!("terrain/campaigns/{name}/snow_mask_{season}{stage}.dds"), own] };
    let mask = names.into_iter().find(|m| files.exists(m))?;
    let pixel_scale = kit_roots(e)
        .iter()
        .find_map(|r| wce_formats::tile_climate::ClimateList::load(&r.join(database_dir(MapKind::Campaign)).join("_settings.xml")).ok())
        .and_then(|c| c.find("default").and_then(|d| d.find("snow")).map(|t| t.blend_pixel_scale as f32))
        .filter(|&v| v > 0.0)
        .unwrap_or(620.0);
    let f = s.frame();
    let (w, h) = (f64::from(f.map.width), f64::from(f.map.height));
    let (sw, ne) = (f.from_map.apply(0.0, h), f.from_map.apply(w, 0.0));
    let size = [(ne.0 - sw.0) as f32, (ne.1 - sw.1) as f32];
    (size[0] > 0.0 && size[1] > 0.0).then(|| wce_scene::render_scene::CampaignSnow {
        mask,
        corner: [sw.0 as f32, sw.1 as f32],
        size,
        group: "terrain/textures/campaign/default/snow".to_owned(),
        season: season.to_owned(),
        pixel_scale,
    })
}

pub(crate) fn map_kind(name: &str) -> (MapKind, &str) {
    // A campaign map of the packs (`campaigns/<map> (game files)`) is named by its folder too.
    match name.strip_prefix(CAMPAIGN) {
        Some(n) => (MapKind::Campaign, n.strip_suffix(wce_map_view::maps::GAME_FILES).unwrap_or(n)),
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

/// Campaign metadata follows the layered database's first settings file, independently of the map resources' source.
fn campaign_render_lf(roots: &[PathBuf], kind: MapKind) -> Option<(f64, f64)> {
    if kind != MapKind::Campaign {
        return None;
    }
    let settings = roots.iter().map(|r| r.join(database_dir(kind)).join("_settings.xml")).find(|p| p.is_file())?;
    render_lf(&settings)
}

#[cfg(test)]
mod campaign_metadata_tests {
    use super::*;

    #[test]
    fn settings_follow_database_order_independently_of_map_resources() {
        let scratch = std::env::temp_dir().join(format!(
            "wce-campaign-metadata-{}-{}",
            std::process::id(),
            std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()
        ));
        let roots: Vec<_> = ["project", "kit-first", "kit-second"].iter().map(|s| scratch.join(s)).collect();
        let settings: Vec<_> = roots.iter().map(|r| r.join(database_dir(MapKind::Campaign)).join("_settings.xml")).collect();
        for p in &settings {
            std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        }
        let write = |i: usize, height: f64, offset: f64| {
            let xml = format!(r#"<ROOT><RENDER_PARAMS lf_height="{height}" vertical_offset="{offset}"/></ROOT>"#);
            std::fs::write(&settings[i], xml).unwrap()
        };
        write(1, 810., 42.);
        write(2, 920., 16.);
        // A saved map and even an incomplete project database directory must not hide the kit's metadata.
        std::fs::create_dir_all(kit_map_dir(&roots[0], "campaigns/test")).unwrap();
        assert_eq!(campaign_render_lf(&roots, MapKind::Campaign), Some((810., 42.)));
        // The project's complete settings file is authoritative over both kits.
        write(0, 1234., 56.);
        assert_eq!(campaign_render_lf(&roots, MapKind::Campaign), Some((1234., 56.)));
        // As with TileDatabase::load_layered, a first settings file lacking a value does not borrow it from a later file.
        std::fs::write(&settings[0], b"<ROOT/>").unwrap();
        assert_eq!(campaign_render_lf(&roots, MapKind::Campaign), None);
        // The native-map adapter's single packs database and battle-map defaults keep their existing behavior.
        assert_eq!(campaign_render_lf(&roots[1..2], MapKind::Campaign), Some((810., 42.)));
        assert_eq!(campaign_render_lf(&roots, MapKind::Battle), None);
        for p in settings {
            if p.is_file() {
                std::fs::remove_file(p).unwrap();
            }
        }
    }
}

#[cfg(test)]
#[path = "campaign_metadata_native_tests.rs"]
mod campaign_metadata_native_tests;

/// How the map list shows a map: its name (a map of the packs without its ` (game files)`) and, in brackets, where it is read from (the
/// packs: `game files`; the projects' kit layout: `project`; an Assembly Kit: `kit: <its folder>`). The name stays the map's key.
fn map_label(name: &str, source: Option<String>) -> String {
    let shown = name.strip_suffix(wce_map_view::maps::GAME_FILES).unwrap_or(name);
    match source {
        Some(s) => format!("{shown}  [{s}]"),
        None => shown.to_owned(),
    }
}

/// An Assembly Kit as the map list names it: the folder holding it (`Total War Attila` for `<game>/assembly_kit/raw_data`), else its own.
fn kit_name(raw: &std::path::Path) -> String {
    let kit = raw.parent().unwrap_or(raw);
    let named =
        if kit.file_name().is_some_and(|n| n.to_string_lossy().eq_ignore_ascii_case("assembly_kit")) { kit.parent() } else { Some(kit) };
    named.and_then(|p| p.file_name()).map_or_else(|| raw.display().to_string(), |n| n.to_string_lossy().into_owned())
}

/// A kit's campaign maps (folders with a `tile_map.png`), as the map list names them.
fn kit_campaign_maps(raw: &std::path::Path) -> Vec<String> {
    let mut out: Vec<String> = std::fs::read_dir(raw.join("terrain/campaigns"))
        .into_iter()
        .flatten()
        .flatten()
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

    /// The `raw_data` folders kit-layout files are looked for in (`kit_roots`: the projects', then the kits'; the packs' tree in Rome 2).
    roots: Vec<PathBuf>,
    db: Arc<TileDatabase>,
    render_lf: Option<(f64, f64)>,
    files: Option<Arc<dyn Files>>,
    packs: Option<Arc<PacksTree>>,

    /// A game without kit maps (Rome 2): the pack map is edited in its built form, from this folder when it has a saved copy (the
    /// projects' `working_data/terrain/battles/<map>`), else from the packs.
    built: Option<PathBuf>,

    /// The game and its install folder (a campaign's startpos and db: its roads at the start).
    game: &'static wce_foundation::game::GameProfile,
    install: Option<PathBuf>,

    /// The map is read from an autosave copy (this folder, `raw` or `built` pointing into it): the document is modified until saved.
    restored_from: Option<PathBuf>,
}

/// The Terrain tool's id: the owner of its documents.
pub const TERRAIN_TOOL_ID: &str = "terrain";

pub struct TerrainTool {
    maps: Vec<String>,

    /// How the map list shows each of `maps`: its name and where it is read from (`map_label`).
    map_labels: Vec<String>,
    map: i32,
    maps_for: Vec<PathBuf>,
    rings: Rings,

    /// The other kind's rings (a campaign map's while a battle map shows, and the other way), whether `rings` are a campaign map's, and
    /// whether the panel or a script changed each kind's (battle, campaign): unchanged, a campaign map takes its own defaults
    /// (`Rings::campaign`, by its cells a world unit).
    rings_other: Rings,
    rings_campaign: bool,
    rings_edited: [bool; 2],

    /// The shown campaign map's camera limits (`campaign_camera_limits`, read on a worker), by its document.
    camera_limits: Option<(DocumentId, Arc<std::sync::OnceLock<Option<[f32; 4]>>>)>,

    /// A campaign map's season in the view (`sp`, `su`, `au`, `wi`): the ground's textures of that season and its snow mask.
    season: String,

    /// A campaign map's weather (a lighting or environment file of its folder); none: the map's global lighting of the season.
    weather: Option<String>,

    /// A campaign map's lighting spheres, blended at the camera every frame (none: one file lights the view).
    lighting: Option<crate::lighting::CampaignLighting>,

    /// A campaign map's skybox at the camera.
    skybox: Option<crate::skybox::CampaignSkybox>,

    /// A campaign map's borders and trade routes (read on a worker for the faction picked).
    lines: crate::lines::CampaignLinesView,

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
    placements: crate::placements::PlacementsPanel,
    props: crate::prop_models::PropModels,

    /// The campaign start's armies, navies and agents (read only).
    figures: crate::start_figures::StartFigures,

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
    launched: Option<wce_launch::launch::Running>,
}

impl Default for TerrainTool {
    fn default() -> Self {
        Self {
            maps: vec![],
            map_labels: vec![],
            map: 0,
            maps_for: vec![],
            rings: Rings::default(),
            rings_other: Rings::default(),
            rings_campaign: false,
            rings_edited: [false, false],
            camera_limits: None,
            season: "su".to_owned(),
            weather: None,
            lighting: None,
            skybox: None,
            lines: crate::lines::CampaignLinesView::default(),
            map_panel: crate::map_panel::MapPanel::new(),
            paint: LfPaint::default(),
            cells: crate::cell_paint::CellPaint::default(),
            explicit: Default::default(),
            terry: Default::default(),
            layout: Default::default(),
            placements: Default::default(),
            props: Default::default(),
            figures: Default::default(),
            findings: vec![],
            refill: Default::default(),
            refill_wanted: false,
            painting: None,
            mirror: None,
            sea_mirror: None,
            sea_lf: None,
            ring: (0, 0),
            pick: None,
            cursor: None,
            view: MapView::default(),
            shown: 0,
            building: 0,
            cameras: std::collections::HashMap::new(),
            rx: None,
            busy: String::new(),
            busy_since: None,
            status: String::new(),
            packs: None,
            provider: None,
            db: None,
            campaign_db: None,
            export: None,
            exported: None,
            launch_after_export: None,
            launched: None,
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
    let m = d.model()?;
    if m.built {
        return Some(built_dir(e, &d.name));
    }

    Some(kit_map_dir(&e.settings_ref().kit_projects_dir().join("raw_data"), &d.name))
}

/// Where a built map (a pack map of a game without kit maps) is saved: the projects' `working_data/terrain/battles/<map>` (a campaign map's `working_data/terrain/campaigns/<map>`).
pub(crate) fn built_dir(e: &Editor, name: &str) -> PathBuf {
    let working = e.settings_ref().kit_projects_dir().join("working_data/terrain");
    match map_kind(name) {
        (MapKind::Campaign, n) => working.join("campaigns").join(n),
        _ => working.join("battles").join(name.strip_suffix(wce_map_view::maps::GAME_FILES).unwrap_or(name)),
    }
}

/// A built map's files: its saved copy's, else the packs' folder's.
fn built_files(i: &OpenInputs, folder: &str) -> Result<wce_terrain_io::MapFiles, String> {
    if let Some(dir) = i.built.as_ref().filter(|d| d.join("lf_height_map.dds").is_file()) {
        return wce_terrain_io::MapFiles::from_dir(dir).map_err(|e| e.to_string());
    }

    let files = i.files.as_ref().ok_or("no game files")?;
    let prefix = format!("{folder}/");
    let mut out = vec![];
    for p in files.list(&prefix, "") {
        out.push((p[prefix.len()..].to_owned(), files.read(&p)?));
    }

    Ok(wce_terrain_io::MapFiles::from_bytes(out))
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
    let (ci, cj) = (-lf.x0 / lf.step, -lf.z0 / lf.dz());
    (0..lf.nz)
        .step_by(stride as usize)
        .flat_map(|j| (0..lf.nx).step_by(stride as usize).map(move |i| (i, j)))
        .map(|(i, j)| (lf.x0 + i as f32 * lf.step, lf.z0 + j as f32 * lf.dz(), (i as f32 - ci).powi(2) + (j as f32 - cj).powi(2)))
        .map(|(x, z, d)| (x, z, d, lf.height_in(&v, x, z)))
        .filter(|&(.., h)| h > ABOVE_SEA)
        .min_by(|a, b| a.2.total_cmp(&b.2))
        .map(|(x, z, _, h)| Float3::new(x, h, z))
}

/// A model's field as the map context's: the same samples, shared.
fn lf_field(f: &wce_terrain::paged::PagedU16) -> LfField {
    LfField::shared(f.grid().clone())
}

/// How far from the camera's target a campaign map's settlements and props are drawn (world units): the props' 96 cells on Attila's map,
/// which on Thrones' and TDD's (3 cells a unit) would be half the distance.
const CAMPAIGN_PROP_REACH: f64 = 64.0;

/// The game's campaign camera limits (`campaign_variables`: `camera_minimum_distance`, `camera_maximum_distance`,
/// `camera_minimum_tilt_angle`, `camera_maximum_tilt_angle`), read on a worker; none when the table or a value is missing.
fn campaign_camera_limits(e: &Editor) -> Arc<std::sync::OnceLock<Option<[f32; 4]>>> {
    let cell = Arc::new(std::sync::OnceLock::new());
    let (Some(files), Some(schema_path)) = (e.files(), wce_foundation::settings::rpfm_schema_path(e.game())) else {
        let _ = cell.set(None);
        return cell;
    };
    let out = cell.clone();
    std::thread::spawn(move || {
        let limits = (|| {
            let schema = rpfm_lib::schema::Schema::load(&schema_path, None).ok()?;
            let list = |p: &str| files.list(p, "");
            let read = |p: &str| files.read(p).ok().map(|b| b.to_vec());
            let rows = wce_terrain_io::attila::cities::table_rows(&schema, "campaign_variables_tables", &list, &read);
            let value = |key: &str| {
                rows.iter().find(|r| r.get("variable_key").is_some_and(|v| v == key)).and_then(|r| r.get("value")?.parse::<f32>().ok())
            };
            Some([
                value("camera_minimum_distance")?,
                value("camera_maximum_distance")?,
                value("camera_minimum_tilt_angle")?,
                value("camera_maximum_tilt_angle")?,
            ])
        })();
        wce_foundation::log::info(format!("terrain: the campaign camera's limits (distance, tilt): {limits:?}"));
        let _ = out.set(limits);
    });
    cell
}

/// A reader of a campaign's startpos by its path (`campaigns/<key>/startpos.esf`): the packs' (a mod's: TDD's), else the game's `data`
/// folder's loose file (the game's own campaigns').
pub(crate) fn startpos_reader<'a>(
    files: &'a Arc<dyn Files>,
    install: Option<&'a std::path::Path>,
) -> impl Fn(&str) -> Option<Vec<u8>> + 'a {
    move |p: &str| files.read(p).ok().map(|b| b.to_vec()).or_else(|| std::fs::read(install?.join("data").join(p)).ok())
}

/// A campaign's roads at its start (`wce_terrain_io::attila::roads`): its map's `map_data.esf`, its campaign's startpos
/// (`wce_terrain_io::attila::start`), `tile_upgrades.xml` (the first of the projects and the kits that has it, then the packs') and the db. None when one is missing (logged).
fn campaign_roads(i: &OpenInputs) -> Option<Arc<wce_terrain_io::attila::roads::CampaignRoads>> {
    let started = std::time::Instant::now();
    let (_, name) = map_kind(&i.name);
    let r = (|| {
        let files = i.files.as_ref().ok_or("no game files")?;
        let schema = rpfm_lib::schema::Schema::load(&wce_foundation::settings::rpfm_schema_path(i.game).ok_or("no rpfm schema")?, None)
            .map_err(|e| e.to_string())?;
        let list = |p: &str| files.list(p, "");
        let read = |p: &str| files.read(p).ok().map(|b| b.to_vec());
        let start = wce_terrain_io::attila::start::find(&schema, name, &list, &read, &startpos_reader(files, i.install.as_deref()))?;
        log::info(format!("terrain open {}: the start of campaign {}", i.name, start.campaign));
        let tables = wce_terrain_io::attila::roads::RoadTables::read(&schema, &start.campaign, &list, &read);
        let map_data = files.read(&format!("campaign_maps/{name}/map_data.esf")).map_err(|e| format!("map_data.esf: {e}"))?;
        let startpos = start.startpos;
        const UPGRADES: &str = "terrain/tiles/campaign/tile_upgrades.xml";
        let upgrades = wce_tile::sources::first_file(&i.roots, UPGRADES)
            .and_then(|p| std::fs::read(p).ok())
            .or_else(|| files.read(UPGRADES).ok().map(|b| b.to_vec()))
            .ok_or("no tile_upgrades.xml")?;
        let dynamic = files.read(&format!("campaign_maps/{name}/dynamic_resources.esf")).ok();
        wce_terrain_io::attila::roads::read(&map_data, &startpos, &upgrades, &tables, dynamic.as_deref())
    })();
    match r {
        Ok(roads) => {
            log::info(format!(
                "terrain open {}: roads at the start, regions a level {:?}; farmland areas {} of {}; season {:?}, climate stage {:?} ({:.1} s)",
                i.name,
                roads.level_counts(),
                roads.areas_on,
                roads.areas_on + roads.areas_off,
                roads.start_season,
                roads.climate_stage,
                started.elapsed().as_secs_f64()
            ));
            Some(Arc::new(roads))
        }

        Err(e) => {
            log::warn(format!("terrain open {}: no roads at the start: {e}", i.name));
            None
        }
    }
}

/// Reads a map (a worker's job).
fn open_map(i: &OpenInputs) -> Result<Loaded, String> {
    let packs_folder = game_map_folder(&i.name, i.game_dir);
    let db = match (&packs_folder, &i.packs) {
        // A map of the packs names the packs' tiles (a mod's among them): read with their database.
        (Some(_), Some(p)) => p
            .raw()
            .ok()
            .and_then(|r| TileDatabase::load(&r.join(database_dir(map_kind(&i.name).0))).ok())
            .map_or_else(|| i.db.clone(), Arc::new),
        _ => i.db.clone(),
    };

    let (mut placed, locations, source, model, sea, messages) = match packs_folder {
        Some(folder) => {
            let (mut placed, locations) = load_map(&i.name, i.game_dir, &i.raw, &db, i.files.as_deref())?;
            let sea =
                i.files.as_ref().and_then(|f| f.read(&format!("{folder}/lf_sea_height_map.dds")).ok()).and_then(|b| l16_dds_heights(&b));
            // A game without kit maps: the built files are the model (its LF the one shown: a saved copy's edits).
            let model = match &i.built {
                Some(_) => {
                    let m = wce_terrain_io::built::read(&i.name, &built_files(i, &folder)?).map_err(|e| e.to_string())?;
                    if let Some(f) = m.lf(LfRole::Land) {
                        Arc::make_mut(&mut placed).lf = Some(lf_field(f.get()));
                    }

                    // The placements the model has (a saved copy's edits).
                    if let Some(list) = &m.tile_list {
                        let lf = placed.lf.clone();
                        placed = Arc::new(wce_tile::battle_map::context::PlacedMap::from_tile_list(&i.name, list.get(), &db, lf));
                    }

                    Some(m)
                }

                None => None,
            };

            (placed, locations, MapSource::Packs(folder), model, sea, vec![])
        }

        // A kit map: its files into the model, the model filled as BOB fills the files (the same fill runs again after an edit).
        None => {
            let dir = kit_map_dir(&i.raw, &i.name);
            let files = wce_terrain_io::MapFiles::from_dir(&dir).map_err(|e| e.to_string())?;
            let model = wce_terrain_io::attila::read(map_kind(&i.name).1, map_kind(&i.name).0, &files).map_err(|e| e.to_string())?;
            log::info(format!("terrain open {}: model read, {}", i.name, wce_foundation::heap::note()));
            let inputs = crate::refill::FillInputs::of(&model).ok_or_else(|| format!("{}: no tile_map.png", dir.display()))?;
            let (placed, messages, cached) = inputs.fill_cached(&db, &i.name);
            log::info(format!(
                "terrain open {}: filled{}, {}",
                i.name,
                if cached { " (from the cache)" } else { "" },
                wce_foundation::heap::note()
            ));
            let sea = model.lf(LfRole::Sea).map(|f| lf_field(f.get()));
            (Arc::new(placed), None, MapSource::Kit(dir), Some(model), sea, messages)
        }
    };

    // The sea tiles stand on the map's sea LF (the Tile tool's map context keeps them on a flat -20 m).
    Arc::make_mut(&mut placed).sea_lf = sea;

    // The placements' climates by index as the game reads them: its tile database in the packs (a mod's, TDD's ninth campaign climate
    // `wasteland`, Mordor's), past the kit's. With the kit's alone the index had no name: Mordor's mountains took the default climate's
    // light rock (TDD capture of 2026-11-21: the game's dark `mordor_a`).
    if let Some(files) = i.files.as_deref() {
        let kind = if map_kind(&i.name).0 == MapKind::Campaign { "campaign" } else { "battle" };
        let packs = wce_tile_editor::climate_db::effective_climates(wce_formats::tile_climate::ClimateList::default(), kind, Some(files));
        let names: Vec<String> = packs.climates.iter().map(|c| c.name.clone()).collect();
        let known = &placed.climates;
        if names.len() > known.len() && names.iter().zip(known).all(|(a, b)| a.eq_ignore_ascii_case(b)) {
            log::info(format!("terrain open {}: climates past the kit's from the packs' database: {:?}", i.name, &names[known.len()..]));
            Arc::make_mut(&mut placed).climates = names;
        }
    }

    // The LF's metres come from the selected database settings, even when the map resources are a saved project copy.
    let kind = map_kind(&i.name).0;
    if let Some((h, o)) = i.render_lf {
        let m = Arc::make_mut(&mut placed);
        (m.lf_height, m.vertical_offset) = (h, o);
    }

    let roads = if kind == MapKind::Campaign { campaign_roads(i) } else { None };
    let world = (kind == MapKind::Campaign)
        .then(|| i.files.as_ref()?.read(&format!("campaign_maps/{}/map_data.esf", map_kind(&i.name).1)).ok())
        .flatten()
        .and_then(|b| wce_terrain_io::attila::cities::world_bounds(&b));
    if kind == MapKind::Campaign {
        log::info(format!("terrain open {}: the world {world:?} (map_data.esf)", i.name));
    }

    let restored_from = i.restored_from.clone();
    Ok(Loaded { name: i.name.clone(), source, model, placed, locations, db, messages, roads, restored_from, world })
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
        let kits = wce_tile::sources::assembly_kit_roots(e.settings_ref());
        let stale = self.packs.as_ref().is_some_and(|t| e.vfs().is_none_or(|v| !t.same_sources(v, &kits)));
        if (self.packs.is_none() || stale)
            && let Some(vfs) = e.vfs().cloned()
        {
            // The Assembly Kits, not the projects' layout (decompiling reads the tile databases' settings of the first kit that has each;
            // a projects folder with one copied in is not a kit), none in Rome 2 (the packs' database then).
            let tree = Arc::new(PacksTree::new(vfs, kits, &e.settings_ref().paths().cache_dir().join("neighbours"), e.game().key));
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
        self.map_labels.clear();
        if let Some(f) = e.files() {
            self.maps.extend(game_maps(f.as_ref(), e.game().campaign_battle_map, !e.game().features.kit_tiles));
            // The packs' campaign maps too (Rome 2's `terrain/campaigns/<map>/terrain.tile_list`; a mod's of a game with kit maps, TDD's).
            self.maps.extend(wce_map_view::maps::game_campaign_maps(f.as_ref()));
            self.map_labels.extend(self.maps.iter().map(|m| map_label(m, Some("game files".to_owned()))));
        }

        let projects = e.settings_ref().kit_projects_dir().join("raw_data");
        for r in &roots {
            for m in kit_maps(r).into_iter().chain(kit_campaign_maps(r)) {
                if !self.maps.contains(&m) {
                    // Read from the first root that has it (`inputs`): the projects' kit layout, else an Assembly Kit.
                    let source = if *r == projects { "project".to_owned() } else { format!("kit: {}", kit_name(r)) };
                    self.map_labels.push(map_label(&m, Some(source)));
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
        // A game without kit maps (Rome 2): the maps of its packs, with the packs' tile database (`PacksTree::raw`). So is a campaign
        // map of the packs in a game with kit maps (a mod's: TDD's): its tiles and its database settings are the packs'.
        let packs_campaign = name.starts_with(wce_map_view::maps::GAME_CAMPAIGN) && name.ends_with(wce_map_view::maps::GAME_FILES);
        if !e.game().features.kit_tiles || packs_campaign {
            if game_map_folder(name, e.game().campaign_battle_map).is_none() {
                return Err(format!("{}: no kit maps; open one of the packs' (game files)", e.game().display_name));
            }

            let raw = self.packs.as_ref().ok_or("no game files")?.raw()?;
            let db = self.databases_of(std::slice::from_ref(&raw), map_kind(name).0)?;
            let render_lf = campaign_render_lf(std::slice::from_ref(&raw), map_kind(name).0);
            let built = Some(built_dir(e, name));
            return Ok(OpenInputs {
                name: name.to_owned(),
                game_dir: e.game().campaign_battle_map,
                roots: vec![raw.clone()],
                raw,
                db,
                render_lf,
                files: e.files(),
                packs: self.packs.clone(),
                built,
                game: e.game(),
                install: e.settings_ref().game_dir(),
                restored_from: None,
            });
        }

        let roots = kit_roots(e);
        let raw = roots
            .iter()
            .find(|r| kit_map_dir(r, name).join("tile_map.png").is_file())
            .cloned()
            .or_else(|| database_root(e))
            .ok_or("no Assembly Kit with a battle tile database in the settings")?;
        // Every kit's tiles (a project's few and the Assembly Kit's): the project's database alone misses the map's tiles.
        let db = self.databases_of(&roots, map_kind(name).0)?;
        let render_lf = campaign_render_lf(&roots, map_kind(name).0);
        Ok(OpenInputs {
            name: name.to_owned(),
            game_dir: e.game().campaign_battle_map,
            roots,
            raw,
            db,
            render_lf,
            files: e.files(),
            packs: self.packs.clone(),
            built: None,
            game: e.game(),
            install: e.settings_ref().game_dir(),
            restored_from: None,
        })
    }

    /// Starts reading a map (the open map gives way: asked about first when it has unsaved changes, the read starts after the prompt's Save
    /// or Discard).
    fn start_open(&mut self, e: &mut Editor, name: &str) {
        let again = name.to_owned();
        let retry = Box::new(move |e: &mut Editor| {
            e.with_tool::<Self, _>(|t, e| {
                t.start_open(e, &again);
                Ok(format!("reading {again}"))
            })
            .unwrap_or_else(|| Err("the Terrain tool is busy".to_owned()))
        });

        match e.clear_to_replace(TERRAIN_TOOL_ID, retry) {
            Ok(true) => {}
            Ok(false) => return,
            Err(err) => {
                e.status(&format!("Terrain: {err}"), true);
                self.status = err;
                return;
            }
        }

        match self.inputs(e, name) {
            Ok(i) => self.spawn_read(e, i),
            Err(err) => self.status = err,
        }
    }

    /// Reads a map on a worker; `poll` takes the document in.
    fn spawn_read(&mut self, e: &mut Editor, i: OpenInputs) {
        let name = i.name.clone();
        let (tx, rx) = channel();
        // A kit map is filled as BOB does: the campaign map's 2.9 million cells take a while.
        let kit = game_map_folder(&name, e.game().campaign_battle_map).is_none();
        self.busy = if kit {
            format!("reading {name} and filling it with tiles as BOB does (the kit's campaign map: about a minute and a half)")
        } else {
            format!("reading {name}")
        };

        self.busy_since = Some(std::time::Instant::now());
        e.status(&format!("Terrain: {}...", self.busy), false);
        self.rx = Some(rx);
        std::thread::spawn(move || {
            let r = open_map(&i);
            if let (Err(err), Some(copy)) = (&r, &i.restored_from) {
                log::warn(format!(
                    "terrain: {name}'s autosave copy did not open ({err}); it stays in {} until the editor quits",
                    copy.display()
                ));
            }

            let _ = tx.send(Job::Opened(r));
        });
    }

    /// A map's autosave copy (`MapDocument::autosave_to`) read as the map: made whole, in the layout the map is read from (a kit map's
    /// `raw_data/terrain/...`, a built map's folder), then read on a worker like any open.
    fn restore_map(&mut self, e: &mut Editor, copy: &Path, name: &str) -> Result<(), String> {
        if self.rx.is_some() {
            return Err(format!("the Terrain tool is busy ({})", self.busy));
        }

        let map = MapDocument::complete_autosave(copy)?;
        self.refresh_maps(e);
        let mut i = self.inputs(e, name)?;
        if i.built.is_some() {
            i.built = Some(map);
        } else {
            let raw = copy.join("raw_data");
            let dir = kit_map_dir(&raw, name);
            if let Some(parent) = dir.parent() {
                std::fs::create_dir_all(parent).map_err(|err| format!("{}: {err}", parent.display()))?;
            }

            std::fs::rename(&map, &dir).map_err(|err| format!("{}: {err}", dir.display()))?;
            i.raw = raw;
        }

        i.restored_from = Some(copy.to_path_buf());
        self.spawn_read(e, i);
        Ok(())
    }

    /// The view's request for a map document: the map's own frame (no open tile), the panel's rings.
    fn request(&self, e: &Editor, d: &MapDocument) -> Option<ContextRequest> {
        // The road tiles at the campaign's start (the view only).
        let m = &d.shown_placed();
        let roots = kit_roots(e);
        Some(ContextRequest {
            map: m.clone(),
            spot: Spot { x: (m.width / 2) as i32, y: (m.height / 2) as i32, rotation: 0x10 },
            cells: (0, 0),
            // A campaign map's heights: its database's `unit_scale` (`wce_tile::tile_project::height_unit`); its ground in the game's
            // world, the hex grid's (longer north to south).
            unit: if map_kind(&d.name).0 == MapKind::Campaign { 0.2 } else { UNIT },
            z_stretch: if map_kind(&d.name).0 == MapKind::Campaign { wce_scene::campaign_lighting::CAMPAIGN_Z_SCALE } else { 1.0 },
            near: self.rings.near,
            mid: self.rings.mid,
            ground: self.rings.ground,
            far: self.view.far_ring(self.rings.far),
            bake_ground: e.game().key != wce_vfs::GameProfile::ROME_2.key,
            raw_root: database_root(e).or_else(|| self.packs.as_ref().and_then(|p| p.raw().ok()))?,
            tile_roots: roots,
            game_kit: wce_tile::vegetation::params_kit(&wce_tile::sources::kit_raw_roots(e.settings_ref())),
            look: TerrainLook { season: self.season.clone(), ..TerrainLook::default() },
            trees: true,
            plain: None,
            groups: None,
            cache: Some(cache::cache_dir()),
            provider: self.provider.clone(),
        })
    }

    /// A document read: the tool's document in place of the open one (the view follows it). The open one was cleared when the read
    /// started; changes made to it during the read are asked about again.
    fn adopt(&mut self, e: &mut Editor, d: Box<MapDocument>) {
        let mut d = d;
        if e.docs.of_tool(TERRAIN_TOOL_ID).and_then(|id| e.docs.find(id)).is_some_and(|old| old.modified()) {
            // The read map waits in the prompt's open; it comes back here when the open can go on at once.
            let held = std::rc::Rc::new(std::cell::RefCell::new(Some(d)));
            let waiting = held.clone();
            let retry = Box::new(move |e: &mut Editor| {
                let Some(d) = waiting.borrow_mut().take() else { return Err("the map was opened already".to_owned()) };
                let name = d.name.clone();
                e.with_tool::<Self, _>(|t, e| t.adopt(e, d))
                    .map(|()| format!("opened {name}"))
                    .ok_or_else(|| "the Terrain tool is busy".to_owned())
            });

            match e.clear_to_replace(TERRAIN_TOOL_ID, retry) {
                Ok(true) => {}
                Ok(false) => return,
                Err(err) => {
                    e.status(&format!("Terrain: {err}"), true);
                    self.status = err;
                    return;
                }
            }

            let Some(back) = held.borrow_mut().take() else { return };
            d = back;
        }

        self.status = format!("{}: {}", d.name, d.summary());
        self.take_start_season(&d);
        d.save_dir = save_dir(e, &d);
        match e.replace_document(TERRAIN_TOOL_ID, d) {
            Ok(id) => self.start_view(e, id),
            Err(err) => e.status(&format!("Terrain: {err}"), true),
        }
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
        self.lighting = None;
        // A campaign map: its own lighting, as the game has it (`environment_collection.xml`'s spheres and global lighting, each
        // resolved for the season and climate stage and blended at the camera: `lighting::CampaignLighting`). A battle weather drew
        // its ground dark and blue.
        if let (MapKind::Campaign, name) = map_kind(&d.name)
            && let Some(files) = e.files()
        {
            let dir = format!("terrain/campaigns/{name}");
            let frame = d.terry_frame;
            // The climate stage as the snow masks' (`campaign_snow`): 0 the season's own file, n its `<season><n>` one (TDD ships
            // `europe_sp` and `europe_sp1` to `sp4`: five stages; both its startpos and Attila's start at 0).
            let n = d.view_roads.as_ref().and_then(|r| r.climate_stage).unwrap_or(0);
            let season = ["sp", "su", "au", "wi"].iter().position(|s| *s == self.season).unwrap_or(1);
            if self.weather.is_none()
                && let Some(mut l) = crate::lighting::CampaignLighting::load(&*files, &dir, season, n, frame)
            {
                log::info(format!("terrain: the map's lighting spheres ({})", l.summary()));
                l.update(e);
                self.lighting = Some(l);
                return;
            }

            // The chosen weather, else (a map without a collection: Rome 2's `light_grid.xml`, not read yet) the folder's season
            // lighting (`environment/summer.lighting`; ASSUMPTION: `default.environment` hazed the view), else the folder's
            // `default.environment`.
            let season_name = match self.season.as_str() {
                "sp" => "spring",
                "au" => "autumn",
                "wi" => "winter",
                _ => "summer",
            };
            let file = self
                .weather
                .clone()
                .filter(|f| files.exists(f))
                .or_else(|| Some(format!("{dir}/environment/{season_name}.lighting")).filter(|f| files.exists(f)))
                .or_else(|| Some(format!("{dir}/default.environment")).filter(|f| files.exists(f)));
            if let Some(file) = file {
                match e.set_weather_preview(Some(&file)) {
                    Ok(_) => log::info(format!("terrain: the map's lighting {file}")),
                    Err(err) => e.status(&format!("the map's lighting: {err}"), true),
                }

                return;
            }
        }

        let Some(d) = e.docs.find_as::<MapDocument>(id) else { return };
        let m = &d.placed;
        let (cx, cy) = ((m.width / 2) as i32, (m.height / 2) as i32);
        let climate = m
            .placements
            .iter()
            .find(|p| {
                let r = p.rect();
                r[0] <= cx && cx < r[2] && r[1] <= cy && cy < r[3]
            })
            .and_then(|p| m.climate_of(p))
            .map(str::to_lowercase);
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

        // The scene is the tile frame (128 units a cell). A battle map is drawn in its database's `unit_scale`, 2 game units a scene
        // unit. A campaign map's tiles in theirs (0.2: their texture repeats and detail distances), everything else in the campaign
        // world's, a cell its width over the map's cells (0.33391 / 128: the props, the skybox, the fog, the shadows; the engine's
        // campaign distances, `campaign_props_max_distance` 60, `campaign_sky_scale` 37.5, are in those).
        let frame = e.docs.find_as::<MapDocument>(id).filter(|d| map_kind(&d.name).0 == MapKind::Campaign).map(|d| d.terry_frame);
        e.scene.game_units = frame.map_or(UNIT as f32, |f| f.unit as f32);
        e.scene.terrain_units = if frame.is_some() { 0.2 } else { 0.0 };
        e.scene.z_stretch = if frame.is_some() { wce_scene::campaign_lighting::CAMPAIGN_Z_SCALE as f32 } else { 1.0 };
        e.scene.campaign = frame.is_some();
        let fog = self.lines.fog.clone();
        e.scene.campaign_shroud = frame.and(s.frame().lf.clone()).map(|lf| wce_scene::render_scene::CampaignShroud { lf, fog });
        if let Some(sky) = self.skybox.take() {
            sky.clear(e);
        }

        if frame.is_some()
            && let Some(name) = e.docs.find_as::<MapDocument>(id).map(|d| map_kind(&d.name).1.to_owned())
        {
            self.skybox = crate::skybox::CampaignSkybox::show(e, id, &name);
            // The borders and trade routes: read again for another map (its default faction).
            if self.lines.shown.as_ref().is_none_or(|s| s.0 != id) {
                self.lines.clear(e);
                self.lines.faction = None;
                self.lines.start(e, id, &name, None);
            }
        } else {
            self.lines.clear(e);
        }
        self.light(e, id);
        e.scene.campaign_sea = campaign_sea(e, id, &s);
        e.scene.campaign_snow = campaign_snow(e, id, &s, &self.season);
        let lift = s.frame().lift(0.0, 0.0);
        let start = s.frame().lf.as_ref().and_then(|lf| land_near_centre(lf));
        // The fields on their own lattices are what is picked and painted (a campaign map draws views of them as the game reads them:
        // the frame's `lf_edit`, `sea_lf_edit`), and the mirrors make those views again over each edit.
        let frame = s.frame();
        self.pick = frame.lf_edit.clone().or_else(|| frame.lf.clone()).map(LfPick::new);
        self.cursor = None;
        self.painting = None;
        self.sea_lf = frame.sea_lf_edit.clone().or_else(|| frame.sea_lf.clone());
        let doc = e.docs.find_as::<MapDocument>(id);
        let mirror = |role: LfRole,
                      lf: Option<&Arc<wce_scene::render_scene::LfMap>>,
                      drawn: Option<(&Arc<wce_scene::render_scene::LfMap>, LfDerive)>| {
            doc.and_then(|d| d.lf_field(role))
                .zip(lf)
                .and_then(|(field, lf)| LfMirror::of(field, lf.values.clone(), lf.nx, lf.nz))
                .map(|m| match drawn {
                    Some((d, derive)) => m.with_derived(d.values.clone(), derive),
                    None => m,
                })
                .map(Arc::new)
        };
        let land_box = frame.lf_edit.as_ref().zip(frame.lf.as_ref()).map(|(e, d)| (d, LfDerive::Box((e.nx / d.nx.max(1)).max(1) as u32)));
        let sea_drawn = frame.sea_lf_edit.as_ref().and(frame.sea_lf.as_ref()).map(|d| (d, LfDerive::GameSea));
        self.mirror = mirror(LfRole::Land, self.pick.as_ref().map(|p| &p.lf), land_box);
        self.sea_mirror = mirror(LfRole::Sea, self.sea_lf.as_ref(), sea_drawn);
        // The rings of the map's kind: a campaign map's by its own scale unless changed.
        if let Some(tf) = e.docs.find_as::<MapDocument>(id).map(|d| d.terry_frame) {
            let campaign = tf.y_from_lf;
            if campaign != self.rings_campaign {
                std::mem::swap(&mut self.rings, &mut self.rings_other);
                self.rings_campaign = campaign;
            }
            if campaign && tf.cell > 0.0 {
                let per_unit = 1.0 / tf.cell;
                if !self.rings_edited[1] {
                    let follow = self.rings.follow;
                    self.rings = Rings { follow, ..Rings::campaign(per_unit) };
                }
                // The settlements' and the props' models to 64 world units (96 cells on Attila's map, as before; 192 on Thrones').
                self.props.radius_cells = (CAMPAIGN_PROP_REACH * per_unit).ceil();
                if self.camera_limits.as_ref().is_none_or(|(d, _)| *d != id) {
                    self.camera_limits = Some((id, campaign_camera_limits(e)));
                }
            } else {
                self.camera_limits = None;
            }
        }
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

    /// A campaign map's weathers: its folder's lighting files (`lighting/`, `environment/`) and `default.environment`.
    fn campaign_weathers(e: &Editor, d: &MapDocument) -> Vec<String> {
        let (MapKind::Campaign, name) = map_kind(&d.name) else { return vec![] };
        let Some(files) = e.files() else { return vec![] };
        let dir = format!("terrain/campaigns/{name}");
        let mut v: Vec<String> = ["lighting", "environment"]
            .iter()
            .flat_map(|f| files.list(&format!("{dir}/{f}/"), ".lighting"))
            .map(|s| s.to_owned())
            .collect();
        v.sort();
        v.extend(Some(format!("{dir}/default.environment")).filter(|f| files.exists(f)));
        v
    }

    /// A campaign map's weather in the view (none: the season's global lighting); the view lit again.
    fn set_weather(&mut self, e: &mut Editor, weather: Option<String>) {
        if self.weather != weather {
            self.weather = weather;
            let id = self.shown;
            if id != 0 {
                self.light(e, id);
            }
        }
    }

    /// A campaign map opened: the view's season is its start's (the startpos's calendar; Attila's main campaign spring).
    fn take_start_season(&mut self, d: &MapDocument) {
        const SEASONS: [&str; 4] = ["sp", "su", "au", "wi"];
        if let Some(k) = d.view_roads.as_ref().and_then(|r| r.start_season).filter(|&k| k < 4) {
            SEASONS[k as usize].clone_into(&mut self.season);
        }
    }

    /// A campaign map's season in the view: its ground textures and snow (the view built again when it changes).
    fn set_season(&mut self, e: &mut Editor, season: &str) {
        if self.season != season {
            season.clone_into(&mut self.season);
            let id = self.shown;
            if id != 0 {
                self.start_view(e, id);
            }
        }
    }

    /// A campaign map's "roads at the start" switch (the view built again when it changes).
    fn draw_roads_control(&mut self, e: &mut Editor) {
        let id = self.shown;
        if e.docs.find_as::<MapDocument>(id).is_some_and(|d| map_kind(&d.name).0 == MapKind::Campaign) {
            const SEASONS: [(&str, &str); 4] = [("sp", "spring"), ("su", "summer"), ("au", "autumn"), ("wi", "winter")];
            let mut k = SEASONS.iter().position(|x| x.0 == self.season).unwrap_or(1) as i32;
            if gui::combo("season##terrain_season", &mut k, &SEASONS.iter().map(|x| x.1).collect::<Vec<_>>()) {
                self.set_season(e, SEASONS[k.clamp(0, 3) as usize].0);
            }

            if gui::is_item_hovered() {
                gui::set_tooltip(
                    "The campaign map's season in the view: the ground's textures of that season and the map's snow mask of the season and climate stage (snow_mask_<season><stage + 1>.dds, the terrain resolve's snow layer). The view only.",
                );
            }

            let weathers = e.docs.find_as::<MapDocument>(id).map(|d| Self::campaign_weathers(e, d)).unwrap_or_default();
            let mut names = vec!["automatic (the season's)".to_owned()];
            names.extend(weathers.iter().map(|w| w.rsplit('/').next().unwrap_or(w).to_owned()));
            let mut k = self.weather.as_ref().and_then(|w| weathers.iter().position(|x| x == w)).map_or(0, |p| p as i32 + 1);
            if gui::combo("weather##terrain_weather", &mut k, &names.iter().map(String::as_str).collect::<Vec<_>>()) {
                let pick = if k <= 0 { None } else { weathers.get(k as usize - 1).cloned() };
                self.set_weather(e, pick);
            }

            if gui::is_item_hovered() {
                gui::set_tooltip(
                    "The campaign map's weather: its lighting files (lighting/<climate>_<season>[n], environment/<season>) or default.environment. Automatic: as the game lights the campaign, environment_collection.xml's spheres blended at the camera over its global lighting, each file of the view's season and climate stage.",
                );
            }

            // The borders' faction: their colours are the diplomacy's towards it (own, allied, at war, neutral, rebels).
            if !self.lines.factions.is_empty() {
                let keys: Vec<&str> = self.lines.factions.iter().map(|f| f.key.as_str()).collect();
                let mut k = self.lines.faction.as_deref().and_then(|f| keys.iter().position(|x| *x == f)).unwrap_or(0) as i32;
                if gui::combo("borders for##terrain_faction", &mut k, &keys) {
                    let pick = keys.get(k.max(0) as usize).map(|s| (*s).to_owned());
                    if pick != self.lines.faction {
                        self.lines.faction.clone_from(&pick);
                        let name = e.docs.find_as::<MapDocument>(id).map(|d| map_kind(&d.name).1.to_owned());
                        if let Some(name) = name {
                            self.lines.start(e, id, &name, pick);
                        }
                    }
                }

                if gui::is_item_hovered() {
                    gui::set_tooltip(
                        "The faction the campaign map's borders are drawn for, as the game draws the player's: own orange, allied blue, at war red, neutral grey, rebels black (the startpos's diplomacy). The view only.",
                    );
                }

                let mut fog = self.lines.fog_on;
                if gui::checkbox("fog of war at the start##terrain_fog", &mut fog) {
                    self.lines.fog_on = fog;
                    let name = e.docs.find_as::<MapDocument>(id).map(|d| map_kind(&d.name).1.to_owned());
                    if let Some(name) = name {
                        let pick = self.lines.faction.clone();
                        self.lines.start(e, id, &name, pick);
                    }
                }

                if gui::is_item_hovered() {
                    gui::set_tooltip(
                        "The faction's fog of war on the campaign's first turn, as the game builds it from the startpos (hidden land dark, explored land darkened, what it sees lit; the shroud's clouds over the hidden). Off: all revealed. The view only.",
                    );
                }
            }

            // The automatic lighting's files at the camera, with their weights.
            if self.weather.is_none()
                && let Some(l) = &self.lighting
            {
                for (name, w) in l.readout(e) {
                    gui::text_disabled(&format!("{:>3.0} %  {name}", w * 100.0));
                }
            }
        }

        let Some(d) = e.docs.find_as_mut::<MapDocument>(id).filter(|d| d.view_roads.is_some()) else { return };
        let mut on = d.roads_at_start;
        if gui::checkbox("roads and farmland at the campaign's start", &mut on) {
            d.roads_at_start = on;
            self.start_view(e, id);
        }

        if gui::is_item_hovered() {
            gui::set_tooltip(
                "The road tiles as the game shows them on the first turn: each region's road level from its buildings in the startpos \
                (campaign_map_roads, tile_upgrades.xml). Off: the kit's roads_climate tiles, as BOB places them. The view only: the map's files keep the kit's tiles.",
            );
        }
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
                if e.docs.find(id).is_some() && e.docs.of_tool(TERRAIN_TOOL_ID) == Some(id) {
                    self.show(e, id, s);
                } else {
                    self.building = 0;
                }
            }
        }
    }

    /// Opens a map and waits for its view (scripts).
    fn open_now(&mut self, e: &mut Editor, name: &str) -> Result<String, String> {
        let again = name.to_owned();
        let retry = Box::new(move |e: &mut Editor| {
            e.with_tool::<Self, _>(|t, e| t.open_now(e, &again)).unwrap_or_else(|| Err("the Terrain tool is busy".to_owned()))
        });
        if !e.clear_to_replace(TERRAIN_TOOL_ID, retry)? {
            return Ok(format!("asking about the map's unsaved changes before opening {name}"));
        }

        self.refresh_maps(e);
        let i = self.inputs(e, name)?;
        let mut d = open_map(&i)?.document();
        d.save_dir = save_dir(e, &d);
        self.take_start_season(&d);
        let summary = format!("{}: {}", d.name, d.summary());
        let rq = self.request(e, &d).ok_or("no kit with a tile database")?;
        let id = e.replace_document(TERRAIN_TOOL_ID, d)?;
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
        let names: Vec<&str> = self.map_labels.iter().map(String::as_str).collect();
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
        let mut rings = gui::slider_int("full detail (cells)", &mut self.rings.near, 1, 128);
        rings |= gui::slider_int("buildings (cells)", &mut self.rings.mid, 1, 512);
        rings |= gui::slider_int("tile ground (cells)", &mut self.rings.ground, 1, 1024);
        rings |= gui::slider_int("LF ground (cells)", &mut self.rings.far, 1, 1024);
        if rings {
            self.rings_edited[usize::from(self.rings_campaign)] = true;
            self.view.set_rings(&self.rings);
        }

        if self.view.far_view() > self.rings.far {
            gui::text_disabled(&format!("LF ground to the camera's view: {} cells", self.view.far_view()));
        }

        let whole = e
            .docs
            .find_as::<MapDocument>(self.shown)
            .map_or(GAME_FAR_PLANE * 4.0, |d| (d.placed.width.max(d.placed.height) as f32 * CELL as f32).max(GAME_FAR_PLANE));
        gui::slider_float("view distance", &mut self.rings.view_distance, GAME_FAR_PLANE, whole, "%.0f");
        gui::checkbox("follow the camera", &mut self.rings.follow);
        self.draw_status(e);
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
        let placements_was = self.placements.claims();
        self.placements.draw_section(e, self.shown);
        let terry_was = self.terry.claims_left_button();
        self.terry.draw_section(e, self.shown);
        self.layout.draw_controls(e, self.shown);
        self.draw_roads_control(e);
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
            self.placements.mode = crate::placements::Mode::Off;
        } else if self.placements.claims() && !placements_was {
            (self.paint.on, self.cells.on, self.explicit.placing) = (false, false, false);
            self.terry.mode = crate::terry_view::Mode::Select;
        } else if self.paint.on || self.cells.on || self.explicit.placing {
            self.terry.mode = crate::terry_view::Mode::Select;
            self.placements.mode = crate::placements::Mode::Off;
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

    /// The map's summary and the view's streaming, as a table (each was one long line); what the tool last said, when it is something
    /// else (an error).
    fn draw_status(&self, e: &Editor) {
        let doc = e.docs.find_as::<MapDocument>(self.shown);
        let summary = doc.map(|d| (format!("{}: {}", d.name, d.summary()), d.summary_rows()));
        let said = summary.as_ref().is_none_or(|(line, _)| *line != self.status);
        let stats = self.view.stats.as_ref();
        if summary.is_none() && stats.is_none() {
            if !self.status.is_empty() {
                gui::text_wrapped(&self.status);
            }
            return;
        }

        gui::separator();
        if gui::begin_table("terrain_status", 2, gui::TABLE_SIZING_STRETCH_PROP, gui::v2(0.0, 0.0)) {
            // The labels as wide as the longest, the values the rest (wrapped).
            gui::table_setup_column("what", gui::COLUMN_WIDTH_FIXED, gui::calc_text_size("LF under the tile").x);
            gui::table_setup_column("value", gui::COLUMN_WIDTH_STRETCH, 0.0);
            let row = |label: &str, value: &str| {
                gui::table_next_row();
                gui::table_next_column();
                gui::text_disabled(label);
                gui::table_next_column();
                gui::text_wrapped(value);
            };
            if let Some((_, rows)) = &summary {
                for (k, v) in rows {
                    row(k, v);
                }
            }

            if let Some(s) = stats {
                let [near, mid, ground, far, blocks, merged, distant] = s.shown;
                row("chunks", &format!("{near} near, {mid} buildings, {ground} tile ground, {far} LF ground"));
                row("blocks", &format!("{merged} merged, {blocks} LF ground, {distant} distant"));
                if s.building > 0 {
                    gui::table_next_row();
                    gui::table_next_column();
                    gui::text_disabled("building");
                    gui::table_next_column();
                    gui::text_colored([1.0, 0.8, 0.3, 1.0], &s.building.to_string());
                }

                row("tiles", &format!("{} opened ({:.1} s), {} from the cache", s.opened, s.open_s, s.from_cache));
                if s.failed > 0 {
                    gui::table_next_row();
                    gui::table_next_column();
                    gui::text_disabled("not opened");
                    gui::table_next_column();
                    gui::text_colored([1.0, 0.45, 0.4, 1.0], &format!("{} tiles", s.failed));
                }

                row("in memory", &format!("{} MB", s.mb));
                row("LF under the tile", s.lf_note.strip_prefix("LF under the tile ").unwrap_or(&s.lf_note));
                let first = s.first_s.map_or(String::new(), |t| format!(", first ring {t:.1} s"));
                row("loaded in", &format!("map {:.1} s{first}", s.map_s));
            }

            gui::end_table();
        }

        if said && !self.status.is_empty() {
            gui::text_wrapped(&self.status);
        }
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
        let (fi, fj) = (f64::from((p.x - lf.x0) / lf.step), f64::from((p.z - lf.z0) / lf.dz()));
        Some((fi, f64::from(lf.nz - 1) - fj))
    }

    /// The brush at a field sample.
    fn brush_at(&self, cx: f64, cy: f64) -> LfBrush {
        let r = f64::from(self.paint.radius_m) / self.sample_metres();
        let strength = if matches!(self.paint.op, 2 | 3) { f64::from(self.paint.share) } else { 1.0 };
        let brush = wce_tile::height_field::Brush {
            cx,
            cy,
            radius: r.max(1.0),
            strength,
            falloff: f64::from(self.paint.falloff),
            curve: None,
            image: None,
        };
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
            let why = if d.model().is_none() {
                "this map is from the game's packs: open the kit's map to paint its LF"
            } else {
                "this map has no lf_sea_heights.tif"
            };
            e.status(why, true);
            return false;
        };

        let mirror = self.target_mirror();
        let target = at.map_or(0.0, |(x, y)| {
            f64::from(field.at((x.round().max(0.0) as u32).min(field.width() - 1), (y.round().max(0.0) as u32).min(field.height() - 1)))
        });
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
                (f64::from(rect[0]) / sx).floor() as i32 - 1,
                (f64::from(rect[1]) / sy).floor() as i32 - 1,
                (f64::from(rect[2]) / sx).ceil() as i32 + 1,
                (f64::from(rect[3]) / sy).ceil() as i32 + 1,
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
                roots
                    .iter()
                    .find_map(|r| std::fs::read(r.join(&rel)).ok())
                    .and_then(|b| wce_formats::image_containers::TiffFile::parse(&b).ok())
                    .filter(|t| t.bits_per_sample == 16 && t.samples_per_pixel == 1)
                    .map(|t| lf_patch::LfPatch {
                        w: t.width,
                        h: t.height,
                        values: t.pixels.as_chunks::<2>().0.iter().map(|c| u16::from_le_bytes(*c)).collect(),
                    })
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
        Ok(format!(
            "{laid} placements of {tiles} tiles with an lf_mask.tif patched ({outside} off the map): {changed} of their {written} low samples changed, the record keeps {kb} KB"
        ))
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
            gui::push_style_color(gui::COL_TEXT, gui::v4(col[0], col[1], col[2], col[3]));
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
        let fill_reads = |k: usize| {
            k == wce_terrain_editor::cells::EXPLICIT
                || k == wce_terrain_editor::cells::TILE_LIST
                || d.model()
                    .and_then(|m| m.layers.get(k))
                    .is_some_and(|(r, _)| matches!(r, wce_terrain::LayerRole::TileMap | wce_terrain::LayerRole::Climate))
        };
        self.refill_wanted |= touched.iter().any(|(k, _)| fill_reads(*k));
        let drape = !self.cells.painting();
        for (k, rect) in touched {
            self.map_panel.cells_changed(e, self.shown, k, rect, drape);
        }

        // The fill again once the stroke is done.
        if self.refill_wanted
            && !self.cells.painting()
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
            let failed =
                if done.messages.is_empty() { String::new() } else { format!(", {} cells with no tile that fits", done.messages.len()) };
            let m = format!(
                "Terrain: filled again in {:.1} s: {} placements, {} chunks changed{failed}",
                done.secs,
                done.map.placements.len(),
                done.changed.len()
            );
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
                let (x, z) = (lf.x0 + fx as f32 * lf.step, lf.z0 + (f64::from(lf.nz - 1) - fy) as f32 * lf.dz());
                pts.push(Float3::new(x, lf.height_in(&values, x, z) + 0.5, z));
            }

            let col = 0xFF40_D0FFu32;
            let lines =
                pts.windows(2).flat_map(|w| [w[0], w[1]]).map(|p| wce_scene::render_scene::OverlayVertex { p, rgba: col }).collect();
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
                    self.ring.1 = e.scene.add(
                        self.ring.0,
                        wce_foundation::math::Float4x4::IDENTITY,
                        wce_scene::render_scene::Payload::Overlay(o),
                        src,
                    );
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

    /// Export starts a build of the shown map on a worker (one at a time), written under `out`.
    pub(crate) fn start_export(&mut self, e: &mut Editor, out: PathBuf) -> Result<String, String> {
        if self.export.is_some() {
            return Err("an export is on its way".to_owned());
        }

        let d = e.docs.find_as::<MapDocument>(self.shown).ok_or("no map shown")?;
        let db = d.db.clone();
        let job = crate::export::start(e, self.shown, db, &kit_roots(e), out)?;
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
        let fresh = self
            .exported
            .as_ref()
            .is_some_and(|x| x.map == d.name && x.revision == wce_editor_core::document::Document::history(d).version());
        if !fresh {
            let m = self.start_export(e, e.build_dir().join("terrain_export"))?;
            self.launch_after_export = Some((spot, dry_run, ignore_movie_packs));
            return Ok(format!("{m} (then the launch)"));
        }

        self.launch_now(e, &spot, dry_run, ignore_movie_packs)
    }

    fn launch_now(
        &mut self,
        e: &mut Editor,
        spot: &crate::launch::Spot,
        dry_run: bool,
        ignore_movie_packs: bool,
    ) -> Result<String, String> {
        let x = self.exported.clone().ok_or("no export")?;
        let d = e.docs.find_as::<MapDocument>(self.shown).ok_or("no map shown")?;
        let (w, h) = (d.placed.width, d.placed.height);
        let p = crate::launch::prepare(e, &x, w, h, spot, dry_run, ignore_movie_packs)?;
        let at = format!("{} ({}) at cell {:.0}, {:.0}", spot.name, spot.kind, spot.cell.0, spot.cell.1);
        if dry_run {
            let xml = e.build_dir().join("dry_wce_launch.xml");
            let _ = std::fs::write(&xml, &p.xml);
            return Ok(format!(
                "dry run: {} at {at}: the pack {}, the battle {}; command line: {}",
                x.map,
                p.pack.display(),
                xml.display(),
                p.command
            ));
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

    /// File > Export > Built files: asks for the folder, then starts the export.
    fn export_asked(&mut self, e: &mut Editor) {
        let Some(out) = e.pick_export_folder(EXPORT_KEY, "Folder for the map's built files (loose, and a mod pack)") else { return };
        match self.start_export(e, out) {
            Ok(m) => e.status(&m, false),
            Err(err) => e.status(&err, true),
        }
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
        if gui::button("Export built files...") {
            self.export_asked(e);
        }

        gui::end_disabled();
        if gui::is_item_hovered_allow_disabled() {
            gui::set_tooltip(EXPORT_TIP);
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
            gui::set_tooltip(
                "The export in the launch pack, a battle at the selected battle catchment area (else the first): one unit a side",
            );
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
        TERRAIN_TOOL_ID
    }

    fn project_folder(&self) -> &'static str {
        "assembly_kit/raw_data/terrain/battles"
    }

    fn export_menu(&mut self, e: &mut Editor) -> bool {
        let can = e.docs.find_as::<MapDocument>(self.shown).is_some_and(|d| d.model().is_some());
        if gui::menu_item("Built files...", None, false, can && self.export.is_none()) {
            self.export_asked(e);
        }

        if gui::is_item_hovered_allow_disabled() {
            gui::set_tooltip(EXPORT_TIP);
        }

        true
    }

    fn title(&self) -> &str {
        "Terrain"
    }

    fn icon(&self) -> &str {
        icons::MOUNTAIN
    }

    fn draw_panels(&mut self, e: &mut Editor) {
        self.draw_map_panel(e);
        self.map_panel.paint = self.cells.on || self.explicit.placing || self.terry.claims_left_button() || self.placements.claims();
        self.map_panel.outline = self.cells.dragging;
        self.map_panel.marks = e
            .docs
            .find_as::<MapDocument>(self.shown)
            .map(|d| [self.explicit.marks(d), self.placements.marks(d)].concat())
            .unwrap_or_default();
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

        // A built map's placements in the Map panel.
        if self.placements.claims()
            && let Some(i) = self.map_panel.input
        {
            if let Some(d) = e.docs.find_as::<MapDocument>(self.shown) {
                self.placements.hover(d, i.cell);
            }

            if i.pressed {
                match self.placements.click(e, self.shown, i.cell) {
                    Ok(m) => e.status(&m, false),
                    Err(err) => e.status(&err, true),
                }
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
            } else if i.released
                && let Some(m) = self.cells.release(e)
            {
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
        gui::dock_builder_dock_window("Terrain", right);
        gui::dock_builder_dock_window("Inspector", right);
        gui::dock_builder_dock_window("Assets", bottom);
        gui::dock_builder_dock_window("Log", bottom);
        gui::dock_builder_dock_window("History", bottom);
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

        if self.placements.claims() {
            let cell = self.cursor.zip(self.view.frame()).map(|((p, _), f)| {
                let (x, y) = f.cell_of(f64::from(p.x), f64::from(p.z));
                (x.floor() as i64, y.floor() as i64)
            });

            if let Some(c) = cell
                && let Some(d) = e.docs.find_as::<MapDocument>(self.shown)
            {
                self.placements.hover(d, c);
            }

            if ev.button == 0 && ev.kind == wce_editor::tool::ViewportEventKind::Press {
                if let Some(c) = cell {
                    match self.placements.click(e, self.shown, c) {
                        Ok(m) => e.status(&m, false),
                        Err(err) => e.status(&err, true),
                    }
                }

                return true;
            }

            return false;
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

    fn writing(&self) -> Option<String> {
        self.export.as_ref().map(|j| format!("the export of {}", j.map))
    }

    fn open_gesture(&self) -> Option<&'static str> {
        (self.painting.is_some() || self.cells.stroke_open()).then_some("the brush stroke")
    }

    fn claims_left_button(&self, _: &Editor) -> bool {
        self.paint.on || self.cells.on || self.explicit.placing || self.terry.claims_left_button() || self.placements.claims()
    }

    fn update(&mut self, e: &mut Editor) {
        self.poll(e);

        // A campaign map's lighting spheres and skybox, at the camera.
        if let Some(l) = self.lighting.as_mut() {
            l.update(e);
        }

        if let Some(s) = &self.skybox {
            s.update(e);
        }

        if self.shown != 0
            && let Some(name) = e.docs.find_as::<MapDocument>(self.shown).map(|d| map_kind(&d.name).1.to_owned())
        {
            self.lines.poll(e, self.shown, &name);
        }

        // The campaign camera's limits while its map shows (the battle's otherwise).
        let limits = self.camera_limits.as_ref().filter(|(d, _)| *d == self.shown).and_then(|(_, c)| c.get().copied().flatten());
        e.viewport.set_campaign_limits(limits);

        // The map closed: its view goes with it.
        if self.shown != 0 && e.docs.find(self.shown).is_none() {
            self.view.clear(e);
            e.scene.campaign = false;
            e.scene.terrain_units = 0.0;
            e.scene.z_stretch = 1.0;
            e.scene.vegetation_season.clear();
            e.scene.campaign_far = None;
            e.scene.campaign_shroud = None;
            self.lighting = None;
            if let Some(sky) = self.skybox.take() {
                sky.clear(e);
            }

            self.lines.clear(e);

            e.scene.campaign_sea = None;
            e.scene.campaign_snow = None;
            self.cameras.remove(&self.shown);
            self.shown = 0;
        }

        // The view follows the tool's map document.
        let active = e.docs.of_tool(TERRAIN_TOOL_ID).unwrap_or(0);
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
        if let Some(m) = self.launched.as_mut().and_then(wce_launch::launch::Running::poll) {
            self.launched = None;
            e.status(&format!("Launch in game: {m}"), false);
        }

        let frame = self.view.frame().filter(|_| self.shown != 0);
        {
            wce_foundation::wce_prof!("terrain_tool.terry");
            self.terry.update(e, self.shown, frame.clone());
        }

        {
            wce_foundation::wce_prof!("terrain_tool.layout");
            self.layout.update(e, self.shown, frame.clone());
        }

        wce_foundation::wce_prof!("terrain_tool.props");
        self.props.season_bit = 1 << ["sp", "su", "au", "wi"].iter().position(|s| *s == self.season).unwrap_or(1);
        // A campaign map's far material, once the streamer has made it (every tile read: a while after the map opens), and each time it
        // bakes it again over an edit's cells.
        let mut far_ready = false;
        if e.scene.campaign
            && let Some(far) = self.view.streamer.as_ref().and_then(|s| s.far_map())
            && e.scene.campaign_far.as_ref().is_none_or(|f| f.id != Arc::as_ptr(&far) as u64)
            && let Some(frame) = self.view.frame()
        {
            let (sw, ne) = frame.corners();
            e.scene.campaign_far = Some(Arc::new(wce_scene::render_scene::CampaignFar {
                id: Arc::as_ptr(&far) as u64,
                size: far.size,
                diffuse: far.diffuse.clone(),
                normal: far.normal.clone(),
                corner: [sw.0 as f32, sw.1 as f32],
                extent: [(ne.0 - sw.0) as f32, (ne.1 - sw.1) as f32],
            }));
            e.scene.touch();
            far_ready = true;
        }
        // The blocks again, with the tiles' relief it carries: all of them the first time, those over the cells baked again after an edit.
        if far_ready && let Some(s) = self.view.streamer.as_mut() {
            match s.far_region() {
                Some(cells) => s.rebuild_blocks_over(cells),
                None => s.rebuild_far(),
            }
        }

        // A campaign map's vegetation takes the view's season (its forests' `<base>_<season>_diffuse.dds`: spring's `_sp` in the game's
        // turn-1 frame); a battle map's tiles set theirs.
        if e.scene.campaign && e.scene.vegetation_season != self.season {
            e.scene.vegetation_season.clone_from(&self.season);
        }
        let view = &self.view;
        self.props.update(e, self.shown, frame.clone(), &|x, z| view.ground_at(x, z), view.tiles_loaded());
        self.props.pose_animated(e, self.shown);
        wce_foundation::wce_prof!("terrain_tool.figures");
        self.figures.update(e, self.shown, frame, &|x, z| view.ground_at(x, z), self.season == "wi");
    }

    fn register_commands(&self, c: &mut CommandRegistry<Editor>) {
        c.add("terrain_maps", "terrain_maps [text]", "the Terrain tool: the map list as it shows it (the name and, in brackets, where the map is read from), those containing the text", |e, a| {
            let filter = a.first().map(|f| f.to_lowercase()).unwrap_or_default();
            with_terrain(e, |t, e| {
                t.refresh_maps(e);
                let shown: Vec<&str> = t.map_labels.iter().map(String::as_str).filter(|l| l.to_lowercase().contains(&filter)).collect();
                CommandResult::success(format!("{} maps: {}", shown.len(), shown.join("; ")))
            })
        });

        c.add(
            "terrain_open",
            "terrain_open <map | campaign>",
            "the Terrain tool: open a battle map (the map list's name; `campaign`: the game's campaign battle map) and show it",
            |e, a| {
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
            },
        );

        c.add(
            "terrain_start",
            "terrain_start <map | campaign>",
            "the Terrain tool: what its Open button does (the map read on a worker); `terrain_wait` for the view",
            |e, a| {
                let Some(name) = a.first() else { return CommandResult::fail("usage: terrain_start <map | campaign>") };
                let want = if name == "campaign" { wce_map_view::maps::GAME_MAP.to_owned() } else { name.clone() };
                with_terrain(e, |t, e| {
                    t.refresh_maps(e);
                    let want = if t.maps.contains(&want) { want } else { format!("{want}{}", wce_map_view::maps::GAME_FILES) };
                    t.start_open(e, &want);
                    CommandResult::success(format!("{}{}", t.busy, t.status))
                })
            },
        );

        c.add(
            "terrain_wait",
            "terrain_wait [seconds]",
            "the Terrain tool: waits until what Open started is shown (the workers polled as the frames would)",
            |e, a| {
                let limit = a.first().and_then(|v| v.parse::<f64>().ok()).unwrap_or(600.0);
                with_terrain(e, |t, e| {
                    let t0 = std::time::Instant::now();
                    while t.rx.is_some() && t0.elapsed().as_secs_f64() < limit {
                        t.poll(e);
                        std::thread::sleep(std::time::Duration::from_millis(50));
                    }

                    CommandResult::success(format!(
                        "after {:.1} s: shown {}, busy '{}', status '{}'",
                        t0.elapsed().as_secs_f64(),
                        t.shown,
                        t.busy,
                        t.status
                    ))
                })
            },
        );

        c.add("terrain_settle", "terrain_settle [all]", "the Terrain tool: wait until the map's chunks around the camera are on the scene (a campaign's settlements and borders read too); says what it has (all: every tile it could not open)", |e, a| {
            let all = a.first().is_some_and(|v| v == "all");
            with_terrain(e, |t, e| {
                if t.view.streamer.is_none() {
                    return CommandResult::fail("no map shown");
                }

                t.view.settle(e, &t.rings);
                // A campaign map's settlements and borders are read on workers (the startpos takes seconds): waited for, so that a
                // script's next frames draw them (the props' update starts the settlements' read and builds the models once read).
                let mut late = vec![];
                if t.shown != 0
                    && let Some(name) = e.docs.find_as::<MapDocument>(t.shown).filter(|d| map_kind(&d.name).0 == MapKind::Campaign).map(|d| map_kind(&d.name).1.to_owned())
                {
                    let max = std::time::Duration::from_secs(60);
                    let frame = t.view.frame();
                    let view = &t.view;
                    t.props.update(e, t.shown, frame.clone(), &|x, z| view.ground_at(x, z), view.tiles_loaded());
                    if !t.props.wait_cities(max) {
                        late.push("the settlements");
                    }
                    t.figures.update(e, t.shown, frame.clone(), &|x, z| view.ground_at(x, z), t.season == "wi");
                    if !t.figures.wait(max) {
                        late.push("the start's figures");
                    }
                    if !t.lines.wait(e, t.shown, &name, max) {
                        late.push("the borders");
                    }
                    t.props.update(e, t.shown, frame, &|x, z| view.ground_at(x, z), view.tiles_loaded());
                }
                let late = if late.is_empty() { String::new() } else { format!("; still reading {}", late.join(" and ")) };
                let failed = t.view.streamer.as_ref().map(|s| s.failed()).unwrap_or_default();
                let note = if failed.is_empty() { String::new() } else { format!("; could not open: {}", failed.iter().take(if all { usize::MAX } else { 4 }).cloned().collect::<Vec<_>>().join("; ")) };
                CommandResult::success(format!("{}{note}{late}", t.view.status))
            })
        });

        c.add(
            "terrain_ray",
            "terrain_ray <x> <y> [width height]",
            "the Terrain tool: what the camera's ray through a pixel of an image of that size (default 1920 x 1080) meets first, the drawn \
             ground (the LF and the tiles' own heights) or the sea: its map cell and height (metres)",
            |e, a| {
                let v: Vec<f32> = a.iter().filter_map(|x| x.parse().ok()).collect();
                if v.len() != 2 && v.len() != 4 {
                    return CommandResult::fail("usage: terrain_ray <x> <y> [width height]");
                }
                let (w, h) = if v.len() == 4 { (v[2], v[3]) } else { (1920.0, 1080.0) };
                let (o, d) = e.viewport.camera().ray(2.0 * (v[0] + 0.5) / w - 1.0, 1.0 - 2.0 * (v[1] + 0.5) / h, w / h);
                with_terrain(e, |t, _| {
                    let (Some(s), Some(frame)) = (t.view.streamer.as_ref(), t.view.frame()) else {
                        return CommandResult::fail("no map shown");
                    };
                    let m = |y: f32| (f64::from(y) - f64::from(frame.sea_y())) * frame.unit;
                    let mut k = 0.0f32;
                    while k < 2.0e6 {
                        let q = o + d * k;
                        let ground = s.ground_at(&frame, f64::from(q.x), f64::from(q.z));
                        let (cx, cy) = frame.cell_of(f64::from(q.x), f64::from(q.z));
                        if q.y <= ground.max(frame.sea_y()) {
                            let what = if ground >= frame.sea_y() { "ground" } else { "sea" };
                            return CommandResult::success(format!(
                                "{what} at cell {cx:.2}, {cy:.2}: ground {:.1} m, ray {:.1} m",
                                m(ground),
                                m(q.y)
                            ));
                        }
                        k += 2.0;
                    }
                    CommandResult::fail("the ray meets nothing")
                })
            },
        );

        c.add(
            "terrain_far_relief",
            "terrain_far_relief <x> <y> [x1 y1 steps]",
            "the Terrain tool: a campaign map's far material's relief (the tiles' own heights over the LF, metres) at a map cell point, or \
             along a line of them",
            |e, a| {
                let v: Vec<f64> = a.iter().filter_map(|x| x.parse().ok()).collect();
                if v.len() != 2 && v.len() != 5 {
                    return CommandResult::fail("usage: terrain_far_relief <x> <y> [x1 y1 steps]");
                }
                with_terrain(e, |t, e| {
                    let (Some(s), Some(frame)) = (t.view.streamer.as_ref(), t.view.frame()) else {
                        return CommandResult::fail("no map shown");
                    };
                    let Some(far) = s.far_map() else { return CommandResult::fail("no far material yet") };
                    let (w, h) = (frame.map.width, frame.map.height);
                    let steps = if v.len() == 5 { v[4].max(1.0) as usize } else { 0 };
                    let (x1, y1) = if steps > 0 { (v[2], v[3]) } else { (v[0], v[1]) };
                    let mut out = vec![];
                    for k in 0..=steps {
                        let f = if steps == 0 { 0.0 } else { k as f64 / steps as f64 };
                        let (x, y) = (v[0] + (x1 - v[0]) * f, v[1] + (y1 - v[1]) * f);
                        let metres = f64::from(far.relief_at(x, y, w, h)) * frame.unit;
                        out.push(format!("({x:.1}, {y:.1}) {metres:.1}"));
                    }
                    // At one point, the placements over it: their tile, size, turn, flip and the point in their own frame.
                    if steps == 0 {
                        // The LFs there (metres; the sea's level 0).
                        let (fx, fz) = frame.from_map.apply(v[0], v[1]);
                        let m = |y: f32| (f64::from(y) - f64::from(frame.sea_y())) * frame.unit;
                        let sea = frame.sea_lf.as_ref().map(|s| m(s.height_at(fx as f32, fz as f32)));
                        let cliff = |i: usize| frame.map.placements[i].location.to_ascii_lowercase().contains("cliff");
                        out.push(frame.cell_note(v[0].floor() as i32, v[1].floor() as i32, &cliff));
                        out.push(frame.plain_note(v[0].floor() as i32, v[1].floor() as i32));
                        out.push(format!(
                            "land LF {:.1} m, sea LF {sea:.1?} m, ground {:.1} m",
                            m(frame.lift(fx, fz)),
                            m(s.ground_at(&frame, fx, fz))
                        ));
                        // The scene's terrains over it: the height each draws there (metres), the sea pass's, the far ones' (baked).
                        for en in e.scene.entities().iter().filter(|en| e.scene.drawable(en)) {
                            let wce_scene::render_scene::Payload::Terrain(tr) = &en.payload else { continue };
                            if let Some(y) = tr.ground_in(&en.world, fx as f32, fz as f32, None) {
                                out.push(format!(
                                    "terrain {} {}x{} step {:.0} at {:.1} m{}{}{}",
                                    en.id,
                                    tr.nx,
                                    tr.nz,
                                    tr.step,
                                    m(y),
                                    if tr.sea_floor { " sea floor" } else { "" },
                                    if tr.custom.is_some() { " custom" } else { "" },
                                    if tr.material.as_ref().is_some_and(|mt| mt.baked) { " baked" } else { "" },
                                ));
                            }
                        }
                        let (cx, cy) = (v[0].floor() as i32, v[1].floor() as i32);
                        for (i, p) in frame.map.placements.iter().enumerate() {
                            let r = p.rect();
                            if (r[0]..r[2]).contains(&cx) && (r[1]..r[3]).contains(&cy) {
                                let (lx, lz) = p.to_map().inverse().apply(v[0], v[1]);
                                out.push(format!(
                                    "{} {} at {},{} {}x{} turn {:#x} flipped {} climate {} (the map's {} {:?}) local ({lx:.1}, {lz:.1})",
                                    p.location,
                                    p.variation,
                                    p.x,
                                    p.y,
                                    p.w,
                                    p.h,
                                    p.rotation,
                                    p.flipped,
                                    frame.climate_name(i),
                                    p.climate,
                                    frame.map.climate_of(p)
                                ));
                                out.push(format!("  its tile: {}", s.tile_note(&frame, i, (lx, lz))));
                            }
                        }
                    }
                    CommandResult::success(out.join("; "))
                })
            },
        );

        c.add(
            "terrain_far_wait",
            "terrain_far_wait [seconds]",
            "the Terrain tool: wait (scripts; default 900 s) until a campaign map's far material is made (every tile read)",
            |e, a| {
                let max = a.first().and_then(|v| v.parse::<u64>().ok()).unwrap_or(900);
                with_terrain(e, |t, _| {
                    let Some(s) = t.view.streamer.as_ref() else { return CommandResult::fail("no map shown") };
                    let t0 = std::time::Instant::now();
                    while s.far_map().is_none() {
                        if t0.elapsed().as_secs() >= max {
                            return CommandResult::fail(format!("no far material after {max} s"));
                        }
                        std::thread::sleep(std::time::Duration::from_millis(200));
                    }
                    CommandResult::success(format!("far material ready ({:.1?} waited)", t0.elapsed()))
                })
            },
        );

        c.add(
            "terrain_far_dump",
            "terrain_far_dump <file.png>",
            "the Terrain tool: a campaign map's far material's colour as the resolve shows it (the stored colour squared, as the shader \
             does), row 0 the map's north edge: to compare with the game's `s_far_diffuse_map` (docs/plans/campaign_tilemap.md)",
            |e, a| {
                let Some(path) = a.first().cloned() else { return CommandResult::fail("usage: terrain_far_dump <file.png>") };
                with_terrain(e, |t, _| {
                    let Some(far) = t.view.streamer.as_ref().and_then(|s| s.far_map()) else {
                        return CommandResult::fail("no far material yet (terrain_far_wait)");
                    };
                    let png = wce_formats::png_file::PngFile {
                        width: far.size,
                        height: far.size,
                        bit_depth: 8,
                        color_type: 2,
                        pixels: far.diffuse.as_chunks::<4>().0.iter().flat_map(|p| [p[0], p[1], p[2]]).collect(),
                        ..Default::default()
                    };
                    match png.encode().map(|b| std::fs::write(&path, b)) {
                        Ok(Ok(())) => CommandResult::success(format!("{} x {} written to {path}", far.size, far.size)),
                        _ => CommandResult::fail(format!("cannot write {path}")),
                    }
                })
            },
        );

        c.add(
            "terrain_pick",
            "terrain_pick <x> <y>",
            "the Terrain tool: where the ray of a viewport pixel meets the shown map's LF",
            |e, a| {
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
                                (0..200_000)
                                    .map(|k| k as f32 * 0.5)
                                    .find(|&k| {
                                        let q = o + d * k;
                                        q.y <= p.lf.height_at(q.x, q.z)
                                    })
                                    .map(|k| o + d * k)
                            });
                            CommandResult::fail(format!("no LF under that pixel (ray {ray:?}; a march finds {march:?})"))
                        }
                    }
                })
            },
        );

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

        c.add(
            "terrain_brush",
            "terrain_brush on|off",
            "the Terrain tool: the LF brush on the left button (the LF brush panel's paint box), with its settings as they are",
            |e, a| {
                let on = match a.first().map(String::as_str) {
                    Some("on") => true,
                    Some("off") => false,
                    _ => return CommandResult::fail("usage: terrain_brush on|off"),
                };

                with_terrain(e, |t, e| {
                    if on && !e.docs.find_as::<MapDocument>(t.shown).is_some_and(|d| d.lf_field(LfRole::Land).is_some()) {
                        return CommandResult::fail("the shown map has no LF to paint (a map of the packs)");
                    }

                    t.paint.on = on;
                    CommandResult::success("")
                })
            },
        );

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

        c.add(
            "terrain_lf_patches",
            "terrain_lf_patches",
            "the Terrain tool: Terry's Apply Per-Tile LF Patches on the shown map (the custom-mesh tiles' lf_mask.tif into the land LF)",
            |e, _| {
                with_terrain(e, |t, e| match t.apply_lf_patches(e) {
                    Ok(m) => CommandResult::success(m),
                    Err(err) => CommandResult::fail(err),
                })
            },
        );

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

        c.add("terrain_export", "terrain_export [folder] [--wait]", "the Terrain tool: File > Export > Built files of the shown kit map, loose under the folder and as a pack (default: its last one, else the projects folder); --wait: until done", |e, a| {
            let out = a.iter().find(|v| *v != "--wait").map_or_else(|| e.export_dir(EXPORT_KEY), PathBuf::from);
            let started = with_terrain(e, |t, e| match t.start_export(e, out) {
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

        c.add(
            "terrain_refill_wait",
            "terrain_refill_wait [seconds]",
            "the Terrain tool: waits for the fill run again after a cell edit (scripts)",
            |e, a| {
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
            },
        );

        c.add("terrain_placements", "terrain_placements count | select <x> <y> | place <x> <y> <tile> [variation] [degrees] | move <x> <y> | turn | remove | variation <k>", "the Terrain tool: a built map's placements (Rome 2's pack maps; cells; on the selected one; one step on the history each)", |e, a| {
            use crate::placements::{Mode, PlacementsPanel};
            let usage = "usage: terrain_placements count | select <x> <y> | place <x> <y> <tile> [variation] [degrees] | move <x> <y> | turn | remove | variation <k>";
            let num = |i: usize| a.get(i).and_then(|v| v.parse::<i64>().ok());
            with_terrain(e, |t, e| {
                let doc = t.shown;
                let at = num(1).zip(num(2));
                let click = |t: &mut TerrainTool, e: &mut Editor, mode: Mode, at: (i64, i64)| {
                    let was = t.placements.mode;
                    t.placements.mode = mode;
                    let r = t.placements.click(e, doc, at);
                    t.placements.mode = was;
                    r
                };

                let selected = t.placements.selected.ok_or_else(|| "select a placement first".to_owned());
                let r = match (a.first().map(String::as_str), at) {
                    (Some("count"), _) => e.docs.find_as::<MapDocument>(doc).and_then(|d| d.model()).and_then(|m| m.tile_list.as_ref())
                        .map(|l| format!("{} placements, {} tiles, bounds {:?}", l.get().placements.len(), l.get().names.len(), l.get().bounds))
                        .ok_or_else(|| "no placements to edit (a built map with a terrain.tile_list)".to_owned()),
                    (Some("select"), Some(at)) => click(t, e, Mode::Select, at),
                    (Some("move"), Some(at)) => click(t, e, Mode::Move, at),
                    (Some("place"), Some(at)) if a.len() >= 4 => {
                        let v = num(4).unwrap_or(0) as i32;
                        let deg = num(5).unwrap_or(0) as i32;
                        t.placements.choose(&a[3], v, deg);
                        click(t, e, Mode::Place, at)
                    }

                    (Some("turn"), _) => selected.and_then(|i| PlacementsPanel::turn(e, doc, i)),
                    (Some("remove"), _) => selected.and_then(|i| {
                        t.placements.selected = None;
                        PlacementsPanel::remove(e, doc, i)
                    }),
                    (Some("variation"), _) => match (selected, num(1)) {
                        (Ok(i), Some(k)) => PlacementsPanel::set_variation(e, doc, i, k.clamp(0, 255) as u8),
                        (Err(err), _) => Err(err),
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

        c.add(
            "terrain_explicit",
            "terrain_explicit add <x> <y> <tile or variation path> <degrees> | remove <i> | list",
            "the Terrain tool: the shown kit map's explicit tiles (one step on the history each)",
            |e, a| {
                let usage = "usage: terrain_explicit add <x> <y> <path> <degrees> | remove <i> | list";
                with_terrain(e, |t, e| {
                    let shown = t.shown;
                    match (a.first().map(String::as_str), &a.get(1..).unwrap_or_default()) {
                        (Some("add"), [x, y, path, deg]) => {
                            let (Ok(x), Ok(y), Ok(deg)) = (x.parse::<i64>(), y.parse::<i64>(), deg.parse::<i32>()) else {
                                return CommandResult::fail(usage);
                            };
                            t.explicit.choose(path, deg);
                            match t.explicit.place(e, shown, (x, y)) {
                                Ok(m) => CommandResult::success(m),
                                Err(err) => CommandResult::fail(err),
                            }
                        }

                        (Some("remove"), [i]) => match i.parse::<usize>() {
                            Ok(i) if crate::explicit_tiles::ExplicitPanel::remove(e, shown, i) => {
                                CommandResult::success(format!("explicit tile {i} removed"))
                            }
                            _ => CommandResult::fail("no such explicit tile"),
                        },

                        (Some("list"), []) => {
                            let d = e.docs.find_as::<MapDocument>(shown);
                            let lines: Vec<String> = d
                                .and_then(|d| d.model())
                                .and_then(|m| m.explicit.as_ref())
                                .map(|x| x.get().tiles().map(|t| format!("{},{},{},{}", t.x, t.y, t.path, t.rotation)).collect())
                                .unwrap_or_default();
                            CommandResult::success(format!("{} explicit tiles: {}", lines.len(), lines.join("; ")))
                        }

                        _ => CommandResult::fail(usage),
                    }
                })
            },
        );

        c.add("terrain_roads", "terrain_roads [on|off]", "the Terrain tool: a campaign's roads drawn at its start's levels (its regions' buildings: on) or as the kit paints them (off); then what they are", |e, a| {
            with_terrain(e, |t, e| {
                let id = t.shown;
                let Some(d) = e.docs.find_as_mut::<MapDocument>(id) else { return CommandResult::fail("no map shown") };
                let Some(roads) = d.view_roads.clone() else { return CommandResult::fail("the shown map has no roads at a start (a campaign map; its startpos and db read)") };
                let was = d.roads_at_start;
                match a.first().map(String::as_str) {
                    Some("on") => d.roads_at_start = true,
                    Some("off") => d.roads_at_start = false,
                    None => {}
                    Some(_) => return CommandResult::fail("usage: terrain_roads [on|off]"),
                }

                let on = d.roads_at_start;
                let (swapped, farmed, first) = if on {
                    let (m, s, f) = roads.apply(&d.placed);
                    // The first few farmland tiles' cells (to look at them).
                    let first: Vec<String> = m.placements.iter().zip(&d.placed.placements).filter(|(a, b)| a.location != b.location && a.location.to_lowercase().contains("farmland"))
                        .take(3).map(|(a, _)| format!("({}, {}) {}", a.x, a.y, a.location)).collect();
                    (s, f, first)
                } else {
                    (0, 0, vec![])
                };
                if on != was {
                    t.start_view(e, id);
                }

                CommandResult::success(format!("roads at the start {}: regions a level {:?}, {swapped} road tiles upgraded, {farmed} tiles farmland{}", if on { "on" } else { "off" }, roads.level_counts(),
                    if first.is_empty() { String::new() } else { format!(" (first at cells {})", first.join("; ")) }))
            })
        });

        c.add(
            "terrain_season",
            "terrain_season [sp|su|au|wi]",
            "the Terrain tool: a campaign map's season in the view (its ground textures and snow mask); then which",
            |e, a| {
                with_terrain(e, |t, e| {
                    match a.first().map(String::as_str) {
                        Some(v @ ("sp" | "su" | "au" | "wi")) => t.set_season(e, v),
                        None => {}
                        Some(_) => return CommandResult::fail("usage: terrain_season [sp|su|au|wi]"),
                    }

                    let snow = e.scene.campaign_snow.as_ref().map_or("no snow mask".to_owned(), |s| format!("snow {}", s.mask));
                    CommandResult::success(format!("season {} ({snow}, as of the last view)", t.season))
                })
            },
        );

        c.add(
            "terrain_faction",
            "terrain_faction [key]",
            "the Terrain tool: the faction a campaign map's borders and fog of war are for, as the panel's choice (none: the factions there are)",
            |e, a| {
                with_terrain(e, |t, e| {
                    let keys: Vec<String> = t.lines.factions.iter().map(|f| f.key.clone()).collect();
                    let Some(key) = a.first() else { return CommandResult::success(keys.join(", ")) };
                    if !keys.contains(key) {
                        return CommandResult::fail(format!("no faction {key} (the map's: {})", keys.join(", ")));
                    }
                    let id = t.shown;
                    let Some(name) = e.docs.find_as::<MapDocument>(id).map(|d| map_kind(&d.name).1.to_owned()) else {
                        return CommandResult::fail("no map shown");
                    };
                    t.lines.faction = Some(key.clone());
                    t.lines.start(e, id, &name, Some(key.clone()));
                    CommandResult::success(format!("borders and fog of war for {key}"))
                })
            },
        );

        c.add(
            "terrain_fog",
            "terrain_fog [on|off]",
            "the Terrain tool: a campaign map's fog of war at the start for the borders' faction (read on a worker); then whether it is shown",
            |e, a| {
                with_terrain(e, |t, e| {
                    let on = match a.first().map(String::as_str) {
                        Some("on") => Some(true),
                        Some("off") => Some(false),
                        None => None,
                        Some(_) => return CommandResult::fail("usage: terrain_fog [on|off]"),
                    };
                    if let Some(on) = on
                        && on != t.lines.fog_on
                    {
                        t.lines.fog_on = on;
                        let name = e.docs.find_as::<MapDocument>(t.shown).map(|d| map_kind(&d.name).1.to_owned());
                        if let Some(name) = name {
                            let pick = t.lines.faction.clone();
                            t.lines.start(e, t.shown, &name, pick);
                        }
                    }

                    CommandResult::success(format!(
                        "fog of war {} (loaded: {}, as of the last view)",
                        if t.lines.fog_on { "on" } else { "off" },
                        t.lines.fog.is_some()
                    ))
                })
            },
        );

        c.add(
            "terrain_weather",
            "terrain_weather [list | auto | <file or file name>]",
            "the Terrain tool: a campaign map's weather in the view (its lighting files; auto: the season's global lighting); then which",
            |e, a| {
                with_terrain(e, |t, e| {
                    let Some(d) = e.docs.find_as::<MapDocument>(t.shown) else { return CommandResult::fail("no map shown") };
                    let weathers = TerrainTool::campaign_weathers(e, d);
                    match a.first().map(String::as_str) {
                        None => {}
                        Some("list") => return CommandResult::success(weathers.join("; ")),
                        Some("auto") => t.set_weather(e, None),
                        Some(w) => match weathers.iter().find(|x| x.as_str() == w || x.ends_with(&format!("/{w}"))) {
                            Some(f) => t.set_weather(e, Some(f.clone())),
                            None => return CommandResult::fail(format!("no weather {w} (terrain_weather list)")),
                        },
                    }

                    let weights = t.lighting.as_ref().map(|l| format!("; at the camera: {}", l.weights_note(e))).unwrap_or_default();
                    CommandResult::success(format!(
                        "weather {}{weights}",
                        t.weather.as_deref().unwrap_or("automatic (the season's global lighting)")
                    ))
                })
            },
        );

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

        c.add(
            "terrain_figures",
            "terrain_figures [on|off] [radius cells]",
            "the Terrain tool: the campaign start's armies, navies and agents near the camera: set, then how many it drew",
            |e, a| {
                with_terrain(e, |t, _| {
                    for v in a {
                        match v.as_str() {
                            "on" => t.figures.on = true,
                            "off" => t.figures.on = false,
                            n => {
                                if let Ok(r) = n.parse() {
                                    t.figures.radius_cells = r;
                                }
                            }
                        }
                    }

                    CommandResult::success(format!(
                        "start figures {}: {} of {} within {} cells (as of the last frame)",
                        if t.figures.on { "on" } else { "off" },
                        t.figures.count,
                        t.figures.total,
                        t.figures.radius_cells
                    ))
                })
            },
        );

        c.add("terrain_props", "terrain_props [on|off] [radius cells]", "the Terrain tool: a campaign's prop models and trees near the camera: set, then how many it drew", |e, a| {
            with_terrain(e, |t, _| {
                for v in a {
                    match v.as_str() {
                        "on" => t.props.on = true,
                        "off" => t.props.on = false,
                        n => {
                            if let Ok(r) = n.parse() {
                                t.props.radius_cells = r;
                            }
                        }
                    }
                }

                // The trees' own heights against the ground they are stood on (the list's units): their median and spread.
                let mut d: Vec<f32> = t.props.heights.iter().map(|(y, g)| g - y).collect();
                d.sort_by(f32::total_cmp);
                let q = |f: f32| d.get(((d.len() as f32 * f) as usize).min(d.len().saturating_sub(1))).copied().unwrap_or(0.0);
                let mut r: Vec<f32> = t.props.heights.iter().filter(|(y, _)| y.abs() > 0.05).map(|(y, g)| g / y).collect();
                r.sort_by(f32::total_cmp);
                let rq = |f: f32| r.get(((r.len() as f32 * f) as usize).min(r.len().saturating_sub(1))).copied().unwrap_or(0.0);
                CommandResult::success(format!(
                    "prop models {}: {} props ({} moving), {} trees (ahead to 240 world units), {} settlement buildings within {} cells, {} placed effects {:?} (the nearest: {:?}) (as of the last frame); ground - tree y (list units) 10/50/90 %: {:.3} {:.3} {:.3}; ground / tree y: {:.3} {:.3} {:.3}",
                    if t.props.on { "on" } else { "off" }, t.props.count, t.props.animated_count, t.props.tree_count, t.props.city_count, t.props.radius_cells, t.props.effect_count, t.props.effect_names, t.props.effect_near, q(0.1), q(0.5), q(0.9), rq(0.1), rq(0.5), rq(0.9)
                ))
            })
        });

        c.add(
            "terrain_validate",
            "terrain_validate",
            "the Terrain tool: the shown map's checks (the Validation section): a count, then each finding",
            |e, _| {
                with_terrain(e, |t, e| {
                    let m = t.validate(e);
                    let lines: Vec<String> = t.findings.iter().map(|f| format!("{:?}: {}", f.severity, f.text)).collect();
                    CommandResult::success(format!("{m}{}{}", if lines.is_empty() { "" } else { "\n" }, lines.join("\n")))
                })
            },
        );

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
                    (Some("zone"), n) if n >= 7 && nums.len() == n - 1 && nums.len().is_multiple_of(2) => {
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

        c.add(
            "terrain_view",
            "terrain_view <distance> [near mid far [ground]]",
            "the Terrain tool: the camera's view distance (scene units) and the rings (cells)",
            |e, a| {
                let Some(d) = a.first().and_then(|v| v.parse::<f32>().ok()) else {
                    return CommandResult::fail("usage: terrain_view <distance> [near mid far [ground]]");
                };
                let rings: Vec<i32> = a[1..].iter().filter_map(|v| v.parse().ok()).collect();
                with_terrain(e, |t, _| {
                    t.rings.view_distance = d.max(GAME_FAR_PLANE);
                    if let [near, mid, far, ..] = rings[..] {
                        (t.rings.near, t.rings.mid, t.rings.far) = (near, mid, far);
                    }

                    if let Some(&ground) = rings.get(3) {
                        t.rings.ground = ground;
                    }

                    // Given rings hold for both kinds of map (a script may set them before it opens one).
                    if !rings.is_empty() {
                        t.rings_other = t.rings;
                        t.rings_edited = [true, true];
                    }

                    t.view.set_rings(&t.rings);
                    CommandResult::success(format!(
                        "view distance {}, rings {} / {} / {} / {}",
                        t.rings.view_distance, t.rings.near, t.rings.mid, t.rings.ground, t.rings.far
                    ))
                })
            },
        );

        c.add(
            "terrain_cam_game",
            "terrain_cam_game <eye x y z> <forward x y z>",
            "the Terrain tool: a campaign map's camera where the game's is (its campaign world's units, as a capture's `camera_VS_PS`: the \
             eye and the view's forward axis, docs/plans/campaign_tilemap.md)",
            |e, a| {
                let v: Vec<f64> = a.iter().filter_map(|x| x.parse().ok()).collect();
                if v.len() != 6 || a.len() != 6 {
                    return CommandResult::fail("usage: terrain_cam_game <eye x y z> <forward x y z>");
                }

                with_terrain(e, |t, e| {
                    let Some(d) = e.docs.find_as::<MapDocument>(t.shown) else { return CommandResult::fail("no map shown") };
                    if map_kind(&d.name).0 != MapKind::Campaign {
                        return CommandResult::fail("not a campaign map");
                    }

                    // The game's world is the `.terry`'s with z times the stretch (from the map's south-west corner, as both are); the
                    // scene's y is the world's over the frame's unit (the sea at 0).
                    let tf = d.terry_frame;
                    let scene = |x: f64, y: f64, z: f64| {
                        let (sx, sz) = tf.to_frame(x, z / tf.z_stretch);
                        Float3::new(sx as f32, (y / tf.unit) as f32, sz as f32)
                    };
                    let eye = scene(v[0], v[1], v[2]);
                    // The target where the view meets the sea's height, else 10 world units on (a view up or level).
                    let reach = if v[4] < -1e-6 { (v[1] / -v[4]).min(400.0) } else { 10.0 };
                    let target = scene(v[0] + v[3] * reach, v[1] + v[4] * reach, v[2] + v[5] * reach);
                    e.viewport.set_mode(wce_editor_core::camera_controller::CameraMode::Orbit);
                    let cam = wce_scene::camera::Camera { eye, target, ..e.viewport.camera() };
                    e.viewport.controller().set_camera(&cam);
                    CommandResult::success(format!(
                        "camera at {:.1} {:.1} {:.1}, looking at {:.1} {:.1} {:.1}",
                        eye.x, eye.y, eye.z, target.x, target.y, target.z
                    ))
                })
            },
        );
    }

    /// The map read from its copy on a worker (`restore_map`); its document comes in when the read is done.
    fn restore(
        &mut self,
        e: &mut Editor,
        copy: &Path,
        meta: &wce_editor_core::autosave::AutosaveMeta,
    ) -> Result<Option<Box<dyn wce_editor_core::document::Document>>, String> {
        self.restore_map(e, copy, &meta.open).map(|()| None)
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}
