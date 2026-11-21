//! The map context's chunks around a focus point, built on worker threads as the focus moves.
//!
//! Every update the streamer works out the level each chunk within the far ring should have (by its distance from the focus, with some
//! slack so that a chunk on a ring's edge does not flip back and forth), queues the chunks whose level changes, nearest first (a queued chunk
//! that is no longer wanted is dropped from the queue), and hands back what the workers finished: a chunk to put on the scene at a level, or
//! one to take off. The workers share the tiles: a tile is opened once (or read from the disk cache) while others wait for it, and kept in
//! memory up to a budget (the least recently used go first; the scene keeps what it draws).

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::mpsc::{Receiver, Sender, channel};
use std::sync::{Arc, Condvar, Mutex, MutexGuard};

use wce_formats::dds_decode::{dds_decode_mip, dds_info};
use wce_foundation::math::Float4x4;
use wce_scene::render_scene::Payload;
use wce_vfs::files::Files;

use super::template::{Template, load_any_template};
use super::{BLOCK, BlockSource, CHUNK, ContextRequest, FarMap, Frame, TileKey, cache, tile_dir};
use crate::terrain_bake::TerrainLook;

/// A chunk: its column and row of CHUNK x CHUNK map cells.
pub type ChunkKey = (i32, i32);

/// A chunk a worker finished: its entities at a level.
/// A built chunk and the frame generation it was built with.
type Built = (ChunkKey, Lod, Vec<(Float4x4, Payload)>, u64);

/// The cells past the tiles' rings where the blocks start.
const BLOCK_MARGIN: i32 = 2 * CHUNK;

/// The first chunk of a chunk's block.
fn block_of(k: ChunkKey) -> ChunkKey {
    (k.0.div_euclid(BLOCK) * BLOCK, k.1.div_euclid(BLOCK) * BLOCK)
}

/// A chunk's level of detail.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Lod {
    /// Everything of its tiles.
    Near,

    /// Their coarse heights, low-resolution materials, buildings and rivers.
    Mid,

    /// Their coarse heights, low-resolution materials and rivers: the ground of the tiles without what stands on it.
    Ground,

    /// The LF ground alone.
    Far,

    /// The LF ground alone over a whole block of chunks ([`BLOCK`]), one terrain under the key of its first chunk (`Frame::build_block`).
    Block,

    /// A block with its tiles' heights and colours merged into that terrain: the ground ring where it can go by blocks, one draw where its
    /// tiles are dozens. It opens them, so a block shows as [`Lod::Block`] first.
    Merged,

    /// The LF ground alone over a square of 2^n x 2^n blocks (n from 1 to [`DISTANT_LEVELS`]), as coarse as it is large and casting no
    /// shadow (`Frame::build_block`): a square goes by one once it is further from the focus than its side. Towards a campaign map's
    /// horizon, where the far ring reaches the game's far plane, the blocks were 731 terrains in every pass (capture A's camera: 7.4 to
    /// 105 ms a frame).
    Distant(u8),
}

impl Lod {
    /// Whether it is a whole block (or more).
    fn is_block(self) -> bool {
        matches!(self, Self::Block | Self::Merged | Self::Distant(_))
    }

    /// Chunks a side of what it covers.
    fn side(self) -> i32 {
        match self {
            Self::Block | Self::Merged => BLOCK,
            Self::Distant(n) => BLOCK << n,
            _ => 1,
        }
    }

    /// Its place in the order: the levels by detail, the distant ones all last.
    fn rank(self) -> usize {
        match self {
            Self::Near => 0,
            Self::Mid => 1,
            Self::Ground => 2,
            Self::Far => 3,
            Self::Block => 4,
            Self::Merged => 5,
            Self::Distant(_) => 6,
        }
    }
}

/// The largest distant square: 2^n blocks a side ([`Lod::Distant`]).
const DISTANT_LEVELS: u8 = 3;

/// What a streamer did so far ([`Streamer::stats`]).
#[derive(Clone, Debug, Default)]
pub struct StreamStats {
    pub map: String,
    /// The chunks on the scene per level ([`Streamer::shown`]: near, mid, ground, far, blocks, merged blocks, distant squares).
    pub shown: [usize; 7],
    /// Chunks wanted and not built yet.
    pub building: usize,
    /// Tiles opened from their files, the seconds spent on them, read from the cache, that could not be opened.
    pub opened: usize,
    pub open_s: f64,
    pub from_cache: usize,
    pub failed: usize,
    /// The tiles in memory (MB).
    pub mb: usize,
    pub lf_note: String,
    /// Seconds to read the map, and to the first ring built.
    pub map_s: f64,
    pub first_s: Option<f64>,
}

/// What changed on the scene's side.
pub enum ChunkChange {
    /// A chunk's entities at a level (replacing what it had).
    Set(ChunkKey, Lod, Vec<(Float4x4, Payload)>),
    Remove(ChunkKey),
}

/// Threads that build chunks (opening a tile holds its whole scene and height map: a few at a time).
const WORKERS: usize = 4;

/// The memory the tiles in memory may take (bytes, about): half the machine's, at most 12 GB (`template_budget`); this without a
/// size to go by. At a fixed 1.5 GB a campaign map's 1 213 tiles were read 5 965 times (the far bake and the workers took turns
/// putting out each other's).
const TEMPLATE_BUDGET: usize = 1536 << 20;

/// The most of [`TEMPLATE_BUDGET`]'s kind the machine's memory gives.
const TEMPLATE_BUDGET_MAX: usize = 12 << 30;

/// The far bake's own tiles' memory (`TileSet`, one a bake), once the workers' is three quarters full (`Shared::bake_tile`): it reads
/// every tile of the map, and kept with the workers' then it put theirs out.
const BAKE_BUDGET: usize = 1 << 30;

/// [`TEMPLATE_BUDGET`] for this machine.
fn template_budget() -> usize {
    wce_foundation::heap::physical().map_or(TEMPLATE_BUDGET, |b| usize::try_from(b / 2).unwrap_or(usize::MAX).min(TEMPLATE_BUDGET_MAX))
}

