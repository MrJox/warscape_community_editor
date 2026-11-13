//! The campaign's battle tile map (`terrain/battles/main_attila_map/tile_map.index` + `tile_map.tiles`), as BOB's Terrain processor saves it
//! (`TILE_MAP::save`, 0x102e3770) and the game reads it (fn 0x118ffb60). Little endian, version 9:
//!
//! - `tile_map.index`: u32 version, u32 width, u32 height (cells);
//! - `tile_map.tiles`: the tile layer then the geometry layer, width x height cells of 20 bytes each, row by row (i32 index: 0 empty,
//!   positive the 1-based name of the tile placed with its origin here, negative -(origin cell + 1); u16 flags (rotation 0x10 .. 0x80,
//!   0x700 layers; an empty cell has 0x10); u16 x 2 the origin cell; u8 climate; u8 flipped (the river flow, `calculate_flow`); f32 x 2
//!   the low-frequency height range); then u32 name count and the names (u16 length + bytes, `<tile location>\<variation location>`).
//!
//! The game also reads versions 6 and 7 (the kit's `main_attila_map/tile_map.tiles` is an old file of that kind; Rome 2's
//! `main_rome_map` is version 6): `tile_map.index` adds a chunk count and the chunks' cell rectangles (x0, y0, x1, y1: 736 of them,
//! 128 + 16 cells a side, 64 apart); a cell is 16 bytes: i32 index, u16 flags, u16 x 2 the origin cell (as version 9), i16 (-1 mostly;
//! not known), u8 climate, u8 flipped, u16 0. No low-frequency height range: read as 0. terrain.pack's (version 9) was built from other
//! sources than the kit's: BOB's fill of the kit map (exact in `battle_map_check`) differs from it.

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct TileMapCell {
    pub index: i32,
    pub flags: u16,
    pub origin: [u16; 2],
    pub climate: u8,
    pub flipped: u8,
    pub lf: [f32; 2],

    /// Versions 6 and 7: the cell's bytes 10, 11 (an i16, -1 mostly), 14 and 15 (unknown).
    pub old: [u8; 4],
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct TileMap {
    pub version: u32,
    pub width: u32,
    pub height: u32,
    pub layers: [Vec<TileMapCell>; 2],
    pub names: Vec<String>,

    /// Versions 6 and 7: `tile_map.index` after its first 12 bytes (the chunk count and rectangles), verbatim.
    pub chunks: Vec<u8>,
}

const CELL: usize = 20;

/// A cell of versions 6 and 7.
const OLD_CELL: usize = 16;

fn u16_at(b: &[u8], p: usize) -> u16 {
    u16::from_le_bytes([b[p], b[p + 1]])
}

fn u32_at(b: &[u8], p: usize) -> u32 {
    u32::from_le_bytes([b[p], b[p + 1], b[p + 2], b[p + 3]])
}

impl TileMap {
    /// # Errors
    ///
    /// Fails on a version before 6 or a truncated file.
    pub fn parse(index: &[u8], tiles: &[u8]) -> Result<Self, String> {
        if index.len() < 12 {
            return Err("tile_map.index: truncated".to_owned());
        }

        let (version, width, height) = (u32_at(index, 0), u32_at(index, 4), u32_at(index, 8));
        if version < 6 {
            return Err(format!("tile_map.index: version {version} (only 6 and later read)"));
        }

        // Versions 6 and 7: 16-byte cells.
        let size = if version < 8 { OLD_CELL } else { CELL };
        let n = width as usize * height as usize;
        let mut p = 2 * n * size;
        if tiles.len() < p + 4 {
            return Err("tile_map.tiles: truncated".to_owned());
        }

        let cell = |o: usize| {
            let (index, flags, origin) = (u32_at(tiles, o) as i32, u16_at(tiles, o + 4), [u16_at(tiles, o + 6), u16_at(tiles, o + 8)]);
            if version < 8 {
                TileMapCell {
                    index,
                    flags,
                    origin,
                    climate: tiles[o + 12],
                    flipped: tiles[o + 13],
                    lf: [0.0; 2],
                    old: [tiles[o + 10], tiles[o + 11], tiles[o + 14], tiles[o + 15]],
                }
            } else {
                TileMapCell {
                    index,
                    flags,
                    origin,
                    climate: tiles[o + 10],
                    flipped: tiles[o + 11],
                    lf: [f32::from_bits(u32_at(tiles, o + 12)), f32::from_bits(u32_at(tiles, o + 16))],
                    old: [0; 4],
                }
            }
        };

        let layers = [(0..n).map(|i| cell(i * size)).collect(), (0..n).map(|i| cell((n + i) * size)).collect()];
        let count = u32_at(tiles, p) as usize;
        p += 4;
        let mut names = Vec::with_capacity(count);
        for _ in 0..count {
            let len = usize::from(u16_at(tiles, p));
            let s = tiles.get(p + 2..p + 2 + len).ok_or("tile_map.tiles: truncated name")?;
            names.push(s.iter().map(|&c| char::from(c)).collect());
            p += 2 + len;
        }

        let chunks = if version < 8 { index[12..].to_vec() } else { vec![] };
        Ok(Self { version, width, height, layers, names, chunks })
    }

    /// The two files: (`tile_map.index`, `tile_map.tiles`): versions 6 and 7 as read (their chunk table and cells' unknown bytes kept),
    /// any other as version 9.
    pub fn write(&self) -> (Vec<u8>, Vec<u8>) {
        let old = matches!(self.version, 6 | 7);
        let mut index = vec![];
        for v in [if old { self.version } else { 9 }, self.width, self.height] {
            index.extend(v.to_le_bytes());
        }

        if old {
            index.extend_from_slice(&self.chunks);
        }

        let mut o = Vec::with_capacity(2 * self.layers[0].len() * if old { OLD_CELL } else { CELL });
        for c in self.layers.iter().flatten() {
            o.extend(c.index.to_le_bytes());
            o.extend(c.flags.to_le_bytes());
            o.extend(c.origin[0].to_le_bytes());
            o.extend(c.origin[1].to_le_bytes());
            if old {
                o.extend([c.old[0], c.old[1], c.climate, c.flipped, c.old[2], c.old[3]]);
            } else {
                o.extend([c.climate, c.flipped]);
                o.extend(c.lf[0].to_le_bytes());
                o.extend(c.lf[1].to_le_bytes());
            }
        }

        o.extend((self.names.len() as u32).to_le_bytes());
        for n in &self.names {
            o.extend((n.len() as u16).to_le_bytes());
            o.extend(n.bytes());
        }

        (index, o)
    }
}

impl TileMap {
    /// A version 6 or 7 tile map's placements as a battle field (Rome 2's form: version 8, a placement's byte 12 its variation): the
    /// origin cells of the tile layer, row by row; name `index - 1`, the placement's origin and flags, the cell's flip byte (0 .. 2: the
    /// river's flow) in `old[0]`. [`Self::with_placements`] makes the cells again from it.
    pub fn placements(&self) -> super::tile_list::TileList {
        use super::tile_list::{TileList, TileListPlacement};
        let placements = self.layers[0]
            .iter()
            .filter(|c| c.index > 0)
            .map(|c| TileListPlacement {
                name: (c.index - 1) as usize,
                x: c.origin[0] as i16,
                y: c.origin[1] as i16,
                flags: c.flags,
                climate: c.climate,
                lf: [0.0; 2],
                old: [c.flipped, 0, 0],
            })
            .collect();

        TileList { version: 8, battle_size: [self.width, self.height], names: self.names.clone(), placements, ..Default::default() }
    }

    /// The tile map of a placement view (versions 6 and 7; Rome 2's): the tile layer's cells made from the placements as BOB's
    /// `place_tile` covers a tile (the first valid cell, turned, the origin: the name, the flags, the variation, and the number of the
    /// 64-cell chunk it is in; the others `-(origin + 1)`, turn 0x10, the same variation and flip, -1 in bytes 10, 11); empty cells as the
    /// game's. The geometry layer, the chunk table and the version stay this map's; so do a cell's bytes 14, 15 (unknown: 0 but a few on
    /// `prologue_map_01`) where it is otherwise as this map has it, and a cell empty in both. Tiles the database lacks cover their origin cell only.
    pub fn with_placements(&self, list: &super::tile_list::TileList, db: &super::tile_db::TileDatabase) -> Self {
        use super::fill::rotate_in_tile_space;
        let (w, h) = (self.width as i32, self.height as i32);
        let cols = self.width.div_ceil(64);
        let empty = TileMapCell { index: 0, flags: 0x10, origin: [0, 0], climate: 0, flipped: 0, lf: [0.0; 2], old: [255, 255, 0, 0] };
        let mut layer = vec![empty; (self.width * self.height) as usize];
        for p in &list.placements {
            let name = list.names.get(p.name).map_or("", String::as_str).replace('/', "\\");
            let tile = db.tiles.iter().find(|t| t.location.eq_ignore_ascii_case(&name));
            let (tw, th) = tile.map_or((1, 1), |t| (t.width, t.height));
            let rotation = p.flags & 0xf0;
            let (flags, flipped) = (p.flags, p.old[0]);
            let mut first: Option<usize> = None;
            for sy in 0..th {
                for sx in 0..tw {
                    if tile.is_some_and(|t| t.masked() && !t.subtile_masked_valid(sx, sy)) {
                        continue;
                    }

                    let (dx, dy) = rotate_in_tile_space(tw, th, rotation, sx, sy);
                    let (cx, cy) = (i32::from(p.x) + dx, i32::from(p.y) + dy);
                    if cx < 0 || cy < 0 || cx >= w || cy >= h {
                        continue;
                    }

                    let c = (cy * w + cx) as usize;
                    let origin = [p.x as u16, p.y as u16];
                    match first {
                        None => {
                            first = Some(c);
                            let chunk = (cx as u32 / 64 + cy as u32 / 64 * cols) as u16;
                            let [lo, hi] = chunk.to_le_bytes();
                            layer[c] = TileMapCell {
                                index: p.name as i32 + 1,
                                flags,
                                origin,
                                climate: p.climate,
                                flipped,
                                lf: [0.0; 2],
                                old: [lo, hi, 0, 0],
                            };
                        }

                        Some(f) => {
                            layer[c] = TileMapCell {
                                index: -(f as i32 + 1),
                                flags: (flags & !0xf0) | 0x10,
                                origin,
                                climate: p.climate,
                                flipped,
                                lf: [0.0; 2],
                                old: [255, 255, 0, 0],
                            };
                        }
                    }
                }
            }
        }

        for (c, o) in layer.iter_mut().zip(&self.layers[0]) {
            // An empty cell stays as it was (`prologue_map_01` has 47 with stray bytes).
            if c.index == 0 && o.index == 0 {
                *c = *o;
                continue;
            }

            let (mut a, mut b) = (*c, *o);
            (a.old[2], a.old[3], b.old[2], b.old[3]) = (0, 0, 0, 0);
            if a == b {
                (c.old[2], c.old[3]) = (o.old[2], o.old[3]);
            }
        }

        Self {
            version: self.version,
            width: self.width,
            height: self.height,
            layers: [layer, self.layers[1].clone()],
            names: list.names.clone(),
            chunks: self.chunks.clone(),
        }
    }
}
