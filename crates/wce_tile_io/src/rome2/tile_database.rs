//! Rome 2's tile database: XML in the packs, not compiled. `terrain/tiles/<kind>/_tile_database/_settings.xml` has the render and
//! conversion parameters, the named texture groups (`GROUP name='group14' red='sand' ...`) and the climates; every other XML of the
//! folder holds tile sets (`TILE_SET` > `TILE` > `NEW_MASK`, `VARIATION`, `LINK_TARGET`, `LINK`). An attribute at its default is left out
//! (docs/formats/rome2_tiles.md).
//!
//! [`Rome2TileDatabase::read`] turns them into rpfm's [`TileDatabase`], the form the decompiler and the editor read (Attila's, compiled
//! by BOB from the kit's XML), and keeps the files: [`Rome2TileDatabase::with_tile`] writes a tile back into its set's file, every other
//! byte of it as it was.

use std::collections::BTreeMap;

use rpfm_lib::files::tile_database::TileDatabase;
use rpfm_lib::files::tile_database::climate::Climate;
use rpfm_lib::files::tile_database::conversion_params::ConversionParams;
use rpfm_lib::files::tile_database::render_params::RenderParams;
use rpfm_lib::files::tile_database::texture::Texture;
use rpfm_lib::files::tile_database::tile::Tile;
use rpfm_lib::files::tile_database::tile_link::TileLink;
use rpfm_lib::files::tile_database::tile_link_target::TileLinkTarget;
use rpfm_lib::files::tile_database::tile_set::TileSet;
use rpfm_lib::files::tile_database::tile_variation::TileVariation;
use wce_formats::xml_doc::XmlDoc;
use wce_vfs::files::Files;

/// The settings file of a database folder.
pub const SETTINGS_FILE: &str = "_settings.xml";

/// A texture group of the settings: its name and the textures of its red, green, blue and alpha channels.
pub type TextureGroup = (String, [String; 4]);

/// A tile database of Rome 2 and the files it was read from.
#[derive(Clone, Debug, Default)]
pub struct Rome2TileDatabase {
    /// The database as Attila's (rpfm's): the tile sets and tiles in the files' order.
    pub database: TileDatabase,

    /// The settings' texture groups, in their order.
    pub texture_groups: Vec<TextureGroup>,

    /// The files by name (`_settings.xml`, `river.xml` ...).
    pub files: BTreeMap<String, Vec<u8>>,

    /// The file each tile set is in, by the set's name (lower case).
    set_files: BTreeMap<String, String>,
}

/// An element's attribute, if it has it.
fn attr(doc: &XmlDoc, e: usize, name: &str) -> Option<String> {
    doc.get(e, name)
}

fn f32_or(doc: &XmlDoc, e: usize, name: &str, default: f32) -> f32 {
    attr(doc, e, name).and_then(|v| v.trim().parse().ok()).unwrap_or(default)
}

fn i32_or(doc: &XmlDoc, e: usize, name: &str, default: i32) -> i32 {
    attr(doc, e, name).and_then(|v| v.trim().parse().ok()).unwrap_or(default)
}

fn bool_or(doc: &XmlDoc, e: usize, name: &str, default: bool) -> bool {
    attr(doc, e, name).map_or(default, |v| v == "true")
}

fn string(doc: &XmlDoc, e: usize, name: &str) -> String {
    attr(doc, e, name).unwrap_or_default()
}

/// The children of an element named `name`.
fn children<'a>(doc: &'a XmlDoc, e: usize, name: &'a str) -> impl Iterator<Item = usize> + 'a {
    doc.elements()[e].children.iter().copied().filter(move |&c| doc.elements()[c].name == name)
}

/// The first element named `name` at the top of a document, or below its root.
fn top(doc: &XmlDoc, name: &str) -> Option<usize> {
    doc.roots().iter().copied().find(|&r| doc.elements()[r].name == name)
}

/// Attila's name of a link test (`equals` -> `TLT_EQUALS`).
fn link_test(test: &str) -> String {
    format!("TLT_{}", test.to_ascii_uppercase())
}

