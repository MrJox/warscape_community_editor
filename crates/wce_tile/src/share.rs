//! Shared projects: a tile project in one zip, for another user to import into their projects folder (File > Share project, File > Import
//! shared project).
//!
//! The zip holds a manifest ([`MANIFEST`]) and, under `raw_data/`, the files as they lie in a kit's `raw_data` (the projects' kit layout):
//! - the tile's location folder (every variation, and whatever else is in it: rivers, terrain layers), without `.bak` files and dot files;
//! - its tile database entry (`_tile_database/TILES/<name>.xml`);
//! - the projects' prefabs (`.wce`, `art/prefabs`) its references use (`project_files::PREFAB_ATTR`), followed into the prefabs they use,
//!   and the road pieces (`road_pieces::FOLDER/<type>`) of the road types its procedural roads are laid with.
//!
//! A prefab that is not the projects' (the Assembly Kit's) is not copied: the manifest names it, and the import says which of them the
//! receiver lacks. Paths in the zip are `/`-separated and relative; an entry that would land outside the projects folder is refused.
//!
//! The manifest is lines of `<key> <value>`: `wce_share 1`, `game <key>`, `location <raw_data path>`, `entry <raw_data path>`,
//! `variation <folder>` (the one to open), `kit_prefab <key>` and `missing_prefab <key>` (found nowhere when it was shared).

use std::collections::{BTreeMap, BTreeSet};
use std::fs::File;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use crate::new_instances::ROAD_SPLINE;
use crate::prefabs::PrefabLibrary;
use crate::project_files::PREFAB_ATTR;

/// The manifest's file name in the zip.
pub const MANIFEST: &str = "wce_share.txt";

/// The folder the kit-layout files are under in the zip.
const RAW: &str = "raw_data/";

const VERSION: u32 = 1;

/// The road type a road without one is laid with (`roads::road_type`'s default).
const DEFAULT_ROAD_TYPE: &str = "dirt";

/// What a shared zip says about itself.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Manifest {
    pub version: u32,
    pub game: String,

    /// The tile's location folder and its entry, relative to `raw_data` (`/`).
    pub location: String,
    pub entry: String,

    /// The variation to open (its folder name), empty: the first.
    pub variation: String,

    /// The prefabs it uses that are not in it: the Assembly Kit's, and those found nowhere when it was shared.
    pub kit_prefabs: Vec<String>,
    pub missing_prefabs: Vec<String>,
}

impl Manifest {
    fn to_text(&self) -> String {
        let mut s = format!("wce_share {}\ngame {}\nlocation {}\nentry {}\nvariation {}\n", self.version, self.game, self.location, self.entry, self.variation);
        for k in &self.kit_prefabs {
            s += &format!("kit_prefab {k}\n");
        }

        for k in &self.missing_prefabs {
            s += &format!("missing_prefab {k}\n");
        }

        s
    }

    fn parse(text: &str) -> Result<Self, String> {
        let mut m = Self::default();
        for line in text.lines() {
            let (key, value) = line.split_once(' ').unwrap_or((line, ""));
            let value = value.trim().to_owned();
            match key {
                "wce_share" => m.version = value.parse().map_err(|_| format!("{MANIFEST}: a version that does not read: {value}"))?,
                "game" => m.game = value,
                "location" => m.location = value,
                "entry" => m.entry = value,
                "variation" => m.variation = value,
                "kit_prefab" => m.kit_prefabs.push(value),
                "missing_prefab" => m.missing_prefabs.push(value),
                _ => {}
            }
        }

        if m.version == 0 || m.location.is_empty() || m.entry.is_empty() {
            return Err(format!("{MANIFEST} is not a shared project's manifest"));
        }

        if m.version > VERSION {
            return Err(format!("the project was shared by a newer editor (format {}, this one reads {VERSION}): update the editor", m.version));
        }

        Ok(m)
    }
}

/// A tile project to share: its files (path in the zip under `raw_data/`, path on disk) and its manifest.
#[derive(Clone, Debug, Default)]
pub struct Share {
    pub files: Vec<(String, PathBuf)>,
    pub manifest: Manifest,
}

/// A path relative to `root`, `/`-separated.
fn relative(path: &Path, root: &Path) -> Option<String> {
    Some(path.strip_prefix(root).ok()?.to_string_lossy().replace('\\', "/"))
}

