//! A new battle tile (Tile > New tile...), as TEd's New Tile makes it and its first "Save As Battlefield" writes it (static RE of
//! `TEd.AssemblyKit.exe`, docs/plans/workflow_review.md), written straight away as a project: `<projects>/assembly_kit/raw_data`, the
//! kit's layout, never the Assembly Kit.
//!
//! - The entry `terrain/tiles/battle/_tile_database/TILES/assembly_kit_<name>.xml`: tile set `assembly_kit`, the size, one variation
//!   `battlefield` with the density, TEd's flags and texture groups (`grass`, `mud`, `forest`, `rock`), the colour TEd picks (`colour`) and
//!   the height range of a flat map at the height (`[height, height + 50]`, `HeightField::FLAT_SCALE`).
//! - `terrain/tiles/battle/assembly_kit/<name>/battlefield/`: `tile.wce` (the modules with their seeds and the scene's root, nothing in it),
//!   `height_map_0.png` (every sample 0: the height), `blend_map.tif` (all the first texture group) and `ground_type_map.png` (no painted
//!   ground type: (0, 0, 0, 255)), each `(cells + 2) * density` pixels a side (one cell of border).
//!
//! The tile database (`tile_database.bin`) is not touched: it is compiled from the entries when the tile is exported.

use std::collections::HashSet;
use std::io::Write as _;
use std::path::{Path, PathBuf};

use md5::{Digest as _, Md5};
use wce_formats::image_containers::TiffFile;
use wce_formats::png_file::PngFile;
use wce_formats::tile_entry::TileEntry;

use crate::height_field::HeightField;
use crate::project_files::{WCE_FILE, WCE_VERSION, WCE_VERSION_ATTR};

/// The size limits of TEd's dialog, in cells of 128 m a side.
pub const MIN_CELLS: u32 = 1;
pub const MAX_CELLS: u32 = 64;

/// The dialog's defaults.
pub const DEFAULT_CELLS: u32 = 8;
pub const DEFAULT_HEIGHT: f64 = 10.0;
pub const DEFAULT_DENSITY: u32 = 128;

/// The densities offered (`raw_data_tri_density`: height map pixels per cell).
pub const DENSITIES: [u32; 3] = [32, 64, 128];

/// Above this many cells on a side TEd's dialog lowers the density to 32, unless it was chosen.
pub const LARGE_CELLS: u32 = 16;
pub const LARGE_DENSITY: u32 = 32;

pub const TILE_SET: &str = "assembly_kit";
pub const VARIATION: &str = "battlefield";

/// What the dialog asks for.
#[derive(Clone, Debug, PartialEq)]
pub struct NewTile {
    /// The tile's folder and entry name (lower case, no spaces: `check_name`).
    pub name: String,
    pub cells_x: u32,
    pub cells_y: u32,

    /// Metres: the height of all the terrain.
    pub height: f64,

    /// Height map pixels per cell.
    pub density: u32,

    /// The vegetation and grass seeds (TEd takes them from the clock: `clock_seed`).
    pub seed: u32,
}

impl NewTile {
    /// The height map's (and the other maps') size in pixels: the tile and one cell of border on every side.
    pub fn map_size(&self) -> (u32, u32) {
        ((self.cells_x + 2) * self.density, (self.cells_y + 2) * self.density)
    }

    /// The bytes of `blend_map.tif`, the largest file (16 samples of 16 bits per pixel).
    pub fn blend_map_bytes(&self) -> u64 {
        let (w, h) = self.map_size();
        u64::from(w) * u64::from(h) * 32 + 242
    }

    /// `terrain\tiles\battle\assembly_kit\<name>` (TILE@location).
    pub fn location(&self) -> String {
        format!("terrain\\tiles\\battle\\{TILE_SET}\\{}", self.name)
    }

    /// The variation folder under a raw_data folder.
    pub fn folder(&self, raw: &Path) -> PathBuf {
        raw.join("terrain/tiles/battle").join(TILE_SET).join(&self.name).join(VARIATION)
    }

    /// The entry file under a raw_data folder.
    pub fn entry_file(&self, raw: &Path) -> PathBuf {
        tiles_dir(raw).join(format!("{TILE_SET}_{}.xml", self.name))
    }