/// Rome 2's name of a link test (`TLT_NOT_EQUALS` -> `not_equals`).
fn rome2_link_test(test: &str) -> String {
    test.strip_prefix("TLT_").unwrap_or(test).to_ascii_lowercase()
}

impl Rome2TileDatabase {

    /// Reads a database folder's files (by file name: `_settings.xml` and the tile set files; the tile sets come in file name order).
    ///
    /// # Errors
    ///
    /// The settings are missing, or a file is not XML.
    pub fn read(files: BTreeMap<String, Vec<u8>>) -> Result<Self, String> {
        let settings = files.get(SETTINGS_FILE).ok_or("the tile database has no _settings.xml")?;
        let doc = XmlDoc::parse(settings.clone()).map_err(|e| format!("{SETTINGS_FILE}: {e}"))?;
        let root = top(&doc, "TILE_DATABASE").ok_or(format!("{SETTINGS_FILE}: no <TILE_DATABASE>"))?;
        let mut out = Self { files: BTreeMap::new(), ..Self::default() };
        let db = &mut out.database;
        db.set_serialise_version(1);
        if let Some(e) = children(&doc, root, "RENDER_PARAMS").next() {
            db.set_render_params(render_params(&doc, e));
        }

        if let Some(e) = children(&doc, root, "CONVERSION_PARAMS").next() {
            db.set_conversion_params(conversion_params(&doc, e));
        }

        if let Some(groups) = children(&doc, root, "TEXTURE_GROUPS").next() {
            for g in children(&doc, groups, "GROUP") {
                out.texture_groups.push((string(&doc, g, "name"), ["red", "green", "blue", "alpha"].map(|c| string(&doc, g, c))));
            }
        }

        if let Some(climates) = children(&doc, root, "CLIMATES").next() {
            db.set_climates(children(&doc, climates, "CLIMATE").map(|c| climate(&doc, c)).collect());
        }

        for (name, bytes) in &files {
            if name == SETTINGS_FILE {
                continue;
            }

            let doc = XmlDoc::parse(bytes.clone()).map_err(|e| format!("{name}: {e}"))?;
            let Some(root) = top(&doc, "TILE_DATABASE") else { continue };
            for set in children(&doc, root, "TILE_SET") {
                let set_name = string(&doc, set, "name");
                out.set_files.insert(set_name.to_lowercase(), name.clone());
                db.tile_sets_mut().push(tile_set(&doc, set));
                for t in children(&doc, set, "TILE") {
                    db.tiles_mut().push(tile(&doc, t, &set_name));
                }
            }
        }

        out.files = files;
        Ok(out)
    }

    /// The folder of a kind's database (`battle`, `campaign`) in the packs.
    pub fn folder(kind: &str) -> String {
        format!("terrain/tiles/{kind}/_tile_database/")
    }

    /// The database of a kind (`battle`, `campaign`) in the game's files: the XML files of its folder (not of folders below it).
    ///
    /// # Errors
    ///
    /// The folder has no settings, or a file can't be read or isn't XML.
    pub fn from_files(files: &dyn Files, kind: &str) -> Result<Self, String> {
        let folder = Self::folder(kind);
        let mut found = BTreeMap::new();
        for path in files.list(&folder, ".xml") {
            let name = &path[folder.len()..];
            if !name.contains('/') {
                found.insert(name.to_owned(), files.read(&path)?);
            }
        }

        Self::read(found).map_err(|e| format!("{folder}: {e}"))
    }

    /// The file a tile set is in, if a file has it.
    pub fn set_file(&self, set: &str) -> Option<&str> {
        self.set_files.get(&set.to_lowercase()).map(String::as_str)
    }

    /// The name of the texture group with these channels (red, green, blue, alpha), if the settings have one.
    pub fn texture_group(&self, channels: &[String; 4]) -> Option<&str> {
        self.texture_groups.iter().find(|(_, c)| c.iter().zip(channels).all(|(a, b)| a.eq_ignore_ascii_case(b))).map(|(n, _)| n.as_str())
    }