/// Every file below `dir` but `.bak` files and what starts with a dot, sorted.
fn files_below(dir: &Path) -> Vec<PathBuf> {
    let mut out = vec![];
    let mut stack = vec![dir.to_path_buf()];
    while let Some(d) = stack.pop() {
        let Ok(read) = std::fs::read_dir(&d) else { continue };
        for entry in read.flatten() {
            let path = entry.path();
            let name = entry.file_name().to_string_lossy().into_owned();
            if name.starts_with('.') || name.to_ascii_lowercase().ends_with(".bak") {
                continue;
            }

            if path.is_dir() {
                stack.push(path);
            } else {
                out.push(path);
            }
        }
    }

    out.sort();
    out
}

/// The values of an attribute in an XML text, as written (`name="v"` or `name='v'`).
fn attr_values(text: &str, name: &str) -> Vec<String> {
    let mut out = vec![];
    let mut rest = text;
    while let Some(i) = rest.find(name) {
        let before = rest[..i].chars().next_back();
        rest = &rest[i + name.len()..];
        if !before.is_some_and(char::is_whitespace) {
            continue;
        }

        let Some(after) = rest.trim_start().strip_prefix('=') else { continue };
        let after = after.trim_start();
        let Some(quote) = after.chars().next().filter(|&c| c == '"' || c == '\'') else { continue };
        if let Some(end) = after[1..].find(quote) {
            out.push(after[1..1 + end].to_owned());
        }
    }

    out
}

/// The prefab keys a tile or prefab text references, and the road types its roads are laid with.
fn references(text: &str) -> (Vec<String>, Vec<String>) {
    let prefabs = attr_values(text, PREFAB_ATTR).into_iter().filter(|k| !k.is_empty()).collect();
    let mut roads = vec![];
    if text.contains(&format!("<{ROAD_SPLINE}")) {
        // A road without a type is laid with the default one.
        let types = attr_values(text, "road_type");
        let roads_written = text.matches(&format!("<{ROAD_SPLINE}")).count();
        if types.len() < roads_written {
            roads.push(DEFAULT_ROAD_TYPE.to_owned());
        }

        roads.extend(types.into_iter().filter(|t| !t.is_empty()));
    }

    (prefabs, roads)
}

/// What a tile project shares (`raw_root`: the `raw_data` folder the tile is in; `entry_file`: its database entry; `location`: its
/// location folder relative to `raw_root`). `projects_raw` is the projects' `raw_data`, whose prefabs are copied; `prefabs` the library the
/// editor resolves keys with (the projects' first, then the kits'), which says which keys are the kits'.
///
/// # Errors
///
/// The location folder or the entry is not there.
pub fn collect(raw_root: &Path, entry_file: &Path, location: &str, variation: &str, projects_raw: &Path, prefabs: &PrefabLibrary, game: &str) -> Result<Share, String> {
    let location = location.replace('\\', "/").trim_matches('/').to_owned();
    let dir = raw_root.join(&location);
    if !dir.is_dir() {
        return Err(format!("the tile's folder {} is not there", dir.display()));
    }

    let entry = relative(entry_file, raw_root).filter(|_| entry_file.is_file()).ok_or_else(|| format!("the tile's entry {} is not in {}", entry_file.display(), raw_root.display()))?;
    let mut files = BTreeMap::new();
    files.insert(entry.clone(), entry_file.to_path_buf());
    let mut texts = vec![];
    for f in files_below(&dir) {
        let rel = relative(&f, raw_root).unwrap_or_default();
        let name = f.file_name().map(|n| n.to_string_lossy().to_ascii_lowercase()).unwrap_or_default();
        if name == crate::project_files::WCE_FILE || name == crate::project_files::TED_FILE {
            texts.push(String::from_utf8_lossy(&std::fs::read(&f).map_err(|e| format!("{}: {e}", f.display()))?).into_owned());
        }

        files.insert(rel, f);
    }

    // The prefabs, followed into those they use; the road types' pieces.
    let projects_prefabs = crate::prefabs::prefab_root(projects_raw);
    let (mut kit, mut missing, mut seen, mut road_types) = (BTreeSet::new(), BTreeSet::new(), BTreeSet::new(), BTreeSet::new());
    let mut queue = texts.iter().map(|t| references(t)).collect::<Vec<_>>();
    while let Some((keys, roads)) = queue.pop() {
        road_types.extend(roads);
        for key in keys {
            if !seen.insert(key.to_lowercase()) {
                continue;
            }

            match prefabs.get(&key) {
                Some(p) if p.path.starts_with(&projects_prefabs) => {
                    let text = std::fs::read(&p.path).map_err(|e| format!("{}: {e}", p.path.display()))?;
                    queue.push(references(&String::from_utf8_lossy(&text)));
                    files.insert(relative(&p.path, projects_raw).unwrap_or_default(), p.path.clone());
                }

                Some(p) => {
                    kit.insert(p.key.clone());
                }

                None => {
                    missing.insert(key);
                }
            }
        }
    }

    for t in road_types.iter().filter(|t| *t != "none") {
        let folder = projects_prefabs.join(crate::road_pieces::FOLDER).join(t);
        for f in files_below(&folder) {
            let text = std::fs::read(&f).map_err(|e| format!("{}: {e}", f.display()))?;
            for key in references(&String::from_utf8_lossy(&text)).0 {
                if prefabs.get(&key).is_some_and(|p| !p.path.starts_with(&projects_prefabs)) {
                    kit.insert(key);
                }
            }

            files.insert(relative(&f, projects_raw).unwrap_or_default(), f);
        }
    }

    let manifest = Manifest {
        version: VERSION, game: game.to_owned(), location, entry, variation: variation.to_owned(), kit_prefabs: kit.into_iter().collect(),
        missing_prefabs: missing.into_iter().collect(),
    };

    Ok(Share { files: files.into_iter().collect(), manifest })
}