/// Tiles in memory (by tile and climate) within a budget: the least recently used go past it. One thread reads a tile, the others
/// wait for it.
struct TileSet {
    /// The tiles, and the bytes the ready ones take.
    tiles: Mutex<(HashMap<TileKey, Slot>, usize)>,
    loaded: Condvar,
    budget: usize,

    /// The tiles the far bake brought in that nobody else has asked for: they go when it ends (`release_bake_only`). Kept, a cold
    /// campaign map's whole set stayed in memory (12 GB of heap on main_attila).
    bake_only: Mutex<std::collections::HashSet<TileKey>>,
}

impl TileSet {
    fn new(budget: usize) -> Self {
        Self { tiles: Mutex::default(), loaded: Condvar::new(), budget, bake_only: Mutex::default() }
    }

    /// The tiles only the far bake used go (it has ended).
    fn release_bake_only(&self) {
        let keys = std::mem::take(&mut *lock(&self.bake_only));
        let mut tiles = lock(&self.tiles);
        for k in keys {
            if let Some(Slot::Ready(t, _)) = tiles.0.get(&k) {
                tiles.1 -= t.bytes;
                tiles.0.remove(&k);
            }
        }
    }

    /// A tile it holds, none when not (or not yet) there.
    fn ready(&self, key: &TileKey) -> Option<Arc<Template>> {
        match lock(&self.tiles).0.get(key) {
            Some(Slot::Ready(t, _)) => Some(t.clone()),
            _ => None,
        }
    }

    /// The bytes its tiles take.
    fn bytes(&self) -> usize {
        lock(&self.tiles).1
    }
}

/// Cells past a ring's edge before a chunk's level changes.
const SLACK: f64 = 2.0;

enum Slot {
    Loading,
    Ready(Arc<Template>, u64),
    Failed,
}

#[derive(Default)]
struct Stats {
    opened: usize,
    from_cache: usize,
    open_s: f64,
    failed: Vec<String>,
}

struct Job {
    key: ChunkKey,
    lod: Lod,
    order: f64,
}

struct Shared {
    /// The threads the workers' data-parallel loops run on (a tile's opening is full of them): below normal priority and apart from
    /// rayon's global pool, which the renderer's culling uses every frame.
    pool: rayon::ThreadPool,

    /// The frame and its generation: the Terrain tool swaps in a frame over new placements (`Streamer::swap_frame`); a job builds with the
    /// frame it starts with, and what an older one built is dropped.
    frame: Mutex<(Arc<Frame>, u64)>,
    rq: ContextRequest,
    files: Option<Arc<dyn Files>>,
    queue: Mutex<Vec<Job>>,
    work: Condvar,
    tiles: TileSet,

    /// Whether a tile (location, variation) is a custom-mesh one (`is_custom`).
    custom_tiles: Mutex<HashMap<(String, String), bool>>,

    /// The blocks that go by chunks at the moment (`Lod::is_block`: the rest are blocks).
    zone: Mutex<std::collections::HashSet<ChunkKey>>,

    /// The mean colours of the ground textures, for the baked ground (`Template::bake_far`): the same few hundred for every tile.
    means: Mutex<HashMap<String, Option<[f32; 3]>>>,
    stats: Mutex<Stats>,
    clock: std::sync::atomic::AtomicU64,
    stop: AtomicBool,

    /// A campaign map's far material, once made (`Frame::far_map`, on a thread of its own), and the cells its last bake covered (none: the
    /// whole map).
    far: Mutex<Option<Arc<FarMap>>>,
    far_region: Mutex<Option<[i32; 4]>>,

    /// The cells an edit changed since the far material was last baked (x0, y0, x1, y1), which the far bake's thread takes once the
    /// edits pause (`far_wake`).
    far_dirty: Mutex<Option<[i32; 4]>>,
    far_wake: Condvar,
}

/// How long the edits must pause for the far material to be baked again over their cells.
const FAR_SETTLE: std::time::Duration = std::time::Duration::from_millis(500);

/// Placements the far bake's tiles are opened ahead of its painting by at most (`Shared::open_ahead`): a few hundred tiles, well inside
/// the tiles' memory.
const OPEN_AHEAD: usize = 8192;

/// The side of a campaign map's far material (the game's is 8192: `FarMap`).
pub const FAR_MAP_SIZE: u32 = 4096;

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|p| p.into_inner())
}

impl Shared {
    fn frame(&self) -> (Arc<Frame>, u64) {
        let f = lock(&self.frame);
        (f.0.clone(), f.1)
    }

    /// Whether a placement's tile is a custom-mesh one (the cliffs), as the first root that has it says (remembered per tile; one the roots
    /// do not have is not: the packs' form is not read yet).
    fn is_custom(&self, frame: &Frame, i: usize) -> bool {
        let p = &frame.map.placements[i];
        let key = (p.location.clone(), p.variation.clone());
        if let Some(&known) = lock(&self.custom_tiles).get(&key) {
            return known;
        }

        let roots: Vec<PathBuf> =
            self.rq.tile_roots.iter().chain(self.rq.provider.is_none().then_some(&self.rq.raw_root)).cloned().collect();
        // A combined project's variation is a scene tile: not written for this.
        let custom =
            wce_tile::sources::find_tile_folder(&roots, &key.0, &key.1).is_some_and(|d| wce_tile::sources::custom_mesh(&d).is_some());
        lock(&self.custom_tiles).insert(key, custom);
        custom
    }