    /// The files to write for `tile` to take the place of the tile of the same location (or join its set, after its last tile; a set no
    /// file has gets a file of its own, `<set>.xml`): its set's file, and `_settings.xml` when the tile's channels need a texture group
    /// the settings lack (it gets the next free `group<n>`). Every other byte of the files is as it was.
    ///
    /// # Errors
    ///
    /// A file doesn't read as XML.
    pub fn with_tile(&self, tile: &Tile) -> Result<Vec<(String, Vec<u8>)>, String> {
        let mut out = vec![];
        let channels = [tile.texture_red(), tile.texture_green(), tile.texture_blue(), tile.texture_alpha()].map(|s| s.to_owned());
        let group = match self.texture_group(&channels) {
            Some(g) => g.to_owned(),
            None => {
                let mut n = self.texture_groups.len();
                while self.texture_groups.iter().any(|(g, _)| *g == format!("group{n}")) {
                    n += 1;
                }

                let name = format!("group{n}");
                let settings = self.files.get(SETTINGS_FILE).ok_or("the tile database has no _settings.xml")?;
                out.push((SETTINGS_FILE.to_owned(), with_texture_group(settings, &name, &channels)?));
                name
            }
        };

        let element = tile_xml(tile, &group);
        match self.set_file(tile.tile_set()) {
            Some(file) => {
                let bytes = &self.files[file];
                out.push((file.to_owned(), with_tile_element(bytes, tile.tile_set(), tile.location(), &element)?));
            }

            None => {
                let set = self.database.tile_sets().iter().find(|s| s.name().eq_ignore_ascii_case(tile.tile_set()));
                let set = set.map_or_else(|| format!("\t<TILE_SET name='{}' red='0' green='0' blue='0'>\r\n", tile.tile_set()), tile_set_open);
                let text = format!("<TILE_DATABASE>\r\n{set}{element}\t</TILE_SET>\r\n</TILE_DATABASE>\r\n");
                out.push((format!("{}.xml", tile.tile_set().to_lowercase()), text.into_bytes()));
            }
        }

        Ok(out)
    }
}

fn render_params(doc: &XmlDoc, e: usize) -> RenderParams {
    let mut p = RenderParams::default();
    p.set_serialise_version(6);
    p.set_lf_height(f32_or(doc, e, "lf_height", 0.0));
    p.set_hf_height(f32_or(doc, e, "hf_height", 0.0));
    p.set_blend_pixel_scale(f32_or(doc, e, "blend_pixel_scale", 0.0));
    p.set_unit_scale(f32_or(doc, e, "unit_scale", 0.0));
    p.set_mid_distance_detail_scale(f32_or(doc, e, "mid_distance_detail_scale", 0.0));
    p.set_mid_distance_detail_strength(f32_or(doc, e, "mid_distance_detail_strength", 0.0));
    p.set_mid_distance_normal_strength(f32_or(doc, e, "mid_distance_normal_strength", 0.0));
    p.set_mid_distance_detail_near(f32_or(doc, e, "mid_distance_detail_near", 0.0));
    p.set_mid_distance_detail_far(f32_or(doc, e, "mid_distance_detail_far", 0.0));
    p.set_mid_distance_detail_slope_low(f32_or(doc, e, "mid_distance_detail_slope_low", 0.0));
    p.set_mid_distance_detail_slope_high(f32_or(doc, e, "mid_distance_detail_slope_high", 0.0));
    p.set_vertical_offset(f32_or(doc, e, "vertical_offset", 0.0));
    p.set_normal_lf_scale(f32_or(doc, e, "normal_lf_scale", 0.0));
    p.set_normal_tile_scale(f32_or(doc, e, "normal_tile_scale", 0.0));
    p.set_normal_terrain_scale(f32_or(doc, e, "normal_terrain_scale", 0.0));
    p
}

