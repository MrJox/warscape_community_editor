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
//!
//! In a tile ([`PrefabTerrain::rasterize`]) a layer turns with its group about the vertical (pitch and roll are not the ground's) and is
//! sampled bilinearly onto the tile's height map, as what a height layer of the tile adds there: off, the heights; on,
//! `weight * (plain + height - below)`, which leaves `below` where the weight is 0 and makes it `plain + height` where it is 1. The weight
//! is the coverage, and past what is covered the coverage of the nearest covered sample falling smoothly to 0 over the feather. A
//! prefab's nested prefabs go first, then its own layer, which was sculpted over theirs.

use std::collections::BTreeMap;

use wce_formats::png_file::{deflate_bytes, inflate_bytes};
use wce_formats::xml_doc::XmlDoc;
use wce_foundation::math::{Double3, Double3x3};

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

/// Where a prefab is: its group's origin in the scene (x, z) and its turn about the vertical.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Placement {
    pub x: f64,
    pub z: f64,

    /// The turn as `Double3x3::rotation_y` makes it: local x goes to (cos, -sin), local z to (sin, cos).
    cos: f64,
    sin: f64,
}

impl Placement {
    /// At a point, turned by `yaw` (radians, as `Double3x3::rotation_y`).
    #[must_use]
    pub fn new(x: f64, z: f64, yaw: f64) -> Self {
        Self { x, z, cos: yaw.cos(), sin: yaw.sin() }
    }

    /// From a group's world transform: the turn about the vertical its rotation makes of local x (or of local z, when x points up).
    #[must_use]
    pub fn from_transform(position: Double3, rotation: &Double3x3) -> Self {
        let (x, z) = (rotation.col(0), rotation.col(2));
        let (cos, sin) = if x.x.hypot(x.z) > 1e-6 { (x.x, -x.z) } else { (z.z, z.x) };
        let n = cos.hypot(sin);
        if n < 1e-9 {
            return Self::new(position.x, position.z, 0.0);
        }

        Self { x: position.x, z: position.z, cos: cos / n, sin: sin / n }
    }

    /// A point of the prefab (x, z from its origin) in the scene.
    #[must_use]
    pub fn to_scene(&self, lx: f64, lz: f64) -> (f64, f64) {
        (self.x + self.cos * lx + self.sin * lz, self.z - self.sin * lx + self.cos * lz)
    }

    /// A point of the scene in the prefab.
    #[must_use]
    pub fn to_local(&self, sx: f64, sz: f64) -> (f64, f64) {
        let (dx, dz) = (sx - self.x, sz - self.z);
        (self.cos * dx - self.sin * dz, self.sin * dx + self.cos * dz)
    }

    /// A prefab placed in this one (`inner`, in this one's frame) in the scene.
    #[must_use]
    pub fn then(&self, inner: &Self) -> Self {
        let (x, z) = self.to_scene(inner.x, inner.z);
        Self { x, z, cos: self.cos * inner.cos - self.sin * inner.sin, sin: self.sin * inner.cos + self.cos * inner.sin }
    }
}

/// A tile's height map in scene units: where pixel (0, 0) is and how far apart pixels are. Its values are scene units already.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Grid {
    pub width: u32,
    pub height: u32,
    pub origin: [f64; 2],
    pub spacing: [f64; 2],
}

impl Grid {
    /// A tile's (`TileProject`): its pixels are world units (scene / unit scale) from -border.
    #[must_use]
    pub fn of(p: &crate::tile_project::TileProject) -> Option<Self> {
        let h = p.height.as_ref()?;
        let (u, b) = (p.unit_scale, p.border_m());
        Some(Self { width: h.width(), height: h.height(), origin: [-b * u, -b * u], spacing: [p.mpp_x() * u, p.mpp_z() * u] })
    }

    fn scene(&self, x: u32, y: u32) -> (f64, f64) {
        (self.origin[0] + f64::from(x) * self.spacing[0], self.origin[1] + f64::from(y) * self.spacing[1])
    }

    fn pixel(&self, sx: f64, sz: f64) -> (f64, f64) {
        ((sx - self.origin[0]) / self.spacing[0], (sz - self.origin[1]) / self.spacing[1])
    }
}