    /// Checks the values against the dialog's limits.
    ///
    /// # Errors
    ///
    /// What is out of them.
    pub fn check(&self) -> Result<(), String> {
        check_name(&self.name)?;
        if !(MIN_CELLS..=MAX_CELLS).contains(&self.cells_x) || !(MIN_CELLS..=MAX_CELLS).contains(&self.cells_y) {
            return Err(format!("the size is {} x {} cells: each side goes from {MIN_CELLS} to {MAX_CELLS}", self.cells_x, self.cells_y));
        }

        if !DENSITIES.contains(&self.density) {
            return Err(format!("the density is {}: it is one of {DENSITIES:?}", self.density));
        }

        if !self.height.is_finite() {
            return Err("the terrain height is not a number".to_owned());
        }

        Ok(())
    }
}

/// `<raw>/terrain/tiles/battle/_tile_database/TILES`.
fn tiles_dir(raw: &Path) -> PathBuf {
    raw.join("terrain/tiles/battle/_tile_database/TILES")
}

/// A name the tile can have: lower case (TEd lowers it), not empty, no white space; nor a character a folder name cannot have, since it
/// names the tile's folder.
///
/// # Errors
///
/// Why the name can't be used.
pub fn check_name(name: &str) -> Result<String, String> {
    if name.is_empty() {
        return Err("the tile needs a name".to_owned());
    }

    if name.chars().any(char::is_whitespace) {
        return Err(format!("'{name}' has spaces: a tile's name has none"));
    }

    if let Some(c) = name.chars().find(|c| matches!(c, '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|') || c.is_control()) {
        return Err(format!("'{name}' has a '{c}', which a folder name cannot have"));
    }

    if name.trim_matches('.').is_empty() || name.ends_with('.') {
        return Err(format!("'{name}' cannot be a folder name"));
    }

    Ok(name.to_lowercase())
}

/// The density the dialog shows for a size: TEd lowers it to `LARGE_DENSITY` above `LARGE_CELLS` cells on a side, unless the user chose it.
pub fn suggested_density(cells_x: u32, cells_y: u32, current: u32, chosen: bool) -> u32 {
    if chosen {
        current
    } else if cells_x > LARGE_CELLS || cells_y > LARGE_CELLS {
        LARGE_DENSITY
    } else {
        DEFAULT_DENSITY
    }
}

/// A seed as TEd takes one (`round(timeGetTime() * 0.5)`): from the clock.
pub fn clock_seed() -> u32 {
    let ms = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_millis());
    ((ms % (1 << 32)) as u32 / 2).max(1)
}

/// Where a tile of that name already is, if it is anywhere: a project (its entry or its folder), the kit's raw_data or the kit's
/// `working_data` (TEd's battlefields keep their entries in `_ak_bf_tile_database`).
pub fn existing(name: &str, projects_raw: &Path, kit: Option<&Path>) -> Option<PathBuf> {
    let probe = NewTile { name: name.to_owned(), cells_x: 1, cells_y: 1, height: 0.0, density: DEFAULT_DENSITY, seed: 0 };
    let mut places = vec![probe.entry_file(projects_raw), probe.folder(projects_raw).parent()?.to_path_buf()];
    if let Some(kit) = kit {
        let raw = kit.join("raw_data");
        places.extend([probe.entry_file(&raw), probe.folder(&raw).parent()?.to_path_buf()]);
        places.push(kit.join("working_data/terrain/tiles/battle/_ak_bf_tile_database/TILES").join(format!("{TILE_SET}_{name}.xml")));
    }

    places.into_iter().find(|p| p.exists())
}

/// The colours (0..255 each) of the entries in these `TILES` folders.
pub fn colours_in(tiles_dirs: &[PathBuf]) -> HashSet<[u8; 3]> {
    let mut colours = HashSet::new();
    for dir in tiles_dirs {
        let Ok(read) = std::fs::read_dir(dir) else { continue };
        for file in read.flatten().map(|f| f.path()).filter(|p| p.extension().is_some_and(|x| x.eq_ignore_ascii_case("xml"))) {
            if let Ok(e) = TileEntry::load(&file) {
                colours.insert([e.red(), e.green(), e.blue()].map(|c| c.round().clamp(0.0, 255.0) as u8));
            }
        }
    }

    colours
}

/// The `TILES` folders whose colours a new tile's must differ from: the projects' and the kit's battle tiles (raw_data and TEd's
/// battlefields in working_data).
pub fn colour_sources(projects_raw: &Path, kit: Option<&Path>) -> Vec<PathBuf> {
    let mut dirs = vec![tiles_dir(projects_raw)];
    if let Some(kit) = kit {
        dirs.push(tiles_dir(&kit.join("raw_data")));
        dirs.push(kit.join("working_data/terrain/tiles/battle/_ak_bf_tile_database/TILES"));
    }

    dirs
}

