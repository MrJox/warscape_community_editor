//! Terry's battle catchment area generator (`TerrainMetadataEditor::BattleCatchmentAreaGenerator`, Thrones of Britannia's
//! TWeak_TerrainMetadataEditor fn 0x10062bc0; docs/re/terry.md, "The BCA generator"), the BCAs without their deployment zones:
//!
//! - the spec (`editor_working_data/Terry/bca_generation_spec.xml`, fn 0x1005fde0): globals, then per `<bca type boundary>` its
//!   `infield` and `infield_restriction` lists (the element's text split on white space: tile set names, tile locations under
//!   `terrain/tiles/battle/`, or `<tile location>/<variation>`) and its `deployment`;
//! - every tile instance of the map in its order, every BCA type in the spec's order: a tile in the type's infield (fn 0x100590c0) makes a
//!   BCA at the cell `(x + width / 2, y + height / 2)` (the tile's own size, whatever its turn), unless a BCA of that type is at that cell
//!   already or the cell is within `map_border_for_outfield` of the map's edge (fn 0x10061050); its name `BCA %1`, the BCAs made so
//!   far, five digits;
//! - a BCA whose 8 x 8 cells around its centre (-4 .. 3) hold another tile instance of the type's `infield_restriction` gets the error
//!   "Infield contains restricted tiles" (fn 0x10059c00); a cell's tile is the first instance whose unmasked subtiles cover it (fn
//!   0x10092750); with `delete_invalid` a BCA with an error is removed at the end (its number stays used).
//!
//! A BCA's entity (fn 0x10007ee0): an `ECRectangle` of 8 unit tiles (2048 m), its `Catchment Region` child `boundary` unit tiles larger.

pub mod zones;

use std::collections::{HashMap, HashSet};

use wce_tile::battle_map::fill::{Placement, rotate_in_tile_space};
use wce_tile::battle_map::tile_db::TileDatabase;
use wce_tile::vegetation::ca_xml;

/// A BCA type's deployment zones (`<deployment category split dual_config>` and its `restriction` list).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct DeploymentSpec {
    pub category: String,
    pub split: bool,
    pub dual_config: bool,
    pub restriction: HashSet<String>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct BcaType {
    pub name: String,
    pub boundary: i32,
    pub infield: HashSet<String>,
    pub restriction: HashSet<String>,
    pub deployment: Option<DeploymentSpec>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Spec {
    pub deployment_min_area: f32,
    pub deployment_split_width: f32,
    pub deployment_coarseness: f32,
    pub map_border_for_outfield: i32,
    pub delete_invalid: bool,

    /// `<debug x y>`: only the BCA centred there is made.
    pub debug: Option<(i32, i32)>,
    pub types: Vec<BcaType>,
}

/// An element's text as a set of words (Terry's `text().trimmed().split("\s+")`).
fn words(e: Option<&ca_xml::Element>) -> HashSet<String> {
    e.map(|e| e.text.split_whitespace().map(str::to_owned).collect()).unwrap_or_default()
}

impl Spec {
    pub fn parse(bytes: &[u8]) -> Self {
        // The kit's spec starts with a UTF-8 byte order mark (Terry's Qt reader skips it).
        let bytes = bytes.strip_prefix(b"\xef\xbb\xbf").unwrap_or(bytes);
        let root = ca_xml::parse(bytes);
        let root = if root.name == "root" { root } else { root.children.into_iter().find(|c| c.name == "root").unwrap_or_default() };
        let num = |e: &ca_xml::Element, a: &str| e.attribute(a).and_then(|v| v.trim().parse::<f32>().ok()).unwrap_or(0.0);
        let int = |e: &ca_xml::Element, a: &str| e.attribute(a).and_then(|v| v.trim().parse::<i32>().ok()).unwrap_or(0);
        let mut s = Self::default();
        if let Some(g) = root.child("global") {
            s.deployment_min_area = num(g, "deployment_min_area");
            s.deployment_split_width = num(g, "deployment_split_width");
            s.deployment_coarseness = num(g, "deployment_coarseness");
            s.map_border_for_outfield = int(g, "map_border_for_outfield");
            s.delete_invalid = g.attribute("delete_invalid") == Some("true");
        }

        s.debug = root.child("debug").map(|d| (int(d, "x"), int(d, "y")));
        for b in root.children.iter().filter(|c| c.name == "bca") {
            let deployment = b.child("deployment").map(|d| DeploymentSpec {
                category: d.attribute("category").unwrap_or_default().to_owned(),
                split: d.attribute("split") == Some("true"),
                dual_config: d.attribute("dual_config") == Some("true"),
                restriction: words(d.child("restriction")),
            });

            s.types.push(BcaType {
                name: b.attribute("type").unwrap_or_default().to_owned(), boundary: int(b, "boundary"), infield: words(b.child("infield")),
                restriction: words(b.child("infield_restriction")), deployment,
            });
        }

        s
    }
}

/// Whether a tile instance is in a list (fn 0x100590c0): its tile set's name, its tile's location under `terrain/tiles/battle/`, or that
/// and `/<variation>`.
pub fn in_list(db: &TileDatabase, list: &HashSet<String>, p: &Placement) -> bool {
    if list.is_empty() {
        return false;
    }

    let tile = &db.tiles[p.tile];
    if tile.set.is_some_and(|s| list.contains(&db.sets[s].name)) {
        return true;
    }

    let location = tile.location.replace('\\', "/");
    let rel = location.strip_prefix("terrain/tiles/battle/").unwrap_or(&location);
    if list.contains(rel) {
        return true;
    }

    tile.variations.get(p.variation).is_some_and(|v| list.contains(&format!("{rel}/{}", v.location)))
}

/// A variation's deployment mask (`<tile>/<variation>/deployment_mask.png` in a kit's `raw_data`, fn 0x1005cfc0 / 0x1005b090): per key
/// its pixels' squares in 1/128 cells of the unturned tile (x0, y0, x1, y1 inclusive), the image mirrored vertically; key 1 the pixels
/// whose luminance `(11 R + 16 G + 5 B) >> 5` is above 25, key -1 those above 76.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct DeploymentMask {
    pub regions: Vec<(i32, Vec<(i32, i32, i32, i32)>)>,
}

