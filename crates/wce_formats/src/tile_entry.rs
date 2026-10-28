//! A tile's entry in the tile database: `_tile_database/TILES/<set>_<tile>.xml`.
//!
//! The entry is the root of a tile: it says where the files are (`TILE@location` + `VARIATION@location` -> the leaf folder that
//! holds tile.ted, height_map_0.png, blend_map.tif...), how big the tile is, its colour on the map tools, the four ground texture
//! groups, per variation the height and normal map parameters, and the tile links. tile_database.bin is BOB's compiled form of
//! all entries (rpfm_lib's `TileDatabase`).
//!
//! The document is the lossless [`XmlDoc`]: an unedited entry saves byte for byte, an edit changes only its span. Typed access
//! reads the attributes on demand; [`TileEntry::unknown_names`] lists what it doesn't cover (nothing in the 1,855 shipped files).

use std::path::{Path, PathBuf};

use crate::c_num::{strtod, strtol};
use crate::xml_doc::{XmlDoc, XmlError};

const TILE_ATTRS: &[&str] = &["serialise_version", "location", "tile_set", "mask", "width", "height", "red", "green", "blue", "requires_infield_lodding",
    "random_rotatable", "custom_alpha_blend_texture", "scalable", "encampable", "custom_blend_tile"];
const GROUP_ATTRS: &[&str] = &["red", "green", "blue", "alpha"];
const VARIATION_ATTRS: &[&str] = &["serialise_version", "location", "min_height", "scale", "normal_strength", "overlap_border_size", "raw_data_tri_density",
    "blend_common", "index_common", "normal_common", "red", "green", "blue", "barbarian"];
const TARGET_ATTRS: &[&str] = &["serialise_version", "tile_set", "x", "y"];
const LINK_ATTRS: &[&str] = &["serialise_version", "link_set", "x", "y", "base_x", "base_y", "is_entry", "blend_size", "no_offline_blend", "test"];

/// Errors of reading an entry.
#[derive(Debug, thiserror::Error)]
pub enum TileEntryError {
    #[error(transparent)]
    Xml(#[from] XmlError),

    #[error("not a tile database entry (no <TILE> element)")]
    NotATile,

    #[error("{0}: {1}")]
    Load(String, Box<TileEntryError>),

    #[error("{0}: {1}")]
    Io(String, std::io::Error),
}

/// The texture group names of the four blend layers (`settlement_1`, `forest`...).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct TileTextureGroup {
    pub red: String,
    pub green: String,
    pub blue: String,
    pub alpha: String,
}

/// A variation of a tile.
#[derive(Clone, Debug, PartialEq)]
pub struct TileVariation {
    /// Element index in the entry's document.
    pub elem: usize,
    pub serialise_version: i32,

    /// Leaf folder name under the tile's location (`battlefield`, `minor`, `medium`...).
    pub location: String,

    /// Metres: height = min_height + raw / 65535 * scale.
    pub min_height: f64,
    pub scale: f64,
    pub normal_strength: f64,

    /// Cells of overlap with the neighbours (the height map's border).
    pub overlap_border_size: f64,
    pub raw_data_tri_density: u32,
    pub blend_common: String,
    pub index_common: String,
    pub normal_common: String,
    pub red: f64,
    pub green: f64,
    pub blue: f64,
    pub barbarian: bool,
}

/// The kind of join a tile offers at an edge cell.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TileLinkTarget {
    pub elem: usize,
    pub serialise_version: i32,
    pub tile_set: String,

    /// Cell of the tile (128 m cells).
    pub x: i32,
    pub y: i32,
}

/// A condition on a neighbour of the tile.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TileLink {
    pub elem: usize,
    pub serialise_version: i32,

    /// `sea`, `river`, `roads`, `sea_coast`...
    pub link_set: String,

    /// The cell that has to (`TLT_EQUALS`) or must not (`TLT_NOT_EQUALS`) belong to `link_set`; x = -1 or width is the neighbour column.
    pub x: i32,
    pub y: i32,
    pub base_x: i32,
    pub base_y: i32,
    pub is_entry: bool,

    /// Pixels the neighbour's terrain is blended into this tile at that edge.
    pub blend_size: u32,
    pub no_offline_blend: bool,

    /// `TLT_EQUALS` or `TLT_NOT_EQUALS`.
    pub test: String,

    /// The `<blend_quad/>` child (empty in every shipped file).
    pub has_blend_quad: bool,
}

