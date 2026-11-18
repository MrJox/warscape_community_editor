//! One variation of a combined project written as a project of its own (docs/plans/tile_variations.md, "Exports and loaders"): its
//! tile.wce ([`super::to_variation`]), its maps composed from the layers that belong to it, its terrain layers, its rivers' files, its
//! own files and those the variations share, under a `raw_data` folder of the kit's layout with the tile's entry. Every export of a
//! combined project goes through it: what reads a variation folder (the compile, Save for Assembly Kit) then reads this one as it
//! reads a project of one variation.

use std::path::{Path, PathBuf};

use wce_formats::png_file::PngFile;

use crate::blend_field::BlendField;
use crate::ground_type_field::GroundTypeField;
use crate::height_field::HeightField;
use crate::project_files::{TED_FILE, WCE_FILE, mark_wce_text};
use crate::ted_scene::TedScene;
use crate::tile_project::TileProject;

/// The folder a combined project keeps what one variation has of its own (painted maps, files it does not model):
/// `<tile>/variations/<v>/`.
pub const OWN_DIR: &str = "variations";

/// The maps an export makes from the terrain layers: a project's own copies of them are not taken over.
const MAPS: [&str; 3] = ["height_map_0.png", "blend_map.tif", "ground_type_map.png"];

/// The file in a variation written for the loaders ([`served`]) that says what it was written from ([`stamp_of`]); an export's write
/// has none (it is made from the editor's scene, roads and walls laid), so a loader writes it again.
pub const STAMP: &str = "wce_variation.stamp";

/// Held while variations are written under a [`build_root`]: by the loaders ([`served`], from the map context's threads) and the exports
/// (`TileDocument::materialize_variations`), one at a time.
pub fn writing() -> std::sync::MutexGuard<'static, ()> {
    static ONE: std::sync::Mutex<()> = std::sync::Mutex::new(());
    ONE.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// What a [`STAMP`] is made with: another when what `materialize` writes changes.
const STAMP_VERSION: u32 = 1;

/// Where the variations of the combined projects in a `raw_data` folder are written as projects of their own: beside its kit layout,
/// `<projects>/build/variations/raw_data` for the projects' `<projects>/assembly_kit/raw_data` (docs/plans/tile_variations.md, the
/// loaders). Throw-away: made again when asked for.
#[must_use]
pub fn build_root(raw_root: &Path) -> PathBuf {
    let base = raw_root.parent().and_then(Path::parent).unwrap_or(raw_root);
    base.join("build").join("variations").join("raw_data")
}

/// What a variation of the combined project in `tile_dir` is written from, as a [`STAMP`]: the scene, the layers, the entry, the files the
/// variations share and its own, each by its name, size and time.
#[must_use]
pub fn stamp_of(tile_dir: &Path, entry: Option<&Path>, variation: &str) -> String {
    let mut out = format!("wce variation {STAMP_VERSION} {variation}\n");
    let mut add = |path: &Path| {
        if let Ok(m) = std::fs::metadata(path) {
            let time = m.modified().ok().and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok()).map_or(0, |d| d.as_nanos());
            out += &format!("{} {} {time}\n", path.display(), m.len());
        }
    };

    if let Some(e) = entry {
        add(e);
    }

    // The tile folder's files (not its folders: a variation's own is below), and every file of the variation's own, subfolders too.
    let mut files = std::fs::read_dir(tile_dir)
        .into_iter()
        .flatten()
        .flatten()
        .map(|f| f.path())
        .filter(|p| p.is_file() && !p.to_string_lossy().ends_with(".bak"))
        .collect::<Vec<_>>();
    files.sort();
    let own = tile_dir.join(OWN_DIR).join(variation);
    files.extend(files_below(&own).into_iter().filter(|n| !n.ends_with(".bak")).map(|n| own.join(n)));
    for f in files {
        add(&f);
    }

    out
}

