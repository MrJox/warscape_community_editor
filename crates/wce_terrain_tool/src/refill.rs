//! BOB's fill run again after a kit map's tile map, climates or explicit tiles change (docs/plans/terrain_editor.md, Phase 4): the
//! model's layers are taken as they are on the UI thread, filled on a worker (the whole map: the fill draws from one random sequence in
//! row order, so one painted cell can change every placement after it, decision 3), and the chunks whose placements differ are built
//! again with a frame over the new placements; the old ones stay shown until then.

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::mpsc::{Receiver, channel};
use std::time::Instant;

use wce_scene::render_scene::DocumentId;
use wce_terrain::{LayerRole, LfRole};
use wce_terrain_editor::MapDocument;
use wce_tile::battle_map::context::{LfField, MapPlacement, PlacedMap};
use wce_tile::battle_map::fill::MapImages;
use wce_tile::battle_map::tile_db::TileDatabase;
use wce_tile::battle_map::tile_list::TileList;
use wce_tile::battle_map::{fill_list, place_images};
use wce_tile_editor::tile_document::context::{CHUNK, ChunkKey, Frame, cache};

/// A fill done.
pub struct Done {
    pub doc: DocumentId,
    pub map: Arc<PlacedMap>,
    pub frame: Arc<Frame>,

    /// The chunks whose placements differ from the shown ones'.
    pub changed: Vec<ChunkKey>,
    pub secs: f64,

    /// BOB's messages ("Failed to find tile for point ...").
    pub messages: Vec<String>,
}

/// What a fill reads of a map's model.
pub struct FillInputs {
    w: u32,
    h: u32,
    tiles: Vec<[u8; 3]>,
    climate: Option<Vec<[u8; 3]>>,
    explicit: Option<String>,
    lf: Option<LfField>,

    /// A built map's battle field: the placements are its, no fill.
    list: Option<wce_tile::battle_map::tile_list::TileList>,
}

impl FillInputs {
    /// The model's tile map, climates, explicit tiles and LF as they are (none without a tile map).
    pub fn of(m: &wce_terrain::MapModel) -> Option<Self> {
        let lf_of = |m: &wce_terrain::MapModel| {
            m.lf(LfRole::Land).map(|f| {
                // The model's own samples (shared: no copy).
                LfField::shared(f.get().grid().clone())
            })
        };

        if let Some(list) = m.tile_list.as_ref().filter(|_| m.built) {
            let l = list.get();
            return Some(Self {
                w: l.battle_size[0],
                h: l.battle_size[1],
                tiles: vec![],
                climate: None,
                explicit: None,
                lf: lf_of(m),
                list: Some(l.clone()),
            });
        }

        let tiles = m.layer(&LayerRole::TileMap).map(|l| l.get())?;
        let (w, h) = (tiles.width(), tiles.height());
        let climate = m.layer(&LayerRole::Climate).map(|l| l.get()).filter(|l| l.width() == w && l.height() == h).map(rgb);
        let explicit = m.explicit.as_ref().map(|x| x.get().write().iter().map(|&c| char::from(c)).collect());
        Some(Self { w, h, tiles: rgb(tiles), climate, explicit, lf: lf_of(m), list: None })
    }

    /// BOB's fill of them: the placements (with the LF) and BOB's messages.
    pub fn fill(&self, db: &TileDatabase, name: &str) -> (PlacedMap, Vec<String>) {
        if let Some(list) = &self.list {
            return (PlacedMap::from_tile_list(name, list, db, self.lf.clone()), vec![]);
        }

        // The LF from `self.lf`, read by the fill's LF step under its lock (`place_images`).
        let lf = None;
        let images = MapImages {
            width: self.w,
            height: self.h,
            tile_map: &self.tiles,
            climate_map: self.climate.as_deref(),
            explicit_tiles: self.explicit.as_deref(),
            lf: None,
            lf_u16: lf,
            // The header only: the placements are the same.
            campaign: false,
        };

        place_images(db, name, &images, self.lf.clone())
    }