/// A grid of samples, read bilinearly with 0 outside.
fn bilinear(v: &[f32], w: u32, h: u32, u: f64, t: f64) -> f64 {
    if !(u > -1.0 && t > -1.0 && u < f64::from(w) && t < f64::from(h)) {
        return 0.0;
    }

    let (x0, y0) = (u.floor(), t.floor());
    let (fx, fy) = (u - x0, t - y0);
    let at = |x: f64, y: f64| {
        if x < 0.0 || y < 0.0 || x >= f64::from(w) || y >= f64::from(h) { 0.0 } else { f64::from(v[y as usize * w as usize + x as usize]) }
    };

    let top = at(x0, y0) * (1.0 - fx) + at(x0 + 1.0, y0) * fx;
    let bottom = at(x0, y0 + 1.0) * (1.0 - fx) + at(x0 + 1.0, y0 + 1.0) * fx;
    top * (1.0 - fy) + bottom * fy
}

/// 0 to 1 and back with a smooth start and end.
fn smooth(t: f64) -> f64 {
    let t = t.clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// The weights `normalize` gives: the layer's grid grown by `margin` samples a side, the coverage inside what is covered, and past it the
/// coverage of the nearest covered sample falling to 0 over `feather` samples.
struct Weights {
    margin: u32,
    width: u32,
    height: u32,
    values: Vec<f32>,
}

impl PrefabTerrain {
    fn weights(&self, feather: f64) -> Weights {
        let feather_px = (feather / self.spacing).max(0.0);
        let margin = feather_px.ceil() as u32;
        let (w, h) = (self.width + 2 * margin, self.height + 2 * margin);
        let n = w as usize * h as usize;

        // Nearest covered sample of each one (two passes over the grid, keeping the nearest a neighbour knows of).
        let mut nearest: Vec<Option<(u32, u32)>> = vec![None; n];
        for z in 0..self.height {
            for x in 0..self.width {
                if self.coverage_at(x, z) > 0.0 {
                    nearest[(z + margin) as usize * w as usize + (x + margin) as usize] = Some((x + margin, z + margin));
                }
            }
        }

        let d2 = |a: (u32, u32), x: u32, y: u32| (f64::from(a.0) - f64::from(x)).powi(2) + (f64::from(a.1) - f64::from(y)).powi(2);
        let look = |x: u32, y: u32, from: &[(i32, i32)], nearest: &mut Vec<Option<(u32, u32)>>| {
            let i = y as usize * w as usize + x as usize;
            for &(dx, dy) in from {
                let (nx, ny) = (x as i32 + dx, y as i32 + dy);
                if nx < 0 || ny < 0 || nx >= w as i32 || ny >= h as i32 {
                    continue;
                }

                if let Some(site) = nearest[ny as usize * w as usize + nx as usize]
                    && nearest[i].is_none_or(|cur| d2(site, x, y) < d2(cur, x, y))
                {
                    nearest[i] = Some(site);
                }
            }
        };

        for y in 0..h {
            for x in 0..w {
                look(x, y, &[(-1, 0), (0, -1), (-1, -1), (1, -1)], &mut nearest);
            }
        }

        for y in (0..h).rev() {
            for x in (0..w).rev() {
                look(x, y, &[(1, 0), (0, 1), (1, 1), (-1, 1)], &mut nearest);
            }
        }

        let mut values = vec![0.0f32; n];
        for y in 0..h {
            for x in 0..w {
                let i = y as usize * w as usize + x as usize;
                let Some(site) = nearest[i] else { continue };
                let c = f64::from(self.coverage_at(site.0 - margin, site.1 - margin));
                let d = d2(site, x, y).sqrt();
                values[i] = if d == 0.0 {
                    c as f32
                } else if feather_px > 0.0 {
                    (c * smooth(1.0 - d / feather_px)) as f32
                } else {
                    0.0
                };
            }
        }

        Weights { margin, width: w, height: h, values }
    }

    /// What the layer adds to each pixel of a tile's height map it reaches, placed `at`, added into `out` (by pixel). `ground` is the map
    /// below it, without `out`: what is in `out` already (a nested prefab's) counts as ground. `feather`: the reference's, else the layer's.
    pub fn rasterize(&self, at: &Placement, grid: &Grid, feather: Option<f64>, ground: &dyn Fn(u32, u32) -> f64, out: &mut BTreeMap<(u32, u32), f32>) {
        if grid.width == 0 || grid.height == 0 {
            return;
        }

        let below = |x: u32, y: u32, out: &BTreeMap<(u32, u32), f32>| ground(x, y) + out.get(&(x, y)).map_or(0.0, |&v| f64::from(v));
        let weights = self.normalize.then(|| self.weights(feather.unwrap_or(self.feather)));
        let margin = weights.as_ref().map_or(0, |w| w.margin);

        // The plain: the ground under the origin.
        let plain = {
            let (u, v) = grid.pixel(at.x, at.z);
            let (u, v) = (u.clamp(0.0, f64::from(grid.width - 1)), v.clamp(0.0, f64::from(grid.height - 1)));
            let (x0, y0) = (u.floor() as u32, v.floor() as u32);
            let (x1, y1) = ((x0 + 1).min(grid.width - 1), (y0 + 1).min(grid.height - 1));
            let (fx, fy) = (u - f64::from(x0), v - f64::from(y0));
            let top = below(x0, y0, out) * (1.0 - fx) + below(x1, y0, out) * fx;
            let bottom = below(x0, y1, out) * (1.0 - fx) + below(x1, y1, out) * fx;
            top * (1.0 - fy) + bottom * fy
        };

        // The pixels the layer (with its margin) can reach.
        let m = f64::from(margin) * self.spacing;
        let (lx0, lz0) = (self.origin[0] - m - self.spacing, self.origin[1] - m - self.spacing);
        let (lx1, lz1) = (self.origin[0] + f64::from(self.width) * self.spacing + m, self.origin[1] + f64::from(self.height) * self.spacing + m);
        let corners = [(lx0, lz0), (lx1, lz0), (lx0, lz1), (lx1, lz1)].map(|(x, z)| {
            let (sx, sz) = at.to_scene(x, z);
            grid.pixel(sx, sz)
        });

        let lo = |v: f64, n: u32| (v.floor().max(0.0) as u32).min(n);
        let hi = |v: f64, n: u32| (v.ceil().max(0.0) as u32 + 1).min(n);
        let (x0, x1) = (lo(corners.iter().map(|c| c.0).fold(f64::MAX, f64::min), grid.width), hi(corners.iter().map(|c| c.0).fold(f64::MIN, f64::max), grid.width));
        let (y0, y1) = (lo(corners.iter().map(|c| c.1).fold(f64::MAX, f64::min), grid.height), hi(corners.iter().map(|c| c.1).fold(f64::MIN, f64::max), grid.height));

        let mut writes = vec![];
        for y in y0..y1 {
            for x in x0..x1 {
                let (sx, sz) = grid.scene(x, y);
                let (lx, lz) = at.to_local(sx, sz);
                let (u, v) = ((lx - self.origin[0]) / self.spacing, (lz - self.origin[1]) / self.spacing);
                let h = bilinear(&self.heights, self.width, self.height, u, v);
                let add = match &weights {
                    None => h,
                    Some(w) => {
                        let weight = bilinear(&w.values, w.width, w.height, u + f64::from(w.margin), v + f64::from(w.margin));
                        if weight <= 0.0 { 0.0 } else { weight * (plain + h - below(x, y, out)) }
                    }
                };

                if add != 0.0 {
                    writes.push((x, y, add as f32));
                }
            }
        }

        for (x, y, v) in writes {
            *out.entry((x, y)).or_insert(0.0) += v;
        }
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

    /// A grid of 64 x 64 one unit apart from (0, 0).
    const GRID: Grid = Grid { width: 64, height: 64, origin: [0.0, 0.0], spacing: [1.0, 1.0] };

    #[test]
    fn a_placement_turns_as_the_groups_rotation() {
        let yaw = 0.7;
        let a = Placement::new(3.0, 4.0, yaw);
        let r = Double3x3::rotation_y(yaw);
        let b = Placement::from_transform(Double3::new(3.0, 9.0, 4.0), &r);
        let p = r * Double3::new(2.0, 0.0, -1.0);
        let (sx, sz) = a.to_scene(2.0, -1.0);
        assert!((sx - (3.0 + p.x)).abs() < 1e-12 && (sz - (4.0 + p.z)).abs() < 1e-12);
        assert!((b.cos - a.cos).abs() < 1e-12 && (b.sin - a.sin).abs() < 1e-12);
        let (lx, lz) = a.to_local(sx, sz);
        assert!((lx - 2.0).abs() < 1e-12 && (lz + 1.0).abs() < 1e-12);

        // Pitch and roll are left out; a nested one adds its turn.
        let tilted = Placement::from_transform(Double3::default(), &(r * Double3x3::rotation_x(0.3)));
        assert!((tilted.cos - a.cos).abs() < 1e-12 && (tilted.sin - a.sin).abs() < 1e-12);
        let both = a.then(&Placement::new(1.0, 0.0, 0.2));
        let direct = Placement::new(a.to_scene(1.0, 0.0).0, a.to_scene(1.0, 0.0).1, yaw + 0.2);
        assert!((both.x - direct.x).abs() < 1e-12 && (both.cos - direct.cos).abs() < 1e-12 && (both.sin - direct.sin).abs() < 1e-12);
    }

    #[test]
    fn off_it_adds_and_a_quarter_turn_lands_exactly() {
        // One sample 2 along local x.
        let mut t = PrefabTerrain::empty([0.0, 0.0], 1.0, 3, 1);
        (t.heights[2], t.coverage[2]) = (1.5, 1.0);
        let ground = |_: u32, _: u32| 7.0;

        let mut out = BTreeMap::new();
        t.rasterize(&Placement::new(20.0, 20.0, 0.0), &GRID, None, &ground, &mut out);
        assert_eq!(out.get(&(22, 20)), Some(&1.5));
        assert_eq!(out.len(), 1, "{out:?}");

        // A quarter turn: local x goes to -z.
        let mut out = BTreeMap::new();
        t.rasterize(&Placement::new(20.0, 20.0, std::f64::consts::FRAC_PI_2), &GRID, None, &ground, &mut out);
        assert!((out[&(20, 18)] - 1.5).abs() < 1e-5, "{out:?}");
        assert!(out.iter().filter(|(k, _)| **k != (20, 18)).all(|(_, v)| v.abs() < 1e-5), "{out:?}");
    }

    #[test]
    fn normalized_it_looks_as_on_the_stage_on_a_slope() {
        // A 9 x 9 covered square from -4 to 4, a 2 high step on its +x half.
        let mut t = PrefabTerrain::empty([-4.0, -4.0], 1.0, 9, 9);
        for z in 0..9 {
            for x in 0..9 {
                let i = t.index(x, z);
                t.coverage[i] = 1.0;
                t.heights[i] = if x >= 5 { 2.0 } else { 0.0 };
            }
        }

        (t.normalize, t.feather) = (true, 3.0);
        let ground = |x: u32, _: u32| 0.5 * f64::from(x);
        let mut out = BTreeMap::new();
        t.rasterize(&Placement::new(30.0, 30.0, 0.0), &GRID, None, &ground, &mut out);
        let at = |x: u32, y: u32| ground(x, y) + out.get(&(x, y)).map_or(0.0, |&v| f64::from(v));

        // The plain is the ground under the origin (15), the step 2 over it, wherever the slope was.
        for y in 26..=34 {
            assert!((at(26, y) - 15.0).abs() < 1e-4 && (at(30, y) - 15.0).abs() < 1e-4, "{} {}", at(26, y), at(30, y));
            assert!((at(34, y) - 17.0).abs() < 1e-4, "{}", at(34, y));
        }

        // Past the feather the ground is its own; within it, between.
        assert_eq!((at(22, 30), at(38, 30)), (11.0, 19.0));
        assert!(at(24, 30) > 12.0 && at(24, 30) < 15.0, "{}", at(24, 30));

        // The reference's feather wins: none, and the edge is sharp.
        let mut sharp = BTreeMap::new();
        t.rasterize(&Placement::new(30.0, 30.0, 0.0), &GRID, Some(0.0), &ground, &mut sharp);
        assert!(!sharp.contains_key(&(24, 30)) && sharp.contains_key(&(26, 30)), "{sharp:?}");
    }

    #[test]
    fn a_nested_layer_is_ground_for_the_one_over_it() {
        // A mound, then a normalized pad on top of it at the same origin: the pad's plain is the mound's top.
        let mut mound = PrefabTerrain::empty([-2.0, -2.0], 1.0, 5, 5);
        mound.heights.iter_mut().for_each(|h| *h = 3.0);
        mound.coverage.iter_mut().for_each(|c| *c = 1.0);
        let mut pad = PrefabTerrain::empty([0.0, 0.0], 1.0, 1, 1);
        (pad.coverage[0], pad.heights[0], pad.normalize, pad.feather) = (1.0, 1.0, true, 0.0);

        let ground = |_: u32, _: u32| 10.0;
        let mut out = BTreeMap::new();
        let at = Placement::new(10.0, 10.0, 0.0);
        mound.rasterize(&at, &GRID, None, &ground, &mut out);
        pad.rasterize(&at, &GRID, None, &ground, &mut out);
        assert!((out[&(10, 10)] - 4.0).abs() < 1e-5, "{out:?}");
        assert_eq!(out[&(11, 10)], 3.0);
    }

    #[test]
    fn a_damaged_layer_says_so() {
        let bad = FILE.replace("</INSTANCE_SELECTION>", "    <WceTerrain origin=\"0,0\" spacing=\"1\" size=\"2,2\" samples=\"AAAA\"/>\n</INSTANCE_SELECTION>");
        assert!(read(bad.as_bytes()).is_err());
        let no_size = FILE.replace("</INSTANCE_SELECTION>", "    <WceTerrain origin=\"0,0\" spacing=\"1\" samples=\"\"/>\n</INSTANCE_SELECTION>");
        assert!(read(no_size.as_bytes()).unwrap_err().contains("size"));
    }
}