    /// A campaign map's far material's cache key: every placement's tile (its folder's `cache::key` for its look) and place, the climates,
    /// the LF's samples and the frame's numbers. None when a tile is not on disk (the provider makes the packs' on demand: the bake opens
    /// them).
    fn far_key(&self, frame: &Frame) -> Option<u64> {
        let roots: Vec<PathBuf> =
            self.rq.tile_roots.iter().chain(self.rq.provider.is_none().then_some(&self.rq.raw_root)).cloned().collect();
        let mut keys: HashMap<(String, String, String), u64> = HashMap::new();
        let mut h = cache::hash(0xCBF2_9CE4_8422_2325, b"far 2");
        h = cache::hash(h, &FAR_MAP_SIZE.to_le_bytes());
        for (i, p) in frame.map.placements.iter().enumerate() {
            let climate = frame.climate_name(i);
            let id = (p.location.clone(), p.variation.clone(), climate.clone());
            let tile = match keys.get(&id) {
                Some(&k) => k,
                None => {
                    let dir = wce_tile::sources::find_tile(&roots, &id.0, &id.1)?;
                    let own_root = wce_tile::tile_project::kit_raw_root(&dir).unwrap_or_else(|| self.rq.raw_root.clone());
                    let own_params = own_root.parent().and_then(wce_tile::vegetation::kit_params_dir).is_some();
                    let game_kit = self.rq.game_kit.as_deref().filter(|_| !own_params);
                    let look = TerrainLook { climate, ..self.rq.look.clone() };
                    let k = cache::key(&dir, &look, self.rq.trees, game_kit);
                    keys.insert(id, k);
                    k
                }
            };
            h = cache::hash(h, &tile.to_le_bytes());
            for v in [p.x, p.y, p.w, p.h, i32::from(p.rotation), i32::from(p.flipped), i32::from(p.climate)] {
                h = cache::hash(h, &v.to_le_bytes());
            }
        }
        h = cache::hash(h, &frame.climate);
        let numbers = [frame.z_stretch, frame.unit, frame.lf_level, frame.map.lf_height, frame.map.vertical_offset];
        for v in numbers {
            h = cache::hash(h, &v.to_le_bytes());
        }
        h = cache::hash(h, &[frame.map.width, frame.map.height].map(u32::to_le_bytes).concat());
        // The LF: its samples, a word at a time (190 million on main_attila).
        if let Some(lf) = &frame.map.lf {
            let v = lf.values.read();
            let mut x = h ^ u64::from(lf.w) << 32 ^ u64::from(lf.h);
            for c in v.chunks(4) {
                let word = c.iter().enumerate().fold(0u64, |a, (k, &s)| a | u64::from(s) << (16 * k));
                x = (x ^ word).wrapping_mul(0x9E37_79B9_7F4A_7C15).rotate_left(29);
            }
            h = cache::hash(h, &x.to_le_bytes());
        }
        Some(h)
    }

    /// Calls `each` with the placements in order, their tiles opened ahead of it side by side ([`BlockSource::open_ahead`]): threads of
    /// its own take the next placement, at most [`OPEN_AHEAD`] past the one `each` is at (the tiles' memory), the opening in the pool
    /// as the workers' (not pool threads waiting on each other's tiles: see `worker`). In batches each waited for its slowest tile, the
    /// pool mostly idle.
    fn open_ahead(&self, frame: &Frame, set: &TileSet, ids: &[usize], each: &mut dyn FnMut(usize) -> bool) {
        let (next, at, done) = (AtomicUsize::new(0), AtomicUsize::new(0), AtomicBool::new(false));
        let threads = self.pool.current_num_threads().min(ids.len());
        std::thread::scope(|sc| {
            for _ in 0..threads {
                sc.spawn(|| {
                    wce_foundation::jobs::lower_thread_priority();
                    loop {
                        let k = next.fetch_add(1, Ordering::Relaxed);
                        let Some(&i) = ids.get(k) else { break };
                        while k > at.load(Ordering::Relaxed) + OPEN_AHEAD {
                            if done.load(Ordering::Relaxed) || self.stop.load(Ordering::Relaxed) {
                                return;
                            }
                            std::thread::sleep(std::time::Duration::from_millis(2));
                        }
                        if done.load(Ordering::Relaxed) || self.stop.load(Ordering::Relaxed) {
                            return;
                        }
                        if !self.is_custom(frame, i) {
                            self.bake_tile(frame, set, i);
                        }
                    }
                });
            }

            for (k, &i) in ids.iter().enumerate() {
                at.store(k, Ordering::Relaxed);
                if !each(i) {
                    break;
                }
            }
            done.store(true, Ordering::Relaxed);
        });
    }

    /// A placement's tile and climate.
    fn tile_key(frame: &Frame, i: usize) -> TileKey {
        let p = &frame.map.placements[i];
        (p.location.clone(), p.variation.clone(), frame.climate_name(i))
    }

    /// A placement's tile: from memory, else read from the cache or opened (one thread does, the others wait for it).
    fn template(&self, frame: &Frame, i: usize) -> Option<Arc<Template>> {
        let t = self.tile_in(&self.tiles, frame, i);
        // Theirs now too: it stays after the bake.
        let mut bake_only = lock(&self.tiles.bake_only);
        if !bake_only.is_empty() {
            bake_only.remove(&Self::tile_key(frame, i));
        }
        t
    }

    /// The far bake's: a tile the workers' set has; else into it while it has room (each tile read once, kept for them), and once it is
    /// three quarters full into the bake's own (`BAKE_BUDGET`), which does not put theirs out. (Its own alone, at 1 GB, read the map's
    /// tiles from the cache 6 150 times.)
    fn bake_tile(&self, frame: &Frame, set: &TileSet, i: usize) -> Option<Arc<Template>> {
        let key = Self::tile_key(frame, i);
        if let Some(t) = self.tiles.ready(&key).or_else(|| set.ready(&key)) {
            return Some(t);
        }
        if self.tiles.bytes() < self.tiles.budget / 4 * 3 {
            lock(&self.tiles.bake_only).insert(key);
            self.tile_in(&self.tiles, frame, i)
        } else {
            self.tile_in(set, frame, i)
        }
    }

