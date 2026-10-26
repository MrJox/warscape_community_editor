//! A battle map seen from one tile: the map's placements (a kit map filled as BOB does, or the campaign's tile map as the game ships it), where a
//! tile is placed, what is around a spot, and how a placed tile's own frame maps into another one's (docs/re/battle_maps.md).
//!
//! Frames. A tile's own frame is the one it is edited in: x along its columns, z along its rows with row 0 (of the tile database's cells) at
//! the +z edge, 128 units a cell. A map cell (x, y) has y growing down the map's image. A placement at origin (ox, oy) with rotation r puts
//! the tile point (u, v) = (x / 128, h - z / 128) at map cell `(ox, oy) + rot(u, v)`, with rot the continuous form of
//! `TILE_MAP::rotate_in_tile_space`: 0x20 (h - v, u), 0x40 (w - u, h - v), 0x80 (v, w - u).
//!
//! Heights. The game draws a battle's ground at `LF(x, z) * lf_height - vertical_offset + the tile's own height` (rigidterrain vs30_geom_new:
//! `y = t_low_res_height_map * g_height_map_lf_scale + hf * g_height_map_hf_scale - g_height_map_vertical_offset`); the shipped tiles'
//! `lf_influence_map.png` are 1 everywhere, and the map's LF texture is its `lf_heights.tif` (BOB's lf_height_map.dds differs from it in 237
//! of 46.7 million pixels on the campaign map).

use super::tile_db::TileDatabase;
use super::tile_list::TileList;
use super::tile_map::TileMap;

/// Map units (and editor units) per cell.
pub const CELL: f64 = 128.0;

/// RENDER_PARAMS of the battle tile database (`lf_height`, `vertical_offset`).
pub const LF_HEIGHT: f64 = 3000.0;
pub const VERTICAL_OFFSET: f64 = 20.0;

#[derive(Clone, Debug, PartialEq)]
pub struct MapPlacement {
    /// The tile's location (`terrain\tiles\battle\<set>\<size>`) and the variation's.
    pub location: String,
    pub variation: String,

    /// The origin cell.
    pub x: i32,
    pub y: i32,

    /// The tile's own size in cells (before the rotation).
    pub w: i32,
    pub h: i32,
    pub rotation: u16,
    pub flipped: bool,

    /// The climate the map gives its cells (an index into [`PlacedMap::climates`]: the database's climates; 0 also when none).
    pub climate: u8,

    /// The map names the tile without a variation (Rome 2's, version 6: the game picks one): `variation` is the tile's first, and the
    /// placement is any variation's.
    pub any_variation: bool,
}

impl MapPlacement {
    /// The map cells it covers: x0, y0, x1, y1 (exclusive).
    pub fn rect(&self) -> [i32; 4] {
        let (w, h) = if matches!(self.rotation, 0x20 | 0x80) { (self.h, self.w) } else { (self.w, self.h) };
        [self.x, self.y, self.x + w, self.y + h]
    }

    /// Its own frame to map cells.
    pub fn to_map(&self) -> Affine2 {
        // (x, z) -> (u, v) = (x / 128, h - z / 128).
        let uv = Affine2 { m: [[1.0 / CELL, 0.0, 0.0], [0.0, -1.0 / CELL, f64::from(self.h)]] };
        let (w, h) = (f64::from(self.w), f64::from(self.h));
        let rot = match self.rotation {
            0x20 => Affine2 { m: [[0.0, -1.0, h], [1.0, 0.0, 0.0]] },
            0x40 => Affine2 { m: [[-1.0, 0.0, w], [0.0, -1.0, h]] },
            0x80 => Affine2 { m: [[0.0, 1.0, 0.0], [-1.0, 0.0, w]] },
            _ => Affine2::IDENTITY,
        };

        let at = Affine2 { m: [[1.0, 0.0, f64::from(self.x)], [0.0, 1.0, f64::from(self.y)]] };
        at.then_after(&rot).then_after(&uv)
    }

    /// The same tile location and variation (case and separators ignored).
    pub fn is(&self, location: &str, variation: &str) -> bool {
        norm(&self.location) == norm(location) && (self.any_variation || self.variation.eq_ignore_ascii_case(variation))
    }
}

