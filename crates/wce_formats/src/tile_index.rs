//! Index of the tile database entries of an Assembly Kit's raw_data folder: one row per `_tile_database/TILES/*.xml` with what a tile
//! list shows (location, set, size in cells, colour, variations, link counts) and the entry's file. Opening a tile resolves its entry
//! from here instead of reading the 626 (battle) / 481 (campaign) files, and the tile browser lists it.
//!
//! The index is built once per process and raw_data folder. With a cache file the rows are keyed by file name, size and modification
//! time: only new or changed entries are read again, so a warm start reads no entry at all (a directory listing gives sizes and times).

use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};
use std::sync::{Arc, LazyLock, Mutex};
use std::time::UNIX_EPOCH;

use rayon::prelude::*;

use crate::tile_entry::{TileEntry, kind_of, normalize_location};

const CACHE_MAGIC: &str = "wce-tile-index\t1";

/// A row of the index.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct TileIndexEntry {
    /// File name without `.xml` (`assembly_kit_osgiliath_test`).
    pub name: String,

    /// The entry file.
    pub xml: PathBuf,

    /// `battle` or `campaign` (from the location).
    pub kind: String,

    /// `TILE@location` as written (backslashes).
    pub location: String,

    /// The location normalized ([`normalize_location`]).
    pub key: String,
    pub tile_set: String,

    /// Cells.
    pub width: u32,
    pub height: u32,

    /// 0..255.
    pub red: f32,
    pub green: f32,
    pub blue: f32,

    /// Leaf folder names.
    pub variations: Vec<String>,

    /// `TILE_LINK` and `TILE_LINK_TARGET` counts.
    pub links: u32,
    pub targets: u32,

    /// Of the entry file.
    pub size: u64,

    /// Last write time, in nanoseconds since the Unix epoch.
    pub mtime: i64,
}

/// An entry file found on disk.
struct Candidate {
    xml: PathBuf,

    /// Relative to raw_root, with `/`.
    rel: String,
    size: u64,
    mtime: i64,
}

/// What [`TileIndex::resolve`] finds for a path.
#[derive(Clone, Copy, Debug, Default)]
pub struct Resolved<'a> {
    pub entry: Option<&'a TileIndexEntry>,

    /// Index in the entry's variations; `None` when the path names the tile folder itself.
    pub variation: Option<usize>,
}

/// The index of one raw_data folder.
#[derive(Debug, Default)]
pub struct TileIndex {
    raw_root: PathBuf,
    entries: Vec<TileIndexEntry>,
    by_key: BTreeMap<String, usize>,

    /// How the index was built (for the log and the gate).
    pub parsed: usize,
    pub from_cache: usize,
    pub removed: usize,
    pub problems: Vec<String>,

    /// Locations claimed by more than one entry.
    pub duplicate_locations: Vec<String>,
}

/// One cache row: rel, size, mtime, location, tile_set, width, height, red, green, blue, links, targets, variations (`|` separated).
fn row_of(e: &TileIndexEntry, rel: &str) -> String {
    format!("{rel}\t{}\t{}\t{}\t{}\t{}\t{}\t{:.6}\t{:.6}\t{:.6}\t{}\t{}\t{}", e.size, e.mtime, e.location, e.tile_set, e.width, e.height,
        f64::from(e.red), f64::from(e.green), f64::from(e.blue), e.links, e.targets, e.variations.join("|"))
}

/// Fills a row from the cache; `None` if the row is not one.
fn fill_from_row(e: &mut TileIndexEntry, f: &[&str]) -> Option<()> {
    let [_, size, mtime, location, tile_set, width, height, red, green, blue, links, targets, variations] = f else { return None };
    e.size = size.parse().ok()?;
    e.mtime = mtime.parse().ok()?;
    e.location = location.to_string();
    e.tile_set = tile_set.to_string();
    e.width = width.parse().ok()?;
    e.height = height.parse().ok()?;
    e.red = red.parse().ok()?;
    e.green = green.parse().ok()?;
    e.blue = blue.parse().ok()?;
    e.links = links.parse().ok()?;
    e.targets = targets.parse().ok()?;
    e.variations = if variations.is_empty() { vec![] } else { variations.split('|').map(str::to_owned).collect() };
    Some(())
}

/// Fills a row from its entry file.
fn fill_from_entry(row: &mut TileIndexEntry, c: &Candidate) -> Result<(), String> {
    let t = TileEntry::load(&c.xml).map_err(|error| error.to_string())?;
    row.size = c.size;
    row.mtime = c.mtime;
    row.location = t.location();
    row.tile_set = t.tile_set();
    row.width = t.width();
    row.height = t.height();
    row.red = t.red() as f32;
    row.green = t.green() as f32;
    row.blue = t.blue() as f32;
    row.variations = t.variations().into_iter().map(|v| v.location).collect();
    row.links = t.links().len() as u32;
    row.targets = t.link_targets().len() as u32;
    Ok(())
}