/// A tile database entry.
#[derive(Clone, Debug)]
pub struct TileEntry {
    doc: XmlDoc,
    root: usize,
    file: PathBuf,
}

/// Lower case, `/` separators, no trailing separator: the one spelling of a location used everywhere.
pub fn normalize_location(location: &str) -> String {
    let mut s = location.replace('\\', "/").to_ascii_lowercase();
    s.truncate(s.trim_end_matches('/').len());
    s
}

/// `<kind>` of a normalized `terrain/tiles/<kind>/...` location, if it is one.
pub(crate) fn kind_of(key: &str) -> Option<&str> {
    let rest = key.strip_prefix("terrain/tiles/")?;
    Some(rest.split('/').next().unwrap_or_default())
}

/// A path plus a location with either separator.
fn append_location(path: &Path, location: &str) -> PathBuf {
    location.split(['\\', '/']).filter(|part| !part.is_empty()).fold(path.to_path_buf(), |path, part| path.join(part))
}

impl TileEntry {

    /// Reads an entry.
    ///
    /// # Errors
    ///
    /// Fails for malformed XML, or a document without a `<TILE>` root.
    pub fn parse(bytes: Vec<u8>) -> Result<Self, TileEntryError> {
        let doc = XmlDoc::parse(bytes)?;
        let root = doc.roots().iter().copied().find(|&root| doc.elements()[root].name == "TILE").ok_or(TileEntryError::NotATile)?;
        Ok(Self { doc, root, file: PathBuf::new() })
    }

    /// Reads an entry file, remembering its path.
    ///
    /// # Errors
    ///
    /// Fails if the file can't be read, or [`Self::parse`] fails; the message starts with the path.
    pub fn load(xml: &Path) -> Result<Self, TileEntryError> {
        let path = xml.display().to_string();
        let bytes = std::fs::read(xml).map_err(|error| TileEntryError::Io(format!("cannot open {path}"), error))?;
        let mut entry = Self::parse(bytes).map_err(|error| TileEntryError::Load(path, Box::new(error)))?;
        entry.file = xml.to_path_buf();
        Ok(entry)
    }

    pub fn save(&self) -> Vec<u8> {
        self.doc.save()
    }

    pub fn modified(&self) -> bool {
        self.doc.modified()
    }

    pub fn doc(&self) -> &XmlDoc {
        &self.doc
    }

    pub fn doc_mut(&mut self) -> &mut XmlDoc {
        &mut self.doc
    }

    /// The file the entry was loaded from; empty when it was parsed from bytes.
    pub fn file(&self) -> &Path {
        &self.file
    }

    /// The file the entry is from now on (a copy of it was made there).
    pub fn set_file(&mut self, file: &Path) {
        self.file = file.to_path_buf();
    }

    /// A variation's height range (`min_height`, `scale`), written as TEd writes them (`%f`); `false` when the text did not change.
    pub fn set_height_range(&mut self, variation: usize, min_height: f64, scale: f64) -> bool {
        let Some(elem) = self.items("VARIATIONS", "VARIATION").get(variation).copied() else { return false };
        let mut changed = false;
        for (name, value) in [("min_height", min_height), ("scale", scale)] {
            let text = format!("{value:.6}");
            if self.doc.get(elem, name).as_deref() != Some(text.as_str()) {
                self.doc.set(elem, name, &text);
                changed = true;
            }
        }

        changed
    }

    /// The `TILE` element.
    pub fn root(&self) -> usize {
        self.root
    }

    fn str_attr(&self, elem: usize, name: &str) -> String {
        self.doc.get(elem, name).unwrap_or_default()
    }

    fn num_attr(&self, elem: usize, name: &str) -> f64 {
        self.doc.get(elem, name).and_then(|value| strtod(&value)).unwrap_or(0.0)
    }