/// TEd's colour for a battlefield (`Save As Battlefield`): the first three bytes of the MD5 of the tile's folder in the kit's working_data
/// (`<kit>/working_data/terrain/tiles/battle/assembly_kit/<name>/battlefield/`, forward slashes, a trailing slash), then one more (as one
/// 24-bit number) while another tile has it. Without a kit, the path starts at `working_data/`.
pub fn colour(name: &str, kit: Option<&Path>, taken: &HashSet<[u8; 3]>) -> [u8; 3] {
    let kit = kit.map(|k| format!("{}/", k.to_string_lossy().replace('\\', "/").trim_end_matches('/'))).unwrap_or_default();
    let path = format!("{kit}working_data/terrain/tiles/battle/{TILE_SET}/{name}/{VARIATION}/");
    let hash = Md5::digest(path.as_bytes());
    let mut c = [hash[0], hash[1], hash[2]];
    for _ in 0..=taken.len() {
        if !taken.contains(&c) {
            break;
        }

        let n = (u32::from(c[0]) << 16 | u32::from(c[1]) << 8 | u32::from(c[2])).wrapping_add(1) & 0xFF_FFFF;
        c = [(n >> 16) as u8, (n >> 8) as u8, n as u8];
    }

    c
}

/// The entry's text, as TEd writes it (single quotes, tabs, CRLF).
pub fn entry_text(t: &NewTile, colour: [u8; 3]) -> String {
    let [r, g, b] = colour.map(f64::from);
    let variation = format!(
        "\t\t<VARIATION serialise_version='2' location='{VARIATION}' min_height='{:.6}' scale='{:.6}' normal_strength='2.000000' overlap_border_size='1.000000' raw_data_tri_density='{}' blend_common='' index_common='' normal_common='' red='0.000000' green='0.000000' blue='0.000000' barbarian='false'/>",
        t.height, crate::height_field::FLAT_SCALE, t.density
    );

    [
        format!(
            "<TILE serialise_version='3' location='{}' tile_set='{TILE_SET}' mask='' width='{}' height='{}' red='{r:.6}' green='{g:.6}' blue='{b:.6}' requires_infield_lodding='true' random_rotatable='true' custom_alpha_blend_texture='' scalable='true' encampable='false' custom_blend_tile=''>",
            t.location(), t.cells_x, t.cells_y
        ),
        "\t<TEXTURE_GROUP red='grass' green='mud' blue='forest' alpha='rock'/>".to_owned(),
        "\t<VARIATIONS>".to_owned(),
        variation,
        "\t</VARIATIONS>".to_owned(),
        "\t<TILE_LINK_TARGETS/>".to_owned(),
        "\t<TILE_LINKS/>".to_owned(),
        "</TILE>".to_owned(),
        String::new(),
    ]
    .join("\r\n")
}

/// `tile.wce` of an empty tile: TEd's modules with their seeds (the three vegetation ones equal, as TEd's clock gives them), the scene's root
/// with an empty `<children>` (as the editor writes a new layer, so that things can go in) and a camera over the tile from its south edge.
pub fn scene_text(t: &NewTile) -> String {
    let grass = t.seed.rotate_left(13) / 2;
    let (cx, cz) = (f64::from(t.cells_x) * 128.0, f64::from(t.cells_y) * 128.0);
    let y = t.height;
    let target = format!("vector({cx},{y},{cz})");
    let position = format!("vector({cx},{},{})", y + cx.max(cz), cz * 2.0 + cx.max(cz) * 0.5);
    format!(
        "<project version=\"17\" {WCE_VERSION_ATTR}=\"{WCE_VERSION}\">
    <modules>
        <Vegetation decal_generation_seed=\"{s}\" generation_seed=\"{s}\" vfx_generation_seed=\"{s}\"/>
        <Grass generation_seed=\"{grass}\"/>
    </modules>
    <scene>
        <SCENE_GRAPH flags=\" OIF_VISIBLE|OIF_ACTIVE\" export_type=\"LayerExportType_Normal\" lf_offset=\"0\" position=\"vector(0,0,0)\" name=\"root\" surface_mode=\"MSM_FREEFORM\" orientation=\"euler(0,0,0)\">
            <children>
            </children>
        </SCENE_GRAPH>
    </scene>
    <camera vertical_fov=\"1.047198\" orthographic=\"false\" orthographic_height=\"1\" far_distance=\"12000\" smoothed=\"true\" target=\"{target}\" ground_height=\"1.4\" DOF_blur_size=\"0\" terrain_collision=\"false\" head_bob=\"false\" aspect_ratio=\"1.777778\" focal_length=\"0\" position=\"{position}\" DOF_focus=\"variance(0,40)\" movement_mode=\"CM_LOOK\" roll=\"0\" near_distance=\"1\"/>
</project>
",
        s = t.seed
    )
}

