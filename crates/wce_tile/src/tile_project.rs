//! One tile opened for editing. Its root is its tile database entry (_tile_database/TILES/*.xml, `tile_entry`): the entry says where the files
//! are (location + variation -> the folder with tile.ted, height_map_0.png, blend_map.tif), how big the tile is, its colour, the ground texture
//! groups, the height and normal map parameters of the variation and the tile links. The project holds the entry, tile.ted (`TedScene`) and the
//! height map; opening a bare folder or tile.ted looks its entry up from the path (indexed: `tile_index`) and warns when there is none. Reads and
//! writes the raw files only.

use std::path::{Path, PathBuf};

use wce_formats::png_file::PngFile;
use wce_formats::tile_entry::TileEntry;
use wce_formats::tile_index::TileIndex;
use wce_foundation::wce_prof;

use crate::height_field::HeightField;
use crate::project_files::{TED_FILE, TedReport, WCE_FILE, mark_wce_text, strip_prefab_contents_with};
use crate::ted_scene::TedScene;
use crate::world_transform::world_transform;

pub type Result<T> = std::result::Result<T, String>;

/// What the tool needs from the entry every frame, copied out of it (the entry itself is `TileProject::entry`).
#[derive(Clone, Debug, PartialEq)]
pub struct TileInfo {
    /// terrain\tiles\battle\<set>\<tile> (TILE@location).
    pub location: String,

    /// Leaf folder, e.g. "medium", "aa".
    pub variation: String,

    /// TILE width / height in cells (128 m each).
    pub cells_x: u32,
    pub cells_y: u32,

    /// VARIATION min_height / scale (metres).
    pub min_height: f64,
    pub scale: f64,

    /// VARIATION values (overlap: cells of the height map's border).
    pub normal_strength: f64,
    pub overlap_border_size: f64,
    pub raw_data_tri_density: u32,

    /// The entry and the variation were found.
    pub found: bool,
}

impl Default for TileInfo {
    fn default() -> Self {
        Self {
            location: String::new(), variation: String::new(), cells_x: 0, cells_y: 0, min_height: 0.0, scale: 100.0, normal_strength: 0.0,
            overlap_border_size: 0.0, raw_data_tri_density: 0, found: false,
        }
    }
}

/// The line `baked = a + b * map` through (map metres with the entry's numbers, baked metres) samples, or `None` when no line fits 60 % of
/// them within 5 cm, with a relief (not only on flat ground), and clearly better than the identity (the entry's own numbers). The baked heights
/// differ from the map where the bake blended neighbours or carved rivers, so this is a robust line (RANSAC over pairs of samples, then a
/// least squares fit of its inliers), not a plain regression. Returns (a, b, inliers).
pub fn fit_baked_heights(pairs: &[(f64, f64)]) -> Option<(f64, f64, usize)> {
    if pairs.len() < 100 {
        return None;
    }

    let near = |a: f64, b: f64, p: &(f64, f64)| (p.1 - (a + b * p.0)).abs() <= 0.05;
    let count = |a: f64, b: f64| pairs.iter().filter(|p| near(a, b, p)).count();
    let mut seed = 0x9E37_79B9_7F4A_7C15u64;
    let mut next = || {
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        seed as usize % pairs.len()
    };

    let mut best = (0.0, 1.0, 0usize);
    for _ in 0..3000 {
        let (p, q) = (pairs[next()], pairs[next()]);
        if (q.0 - p.0).abs() < 1.0 {
            continue;
        }

        let b = (q.1 - p.1) / (q.0 - p.0);
        if !(0.2..5.0).contains(&b) {
            continue;
        }

        let a = p.1 - b * p.0;
        let c = count(a, b);
        if c > best.2 {
            best = (a, b, c);
        }
    }

    let (mut a, mut b) = (best.0, best.1);
    for _ in 0..3 {
        let inliers = pairs.iter().filter(|p| near(a, b, p)).collect::<Vec<_>>();
        let n = inliers.len() as f64;
        if n < 2.0 {
            return None;
        }

        let (mc, mb) = (inliers.iter().map(|p| p.0).sum::<f64>() / n, inliers.iter().map(|p| p.1).sum::<f64>() / n);
        let var = inliers.iter().map(|p| (p.0 - mc) * (p.0 - mc)).sum::<f64>() / n;
        if var < 0.25 {
            return None;
        }

        b = inliers.iter().map(|p| (p.0 - mc) * (p.1 - mb)).sum::<f64>() / n / var;
        a = mb - b * mc;
    }

    let inliers = pairs.iter().filter(|p| near(a, b, p)).collect::<Vec<_>>();
    let n = inliers.len() as f64;
    let mc = inliers.iter().map(|p| p.0).sum::<f64>() / n;
    let spread = (inliers.iter().map(|p| (p.0 - mc) * (p.0 - mc)).sum::<f64>() / n).sqrt();
    if !(0.2..5.0).contains(&b) || inliers.len() * 5 < pairs.len() * 3 || spread < 0.5 || inliers.len() * 2 < count(0.0, 1.0) * 3 {
        return None;
    }

    Some((a, b, inliers.len()))
}

