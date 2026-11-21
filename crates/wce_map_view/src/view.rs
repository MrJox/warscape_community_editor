//! A battle map on the scene, streamed around the camera (wce_tile_editor's `context`): the streamer's chunks put on one scene layer once
//! the renderer has their terrains' GPU side (the version a chunk replaces stays until then), the sea following the focus, the far ring
//! following what the camera sees, the map's snow regions, and the overview map over the viewport. What the view is of (a tile's
//! neighbours, a whole map) is the streamer's frame; the tools own their panels and what they add on top.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use wce_editor::editor::Editor;
use wce_editor::gui;
use wce_foundation::math::Float4x4;
use wce_foundation::{log, wce_prof};
use wce_scene::render_scene::{DocumentId, EntityId, LayerId, Payload, RenderScene, SnowRegions, SourceRef};
use wce_tile::battle_map::context::{CELL, PlacedMap};
use wce_tile_editor::tile_document::context::{CHUNK, ChunkChange, ChunkKey, Frame, StreamStats, Streamer};

use crate::maps::{GAME_FAR_PLANE, MINIMAP_KEY, MINIMAP_SIDE, minimap_pixels};

/// How long a frame spends putting finished chunks on the scene (one at least; the rest wait for the next frames).
const APPLY_BUDGET: std::time::Duration = std::time::Duration::from_millis(3);

/// A season's snow map: its size and a byte per cell.
type SnowMap = (u32, u32, Vec<u8>);

/// The rings and the camera settings a view follows (the tool's panel owns them).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Rings {
    /// Cells around the focus: everything, the buildings, the tiles' coarse ground, the LF ground (at least what the camera sees).
    pub near: i32,
    pub mid: i32,
    pub ground: i32,
    pub far: i32,

    /// The camera's view distance while the view shows (scene units; the game's battle camera sees 13 000).
    pub view_distance: f32,

    /// The rings follow the camera (else they stay around the frame's open tile).
    pub follow: bool,
}

impl Default for Rings {
    /// A battle map's.
    fn default() -> Self {
        Self { near: 8, mid: 24, ground: 24, far: 64, view_distance: GAME_FAR_PLANE, follow: true }
    }
}

impl Rings {
    /// A campaign map's, `cells_per_unit` map cells a world unit (1.5 on Attila's map, 3 on TDD's and Thrones'): the battle's cells are
    /// a fraction of a campaign view. The game draws its tiles' ground to 37 world units from the camera and blends it into its far
    /// material from 9 (capture A's resolve, `g_far_blend_range` (9, 37)): the full detail to 9, the tiles' buildings and ground to 37,
    /// the LF ground to 128 (the far ring follows the camera's view past it).
    #[must_use]
    pub fn campaign(cells_per_unit: f64) -> Self {
        let cells = |units: f64| (units * cells_per_unit).ceil().max(1.0) as i32;
        Self { near: cells(9.0), mid: cells(37.0), ground: cells(37.0), far: cells(128.0), view_distance: GAME_FAR_PLANE, follow: true }
    }
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

static APPLY_COMING: std::sync::OnceLock<&'static wce_foundation::profile::Slot> = std::sync::OnceLock::new();

/// The longest a change waits for its neighbours (`MapView::apply`): past it, a chunk goes on and a removed one off whatever comes. Long:
/// a fast move from far away queues hundreds of chunks, and at 3 s the held ones went before their replacements came (the holes again).
const GONE_HOLD: std::time::Duration = std::time::Duration::from_secs(15);

#[derive(Default)]
pub struct MapView {
    /// The streamer, the scene layer of the view and the document it belongs to.
    pub streamer: Option<Box<Streamer>>,
    layer: LayerId,
    doc: DocumentId,

    /// The LF ground the camera sees (cells around the focus, whole chunks): the far ring is at least that, so that no chunk comes and
    /// goes at the horizon.
    far_view: i32,

    /// Each chunk's entities on the layer (added in a row: a range of ids) and its side (chunks), and the sea's (with the focus it is
    /// around).
    chunks: HashMap<ChunkKey, (std::ops::Range<EntityId>, i32)>,

