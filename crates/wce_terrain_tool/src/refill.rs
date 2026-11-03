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
use wce_tile::battle_map::place_images;
use wce_tile::battle_map::tile_db::TileDatabase;
use wce_tile_editor::tile_document::context::{CHUNK, ChunkKey, Frame};

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
}

impl FillInputs {
    /// The model's tile map, climates, explicit tiles and LF as they are (none without a tile map).
    pub fn of(m: &wce_terrain::MapModel) -> Option<Self> {
        let tiles = m.layer(&LayerRole::TileMap).map(|l| l.get())?;
        let (w, h) = (tiles.width(), tiles.height());
        let climate = m.layer(&LayerRole::Climate).map(|l| l.get()).filter(|l| l.width() == w && l.height() == h).map(rgb);
        let explicit = m.explicit.as_ref().map(|x| x.get().write().iter().map(|&c| char::from(c)).collect());
        let lf = m.lf(LfRole::Land).map(|f| {
            let f = f.get();
            LfField { w: f.width(), h: f.height(), values: f.to_le_rows().as_chunks::<2>().0.iter().map(|c| u16::from_le_bytes(*c)).collect() }
        });

        Some(Self { w, h, tiles: rgb(tiles), climate, explicit, lf })
    }

    /// BOB's fill of them: the placements (with the LF) and BOB's messages.
    pub fn fill(&self, db: &TileDatabase, name: &str) -> (PlacedMap, Vec<String>) {
        let lf = self.lf.as_ref().map(|f| (f.values.as_slice(), f.w, f.h));
        let images = MapImages {
            width: self.w, height: self.h, tile_map: &self.tiles, climate_map: self.climate.as_deref(), explicit_tiles: self.explicit.as_deref(), lf: None, lf_u16: lf,
        };

        place_images(db, name, &images, self.lf.clone())
    }
}

/// What a fill again reads.
struct Inputs {
    doc: DocumentId,
    name: String,
    db: Arc<TileDatabase>,
    fill: FillInputs,
    old: Arc<PlacedMap>,
    frame: Arc<Frame>,
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
    l.cells().iter().map(|&i| {
        let c = l.palette()[usize::from(i)];
        [c[0], c[1], c[2]]
    }).collect()
}

/// A placement as the comparison of two fills sees it.
fn same(a: &MapPlacement, b: &MapPlacement) -> bool {
    a.location == b.location && a.variation == b.variation && a.x == b.x && a.y == b.y && a.rotation == b.rotation && a.flipped == b.flipped && a.climate == b.climate
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
    let differ: Vec<ChunkKey> = a.keys().chain(b.keys()).copied().filter(|k| {
        let (pa, pb) = (a.get(k).map_or(&[][..], Vec::as_slice), b.get(k).map_or(&[][..], Vec::as_slice));
        pa.len() != pb.len() || pa.iter().zip(pb).any(|(&i, &j)| !same(&old.placements[i], &new.placements[j]))
    }).collect();

    let mut out = differ.clone();
    for k in &differ {
        let rects = a.get(k).into_iter().flatten().map(|&i| old.placements[i].rect()).chain(b.get(k).into_iter().flatten().map(|&j| new.placements[j].rect()));
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
    map.sea_lf.clone_from(&i.old.sea_lf);
    let changed = changed_chunks(&i.old, &map);
    let map = Arc::new(map);
    let frame = Arc::new(i.frame.with_map(map.clone()));
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
        let inputs = Inputs { doc, name: d.placed.name.clone(), db: d.db.clone(), fill, old: d.placed.clone(), frame };
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
        MapPlacement { location: loc.to_owned(), variation: "aa".to_owned(), x, y, w: 1, h: 1, rotation: 0x10, flipped: false, climate: 0, any_variation: false }
    }

    #[test]
    fn only_the_chunks_that_differ() {
        let old = PlacedMap { width: 64, height: 64, placements: vec![p(1, 1, "a"), p(9, 1, "b"), p(17, 1, "c")], ..Default::default() };
        let new = PlacedMap { width: 64, height: 64, placements: vec![p(1, 1, "a"), p(9, 1, "x"), p(17, 1, "c"), p(30, 30, "d")], ..Default::default() };
        assert_eq!(changed_chunks(&old, &new), vec![(1, 0), (3, 3)]);

        // A 12 x 12 tile at (14, 2) reaches into the chunks to its right and below.
        let mut big = p(14, 2, "e");
        (big.w, big.h) = (12, 12);
        let new = PlacedMap { width: 64, height: 64, placements: vec![p(1, 1, "a"), big], ..Default::default() };
        assert_eq!(changed_chunks(&old, &new), vec![(1, 0), (1, 1), (2, 0), (2, 1), (3, 0), (3, 1)]);
    }
}