/// Whether a folder holds a tile: the editor's tile.wce or TEd's tile.ted.
pub fn has_tile(dir: &Path) -> bool {
    dir.join(WCE_FILE).exists() || dir.join(TED_FILE).exists()
}

/// The raw_data folder an entry file or a tile path lives under (the part before terrain/tiles), if there is one.
pub fn kit_raw_root(path: &Path) -> Option<PathBuf> {
    let g = path.to_string_lossy().replace('\\', "/");
    let at = g.to_lowercase().find("/terrain/tiles/")?;
    Some(PathBuf::from(&g[..at]))
}

/// The numbers of TileInfo from the entry and one of its variations (`None`: the tile folder itself, no variation numbers).
fn fill_info(e: &TileEntry, variation: Option<usize>, info: &mut TileInfo) {
    info.location = e.location();
    info.cells_x = e.width();
    info.cells_y = e.height();
    let vars = e.variations();
    let Some(v) = variation.and_then(|v| vars.get(v)) else { return };
    info.variation = v.location.clone();
    info.min_height = v.min_height;
    info.scale = v.scale;
    info.normal_strength = v.normal_strength;
    info.overlap_border_size = v.overlap_border_size;
    info.raw_data_tri_density = v.raw_data_tri_density;
    info.found = true;
}

/// The tile database entry of a raw tile folder (<...>/raw_data/terrain/tiles/<battle|campaign>/<set>/.../<variation>), found through the
/// kit's index (no scan per call).
#[derive(Debug, Default)]
pub struct FoundTile {
    pub info: TileInfo,
    pub entry: Option<TileEntry>,
    pub variation: Option<usize>,
    pub raw_root: Option<PathBuf>,
}

pub fn find_tile_info(tile_dir: &Path) -> FoundTile {
    let Some(root) = kit_raw_root(tile_dir) else { return FoundTile::default() };
    let index = TileIndex::shared(&root);
    let r = index.resolve(tile_dir);
    let Some(indexed) = r.entry else { return FoundTile::default() };
    let Ok(entry) = TileEntry::load(&indexed.xml) else { return FoundTile::default() };
    let mut info = TileInfo::default();
    fill_info(&entry, r.variation, &mut info);

    // The folder is not one of the entry's variations: say which tile it is anyway.
    if info.variation.is_empty() {
        let g = tile_dir.to_string_lossy().replace('\\', "/");
        info.variation = g.rsplit('/').next().unwrap_or_default().to_owned();
    }

    FoundTile { info, entry: Some(entry), variation: r.variation, raw_root: Some(root) }
}

#[derive(Debug)]
pub struct TileProject {
    /// The variation folder (holds tile.ted).
    pub dir: PathBuf,
    pub info: TileInfo,

    /// The tile's database entry, `None` when there is none (a tile.ted outside a kit).
    pub entry: Option<TileEntry>,

    /// Index in the entry's variations, `None` when unknown.
    pub variation: Option<usize>,

    /// The kit's raw_data folder, `None` outside a kit.
    pub raw_root: Option<PathBuf>,

    /// What is off about how it opened (no entry, ...), empty when nothing.
    pub warning: String,
    pub scene: TedScene,
    pub height_png: Option<PngFile>,
    pub height: Option<HeightField>,
    pub height_dirty: bool,

    /// Scene units per metre horizontally.
    pub unit_scale: f64,

    /// Which min_height / scale the height map uses when it is not the entry's (see `calibrate_from_final_heights`), empty otherwise.
    pub height_calibration: String,

    /// The rivers' text when the tile was opened or last saved (`river_files`): a save writes the rivers' files only when one changed.
    pub river_texts: Vec<Vec<u8>>,

    /// The prefabs' contents (`PrefabLibrary::contents`): a save keeps what the references' contents differ by from them as their
    /// overrides (`prefab_refs`). Without it, a save drops them.
    pub prefab_base: Option<PrefabSource>,