/// Writes a share into a zip (deflated; the large maps as zip64).
///
/// # Errors
///
/// A file that does not read, or the zip that cannot be written.
pub fn write_zip(share: &Share, path: &Path) -> Result<(), String> {
    let fail = |e: &dyn std::fmt::Display| format!("{}: {e}", path.display());
    let mut zip = zip::ZipWriter::new(std::io::BufWriter::new(File::create(path).map_err(|e| fail(&e))?));
    let options = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
    zip.start_file(MANIFEST, options).map_err(|e| fail(&e))?;
    zip.write_all(share.manifest.to_text().as_bytes()).map_err(|e| fail(&e))?;
    for (rel, disk) in &share.files {
        let mut f = File::open(disk).map_err(|e| format!("{}: {e}", disk.display()))?;
        let large = f.metadata().map_or(true, |m| m.len() >= u64::from(u32::MAX));
        zip.start_file(format!("{RAW}{rel}"), options.large_file(large)).map_err(|e| fail(&e))?;
        std::io::copy(&mut f, &mut zip).map_err(|e| format!("{}: {e}", disk.display()))?;
    }

    zip.finish().map_err(|e| fail(&e))?.flush().map_err(|e| fail(&e))
}

/// What importing a zip would do: its manifest, and its files (relative to `raw_data`) by what is in the projects there now.
#[derive(Clone, Debug, Default)]
pub struct ImportPlan {
    pub manifest: Manifest,

    /// Not there yet.
    pub new: Vec<String>,

    /// There with the same bytes: skipped.
    pub same: Vec<String>,

    /// There with other bytes: replaced only when the user says so (the old file kept as `.bak`).
    pub differs: Vec<String>,

    /// Files of the tile's location folder there now that the zip does not have: moved aside (`.bak`) when the tile is replaced, so that the
    /// project is the one shared and not a mix.
    pub stale: Vec<String>,

    /// The kit prefabs it needs that `prefabs` lacks (and those it lacked when shared).
    pub lacking_prefabs: Vec<String>,
}

impl ImportPlan {
    /// Whether the import replaces something.
    #[must_use]
    pub fn overwrites(&self) -> bool {
        !self.differs.is_empty() || !self.stale.is_empty()
    }

    /// The variation folder to open after it, relative to `raw_data`.
    #[must_use]
    pub fn open_path(&self) -> String {
        if self.manifest.variation.is_empty() { self.manifest.location.clone() } else { format!("{}/{}", self.manifest.location, self.manifest.variation) }
    }
}