    /// A placement's tile in `set`: from it, else read from the cache or opened into it.
    fn tile_in(&self, set: &TileSet, frame: &Frame, i: usize) -> Option<Arc<Template>> {
        let key = Self::tile_key(frame, i);
        // Its climate's look: the map gives each placement its own.
        let look = TerrainLook { climate: key.2.clone(), ..self.rq.look.clone() };
        let now = self.clock.fetch_add(1, Ordering::Relaxed);
        let mut tiles = lock(&set.tiles);
        loop {
            match tiles.0.get_mut(&key) {
                Some(Slot::Ready(t, used)) => {
                    *used = now;
                    return Some(t.clone());
                }

                Some(Slot::Failed) => return None,
                Some(Slot::Loading) => tiles = set.loaded.wait(tiles).unwrap_or_else(|p| p.into_inner()),
                None => break,
            }
        }

        tiles.0.insert(key.clone(), Slot::Loading);
        drop(tiles);
        // The first source that has the tile (`wce_tile::sources`): the roots in order, then the provider (the packs' tiles, decompiled on
        // demand: it returns a folder it has at once, and makes a missing or stale one); without a provider `raw_root` last.
        let roots: Vec<PathBuf> =
            self.rq.tile_roots.iter().chain(self.rq.provider.is_none().then_some(&self.rq.raw_root)).cloned().collect();
        let mut dir = tile_dir(&self.rq.raw_root, &key.0, &key.1);
        match wce_tile::sources::find_tile(&roots, &key.0, &key.1) {
            Some(found) => dir = found,
            None => {
                if let Some(provide) = self.rq.provider.as_ref() {
                    match provide(&key.0, &key.1) {
                        Ok(made) => dir = made,
                        Err(e) => wce_foundation::log::info(format!("map context: {}\\{}: {e}", key.0, key.1)),
                    }
                }
            }
        }

        // The game's kit, when the tile's own (the root it was found in) has no vegetation parameters.
        let own_root = wce_tile::tile_project::kit_raw_root(&dir).unwrap_or_else(|| self.rq.raw_root.clone());
        let own_params = own_root.parent().and_then(wce_tile::vegetation::kit_params_dir).is_some();
        let game_kit = self.rq.game_kit.as_deref().filter(|_| !own_params);
        let ck = self.rq.cache.as_ref().map(|c| (c, cache::key(&dir, &look, self.rq.trees, game_kit)));
        let (result, cached) = self.pool.install(|| match ck.as_ref().and_then(|(c, k)| cache::read(c, *k)) {
            Some(t) => (Ok(t), true),
            None => (load_any_template(&dir, &look, self.rq.trees, game_kit, self.files.clone()), false),
        });

        // The bake is not cached: it takes a fraction of what reading the template does, and follows the textures.
        let mut result = result;
        if let (Ok(t), true) = (&mut result, self.rq.bake_ground) {
            t.bake_far(&|path| self.mean_colour(path));
        }

        // A custom-mesh tile is not cached (the cache has no place for its mesh; it opens in a fraction of a scene tile's time).
        if let (Ok(t), Some((c, k)), false) = (&result, &ck, cached)
            && t.custom.is_none()
            && let Err(e) = cache::write(c, *k, t)
        {
            wce_foundation::log::info(format!("map context: not cached: {}: {e}", dir.display()));
        }

        let mut stats = lock(&self.stats);
        let slot = match result {
            Ok(t) => {
                if cached {
                    stats.from_cache += 1
                } else {
                    stats.opened += 1
                }
                stats.open_s += t.load_s;
                if t.material.is_none() {
                    wce_foundation::log::info(format!(
                        "map context: {}\\{}: no ground material ({})",
                        key.0,
                        key.1,
                        if t.custom.is_some() { "custom mesh" } else { "scene tile" }
                    ));
                }

                // A campaign map's plain ground (its far ring) from a grass tile, the climates' ground: the first tile to come was any
                // (a desert's sand laid beige over the whole far ring).
                let set = key.0.replace('/', "\\").to_lowercase();
                let campaign = set.contains("\\tiles\\campaign\\");
                let tile_set = set.split('\\').filter(|s| !s.is_empty()).nth(3).unwrap_or_default().to_owned();
                if !campaign || tile_set.starts_with("grass_") {
                    frame.offer_plain(&t.material);
                }
                // Its sea's ground past the rings from a sea tile (`Frame::offer_sea`).
                if campaign && tile_set == "sea" {
                    frame.offer_sea(&t.material);
                }
                Slot::Ready(Arc::new(t), now)
            }

            Err(e) => {
                stats.failed.push(format!("{}\\{}: {e}", key.0, key.1));
                Slot::Failed
            }
        };

        drop(stats);

        // Over the budget: the least recently used go.
        let mut tiles = lock(&set.tiles);
        if let Slot::Ready(t, _) = &slot {
            tiles.1 += t.bytes;
        }
        if tiles.1 > set.budget {
            let mut ready: Vec<(u64, TileKey, usize)> = tiles
                .0
                .iter()
                .filter_map(|(k, s)| match s {
                    Slot::Ready(t, used) => Some((*used, k.clone(), t.bytes)),
                    _ => None,
                })
                .collect();

            ready.sort();
            for (_, k, bytes) in ready {
                if tiles.1 <= set.budget / 4 * 3 {
                    break;
                }

                tiles.0.remove(&k);
                tiles.1 -= bytes;
            }
        }

        let out = match &slot {
            Slot::Ready(t, _) => Some(t.clone()),
            _ => None,
        };

        tiles.0.insert(key, slot);
        set.loaded.notify_all();
        out
    }

    /// A ground texture's mean colour (linear), from one of its last mips.
    fn mean_colour(&self, path: &str) -> Option<[f32; 3]> {
        if let Some(known) = lock(&self.means).get(path) {
            return *known;
        }

        let mean = self.files.as_ref().and_then(|f| f.read(path).ok()).and_then(|bytes| {
            let image = dds_decode_mip(&bytes, dds_info(&bytes).mips.saturating_sub(4)).ok()?;
            let n = (image.px.len() / 4).max(1) as f32;
            let sum = image
                .px
                .as_chunks::<4>()
                .0
                .iter()
                .fold([0.0f32; 3], |s, p| std::array::from_fn(|k| s[k] + (f32::from(p[k]) / 255.0).powf(2.2)));
            Some(sum.map(|v| v / n))
        });

        lock(&self.means).insert(path.to_owned(), mean);
        mean
    }

