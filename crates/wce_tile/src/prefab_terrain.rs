//! A prefab's height layer (docs/plans/prefab_tool.md): the ground a prefab brings with it, so that it does not need meshes that pretend to
//! be terrain. Heights only: texture and ground type layers would mean other things in each tile.
//!
//! The editor's prefab file (`<key>.wce`) keeps it in one element after the group's `children`, at the root:
//!
//! ```text
//! <INSTANCE_SELECTION version="17" wce_version="1">
//!     <children>
//!         <UserInstanceGroup ...>...</UserInstanceGroup>
//!     </children>
//!     <WceTerrain origin="-12,-8" spacing="1" size="25,17" normalize="true" feather="4" samples="eJzt..."/>
//! </INSTANCE_SELECTION>
//! ```
//!
//! Everything is in the prefab's own units, those of its items' positions (the same in every tile; a tile turns them into its height
//! map's): `origin` is where the first sample is (x, z) from the group's origin, `spacing` how far apart samples are, `size` how many there
//! are across (x) and down (z). `samples` is base64 of the zlib stream of the heights (`f32`, row by row) and then the coverage (`f32`,
//! 0 to 1), little-endian.
//!
//! - **Heights** are above the plain the prefab was made on (0: untouched).
//! - **Coverage** is how much the prefab's sculpting touched each sample: the area `normalize` flattens.
//! - **`normalize`** off: the heights add to whatever ground the prefab is put on. On: where it covers, the ground is taken as the plain
//!   first (at the tile's height under the prefab's origin), blended into what is around over `feather`, then the heights go over it: it
//!   looks as it did where it was made.
//!
//! TEd's prefabs have no terrain: `project_files::to_ted` drops the element, and says so.

use wce_formats::png_file::{deflate_bytes, inflate_bytes};
use wce_formats::xml_doc::XmlDoc;

use crate::ted_scene::format_number;

/// The element.
pub const ELEMENT: &str = "WceTerrain";

/// The feather a new layer gets, in the prefab's units.
pub const DEFAULT_FEATHER: f64 = 4.0;

/// The most samples a side (a 4096 unit prefab at spacing 1): past it a file is taken as damaged.
const MAX_SIDE: u32 = 4096;

/// A prefab's height layer.
#[derive(Clone, Debug, PartialEq)]
pub struct PrefabTerrain {
    /// Where the first sample is, (x, z) from the group's origin.
    pub origin: [f64; 2],

    /// How far apart samples are.
    pub spacing: f64,
    pub width: u32,
    pub height: u32,

    /// Above the plain, row by row (z), each row along x.
    pub heights: Vec<f32>,

    /// 0 to 1, as `heights`.
    pub coverage: Vec<f32>,
    pub normalize: bool,
    pub feather: f64,
}

impl PrefabTerrain {
    /// A layer with nothing in it over an area.
    #[must_use]
    pub fn empty(origin: [f64; 2], spacing: f64, width: u32, height: u32) -> Self {
        let n = width as usize * height as usize;
        Self { origin, spacing, width, height, heights: vec![0.0; n], coverage: vec![0.0; n], normalize: false, feather: DEFAULT_FEATHER }
    }

    fn index(&self, x: u32, z: u32) -> usize {
        z as usize * self.width as usize + x as usize
    }

    #[must_use]
    pub fn height_at(&self, x: u32, z: u32) -> f32 {
        self.heights[self.index(x, z)]
    }

    #[must_use]
    pub fn coverage_at(&self, x: u32, z: u32) -> f32 {
        self.coverage[self.index(x, z)]
    }

    /// Whether a sample holds anything: some coverage, or a height (a smoothed edge can carry one past what was covered).
    fn holds(&self, i: usize) -> bool {
        self.coverage[i] > 0.0 || self.heights[i] != 0.0
    }