fn conversion_params(doc: &XmlDoc, e: usize) -> ConversionParams {
    let mut p = ConversionParams::default();
    p.set_serialise_version(1);
    p.set_triangle_density(i32_or(doc, e, "triangle_density", 128));
    p.set_max_lf_heights_per_pixel(i32_or(doc, e, "max_lf_heights_per_pixel", 128));
    let size = |k: usize| f32_or(doc, e, &format!("triangle_decimation_size_factor{k}"), 0.0);
    let angle = |k: usize| f32_or(doc, e, &format!("triangle_decimation_angle_factor{k}"), 0.0);
    p.set_triangle_decimation_size_factors0(size(0));
    p.set_triangle_decimation_size_factors1(size(1));
    p.set_triangle_decimation_size_factors2(size(2));
    p.set_triangle_decimation_size_factors3(size(3));
    p.set_triangle_decimation_size_factors4(size(4));
    p.set_triangle_decimation_size_factors5(size(5));
    p.set_triangle_decimation_angle_factors0(angle(0));
    p.set_triangle_decimation_angle_factors1(angle(1));
    p.set_triangle_decimation_angle_factors2(angle(2));
    p.set_triangle_decimation_angle_factors3(angle(3));
    p.set_triangle_decimation_angle_factors4(angle(4));
    p.set_triangle_decimation_angle_factors5(angle(5));
    p
}

/// A climate. Its textures list only what differs from the defaults (the defaults are the `default` climate's in every shipped file:
/// reflectivity 0.009, smoothness 0.1; the repeat 40.1 m, Attila's reader's default too); Rome 2's `const_smoothness` is Attila's
/// `lerp_smoothness`. Rome 2 has no outfield repeat: Attila's default (1200.1 m).
fn climate(doc: &XmlDoc, e: usize) -> Climate {
    let mut c = Climate::default();
    c.set_serialise_version(3);
    c.set_name(string(doc, e, "name"));
    c.set_texture_set(string(doc, e, "texture_set"));
    c.set_red(f32_or(doc, e, "red", 0.0));
    c.set_green(f32_or(doc, e, "green", 0.0));
    c.set_blue(f32_or(doc, e, "blue", 0.0));
    let textures = children(doc, e, "TEXTURE")
        .map(|t| {
            let mut x = Texture::default();
            x.set_serialise_version(4);
            x.set_name(string(doc, t, "name"));
            x.set_mid_distance_strength(1.0);
            x.set_lerp_reflectivity0(f32_or(doc, t, "lerp_reflectivity0", 0.009));
            x.set_lerp_reflectivity1(f32_or(doc, t, "lerp_reflectivity1", 0.009));
            x.set_lerp_reflectivity2(f32_or(doc, t, "lerp_reflectivity2", 0.009));
            x.set_lerp_reflectivity3(f32_or(doc, t, "lerp_reflectivity3", 0.009));
            x.set_lerp_smoothness0(f32_or(doc, t, "const_smoothness0", 0.1));
            x.set_lerp_smoothness1(f32_or(doc, t, "const_smoothness1", 0.1));
            x.set_lerp_smoothness2(f32_or(doc, t, "const_smoothness2", 0.1));
            x.set_lerp_smoothness3(f32_or(doc, t, "const_smoothness3", 0.1));
            x.set_blend_pixel_scale(f32_or(doc, t, "blend_pixel_scale", 40.099_998));
            x.set_outfield_blend_pixel_scale(1200.1);
            x
        })
        .collect();
    c.set_textures(textures);
    c
}

fn tile_set(doc: &XmlDoc, e: usize) -> TileSet {
    let mut s = TileSet::default();
    s.set_serialise_version(1);
    s.set_name(string(doc, e, "name"));
    s.set_linking_tile(string(doc, e, "linking_tile"));
    s.set_shared_geometry(string(doc, e, "shared_geometry"));
    s.set_also_place_tile_set(string(doc, e, "also_place_tile_set"));
    s.set_link_as_set(string(doc, e, "link_as_set"));
    s.set_red(f32_or(doc, e, "red", 0.0));
    s.set_green(f32_or(doc, e, "green", 0.0));
    s.set_blue(f32_or(doc, e, "blue", 0.0));
    s
}