    fn worker(self: Arc<Self>, tx: Sender<Built>) {
        wce_foundation::jobs::lower_thread_priority();
        loop {
            let job = {
                let mut q = lock(&self.queue);
                loop {
                    if self.stop.load(Ordering::Acquire) {
                        return;
                    }

                    // The nearest first (the queue is short: the chunks of a ring).
                    if let Some(k) = (0..q.len()).min_by(|&a, &b| q[a].order.total_cmp(&q[b].order)) {
                        break q.swap_remove(k);
                    }

                    q = self.work.wait(q).unwrap_or_else(|p| p.into_inner());
                }
            };

            // Not in the pool: `template` may wait for another worker's tile, and a pool thread waiting inside a parallel loop can take up
            // that very worker's job (a deadlock); only the opening and the cache read run there.
            let (frame, generation) = self.frame();
            let zone = lock(&self.zone).clone();
            let far = lock(&self.far).clone();
            let (mw, mh) = (frame.map.width, frame.map.height);
            let source = BlockSource {
                template: &|i| self.template(&frame, i),
                is_custom: &|i| self.is_custom(&frame, i),
                mean: &|path| self.mean_colour(path),
                in_block: &|k| self.rq.bake_ground && !zone.contains(&block_of(k)),
                relief: &|x, y| far.as_ref().map(|f| f.relief_at(x, y, mw, mh)),
                open_ahead: &|ids, each| {
                    for &i in ids {
                        if !each(i) {
                            break;
                        }
                    }
                },
            };

            let entities = frame.build_chunk(job.key, job.lod, &source);
            if tx.send((job.key, job.lod, entities, generation)).is_err() {
                return;
            }
        }
    }
}

/// The chunks of a map context around a moving focus.
pub struct Streamer {
    shared: Arc<Shared>,
    results: Receiver<Built>,
    workers: Vec<std::thread::JoinHandle<()>>,

    /// The level each chunk is on the scene with, and the one it is being built at.
    shown: HashMap<ChunkKey, Lod>,
    wanted: HashMap<ChunkKey, Lod>,

    /// The rings around the focus (cells): near, mid, ground, far.
    rings: [i32; 4],
    started: std::time::Instant,
    frame_s: f64,
    first_s: Option<f64>,
}

impl Streamer {
    /// The frame of the request (slow on the campaign map: run it away from the UI thread) and the workers.
    pub fn new(rq: ContextRequest, files: Option<Arc<dyn Files>>) -> Self {
        let started = std::time::Instant::now();
        wce_foundation::log::info(format!("map context: tiles in memory up to {} MB", template_budget() >> 20));
        let cores = std::thread::available_parallelism().map_or(4, |n| n.get());
        // All the cores but two (the UI and the renderer's): opening a tile is parallel inside, and the far bake's tiles open side by
        // side (`open_ahead`). Capped at 8, a 16-thread machine sat at 40 % through a cold campaign map's load (150 s; 87 s now).
        let pool = rayon::ThreadPoolBuilder::new()
            .num_threads(cores.saturating_sub(2).max(2))
            .thread_name(|k| format!("map context {k}"))
            .start_handler(|_| wce_foundation::jobs::lower_thread_priority())
            .build()
            .expect("the map context's threads");
        let frame = Arc::new(pool.install(|| Frame::new(&rq, files.clone())));

        // The cache's templates of an older format go (in the background: a large cache takes a while to look through).
        if let Some(dir) = rq.cache.clone() {
            std::thread::spawn(move || {
                let (n, bytes) = cache::prune(&dir);
                if n > 0 {
                    wce_foundation::log::info(format!("map context cache: {n} templates of an older format removed ({} MB)", bytes >> 20));
                }
            });
        }
        let rings = [rq.near, rq.mid, rq.ground, rq.far];
        let frame_s = started.elapsed().as_secs_f64();
        let shared = Arc::new(Shared {
            pool,
            frame: Mutex::new((frame, 0)),
            rq,
            files,
            queue: Mutex::default(),
            work: Condvar::new(),
            tiles: TileSet::new(template_budget()),
            custom_tiles: Mutex::default(),
            zone: Mutex::default(),
            means: Mutex::default(),
            stats: Mutex::default(),
            clock: Default::default(),
            stop: AtomicBool::new(false),
            far: Mutex::default(),
            far_region: Mutex::default(),
            far_dirty: Mutex::default(),
            far_wake: Condvar::new(),
        });

        // A campaign map's far material, made once every tile is read (minutes on a whole map the first time: the cache's later), then
        // again over the cells of each edit once the edits pause (`refresh_far`).
        if shared.frame().0.is_campaign() {
            let s = shared.clone();
            std::thread::spawn(move || {
                wce_foundation::jobs::lower_thread_priority();
                let stop = || s.stop.load(Ordering::Relaxed);
                let mut cells: Option<[i32; 4]> = None;
                loop {
                    let t0 = std::time::Instant::now();
                    let (frame, _) = s.frame();
                    // Its own tiles, gone with it (`Shared::bake_tile`).
                    let bake_set = TileSet::new(BAKE_BUDGET);
                    let source = BlockSource {
                        template: &|i| s.bake_tile(&frame, &bake_set, i),
                        is_custom: &|i| s.is_custom(&frame, i),
                        mean: &|path| s.mean_colour(path),
                        in_block: &|_| false,
                        relief: &|_, _| None,
                        open_ahead: &|ids, each| s.open_ahead(&frame, &bake_set, ids, each),
                    };
                    let base = lock(&s.far).clone();
                    let mut how = String::new();
                    let made = match (&base, cells) {
                        (Some(base), Some(c)) => {
                            how = format!(" again over cells {c:?}");
                            frame.far_map_over(base, c, &source, &stop)
                        }
                        // The whole map: from the cache when its tiles and LF are as they were.
                        _ => {
                            let key = s.rq.cache.as_ref().and_then(|c| Some((c.clone(), s.far_key(&frame)?)));
                            match key.as_ref().and_then(|(c, k)| cache::read_far(c, *k, FAR_MAP_SIZE)) {
                                Some(far) => {
                                    how = " (read from the cache)".to_owned();
                                    Some(far)
                                }
                                None => {
                                    let far = frame.far_map(FAR_MAP_SIZE, &source, &stop);
                                    if let (Some(f), Some((c, k))) = (&far, &key)
                                        && let Err(e) = cache::write_far(c, *k, f)
                                    {
                                        wce_foundation::log::info(format!("map context: the far material not cached: {e}"));
                                    }
                                    far
                                }
                            }
                        }
                    };
                    // What only the bake used goes (`TileSet::bake_only`).
                    s.tiles.release_bake_only();
                    let Some(far) = made else { return };
                    wce_foundation::log::info(format!(
                        "map context: the campaign's far material made{how}, {FAR_MAP_SIZE} a side, in {:.1?}",
                        t0.elapsed()
                    ));
                    *lock(&s.far_region) = cells.filter(|_| base.is_some());
                    *lock(&s.far) = Some(Arc::new(far));

                    // The next edit's cells, once the edits pause.
                    let mut dirty = lock(&s.far_dirty);
                    while dirty.is_none() && !stop() {
                        dirty = s.far_wake.wait(dirty).unwrap_or_else(std::sync::PoisonError::into_inner);
                    }
                    loop {
                        let (d, wait) = s.far_wake.wait_timeout(dirty, FAR_SETTLE).unwrap_or_else(std::sync::PoisonError::into_inner);
                        dirty = d;
                        if stop() {
                            return;
                        }
                        if wait.timed_out() {
                            break;
                        }
                    }
                    cells = dirty.take();
                }
            });
        }

        let (tx, results) = channel();
        let workers = (0..WORKERS)
            .map(|_| {
                let (s, tx) = (shared.clone(), tx.clone());
                std::thread::spawn(move || s.worker(tx))
            })
            .collect();

        Self { shared, results, workers, shown: HashMap::new(), wanted: HashMap::new(), rings, started, frame_s, first_s: None }
    }

