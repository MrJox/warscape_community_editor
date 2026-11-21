//! The battle's real ground under the scene (docs/plans/cinematic.md, step 4): the Tile tool's map streamer (`wce_map_view::MapView`
//! over `wce_tile_editor`'s context streamer) on the battle's map from the packs. A place on a campaign map: the map around that
//! position (`tile_map_position`, the scene's origin there); a whole battle map: around its centre; one battle tile: the tile alone,
//! as a map of one placement. The packs' tiles come from the Tile tool's tree of them (`PacksTree`, decompiled into the cache the
//! first time), the kits' first. Read on a worker; the flat grass stands in until it is there, or when it can't be read.

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::mpsc::{Receiver, channel};

use wce_cinematic::scene::Project;
use wce_editor::editor::Editor;
use wce_map_view::maps::{GAME_FILES, PacksTree, load_map};
use wce_map_view::{MapView, Rings};
use wce_scene::render_scene::DocumentId;
use wce_tile::battle_map::context::{MapPlacement, PlacedMap, Spot};
use wce_tile::battle_map::tile_db::TileDatabase;
use wce_tile_editor::terrain_bake::TerrainLook;
use wce_tile_editor::tile_document::context::{ContextRequest, Streamer, TileProvider, cache};

/// A battle map's scene units: 2 game units (metres) a scene unit, as the Terrain tool draws battle maps.
pub const UNIT: f64 = 2.0;

/// What the ground is of: the map (or the tile), where, the season, the packs mounted.
#[derive(Clone, Debug, PartialEq)]
struct Key {
    map: String,
    tile: String,
    whole: bool,
    /// The position in millionths of the map.
    at: [i32; 2],
    season: String,
    mount: u64,
}

impl Key {
    fn of(p: &Project, mount: u64) -> Self {
        Self {
            map: p.map.campaign_map.trim().trim_matches('/').to_owned(),
            tile: p.map.tile.trim().trim_matches('/').to_owned(),
            whole: p.map.whole_map,
            at: p.map.position.map(|v| (v * 1_000_000.0).round() as i32),
            season: match p.battle.season.to_lowercase().as_str() {
                "autumn" => "au",
                "winter" => "wi",
                _ => "su",
            }
            .to_owned(),
            mount,
        }
    }
}

#[derive(Default)]
pub struct Ground {
    view: MapView,
    rings: Rings,
    shown: Option<Key>,
    building: Option<(Key, Receiver<Result<Box<Streamer>, String>>)>,
    /// The key that failed (not tried again until it changes) and why.
    failed: Option<Key>,
    packs: Option<Arc<PacksTree>>,
    pub status: String,
    arrived: bool,
    /// The project's key and since when it is so (a drag of the position starts one read when it stops).
    settling: Option<(Key, std::time::Instant)>,
}

/// The `raw_data` folders tiles are looked for in: the projects' kit layout, then the Assembly Kits (with a `terrain` folder).
fn kit_roots(e: &Editor) -> Vec<PathBuf> {
    let mut roots = e.settings_ref().assembly_kit_raw_dirs();
    roots.retain(|raw| raw.join("terrain").is_dir());
    let projects = e.settings_ref().kit_projects_dir().join("raw_data");
    if projects.join("terrain").is_dir() {
        roots.insert(0, projects);
    }
    roots
}

/// A kit with the tile databases (both, else the battle one).
fn database_root(roots: &[PathBuf]) -> Option<PathBuf> {
    let has = |r: &PathBuf, kind: &str| r.join(format!("terrain/tiles/{kind}/_tile_database")).is_dir();
    roots.iter().find(|r| has(r, "battle") && has(r, "campaign")).or_else(|| roots.iter().find(|r| has(r, "battle"))).cloned()
}

/// One battle tile as a map of its one placement (`terrain/tiles/battle/<set>/<tile>/<variation>`).
fn tile_map(db: &TileDatabase, folder: &str) -> Result<PlacedMap, String> {
    let folder = folder.replace('\\', "/");
    let (location, variation) = folder.rsplit_once('/').ok_or_else(|| format!("{folder} is not a tile's variation folder"))?;
    let norm = |s: &str| s.replace('/', "\\").trim_matches('\\').to_lowercase();
    let t = db.tiles.iter().find(|t| norm(&t.location) == norm(location)).ok_or_else(|| format!("{location}: not in the tile database"))?;
    let (w, h) = (t.width.max(1), t.height.max(1));
    Ok(PlacedMap {
        name: folder.clone(),
        width: w as u32,
        height: h as u32,
        placements: vec![MapPlacement {
            location: location.replace('/', "\\"),
            variation: variation.to_owned(),
            x: 0,
            y: 0,
            w,
            h,
            rotation: 0x10,
            flipped: false,
            climate: 0,
            any_variation: false,
        }],
        ..PlacedMap::default()
    })
}

impl Ground {
    /// The real ground is on the scene (the flat grass goes).
    pub fn shown(&self) -> bool {
        self.view.streamer.is_some()
    }

    /// The ground's height at a scene point (metres), 0 without the map.
    pub fn height(&self, x: f32, z: f32) -> f32 {
        if !self.shown() {
            return 0.0;
        }
        (self.view.ground_at(f64::from(x) / UNIT, f64::from(z) / UNIT) as f64 * UNIT) as f32
    }

