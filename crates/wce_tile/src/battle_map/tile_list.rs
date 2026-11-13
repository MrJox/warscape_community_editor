//! `terrain.tile_list`: the battle field BOB's Terrain processor writes (BATTLE_TILE_MAP::save, 0x1027be20) and the game reads (fn 0x118ff250).
//! Little endian, version 11:
//!
//! - u32 version, u32 placement count;
//! - f32 x 2 low-frequency world position, f32 x 2 low-frequency world scale, f32 x 2 (500, 1.333);
//! - two strings (u16 length + bytes);
//! - u32 x 2 battle size, u32 x 2 battle centre, f32 x 2 origin offset (all in cells);
//! - i32 x 4 the placements' bounds (min x - size, min y - size, max x + size, max y + size);
//! - u8 is_campaign;
//! - u32 name count, names (`<tile location>\<variation location>`, in the order of their first placement);
//! - per placement, 19 bytes: u32 name index, i16 x, i16 y, u16 flags (rotation 0x10 .. 0x80, 4 flipped, 0x700 layers, 0x800), u8 climate,
//!   f32 x 2 the low-frequency height range under it.
//!
//! Rome 2's versions 7 to 9 (its loader, empire.retail.dll fn 0x115b9940): the same up to the names (before version 8 the strings are
//! UTF-16: u16 characters, then the characters); then per placement 14 bytes (u32
//! name index, i16 x, i16 y, u16 flags, two bytes, u8 climate, a byte; the three unknown bytes kept in [`TileListPlacement::old`]); then
//! (version 7 and up) a u32 count of rectangles, i32 x 4 each; version 9 then more (a u32, a list of names, a structure: kept verbatim in
//! [`TileList::tail`], not decoded).

#[derive(Clone, Debug, Default, PartialEq)]
pub struct TileListPlacement {
    pub name: usize,
    pub x: i16,
    pub y: i16,
    pub flags: u16,
    pub climate: u8,
    pub lf: [f32; 2],

    /// Before version 11 (Rome 2): the placement's bytes 10, 11 and 13 (unknown).
    pub old: [u8; 3],
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct TileList {
    pub version: u32,
    pub lf_position: [f32; 2],
    pub lf_scale: [f32; 2],
    pub unknown: [f32; 2],
    pub strings: [String; 2],
    pub battle_size: [u32; 2],
    pub battle_centre: [u32; 2],
    pub origin_offset: [f32; 2],
    pub bounds: [i32; 4],
    pub is_campaign: bool,
    pub names: Vec<String>,
    pub placements: Vec<TileListPlacement>,

    /// Before version 11, from version 7 (Rome 2): rectangles (i32 x 4; meaning open: `canyon`'s is -8, -8, 72, 72 on a 64-cell map).
    pub rects: Vec<[i32; 4]>,

    /// Rome 2's version 9: the bytes after the rectangles, verbatim.
    pub tail: Vec<u8>,
}

struct Reader<'a> {
    b: &'a [u8],
    p: usize,
}

impl Reader<'_> {
    fn take(&mut self, n: usize) -> Result<&[u8], String> {
        let s = self.b.get(self.p..self.p + n).ok_or_else(|| format!("tile_list: truncated at {}", self.p))?;
        self.p += n;
        Ok(s)
    }

    fn u8(&mut self) -> Result<u8, String> {
        Ok(self.take(1)?[0])
    }

    fn u16(&mut self) -> Result<u16, String> {
        Ok(u16::from_le_bytes(self.take(2)?.try_into().unwrap_or_default()))
    }

    fn u32(&mut self) -> Result<u32, String> {
        Ok(u32::from_le_bytes(self.take(4)?.try_into().unwrap_or_default()))
    }

    fn f32(&mut self) -> Result<f32, String> {
        Ok(f32::from_bits(self.u32()?))
    }

    fn string(&mut self) -> Result<String, String> {
        let n = usize::from(self.u16()?);
        Ok(self.take(n)?.iter().map(|&c| char::from(c)).collect())
    }

    /// A UTF-16 string (u16 characters, then the characters).
    fn wide(&mut self) -> Result<String, String> {
        let n = usize::from(self.u16()?);
        let units: Vec<u16> = self.take(n * 2)?.chunks_exact(2).map(|c| u16::from_le_bytes([c[0], c[1]])).collect();
        Ok(String::from_utf16_lossy(&units))
    }

    /// A string as the version has them.
    fn text(&mut self, version: u32) -> Result<String, String> {
        if version < 8 { self.wide() } else { self.string() }
    }
}