    /// A campaign map's far material, once made (`Frame::far_map`).
    pub fn far_map(&self) -> Option<Arc<FarMap>> {
        lock(&self.shared.far).clone()
    }

    /// The cells the far material's last bake covered (none: the whole map).
    pub fn far_region(&self) -> Option<[i32; 4]> {
        *lock(&self.shared.far_region)
    }

    /// A campaign map's far material is baked again over these map cells (x0, y0, x1, y1) once the edits pause: an edit changed them.
    pub fn refresh_far(&self, cells: [i32; 4]) {
        if !self.frame().is_campaign() {
            return;
        }
        let mut dirty = lock(&self.shared.far_dirty);
        *dirty = Some(dirty.map_or(cells, |d| [d[0].min(cells[0]), d[1].min(cells[1]), d[2].max(cells[2]), d[3].max(cells[3])]));
        self.shared.far_wake.notify_all();
    }

    /// New rings (cells): the next update moves the chunks to their levels.
    pub fn set_rings(&mut self, near: i32, mid: i32, ground: i32, far: i32) {
        self.rings = [near, mid, ground, far];
    }

    pub fn frame(&self) -> Arc<Frame> {
        self.shared.frame().0
    }

    /// How many tiles have come into memory so far (opened or read from the cache).
    pub fn tiles_loaded(&self) -> usize {
        let stats = lock(&self.shared.stats);
        stats.opened + stats.from_cache
    }

    /// The drawn ground at a frame point (`Frame::ground_at`), from the tiles in memory: none is opened for it.
    pub fn ground_at(&self, frame: &Frame, x: f64, z: f64) -> f32 {
        let tiles = lock(&self.shared.tiles.tiles);
        frame.ground_at(x, z, &|i| {
            let p = &frame.map.placements[i];
            match tiles.0.get(&(p.location.clone(), p.variation.clone(), frame.climate_name(i))) {
                Some(Slot::Ready(t, _)) => Some(t.clone()),
                _ => None,
            }
        })
    }

    /// A frame over new placements of the map (`Frame::with_map`, made away from the UI thread): the chunks in `changed` (whose
    /// placements differ) and those being built are built again with it; the others stay as they are on the scene.
    pub fn swap_frame(&mut self, frame: Arc<Frame>, changed: &[ChunkKey]) {
        {
            let mut f = lock(&self.shared.frame);
            *f = (frame, f.1 + 1);
        }

        // A block draws what its cells hold, whichever chunk's tile that is: every block a tile of the chunk can reach.
        let reach = (self.frame().reach() + CHUNK - 1) / CHUNK;
        let changed: std::collections::HashSet<ChunkKey> = changed
            .iter()
            .flat_map(|&k| {
                [k, block_of(k), block_of((k.0 + reach, k.1)), block_of((k.0, k.1 + reach)), block_of((k.0 + reach, k.1 + reach))]
            })
            .collect();
        let shown = &self.shown;
        self.wanted.retain(|k, l| shown.get(k) == Some(l) && !changed.contains(k));
        lock(&self.shared.queue).clear();

        // The far material over the changed chunks' tiles (by their origins: up to the largest tile past them).
        let r = self.frame().reach();
        let cells = changed.iter().fold(None, |a: Option<[i32; 4]>, &k| {
            let c = [k.0 * CHUNK, k.1 * CHUNK, (k.0 + 1) * CHUNK + r, (k.1 + 1) * CHUNK + r];
            Some(a.map_or(c, |a| [a[0].min(c[0]), a[1].min(c[1]), a[2].max(c[2]), a[3].max(c[3])]))
        });
        if let Some(c) = cells {
            self.refresh_far(c);
        }
    }

    /// The open tile's centre in its frame (the focus before there is a camera).
    pub fn open_centre(&self) -> (f64, f64) {
        let f = self.frame();
        let o = f.open;
        let (cx, cy) = (f64::from(o[0] + o[2]) * 0.5, f64::from(o[1] + o[3]) * 0.5);
        f.from_map.apply(cx, cy)
    }

    /// The level a chunk should have at `d` cells from the focus, given the one it has (it keeps it within the slack).
    fn level(&self, d: f64, now: Option<Lod>) -> Option<Lod> {
        let [near, mid, ground, far] = self.rings;
        let (mid, ground) = (mid.max(near), ground.max(mid).max(near));
        let rings = [(Lod::Near, near), (Lod::Mid, mid), (Lod::Ground, ground), (Lod::Far, far.max(ground))];
        let plain = rings.iter().find(|(_, r)| d <= f64::from(*r)).map(|(l, _)| *l);
        let Some(now) = now else { return plain };

        // Within the slack of its own ring's edge it stays.
        let edge = rings.iter().find(|(l, _)| *l == now).map_or(0.0, |(_, r)| f64::from(*r));
        let inner = rings.iter().take_while(|(l, _)| *l != now).last().map_or(f64::MIN, |(_, r)| f64::from(*r));
        if d <= edge + SLACK && d > inner - SLACK { Some(now) } else { plain }
    }