fn tile(doc: &XmlDoc, e: usize, set: &str) -> Tile {
    let mut t = Tile::default();
    t.set_serialise_version(3);
    t.set_location(string(doc, e, "location"));
    t.set_tile_set(set.to_owned());
    if bool_or(doc, e, "masked", false) {
        t.set_mask(children(doc, e, "NEW_MASK").next().map(|m| string(doc, m, "data")).unwrap_or_default());
    }

    t.set_width(i32_or(doc, e, "width", 1).max(0) as u32);
    t.set_height(i32_or(doc, e, "height", 1).max(0) as u32);
    t.set_red(f32_or(doc, e, "red", 0.0));
    t.set_green(f32_or(doc, e, "green", 0.0));
    t.set_blue(f32_or(doc, e, "blue", 0.0));
    t.set_requires_infield_lodding(bool_or(doc, e, "requires_infield_lodding", false));
    t.set_random_rotatable(bool_or(doc, e, "random_rotatable", true));
    t.set_custom_alpha_blend_texture(string(doc, e, "custom_alpha_blend_texture"));
    t.set_encampable(bool_or(doc, e, "encampable", false));
    t.set_texture_red(string(doc, e, "red_channel"));
    t.set_texture_green(string(doc, e, "green_channel"));
    t.set_texture_blue(string(doc, e, "blue_channel"));
    t.set_texture_alpha(string(doc, e, "alpha_channel"));
    t.set_variations(children(doc, e, "VARIATION").map(|v| variation(doc, v)).collect());
    t.set_link_targets(
        children(doc, e, "LINK_TARGET")
            .map(|l| {
                let mut x = TileLinkTarget::default();
                x.set_serialise_version(1);
                x.set_tile_set(string(doc, l, "target_set"));
                x.set_x(i32_or(doc, l, "x", 0));
                x.set_y(i32_or(doc, l, "y", 0));
                x
            })
            .collect(),
    );
    t.set_links(
        children(doc, e, "LINK")
            .map(|l| {
                let mut x = TileLink::default();
                x.set_serialise_version(1);
                x.set_link_set(string(doc, l, "name"));
                x.set_x(i32_or(doc, l, "x", 0));
                x.set_y(i32_or(doc, l, "y", 0));
                x.set_base_x(i32_or(doc, l, "base_x", 0));
                x.set_base_y(i32_or(doc, l, "base_y", 0));
                x.set_is_entry(bool_or(doc, l, "is_entry", false));
                x.set_blend_size(i32_or(doc, l, "blend_size", 64).max(0) as u32);
                x.set_no_offline_blend(bool_or(doc, l, "no_offline_blend", false));
                x.set_test(link_test(&string(doc, l, "test")));
                x
            })
            .collect(),
    );
    t
}

fn variation(doc: &XmlDoc, e: usize) -> TileVariation {
    let mut v = TileVariation::default();
    v.set_serialise_version(2);
    v.set_location(string(doc, e, "name"));
    v.set_min_height(f32_or(doc, e, "min_height", 0.0));
    v.set_scale(f32_or(doc, e, "scale", 1.0));
    v.set_normal_strength(f32_or(doc, e, "normal_strength", 1.0));
    v.set_overlap_border_size(f32_or(doc, e, "overlap_border_size", 1.0));
    v.set_raw_data_tri_density(i32_or(doc, e, "raw_data_tri_density", 128).max(0) as u32);
    v.set_blend_common(string(doc, e, "blend_common"));
    v.set_index_common(string(doc, e, "index_common"));
    v.set_normal_common(string(doc, e, "normal_common"));
    v.set_red(f32_or(doc, e, "red", 0.0));
    v.set_green(f32_or(doc, e, "green", 0.0));
    v.set_blue(f32_or(doc, e, "blue", 0.0));
    v
}

/// An attribute value as the files write it: `'` is the quote, so it and the markup characters are escaped.
fn esc(v: &str) -> String {
    v.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('\'', "&apos;")
}

/// A number as the files write it: `%.6f` of the float, as MSVC's printf rounds (half away from zero).
fn fixed6(v: f32) -> String {
    wce_formats::c_num::printf_f_msvc(f64::from(v), 6)
}