fn norm(s: &str) -> String {
    s.replace('/', "\\").trim_matches('\\').to_lowercase()
}

/// A 2D affine map: (x, y) -> (m00 x + m01 y + m02, m10 x + m11 y + m12).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Affine2 {
    pub m: [[f64; 3]; 2],
}

impl Affine2 {
    pub const IDENTITY: Self = Self { m: [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0]] };

    pub fn apply(&self, x: f64, y: f64) -> (f64, f64) {
        let m = &self.m;
        (m[0][0] * x + m[0][1] * y + m[0][2], m[1][0] * x + m[1][1] * y + m[1][2])
    }

    /// `self ∘ other`: other first.
    pub fn then_after(&self, other: &Self) -> Self {
        let (a, b) = (&self.m, &other.m);
        let mut m = [[0.0; 3]; 2];
        for (r, row) in m.iter_mut().enumerate() {
            for (c, v) in row.iter_mut().enumerate() {
                *v = a[r][0] * b[0][c] + a[r][1] * b[1][c] + if c == 2 { a[r][2] } else { 0.0 };
            }
        }

        Self { m }
    }

    pub fn inverse(&self) -> Self {
        let m = &self.m;
        let det = m[0][0] * m[1][1] - m[0][1] * m[1][0];
        let (a, b, c, d) = (m[1][1] / det, -m[0][1] / det, -m[1][0] / det, m[0][0] / det);
        Self { m: [[a, b, -(a * m[0][2] + b * m[1][2])], [c, d, -(c * m[0][2] + d * m[1][2])]] }
    }
}

/// A map's low-frequency heights, the 16-bit image (65535 = 1), row 0 at the map's top.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct LfField {
    pub w: u32,
    pub h: u32,
    pub values: Vec<u16>,
}

impl LfField {
    /// Bilinear at a map point in cells (`cells_w` x `cells_h` the map's size), in metres (`LF * lf_height - vertical_offset`); clamped at the
    /// edges.
    pub fn metres(&self, x: f64, y: f64, cells_w: u32, cells_h: u32) -> f64 {
        if self.w == 0 || self.h == 0 {
            return 0.0;
        }

        let px = (x * f64::from(self.w) / f64::from(cells_w)).clamp(0.0, f64::from(self.w - 1));
        let py = (y * f64::from(self.h) / f64::from(cells_h)).clamp(0.0, f64::from(self.h - 1));
        let (x0, y0) = (px.floor() as u32, py.floor() as u32);
        let (x1, y1) = ((x0 + 1).min(self.w - 1), (y0 + 1).min(self.h - 1));
        let (fx, fy) = (px - f64::from(x0), py - f64::from(y0));
        let v = |x: u32, y: u32| f64::from(self.values[(y * self.w + x) as usize]) / 65535.0;
        let top = v(x0, y0) * (1.0 - fx) + v(x1, y0) * fx;
        let bottom = v(x0, y1) * (1.0 - fx) + v(x1, y1) * fx;
        (top * (1.0 - fy) + bottom * fy) * LF_HEIGHT - VERTICAL_OFFSET
    }
}

/// A battle map's placements.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct PlacedMap {
    pub name: String,
    pub width: u32,
    pub height: u32,
    pub placements: Vec<MapPlacement>,
    pub lf: Option<LfField>,

    /// Names the database doesn't know (their size is taken as 1 x 1).
    pub unknown: Vec<String>,

    /// The database's climates by index (a placement's `climate`).
    pub climates: Vec<String>,
}

/// The database's climate names by index.
fn climate_names(db: &TileDatabase) -> Vec<String> {
    db.climates.iter().map(|c| c.name.clone()).collect()
}

/// `<tile location>\<variation>` split at the last separator.
fn split_name(name: &str) -> (String, String) {
    let n = name.replace('/', "\\");
    match n.rsplit_once('\\') {
        Some((a, b)) => (a.to_owned(), b.to_owned()),
        None => (n, String::new()),
    }
}

/// A name split into (location, variation) and the tile's size when the database knows it.
type SplitName = (String, String, Option<(i32, i32)>, bool);

