//! The map context's chunks around a focus point, built on worker threads as the focus moves.
//!
//! Every update the streamer works out the level each chunk within the far ring should have (by its distance from the focus, with some
//! slack so that a chunk on a ring's edge does not flip back and forth), queues the chunks whose level changes, nearest first (a queued chunk
//! that is no longer wanted is dropped from the queue), and hands back what the workers finished: a chunk to put on the scene at a level, or
//! one to take off. The workers share the tiles: a tile is opened once (or read from the disk cache) while others wait for it, and kept in
//! memory up to a budget (the least recently used go first; the scene keeps what it draws).

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, Sender, channel};
use std::sync::{Arc, Condvar, Mutex, MutexGuard};

use wce_foundation::math::Float4x4;
use wce_scene::render_scene::Payload;
use wce_vfs::files::Files;

use super::template::{Template, load_template};
use crate::terrain_bake::TerrainLook;
use super::{CHUNK, ContextRequest, Frame, TileKey, cache, tile_dir};

/// A chunk: its column and row of CHUNK x CHUNK map cells.
pub type ChunkKey = (i32, i32);

/// A chunk a worker finished: its entities at a level.
type Built = (ChunkKey, Lod, Vec<(Float4x4, Payload)>);

/// A chunk's level of detail.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Lod {
    /// Everything of its tiles.
    Near,

    /// Their coarse heights, low-resolution materials, buildings and rivers.
    Mid,

    /// The LF ground alone.
    Far,
}

/// What changed on the scene's side.
pub enum ChunkChange {
    /// A chunk's entities at a level (replacing what it had).
    Set(ChunkKey, Lod, Vec<(Float4x4, Payload)>),
    Remove(ChunkKey),
}

/// Threads that build chunks (opening a tile holds its whole scene and height map: a few at a time).
const WORKERS: usize = 4;

/// The memory the tiles in memory may take (bytes, about).
const TEMPLATE_BUDGET: usize = 1536 << 20;

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
    bytes: usize,
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
    frame: Arc<Frame>,
    rq: ContextRequest,
    files: Option<Arc<dyn Files>>,
    queue: Mutex<Vec<Job>>,
    work: Condvar,
    tiles: Mutex<HashMap<TileKey, Slot>>,
    loaded: Condvar,
    stats: Mutex<Stats>,
    clock: std::sync::atomic::AtomicU64,
    stop: AtomicBool,
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|p| p.into_inner())
}

impl Shared {