    /// The streamer's chunks waiting for the renderer to have their terrains' GPU side and for their neighbours (with their sides and since
    /// when: `apply`), and the ones to take off: their square (key, side), entities and since when (`apply` keeps one until what
    /// replaces it is there). Held by their entities, not their keys: a block's first chunk, or a distant square's first quarter, has
    /// the larger piece's key, and put on, it took the whole piece off (the rest of it a hole until its other parts came).
    waiting: Vec<(ChunkKey, i32, std::time::Instant, Vec<(Float4x4, Payload)>)>,

    /// The waiting chunks whose terrains the renderer has ready (asked once: held for their neighbours, a fast move's hundreds were asked
    /// every frame, 2 ms of it).
    terrains_ok: std::collections::HashSet<ChunkKey>,
    gone: Vec<(ChunkKey, i32, std::ops::Range<EntityId>, std::time::Instant)>,
    sea: Option<EntityId>,
    sea_at: (f64, f64),

    /// The snow map loaded (map folder, season) and the regions' key the scene has (season, on, amount).
    snow_map: Option<(PathBuf, String, Option<Arc<SnowMap>>)>,
    snow_key: Option<(String, bool, u32)>,

    /// The overview map's picture: the map it is of (its name).
    overview_of: String,

    /// The streamer's report, and the same as fields (the panel's table).
    pub status: String,
    pub stats: Option<StreamStats>,
}

/// How far the ground shows from under the camera (scene units): straight ahead, and to each side there. What ends the view is its far plane,
/// `zfar` along the camera's axis: at the sides of a flat cone it is further than `zfar` from the eye, and a tilted axis meets the ground
/// past it. `height` is the eye's over the ground, `aspect` the viewport's.
fn view_reach(cam: &wce_scene::camera::Camera, height: f64, aspect: f64) -> (f64, f64) {
    let (dx, dy, dz) = (f64::from(cam.target.x - cam.eye.x), f64::from(cam.target.y - cam.eye.y), f64::from(cam.target.z - cam.eye.z));
    let flat = dx.hypot(dz);

    // Not looking straight down, where the ground under the view has no ahead.
    let pitch = (-dy).atan2(flat).clamp(-1.4, 1.4);
    let far = f64::from(cam.zfar);
    (((far - height * pitch.sin()) / pitch.cos()).max(0.0), far * (f64::from(cam.fov_y) * 0.5).tan() * aspect)
}

impl MapView {
    /// The document whose layer the view is on (0: none yet).
    pub fn doc(&self) -> DocumentId {
        self.doc
    }

    /// The scene layer the view is on (0: none).
    pub fn layer(&self) -> LayerId {
        self.layer
    }

    pub fn frame(&self) -> Option<Arc<Frame>> {
        self.streamer.as_ref().map(|s| s.frame())
    }

    /// The drawn ground at a frame point: the LF and the tiles in memory there (`Streamer::ground_at`); 0 without a map.
    pub fn ground_at(&self, x: f64, z: f64) -> f32 {
        self.streamer.as_ref().map_or(0.0, |s| s.ground_at(&s.frame(), x, z))
    }

    /// The ground's height at a point when the tiles there are read (None while one is still on its way, or without a map).
    pub fn ground_known(&self, x: f64, z: f64) -> Option<f32> {
        let s = self.streamer.as_ref()?;
        let (h, known) = s.ground_known(&s.frame(), x, z);
        known.then_some(h)
    }

    /// How many tiles the map's view has in memory so far (`Streamer::tiles_loaded`).
    pub fn tiles_loaded(&self) -> usize {
        self.streamer.as_ref().map_or(0, |s| s.tiles_loaded())
    }

    /// A new streamer on the scene: the layer (emptied), the scene's sea level, the sea and what `extra` adds next to it (with the sea, so
    /// it stays when the chunks come and go; the tile's floor).
    pub fn begin(
        &mut self,
        e: &mut Editor,
        id: DocumentId,
        s: Box<Streamer>,
        rings: &Rings,
        extra: impl FnOnce(&Frame) -> Vec<(Float4x4, Payload)>,
    ) {
        if self.layer == 0 || self.doc != id || e.scene.layer(self.layer).is_none() {
            if self.layer != 0 {
                e.scene.remove_layer(self.layer);
            }

            self.layer = e.scene.add_layer(id, wce_scene::render_scene::CONTEXT_LAYER);
            self.doc = id;
        } else {
            e.scene.clear_layer(self.layer);
        }

        if let Some(l) = e.scene.layer_mut(self.layer) {
            l.locked = true;
        }

        self.chunks.clear();
        self.waiting.clear();
        self.terrains_ok.clear();
        self.gone.clear();

        // The sea is at 0 m: in the frame, below its origin by the LF level it is measured from (the shaders measure from a little above it).
        e.scene.sea_level = s.frame().bias_y();
        log::info(format!("map context: {}", s.frame().lf_note()));
        self.streamer = Some(s);
        let at = self.focus(e, rings.follow).unwrap_or_default();
        let frame = self.streamer.as_ref().expect("set").frame().clone();
        let sea =
            add_range(&mut e.scene, self.layer, id, std::iter::once(frame.sea(at, self.sea_reach(rings))).chain(extra(&frame)).collect());
        (self.sea, self.sea_at) = (Some(sea.start), at);
    }