    /// Whether nothing is in it.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        !(0..self.heights.len()).any(|i| self.holds(i))
    }

    /// The layer cut down to the samples that hold anything (as it is saved); none when nothing does.
    #[must_use]
    pub fn cropped(&self) -> Option<Self> {
        let (mut x0, mut z0, mut x1, mut z1) = (u32::MAX, u32::MAX, 0, 0);
        for z in 0..self.height {
            for x in 0..self.width {
                if self.holds(self.index(x, z)) {
                    (x0, z0, x1, z1) = (x0.min(x), z0.min(z), x1.max(x), z1.max(z));
                }
            }
        }

        if x0 == u32::MAX {
            return None;
        }

        let (w, h) = (x1 - x0 + 1, z1 - z0 + 1);
        let mut out = Self::empty([self.origin[0] + f64::from(x0) * self.spacing, self.origin[1] + f64::from(z0) * self.spacing], self.spacing, w, h);
        (out.normalize, out.feather) = (self.normalize, self.feather);
        for z in 0..h {
            let from = self.index(x0, z0 + z);
            let to = out.index(0, z);
            out.heights[to..to + w as usize].copy_from_slice(&self.heights[from..from + w as usize]);
            out.coverage[to..to + w as usize].copy_from_slice(&self.coverage[from..from + w as usize]);
        }

        Some(out)
    }

    /// The element's text (one line, no indentation).
    ///
    /// # Errors
    ///
    /// zlib failed.
    pub fn element_text(&self) -> Result<String, String> {
        let mut raw = Vec::with_capacity(self.heights.len() * 8);
        for v in self.heights.iter().chain(&self.coverage) {
            raw.extend(v.to_le_bytes());
        }

        let packed = deflate_bytes(&raw).map_err(|e| e.to_string())?;
        Ok(format!(
            "<{ELEMENT} origin=\"{},{}\" spacing=\"{}\" size=\"{},{}\" normalize=\"{}\" feather=\"{}\" samples=\"{}\"/>",
            format_number(self.origin[0]), format_number(self.origin[1]), format_number(self.spacing), self.width, self.height, self.normalize,
            format_number(self.feather), base64_encode(&packed),
        ))
    }

    /// Reads an element of a document.
    ///
    /// # Errors
    ///
    /// An attribute is missing or wrong, or the samples don't read.
    pub fn from_element(doc: &XmlDoc, e: usize) -> Result<Self, String> {
        let attr = |name: &str| doc.get(e, name).ok_or_else(|| format!("{ELEMENT}: no {name}"));
        let pair = |name: &str| -> Result<(String, String), String> {
            let v = attr(name)?;
            let (a, b) = v.split_once(',').ok_or_else(|| format!("{ELEMENT}: {name} is not two numbers"))?;
            Ok((a.trim().to_owned(), b.trim().to_owned()))
        };

        let number = |v: &str, name: &str| v.parse::<f64>().ok().filter(|v| v.is_finite()).ok_or_else(|| format!("{ELEMENT}: bad {name} '{v}'"));
        let (ox, oz) = pair("origin")?;
        let origin = [number(&ox, "origin")?, number(&oz, "origin")?];
        let spacing = number(&attr("spacing")?, "spacing")?;
        let (w, h) = pair("size")?;
        let side = |v: &str| v.parse::<u32>().ok().filter(|&v| v > 0 && v <= MAX_SIDE).ok_or_else(|| format!("{ELEMENT}: bad size '{v}'"));
        let (width, height) = (side(&w)?, side(&h)?);
        if spacing <= 0.0 {
            return Err(format!("{ELEMENT}: bad spacing {spacing}"));
        }

        let normalize = doc.get(e, "normalize").is_some_and(|v| v == "true");
        let feather = doc.get(e, "feather").map_or(Ok(DEFAULT_FEATHER), |v| number(&v, "feather"))?.max(0.0);
        let packed = base64_decode(attr("samples")?.as_bytes()).ok_or_else(|| format!("{ELEMENT}: samples are not base64"))?;
        let n = width as usize * height as usize;
        let mut raw = vec![0u8; n * 8];
        inflate_bytes(&packed, &mut raw).map_err(|e| format!("{ELEMENT}: {e}"))?;
        let floats = raw.as_chunks::<4>().0.iter().map(|c| f32::from_le_bytes(*c)).collect::<Vec<_>>();
        let (heights, coverage) = floats.split_at(n);
        Ok(Self { origin, spacing, width, height, heights: heights.to_vec(), coverage: coverage.iter().map(|c| c.clamp(0.0, 1.0)).collect(), normalize, feather })
    }
}