    /// The cells from the focus past which the ground goes by blocks: past the rings of what stands on it. They need the baked ground's
    /// blend pass: without it (Rome 2) they don't start.
    fn block_start(&self) -> f64 {
        if self.shared.rq.bake_ground { f64::from(self.rings[..2].iter().copied().max().unwrap_or(0) + BLOCK_MARGIN) } else { f64::MAX }
    }

    /// Works out the levels around the focus (a point of the frame), queues what changes and returns what the workers finished.
    pub fn update(&mut self, focus: (f64, f64)) -> Vec<ChunkChange> {
        let (f, generation) = self.shared.frame();
        let (fx, fy) = f.cell_of(focus.0, focus.1);
        let reach = f64::from(self.rings.into_iter().max().unwrap_or(0)) + SLACK;
        let (cw, ch) = f.chunks();
        let block_cells = BLOCK * CHUNK;

        // The distance from the focus to a square of cells.
        let distance = |x: i32, y: i32, size: i32| {
            let (x0, y0, size) = (f64::from(x), f64::from(y), f64::from(size));
            let dx = (x0 - fx).max(fx - (x0 + size)).max(0.0);
            let dy = (y0 - fy).max(fy - (y0 + size)).max(0.0);
            (dx * dx + dy * dy).sqrt()
        };

        let (start, ground) = (self.block_start(), f64::from(self.rings[2]));
        let mut want: HashMap<ChunkKey, (Lod, f64)> = HashMap::new();

        // Squares of blocks, from the largest down: one goes by a distant piece once it is further than its side (and past the blocks'
        // own rings); else its four quarters are looked at, down to the blocks, which go below.
        let near_blocks = start.max(ground) + f64::from(block_cells);
        let top = BLOCK << DISTANT_LEVELS;
        let mut blocks = vec![];
        let mut squares: Vec<(ChunkKey, u8)> = vec![];
        let top_cells = f64::from(top * CHUNK);
        let top_span = |c: f64| ((c - reach) / top_cells).floor() as i32..=((c + reach) / top_cells).floor() as i32;
        for ty in top_span(fy).filter(|&y| y >= 0 && y * top < ch) {
            for tx in top_span(fx).filter(|&x| x >= 0 && x * top < cw) {
                squares.push(((tx * top, ty * top), DISTANT_LEVELS));
            }
        }

        while let Some((key, n)) = squares.pop() {
            if n == 0 {
                blocks.push((key.0 / BLOCK, key.1 / BLOCK));
                continue;
            }

            let side = (BLOCK << n) * CHUNK;
            let d = distance(key.0 * CHUNK, key.1 * CHUNK, side);
            let slack = if self.wanted.get(&key) == Some(&Lod::Distant(n)) { SLACK } else { 0.0 };
            if d > f64::from(side).max(near_blocks) - slack {
                if d <= reach {
                    want.insert(key, (Lod::Distant(n), d));
                }

                continue;
            }

            let half = BLOCK << (n - 1);
            for (qx, qy) in [(0, 0), (half, 0), (0, half), (half, half)] {
                let q = (key.0 + qx, key.1 + qy);
                if q.0 < cw && q.1 < ch {
                    squares.push((q, n - 1));
                }
            }
        }

        let mut zone = std::collections::HashSet::new();
        for (bx, by) in blocks {
            {
                // Past the rings a block is one piece; it stays one within the slack.
                let key = (bx * BLOCK, by * BLOCK);
                let d = distance(bx * block_cells, by * block_cells, block_cells);
                let now = self.wanted.get(&key).copied();
                let slack = if now.is_some_and(Lod::is_block) { SLACK } else { 0.0 };
                if d > start - slack {
                    // Merged once it shows as LF ground: its tiles take a while, and until then there would be nothing there.
                    let shows = matches!(self.shown.get(&key), Some(Lod::Block | Lod::Merged));
                    let merged = shows && d <= ground + if now == Some(Lod::Merged) { SLACK } else { 0.0 };
                    if d <= reach {
                        want.insert(key, (if merged { Lod::Merged } else { Lod::Block }, d));
                    }

                    continue;
                }

                zone.insert(key);
                for cy in key.1..(key.1 + BLOCK).min(ch) {
                    for cx in key.0..(key.0 + BLOCK).min(cw) {
                        let d = distance(cx * CHUNK, cy * CHUNK, CHUNK);
                        let now = self.wanted.get(&(cx, cy)).copied().filter(|l| !l.is_block());
                        if let Some(l) = self.level(d, now) {
                            want.insert((cx, cy), (l, d));
                        }
                    }
                }
            }
        }

        // The zone moved: the chunks by the blocks that came or left built again, for the cells of tiles they fill in (`Frame::build_chunk`).
        {
            let mut now = lock(&self.shared.zone);
            if *now != zone {
                let moved: Vec<ChunkKey> = now.symmetric_difference(&zone).copied().collect();
                let near = (f.reach() + CHUNK - 1) / CHUNK + 1;
                self.wanted.retain(|&(cx, cy), l| {
                    l.is_block()
                        || !moved
                            .iter()
                            .any(|&(bx, by)| cx >= bx - near && cx < bx + BLOCK + near && cy >= by - near && cy < by + BLOCK + near)
                });
                *now = zone;
            }
        }

        let mut out = vec![];

        // The chunks that go: off the queue, off the scene.
        let gone: std::collections::HashSet<ChunkKey> =
            self.wanted.keys().chain(self.shown.keys()).filter(|k| !want.contains_key(k)).copied().collect();
        for k in gone {
            self.wanted.remove(&k);
            if self.shown.remove(&k).is_some() {
                out.push(ChunkChange::Remove(k));
            }
        }

        {
            let mut q = lock(&self.shared.queue);
            q.retain(|j| want.get(&j.key).is_some_and(|(l, _)| *l == j.lod));
            for (&k, &(l, d)) in &want {
                // Near before mid before ground before far, each by distance.
                let order = l.rank() as f64 * 1.0e6 + d;
                if self.wanted.get(&k) == Some(&l) {
                    if let Some(j) = q.iter_mut().find(|j| j.key == k) {
                        j.order = order;
                    }

                    continue;
                }

                self.wanted.insert(k, l);
                q.retain(|j| j.key != k);
                q.push(Job { key: k, lod: l, order });
            }
        }

        self.shared.work.notify_all();

        // What the workers finished, if still wanted at that level.
        while let Ok((k, l, entities, built_with)) = self.results.try_recv() {
            if built_with == generation && self.wanted.get(&k) == Some(&l) {
                self.shown.insert(k, l);
                out.push(ChunkChange::Set(k, l, entities));
            }
        }

        if self.first_s.is_none() && !self.wanted.is_empty() && self.busy() == 0 {
            self.first_s = Some(self.started.elapsed().as_secs_f64());
        }

        out
    }

