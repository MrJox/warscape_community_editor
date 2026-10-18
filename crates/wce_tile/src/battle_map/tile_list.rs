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

#[derive(Clone, Debug, Default, PartialEq)]
pub struct TileListPlacement {
    pub name: usize,
    pub x: i16,
    pub y: i16,
    pub flags: u16,
    pub climate: u8,
    pub lf: [f32; 2],
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
        t.strings = [r.string()?, r.string()?];
        t.battle_size = [r.u32()?, r.u32()?];
        t.battle_centre = [r.u32()?, r.u32()?];
        t.origin_offset = [r.f32()?, r.f32()?];
        t.bounds = [r.u32()? as i32, r.u32()? as i32, r.u32()? as i32, r.u32()? as i32];
        t.is_campaign = r.u8()? != 0;
        let names = r.u32()? as usize;
        for _ in 0..names {
            t.names.push(r.string()?);
        }

        for _ in 0..count {
            t.placements.push(TileListPlacement {
                name: r.u32()? as usize,
                x: r.u16()? as i16,
                y: r.u16()? as i16,
                flags: r.u16()?,
                climate: r.u8()?,
                lf: [r.f32()?, r.f32()?],
            });
        }

        Ok(t)
    }

    pub fn write(&self) -> Vec<u8> {
        let mut o = vec![];
        let f = |o: &mut Vec<u8>, v: f32| o.extend(v.to_le_bytes());
        let s = |o: &mut Vec<u8>, v: &str| {
            o.extend((v.len() as u16).to_le_bytes());
            o.extend(v.bytes());
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

        for p in &self.placements {
            o.extend((p.name as u32).to_le_bytes());
            o.extend(p.x.to_le_bytes());
            o.extend(p.y.to_le_bytes());
            o.extend(p.flags.to_le_bytes());
            o.push(p.climate);
            f(&mut o, p.lf[0]);
            f(&mut o, p.lf[1]);
        }

        o
    }
}