    /// The heights as a function of a point (metres), while the ground is shown.
    pub fn heights(&self) -> Option<impl Fn(f32, f32) -> f32 + '_> {
        self.shown().then_some(move |x: f32, z: f32| self.height(x, z))
    }

    /// Streams around the viewport's camera now (a movie's frame): to idle in scripts.
    pub fn follow(&mut self, e: &mut Editor) {
        if self.view.streamer.is_some() {
            if e.scripted_input() {
                self.view.settle(e, &self.rings);
            } else {
                self.view.stream(e, &self.rings);
            }
        }
    }

    /// The ground arrived since the last call (the tool frames the camera on it once).
    pub fn take_arrived(&mut self) -> bool {
        std::mem::take(&mut self.arrived)
    }

    pub fn clear(&mut self, e: &mut Editor) {
        self.view.clear(e);
        self.shown = None;
        self.building = None;
    }

    /// Every frame: the ground of the project's map read when it changes, streamed around the camera. Scripts wait for it.
    pub fn update(&mut self, e: &mut Editor, doc: DocumentId, p: &Project) {
        let mount = e.files().map_or(0, |f| f.mount_id());
        let key = Key::of(p, mount);
        let wanted = !key.map.is_empty() || !key.tile.is_empty();
        // Read when it has been the same a moment (scripts at once).
        let steady = e.scripted_input()
            || match &self.settling {
                Some((k, since)) if *k == key => since.elapsed().as_secs_f32() > 0.6,
                _ => {
                    self.settling = Some((key.clone(), std::time::Instant::now()));
                    false
                }
            };
        if steady
            && self.shown.as_ref() != Some(&key)
            && self.building.as_ref().is_none_or(|(k, _)| *k != key)
            && self.failed.as_ref() != Some(&key)
            && wanted
        {
            self.start(e, key.clone());
        }
        let scripted = e.scripted_input();
        if let Some((k, rx)) = &self.building {
            let got = if scripted { rx.recv().ok() } else { rx.try_recv().ok() };
            if let Some(r) = got {
                let k = k.clone();
                self.building = None;
                match r {
                    Ok(s) => {
                        e.scene.game_units = UNIT as f32;
                        self.view.begin(e, doc, s, &self.rings, |_| vec![]);
                        if scripted {
                            self.view.settle(e, &self.rings);
                        }
                        self.status.clear();
                        self.shown = Some(k);
                        self.arrived = true;
                    }
                    Err(err) => {
                        self.status = format!("the ground: {err}");
                        wce_foundation::log::warn(format!("cinematic: {}", self.status));
                        self.failed = Some(k);
                    }
                }
            }
        }
        if self.view.streamer.is_some() {
            if scripted {
                self.view.settle(e, &self.rings);
            } else {
                self.view.stream(e, &self.rings);
            }
        }
    }

    /// Reads the map and makes its streamer on a worker.
    fn start(&mut self, e: &mut Editor, key: Key) {
        let (Some(vfs), Some(files)) = (e.vfs().cloned(), e.files()) else { return };
        let roots = kit_roots(e);
        let kits = wce_tile::sources::kit_raw_roots(e.settings_ref());
        if self.packs.as_ref().is_none_or(|t| !t.same_sources(&vfs, &kits)) {
            self.packs =
                Some(Arc::new(PacksTree::new(vfs, kits.clone(), &e.settings_ref().paths().cache_dir().join("neighbours"), e.game().key)));
        }
        let packs = self.packs.clone().expect("made above");
        let game_dir = e.game().campaign_battle_map;
        let raw_root = database_root(&roots);
        let game_kit = wce_tile::vegetation::params_kit(&kits);
        let bake_ground = e.game().key != wce_vfs::GameProfile::ROME_2.key;
        let rings = self.rings;
        self.status = "reading the battle map's ground...".to_owned();
        let (tx, rx) = channel();
        let k = key.clone();
        std::thread::spawn(move || {
            let r = (|| {
                // The packs' tiles in the kit's layout (made once, in the cache): their database names a pack map's tiles.
                let packs_raw = packs.raw();
                let db_root = packs_raw.clone().ok().or_else(|| raw_root.clone()).ok_or("no tile database: the game's tile data")?;
                let db = Arc::new(TileDatabase::load(&db_root.join("terrain/tiles/battle/_tile_database"))?);
                let (map, spot) = if k.tile.is_empty() {
                    let (map, _) = load_map(&format!("{}{GAME_FILES}", k.map), game_dir, &db_root, &db, Some(files.as_ref()))?;
                    let spot = if k.whole {
                        Spot { x: (map.width / 2) as i32, y: (map.height / 2) as i32, rotation: 0x10 }
                    } else {
                        Spot {
                            x: (f64::from(k.at[0]) / 1e6 * f64::from(map.width)).round() as i32,
                            y: (f64::from(k.at[1]) / 1e6 * f64::from(map.height)).round() as i32,
                            rotation: 0x10,
                        }
                    };
                    (map, spot)
                } else {
                    let map = tile_map(&db, &k.tile)?;
                    let spot = Spot { x: (map.width / 2) as i32, y: (map.height / 2) as i32, rotation: 0x10 };
                    (Arc::new(map), spot)
                };
                let t = packs.clone();
                let provider: TileProvider = Arc::new(move |location: &str, variation: &str| t.tile(location, variation));
                let rq = ContextRequest {
                    map,
                    spot,
                    cells: (0, 0),
                    unit: UNIT,
                    z_stretch: 1.0,
                    near: rings.near,
                    mid: rings.mid,
                    ground: rings.ground,
                    far: rings.far,
                    raw_root: raw_root.clone().or(packs_raw.ok()).ok_or("no kit with a tile database")?,
                    tile_roots: roots,
                    game_kit,
                    look: TerrainLook { season: k.season.clone(), ..TerrainLook::default() },
                    trees: true,
                    bake_ground,
                    plain: None,
                    groups: None,
                    cache: Some(cache::cache_dir()),
                    provider: Some(provider),
                };
                Ok::<_, String>(Box::new(Streamer::new(rq, Some(files))))
            })();
            let _ = tx.send(r);
        });
        self.building = Some((key, rx));
    }
}