/// Tile sizes and first variations by normalised location, and the split names already seen.
#[derive(Default)]
struct Sizes {
    by_location: std::collections::HashMap<String, (i32, i32)>,
    first_variation: std::collections::HashMap<String, String>,
    names: std::collections::HashMap<String, SplitName>,
}

impl Sizes {
    fn new(db: &TileDatabase) -> Self {
        Self {
            by_location: db.tiles.iter().map(|t| (norm(&t.location), (t.width, t.height))).collect(),
            first_variation: db.tiles.iter().filter_map(|t| Some((norm(&t.location), t.variations.first()?.location.clone()))).collect(),
            names: Default::default(),
        }
    }
}

impl PlacedMap {
    fn placement(&mut self, sizes: &mut Sizes, name: &str, (x, y): (i32, i32), flags: u16, flipped: bool, climate: u8) -> MapPlacement {
        if !sizes.names.contains_key(name) {
            // An older map (Rome 2's, version 6) names the tile alone: its first variation (the game's choice is not known).
            let ((location, variation), any) = match sizes.first_variation.get(&norm(name)) {
                Some(v) => ((name.replace('/', "\\"), v.clone()), true),
                None => (split_name(name), false),
            };

            let size = sizes.by_location.get(&norm(&location)).copied();
            sizes.names.insert(name.to_owned(), (location, variation, size, any));
        }

        let (location, variation, size, any_variation) = sizes.names[name].clone();
        if size.is_none() && !self.unknown.contains(&location) {
            self.unknown.push(location.clone());
        }

        let (w, h) = size.unwrap_or((1, 1));
        MapPlacement { location, variation, x, y, w, h, rotation: flags & 0xf0, flipped, climate, any_variation }
    }

    /// A placement's climate name (none when the map gives none the database knows).
    pub fn climate_of(&self, p: &MapPlacement) -> Option<&str> {
        self.climates.get(usize::from(p.climate)).map(String::as_str).filter(|c| !c.is_empty())
    }

    /// From a battle field (`terrain.tile_list`, or the fill of a kit map).
    pub fn from_tile_list(name: &str, list: &TileList, db: &TileDatabase, lf: Option<LfField>) -> Self {
        let mut m = Self { name: name.to_owned(), width: list.battle_size[0], height: list.battle_size[1], lf, climates: climate_names(db), ..Default::default() };
        let mut sizes = Sizes::new(db);
        for p in &list.placements {
            let n = list.names.get(p.name).map_or("", String::as_str);
            let pl = m.placement(&mut sizes, n, (i32::from(p.x), i32::from(p.y)), p.flags, p.flags & 4 != 0, p.climate);
            m.placements.push(pl);
        }

        m
    }

    /// From a saved tile map (the campaign's `tile_map.tiles`): the origin cells of both layers.
    pub fn from_tile_map(name: &str, map: &TileMap, db: &TileDatabase, lf: Option<LfField>) -> Self {
        let mut m = Self { name: name.to_owned(), width: map.width, height: map.height, lf, climates: climate_names(db), ..Default::default() };
        let mut sizes = Sizes::new(db);
        for layer in &map.layers {
            for c in layer.iter().filter(|c| c.index > 0) {
                let n = map.names.get(c.index as usize - 1).map_or("", String::as_str);
                let pl = m.placement(&mut sizes, n, (i32::from(c.origin[0]), i32::from(c.origin[1])), c.flags, c.flipped != 0, c.climate);
                m.placements.push(pl);
            }
        }

        m
    }

    /// The placements of a tile variation.
    pub fn find(&self, location: &str, variation: &str) -> Vec<usize> {
        (0..self.placements.len()).filter(|&i| self.placements[i].is(location, variation)).collect()
    }

    /// The placements of a tile with any variation.
    pub fn find_tile(&self, location: &str) -> Vec<usize> {
        (0..self.placements.len()).filter(|&i| norm(&self.placements[i].location) == norm(location)).collect()
    }

    /// The placements that overlap the cells `[x0, x1) x [y0, y1)`.
    pub fn in_rect(&self, r: [i32; 4]) -> Vec<usize> {
        (0..self.placements.len())
            .filter(|&i| {
                let p = self.placements[i].rect();
                p[0] < r[2] && r[0] < p[2] && p[1] < r[3] && r[1] < p[3]
            })
            .collect()
    }