/// The root's terrain element in a prefab file's text.
fn find(doc: &XmlDoc) -> Option<usize> {
    let root = *doc.roots().first()?;
    doc.elements()[root].children.iter().copied().find(|&c| doc.elements()[c].name == ELEMENT)
}

/// A prefab file's height layer, none if it has none.
///
/// # Errors
///
/// The text is not XML, or its layer does not read.
pub fn read(text: &[u8]) -> Result<Option<PrefabTerrain>, String> {
    let doc = XmlDoc::parse(text.to_vec()).map_err(|e| e.to_string())?;
    find(&doc).map(|e| PrefabTerrain::from_element(&doc, e)).transpose()
}

/// A prefab file's text with this height layer (cropped; an empty one or none takes the file's away). The rest of the text stays as it is:
/// a file without a layer that gets none comes back byte for byte.
///
/// # Errors
///
/// The text is not XML, has no root to put it in, or zlib failed.
pub fn with_terrain(text: Vec<u8>, terrain: Option<&PrefabTerrain>) -> Result<Vec<u8>, String> {
    let doc = XmlDoc::parse(text.clone()).map_err(|e| e.to_string())?;
    let mut text = text;

    // The old element goes with its line.
    if let Some(e) = find(&doc) {
        let el = &doc.elements()[e];
        let mut start = el.start_off;
        while start > 0 && matches!(text[start - 1], b' ' | b'\t') {
            start -= 1;
        }

        let mut end = el.end_off;
        if text.get(end) == Some(&b'\r') {
            end += 1;
        }

        if text.get(end) == Some(&b'\n') {
            end += 1;
        }

        text.drain(start..end);
    }

    let Some(terrain) = terrain.and_then(PrefabTerrain::cropped) else { return Ok(text) };

    // The new one before the root's end tag, a level in.
    let doc = XmlDoc::parse(text.clone()).map_err(|e| e.to_string())?;
    let root = *doc.roots().first().ok_or("a prefab file without a root")?;
    let el = &doc.elements()[root];
    if el.self_closing {
        return Err("a prefab file with an empty root".to_owned());
    }

    let close = text[..el.end_off].iter().rposition(|&c| c == b'<').ok_or("a root without an end tag")?;
    let mut line_start = close;
    while line_start > 0 && matches!(text[line_start - 1], b' ' | b'\t') {
        line_start -= 1;
    }

    let at_line = line_start == 0 || text[line_start - 1] == b'\n';
    let indent = String::from_utf8_lossy(&text[line_start..close]).into_owned();
    let line = format!("{indent}    {}\n", terrain.element_text()?);
    if at_line {
        text.splice(line_start..line_start, line.into_bytes());
    } else {
        text.splice(close..close, format!("\n{line}").into_bytes());
    }

    Ok(text)
}

const BASE64: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

fn base64_encode(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let b = [chunk[0], chunk.get(1).copied().unwrap_or(0), chunk.get(2).copied().unwrap_or(0)];
        let n = u32::from(b[0]) << 16 | u32::from(b[1]) << 8 | u32::from(b[2]);
        for i in 0..4 {
            if i <= chunk.len() {
                out.push(BASE64[(n >> (18 - 6 * i) & 63) as usize] as char);
            } else {
                out.push('=');
            }
        }
    }

    out
}