    /// [`Self::fill`] from the cache when the same inputs were filled before (a kit campaign map's fill is half a minute on one thread,
    /// at every open), else filled and kept there; whether it came from the cache.
    pub fn fill_cached(&self, db: &TileDatabase, name: &str) -> (PlacedMap, Vec<String>, bool) {
        if self.list.is_some() {
            let (m, messages) = self.fill(db, name);
            return (m, messages, false);
        }

        let dir = fill_cache_dir();
        let key = self.key(db);
        if let Some((list, messages)) = read_fill(&dir, key) {
            return (PlacedMap::from_tile_list(name, &list, db, self.lf.clone()), messages, true);
        }

        let images = MapImages {
            width: self.w,
            height: self.h,
            tile_map: &self.tiles,
            climate_map: self.climate.as_deref(),
            explicit_tiles: self.explicit.as_deref(),
            lf: None,
            lf_u16: None,
            campaign: false,
        };
        let (list, messages) = fill_list(db, &images, self.lf.as_ref());
        if let Err(e) = write_fill(&dir, key, &list, &messages) {
            wce_foundation::log::info(format!("{name}: its fill not cached: {e}"));
        }

        (PlacedMap::from_tile_list(name, &list, db, self.lf.clone()), messages, false)
    }

    /// What a fill depends on: the cells' colours, the climates, the explicit tiles, the LF's samples and the tile database.
    fn key(&self, db: &TileDatabase) -> u64 {
        let mut h = cache::hash(0xCBF2_9CE4_8422_2325, FILL_MAGIC);
        h = cache::hash(h, &FILL_VERSION.to_le_bytes());
        h = cache::hash(h, &[self.w, self.h].map(u32::to_le_bytes).concat());
        h = cache::hash(h, self.tiles.as_flattened());
        h = cache::hash(h, self.climate.as_deref().map_or(&[][..], <[[u8; 3]]>::as_flattened));
        h = cache::hash(h, self.explicit.as_deref().unwrap_or_default().as_bytes());
        h = cache::hash(h, format!("{db:?}").as_bytes());
        // The LF a word at a time (190 million samples on main_attila).
        if let Some(lf) = &self.lf {
            let v = lf.values.read();
            let mut x = h ^ u64::from(lf.w) << 32 ^ u64::from(lf.h);
            for c in v.chunks(4) {
                let word = c.iter().enumerate().fold(0u64, |a, (k, &s)| a | u64::from(s) << (16 * k));
                x = (x ^ word).wrapping_mul(0x9E37_79B9_7F4A_7C15).rotate_left(29);
            }
            h = cache::hash(h, &x.to_le_bytes());
        }
        h
    }
}

const FILL_MAGIC: &[u8; 7] = b"WCEFILL";
// 1: the first.
const FILL_VERSION: u16 = 1;

/// How many fills the cache keeps (the newest; a campaign map's is about 10 MB).
const FILL_KEEP: usize = 8;

/// Where the fills are kept.
fn fill_cache_dir() -> std::path::PathBuf {
    wce_foundation::settings::Paths::standard().cache_dir().join("map_fill")
}

fn fill_path(dir: &std::path::Path, key: u64) -> std::path::PathBuf {
    dir.join(format!("{key:016x}.fill"))
}

/// A kept fill: its battle field and BOB's messages.
fn read_fill(dir: &std::path::Path, key: u64) -> Option<(TileList, Vec<String>)> {
    let bytes = std::fs::read(fill_path(dir, key)).ok()?;
    let rest = bytes.strip_prefix(FILL_MAGIC.as_slice())?;
    let (version, rest) = rest.split_first_chunk::<2>()?;
    let (len, rest) = rest.split_first_chunk::<8>()?;
    if u16::from_le_bytes(*version) != FILL_VERSION {
        return None;
    }
    let (list, messages) = rest.split_at_checked(usize::try_from(u64::from_le_bytes(*len)).ok()?)?;
    let list = TileList::parse(list).ok()?;
    let messages = std::str::from_utf8(messages).ok()?.lines().map(str::to_owned).collect();
    Some((list, messages))
}