/// For the loaders (`sources::find_tile`): the folder of a variation the combined project in `tile_dir` (under the `raw_data` folder
/// `raw_root`) serves, a project of its own under [`build_root`], written again when what it was written from changed ([`STAMP`]). One
/// at a time: the map context asks from several threads.
///
/// # Errors
///
/// The project does not open, has no such variation, or a file can't be written.
pub fn served(tile_dir: &Path, variation: &str, raw_root: &Path) -> Result<PathBuf, String> {
    let _one = writing();

    // The project's name of it (a map may write it in another case).
    let listed = super::listed_in_file(&tile_dir.join(WCE_FILE));
    let name = listed
        .iter()
        .find(|v| v.eq_ignore_ascii_case(variation))
        .cloned()
        .ok_or_else(|| format!("{} has no variation '{variation}'", tile_dir.display()))?;
    let found = crate::tile_project::find_tile_info(&tile_dir.join(&name));
    let entry = found.entry.as_ref().ok_or_else(|| format!("{}: no tile database entry", tile_dir.display()))?;
    let row = entry.find_variation(&name).ok_or_else(|| format!("{} has no variation '{name}'", entry.file().display()))?;
    let build = build_root(raw_root);
    let dir = entry.variation_folder(&build, row);
    let stamp = stamp_of(tile_dir, Some(entry.file()), &name);
    if std::fs::read_to_string(dir.join(STAMP)).is_ok_and(|s| s == stamp) && crate::tile_project::has_tile(&dir) {
        return Ok(dir);
    }

    let project = TileProject::open(&tile_dir.join(&name))?;
    let m = project.materialize(&name, &project.scene.save(), &build)?;
    put(&m.dir.join(STAMP), stamp.as_bytes())?;
    wce_foundation::log::info(format!("{}: {name} written as a project of its own in {}", tile_dir.display(), m.dir.display()));
    Ok(m.dir)
}

/// A variation written as a project of its own.
#[derive(Clone, Debug)]
pub struct Materialized {
    pub variation: String,

    /// Its folder (`<raw_out>/terrain/tiles/.../<variation>`).
    pub dir: PathBuf,

    /// The height range its height map is stored with, and whether it is another than its row's (the composite left the row's range:
    /// the entry written with it has the new one).
    pub range: (f64, f64),
    pub refit: bool,
}

/// A blank blend map of a size in TEd's layout, for a stack to compose into.
///
/// # Errors
///
/// It does not read back (never: the header is TEd's).
pub fn blank_blend(w: u32, h: u32) -> Result<BlendField, String> {
    let mut bytes = wce_formats::image_containers::TiffFile::ted_header(w, h);
    bytes.resize(bytes.len() + w as usize * h as usize * 32, 0);
    let tif = wce_formats::image_containers::TiffFile::parse(&bytes).map_err(|e| format!("a blank blend map: {e}"))?;
    BlendField::from_tiff(tif)
}

/// A blank ground type map of a size, for a stack to compose into.
///
/// # Errors
///
/// It does not read back (never).
pub fn blank_ground(w: u32, h: u32) -> Result<GroundTypeField, String> {
    let png = PngFile {
        width: w,
        height: h,
        bit_depth: 8,
        color_type: 6,
        pixels: [0, 0, 0, 255].repeat(w as usize * h as usize),
        pixels_edited: true,
        ..Default::default()
    };
    GroundTypeField::from_png(png)
}

/// Whether a file of the combined project's folder is the project's own (its scene, its layers, a backup, a map an export makes, a
/// river's file the scene makes) rather than one the variations share.
fn project_file(name: &str) -> bool {
    let lower = name.to_lowercase();
    lower == WCE_FILE
        || lower == TED_FILE
        || lower == crate::terrain_layers::FILE
        || lower == crate::texture_rules::FILE
        || lower == crate::biomes::FILE
        || lower.ends_with(".bak")
        || lower.starts_with("river_spline")
        || MAPS.contains(&lower.as_str())
}

/// The files below a folder, by their path from it (`/` separated), in their subfolders too.
pub(crate) fn files_below(dir: &Path) -> Vec<String> {
    let mut out = vec![];
    let mut pending = vec![(dir.to_path_buf(), String::new())];
    while let Some((d, prefix)) = pending.pop() {
        for f in std::fs::read_dir(&d).into_iter().flatten().flatten() {
            let name = format!("{prefix}{}", f.file_name().to_string_lossy());
            if f.path().is_dir() {
                pending.push((f.path(), format!("{name}/")));
            } else {
                out.push(name);
            }
        }
    }

    out.sort();
    out
}