    /// The focus of the rings: what the camera looks at (the orbit's target, the fly camera's eye), or the frame's open tile.
    pub fn focus(&self, e: &Editor, follow: bool) -> Option<(f64, f64)> {
        let s = self.streamer.as_ref()?;
        if !follow {
            return Some(s.open_centre());
        }

        let p = e.viewport.pivot();
        Some((f64::from(p.x), f64::from(p.z)))
    }

    /// The far ring: the panel's, or what the camera sees.
    pub fn far_ring(&self, far: i32) -> i32 {
        far.max(self.far_view)
    }

    /// The LF ground the camera sees (cells), when more than the panel's far ring.
    pub fn far_view(&self) -> i32 {
        self.far_view
    }

    /// The cells around the focus the sea covers: the far ring and some.
    fn sea_reach(&self, r: &Rings) -> i32 {
        self.far_ring(r.far).max(r.ground).max(r.mid).max(r.near) + 4 * CHUNK
    }

    /// The cells around the focus the camera sees to: its far plane from its eye, which is off the focus (the orbit's target) by its
    /// distance, in whole chunks.
    fn view_cells(e: &Editor, focus: (f64, f64)) -> i32 {
        let cam = e.viewport.camera();
        let off = (f64::from(cam.eye.x) - focus.0).hypot(f64::from(cam.eye.z) - focus.1);
        let cells = ((f64::from(cam.zfar) + off) / CELL).ceil() as i32;
        (cells + CHUNK - 1) / CHUNK * CHUNK
    }

    /// The far ring follows what the camera sees from the focus; whether it changed.
    fn follow_view(&mut self, e: &Editor, focus: (f64, f64), r: &Rings) -> bool {
        let view = Self::view_cells(e, focus);
        if view == self.far_view {
            return false;
        }

        self.far_view = view;
        let far = self.far_ring(r.far);
        if let Some(s) = self.streamer.as_mut() {
            s.set_rings(r.near, r.mid, r.ground, far);
        }

        true
    }

    /// The panel's rings changed.
    pub fn set_rings(&mut self, r: &Rings) {
        let far = self.far_ring(r.far);
        if let Some(s) = self.streamer.as_mut() {
            s.set_rings(r.near, r.mid, r.ground, far);
        }
    }

    /// The scene's snow regions of a season (the map's `snow_map_<season>.png` in `dir`), made again when the season, the switch or the
    /// amount change.
    pub fn snow(&mut self, e: &mut Editor, dir: Option<PathBuf>, season: &str, on: bool, amount: f32) {
        let key = (season.to_owned(), on, amount.to_bits());
        if self.snow_key.as_ref() == Some(&key) {
            return;
        }

        let Some(s) = self.streamer.as_ref() else { return };
        self.snow_key = Some(key);
        let regions = match (on, dir) {
            (true, Some(dir)) => {
                if self.snow_map.as_ref().is_none_or(|(d, se, _)| *d != dir || se != season) {
                    let cells = wce_tile::battle_map::load_snow_map(&dir, season).map(Arc::new);
                    self.snow_map = Some((dir, season.to_owned(), cells));
                }

                let to_cell = s.frame().to_map.m.map(|row| row.map(|v| v as f32));
                self.snow_map.as_ref().and_then(|(_, _, c)| c.clone()).map(|c| SnowRegions {
                    to_cell,
                    w: c.0,
                    h: c.1,
                    cells: Arc::new(c.2.clone()),
                    amount,
                })
            }

            _ => None,
        };

        e.scene.set_snow(regions);
    }