    /// Builds the chunks over a rectangle of map cells (x0, y0, x1, y1; end exclusive) again, at their levels: the LF under them changed
    /// (the Terrain tool's brushes), and their objects stand on it. A chunk owns the placements whose origin it holds, so the chunks up to
    /// the largest tile's size before the rectangle too. What they show stays until the new version is built.
    pub fn rebuild(&mut self, cells: [i32; 4]) {
        self.refresh_far(cells);
        let reach = self.frame().reach();
        let (x0, y0) = ((cells[0] - reach).div_euclid(CHUNK), (cells[1] - reach).div_euclid(CHUNK));
        let (x1, y1) = ((cells[2] - 1).div_euclid(CHUNK), (cells[3] - 1).div_euclid(CHUNK));
        self.wanted.retain(|&(cx, cy), l| {
            let side = l.side() - 1;
            cx + side < x0 || cx > x1 || cy + side < y0 || cy > y1
        });
    }

    /// The blocks and the far chunks over a rectangle of map cells made again at the next update: the far material was baked again
    /// there (their relief). Not `rebuild`: that asks for another bake.
    pub fn rebuild_blocks_over(&mut self, cells: [i32; 4]) {
        let reach = self.frame().reach();
        let (x0, y0) = ((cells[0] - reach).div_euclid(CHUNK), (cells[1] - reach).div_euclid(CHUNK));
        let (x1, y1) = ((cells[2] - 1).div_euclid(CHUNK), (cells[3] - 1).div_euclid(CHUNK));
        self.wanted.retain(|&(cx, cy), l| {
            let side = l.side() - 1;
            !(l.is_block() || *l == Lod::Far) || cx + side < x0 || cx > x1 || cy + side < y0 || cy > y1
        });
    }

    /// The blocks and the far chunks made again at the next update (their relief once the far material is made).
    pub fn rebuild_far(&mut self) {
        self.wanted.retain(|_, l| !l.is_block() && *l != Lod::Far);
    }

    /// The chunks wanted and not on the scene at their level yet.
    pub fn busy(&self) -> usize {
        self.wanted.iter().filter(|(k, l)| self.shown.get(k) != Some(l)).count()
    }

    /// Updates until every chunk around the focus is built (for scripts and checks): everything that changed.
    pub fn run_to_idle(&mut self, focus: (f64, f64)) -> Vec<ChunkChange> {
        let mut out = self.update(focus);
        while self.busy() > 0 {
            match self.results.recv_timeout(std::time::Duration::from_millis(50)) {
                Ok(r) => {
                    let (k, l, entities, built_with) = r;
                    if built_with == self.shared.frame().1 && self.wanted.get(&k) == Some(&l) {
                        self.shown.insert(k, l);
                        out.push(ChunkChange::Set(k, l, entities));
                    }
                }

                Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
                Err(_) => break,
            }
        }

        out.extend(self.update(focus));
        out
    }

    /// The chunks on the scene per level.
    pub fn shown(&self) -> [usize; 7] {
        let mut n = [0; 7];
        for l in self.shown.values() {
            n[l.rank()] += 1;
        }

        n
    }

    /// What it did so far, for the panel ([`StreamStats`]).
    pub fn stats(&self) -> StreamStats {
        let f = self.frame();
        let s = lock(&self.shared.stats);
        StreamStats {
            map: f.map.name.clone(),
            shown: self.shown(),
            building: self.busy(),
            opened: s.opened,
            open_s: s.open_s,
            from_cache: s.from_cache,
            failed: s.failed.len(),
            mb: self.shared.tiles.bytes() >> 20,
            lf_note: f.lf_note(),
            map_s: self.frame_s,
            first_s: self.first_s,
        }
    }

    /// What it did so far, as one line (the logs and the scripts' `terrain_settle`).
    pub fn report(&self) -> String {
        self.stats().line()
    }

    /// The tiles that could not be opened (the first few).
    pub fn failed(&self) -> Vec<String> {
        lock(&self.shared.stats).failed.iter().take(10).cloned().collect()
    }
}

impl StreamStats {
    /// As one line ([`Streamer::report`]).
    pub fn line(&self) -> String {
        let s = self;
        let [near, mid, ground, far, blocks, merged, distant] = s.shown;
        let failed = if s.failed == 0 { String::new() } else { format!(", {} could not be opened", s.failed) };
        format!(
            "{}: chunks {near} near, {mid} mid, {ground} ground, {far} far, {merged} merged blocks, {blocks} far blocks, {distant} distant{}; tiles: {} opened ({:.1} s), {} from the cache{failed}, {} MB in memory; {} (map {:.1} s{})",
            s.map,
            if s.building > 0 { format!(", {} building", s.building) } else { String::new() },
            s.opened,
            s.open_s,
            s.from_cache,
            s.mb,
            s.lf_note,
            s.map_s,
            s.first_s.map_or(String::new(), |t| format!(", first ring {t:.1} s"))
        )
    }
}

impl Drop for Streamer {
    fn drop(&mut self) {
        self.shared.stop.store(true, Ordering::Release);
        lock(&self.shared.queue).clear();
        self.shared.work.notify_all();
        self.shared.far_wake.notify_all();

        // A worker in the middle of a tile finishes it on its own (the threads are not joined: closing the context must not wait for a tile).
        self.workers.clear();
    }
}