/// Writes the new tile into the projects' raw_data (`projects_raw`) and returns its variation folder. `kit` (the Assembly Kit's folder)
/// gives the colour and the tile database's `_settings.xml` (copied when the projects have none). Nothing is written when the tile exists.
///
/// # Errors
///
/// The values are out of the dialog's limits, the tile exists, or a file can't be written.
pub fn create(t: &NewTile, projects_raw: &Path, kit: Option<&Path>) -> Result<PathBuf, String> {
    t.check()?;
    if let Some(there) = existing(&t.name, projects_raw, kit) {
        return Err(format!("a tile named '{}' exists: {}", t.name, there.display()));
    }

    let dir = t.folder(projects_raw);
    let entry = t.entry_file(projects_raw);
    let mkdir = |d: &Path| std::fs::create_dir_all(d).map_err(|e| format!("{}: {e}", d.display()));
    let write = |p: &Path, bytes: &[u8]| std::fs::write(p, bytes).map_err(|e| format!("{}: {e}", p.display()));
    mkdir(&dir)?;
    mkdir(entry.parent().expect("TILES"))?;

    let colour = colour(&t.name, kit, &colours_in(&colour_sources(projects_raw, kit)));
    write(&entry, entry_text(t, colour).as_bytes())?;

    // The tile database's settings, beside the entries (the vegetation generator reads the links' tile sets there).
    let settings = projects_raw.join("terrain/tiles/battle/_tile_database/_settings.xml");
    if let Some(kit_settings) = kit.map(|k| k.join("raw_data/terrain/tiles/battle/_tile_database/_settings.xml")).filter(|s| !settings.exists() && s.is_file()) {
        std::fs::copy(&kit_settings, &settings).map_err(|e| format!("{}: {e}", kit_settings.display()))?;
    }

    write(&dir.join(WCE_FILE), scene_text(t).as_bytes())?;
    let (w, h) = t.map_size();
    let field = HeightField::flat(w, h, t.height);
    let png = field.to_png(&field.blank_png()).encode().map_err(|e| e.to_string())?;
    write(&dir.join("height_map_0.png"), &png)?;
    write_blend_map(&dir.join("blend_map.tif"), w, h)?;

    let ground = PngFile { width: w, height: h, bit_depth: 8, color_type: 6, pixels: [0, 0, 0, 255].repeat(w as usize * h as usize), pixels_edited: true, ..Default::default() };
    write(&dir.join("ground_type_map.png"), &ground.encode().map_err(|e| e.to_string())?)?;

    // The editor finds tiles through the kits' indices, which have another entry now.
    wce_formats::tile_index::TileIndex::forget_shared();
    Ok(dir)
}