    /// A placement's tile: from memory, else read from the cache or opened (one thread does, the others wait for it).
    fn template(&self, i: usize) -> Option<Arc<Template>> {
        // Its climate's look: the map gives each placement its own.
        let p = &self.frame.map.placements[i];
        let climate = self.frame.climate_name(i);
        let look = TerrainLook { climate: climate.clone(), ..self.rq.look.clone() };
        let key = (p.location.clone(), p.variation.clone(), climate);
        let now = self.clock.fetch_add(1, Ordering::Relaxed);
        let mut tiles = lock(&self.tiles);
        loop {
            match tiles.get_mut(&key) {
                Some(Slot::Ready(t, used)) => {
                    *used = now;
                    return Some(t.clone());
                }

                Some(Slot::Failed) => return None,
                Some(Slot::Loading) => tiles = self.loaded.wait(tiles).unwrap_or_else(|p| p.into_inner()),
                None => break,
            }
        }

        tiles.insert(key.clone(), Slot::Loading);
        drop(tiles);
        let mut dir = tile_dir(&self.rq.raw_root, &key.0, &key.1);
        if let Some(provide) = self.rq.provider.as_ref().filter(|_| !dir.is_dir()) {
            match provide(&key.0, &key.1) {
                Ok(made) => dir = made,
                Err(e) => wce_foundation::log::info(format!("map context: {}\\{}: {e}", key.0, key.1)),
            }
        }

        let ck = self.rq.cache.as_ref().map(|c| (c, cache::key(&dir, &look, self.rq.trees)));
        let (result, cached) = self.pool.install(|| match ck.as_ref().and_then(|(c, k)| cache::read(c, *k)) {
            Some(t) => (Ok(t), true),
            None => (load_template(&dir, &look, self.rq.trees, self.files.clone()), false),
        });

        if let (Ok(t), Some((c, k)), false) = (&result, &ck, cached)
            && let Err(e) = cache::write(c, *k, t)
        {
            wce_foundation::log::info(format!("map context: not cached: {}: {e}", dir.display()));
        }

        let mut stats = lock(&self.stats);
        let slot = match result {
            Ok(t) => {
                if cached { stats.from_cache += 1 } else { stats.opened += 1 }
                stats.open_s += t.load_s;
                stats.bytes += t.bytes;
                self.frame.offer_plain(&t.material);
                Slot::Ready(Arc::new(t), now)
            }

            Err(e) => {
                stats.failed.push(format!("{}\\{}: {e}", key.0, key.1));
                Slot::Failed
            }
        };

        // Over the budget: the least recently used go.
        let mut tiles = lock(&self.tiles);
        if stats.bytes > TEMPLATE_BUDGET {
            let mut ready: Vec<(u64, TileKey, usize)> = tiles.iter().filter_map(|(k, s)| match s {
                Slot::Ready(t, used) => Some((*used, k.clone(), t.bytes)),
                _ => None,
            }).collect();

            ready.sort();
            for (_, k, bytes) in ready {
                if stats.bytes <= TEMPLATE_BUDGET * 3 / 4 {
                    break;
                }

                tiles.remove(&k);
                stats.bytes -= bytes;
            }
        }

        drop(stats);
        let out = match &slot {
            Slot::Ready(t, _) => Some(t.clone()),
            _ => None,
        };

        tiles.insert(key, slot);
        self.loaded.notify_all();
        out
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
            let entities = self.frame.build_chunk(job.key, job.lod, &|i| self.template(i));
            if tx.send((job.key, job.lod, entities)).is_err() {
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

    /// The rings around the focus (cells): near, mid, far.
    rings: [i32; 3],
    started: std::time::Instant,
    frame_s: f64,
    first_s: Option<f64>,
}

impl Streamer {

    /// The frame of the request (slow on the campaign map: run it away from the UI thread) and the workers.
    pub fn new(rq: ContextRequest, files: Option<Arc<dyn Files>>) -> Self {
        let started = std::time::Instant::now();
        let cores = std::thread::available_parallelism().map_or(4, |n| n.get());
        let pool = rayon::ThreadPoolBuilder::new()
            .num_threads(cores.saturating_sub(2).clamp(2, 8))
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
        let rings = [rq.near, rq.mid, rq.far];
        let frame_s = started.elapsed().as_secs_f64();
        let shared = Arc::new(Shared {
            pool, frame, rq, files, queue: Mutex::default(), work: Condvar::new(), tiles: Mutex::default(), loaded: Condvar::new(), stats: Mutex::default(),
            clock: Default::default(), stop: AtomicBool::new(false),
        });

        let (tx, results) = channel();
        let workers = (0..WORKERS).map(|_| {
            let (s, tx) = (shared.clone(), tx.clone());
            std::thread::spawn(move || s.worker(tx))
        }).collect();

        Self { shared, results, workers, shown: HashMap::new(), wanted: HashMap::new(), rings, started, frame_s, first_s: None }
    }

    /// New rings (cells): the next update moves the chunks to their levels.
    pub fn set_rings(&mut self, near: i32, mid: i32, far: i32) {
        self.rings = [near, mid, far];
    }

    pub fn frame(&self) -> &Arc<Frame> {
        &self.shared.frame
    }

    /// The open tile's centre in its frame (the focus before there is a camera).
    pub fn open_centre(&self) -> (f64, f64) {
        let f = &self.shared.frame;
        let o = f.open;
        let (cx, cy) = (f64::from(o[0] + o[2]) * 0.5, f64::from(o[1] + o[3]) * 0.5);
        f.from_map.apply(cx, cy)
    }

    /// The level a chunk should have at `d` cells from the focus, given the one it has (it keeps it within the slack).
    fn level(&self, d: f64, now: Option<Lod>) -> Option<Lod> {
        let [near, mid, far] = self.rings;
        let rings = [(Lod::Near, near), (Lod::Mid, mid.max(near)), (Lod::Far, far.max(mid).max(near))];
        let plain = rings.iter().find(|(_, r)| d <= f64::from(*r)).map(|(l, _)| *l);
        let Some(now) = now else { return plain };

        // Within the slack of its own ring's edge it stays.
        let edge = rings.iter().find(|(l, _)| *l == now).map_or(0.0, |(_, r)| f64::from(*r));
        let inner = rings.iter().take_while(|(l, _)| *l != now).last().map_or(f64::MIN, |(_, r)| f64::from(*r));
        if d <= edge + SLACK && d > inner - SLACK { Some(now) } else { plain }
    }

    /// Works out the levels around the focus (a point of the frame), queues what changes and returns what the workers finished.
    pub fn update(&mut self, focus: (f64, f64)) -> Vec<ChunkChange> {
        let f = self.shared.frame.clone();
        let (fx, fy) = f.cell_of(focus.0, focus.1);
        let reach = f64::from(self.rings.into_iter().max().unwrap_or(0)) + SLACK;
        let (cw, ch) = f.chunks();
        let span = |c: f64| ((c - reach) / f64::from(CHUNK)).floor() as i32..=((c + reach) / f64::from(CHUNK)).floor() as i32;
        let mut want: HashMap<ChunkKey, (Lod, f64)> = HashMap::new();
        for cy in span(fy).filter(|&y| y >= 0 && y < ch) {
            for cx in span(fx).filter(|&x| x >= 0 && x < cw) {
                // The distance from the focus to the chunk's cells.
                let (x0, y0) = (f64::from(cx * CHUNK), f64::from(cy * CHUNK));
                let dx = (x0 - fx).max(fx - (x0 + f64::from(CHUNK))).max(0.0);
                let dy = (y0 - fy).max(fy - (y0 + f64::from(CHUNK))).max(0.0);
                let d = (dx * dx + dy * dy).sqrt();
                let now = self.wanted.get(&(cx, cy)).copied();
                if let Some(l) = self.level(d, now) {
                    want.insert((cx, cy), (l, d));
                }
            }
        }

        let mut out = vec![];

        // The chunks that go: off the queue, off the scene.
        let gone: Vec<ChunkKey> = self.wanted.keys().filter(|k| !want.contains_key(k)).copied().collect();
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
                // Near before mid before far, each by distance.
                let order = f64::from(l as i32) * 1.0e6 + d;
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
        while let Ok((k, l, entities)) = self.results.try_recv() {
            if self.wanted.get(&k) == Some(&l) {
                self.shown.insert(k, l);
                out.push(ChunkChange::Set(k, l, entities));
            }
        }

        if self.first_s.is_none() && !self.wanted.is_empty() && self.busy() == 0 {
            self.first_s = Some(self.started.elapsed().as_secs_f64());
        }

        out
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
                    let (k, l, entities) = r;
                    if self.wanted.get(&k) == Some(&l) {
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
    pub fn shown(&self) -> [usize; 3] {
        let mut n = [0; 3];
        for l in self.shown.values() {
            n[*l as usize] += 1;
        }

        n
    }

    /// What it did so far, for the status line.
    pub fn report(&self) -> String {
        let f = &self.shared.frame;
        let s = lock(&self.shared.stats);
        let [near, mid, far] = self.shown();
        let failed = if s.failed.is_empty() { String::new() } else { format!(", {} could not be opened", s.failed.len()) };
        format!(
            "{}: chunks {near} near, {mid} mid, {far} far{}; tiles: {} opened ({:.1} s), {} from the cache{failed}, {} MB in memory; {} (map {:.1} s{})",
            f.map.name, if self.busy() > 0 { format!(", {} building", self.busy()) } else { String::new() }, s.opened, s.open_s, s.from_cache, s.bytes >> 20,
            f.lf_note(), self.frame_s, self.first_s.map_or(String::new(), |t| format!(", first ring {t:.1} s"))
        )
    }

    /// The tiles that could not be opened (the first few).
    pub fn failed(&self) -> Vec<String> {
        lock(&self.shared.stats).failed.iter().take(10).cloned().collect()
    }
}

impl Drop for Streamer {
    fn drop(&mut self) {
        self.shared.stop.store(true, Ordering::Release);
        lock(&self.shared.queue).clear();
        self.shared.work.notify_all();

        // A worker in the middle of a tile finishes it on its own (the threads are not joined: closing the context must not wait for a tile).
        self.workers.clear();
    }
}