/// Keeps a fill (through a temporary file), when its battle field reads back the same; drops the cache's older ones past the newest few.
fn write_fill(dir: &std::path::Path, key: u64, list: &TileList, messages: &[String]) -> Result<(), String> {
    let bytes = list.write();
    if TileList::parse(&bytes).as_ref() != Ok(list) {
        return Err("its battle field does not read back the same".to_owned());
    }
    std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    let mut out = FILL_MAGIC.to_vec();
    out.extend(FILL_VERSION.to_le_bytes());
    out.extend((bytes.len() as u64).to_le_bytes());
    out.extend(bytes);
    out.extend(messages.join("\n").bytes());
    let (to, tmp) = (fill_path(dir, key), dir.join(format!("{key:016x}.fill.{}.tmp", std::process::id())));
    std::fs::write(&tmp, out).map_err(|e| e.to_string())?;
    std::fs::rename(&tmp, &to).map_err(|e| e.to_string())?;

    let mut fills: Vec<(std::time::SystemTime, std::path::PathBuf)> = std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .filter(|e| e.path().extension().is_some_and(|x| x == "fill"))
        .filter_map(|e| Some((e.metadata().ok()?.modified().ok()?, e.path())))
        .collect();
    fills.sort_by_key(|f| std::cmp::Reverse(f.0));
    for (_, p) in fills.into_iter().skip(FILL_KEEP) {
        let _ = std::fs::remove_file(p);
    }
    Ok(())
}

/// What a fill again reads.
struct Inputs {
    doc: DocumentId,
    name: String,
    db: Arc<TileDatabase>,
    fill: FillInputs,
    old: Arc<PlacedMap>,
    frame: Arc<Frame>,

    /// A campaign's roads at its start: the view's road tiles at their levels (the document's map keeps the kit's).
    roads: Option<Arc<wce_terrain_io::attila::roads::CampaignRoads>>,
}

#[derive(Default)]
pub struct Refill {
    rx: Option<Receiver<Done>>,

    /// Another edit came while one ran: fill again when it is done.
    pub again: bool,
    since: Option<Instant>,
}

/// A cell layer's colours, row by row (RGB).
fn rgb(l: &wce_terrain::cells::CellLayer) -> Vec<[u8; 3]> {
    l.cells()
        .iter()
        .map(|&i| {
            let c = l.palette()[usize::from(i)];
            [c[0], c[1], c[2]]
        })
        .collect()
}

/// A placement as the comparison of two fills sees it.
fn same(a: &MapPlacement, b: &MapPlacement) -> bool {
    a.location == b.location
        && a.variation == b.variation
        && a.x == b.x
        && a.y == b.y
        && a.rotation == b.rotation
        && a.flipped == b.flipped
        && a.climate == b.climate
}

/// The chunks to build again after a fill: those whose placements (by origin cell) differ, and every chunk the old and new placements of
/// those reach into (the cells a tile covers are its: the ground under them changes with it).
fn changed_chunks(old: &PlacedMap, new: &PlacedMap) -> Vec<ChunkKey> {
    let by_chunk = |m: &PlacedMap| {
        let mut out: HashMap<ChunkKey, Vec<usize>> = HashMap::new();
        for (i, p) in m.placements.iter().enumerate() {
            out.entry((p.x.div_euclid(CHUNK), p.y.div_euclid(CHUNK))).or_default().push(i);
        }

        out
    };

    let (a, b) = (by_chunk(old), by_chunk(new));
    let differ: Vec<ChunkKey> = a
        .keys()
        .chain(b.keys())
        .copied()
        .filter(|k| {
            let (pa, pb) = (a.get(k).map_or(&[][..], Vec::as_slice), b.get(k).map_or(&[][..], Vec::as_slice));
            pa.len() != pb.len() || pa.iter().zip(pb).any(|(&i, &j)| !same(&old.placements[i], &new.placements[j]))
        })
        .collect();

    let mut out = differ.clone();
    for k in &differ {
        let rects = a
            .get(k)
            .into_iter()
            .flatten()
            .map(|&i| old.placements[i].rect())
            .chain(b.get(k).into_iter().flatten().map(|&j| new.placements[j].rect()));
        for r in rects {
            for cy in r[1].div_euclid(CHUNK)..=(r[3] - 1).div_euclid(CHUNK) {
                for cx in r[0].div_euclid(CHUNK)..=(r[2] - 1).div_euclid(CHUNK) {
                    out.push((cx, cy));
                }
            }
        }
    }

    out.sort_unstable();
    out.dedup();
    out
}