    /// The LF height in metres at a map point in cells (0 without a field).
    pub fn lf_metres(&self, x: f64, y: f64) -> f64 {
        self.lf.as_ref().map_or(0.0, |f| f.metres(x, y, self.width, self.height))
    }

    /// The mean and range (max - min) of the LF over a cell rectangle, sampled every quarter cell.
    pub fn lf_stats(&self, r: [i32; 4]) -> (f64, f64) {
        let (mut sum, mut n, mut lo, mut hi) = (0.0, 0.0, f64::MAX, f64::MIN);
        let steps_x = ((r[2] - r[0]) * 4).max(1);
        let steps_y = ((r[3] - r[1]) * 4).max(1);
        for j in 0..=steps_y {
            for i in 0..=steps_x {
                let v = self.lf_metres(f64::from(r[0]) + f64::from(i) / 4.0, f64::from(r[1]) + f64::from(j) / 4.0);
                sum += v;
                n += 1.0;
                lo = lo.min(v);
                hi = hi.max(v);
            }
        }

        (sum / n, hi - lo)
    }
}

/// Where the open tile sits in a map: its origin cell and rotation (a placement of the map, or one the user chose).
#[derive(Clone, Debug, PartialEq)]
pub struct Spot {
    pub x: i32,
    pub y: i32,
    pub rotation: u16,
}

/// What surrounds the open tile at a spot.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Context {
    /// (placement index, its own frame -> the open tile's frame), the open tile's own placement excluded.
    pub neighbours: Vec<(usize, Affine2)>,

    /// The open tile's frame -> map cells.
    pub to_map: Option<Affine2>,

    /// The cells around the open tile within the radius (the neighbours overlap it and may stick out of it).
    pub window: [i32; 4],

    /// The open tile's cells.
    pub open: [i32; 4],

    /// The mean and range of the LF under the open tile (metres).
    pub lf_mean: f64,
    pub lf_range: f64,
}

impl PlacedMap {

    /// The tiles within `radius` cells of the open tile (`w` x `h` cells) placed at the spot: every placement that overlaps the window and not
    /// the open tile's own cells.
    pub fn context(&self, spot: &Spot, w: i32, h: i32, radius: i32) -> Context {
        let me = MapPlacement { location: String::new(), variation: String::new(), x: spot.x, y: spot.y, w, h, rotation: spot.rotation, flipped: false, climate: 0, any_variation: false };
        let r = me.rect();
        let window = [(r[0] - radius).max(0), (r[1] - radius).max(0), (r[2] + radius).min(self.width as i32), (r[3] + radius).min(self.height as i32)];

        let inside = self.in_rect(window);
        let to_map = me.to_map();
        let from_map = to_map.inverse();
        let neighbours = inside
            .into_iter()
            .filter(|&i| {
                let p = self.placements[i].rect();
                !(p[0] < r[2] && r[0] < p[2] && p[1] < r[3] && r[1] < p[3])
            })
            .map(|i| (i, from_map.then_after(&self.placements[i].to_map())))
            .collect();

        let (lf_mean, lf_range) = self.lf_stats(r);
        Context { neighbours, to_map: Some(to_map), window, open: r, lf_mean, lf_range }
    }
}

impl Context {