/// The entry files under `<raw_root>/terrain/tiles/<kind>/_tile_database/TILES`, by relative path.
fn find_candidates(raw_root: &Path) -> Vec<Candidate> {
    let mut found = vec![];
    let Ok(kinds) = std::fs::read_dir(raw_root.join("terrain").join("tiles")) else { return found };
    for kind in kinds.flatten() {
        let Ok(files) = std::fs::read_dir(kind.path().join("_tile_database").join("TILES")) else { continue };
        for file in files.flatten() {
            let xml = file.path();
            let Ok(meta) = file.metadata() else { continue };
            if !meta.is_file() || !xml.extension().is_some_and(|ext| ext.eq_ignore_ascii_case("xml")) {
                continue;
            }

            let rel = xml.strip_prefix(raw_root).unwrap_or(&xml).to_string_lossy().replace('\\', "/");
            let mtime = meta.modified().ok()
                .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
                .map_or(0, |since| i64::try_from(since.as_nanos()).unwrap_or(i64::MAX));
            found.push(Candidate { xml, rel, size: meta.len(), mtime });
        }
    }

    found.sort_by(|a, b| a.rel.cmp(&b.rel));
    found
}

/// The rows of a cache file, by relative path.
fn read_cache(cache: &Path) -> HashMap<String, Vec<String>> {
    let Ok(text) = std::fs::read_to_string(cache) else { return HashMap::new() };
    let mut lines = text.lines();
    if !lines.next().is_some_and(|magic| magic.starts_with(CACHE_MAGIC)) {
        return HashMap::new();
    }

    lines.map(|line| line.split('\t').map(str::to_owned).collect::<Vec<_>>())
        .filter_map(|fields| Some((fields.first()?.clone(), fields)))
        .collect()
}

/// The shared indices of this process, by normalized folder, and the cache folder.
#[derive(Default)]
struct Shared {
    indices: HashMap<String, Arc<TileIndex>>,
    cache_dir: Option<PathBuf>,
}

static SHARED: LazyLock<Mutex<Shared>> = LazyLock::new(Default::default);

/// FNV-1a 64 of a text: names the cache file of a folder.
fn fnv1a(text: &str) -> u64 {
    text.bytes().fold(0xCBF2_9CE4_8422_2325, |hash, byte| (hash ^ u64::from(byte)).wrapping_mul(0x0100_0000_01B3))
}

impl TileIndex {

    /// The entries of one raw_data folder (`<raw_root>/terrain/tiles/<kind>/_tile_database/TILES/*.xml` for every kind). `cache` is
    /// a file the rows are kept in between runs. Unreadable entries are skipped and listed in `problems`.
    pub fn build(raw_root: &Path, cache: Option<&Path>) -> Self {
        let found = find_candidates(raw_root);
        let cached = cache.map(read_cache).unwrap_or_default();

        // Rows from the cache when the file is the one it describes, else read.
        let rows = found.par_iter().map(|c| {
            let mut row = TileIndexEntry {
                xml: c.xml.clone(),
                name: c.xml.file_stem().map(|stem| stem.to_string_lossy().into_owned()).unwrap_or_default(),
                ..Default::default()
            };

            let from_cache = cached.get(&c.rel).is_some_and(|fields| {
                let fields = fields.iter().map(String::as_str).collect::<Vec<_>>();
                fill_from_row(&mut row, &fields).is_some() && row.size == c.size && row.mtime == c.mtime
            });

            let error = if from_cache { None } else { fill_from_entry(&mut row, c).err() };
            row.key = normalize_location(&row.location);
            row.kind = kind_of(&row.key).unwrap_or_default().to_owned();
            (row, from_cache, error)
        }).collect::<Vec<_>>();

        let mut index = Self { raw_root: raw_root.to_path_buf(), ..Default::default() };
        let mut kept = vec![];
        for ((row, from_cache, error), c) in rows.into_iter().zip(&found) {
            if error.is_some() || row.key.is_empty() {
                index.problems.push(format!("{}: {}", c.rel, error.as_deref().unwrap_or("no location")));
                continue;
            }

            if from_cache {
                index.from_cache += 1;
            } else {
                index.parsed += 1;
            }

            kept.push((row, c.rel.as_str()));
        }

        index.removed = cached.keys().filter(|rel| found.binary_search_by(|c| c.rel.as_str().cmp(rel)).is_err()).count();
        if let Some(cache) = cache
            && (index.parsed > 0 || index.removed > 0 || cached.is_empty())
        {
            let mut text = format!("{CACHE_MAGIC}\t{}\n", raw_root.to_string_lossy().replace('\\', "/"));
            for (row, rel) in &kept {
                text.push_str(&row_of(row, rel));
                text.push('\n');
            }

            // A cache that can't be written only costs time on the next start.
            let _ = cache.parent().map(std::fs::create_dir_all);
            let _ = std::fs::write(cache, text);
        }

        // The list order of the browser: by location, then by file name.
        let mut entries = kept.into_iter().map(|(row, _)| row).collect::<Vec<_>>();
        entries.sort_by(|a, b| a.key.cmp(&b.key).then_with(|| a.name.cmp(&b.name)));
        for (i, entry) in entries.iter().enumerate() {
            if i > 0 && entry.key == entries[i - 1].key {
                index.duplicate_locations.push(entry.key.clone());
            }

            index.by_key.entry(entry.key.clone()).or_insert(i);
        }

        index.entries = entries;
        index
    }