fn run(i: Inputs) -> Done {
    let started = Instant::now();
    let (mut map, messages) = i.fill.fill(&i.db, &i.name);
    // What the fill does not make: the sea LF and the LF's metres (a campaign map's are its database's).
    map.sea_lf.clone_from(&i.old.sea_lf);
    (map.lf_height, map.vertical_offset) = (i.old.lf_height, i.old.vertical_offset);
    let changed = changed_chunks(&i.old, &map);
    let map = Arc::new(map);
    let frame = Arc::new(i.frame.with_map(wce_terrain_editor::document::shown_placed(&map, i.roads.as_deref())));
    Done { doc: i.doc, map, frame, changed, secs: started.elapsed().as_secs_f64(), messages }
}

impl Refill {
    pub fn busy(&self) -> bool {
        self.rx.is_some()
    }

    /// The seconds the running fill has taken so far.
    pub fn elapsed(&self) -> Option<f64> {
        self.since.map(|t| t.elapsed().as_secs_f64())
    }

    /// Fills the document's map again on a worker (when one is running, once it is done). False when the map has no tile map to fill.
    pub fn request(&mut self, d: &MapDocument, doc: DocumentId, frame: Arc<Frame>) -> bool {
        if self.busy() {
            self.again = true;
            return true;
        }

        let Some(fill) = d.model().and_then(FillInputs::of) else { return false };
        let inputs =
            Inputs { doc, name: d.placed.name.clone(), db: d.db.clone(), fill, old: d.placed.clone(), frame, roads: d.start_roads() };
        let (tx, rx) = channel();
        std::thread::spawn(move || {
            let _ = tx.send(run(inputs));
        });

        self.rx = Some(rx);
        self.since = Some(Instant::now());
        self.again = false;
        true
    }

    /// The fill, once done.
    pub fn poll(&mut self) -> Option<Done> {
        let done = self.rx.as_ref()?.try_recv().ok()?;
        self.rx = None;
        self.since = None;
        Some(done)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(x: i32, y: i32, loc: &str) -> MapPlacement {
        MapPlacement {
            location: loc.to_owned(),
            variation: "aa".to_owned(),
            x,
            y,
            w: 1,
            h: 1,
            rotation: 0x10,
            flipped: false,
            climate: 0,
            any_variation: false,
        }
    }

    #[test]
    fn only_the_chunks_that_differ() {
        let old = PlacedMap { width: 64, height: 64, placements: vec![p(1, 1, "a"), p(9, 1, "b"), p(17, 1, "c")], ..Default::default() };
        let new = PlacedMap {
            width: 64,
            height: 64,
            placements: vec![p(1, 1, "a"), p(9, 1, "x"), p(17, 1, "c"), p(30, 30, "d")],
            ..Default::default()
        };
        assert_eq!(changed_chunks(&old, &new), vec![(1, 0), (3, 3)]);

        // A 12 x 12 tile at (14, 2) reaches into the chunks to its right and below.
        let mut big = p(14, 2, "e");
        (big.w, big.h) = (12, 12);
        let new = PlacedMap { width: 64, height: 64, placements: vec![p(1, 1, "a"), big], ..Default::default() };
        assert_eq!(changed_chunks(&old, &new), vec![(1, 0), (1, 1), (2, 0), (2, 1), (3, 0), (3, 1)]);
    }
}

#[cfg(test)]
mod fill_cache_tests {
    use super::*;

    #[test]
    fn a_kept_fill_reads_back_with_its_messages() {
        let dir = std::env::temp_dir().join(format!("wce_fill_cache_test_{}", std::process::id()));
        let mut list = TileList { version: 11, battle_size: [4, 2], battle_centre: [2, 1], ..TileList::default() };
        list.origin_offset = [-2.0, -1.0];
        let messages = vec!["Failed to find tile for point 1, 2".to_owned(), "another".to_owned()];
        write_fill(&dir, 42, &list, &messages).expect("kept");
        let (back, said) = read_fill(&dir, 42).expect("read back");
        assert_eq!(back, list);
        assert_eq!(said, messages);
        assert!(read_fill(&dir, 43).is_none(), "another key");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