/// Writes `bytes` to `path` unless it holds them already. Whether it wrote.
fn put(path: &Path, bytes: &[u8]) -> Result<bool, String> {
    if std::fs::read(path).is_ok_and(|old| old == bytes) {
        return Ok(false);
    }

    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    }

    wce_foundation::files::write_replacing(path, bytes).map_err(|e| format!("{}: {e}", path.display()))?;
    Ok(true)
}

impl TileProject {
    /// Writes a variation of a combined project as a project of its own under `raw_out` (a `raw_data` folder of the kit's layout, its
    /// entry in `_tile_database/TILES` there). `text` is the combined scene as the export takes it (the editor's, prefab references with
    /// their contents). The height map keeps its row's range while the composite fits it (an untouched variation exports with the numbers
    /// it was combined with), else gets the composite's ([`HeightField::fit_range`]); the entry there has it. The entry's other rows keep
    /// what an earlier write there gave them. Files already there that are the same are not written again; a file of an earlier write
    /// that the variation no longer has is removed.
    ///
    /// # Errors
    ///
    /// Not a combined project, no such variation or entry, the scene does not give the variation back, or a file can't be written.
    pub fn materialize(&self, variation: &str, text: &[u8], raw_out: &Path) -> Result<Materialized, String> {
        if !self.variations.iter().any(|v| v == variation) {
            return Err(format!("{} has no variation '{variation}' (it has {})", self.dir.display(), self.variations.join(", ")));
        }

        let entry =
            self.entry.as_ref().ok_or_else(|| format!("{}: a combined project needs its tile database entry", self.dir.display()))?;
        let row = entry.find_variation(variation).ok_or_else(|| format!("{} has no variation '{variation}'", entry.file().display()))?;
        let dir = entry.variation_folder(raw_out, row);
        std::fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
        let mut written = std::collections::BTreeSet::new();

        // The scene.
        let one = super::to_variation(text, variation)?;
        let scene = TedScene::parse(one.clone())?;
        put(&dir.join(WCE_FILE), &mark_wce_text(one))?;
        written.insert(WCE_FILE.to_owned());

        // The files the variations share, then its own over them (with their folders: a decompiled tile's carried terrain and
        // vegetation).
        if let Ok(read) = std::fs::read_dir(&self.dir) {
            for f in read.flatten().filter(|f| f.path().is_file()) {
                let name = f.file_name().to_string_lossy().into_owned();
                if !project_file(&name) {
                    put(&dir.join(&name), &std::fs::read(f.path()).map_err(|e| format!("{}: {e}", f.path().display()))?)?;
                    written.insert(name);
                }
            }
        }

        let own_dir = self.dir.join(OWN_DIR).join(variation);
        for name in files_below(&own_dir) {
            let from = own_dir.join(&name);
            put(&dir.join(&name), &std::fs::read(&from).map_err(|e| format!("{}: {e}", from.display()))?)?;
            written.insert(name);
        }

        // The rivers' files, with the factors of its own (when it keeps the files TEd wrote).
        let campaign = entry.kind() == "campaign";
        for (name, bytes) in crate::river_files::river_files(&scene, Some(own_dir.as_path()), campaign) {
            put(&dir.join(&name), &bytes)?;
            written.insert(name);
        }

        // Its layers, and the maps they compose.
        let row_info = &entry.variations()[row];
        let (mut range, mut refit) = ((row_info.min_height, row_info.scale), false);
        if let Some(layers) = &self.layers {
            let keys = super::layer_key_variations(self.scene.doc(), &self.variations);
            let one = layers.for_variation(variation, &|k| keys.get(k).is_none_or(|l| l.iter().any(|v| v == variation)));
            if let Some(s) = &one.height {
                let (w, h) = s.size();
                let mut field = HeightField::flat(w, h, 0.0);
                field.set_range(range.0, range.1);
                s.apply_all(&mut field);
                if !field.holds_range() {
                    range = field.fit_range();
                    field.set_range(range.0, range.1);
                    refit = true;
                }

                let png = field.to_png(&field.blank_png());
                put(&dir.join(MAPS[0]), &png.write().map_err(|e| e.to_string())?)?;
                written.insert(MAPS[0].to_owned());
            }

            if let Some(s) = &one.blend {
                let (w, h) = s.size();
                let mut field = blank_blend(w, h)?;
                s.apply_all(&mut field);
                put(&dir.join(MAPS[1]), &field.to_bytes())?;
                written.insert(MAPS[1].to_owned());
            }

            if let Some(s) = &one.ground {
                let (w, h) = s.size();
                let mut field = blank_ground(w, h)?;
                s.apply_all(&mut field);
                put(&dir.join(MAPS[2]), &field.to_bytes()?)?;
                written.insert(MAPS[2].to_owned());
            }

            if !one.is_empty() {
                put(&dir.join(crate::terrain_layers::FILE), &one.to_bytes()?)?;
                written.insert(crate::terrain_layers::FILE.to_owned());
            }
        }

        // What an earlier write left that it no longer has, in its folders too.
        for name in files_below(&dir) {
            if !written.contains(&name) {
                let p = dir.join(&name);
                std::fs::remove_file(&p).map_err(|e| format!("{}: {e}", p.display()))?;
            }
        }

        self.materialize_entry(raw_out, row, range)?;
        Ok(Materialized { variation: variation.to_owned(), dir, range, refit })
    }