    /// The snowy cells of the season's snow map when it is the one loaded: some(count), or some(0) for none that season.
    pub fn snowy_cells(&self, season: &str) -> Option<usize> {
        match &self.snow_map {
            Some((_, s, Some(c))) if s == season => Some(c.2.iter().filter(|&&v| v != 0).count()),
            Some((_, s, None)) if s == season => Some(0),
            _ => None,
        }
    }

    /// One update of the streamer: the chunks it finished replace theirs on the layer, the ones it dropped go (one pass over the scene); the
    /// sea follows the focus. Whether the scene changed.
    pub fn stream(&mut self, e: &mut Editor, r: &Rings) -> bool {
        wce_prof!("context.stream");
        e.viewport.set_far_plane(r.view_distance.max(GAME_FAR_PLANE));
        // A campaign map's lens is the game's campaign camera's; a battle map's the battle camera's.
        e.viewport.set_campaign_lens(e.scene.campaign, e.scene.game_units);
        let Some(focus) = self.focus(e, r.follow) else { return false };
        let widened = self.follow_view(e, focus, r);
        let reach = self.sea_reach(r);
        let Some(s) = self.streamer.as_mut() else { return false };
        let changes = s.update(focus);
        let stats = s.stats();
        (self.status, self.stats) = (stats.line(), Some(stats));
        let (dx, dz) = (focus.0 - self.sea_at.0, focus.1 - self.sea_at.1);
        let moved = (dx * dx + dz * dz).sqrt() > f64::from(2 * CHUNK) * CELL || widened;
        if let (Some(id), true) = (self.sea, moved) {
            let (_, sea) = s.frame().sea(focus, reach);
            e.scene.update_payload(id, sea);
            self.sea_at = focus;
        }

        self.take(changes);
        self.apply(e, Some(APPLY_BUDGET)) || moved
    }

    /// Waits for every chunk around the focus and puts them on the scene (scripts).
    pub fn settle(&mut self, e: &mut Editor, r: &Rings) {
        let Some(focus) = self.focus(e, r.follow) else { return };
        self.follow_view(e, focus, r);
        let Some(s) = self.streamer.as_mut() else { return };
        let changes = s.run_to_idle(focus);
        let stats = s.stats();
        (self.status, self.stats) = (stats.line(), Some(stats));
        self.take(changes);

        // The renderer makes their terrains' GPU side on its loading threads.
        let t0 = std::time::Instant::now();
        while !self.waiting.is_empty() && t0.elapsed().as_secs() < 60 {
            if !self.apply(e, None) {
                std::thread::sleep(std::time::Duration::from_millis(5));
            }
        }

        // The chunks kept for what was coming over them: everything is there now.
        self.apply(e, None);
    }

    /// The streamer's changes: the chunks it removed go now (with what was waiting of them), the ones it built wait for their terrains.
    fn take(&mut self, changes: Vec<ChunkChange>) {
        // What waits of the chunks the changes touch goes in one pass (one per change was quadratic in a burst as the camera moves).
        let touched = changes
            .iter()
            .map(|c| match c {
                ChunkChange::Set(k, ..) | ChunkChange::Remove(k) => *k,
            })
            .collect::<std::collections::HashSet<_>>();
        self.waiting.retain(|(w, _, _, _)| !touched.contains(w));
        self.terrains_ok.retain(|w| !touched.contains(w));

        // The batch's own last word per chunk, in its order.
        let mut set: Vec<(ChunkKey, i32, std::time::Instant, Vec<(Float4x4, Payload)>)> = vec![];
        let now = std::time::Instant::now();
        for c in changes {
            match c {
                ChunkChange::Set(k, lod, entities) => {
                    set.retain(|(w, _, _, _)| *w != k);
                    set.push((k, lod.side(), now, entities));
                }

                ChunkChange::Remove(k) => {
                    set.retain(|(w, _, _, _)| *w != k);
                    if let Some((range, side)) = self.chunks.remove(&k) {
                        self.gone.push((k, side, range, now));
                    }
                }
            }
        }

        self.waiting.extend(set);
    }