    fn int_attr(&self, elem: usize, name: &str) -> i32 {
        self.doc.get(elem, name).and_then(|value| strtol(&value)).unwrap_or(0) as i32
    }

    fn bool_attr(&self, elem: usize, name: &str) -> bool {
        self.doc.get(elem, name).is_some_and(|value| value == "true")
    }

    /// The first child element with a name.
    fn child(&self, elem: usize, name: &str) -> Option<usize> {
        self.doc.elements()[elem].children.iter().copied().find(|&child| self.doc.elements()[child].name == name)
    }

    /// The children of the root's `list` element named `item`.
    fn items(&self, list: &str, item: &str) -> Vec<usize> {
        self.child(self.root, list).map(|list| {
            self.doc.elements()[list].children.iter().copied().filter(|&child| self.doc.elements()[child].name == item).collect()
        }).unwrap_or_default()
    }

    pub fn serialise_version(&self) -> i32 {
        self.int_attr(self.root, "serialise_version")
    }

    /// `terrain\tiles\battle\<set>\<tile>`, as written (backslashes).
    pub fn location(&self) -> String {
        self.str_attr(self.root, "location")
    }

    /// The location with `/`.
    pub fn location_generic(&self) -> String {
        self.location().replace('\\', "/")
    }

    /// `battle` or `campaign`: the folder under terrain/tiles. Empty if the location isn't under it.
    pub fn kind(&self) -> String {
        kind_of(&normalize_location(&self.location())).unwrap_or_default().to_owned()
    }

    pub fn tile_set(&self) -> String {
        self.str_attr(self.root, "tile_set")
    }

    pub fn mask(&self) -> String {
        self.str_attr(self.root, "mask")
    }

    /// Cells of 128 m.
    pub fn width(&self) -> u32 {
        self.int_attr(self.root, "width") as u32
    }

    pub fn height(&self) -> u32 {
        self.int_attr(self.root, "height") as u32
    }

    /// The tile's colour on the map tools (0..255).
    pub fn red(&self) -> f64 {
        self.num_attr(self.root, "red")
    }

    pub fn green(&self) -> f64 {
        self.num_attr(self.root, "green")
    }

    pub fn blue(&self) -> f64 {
        self.num_attr(self.root, "blue")
    }

    pub fn requires_infield_lodding(&self) -> bool {
        self.bool_attr(self.root, "requires_infield_lodding")
    }

    pub fn random_rotatable(&self) -> bool {
        self.bool_attr(self.root, "random_rotatable")
    }

    pub fn custom_alpha_blend_texture(&self) -> String {
        self.str_attr(self.root, "custom_alpha_blend_texture")
    }

    pub fn scalable(&self) -> bool {
        self.bool_attr(self.root, "scalable")
    }

    pub fn encampable(&self) -> bool {
        self.bool_attr(self.root, "encampable")
    }

    pub fn custom_blend_tile(&self) -> String {
        self.str_attr(self.root, "custom_blend_tile")
    }

    pub fn texture_group(&self) -> TileTextureGroup {
        self.child(self.root, "TEXTURE_GROUP").map(|c| TileTextureGroup {
            red: self.str_attr(c, "red"),
            green: self.str_attr(c, "green"),
            blue: self.str_attr(c, "blue"),
            alpha: self.str_attr(c, "alpha"),
        }).unwrap_or_default()
    }