    /// The tile's entry under `raw_out`, its row `row` with the height range `range`, the others the project's. The tile sets' `_settings.xml` beside it when it has none.
    fn materialize_entry(&self, raw_out: &Path, row: usize, range: (f64, f64)) -> Result<(), String> {
        let entry = self.entry.as_ref().ok_or("no tile database entry")?;
        let database = |raw: &Path| raw.join("terrain/tiles").join(entry.kind()).join("_tile_database");
        let name = entry.file().file_name().ok_or("the entry has no file name")?;
        let target = database(raw_out).join("TILES").join(name);
        // The other rows as the project has them (not an earlier write's): an extracted tile's carried files hold while its entry is the
        // one it was extracted with (`stage_b::fingerprints` hashes the whole entry), and another variation's export changes only its own.
        let mut out = entry.clone();
        out.set_height_range(row, range.0, range.1);
        let wrote = put(&target, &out.save())?;
        if let Some(from) = entry.raw_root().map(|r| database(&r).join("_settings.xml")).filter(|p| p.is_file()) {
            let settings = database(raw_out).join("_settings.xml");
            if !settings.is_file() {
                std::fs::copy(&from, &settings).map_err(|e| format!("{}: {e}", settings.display()))?;
            }
        }

        if wrote {
            wce_formats::tile_index::TileIndex::forget_shared();
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_build_folder_is_beside_the_kit_layout_and_a_change_is_a_new_stamp() {
        let projects = std::path::Path::new("projects");
        assert_eq!(build_root(&projects.join("assembly_kit").join("raw_data")), projects.join("build").join("variations").join("raw_data"));

        let dir = std::env::temp_dir().join(format!("wce_stamp_test_{}", std::process::id()));
        std::fs::create_dir_all(dir.join(OWN_DIR).join("small")).unwrap();
        std::fs::write(dir.join(WCE_FILE), b"<project/>").unwrap();
        let before = stamp_of(&dir, None, "small");
        assert_eq!(stamp_of(&dir, None, "small"), before, "the same files, the same stamp");
        std::fs::write(dir.join(OWN_DIR).join("small").join("nogo.png"), b"x").unwrap();
        let own = stamp_of(&dir, None, "small");
        assert_ne!(own, before, "a file of its own");
        assert_eq!(stamp_of(&dir, None, "minor").lines().count(), before.lines().count(), "not another variation's");
        std::fs::write(dir.join(WCE_FILE), b"<project wce_variations=\"small\"/>").unwrap();
        assert_ne!(stamp_of(&dir, None, "small"), own, "the scene");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn project_files_are_not_shared_files() {
        for name in ["tile.wce", "tile.ted", "terrain_layers.wcl", "nogo.png.bak", "river_spline0.xml", "height_map_0.png", "Blend_Map.tif"]
        {
            assert!(project_file(name), "{name}");
        }

        for name in ["nogo.png", "lf_influence_map.png", "protection_map.dds"] {
            assert!(!project_file(name), "{name}");
        }
    }
}