    /// Puts on the layer the waiting chunks whose terrains the renderer has ready (the version each replaces stays until then), in the
    /// order they came, until `budget` is spent (one at least), and takes off the removed ones. Whether anything changed.
    pub fn apply(&mut self, e: &mut Editor, budget: Option<std::time::Duration>) -> bool {
        wce_prof!("context.apply");
        let t0 = std::time::Instant::now();

        // What is still coming: the streamer's chunks not built yet and the ones waiting here, a chunk at a time (the larger squares,
        // blocks and distant pieces, in a list of their own). A chunk's content hangs on its neighbours': a tile that starts in one reaches
        // into those right and down of it, and a chunk draws the LF ground under the tiles of its neighbours that a block holds
        // (`Frame::build_chunk`). Put on the scene alone, a chunk built for the new state beside a neighbour still in the old one left
        // the cells of their shared tiles to nobody: tile-shaped holes down to the sea as the camera dived and the blocks gave way.
        let coming_scope =
            wce_foundation::profile::Scope::new(APPLY_COMING.get_or_init(|| wce_foundation::profile::slot("context.apply.coming")));
        let band = self.streamer.as_deref().map_or(0, Streamer::reach_chunks);
        let mut coming: std::collections::HashSet<ChunkKey> = std::collections::HashSet::new();
        let mut coming_squares: Vec<(ChunkKey, i32)> = vec![];
        let mut note = |coming: &mut std::collections::HashSet<ChunkKey>, k: ChunkKey, side: i32| {
            if side == 1 {
                coming.insert(k);
            } else {
                coming_squares.push((k, side));
            }
        };
        for (k, side) in self.streamer.as_deref().map(Streamer::pending).unwrap_or_default() {
            note(&mut coming, k, side);
        }
        for (k, side, _, _) in &self.waiting {
            note(&mut coming, *k, *side);
        }

        // Whether anything still coming lies over the chunks [x0, x1) x [y0, y1) but `own`.
        let any_coming = |coming: &std::collections::HashSet<ChunkKey>, squares: &[(ChunkKey, i32)], r: [i32; 4], own: ChunkKey| {
            (r[1]..r[3]).any(|y| (r[0]..r[2]).any(|x| (x, y) != own && coming.contains(&(x, y))))
                || squares.iter().any(|&(k, s)| k != own && k.0 < r[2] && r[0] < k.0 + s && k.1 < r[3] && r[1] < k.1 + s)
        };

        // The ready ones (their terrains' GPU side made) in the map's order, top left first: a chunk goes on once nothing still coming
        // can reach into it (those up and left of it by a tile's reach), and is no longer coming for the next, so a run of them goes
        // on in one pass. Until the budget is spent (one at least); after GONE_HOLD, whatever else.
        let mut gone: Vec<std::ops::Range<EntityId>> = vec![];
        let mut ready = vec![];
        let mut kept = Vec::with_capacity(self.waiting.len());
        let mut spent = false;
        let mut waiting = std::mem::take(&mut self.waiting);
        waiting.sort_by_key(|(k, _, _, _)| (k.1, k.0));
        for (key, side, since, entities) in waiting {
            let terrains_ready = !spent
                && (self.terrains_ok.contains(&key)
                    || e.renderer.as_mut().is_none_or(|r| {
                        entities.iter().all(|(_, p)| match p {
                            Payload::Terrain(t) => r.terrain_ready(t),
                            _ => true,
                        })
                    }));
            if terrains_ready {
                self.terrains_ok.insert(key);
            }
            let around = [key.0 - band, key.1 - band, key.0 + side, key.1 + side];
            let held = since.elapsed() < GONE_HOLD && any_coming(&coming, &coming_squares, around, key);
            if terrains_ready && !held {
                self.terrains_ok.remove(&key);
                coming.remove(&key);
                coming_squares.retain(|(k, _)| *k != key);
                // What it replaces: off now when as large (or smaller: it covers it), else held as a removed piece.
                match self.chunks.remove(&key) {
                    Some((range, was)) if was > side => self.gone.push((key, was, range, std::time::Instant::now())),
                    Some((range, _)) => gone.push(range),
                    None => {}
                }
                ready.push((key, side, entities));
                spent = budget.is_some_and(|b| t0.elapsed() > b);
            } else {
                kept.push((key, side, since, entities));
            }
        }

        self.waiting = kept;

        // The chunks to take off, once nothing still coming lies where they or their tiles reach (right and down by a tile's reach), or
        // after GONE_HOLD whatever comes.
        let mut held = vec![];
        // (Its own key counts: a new version of it still coming is what replaces it.)
        let none = (i32::MIN, i32::MIN);
        for (k, side, range, since) in std::mem::take(&mut self.gone) {
            let reach = [k.0, k.1, k.0 + side + band, k.1 + side + band];
            if since.elapsed() < GONE_HOLD && any_coming(&coming, &coming_squares, reach, none) {
                held.push((k, side, range, since));
            } else {
                gone.push(range);
            }
        }

        self.gone = held;
        if gone.is_empty() && ready.is_empty() {
            return false;
        }

        drop(coming_scope);
        wce_prof!("context.apply.scene");
        let scene = &mut e.scene;
        scene.remove_ranges(&gone);
        scene.reserve(ready.iter().map(|(_, _, v)| v.len()).sum());
        for (key, side, entities) in ready {
            let ids = add_range(scene, self.layer, self.doc, entities);
            self.chunks.insert(key, (ids, side));
        }

        true
    }