impl DeploymentMask {
    /// From an image's RGB pixels, row by row, for a tile of `tile_w` x `tile_h` cells: none when Terry skips it (more than 128 pixels a
    /// cell) or asserts (the sides' pixels a cell differ, or 128 is no multiple of them).
    pub fn from_rgb(w: u32, h: u32, rgb: &[[u8; 3]], tile_w: i32, tile_h: i32) -> Option<Self> {
        if tile_w <= 0 || tile_h <= 0 || rgb.len() != (w * h) as usize {
            return None;
        }

        let ppt = w as i32 / tile_w;
        if ppt == 0 || ppt != h as i32 / tile_h || ppt > zones::UNITS || zones::UNITS % ppt != 0 {
            return None;
        }

        let s = zones::UNITS / ppt;
        let (mut grey, mut white) = (vec![], vec![]);
        for iy in 0..h as i32 {
            let row = h as i32 - 1 - iy;
            for ix in 0..w as i32 {
                let [r, g, b] = rgb[(row * w as i32 + ix) as usize];
                let l = (i32::from(r) * 11 + i32::from(g) * 16 + i32::from(b) * 5) >> 5;
                let rect = (ix * s, iy * s, ix * s + s - 1, iy * s + s - 1);
                if l > 25 {
                    grey.push(rect);
                }

                if l > 76 {
                    white.push(rect);
                }
            }
        }

        let mut regions = vec![];
        if !grey.is_empty() {
            regions.push((1, grey));
        }

        if !white.is_empty() {
            regions.push((-1, white));
        }

        Some(Self { regions })
    }

    /// A key's squares (none: empty).
    pub fn key(&self, k: i32) -> &[(i32, i32, i32, i32)] {
        self.regions.iter().find(|r| r.0 == k).map_or(&[], |r| r.1.as_slice())
    }
}