impl TileList {
    /// # Errors
    ///
    /// Fails on a truncated file.
    pub fn parse(b: &[u8]) -> Result<Self, String> {
        let mut r = Reader { b, p: 0 };
        let mut t = Self { version: r.u32()?, ..Default::default() };
        let count = r.u32()? as usize;
        t.lf_position = [r.f32()?, r.f32()?];
        t.lf_scale = [r.f32()?, r.f32()?];
        t.unknown = [r.f32()?, r.f32()?];
        t.strings = [r.text(t.version)?, r.text(t.version)?];
        t.battle_size = [r.u32()?, r.u32()?];
        t.battle_centre = [r.u32()?, r.u32()?];
        t.origin_offset = [r.f32()?, r.f32()?];
        t.bounds = [r.u32()? as i32, r.u32()? as i32, r.u32()? as i32, r.u32()? as i32];
        t.is_campaign = r.u8()? != 0;
        let names = r.u32()? as usize;
        for _ in 0..names {
            t.names.push(r.text(t.version)?);
        }

        let old = t.version < 11;
        for _ in 0..count {
            let (name, x, y, flags) = (r.u32()? as usize, r.u16()? as i16, r.u16()? as i16, r.u16()?);
            t.placements.push(if old {
                let (a, b, climate, c) = (r.u8()?, r.u8()?, r.u8()?, r.u8()?);
                TileListPlacement { name, x, y, flags, climate, lf: [0.0; 2], old: [a, b, c] }
            } else {
                TileListPlacement { name, x, y, flags, climate: r.u8()?, lf: [r.f32()?, r.f32()?], old: [0; 3] }
            });
        }

        if old && t.version >= 7 {
            for _ in 0..r.u32()? {
                t.rects.push([r.u32()? as i32, r.u32()? as i32, r.u32()? as i32, r.u32()? as i32]);
            }

            t.tail = b[r.p..].to_vec();
        }

        Ok(t)
    }

    pub fn write(&self) -> Vec<u8> {
        let mut o = vec![];
        let f = |o: &mut Vec<u8>, v: f32| o.extend(v.to_le_bytes());
        let wide = self.version < 8;
        let s = |o: &mut Vec<u8>, v: &str| {
            if wide {
                let units: Vec<u16> = v.encode_utf16().collect();
                o.extend((units.len() as u16).to_le_bytes());
                o.extend(units.iter().flat_map(|u| u.to_le_bytes()));
            } else {
                o.extend((v.len() as u16).to_le_bytes());
                o.extend(v.bytes());
            }
        };

        o.extend(self.version.to_le_bytes());
        o.extend((self.placements.len() as u32).to_le_bytes());
        for v in self.lf_position.iter().chain(&self.lf_scale).chain(&self.unknown) {
            f(&mut o, *v);
        }

        s(&mut o, &self.strings[0]);
        s(&mut o, &self.strings[1]);
        for v in self.battle_size.iter().chain(&self.battle_centre) {
            o.extend(v.to_le_bytes());
        }

        for v in &self.origin_offset {
            f(&mut o, *v);
        }

        for v in &self.bounds {
            o.extend(v.to_le_bytes());
        }

        o.push(u8::from(self.is_campaign));
        o.extend((self.names.len() as u32).to_le_bytes());
        for n in &self.names {
            s(&mut o, n);
        }

        let old = self.version < 11;
        for p in &self.placements {
            o.extend((p.name as u32).to_le_bytes());
            o.extend(p.x.to_le_bytes());
            o.extend(p.y.to_le_bytes());
            o.extend(p.flags.to_le_bytes());
            if old {
                o.extend([p.old[0], p.old[1], p.climate, p.old[2]]);
            } else {
                o.push(p.climate);
                f(&mut o, p.lf[0]);
                f(&mut o, p.lf[1]);
            }
        }

        if old && self.version >= 7 {
            o.extend((self.rects.len() as u32).to_le_bytes());
            for r in &self.rects {
                for v in r {
                    o.extend(v.to_le_bytes());
                }
            }

            o.extend_from_slice(&self.tail);
        }

        o
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn list(version: u32) -> TileList {
        TileList {
            version,
            lf_position: [0.5, 0.5],
            lf_scale: [1.0, 1.0],
            unknown: [500.0, 1.333],
            strings: ["a".to_owned(), String::new()],
            battle_size: [64, 64],
            battle_centre: [32, 32],
            origin_offset: [-32.0, -32.0],
            bounds: [26, 26, 34, 34],
            is_campaign: false,
            names: vec![r"terrain\tiles\battle\plains\8x8".to_owned()],
            placements: vec![TileListPlacement { name: 0, x: 30, y: 30, flags: 0xb710, climate: 2, lf: [0.0; 2], old: [1, 2, 3] }],
            rects: vec![[-8, -8, 72, 72]],
            tail: if version == 9 { vec![9, 9, 9] } else { vec![] },
        }
    }

    #[test]
    fn rome2_versions_round_trip() {
        for v in [7, 8, 9] {
            let t = list(v);
            let b = t.write();
            assert_eq!(TileList::parse(&b).unwrap(), t, "version {v}");
            // A placement is 14 bytes; version 7's strings are UTF-16.
            let names = if v < 8 { 2 + 2 * t.names[0].len() } else { 2 + t.names[0].len() };
            let strings = if v < 8 { 2 + 2 + 2 } else { 2 + 1 + 2 };
            assert_eq!(b.len(), 8 + 24 + strings + 40 + 1 + 4 + names + 14 + 4 + 16 + t.tail.len(), "version {v}");
        }
    }
}