    /// The shared index of a folder in this process (built on first use; thread safe). With a cache folder set
    /// ([`Self::set_cache_dir`]), its rows are kept in `tile_index_<hash of the folder>.tsv` there.
    pub fn shared(raw_root: &Path) -> Arc<TileIndex> {
        let key = normalize_location(&raw_root.to_string_lossy());
        let cache_dir = {
            let shared = SHARED.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
            if let Some(index) = shared.indices.get(&key) {
                return index.clone();
            }

            shared.cache_dir.clone()
        };

        // Built outside the lock: a second thread may build the same one, and the first result wins.
        let cache = cache_dir.map(|dir| dir.join(format!("tile_index_{:016x}.tsv", fnv1a(&key))));
        let built = Arc::new(Self::build(raw_root, cache.as_deref()));
        let mut shared = SHARED.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        shared.indices.entry(key).or_insert(built).clone()
    }

    /// The folder the shared indices keep their rows in (set once, at start-up).
    pub fn set_cache_dir(dir: &Path) {
        SHARED.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).cache_dir = Some(dir.to_path_buf());
    }

    /// Drops the shared indices (the files changed on disk).
    pub fn forget_shared() {
        SHARED.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).indices.clear();
    }

    pub fn raw_root(&self) -> &Path {
        &self.raw_root
    }

    pub fn entries(&self) -> &[TileIndexEntry] {
        &self.entries
    }

    /// The entry of a `TILE@location`, in any spelling.
    pub fn find_location(&self, location: &str) -> Option<&TileIndexEntry> {
        self.by_key.get(&normalize_location(location)).map(|&i| &self.entries[i])
    }

    /// The entry of a file name (without .xml), a location, or the end of a location (`<set>/<tile>`); case-insensitive.
    pub fn find_name(&self, name: &str) -> Option<&TileIndexEntry> {
        let want = normalize_location(name);
        if want.is_empty() {
            return None;
        }

        if let Some(entry) = self.entries.iter().find(|e| e.name.to_ascii_lowercase() == want) {
            return Some(entry);
        }

        if let Some(entry) = self.find_location(&want) {
            return Some(entry);
        }

        let tail = format!("/{want}");
        self.entries.iter().find(|e| e.key.len() > tail.len() && e.key.ends_with(&tail))
    }

    /// The entry and variation of a variation folder, a tile.ted or a tile folder of the kit.
    pub fn resolve(&self, path: &Path) -> Resolved<'_> {
        let mut g = normalize_location(&path.to_string_lossy());
        let Some(at) = g.find("terrain/tiles/") else { return Resolved::default() };
        g.drain(..at);
        if g.len() > "/tile.ted".len() && g.ends_with("/tile.ted") {
            g.truncate(g.len() - "/tile.ted".len());
        }

        // The variation is the last folder (or the last two when a variation location has a slash), else the path is the tile folder itself.
        let (mut loc, mut var) = (g, String::new());
        for up in 0..3 {
            if let Some(entry) = self.find_location(&loc) {
                let variation = if up == 0 { None } else { entry.variations.iter().position(|v| normalize_location(v) == var) };
                return Resolved { entry: Some(entry), variation };
            }

            let Some(slash) = loc.rfind('/') else { break };
            var = if up == 0 { loc[slash + 1..].to_owned() } else { format!("{}/{var}", &loc[slash + 1..]) };
            loc.truncate(slash);
        }

        Resolved::default()
    }
}
