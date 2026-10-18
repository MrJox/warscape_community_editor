//! The campaign's battle tile map (`terrain/battles/main_attila_map/tile_map.index` + `tile_map.tiles`), as BOB's Terrain processor saves it
//! (`TILE_MAP::save`, 0x102e3770) and the game reads it (fn 0x118ffb60). Little endian, version 9:
//!
//! - `tile_map.index`: u32 version, u32 width, u32 height (cells);
//! - `tile_map.tiles`: the tile layer then the geometry layer, width x height cells of 20 bytes each, row by row (i32 index: 0 empty,
//!   positive the 1-based name of the tile placed with its origin here, negative -(origin cell + 1); u16 flags (rotation 0x10 .. 0x80,
//!   0x700 layers; an empty cell has 0x10); u16 x 2 the origin cell; u8 climate; u8 flipped (the river flow, `calculate_flow`); f32 x 2
//!   the low-frequency height range); then u32 name count and the names (u16 length + bytes, `<tile location>\<variation location>`).
//!
//! The game also reads versions 6 and 7 (16-byte cells, a chunk list in the index): the kit's `main_attila_map/tile_map.tiles` is an old
//! file of that kind. terrain.pack's (version 9) was built from other sources than the kit's: BOB's fill of the kit map (exact in
//! `battle_map_check`) differs from it.

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct TileMapCell {
    pub index: i32,
    pub flags: u16,
    pub origin: [u16; 2],
    pub climate: u8,
    pub flipped: u8,
    pub lf: [f32; 2],
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct TileMap {
    pub version: u32,
    pub width: u32,
    pub height: u32,
    pub layers: [Vec<TileMapCell>; 2],
    pub names: Vec<String>,
}

const CELL: usize = 20;

fn u16_at(b: &[u8], p: usize) -> u16 {
    u16::from_le_bytes([b[p], b[p + 1]])
}

fn u32_at(b: &[u8], p: usize) -> u32 {
    u32::from_le_bytes([b[p], b[p + 1], b[p + 2], b[p + 3]])
}

impl TileMap {

    /// # Errors
    ///
    /// Fails on a version other than 9 or a truncated file.
    pub fn parse(index: &[u8], tiles: &[u8]) -> Result<Self, String> {
        if index.len() < 12 {
            return Err("tile_map.index: truncated".to_owned());
        }

        let (version, width, height) = (u32_at(index, 0), u32_at(index, 4), u32_at(index, 8));
        if version < 8 {
            return Err(format!("tile_map.index: version {version} (only 8 and later read)"));
        }

        let n = width as usize * height as usize;
        let mut p = 2 * n * CELL;
        if tiles.len() < p + 4 {
            return Err("tile_map.tiles: truncated".to_owned());
        }

        let cell = |o: usize| TileMapCell {
            index: u32_at(tiles, o) as i32,
            flags: u16_at(tiles, o + 4),
            origin: [u16_at(tiles, o + 6), u16_at(tiles, o + 8)],
            climate: tiles[o + 10],
            flipped: tiles[o + 11],
            lf: [f32::from_bits(u32_at(tiles, o + 12)), f32::from_bits(u32_at(tiles, o + 16))],
        };

        let layers = [(0..n).map(|i| cell(i * CELL)).collect(), (0..n).map(|i| cell((n + i) * CELL)).collect()];
        let count = u32_at(tiles, p) as usize;
        p += 4;
        let mut names = Vec::with_capacity(count);
        for _ in 0..count {
            let len = usize::from(u16_at(tiles, p));
            let s = tiles.get(p + 2..p + 2 + len).ok_or("tile_map.tiles: truncated name")?;
            names.push(s.iter().map(|&c| char::from(c)).collect());
            p += 2 + len;
        }

        Ok(Self { version, width, height, layers, names })
    }

    /// The two files: (`tile_map.index`, `tile_map.tiles`).
    pub fn write(&self) -> (Vec<u8>, Vec<u8>) {
        let mut index = vec![];
        for v in [self.version, self.width, self.height] {
            index.extend(v.to_le_bytes());
        }

        let mut o = Vec::with_capacity(2 * self.layers[0].len() * CELL);
        for c in self.layers.iter().flatten() {
            o.extend(c.index.to_le_bytes());
            o.extend(c.flags.to_le_bytes());
            o.extend(c.origin[0].to_le_bytes());
            o.extend(c.origin[1].to_le_bytes());
            o.extend([c.climate, c.flipped]);
            o.extend(c.lf[0].to_le_bytes());
            o.extend(c.lf[1].to_le_bytes());
        }

        o.extend((self.names.len() as u32).to_le_bytes());
        for n in &self.names {
            o.extend((n.len() as u16).to_le_bytes());
            o.extend(n.bytes());
        }

        (index, o)
    }
}