    /// The cells of `area` that neither a neighbour nor one of the `covered` rectangles (the open tile's, ...) covers, as rectangles (x0, y0, x1,
    /// y1 exclusive): rows of uncovered cells cut into runs, a run grown down while the rows below have the same run free.
    pub fn uncovered(&self, map: &PlacedMap, covered: &[[i32; 4]], area: [i32; 4]) -> Vec<[i32; 4]> {
        let (w, h) = ((area[2] - area[0]).max(0) as usize, (area[3] - area[1]).max(0) as usize);
        let mut taken = vec![false; w * h];
        let mut mark = |r: [i32; 4]| {
            for y in r[1].max(area[1])..r[3].min(area[3]) {
                for x in r[0].max(area[0])..r[2].min(area[2]) {
                    taken[(y - area[1]) as usize * w + (x - area[0]) as usize] = true;
                }
            }
        };

        for &r in covered {
            mark(r);
        }

        for (i, _) in &self.neighbours {
            mark(map.placements[*i].rect());
        }

        let mut out = vec![];
        for y in 0..h {
            let mut x = 0;
            while x < w {
                if taken[y * w + x] {
                    x += 1;
                    continue;
                }

                let x0 = x;
                while x < w && !taken[y * w + x] {
                    x += 1;
                }

                // Down while the same run is free.
                let mut y1 = y + 1;
                while y1 < h && (x0..x).all(|k| !taken[y1 * w + k]) {
                    y1 += 1;
                }

                for yy in y..y1 {
                    for k in x0..x {
                        taken[yy * w + k] = true;
                    }
                }

                out.push([area[0] + x0 as i32, area[1] + y as i32, area[0] + x as i32, area[1] + y1 as i32]);
            }
        }

        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn placed(x: i32, y: i32, w: i32, h: i32, rotation: u16) -> MapPlacement {
        MapPlacement { location: String::new(), variation: String::new(), x, y, w, h, rotation, flipped: false, climate: 0, any_variation: false }
    }

    #[test]
    fn rotations_follow_rotate_in_tile_space() {
        // The centre of tile cell (sx, sy) (row 0 at +z) lands in the map cell rotate_in_tile_space gives.
        for rotation in [0x10, 0x20, 0x40, 0x80] {
            let (w, h) = (3, 2);
            let p = placed(10, 20, w, h, rotation);
            for sy in 0..h {
                for sx in 0..w {
                    let (x, z) = ((f64::from(sx) + 0.5) * CELL, (f64::from(h - sy) - 0.5) * CELL);
                    let (mx, my) = p.to_map().apply(x, z);
                    let (dx, dy) = crate::battle_map::fill::rotate_in_tile_space(w, h, rotation, sx, sy);
                    assert_eq!((mx.floor() as i32, my.floor() as i32), (10 + dx, 20 + dy), "rotation {rotation:#x} cell ({sx}, {sy})");
                }
            }
        }
    }

    #[test]
    fn a_neighbour_east_is_one_tile_along_x() {
        let mut m = PlacedMap { width: 64, height: 64, ..Default::default() };
        m.placements.push(placed(10, 10, 2, 2, 0x10));
        m.placements.push(placed(12, 10, 2, 2, 0x10));
        m.placements.push(placed(10, 8, 2, 2, 0x10));
        let c = m.context(&Spot { x: 10, y: 10, rotation: 0x10 }, 2, 2, 4);
        assert_eq!(c.neighbours.len(), 2);
        // Everything of a 4-cell ring but the three tiles is uncovered, and the rectangles don't overlap them.
        let free = c.uncovered(&m, &[c.open], [6, 6, 18, 18]);
        let cells: i32 = free.iter().map(|r| (r[2] - r[0]) * (r[3] - r[1])).sum();
        assert_eq!(cells, 144 - 12);
        let east = c.neighbours.iter().find(|n| n.0 == 1).unwrap().1;
        assert_eq!(east.apply(0.0, 0.0), (256.0, 0.0));

        // North on the map (smaller y) is +z.
        let north = c.neighbours.iter().find(|n| n.0 == 2).unwrap().1;
        assert_eq!(north.apply(0.0, 0.0), (0.0, 256.0));
    }

    #[test]
    fn the_open_tile_rotated_keeps_its_own_frame() {
        let mut m = PlacedMap { width: 64, height: 64, ..Default::default() };
        m.placements.push(placed(12, 10, 1, 1, 0x10));
        // Open tile rotated by 0x20 at (10, 10), 2 x 2: the east neighbour's origin in the open frame.
        let c = m.context(&Spot { x: 10, y: 10, rotation: 0x20 }, 2, 2, 4);
        let a = c.neighbours[0].1;
        // The neighbour cell's corners map onto a 128 x 128 square of the open frame.
        let (p0, p1) = (a.apply(0.0, 0.0), a.apply(128.0, 128.0));
        assert!(((p0.0 - p1.0).abs() - 128.0).abs() < 1e-9 && ((p0.1 - p1.1).abs() - 128.0).abs() < 1e-9);
    }
}