    /// Where the scene sits in the map for the coordinates readout (cells, height above the sea), while the view shows.
    pub fn map_frame(&self) -> Option<wce_editor::coords::MapFrame> {
        let f = self.streamer.as_ref()?.frame();
        Some(wce_editor::coords::MapFrame { to_map: f.to_map.m, sea_y: f.sea_y(), metres: f.unit })
    }

    /// Where north is in the world for the compass: the map's while the view shows, else +z.
    pub fn north(&self) -> [f32; 2] {
        self.streamer.as_ref().map_or([0.0, 1.0], |s| s.frame().north())
    }

    /// Shows or hides the view's sea.
    pub fn show_sea(&self, e: &mut Editor, on: bool) {
        let Some(id) = self.sea else { return };
        if let Some(en) = e.scene.find_mut(id)
            && en.visible != on
        {
            en.visible = on;
            e.scene.touch();
        }
    }

    /// Takes the view off the scene (and stops its streamer).
    pub fn clear(&mut self, e: &mut Editor) {
        e.viewport.set_far_plane(GAME_FAR_PLANE);
        e.viewport.set_campaign_lens(false, 1.0);
        if self.layer != 0 {
            e.scene.remove_layer(self.layer);
            self.layer = 0;
        }

        self.streamer = None;
        e.scene.sea_level = 0.0;
        e.scene.set_snow(None);
        self.snow_key = None;
        self.chunks.clear();
        self.waiting.clear();
        self.terrains_ok.clear();
        self.gone.clear();
        self.sea = None;
    }