/// A colour component as the files write it (an integer).
fn colour(v: f32) -> String {
    format!("{}", v.round() as i64)
}

/// A tile set's start tag, with its line's indent.
fn tile_set_open(s: &TileSet) -> String {
    let mut a = format!("\t<TILE_SET name='{}' red='{}' green='{}' blue='{}'", esc(s.name()), colour(*s.red()), colour(*s.green()), colour(*s.blue()));
    for (k, v) in [("also_place_tile_set", s.also_place_tile_set()), ("link_as_set", s.link_as_set()), ("linking_tile", s.linking_tile()), ("shared_geometry", s.shared_geometry())] {
        if !v.is_empty() {
            a += &format!(" {k}='{}'", esc(v));
        }
    }

    a + ">\r\n"
}

/// A tile as Rome 2's files write it (the attributes at their defaults left out, in the files' order), with its lines' indents.
pub fn tile_xml(t: &Tile, group: &str) -> String {
    let mut s = format!(
        "\t\t<TILE location='{}' masked='{}' width='{}' height='{}' texture_group='{}' red_channel='{}' green_channel='{}' blue_channel='{}' alpha_channel='{}' custom_alpha_blend_texture='{}'",
        esc(t.location()),
        !t.mask().is_empty(),
        t.width(),
        t.height(),
        esc(group),
        esc(t.texture_red()),
        esc(t.texture_green()),
        esc(t.texture_blue()),
        esc(t.texture_alpha()),
        esc(t.custom_alpha_blend_texture())
    );
    if *t.encampable() {
        s += " encampable='true'";
    }

    if *t.requires_infield_lodding() {
        s += " requires_infield_lodding='true'";
    }

    if !*t.random_rotatable() {
        s += " random_rotatable='false'";
    }

    if [t.red(), t.green(), t.blue()].iter().any(|c| **c != 0.0) {
        s += &format!(" red='{}' green='{}' blue='{}'", colour(*t.red()), colour(*t.green()), colour(*t.blue()));
    }

    s += ">\r\n";
    if !t.mask().is_empty() {
        s += &format!("\t\t\t<NEW_MASK data='{}' />\r\n", esc(t.mask()));
    }

    for l in t.link_targets() {
        s += &format!("\t\t\t<LINK_TARGET x='{}' y='{}' target_set='{}'  />\r\n", l.x(), l.y(), esc(l.tile_set()));
    }

    for v in t.variations() {
        s += &format!("\t\t\t<VARIATION name='{}'", esc(v.location()));
        if [v.red(), v.green(), v.blue()].iter().any(|c| **c != 0.0) {
            s += &format!(" red='{}' green='{}' blue='{}'", colour(*v.red()), colour(*v.green()), colour(*v.blue()));
        }

        s += &format!(
            " scale='{}' min_height='{}' normal_strength='{}' overlap_border_size='{}'",
            fixed6(*v.scale()), fixed6(*v.min_height()), fixed6(*v.normal_strength()), fixed6(*v.overlap_border_size())
        );
        if *v.raw_data_tri_density() != 128 {
            s += &format!(" raw_data_tri_density='{}'", v.raw_data_tri_density());
        }

        if !v.blend_common().is_empty() || !v.index_common().is_empty() {
            s += &format!(" blend_common='{}' index_common='{}'", esc(v.blend_common()), esc(v.index_common()));
        }

        if !v.normal_common().is_empty() {
            s += &format!(" normal_common='{}'", esc(v.normal_common()));
        }

        s += "/>\r\n";
    }

    for l in t.links() {
        s += &format!(
            "\t\t\t<LINK x='{}' y='{}' test='{}' name='{}' base_x='{}' base_y='{}' no_offline_blend='{}' is_entry='{}' blend_size='{}'/>\r\n",
            l.x(),
            l.y(),
            rome2_link_test(l.test()),
            esc(l.link_set()),
            l.base_x(),
            l.base_y(),
            l.no_offline_blend(),
            l.is_entry(),
            l.blend_size()
        );
    }

    s + "\t\t</TILE>\r\n"
}