    /// What the last save found about the references (each one that kept its contents: they differ from its prefab by more than values).
    pub prefab_notes: Vec<String>,
}

/// A key's prefab contents, as a copy holds them (`PrefabLibrary::contents`).
#[derive(Clone)]
pub struct PrefabSource(pub std::sync::Arc<SourceFn>);

type SourceFn = dyn Fn(&str) -> Option<Vec<u8>> + Send + Sync;

impl std::fmt::Debug for PrefabSource {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("PrefabSource")
    }
}

/// What the parallel half of opening reads besides the scene.
struct Side {
    found: Option<FoundTile>,
    height: Option<(PngFile, HeightField)>,
}

impl TileProject {

    /// A variation folder, a tile.ted or a tile folder: the entry is resolved from the path (warning when there is none).
    ///
    /// # Errors
    ///
    /// No tile.ted, or a file that does not read.
    pub fn open(path: &Path) -> Result<Self> {
        let named = |n: &std::ffi::OsStr| n == TED_FILE || n == WCE_FILE;
        let dir = if path.is_file() && path.file_name().is_some_and(named) { path.parent().unwrap_or(path).to_path_buf() } else { path.to_path_buf() };
        if !has_tile(&dir) {
            // A tile folder that holds its variations: the first one (the entry lists them in order).
            if let Some(root) = kit_raw_root(&dir) {
                let index = TileIndex::shared(&root);
                let r = index.resolve(&dir);
                if let Some(indexed) = r.entry
                    && r.variation.is_none() && !indexed.variations.is_empty()
                {
                    let entry = TileEntry::load(&indexed.xml).map_err(|error| error.to_string())?;
                    return Self::open_entry_resolved(entry, 0, root);
                }
            }
        }

        Self::open_dir(dir, None)
    }

    /// The entry file (_tile_database/TILES/x.xml) and one of its variations (`None` = the first).
    ///
    /// # Errors
    ///
    /// The entry does not read, is not inside a kit, or has no such variation.
    pub fn open_entry(entry_xml: &Path, variation: Option<usize>) -> Result<Self> {
        let entry = TileEntry::load(entry_xml).map_err(|error| error.to_string())?;
        let root = entry.raw_root().ok_or_else(|| format!("{} is not inside <kit>/raw_data/terrain/tiles/<kind>/_tile_database/TILES", entry_xml.display()))?;
        Self::open_entry_resolved(entry, variation.unwrap_or(0), root)
    }

    fn open_entry_resolved(entry: TileEntry, v: usize, root: PathBuf) -> Result<Self> {
        let vars = entry.variations();
        if vars.is_empty() {
            return Err(format!("{} lists no variations", entry.file().display()));
        }

        if v >= vars.len() {
            let name = entry.file().file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
            return Err(format!("{name} has {} variations, no number {v}", vars.len()));
        }

        let mut info = TileInfo::default();
        fill_info(&entry, Some(v), &mut info);
        let dir = entry.variation_folder(&root, v);
        let found = FoundTile { info, entry: Some(entry), variation: Some(v), raw_root: Some(root) };
        Self::open_dir(dir, Some(found))
    }