/// A zip entry's path relative to `raw_data`, when it is one and stays inside it.
fn raw_path(name: &str) -> Option<String> {
    let rel = name.replace('\\', "/").strip_prefix(RAW)?.to_owned();
    let safe = !rel.is_empty() && !rel.starts_with('/') && !rel.contains(':') && rel.split('/').all(|p| !p.is_empty() && p != "." && p != "..");
    safe.then_some(rel)
}

/// Whether a file holds the bytes of a reader, without reading either whole.
fn same_bytes(path: &Path, size: u64, mut other: impl Read) -> bool {
    let Ok(mut f) = File::open(path) else { return false };
    if f.metadata().map_or(true, |m| m.len() != size) {
        return false;
    }

    let (mut a, mut b) = (vec![0u8; 1 << 16], vec![0u8; 1 << 16]);
    loop {
        let Ok(n) = f.read(&mut a) else { return false };
        if n == 0 {
            return other.read(&mut b[..1]).is_ok_and(|m| m == 0);
        }

        if other.read_exact(&mut b[..n]).is_err() || a[..n] != b[..n] {
            return false;
        }
    }
}

fn open_zip(path: &Path) -> Result<zip::ZipArchive<std::io::BufReader<File>>, String> {
    let file = File::open(path).map_err(|e| format!("{}: {e}", path.display()))?;
    zip::ZipArchive::new(std::io::BufReader::new(file)).map_err(|e| format!("{} is not a zip: {e}", path.display()))
}

/// Reads a shared zip and compares it with the projects' `raw_data` (`projects_raw`). `prefabs`: the library the editor resolves prefab keys
/// with, which says which of the kit prefabs it needs are missing; `game`: the game it is for (another game's zip is refused).
///
/// # Errors
///
/// Not a shared project, another game's, or a file in it that would land outside the projects folder.
pub fn plan_import(zip_path: &Path, projects_raw: &Path, prefabs: &PrefabLibrary, game: &str) -> Result<ImportPlan, String> {
    let mut zip = open_zip(zip_path)?;
    let mut text = String::new();
    zip.by_name(MANIFEST).map_err(|_| format!("{} is not a shared project (no {MANIFEST})", zip_path.display()))?.read_to_string(&mut text).map_err(|e| e.to_string())?;
    let manifest = Manifest::parse(&text)?;
    if !manifest.game.is_empty() && !manifest.game.eq_ignore_ascii_case(game) {
        return Err(format!("the project is for {}, not {game}: switch the game first", manifest.game));
    }

    let mut plan = ImportPlan { manifest, ..Default::default() };
    let mut names = BTreeSet::new();
    for i in 0..zip.len() {
        let entry = zip.by_index(i).map_err(|e| e.to_string())?;
        let name = entry.name().map_err(|e| e.to_string())?.into_owned();
        if entry.is_dir() || name == MANIFEST {
            continue;
        }

        let rel = raw_path(&name).ok_or_else(|| format!("{} has a file outside its folder: {name}", zip_path.display()))?;
        let target = projects_raw.join(&rel);
        let size = entry.size();
        if !target.exists() {
            plan.new.push(rel.clone());
        } else if same_bytes(&target, size, entry) {
            plan.same.push(rel.clone());
        } else {
            plan.differs.push(rel.clone());
        }

        names.insert(rel);
    }

    let location = projects_raw.join(&plan.manifest.location);
    plan.stale = files_below(&location).iter().filter_map(|f| relative(f, projects_raw)).filter(|r| !names.contains(r)).collect();
    plan.lacking_prefabs = plan.manifest.kit_prefabs.iter().filter(|k| prefabs.get(k).is_none()).chain(&plan.manifest.missing_prefabs).cloned().collect();
    Ok(plan)
}

/// Moves a file aside to `<name>.bak` (replacing an older one).
fn move_aside(path: &Path) -> Result<(), String> {
    let mut bak = path.as_os_str().to_owned();
    bak.push(".bak");
    std::fs::rename(path, &bak).map_err(|e| format!("cannot move {} aside: {e}", path.display()))
}

/// What an import did.
#[derive(Clone, Debug, Default)]
pub struct ImportReport {
    pub written: usize,
    pub replaced: usize,
    pub skipped: usize,
    pub moved_aside: usize,
}