/// `blend_map.tif` all of the first texture group (sample 0 = 65535, the other 15 = 0), in TEd's layout, a row at a time (a 64 x 64 tile at
/// density 128 has a 2.3 GB one).
fn write_blend_map(path: &Path, w: u32, h: u32) -> Result<(), String> {
    let fail = |e: std::io::Error| format!("{}: {e}", path.display());
    let file = std::fs::File::create(path).map_err(fail)?;
    let mut out = std::io::BufWriter::with_capacity(1 << 20, file);
    out.write_all(&TiffFile::ted_header(w, h)).map_err(fail)?;
    let mut pixel = [0u8; 32];
    pixel[..2].copy_from_slice(&u16::MAX.to_le_bytes());
    let row = pixel.repeat(w as usize);
    for _ in 0..h {
        out.write_all(&row).map_err(fail)?;
    }

    out.flush().map_err(fail)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tile(name: &str) -> NewTile {
        NewTile { name: name.to_owned(), cells_x: 2, cells_y: 1, height: 10.0, density: 32, seed: 263_473_888 }
    }

    #[test]
    fn names_follow_teds_rules() {
        assert_eq!(check_name("Helms_Deep").unwrap(), "helms_deep");
        assert!(check_name("").is_err());
        assert!(check_name("helms deep").is_err());
        assert!(check_name("a/b").is_err() && check_name("..").is_err());
    }

    #[test]
    fn the_density_drops_for_large_tiles_unless_chosen() {
        assert_eq!(suggested_density(16, 16, 128, false), 128);
        assert_eq!(suggested_density(17, 4, 128, false), 32);
        assert_eq!(suggested_density(17, 4, 64, true), 64);
    }

    #[test]
    fn the_colour_is_teds_md5_of_the_working_data_folder() {
        // Measured on the kit's own battlefields: assembly_kit_tesd.xml has 31 107 226.
        let kit = Path::new("D:/Juegos/Steam/steamapps/common/Total War Attila/assembly_kit");
        assert_eq!(colour("tesd", Some(kit), &HashSet::new()), [31, 107, 226]);
        assert_eq!(colour("hornburg", Some(Path::new("D:\\Juegos\\Steam\\steamapps\\common\\Total War Attila\\assembly_kit\\")), &HashSet::new()), [215, 63, 116]);
        let taken = HashSet::from([[31, 107, 226], [31, 107, 227]]);
        assert_eq!(colour("tesd", Some(kit), &taken), [31, 107, 228]);
    }

    #[test]
    fn a_new_tile_is_a_project_that_opens_flat_at_its_height() {
        let tmp = tempfile::tempdir().unwrap();
        let raw = tmp.path().join("assembly_kit/raw_data");
        let t = tile("test_new");
        let dir = create(&t, &raw, None).unwrap();
        assert_eq!(dir, raw.join("terrain/tiles/battle/assembly_kit/test_new/battlefield"));
        assert!(create(&t, &raw, None).unwrap_err().contains("exists"), "a second one is refused");

        let entry = std::fs::read_to_string(t.entry_file(&raw)).unwrap();
        assert!(entry.starts_with("<TILE serialise_version='3' location='terrain\\tiles\\battle\\assembly_kit\\test_new' tile_set='assembly_kit' mask='' width='2' height='1' "), "{entry}");
        assert!(entry.contains("min_height='10.000000' scale='50.000000'") && entry.contains("raw_data_tri_density='32'") && entry.ends_with("</TILE>\r\n"), "{entry}");

        let blend = TiffFile::parse(&std::fs::read(dir.join("blend_map.tif")).unwrap()).unwrap();
        assert!(blend.header_is_regenerable() && (blend.width, blend.height) == (128, 96));
        assert_eq!((&blend.pixels[..4], &blend.pixels[32..36]), (&[255, 255, 0, 0][..], &[255, 255, 0, 0][..]));

        let ground = PngFile::parse(&std::fs::read(dir.join("ground_type_map.png")).unwrap()).unwrap();
        assert_eq!((ground.width, ground.height, ground.color_type, &ground.pixels[..4]), (128, 96, 6, &[0, 0, 0, 255][..]));

        let p = crate::tile_project::TileProject::open(&dir).unwrap();
        assert!(p.warning.is_empty() && p.info.found, "{}", p.warning);
        assert_eq!((p.info.cells_x, p.info.cells_y, p.info.raw_data_tri_density), (2, 1, 32));
        assert!(p.bordered() && (p.height_at(100.0, 50.0) - 10.0).abs() < 1e-9);
        assert!(p.scene.save().starts_with(b"<project version=\"17\" wce_version=\"1\">"));
    }

    #[test]
    fn saving_fits_the_height_range_and_the_entry_follows() {
        let tmp = tempfile::tempdir().unwrap();
        let raw = tmp.path().join("assembly_kit/raw_data");
        let t = tile("test_range");
        let dir = create(&t, &raw, None).unwrap();
        let mut p = crate::tile_project::TileProject::open(&dir).unwrap();

        // Below the flat map's range and far above it: the range follows.
        let hf = p.height.as_mut().unwrap();
        hf.begin_edit().unwrap();
        let b = crate::height_field::Brush { cx: 40.0, cy: 40.0, radius: 6.0, strength: 1.0, falloff: 0.0 };
        hf.raise(&b, 120.0).unwrap();
        hf.raise(&crate::height_field::Brush { cx: 90.0, ..b }, -15.0).unwrap();
        hf.end_edit();
        p.height_dirty = true;
        let (_, msg) = p.save(&dir.clone(), false, false).unwrap();
        assert!(msg.contains("height range -5.000 .. 130.000 m") && msg.contains("assembly_kit_test_range.xml saved"), "{msg}");

        let entry = std::fs::read_to_string(t.entry_file(&raw)).unwrap();
        assert!(entry.contains("min_height='-5.000000' scale='135.000000'"), "{entry}");

        let back = crate::tile_project::TileProject::open(&dir).unwrap();
        assert!((back.height_at(-128.0 + 40.0 * 4.0, -128.0 + 40.0 * 4.0) - 130.0).abs() < 0.01);
        assert!((back.height_at(-128.0 + 90.0 * 4.0, -128.0 + 40.0 * 4.0) - -5.0).abs() < 0.01);
        assert!((back.height_at(200.0, 100.0) - 10.0).abs() < 0.01, "{}", back.height_at(200.0, 100.0));
    }
}