    /// The names of the TEXTURE_GROUP attributes, in the order of the blend map's groups.
    pub const TEXTURE_GROUP_SLOTS: [&'static str; 4] = ["red", "green", "blue", "alpha"];

    /// Sets one of the four texture groups (0..4 = red, green, blue, alpha); false when nothing changed or there is no such slot or
    /// TEXTURE_GROUP element.
    pub fn set_texture_group(&mut self, slot: usize, name: &str) -> bool {
        let (Some(attr), Some(elem)) = (Self::TEXTURE_GROUP_SLOTS.get(slot), self.child(self.root, "TEXTURE_GROUP")) else { return false };
        if self.doc.get(elem, attr).as_deref() == Some(name) {
            return false;
        }

        self.doc.set(elem, attr, name);
        true
    }

    pub fn variations(&self) -> Vec<TileVariation> {
        self.items("VARIATIONS", "VARIATION").into_iter().map(|c| TileVariation {
            elem: c,
            serialise_version: self.int_attr(c, "serialise_version"),
            location: self.str_attr(c, "location"),
            min_height: self.num_attr(c, "min_height"),
            scale: if self.doc.find_attr(c, "scale").is_some() { self.num_attr(c, "scale") } else { 100.0 },
            normal_strength: self.num_attr(c, "normal_strength"),
            overlap_border_size: self.num_attr(c, "overlap_border_size"),
            raw_data_tri_density: self.int_attr(c, "raw_data_tri_density") as u32,
            blend_common: self.str_attr(c, "blend_common"),
            index_common: self.str_attr(c, "index_common"),
            normal_common: self.str_attr(c, "normal_common"),
            red: self.num_attr(c, "red"),
            green: self.num_attr(c, "green"),
            blue: self.num_attr(c, "blue"),
            barbarian: self.bool_attr(c, "barbarian"),
        }).collect()
    }

    pub fn link_targets(&self) -> Vec<TileLinkTarget> {
        self.items("TILE_LINK_TARGETS", "TILE_LINK_TARGET").into_iter().map(|c| TileLinkTarget {
            elem: c,
            serialise_version: self.int_attr(c, "serialise_version"),
            tile_set: self.str_attr(c, "tile_set"),
            x: self.int_attr(c, "x"),
            y: self.int_attr(c, "y"),
        }).collect()
    }

    pub fn links(&self) -> Vec<TileLink> {
        self.items("TILE_LINKS", "TILE_LINK").into_iter().map(|c| TileLink {
            elem: c,
            serialise_version: self.int_attr(c, "serialise_version"),
            link_set: self.str_attr(c, "link_set"),
            x: self.int_attr(c, "x"),
            y: self.int_attr(c, "y"),
            base_x: self.int_attr(c, "base_x"),
            base_y: self.int_attr(c, "base_y"),
            is_entry: self.bool_attr(c, "is_entry"),
            blend_size: self.int_attr(c, "blend_size") as u32,
            no_offline_blend: self.bool_attr(c, "no_offline_blend"),
            test: self.str_attr(c, "test"),
            has_blend_quad: self.child(c, "blend_quad").is_some(),
        }).collect()
    }

    /// Replaces the link targets and the links (TEd's Tile links), written as the kit writes them: one element a line, tab-indented, the
    /// attributes in the kit's order with single quotes, every `TILE_LINK` with its `<blend_quad/>`, an empty list as `<TILE_LINKS/>`. The
    /// blocks are spliced only when they change, so an unchanged entry keeps its bytes. `elem` of the items is ignored. Returns whether
    /// anything changed.
    ///
    /// # Errors
    ///
    /// The entry has no `TILE_LINK_TARGETS` / `TILE_LINKS` element, or the new text can't be read.
    pub fn set_links(&mut self, targets: &[TileLinkTarget], links: &[TileLink]) -> Result<bool, TileEntryError> {
        let (old_targets, old_links) = (self.link_targets(), self.links());
        let same = |a: &TileLinkTarget, b: &TileLinkTarget| (a.serialise_version, &a.tile_set, a.x, a.y) == (b.serialise_version, &b.tile_set, b.x, b.y);
        let same_link = |a: &TileLink, b: &TileLink| {
            (a.serialise_version, &a.link_set, a.x, a.y, a.base_x, a.base_y, a.is_entry, a.blend_size, a.no_offline_blend, &a.test, a.has_blend_quad)
                == (b.serialise_version, &b.link_set, b.x, b.y, b.base_x, b.base_y, b.is_entry, b.blend_size, b.no_offline_blend, &b.test, b.has_blend_quad)
        };
        let targets_same = old_targets.len() == targets.len() && old_targets.iter().zip(targets).all(|(a, b)| same(a, b));
        let links_same = old_links.len() == links.len() && old_links.iter().zip(links).all(|(a, b)| same_link(a, b));
        if targets_same && links_same {
            return Ok(false);
        }

        self.doc.bake_pending()?;
        let crlf = self.doc.original_text().windows(2).any(|w| w == b"\r\n");
        let nl = if crlf { "\r\n" } else { "\n" };
        let b = |v: bool| if v { "true" } else { "false" };
        if !links_same {
            let mut text = String::new();
            if links.is_empty() {
                text += "<TILE_LINKS/>";
            } else {
                text += &format!("<TILE_LINKS>{nl}");
                for l in links {
                    text += &format!(
                        "\t\t<TILE_LINK serialise_version='{}' link_set='{}' x='{}' y='{}' base_x='{}' base_y='{}' is_entry='{}' blend_size='{}' no_offline_blend='{}' test='{}'",
                        l.serialise_version, l.link_set, l.x, l.y, l.base_x, l.base_y, b(l.is_entry), l.blend_size, b(l.no_offline_blend), l.test
                    );
                    text += &if l.has_blend_quad { format!(">{nl}\t\t\t<blend_quad/>{nl}\t\t</TILE_LINK>{nl}") } else { format!("/>{nl}") };
                }

                text += "\t</TILE_LINKS>";
            }

            self.replace_block("TILE_LINKS", &text)?;
        }

        if !targets_same {
            let mut text = String::new();
            if targets.is_empty() {
                text += "<TILE_LINK_TARGETS/>";
            } else {
                text += &format!("<TILE_LINK_TARGETS>{nl}");
                for t in targets {
                    text += &format!("\t\t<TILE_LINK_TARGET serialise_version='{}' tile_set='{}' x='{}' y='{}'/>{nl}", t.serialise_version, t.tile_set, t.x, t.y);
                }

                text += "\t</TILE_LINK_TARGETS>";
            }

            self.replace_block("TILE_LINK_TARGETS", &text)?;
        }

        Ok(true)
    }

    /// Sets an attribute of the `<TILE>` element (`location`, `tile_set`, `red`...): false when it had that value.
    pub fn set_tile_attr(&mut self, name: &str, value: &str) -> bool {
        if self.doc.get(self.root, name).as_deref() == Some(value) {
            return false;
        }

        self.doc.set(self.root, name, value);
        true
    }

    /// A variation's element as written (its text, edits applied), to put into another entry (`set_variations`).
    pub fn variation_text(&self, index: usize) -> Option<String> {
        let elem = *self.items("VARIATIONS", "VARIATION").get(index)?;
        let el = &self.doc.elements()[elem];
        Some(String::from_utf8_lossy(&self.doc.text_of_range(el.start_off, el.end_off)).into_owned())
    }

    /// Replaces the variations with these elements' texts (`variation_text`'s, the `location` attribute of each set to the paired name when
    /// there is one), one a line as the kit writes them.
    ///
    /// # Errors
    ///
    /// The entry has no `VARIATIONS` element, or the new text can't be read.
    pub fn set_variations(&mut self, variations: &[(String, Option<String>)]) -> Result<(), TileEntryError> {
        self.doc.bake_pending()?;
        let crlf = self.doc.original_text().windows(2).any(|w| w == b"\r\n");
        let nl = if crlf { "\r\n" } else { "\n" };
        let mut text = format!("<VARIATIONS>{nl}");
        for (v, location) in variations {
            let v = match location {
                Some(loc) => match v.find("location='").map(|i| i + "location='".len()) {
                    Some(start) => {
                        let end = v[start..].find('\'').map_or(start, |e| start + e);
                        format!("{}{loc}{}", &v[..start], &v[end..])
                    }

                    None => v.clone(),
                },

                None => v.clone(),
            };

            text += &format!("\t\t{}{nl}", v.trim());
        }

        text += "\t</VARIATIONS>";
        self.replace_block("VARIATIONS", &text)
    }

    /// Replaces the root's child element `name` (its whole text) with `text`.
    fn replace_block(&mut self, name: &str, text: &str) -> Result<(), TileEntryError> {
        let elem = self.child(self.root, name).ok_or(TileEntryError::NotATile)?;
        let (start, end) = (self.doc.elements()[elem].start_off, self.doc.elements()[elem].end_off);
        self.doc.splice(start, end - start, text.as_bytes())?;
        self.root = self.doc.roots().iter().copied().find(|&r| self.doc.elements()[r].name == "TILE").ok_or(TileEntryError::NotATile)?;
        Ok(())
    }

    /// The folder that holds terrain/tiles (an Assembly Kit's raw_data), from the entry's file
    /// (`<raw_root>/terrain/tiles/<kind>/_tile_database/TILES/<name>.xml`). `None` when the entry was not loaded from a kit.
    pub fn raw_root(&self) -> Option<PathBuf> {
        let tiles_dir = self.file.parent()?;
        let db = tiles_dir.parent()?;
        let kind = db.parent()?;
        let tiles = kind.parent()?;
        let terrain = tiles.parent()?;
        let named = |path: &Path, name: &str| path.file_name().is_some_and(|file| file.to_string_lossy().eq_ignore_ascii_case(name));
        if !named(tiles_dir, "tiles") || !named(db, "_tile_database") || !named(tiles, "tiles") || !named(terrain, "terrain") {
            return None;
        }

        terrain.parent().map(Path::to_path_buf)
    }

    /// `<raw_root>/<location>`.
    pub fn tile_folder(&self, raw_root: &Path) -> PathBuf {
        append_location(raw_root, &self.location())
    }

    /// The leaf folder of a variation; the tile folder if there's no such variation.
    pub fn variation_folder(&self, raw_root: &Path, variation: usize) -> PathBuf {
        let folder = self.tile_folder(raw_root);
        match self.variations().get(variation) {
            Some(v) => append_location(&folder, &v.location),
            None => folder,
        }
    }

    pub fn ted_path(&self, raw_root: &Path, variation: usize) -> PathBuf {
        self.variation_folder(raw_root, variation).join("tile.ted")
    }

    /// The index of the variation whose leaf folder is `name` (case-insensitive).
    pub fn find_variation(&self, name: &str) -> Option<usize> {
        let want = normalize_location(name);
        self.variations().iter().position(|v| normalize_location(&v.location) == want)
    }

    /// Names of attributes and children (as `TILE/VARIATIONS/VARIATION@foo`, `TILE/bar`) the typed access doesn't know.
    pub fn unknown_names(&self) -> Vec<String> {
        let els = self.doc.elements();
        let mut out = vec![];
        let attrs = |out: &mut Vec<String>, e: usize, known: &[&str], path: &str| {
            out.extend(els[e].attrs.iter().filter(|at| !known.contains(&at.name.as_str())).map(|at| format!("{path}@{}", at.name)));
        };

        attrs(&mut out, self.root, TILE_ATTRS, "TILE");
        for &c in &els[self.root].children {
            let n = els[c].name.as_str();
            let (item, known) = match n {
                "TEXTURE_GROUP" => {
                    attrs(&mut out, c, GROUP_ATTRS, "TILE/TEXTURE_GROUP");
                    out.extend(els[c].children.iter().map(|&g| format!("TILE/TEXTURE_GROUP/{}", els[g].name)));
                    continue;
                }

                "VARIATIONS" => ("VARIATION", VARIATION_ATTRS),
                "TILE_LINK_TARGETS" => ("TILE_LINK_TARGET", TARGET_ATTRS),
                "TILE_LINKS" => ("TILE_LINK", LINK_ATTRS),
                _ => {
                    out.push(format!("TILE/{n}"));
                    continue;
                }
            };

            attrs(&mut out, c, &[], &format!("TILE/{n}"));
            for &i in &els[c].children {
                if els[i].name != item {
                    out.push(format!("TILE/{n}/{}", els[i].name));
                    continue;
                }

                attrs(&mut out, i, known, &format!("TILE/{n}/{item}"));
                for &k in &els[i].children {
                    if !(n == "TILE_LINKS" && els[k].name == "blend_quad" && els[k].children.is_empty()) {
                        out.push(format!("TILE/{n}/{item}/{}", els[k].name));
                    }
                }
            }
        }

        out
    }
}