    /// The overview map over the viewport (top right, under the camera chips): the map's LF, a highlighted rectangle of cells (the open
    /// tile's placement), the camera and where it looks; a click (or a drag) moves the camera there, keeping its height and angle.
    pub fn draw_overview(
        &mut self,
        e: &mut Editor,
        origin: gui::Vec2,
        size: gui::Vec2,
        name: &str,
        map: &PlacedMap,
        highlight: Option<[i32; 4]>,
    ) {
        let Some(frame) = self.streamer.as_ref().map(|s| s.frame().clone()) else { return };
        let Some(lf) = map.lf.as_ref().filter(|f| f.w > 1 && f.h > 1) else { return };
        if self.overview_of != name {
            let (w, h, px) = minimap_pixels(lf, MINIMAP_SIDE);
            e.images.forget(MINIMAP_KEY);
            e.images.submit(MINIMAP_KEY, w, h, px);
            name.clone_into(&mut self.overview_of);
        }

        let Some(img) = e.images.get(MINIMAP_KEY) else { return };

        // About a third of the image's width (the whole map's shape), under the camera chips (the coordinates are bottom left).
        let mw = (size.x * 0.3).clamp(160.0, 420.0);
        let mh = mw * img.h as f32 / img.w.max(1) as f32;
        let top = e.viewport.chips_top + 44.0;
        if mh > size.y - top - 120.0 || size.x < 400.0 {
            return;
        }

        let pos = gui::v2(origin.x + size.x - mw - 10.0, origin.y + top);
        gui::set_cursor_screen_pos(pos);
        gui::push_style_color(gui::COL_CHILD_BG, gui::v4(0.0, 0.0, 0.0, 0.6));
        if gui::begin_child("##minimap", gui::v2(mw, mh), 0, gui::WINDOW_NO_SCROLLBAR) {
            let dl = gui::window_draw_list();
            let (x0, y0) = (pos.x, pos.y);
            dl.image(
                img.id,
                gui::v2(x0, y0),
                gui::v2(x0 + mw, y0 + mh),
                gui::v2(0.0, 0.0),
                gui::v2(1.0, 1.0),
                gui::col32(255, 255, 255, 255),
            );
            dl.rect(gui::v2(x0, y0), gui::v2(x0 + mw, y0 + mh), gui::col32(0, 0, 0, 200), 0.0, 1.0);

            // Map cells to the picture.
            let (cw, ch) = (map.width.max(1) as f32, map.height.max(1) as f32);
            let at = |cx: f64, cy: f64| gui::v2(x0 + cx as f32 / cw * mw, y0 + cy as f32 / ch * mh);

            if let Some(o) = highlight {
                let (a, b) = (at(f64::from(o[0]), f64::from(o[1])), at(f64::from(o[2]), f64::from(o[3])));
                dl.rect(
                    gui::v2(a.x - 1.0, a.y - 1.0),
                    gui::v2(b.x.max(a.x + 2.0) + 1.0, b.y.max(a.y + 2.0) + 1.0),
                    gui::col32(255, 210, 60, 255),
                    0.0,
                    2.0,
                );
            }

            // The camera and where it looks (the view distance's reach).
            let cam = e.viewport.camera();
            let (ex, ey) = frame.cell_of(f64::from(cam.eye.x), f64::from(cam.eye.z));
            let (tx, ty) = frame.cell_of(f64::from(cam.target.x), f64::from(cam.target.z));
            let (dx, dy) = (tx - ex, ty - ey);
            let l = dx.hypot(dy).max(1e-9);
            let (along, across) = view_reach(&cam, f64::from(cam.eye.y) - f64::from(frame.sea_y()), f64::from(size.x / size.y.max(1.0)));
            let (along, across) = (along / CELL, across / CELL);
            let eye = at(ex, ey);
            let side = |s: f64| {
                let (fx, fy) = (dx / l, dy / l);
                at(ex + fx * along - s * fy * across, ey + fy * along + s * fx * across)
            };

            dl.triangle_filled(eye, side(-1.0), side(1.0), gui::col32(255, 255, 255, 40));
            dl.line(eye, side(-1.0), gui::col32(255, 255, 255, 140), 1.0);
            dl.line(eye, side(1.0), gui::col32(255, 255, 255, 140), 1.0);
            dl.circle_filled(eye, 4.0, gui::col32(230, 60, 50, 255));

            // A click or a drag: the camera over that cell.
            gui::set_cursor_screen_pos(pos);
            gui::invisible_button("##minimap_click", gui::v2(mw, mh), 0);
            if gui::is_item_active() {
                let m = gui::mouse_pos();
                let (cx, cy) = (f64::from((m.x - x0) / mw * cw), f64::from((m.y - y0) / mh * ch));
                let (fx, fz) = frame.from_map.apply(cx, cy);
                let (fx, fz) = (fx as f32, fz as f32);
                let d = wce_foundation::math::Float3::new(fx - cam.target.x, 0.0, fz - cam.target.z);
                let to = wce_scene::camera::Camera { eye: cam.eye + d, target: cam.target + d, ..cam };
                e.viewport.controller().set_camera(&to);
            }

            if gui::is_item_hovered() {
                gui::set_tooltip(&format!("{}: click to move the camera there", self.overview_of));
            }
        }

        gui::end_child();
        gui::pop_style_color(1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_view_ends_at_its_far_plane() {
        use wce_foundation::math::Float3;
        let cam = |eye: Float3, target: Float3| wce_scene::camera::Camera { eye, target, zfar: 1000.0, fov_y: 1.0, ..Default::default() };

        // Level: `zfar` ahead, wider at the sides than a cone of that length would be.
        let (along, across) = view_reach(&cam(Float3::new(0.0, 0.0, 0.0), Float3::new(0.0, 0.0, 10.0)), 0.0, 1.5);
        assert!((along - 1000.0).abs() < 1e-3);
        assert!((across - 1000.0 * 0.5f64.tan() * 1.5).abs() < 1e-3);

        // 45 degrees down from 100 up: where the far plane meets the ground.
        let (along, _) = view_reach(&cam(Float3::new(0.0, 100.0, 0.0), Float3::new(0.0, 0.0, 100.0)), 100.0, 1.5);
        assert!((along - (1000.0 - 100.0 * std::f64::consts::FRAC_1_SQRT_2) * std::f64::consts::SQRT_2).abs() < 1e-3);
    }
}