/// The deployment masks of the variations a battle field places, from the first of `raw_roots` (kits' `raw_data`, in the data source
/// order) that has one.
pub fn load_masks(db: &TileDatabase, instances: &[Placement], raw_roots: &[std::path::PathBuf]) -> HashMap<(usize, usize), DeploymentMask> {
    let mut out = HashMap::new();
    let mut tried = HashSet::new();
    for p in instances {
        if !tried.insert((p.tile, p.variation)) {
            continue;
        }

        let t = &db.tiles[p.tile];
        let Some(v) = t.variations.get(p.variation) else { continue };
        let rel = std::path::PathBuf::from(t.location.replace('\\', "/")).join(&v.location).join("deployment_mask.png");
        let Some(bytes) = raw_roots.iter().find_map(|r| std::fs::read(r.join(&rel)).ok()) else { continue };
        let Ok(png) = wce_formats::png_file::PngFile::parse(&bytes) else { continue };
        let bpp = png.bytes_per_pixel();
        if png.bit_depth != 8 || !(bpp == 3 || bpp == 4) {
            continue;
        }

        let rgb: Vec<[u8; 3]> = png.pixels.chunks_exact(bpp).map(|c| [c[0], c[1], c[2]]).collect();
        if let Some(m) = DeploymentMask::from_rgb(png.width, png.height, &rgb, t.width, t.height) {
            out.insert((p.tile, p.variation), m);
        }
    }

    out
}

/// A generated BCA.
#[derive(Clone, Debug, PartialEq)]
pub struct Bca {
    pub name: String,
    pub kind: String,

    /// Its centre cell.
    pub cell: (i32, i32),

    /// Its type's index in the spec, the tile instance that made it.
    pub type_index: usize,
    pub instance: usize,
    pub errors: Vec<String>,

    /// Its deployment zones, in the order Terry makes them.
    pub zones: Vec<Dz>,
}

/// A deployment zone (an `ECDeploymentZone` entity under its BCA, fn 0x100619c0): "DZ <configuration>/<alliance>".
#[derive(Clone, Debug, PartialEq)]
pub struct Dz {
    pub name: String,
    pub configuration: i32,
    pub alliance: i32,
    pub category: String,
    pub regions: Vec<Dzr>,
}

/// A deployment zone region (`ECDeploymentZoneRegion` and `ECPolyline`, fn 0x10061320): "DZR <n>"; its points in cells (f32, as
/// Terry's polyline vertices before they become metres), y south.
#[derive(Clone, Debug, PartialEq)]
pub struct Dzr {
    pub name: String,
    pub subtractive: bool,
    pub points: Vec<(f32, f32)>,
}

/// A zone's regions: its outline (if any) then its holes', depth first; additive with a positive area (fn 0x10061320).
fn regions(z: &zones::Polygon, centre: (i32, i32), out: &mut Vec<Dzr>) {
    if !z.points.is_empty() {
        out.push(Dzr {
            name: format!("DZR {}", out.len()), subtractive: z.area() < 0.0,
            points: z.points.iter().map(|&(x, y)| (centre.0 as f32 + x as f32, centre.1 as f32 + y as f32)).collect(),
        });
    }

    for c in &z.children {
        regions(c, centre, out);
    }
}

/// A BCA's deployment zones for its type (fn 0x10062850): one configuration (0) of two alliances, or with `dual_config` two, the
/// second's alliances swapped when its zones face the other way (their centroids' axes' dot product below 0).
#[allow(clippy::too_many_arguments)]
fn deploy(
    spec: &Spec, db: &TileDatabase, w: u32, h: u32, instances: &[Placement], cells: &[i32], masks: &HashMap<(usize, usize), DeploymentMask>,
    b: &mut Bca, dep: &DeploymentSpec,
) {
    let inset = zones::battle_inset(2.0);
    let mut make = |config: i32| {
        let (z, e) = zones::zones(spec, dep, db, w, h, instances, cells, masks, config, b.cell, inset);
        b.errors.extend(e);
        let mut z = z.into_iter();
        (z.next(), z.next())
    };

    let order: Vec<(i32, i32, Option<zones::Polygon>)> = if dep.dual_config {
        let (a0, a1) = make(0);
        let (mut b0, mut b1) = make(1);
        if let (Some(p), Some(q), Some(r), Some(s)) = (&a0, &a1, &b0, &b1) {
            let c = |z: &zones::Polygon| {
                let (a, mx, my) = z.moments();
                let k = 1.0 / f64::from(a);
                (k * mx, my * k)
            };

            let (d0, d1) = ((c(q).0 - c(p).0, c(q).1 - c(p).1), (c(s).0 - c(r).0, c(s).1 - c(r).1));
            if ((d1.0 * d0.0 + d1.1 * d0.1) as f32) < 0.0 {
                std::mem::swap(&mut b0, &mut b1);
            }
        }

        vec![(0, 0, a0), (0, 1, b1), (1, 0, a1), (1, 1, b0)]
    } else {
        let (a0, a1) = make(-1);
        vec![(0, 0, a0), (0, 1, a1)]
    };

    for (configuration, alliance, z) in order {
        let Some(z) = z else { continue };
        let name = format!("DZ {configuration}/{alliance}");
        if z.area() < spec.deployment_min_area {
            b.errors.push(format!("{name} is too small"));
        }

        let mut rs = vec![];
        regions(&z, b.cell, &mut rs);
        b.zones.push(Dz { name, configuration, alliance, category: dep.category.clone(), regions: rs });
    }
}