/// Imports a shared zip into the projects' `raw_data` as `plan` says: new files written, identical ones skipped, and those that differ and
/// the tile's stale files moved aside (`.bak`) and replaced only with `overwrite` (without it, a plan that overwrites is refused).
///
/// # Errors
///
/// The plan overwrites without `overwrite`, or a file that cannot be read or written.
pub fn import(zip_path: &Path, projects_raw: &Path, plan: &ImportPlan, overwrite: bool) -> Result<ImportReport, String> {
    if plan.overwrites() && !overwrite {
        return Err(format!("{} file(s) of the project are there already and differ", plan.differs.len() + plan.stale.len()));
    }

    let mut zip = open_zip(zip_path)?;
    let mut report = ImportReport { skipped: plan.same.len(), ..Default::default() };
    for rel in &plan.stale {
        move_aside(&projects_raw.join(rel))?;
        report.moved_aside += 1;
    }

    for rel in plan.new.iter().chain(&plan.differs) {
        let target = projects_raw.join(rel);
        if target.exists() {
            move_aside(&target)?;
            report.replaced += 1;
        } else {
            report.written += 1;
        }

        if let Some(parent) = target.parent() {
            std::fs::create_dir_all(parent).map_err(|e| format!("cannot create {}: {e}", parent.display()))?;
        }

        let mut entry = zip.by_name(&format!("{RAW}{rel}")).map_err(|e| format!("{rel}: {e}"))?;
        let mut out = std::io::BufWriter::new(File::create(&target).map_err(|e| format!("{}: {e}", target.display()))?);
        std::io::copy(&mut entry, &mut out).map_err(|e| format!("{}: {e}", target.display()))?;
        out.flush().map_err(|e| format!("{}: {e}", target.display()))?;
    }

    wce_formats::tile_index::TileIndex::forget_shared();
    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write(path: &Path, text: &str) {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, text).unwrap();
    }

    /// A projects raw_data with a tile using a project prefab (which uses another), a kit prefab and a road; a kit with that prefab.
    fn projects() -> (tempfile::TempDir, PathBuf, PathBuf, PrefabLibrary) {
        let dir = tempfile::tempdir().unwrap();
        let raw = dir.path().join("projects/assembly_kit/raw_data");
        let kit = dir.path().join("kit/raw_data");
        let loc = raw.join("terrain/tiles/battle/my_set/my_tile");
        write(&loc.join("a/tile.wce"), &format!(r#"<project><g {PREFAB_ATTR}="house_block"/><g {PREFAB_ATTR}="Kit_Rock"/><g {PREFAB_ATTR}="gone"/><{ROAD_SPLINE} road_type="stone"/></project>"#));
        write(&loc.join("a/height_map_0.png"), "png");
        write(&loc.join("a/tile.wce.bak"), "old");
        write(&loc.join("b/tile.wce"), "<project/>");
        write(&raw.join("terrain/tiles/battle/_tile_database/TILES/my_set_my_tile.xml"), "<TILE/>");
        write(&raw.join("art/prefabs/town/house_block.wce"), &format!(r#"<INSTANCE_SELECTION><g {PREFAB_ATTR}="chimney"/></INSTANCE_SELECTION>"#));
        write(&raw.join("art/prefabs/chimney.wce"), "<INSTANCE_SELECTION/>");
        write(&raw.join("art/prefabs/unused.wce"), "<INSTANCE_SELECTION/>");
        write(&raw.join("art/prefabs/roads_proc/stone/road_stone_straight.wce"), "<INSTANCE_SELECTION/>");
        write(&raw.join("art/prefabs/roads_proc/dirt/road_dirt_straight.wce"), "<INSTANCE_SELECTION/>");
        write(&kit.join("art/prefabs/kit_rock.xml"), "<INSTANCE_SELECTION/>");
        let lib = PrefabLibrary::scan(&[raw.clone(), kit]);
        (dir, raw, loc, lib)
    }

    #[test]
    fn a_share_has_the_tile_its_entry_and_the_projects_prefabs_it_uses_and_imports_elsewhere() {
        let (dir, raw, _, lib) = projects();
        let entry = raw.join("terrain/tiles/battle/_tile_database/TILES/my_set_my_tile.xml");
        let share = collect(&raw, &entry, "terrain\\tiles\\battle\\my_set\\my_tile", "a", &raw, &lib, "attila").unwrap();
        let names = share.files.iter().map(|f| f.0.as_str()).collect::<Vec<_>>();
        assert_eq!(names, [
            "art/prefabs/chimney.wce",
            "art/prefabs/roads_proc/stone/road_stone_straight.wce",
            "art/prefabs/town/house_block.wce",
            "terrain/tiles/battle/_tile_database/TILES/my_set_my_tile.xml",
            "terrain/tiles/battle/my_set/my_tile/a/height_map_0.png",
            "terrain/tiles/battle/my_set/my_tile/a/tile.wce",
            "terrain/tiles/battle/my_set/my_tile/b/tile.wce",
        ]);
        assert_eq!(share.manifest.kit_prefabs, ["kit_rock"]);
        assert_eq!(share.manifest.missing_prefabs, ["gone"]);

        let zip = dir.path().join("shared.zip");
        write_zip(&share, &zip).unwrap();

        // Into empty projects: all new, the kit prefab missing there.
        let other = dir.path().join("other/raw_data");
        let empty = PrefabLibrary::scan(&[other.clone()]);
        let plan = plan_import(&zip, &other, &empty, "attila").unwrap();
        assert_eq!(plan.new.len(), 7);
        assert!(!plan.overwrites());
        assert_eq!(plan.lacking_prefabs, ["kit_rock", "gone"]);
        assert_eq!(plan.open_path(), "terrain/tiles/battle/my_set/my_tile/a");
        let report = import(&zip, &other, &plan, false).unwrap();
        assert_eq!(report.written, 7);
        assert_eq!(std::fs::read(other.join("art/prefabs/town/house_block.wce")).unwrap(), std::fs::read(raw.join("art/prefabs/town/house_block.wce")).unwrap());

        // Again: all the same, nothing to ask.
        let plan = plan_import(&zip, &other, &empty, "attila").unwrap();
        assert_eq!((plan.new.len(), plan.same.len()), (0, 7));
        assert!(!plan.overwrites());

        // Changed there, and a file the zip lacks: overwriting asks, and keeps both aside.
        write(&other.join("terrain/tiles/battle/my_set/my_tile/a/tile.wce"), "<project mine='1'/>");
        write(&other.join("terrain/tiles/battle/my_set/my_tile/a/terrain_layers.wcl"), "x");
        let plan = plan_import(&zip, &other, &empty, "attila").unwrap();
        assert_eq!(plan.differs, ["terrain/tiles/battle/my_set/my_tile/a/tile.wce"]);
        assert_eq!(plan.stale, ["terrain/tiles/battle/my_set/my_tile/a/terrain_layers.wcl"]);
        assert!(import(&zip, &other, &plan, false).is_err());
        let report = import(&zip, &other, &plan, true).unwrap();
        assert_eq!((report.replaced, report.moved_aside), (1, 1));
        assert!(other.join("terrain/tiles/battle/my_set/my_tile/a/tile.wce.bak").is_file());
        assert!(!other.join("terrain/tiles/battle/my_set/my_tile/a/terrain_layers.wcl").exists());

        // Another game's is refused.
        assert!(plan_import(&zip, &other, &empty, "rome2").is_err());
    }

    #[test]
    fn a_zip_path_outside_raw_data_is_refused() {
        assert_eq!(raw_path("raw_data/art/prefabs/a.wce").as_deref(), Some("art/prefabs/a.wce"));
        assert_eq!(raw_path("raw_data/../evil"), None);
        assert_eq!(raw_path("raw_data//abs"), None);
        assert_eq!(raw_path("raw_data/c:/x"), None);
        assert_eq!(raw_path("other/x"), None);
    }

    #[test]
    fn the_manifest_reads_back() {
        let m = Manifest { version: 1, game: "attila".into(), location: "terrain/tiles/battle/a/b".into(), entry: "e.xml".into(), variation: "v".into(), kit_prefabs: vec!["k".into()], missing_prefabs: vec!["m".into()] };
        assert_eq!(Manifest::parse(&m.to_text()).unwrap(), m);
        assert!(Manifest::parse("wce_share 99\nlocation a\nentry b\n").is_err(), "a newer format");
    }
}
