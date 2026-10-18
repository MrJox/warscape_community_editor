//! The campaign's battle locations (`terrain/battles/main_attila_map/locations.blm`, game reader fn 0x118573a0): where on the campaign's
//! battle tile map each kind of battle can be fought. Little endian:
//!
//! - "BLMF", u32 version (8; the game reads 7 and later), u32 type count, u32 width, u32 height (the tile map's cells);
//! - per type: u16 length + name (`land_normal`, `settlement_unfortified`, `land_ambush`, `naval_normal`, `port_assault`, `coastal_battle`,
//!   `settlement_standard`, `unfortified_port`, `land_bridge`), u32 count, then per location 9 x i32: the cell rectangle (x0, y0, x1, y1,
//!   inclusive), its centre (x, y), an id (-1, or 0xffff0000 + n for the coastal and bridge battles), 0, 0;
//! - then (version 8) u32 cell count and a climate table (u32 count, per climate a name and two u16), not read here.
//!
//! `locations.blm.mask0` (version 8; `.mask1` for version 7) is one u16 per cell (meaning open).

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Location {
    /// x0, y0, x1, y1 in cells, inclusive.
    pub rect: [i32; 4],
    pub centre: [i32; 2],
    pub id: i32,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct BattleLocations {
    pub version: u32,
    pub width: u32,
    pub height: u32,

    /// (battle type, its locations), in the file's order.
    pub types: Vec<(String, Vec<Location>)>,
}

impl BattleLocations {

    /// # Errors
    ///
    /// Fails on a file that isn't BLMF or is truncated.
    pub fn parse(b: &[u8]) -> Result<Self, String> {
        let u32_at = |p: usize| b.get(p..p + 4).map(|s| u32::from_le_bytes([s[0], s[1], s[2], s[3]])).ok_or_else(|| "locations.blm: truncated".to_owned());
        if b.get(..4) != Some(b"BLMF") {
            return Err("locations.blm: not a BLMF file".to_owned());
        }

        let mut out = Self { version: u32_at(4)?, width: u32_at(12)?, height: u32_at(16)?, types: vec![] };
        let count = u32_at(8)?;
        let mut p = 20;
        for _ in 0..count {
            let len = b.get(p..p + 2).map(|s| usize::from(u16::from_le_bytes([s[0], s[1]]))).ok_or("locations.blm: truncated")?;
            let name = b.get(p + 2..p + 2 + len).ok_or("locations.blm: truncated")?.iter().map(|&c| char::from(c)).collect();
            p += 2 + len;
            let n = u32_at(p)? as usize;
            p += 4;
            let mut locations = Vec::with_capacity(n);
            for _ in 0..n {
                let v = (0..9).map(|i| u32_at(p + i * 4).map(|v| v as i32)).collect::<Result<Vec<_>, _>>()?;
                locations.push(Location { rect: [v[0], v[1], v[2], v[3]], centre: [v[4], v[5]], id: v[6] });
                p += 36;
            }

            out.types.push((name, locations));
        }

        Ok(out)
    }

    /// The locations whose rectangle holds the cell, as (battle type, location).
    pub fn at(&self, x: i32, y: i32) -> impl Iterator<Item = (&str, &Location)> {
        self.types.iter().flat_map(|(t, l)| l.iter().map(move |l| (t.as_str(), l))).filter(move |(_, l)| l.rect[0] <= x && x <= l.rect[2] && l.rect[1] <= y && y <= l.rect[3])
    }
}