/// The map's cells' tile instances (fn 0x10092750): each the first instance whose unmasked subtiles cover it (none: -1).
pub fn cell_instances(db: &TileDatabase, w: u32, h: u32, instances: &[Placement]) -> Vec<i32> {
    let mut out = vec![-1i32; (w * h) as usize];
    for (i, p) in instances.iter().enumerate() {
        let t = &db.tiles[p.tile];
        let also = t.set.is_some_and(|s| !db.sets[s].also_place_tile_set.is_empty());
        for sy in 0..t.height {
            for sx in 0..t.width {
                if !t.subtile_masked_valid(sx, sy) {
                    continue;
                }

                let (dx, dy) = rotate_in_tile_space(t.width, t.height, p.flags & 0xf0, sx, sy);
                let (x, y) = (i32::from(p.x) + dx, i32::from(p.y) + dy);
                if x < 0 || y < 0 || x >= w as i32 || y >= h as i32 {
                    continue;
                }

                let c = &mut out[(y as u32 * w + x as u32) as usize];
                if *c == -1 || also {
                    *c = i as i32;
                }
            }
        }
    }

    out
}

/// The BCAs of a map (`w` x `h` cells, its tile instances in the battle field's order), in the order Terry makes them; with
/// `delete_invalid` the ones with an error are still listed (their `errors` say why they go).
pub fn generate(
    spec: &Spec, db: &TileDatabase, w: u32, h: u32, instances: &[Placement], masks: &HashMap<(usize, usize), DeploymentMask>,
) -> Vec<Bca> {
    let cells = cell_instances(db, w, h, instances);
    let mut out: Vec<Bca> = vec![];
    let mut at: HashMap<(i32, i32), Vec<usize>> = HashMap::new();
    let border = spec.map_border_for_outfield;
    for (i, p) in instances.iter().enumerate() {
        for (k, t) in spec.types.iter().enumerate() {
            if !in_list(db, &t.infield, p) {
                continue;
            }

            let tile = &db.tiles[p.tile];
            let c = (i32::from(p.x) + tile.width / 2, i32::from(p.y) + tile.height / 2);
            if spec.debug.is_some_and(|d| d != c) {
                continue;
            }

            if at.get(&c).is_some_and(|v| v.iter().any(|&b| out[b].kind == t.name)) {
                continue;
            }

            if c.0 < border || c.0 >= w as i32 - border || c.1 < border || c.1 >= h as i32 - border {
                continue;
            }

            let mut b = Bca { name: format!("BCA {:05}", out.len()), kind: t.name.clone(), cell: c, type_index: k, instance: i, errors: vec![], zones: vec![] };
            let restricted = (-4..4).any(|dy| (-4..4).any(|dx| {
                let (x, y) = (c.0 + dx, c.1 + dy);
                if x < 0 || y < 0 || x >= w as i32 || y >= h as i32 {
                    return false;
                }

                let j = cells[(y as u32 * w + x as u32) as usize];
                j >= 0 && j as usize != i && in_list(db, &t.restriction, &instances[j as usize])
            }));

            if restricted {
                b.errors.push("Infield contains restricted tiles".to_owned());
            }

            at.entry(c).or_default().push(out.len());
            out.push(b);
        }
    }

    for b in &mut out {
        if let Some(dep) = spec.types[b.type_index].deployment.as_ref().filter(|d| !d.category.is_empty()) {
            deploy(spec, db, w, h, instances, &cells, masks, b, dep);
        }
    }

    out
}