    /// Reads the three parts of a tile side by side: the scene (the long one), the height map (needs the entry's min_height / scale) and, when
    /// the caller did not bring it, the entry.
    fn open_dir(dir: PathBuf, found: Option<FoundTile>) -> Result<Self> {
        wce_prof!("tile.open");

        // The editor's tile.wce, else TEd's tile.ted.
        let ted = if dir.join(WCE_FILE).exists() { dir.join(WCE_FILE) } else { dir.join(TED_FILE) };
        if !ted.exists() {
            return Err(format!("no {WCE_FILE} or {TED_FILE} in {}", dir.display()));
        }

        let (scene, side) = rayon::join(
            || -> Result<TedScene> {
                let bytes = {
                    wce_prof!("tile.open.slurp_ted");
                    std::fs::read(&ted).map_err(|error| format!("{}: {error}", ted.display()))?
                };

                wce_prof!("tile.open.parse_ted");
                TedScene::parse(bytes)
            },
            || -> Result<Side> {
                let found = found.unwrap_or_else(|| {
                    wce_prof!("tile.open.tile_info");
                    find_tile_info(&dir)
                });

                let hm = dir.join("height_map_0.png");
                let height = if hm.exists() {
                    wce_prof!("tile.open.height");
                    let bytes = std::fs::read(&hm).map_err(|error| format!("{}: {error}", hm.display()))?;
                    let png = PngFile::parse(&bytes).map_err(|error| error.to_string())?;
                    let field = HeightField::from_png(&png, found.info.min_height, found.info.scale)?;
                    Some((png, field))
                } else {
                    None
                };

                Ok(Side { found: Some(found), height })
            },
        );

        let scene = scene?;
        let side = side?;
        let found = side.found.unwrap_or_default();
        let (height_png, height) = side.height.map_or((None, None), |(png, field)| (Some(png), Some(field)));
        let warning = match &found.entry {
            None => format!("no tile database entry for {}: the tile opens without its height scale, size, colour and links", dir.display()),
            Some(entry) if !found.info.found => format!(
                "the folder '{}' is not a variation of the tile database entry {}",
                dir.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default(),
                entry.file().file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()
            ),

            Some(_) => String::new(),
        };

        let mut project = Self {
            dir, info: found.info, entry: found.entry, variation: found.variation, raw_root: found.raw_root, warning, scene, height_png, height,
            height_dirty: false, unit_scale: 2.0, height_calibration: String::new(), river_texts: vec![], prefab_base: None, prefab_notes: vec![],
        };

        project.river_texts = project.current_river_texts();

        project.calibrate_from_final_heights();
        if project.height_calibration.is_empty() {
            project.calibrate_from_instances();
        }

        Ok(project)
    }

    /// A tile without a usable `final_heights.dds` (or whose bake does not fit): the landscape instances say what the terrain was when they were
    /// placed. TEd puts a Landscape instance on the terrain at placement, so the heights of the map at their places, with the numbers it had then,
    /// are their y. When most of them (60 %) fit one line of (map metres, y) and the entry's own numbers fit far fewer, the map has other numbers
    /// than the entry (the entry changed after the placing), and those numbers are used, as with the baked heights.
    fn calibrate_from_instances(&mut self) {
        let Some(field) = self.height.as_ref() else { return };
        if !self.bordered() {
            return;
        }

        let sc = &self.scene;
        let els = sc.doc().elements();
        let landscape = |e: usize| {
            matches!(els[e].name.as_str(), "BUILDING_INSTANCE" | "VEGETATION_INSTANCE" | "ProjectedDecalInstance")
                && els[e].children.is_empty()
                && sc.doc().get(e, "surface_mode").is_some_and(|m| m == "MSM_LANDSCAPE")
                && sc.position(e).is_some()
        };

        let all = (0..els.len()).filter(|&e| landscape(e)).collect::<Vec<_>>();
        let stride = (all.len() / 20_000).max(1);
        let pairs = all.iter().step_by(stride).map(|&e| {
            let w = world_transform(sc, e);
            (field.sample(self.pixel_x(w.position.x / self.unit_scale), self.pixel_z(w.position.z / self.unit_scale)), w.position.y)
        }).collect::<Vec<_>>();

        let (min, scale) = (field.min_height(), field.scale());
        let Some((a, b, inliers)) = fit_baked_heights(&pairs) else { return };
        let (new_min, new_scale) = (a + b * min, b * scale);
        if (new_min - min).abs() <= 0.03 * scale.abs() && (new_scale - scale).abs() <= 0.03 * scale.abs() {
            return;
        }

        self.height_calibration = format!(
            "height scale from the placed landscape instances: min {new_min:.3} scale {new_scale:.3} (the entry says min {min:.3} scale {scale:.3}; {inliers} of {} instances sit on it within 5 cm)", pairs.len()
        );
        wce_foundation::log::info(format!("{}: {}", self.dir.display(), self.height_calibration));
        if let Some(field) = self.height.as_mut() {
            field.recalibrate(new_min, new_scale);
        }
    }