/// The start of the line an offset is on (after its `\n`).
fn line_start(text: &[u8], at: usize) -> usize {
    text[..at].iter().rposition(|&b| b == b'\n').map_or(0, |i| i + 1)
}

/// The end of the line an offset is on (after its `\n`), or the text's end.
fn line_end(text: &[u8], at: usize) -> usize {
    text[at..].iter().position(|&b| b == b'\n').map_or(text.len(), |i| at + i + 1)
}

/// A tile set file with `element` (a tile's lines, [`tile_xml`]) in place of the tile of `location`'s lines, or after the last tile of
/// the set `set` (before its end tag) when the file has no such tile.
fn with_tile_element(bytes: &[u8], set: &str, location: &str, element: &str) -> Result<Vec<u8>, String> {
    let doc = XmlDoc::parse(bytes.to_vec()).map_err(|e| e.to_string())?;
    let text = doc.original_text();
    let root = top(&doc, "TILE_DATABASE").ok_or("no <TILE_DATABASE>")?;
    let sets = children(&doc, root, "TILE_SET").collect::<Vec<_>>();
    let (start, end) = match sets.iter().flat_map(|&s| children(&doc, s, "TILE")).find(|&t| string(&doc, t, "location").eq_ignore_ascii_case(location)) {
        Some(t) => {
            let e = &doc.elements()[t];
            (line_start(text, e.start_off), line_end(text, e.end_off))
        }

        None => {
            let s = *sets.iter().find(|&&s| string(&doc, s, "name").eq_ignore_ascii_case(set)).ok_or_else(|| format!("no tile set {set}"))?;
            let e = &doc.elements()[s];
            let close = text[..e.end_off].windows(2).rposition(|w| w == b"</").ok_or("a tile set without its end tag")?;
            let at = line_start(text, close);
            (at, at)
        }
    };

    let mut out = text[..start].to_vec();
    out.extend_from_slice(element.as_bytes());
    out.extend_from_slice(&text[end..]);
    Ok(out)
}