fn base64_decode(text: &[u8]) -> Option<Vec<u8>> {
    let value = |c: u8| BASE64.iter().position(|&b| b == c).map(|v| v as u32);
    let text = text.iter().copied().filter(|c| !c.is_ascii_whitespace()).collect::<Vec<_>>();
    if text.len() % 4 != 0 {
        return None;
    }

    let mut out = Vec::with_capacity(text.len() / 4 * 3);
    for quad in text.chunks(4) {
        let pad = quad.iter().rev().take_while(|&&c| c == b'=').count();
        if pad > 2 {
            return None;
        }

        let mut n = 0u32;
        for &c in &quad[..4 - pad] {
            n = n << 6 | value(c)?;
        }

        n <<= 6 * pad as u32;
        let bytes = [(n >> 16) as u8, (n >> 8) as u8, n as u8];
        out.extend_from_slice(&bytes[..3 - pad]);
    }

    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    const FILE: &str = "<INSTANCE_SELECTION version=\"17\" wce_version=\"1\">\n    <children>\n        <UserInstanceGroup flags=\" OIF_VISIBLE\" position=\"vector(0,0,0)\" name=\"rocks\" orientation=\"euler(0,0,0)\">\n            <children>\n                <BUILDING_INSTANCE position=\"vector(1,0,0)\" key=\"rock\" name=\"rock\"/>\n            </children>\n        </UserInstanceGroup>\n    </children>\n</INSTANCE_SELECTION>\n";

    fn mound() -> PrefabTerrain {
        let mut t = PrefabTerrain::empty([-5.0, -5.0], 0.5, 21, 21);
        for z in 6..13 {
            for x in 4..15 {
                let i = t.index(x, z);
                t.heights[i] = (x as f32 - 4.0) * 0.25;
                t.coverage[i] = if x == 4 { 0.5 } else { 1.0 };
            }
        }

        t.normalize = true;
        t.feather = 2.5;
        t
    }

    #[test]
    fn base64_round_trips() {
        for n in 0..10 {
            let bytes = (0..n).map(|i| (i * 37 + 11) as u8).collect::<Vec<_>>();
            assert_eq!(base64_decode(base64_encode(&bytes).as_bytes()).unwrap(), bytes);
        }

        assert_eq!(base64_encode(b"Man"), "TWFu");
        assert_eq!(base64_encode(b"Ma"), "TWE=");
        assert!(base64_decode(b"TWF").is_none());
    }

    #[test]
    fn a_layer_crops_and_round_trips_in_the_file() {
        let t = mound();
        let c = t.cropped().unwrap();
        assert_eq!((c.width, c.height, c.origin), (11, 7, [-3.0, -2.0]));
        assert_eq!((c.height_at(0, 0), c.coverage_at(0, 0), c.height_at(10, 6)), (0.0, 0.5, 2.5));

        let text = with_terrain(FILE.as_bytes().to_vec(), Some(&t)).unwrap();
        let s = String::from_utf8(text.clone()).unwrap();
        assert!(s.starts_with(&FILE[..FILE.len() - "</INSTANCE_SELECTION>\n".len()]) && s.ends_with("\"/>\n</INSTANCE_SELECTION>\n"), "{s}");
        assert!(s.contains("\n    <WceTerrain origin=\"-3,-2\" spacing=\"0.5\" size=\"11,7\" normalize=\"true\" feather=\"2.5\" samples=\""), "{s}");
        assert_eq!(read(&text).unwrap().unwrap(), c);

        // Saved again: one element, the same text.
        assert_eq!(with_terrain(text.clone(), Some(&c)).unwrap(), text);

        // Taken away: the file as it was.
        assert_eq!(with_terrain(text, None).unwrap(), FILE.as_bytes());
    }

    #[test]
    fn a_prefab_without_a_layer_stays_as_it_is() {
        assert_eq!(read(FILE.as_bytes()).unwrap(), None);
        assert_eq!(with_terrain(FILE.as_bytes().to_vec(), None).unwrap(), FILE.as_bytes());
        let empty = PrefabTerrain::empty([0.0, 0.0], 1.0, 4, 4);
        assert!(empty.is_empty() && empty.cropped().is_none());
        assert_eq!(with_terrain(FILE.as_bytes().to_vec(), Some(&empty)).unwrap(), FILE.as_bytes());
    }

    #[test]
    fn a_damaged_layer_says_so() {
        let bad = FILE.replace("</INSTANCE_SELECTION>", "    <WceTerrain origin=\"0,0\" spacing=\"1\" size=\"2,2\" samples=\"AAAA\"/>\n</INSTANCE_SELECTION>");
        assert!(read(bad.as_bytes()).is_err());
        let no_size = FILE.replace("</INSTANCE_SELECTION>", "    <WceTerrain origin=\"0,0\" spacing=\"1\" samples=\"\"/>\n</INSTANCE_SELECTION>");
        assert!(read(no_size.as_bytes()).unwrap_err().contains("size"));
    }
}