    /// BOB's `final_heights.dds` (R32F metres over the tile without its border, one sample per `extent / (n - 1)` m) is what the game draws, and
    /// what the instances were snapped to. In some tiles the entry's min_height / scale are not the numbers the height map was baked with
    /// (dunedain_city_a: entry -12.26 / 24.55, the bake -14 / 17.75: the terrain came out 5 m off and every building floated or sank). When
    /// the samples of the map fit the baked heights with other numbers (most of the samples within centimetres), those numbers are used.
    fn calibrate_from_final_heights(&mut self) {
        let Some(field) = self.height.as_ref() else { return };
        if !self.bordered() {
            return;
        }

        let Ok(bytes) = std::fs::read(self.dir.join("final_heights.dds")) else { return };
        let word = |at: usize| bytes.get(at..at + 4).map_or(0, |b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]));

        // "DDS ", the header size, D3DFMT_R32F as the pixel format's FourCC.
        let (fw, fh) = (word(16) as usize, word(12) as usize);
        if bytes.len() < 128 || &bytes[..4] != b"DDS " || word(4) != 124 || word(84) != 114 || fw < 2 || fh < 2 || bytes.len() < 128 + fw * fh * 4 {
            return;
        }

        let sample = |i: usize, j: usize| f64::from(f32::from_le_bytes([bytes[128 + (j * fw + i) * 4], bytes[129 + (j * fw + i) * 4], bytes[130 + (j * fw + i) * 4], bytes[131 + (j * fw + i) * 4]]));
        let (si, sj) = ((fw / 100).max(1), (fh / 100).max(1));
        let mut pairs = vec![];
        for j in (0..fh).step_by(sj) {
            for i in (0..fw).step_by(si) {
                let (x, z) = (i as f64 * self.extent_x() / (fw - 1) as f64, j as f64 * self.extent_z() / (fh - 1) as f64);
                let baked = sample(i, j);
                if baked.is_finite() {
                    pairs.push((field.sample(self.pixel_x(x), self.pixel_z(z)), baked));
                }
            }
        }

        if pairs.len() < 100 {
            return;
        }

        let (min, scale) = (field.min_height(), field.scale());
        let Some((a, b, inliers)) = fit_baked_heights(&pairs) else { return };
        let (new_min, new_scale) = (a + b * min, b * scale);

        // The bake of every tile comes out about 1.2 % lower than its entry says (the buildings of those tiles follow the entry): not a stale entry.
        if (new_min - min).abs() <= 0.03 * scale.abs() && (new_scale - scale).abs() <= 0.03 * scale.abs() {
            return;
        }

        self.height_calibration = format!("height scale from final_heights.dds: min {new_min:.3} scale {new_scale:.3} (the entry says min {min:.3} scale {scale:.3}; {} of {} samples fit to 0.05 m)", inliers, pairs.len());
        wce_foundation::log::info(format!("{}: {}", self.dir.display(), self.height_calibration));
        if let Some(field) = self.height.as_mut() {
            field.recalibrate(new_min, new_scale);
        }
    }

    /// Writes tile.ted (when the scene is modified, or `force_scene`) and height_map_0.png (when `height_dirty`, or `force_height`); the
    /// message says what was written, `false` if nothing was. A file that is overwritten the first time gets a `.bak` copy.
    ///
    /// # Errors
    ///
    /// A file could not be written.
    /// Every river's text (its element with its points), in document order.
    pub fn current_river_texts(&self) -> Vec<Vec<u8>> {
        crate::river_files::rivers(&self.scene).into_iter().map(|r| self.scene.element_text(r)).collect()
    }

    pub fn save(&mut self, out_dir: &Path, force_scene: bool, force_height: bool) -> Result<(bool, String)> {
        let mut msg = String::new();
        let mut wrote = false;
        let elsewhere = out_dir != self.dir;

        // Into another folder (the first save of a kit's tile into the projects, "Save as"): the tile's other files go with it.
        if elsewhere {
            std::fs::create_dir_all(out_dir).map_err(|error| error.to_string())?;
            let copied = self.copy_other_files(out_dir)?;
            if copied != 0 {
                msg += &format!("{copied} other file(s) copied. ");
            }
        }

        let write = |name: &str, bytes: &[u8]| -> Result<()> {
            let target = out_dir.join(name);
            let bak = out_dir.join(format!("{name}.bak"));
            if target.exists() && !bak.exists() {
                std::fs::copy(&target, &bak).map_err(|error| error.to_string())?;
            }

            std::fs::write(&target, bytes).map_err(|_| format!("could not write {}", target.display()))
        };

        std::fs::create_dir_all(out_dir).map_err(|error| error.to_string())?;
        if self.scene.modified() || force_scene || elsewhere || !out_dir.join(WCE_FILE).exists() {
            let base = self.prefab_base.clone();
            let (text, kept) = strip_prefab_contents_with(self.scene.save(), base.as_ref().map(|b| &*b.0 as crate::project_files::PrefabContents<'_>));
            write(WCE_FILE, &mark_wce_text(text))?;
            msg += &format!("{WCE_FILE} saved. ");
            if !kept.is_empty() {
                msg += &format!("{} prefab reference(s) differ from their prefab by more than values: they keep their own contents ({}). ", kept.len(), kept.join("; "));
            }

            self.prefab_notes = kept;
            wrote = true;
        }

        // The rivers' files (TEd writes them next to tile.ted; BOB reads them): only when a river changed, so that an old river keeps the
        // file TEd left (see `river_files`). The files past the last river become empty: BOB reads up to the first missing one.
        // Saved elsewhere ("Save as"): every river's file, with the factors of the tile's own files.
        let rivers = self.current_river_texts();
        if rivers != self.river_texts || elsewhere {
            let campaign = self.entry.as_ref().is_some_and(|e| e.kind() == "campaign") || self.dir.to_string_lossy().replace('\\', "/").contains("/tiles/campaign/");
            let files = crate::river_files::river_files(&self.scene, Some(&self.dir), campaign);
            for (n, (name, text)) in files.iter().enumerate() {
                let changed = elsewhere || self.river_texts.get(n) != rivers.get(n);
                if changed || !out_dir.join(name).exists() {
                    std::fs::write(out_dir.join(name), text).map_err(|_| format!("could not write {}", out_dir.join(name).display()))?;
                }
            }

            let mut n = files.len();
            while out_dir.join(format!("river_spline{n}.xml")).exists() {
                std::fs::write(out_dir.join(format!("river_spline{n}.xml")), b"<SEGMENTED_SPLINE/>\r\n").map_err(|e| e.to_string())?;
                n += 1;
            }

            msg += &format!("river_spline*.xml saved ({} river{}). ", files.len(), if files.len() == 1 { "" } else { "s" });
            wrote = true;
        }

        if let (Some(field), Some(base)) = (&self.height, &self.height_png)
            && (self.height_dirty || force_height || (elsewhere && !out_dir.join("height_map_0.png").exists()))
        {
            let png = field.to_png(base);
            let bytes = if png.has_ancillary { png.encode() } else { png.write() }.map_err(|error| error.to_string())?;
            write("height_map_0.png", &bytes)?;
            msg += if png.has_ancillary { "height_map_0.png saved (source had extra chunks: re-encoded). " } else { "height_map_0.png saved. " };
            wrote = true;
        }

        Ok((wrote, if wrote { msg } else { "nothing to save".to_owned() }))
    }

    /// Fills in the prefab references' contents from their prefabs (`contents`: a key's group contents; `PrefabLibrary::contents`), as the
    /// editor holds them; the scene is read again (unmodified) only when it has references. Returns the keys no prefab was found for (their
    /// references stay empty, and are kept as they are).
    ///
    /// # Errors
    ///
    /// The filled text does not read.
    pub fn fill_prefab_references(&mut self, contents: &dyn Fn(&str) -> Option<Vec<u8>>) -> Result<Vec<String>> {
        let text = self.scene.save();
        if !text.windows(crate::project_files::PREFAB_ATTR.len()).any(|w| w == crate::project_files::PREFAB_ATTR.as_bytes()) {
            return Ok(vec![]);
        }

        let (filled, missing) = crate::project_files::expand_prefabs(text, contents)?;
        self.scene = TedScene::parse(filled)?;
        self.river_texts = self.current_river_texts();
        if !missing.is_empty() {
            let note = format!("no prefab {} for the references placed in the tile (kept empty)", missing.join(", "));
            self.warning = if self.warning.is_empty() { note } else { format!("{}; {note}", self.warning) };
        }

        Ok(missing)
    }

    /// Makes instance groups references to prefabs (each with its key: the copies `prefab_refs::recognise` found) as the tile opens: the
    /// scene is read again, unmodified.
    ///
    /// # Errors
    ///
    /// The text does not read again.
    pub fn make_references(&mut self, groups: &[(usize, String)]) -> Result<()> {
        if groups.is_empty() {
            return Ok(());
        }

        for (g, key) in groups {
            self.scene.set_attr(*g, crate::project_files::PREFAB_ATTR, key);
        }

        self.scene = TedScene::parse(self.scene.save())?;
        self.river_texts = self.current_river_texts();
        Ok(())
    }

    /// Copies the tile's files that the save does not write (blend map, ground types, the other height maps, ...) into `out_dir`: every
    /// file of the folder but the tile itself, its height map (the save writes it) and the backups, replacing what is there. Returns how many.
    ///
    /// # Errors
    ///
    /// A file can't be copied.
    fn copy_other_files(&self, out_dir: &Path) -> Result<usize> {
        let Ok(read) = std::fs::read_dir(&self.dir) else { return Ok(0) };
        let mut n = 0;
        for entry in read.flatten() {
            let name = entry.file_name().to_string_lossy().into_owned();
            let skip = name == TED_FILE || name == WCE_FILE || name == "height_map_0.png" || name.ends_with(".bak") || !entry.path().is_file();
            if !skip {
                std::fs::copy(entry.path(), out_dir.join(&name)).map_err(|error| format!("{}: {error}", entry.path().display()))?;
                n += 1;
            }
        }

        Ok(n)
    }

    /// "Save for Assembly Kit": the tile as TEd reads it in `kit_dir` (a variation folder of the kit): tile.ted (the conversion's polyfills
    /// applied, what TEd lacks dropped), every river's file, the height map and the tile's other files. A file it replaces gets a `.bak`.
    /// Returns the message and the conversion's report.
    ///
    /// # Errors
    ///
    /// The scene does not convert, or a file can't be written.
    pub fn save_for_kit(&self, kit_dir: &Path) -> Result<(String, TedReport)> {
        self.save_for_kit_scene(kit_dir, self.scene.save(), vec![])
    }

    /// [`Self::save_for_kit`] with the scene given as text (the project's with a feature's polyfill applied that needs more than the
    /// scene, the procedural walls), its polyfills listed in the report with the conversion's.
    ///
    /// # Errors
    ///
    /// As `save_for_kit`.
    pub fn save_for_kit_scene(&self, kit_dir: &Path, scene: Vec<u8>, polyfilled: Vec<String>) -> Result<(String, TedReport)> {
        std::fs::create_dir_all(kit_dir).map_err(|error| format!("{}: {error}", kit_dir.display()))?;
        // A file the kit has the same bytes of is left alone; one it has otherwise keeps a `.bak`.
        let write = |name: &str, bytes: &[u8]| -> Result<bool> {
            let target = kit_dir.join(name);
            match std::fs::read(&target) {
                Ok(old) if old == bytes => return Ok(false),
                Ok(_) => {
                    std::fs::copy(&target, kit_dir.join(format!("{name}.bak"))).map_err(|error| error.to_string())?;
                }

                Err(_) => {}
            }

            std::fs::write(&target, bytes).map_err(|_| format!("could not write {}", target.display()))?;
            Ok(true)
        };

        let (ted, mut report) = crate::project_files::to_ted(scene)?;
        report.polyfilled.splice(0..0, polyfilled);
        write(TED_FILE, &ted)?;

        // Every river's file, with the factors of the project's (`river_files`); the files past the last river become empty.
        let campaign = self.entry.as_ref().is_some_and(|e| e.kind() == "campaign") || self.dir.to_string_lossy().replace('\\', "/").contains("/tiles/campaign/");
        let files = crate::river_files::river_files(&self.scene, Some(&self.dir), campaign);
        for (name, text) in &files {
            write(name, text)?;
        }

        let mut n = files.len();
        while kit_dir.join(format!("river_spline{n}.xml")).exists() {
            write(&format!("river_spline{n}.xml"), b"<SEGMENTED_SPLINE/>\r\n")?;
            n += 1;
        }

        if let (Some(field), Some(base)) = (&self.height, &self.height_png) {
            let png = field.to_png(base);
            write("height_map_0.png", &if png.has_ancillary { png.encode() } else { png.write() }.map_err(|error| error.to_string())?)?;
        }

        // The other files, as the project has them.
        let mut others = 0;
        if let Ok(read) = std::fs::read_dir(&self.dir) {
            for entry in read.flatten() {
                let name = entry.file_name().to_string_lossy().into_owned();
                let ours = name == TED_FILE || name == WCE_FILE || name == "height_map_0.png" || name.starts_with("river_spline") || name.ends_with(".bak");
                if !ours && entry.path().is_file() && write(&name, &std::fs::read(entry.path()).map_err(|error| error.to_string())?)? {
                    others += 1;
                }
            }
        }

        Ok((format!("saved for the Assembly Kit in {} ({TED_FILE}, {} river file(s), {others} other file(s) changed); {}", kit_dir.display(), files.len(), report.summary()), report))
    }

    pub fn has_entry(&self) -> bool {
        self.entry.is_some()
    }

    pub fn has_height(&self) -> bool {
        self.height.is_some()
    }

    pub fn scene_modified(&self) -> bool {
        self.scene.modified()
    }

    /// Tile side in metres along x.
    pub fn extent_m(&self) -> f64 {
        if self.info.found { f64::from(self.info.cells_x) * 128.0 } else { 256.0 }
    }

    pub fn extent_x(&self) -> f64 {
        self.extent_m()
    }

    /// Along z (the height map's rows).
    pub fn extent_z(&self) -> f64 {
        if self.info.found && self.info.cells_y != 0 { f64::from(self.info.cells_y) * 128.0 } else { self.extent_m() }
    }

    /// Where the height map sits. Measured on the shipped tiles (docs/formats/tile_ted.md): the map has a border of one cell (128 m) on every
    /// side, (cells + 2) * ppc pixels per axis, one pixel per 128 / ppc metres, pixel (0, 0) at (-128, -128) m, no flip; instance heights then
    /// sit on the terrain (mean error 0.7 m on western_city_rome, 1.9 m with the map stretched over the tile). Without a tile database entry (or
    /// when the sizes do not fit) the map is stretched over the tile.
    pub fn bordered(&self) -> bool {
        let Some(h) = &self.height else { return false };
        let (cx, cy) = (self.info.cells_x + 2, self.info.cells_y + 2);
        self.info.found && self.info.cells_x != 0 && self.info.cells_y != 0 && h.width() % cx == 0 && h.height() % cy == 0 && h.width() / cx == h.height() / cy
    }

    pub fn border_m(&self) -> f64 {
        if self.bordered() { 128.0 } else { 0.0 }
    }

    pub fn mpp_x(&self) -> f64 {
        match &self.height {
            None => 1.0,
            Some(h) if self.bordered() => 128.0 / f64::from(h.width() / (self.info.cells_x + 2)),
            Some(h) => self.extent_x() / f64::from(h.width() - 1),
        }
    }

    pub fn mpp_z(&self) -> f64 {
        match &self.height {
            None => 1.0,
            Some(h) if self.bordered() => 128.0 / f64::from(h.height() / (self.info.cells_y + 2)),
            Some(h) => self.extent_z() / f64::from(h.height() - 1),
        }
    }

    pub fn pixel_x(&self, x_m: f64) -> f64 {
        (x_m + self.border_m()) / self.mpp_x()
    }

    pub fn pixel_z(&self, z_m: f64) -> f64 {
        (z_m + self.border_m()) / self.mpp_z()
    }

    pub fn height_at(&self, x_m: f64, z_m: f64) -> f64 {
        self.height.as_ref().map_or(0.0, |h| h.sample(self.pixel_x(x_m), self.pixel_z(z_m)))
    }
}

#[cfg(test)]
mod tests {
    use super::fit_baked_heights;

    /// Samples of `baked = -14 + 17.75 / 24.55 * (map - -12.26 ...)`: the map in metres with the entry's numbers, the baked heights with
    /// the numbers the map was baked with, a third of them overwritten (blended neighbours, carved rivers).
    fn samples(entry: (f64, f64), baked: (f64, f64)) -> Vec<(f64, f64)> {
        (0..4000).map(|i| {
            let raw = f64::from((i * 7919) % 65536);
            let on_map = entry.0 + raw / 65535.0 * entry.1;
            let truth = baked.0 + raw / 65535.0 * baked.1;
            (on_map, if i % 3 == 0 { truth + 3.0 + f64::from(i % 17) } else { truth })
        }).collect()
    }

    #[test]
    fn a_stale_entry_is_found_through_the_outliers() {
        let (a, b, inliers) = fit_baked_heights(&samples((-12.26, 24.553), (-14.0, 17.75))).expect("a fit");
        let (min, scale) = (a + b * -12.26, b * 24.553);
        assert!((min - -14.0).abs() < 0.01 && (scale - 17.75).abs() < 0.01, "min {min} scale {scale}");
        assert!(inliers > 2500);
    }

    #[test]
    fn the_entrys_own_numbers_need_no_fit() {
        assert!(fit_baked_heights(&samples((-5.0, 40.0), (-5.0, 40.0))).is_none());
    }

    #[test]
    fn flat_ground_and_noise_fit_nothing() {
        let flat = (0..500).map(|i| (1.0 + f64::from(i % 3) * 0.001, 2.0)).collect::<Vec<_>>();
        assert!(fit_baked_heights(&flat).is_none());
        let noise = (0..2000).map(|i| (f64::from((i * 31) % 97), f64::from((i * 57) % 101) * 0.37)).collect::<Vec<_>>();
        assert!(fit_baked_heights(&noise).is_none());
    }
}