/// The settings with a texture group added after the last one.
fn with_texture_group(bytes: &[u8], name: &str, channels: &[String; 4]) -> Result<Vec<u8>, String> {
    let doc = XmlDoc::parse(bytes.to_vec()).map_err(|e| e.to_string())?;
    let text = doc.original_text();
    let root = top(&doc, "TILE_DATABASE").ok_or("no <TILE_DATABASE>")?;
    let groups = children(&doc, root, "TEXTURE_GROUPS").next().ok_or("the settings have no <TEXTURE_GROUPS>")?;
    let e = &doc.elements()[groups];
    let close = text[..e.end_off].windows(2).rposition(|w| w == b"</").ok_or("<TEXTURE_GROUPS> without its end tag")?;
    let at = line_start(text, close);
    let [r, g, b, a] = channels.each_ref().map(|c| esc(c));
    let line = format!("\t\t<GROUP name='{}' red='{r}' green='{g}' blue='{b}' alpha='{a}' />\r\n", esc(name));
    let mut out = text[..at].to_vec();
    out.extend_from_slice(line.as_bytes());
    out.extend_from_slice(&text[at..]);
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    const SETTINGS: &str = "<TILE_DATABASE>\r\n\t<CONVERSION_PARAMS triangle_density='128' max_lf_heights_per_pixel='128' triangle_decimation_size_factor0='3.000000'/>\r\n\
        \t<TEXTURE_GROUPS>\r\n\t\t<GROUP name='group14' red='sand' green='forest' blue='grass' alpha='mud' />\r\n\t</TEXTURE_GROUPS>\r\n\
        \t<CLIMATES>\r\n\t\t<CLIMATE name='ROME_Med' texture_set='grass' red='255' green='120' blue='0' >\r\n\t\t<TEXTURE name='rock' blend_pixel_scale='40.099998' />\r\n\t\t</CLIMATE>\r\n\t</CLIMATES>\r\n</TILE_DATABASE>\r\n";

    const SET: &str = "<TILE_DATABASE>\r\n\t<TILE_SET name='river_ford' red='60' green='60' blue='255' link_as_set='river' linking_tile='terrain\\tiles\\battle\\river\\1x1_blend\\aa'>\r\n\
        \t\t<TILE location='terrain\\tiles\\battle\\river_ford\\2x2_a' masked='true' width='2' height='2' texture_group='group14' red_channel='sand' green_channel='forest' blue_channel='grass' alpha_channel='mud' custom_alpha_blend_texture='' requires_infield_lodding='true'>\r\n\
        \t\t\t<NEW_MASK data='0110' />\r\n\
        \t\t\t<LINK_TARGET x='0' y='0' target_set='river'  />\r\n\
        \t\t\t<VARIATION name='aa' scale='12.000000' min_height='-12.000000' normal_strength='1.000000' overlap_border_size='1.000000'/>\r\n\
        \t\t\t<LINK x='0' y='-1' test='equals' name='river' base_x='1' base_y='0' no_offline_blend='false' is_entry='false' blend_size='64'/>\r\n\
        \t\t</TILE>\r\n\t</TILE_SET>\r\n</TILE_DATABASE>\r\n";

    fn database() -> Rome2TileDatabase {
        let files = [(SETTINGS_FILE.to_owned(), SETTINGS.as_bytes().to_vec()), ("river_ford.xml".to_owned(), SET.as_bytes().to_vec())].into_iter().collect();
        Rome2TileDatabase::read(files).unwrap()
    }

    #[test]
    fn reads_as_attilas_database() {
        let db = database();
        let d = &db.database;
        assert_eq!(*d.conversion_params().triangle_decimation_size_factors0(), 3.0);
        assert_eq!(d.climates()[0].name(), "ROME_Med");
        assert_eq!(*d.climates()[0].textures()[0].lerp_reflectivity0(), 0.009);
        assert_eq!(d.tile_sets()[0].link_as_set(), "river");
        let t = &d.tiles()[0];
        assert_eq!(t.tile_set(), "river_ford");
        assert_eq!(t.mask(), "0110");
        assert!(*t.random_rotatable() && *t.requires_infield_lodding() && !*t.encampable());
        assert_eq!(t.texture_red(), "sand");
        assert_eq!(*t.variations()[0].raw_data_tri_density(), 128);
        assert_eq!(t.links()[0].test(), "TLT_EQUALS");
        assert_eq!(t.links()[0].link_set(), "river");
        assert_eq!(t.link_targets()[0].tile_set(), "river");
        assert_eq!(db.set_file("RIVER_FORD"), Some("river_ford.xml"));
    }

    #[test]
    fn an_unchanged_tile_writes_its_file_back_byte_for_byte() {
        let db = database();
        let files = db.with_tile(&db.database.tiles()[0]).unwrap();
        assert_eq!(files, [("river_ford.xml".to_owned(), SET.as_bytes().to_vec())]);
    }

    #[test]
    fn new_channels_get_a_texture_group_and_new_tiles_join_their_set() {
        let db = database();
        let mut t = db.database.tiles()[0].clone();
        t.set_location("terrain\\tiles\\battle\\river_ford\\2x2_b".to_owned());
        t.set_texture_alpha("rock".to_owned());
        t.set_mask(String::new());
        let files = db.with_tile(&t).unwrap();
        let settings = String::from_utf8(files[0].1.clone()).unwrap();
        assert!(settings.contains("\t\t<GROUP name='group1' red='sand' green='forest' blue='grass' alpha='rock' />\r\n\t</TEXTURE_GROUPS>"));
        let set = String::from_utf8(files[1].1.clone()).unwrap();
        assert!(set.starts_with(&SET[..SET.find("\t</TILE_SET>").unwrap()]));
        assert!(set.contains("2x2_b' masked='false' width='2' height='2' texture_group='group1'"));
        let again = Rome2TileDatabase::read([(SETTINGS_FILE.to_owned(), files[0].1.clone()), ("river_ford.xml".to_owned(), files[1].1.clone())].into_iter().collect()).unwrap();
        assert_eq!(again.database.tiles().len(), 2);
        assert_eq!(again.database.tiles()[1], t);
    }
}
