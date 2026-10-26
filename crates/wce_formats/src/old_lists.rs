//! The older tree and grass lists still shipped next to the FASTBIN ones (`<climate>.tree_list`, `default.grass_list`, no `.bin`; 3,201 and
//! 708 files in Attila's packs, several for climates Attila does not have). docs/formats/tile_binary.md, "Older tree and grass lists".
//!
//! Layout (little endian): u32 version, u32 type count, u32 record count; per type a u16-counted name (the model, `battleterrain\
//! vegetation\trees\birch\birch_a.rigid_model_v2`, or `grass/1`) and the u32 index of its first record (in order, the first 0); then the
//! records, of a size per kind and version: trees v2 10 bytes (half floats x, y, z in metres of the tile, u8 0, u8 scale * 127, 2 bytes),
//! trees v0 28 bytes (f32 x, y, z, f32 x 3 (0, 1, 0), 4 bytes), grass v1 6 bytes (half floats x, y, z), grass v0 12 bytes (f32 x, y, z).
//! The records are kept as bytes; [`OldList::position`] reads a record's position.

#[derive(Debug, thiserror::Error)]
#[error("{0}")]
pub struct OldListError(pub String);

/// Tree or grass.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum OldListKind {
    Tree,
    Grass,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OldList {
    pub kind: OldListKind,
    pub version: u32,

    /// (name, index of its first record).
    pub types: Vec<(Vec<u8>, u32)>,
    pub records: Vec<u8>,
}

/// The record size of a kind and version.
pub fn record_size(kind: OldListKind, version: u32) -> Option<usize> {
    match (kind, version) {
        (OldListKind::Tree, 2) => Some(10),
        (OldListKind::Tree, 0) => Some(28),
        (OldListKind::Grass, 1) => Some(6),
        (OldListKind::Grass, 0) => Some(12),
        _ => None,
    }
}

fn half(bits: u16) -> f32 {
    let sign = if bits & 0x8000 != 0 { -1.0 } else { 1.0 };
    let exp = i32::from((bits >> 10) & 0x1F);
    let frac = f32::from(bits & 0x3FF);
    sign * match exp {
        0 => frac * 2f32.powi(-24),
        31 => if frac == 0.0 { f32::INFINITY } else { f32::NAN },
        e => (1.0 + frac / 1024.0) * 2f32.powi(e - 15),
    }
}

/// The half float nearest to a value (ties to even; out of range: infinity).
pub fn to_half(v: f32) -> u16 {
    let bits = v.to_bits();
    let sign = ((bits >> 16) & 0x8000) as u16;
    let exp = ((bits >> 23) & 0xFF) as i32;
    let mant = bits & 0x7F_FFFF;
    if exp == 0xFF {
        return sign | 0x7C00 | if mant != 0 { 0x200 } else { 0 };
    }

    let e = exp - 127 + 15;
    if e >= 0x1F {
        return sign | 0x7C00;
    }

    // The mantissa with its implicit bit, shifted to the half's precision (subnormals shift further), rounded to nearest even.
    let (m, shift) = if e <= 0 { (mant | 0x80_0000, 14 - e) } else { (mant, 13) };
    if shift > 24 {
        return sign;
    }

    let half = 1u32 << (shift - 1);
    let mut r = m >> shift;
    let rest = m & ((1 << shift) - 1);
    if rest > half || (rest == half && r & 1 == 1) {
        r += 1;
    }

    // A carry out of the mantissa steps the exponent (the sum does it).
    sign | if e <= 0 { r as u16 } else { ((e as u32) << 10) as u16 + r as u16 }
}

impl OldList {

    /// # Errors
    ///
    /// A version without a known record size, or a file whose records do not fill it exactly.
    pub fn parse(kind: OldListKind, b: &[u8]) -> Result<Self, OldListError> {
        let rd32 = |o: usize| b.get(o..o + 4).map(|x| u32::from_le_bytes(x.try_into().expect("4 bytes"))).ok_or_else(|| OldListError(format!("truncated at {o}")));
        let (version, n, total) = (rd32(0)?, rd32(4)? as usize, rd32(8)? as usize);
        let Some(size) = record_size(kind, version) else { return Err(OldListError(format!("{kind:?} list version {version}"))) };
        if n > b.len() / 6 {
            return Err(OldListError("type count".to_owned()));
        }

        let mut p = 12;
        let mut types = Vec::with_capacity(n);
        for _ in 0..n {
            let len = b.get(p..p + 2).map(|x| usize::from(u16::from_le_bytes([x[0], x[1]]))).ok_or_else(|| OldListError("truncated name".to_owned()))?;
            let name = b.get(p + 2..p + 2 + len).ok_or_else(|| OldListError("truncated name".to_owned()))?.to_vec();
            p += 2 + len;
            types.push((name, rd32(p)?));
            p += 4;
        }

        if b.len() - p != total * size {
            return Err(OldListError(format!("{} bytes of records for {total} records of {size}", b.len() - p)));
        }

        Ok(Self { kind, version, types, records: b[p..].to_vec() })
    }

    pub fn write(&self) -> Vec<u8> {
        let size = record_size(self.kind, self.version).expect("a parsed version");
        let mut o = vec![];
        for v in [self.version, self.types.len() as u32, (self.records.len() / size) as u32] {
            o.extend_from_slice(&v.to_le_bytes());
        }

        for (name, first) in &self.types {
            o.extend_from_slice(&(name.len() as u16).to_le_bytes());
            o.extend_from_slice(name);
            o.extend_from_slice(&first.to_le_bytes());
        }

        o.extend_from_slice(&self.records);
        o
    }

    pub fn len(&self) -> usize {
        self.records.len() / record_size(self.kind, self.version).expect("a parsed version")
    }

    pub fn is_empty(&self) -> bool {
        self.records.is_empty()
    }

    /// The bytes of record `i`.
    pub fn record(&self, i: usize) -> &[u8] {
        let size = record_size(self.kind, self.version).expect("a parsed version");
        &self.records[i * size..(i + 1) * size]
    }

    /// The position of record `i` (metres in the tile).
    pub fn position(&self, i: usize) -> [f32; 3] {
        let r = self.record(i);
        let f = |o: usize| f32::from_le_bytes(r[o..o + 4].try_into().expect("4 bytes"));
        let h = |o: usize| half(u16::from_le_bytes([r[o], r[o + 1]]));
        match (self.kind, self.version) {
            (_, 0) => [f(0), f(4), f(8)],
            _ => [h(0), h(2), h(4)],
        }
    }

    /// The scale of tree `i` (its byte / 127), `None` for grass.
    pub fn scale(&self, i: usize) -> Option<f32> {
        let r = self.record(i);
        match (self.kind, self.version) {
            (OldListKind::Tree, 2) => Some(f32::from(r[7]) / 127.0),
            (OldListKind::Tree, 0) => Some(f32::from(r[26]) / 127.0),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn halves_round_trip() {
        for v in [0.0f32, 1.0, -2.0, 117.5, 255.875, -6.21875, 0.000_061_035_156, 65504.0] {
            assert_eq!(half(to_half(v)), v, "{v}");
        }

        // Subnormals: 1e-7 is 1.68 steps of 2^-24.
        assert_eq!(to_half(1e-7), 2);
        assert_eq!(to_half(1e-8), 0);

        // To nearest, ties to even: 1 + 2^-11 is halfway between 1 and 1 + 2^-10.
        assert_eq!(half(to_half(1.0 + 2f32.powi(-11))), 1.0);
        assert_eq!(half(to_half(1.0 + 3.0 * 2f32.powi(-11))), 1.0 + 2.0 * 2f32.powi(-10));
        assert_eq!(to_half(1e6), 0x7C00);
    }

    #[test]
    fn tree_list_round_trip() {
        let mut b = vec![];
        for v in [2u32, 1, 2] {
            b.extend_from_slice(&v.to_le_bytes());
        }

        b.extend_from_slice(&5u16.to_le_bytes());
        b.extend_from_slice(b"birch");
        b.extend_from_slice(&0u32.to_le_bytes());
        // x 80.4375 (0x5507), y -0.0085 (0xa05a), z 264.5 (0x5c22), scale 1.0.
        b.extend_from_slice(&[0x07, 0x55, 0x5a, 0xa0, 0x22, 0x5c, 0, 127, 0, 0x6b]);
        b.extend_from_slice(&[0x00, 0x3c, 0, 0, 0, 0x40, 0, 254, 1, 0]);
        let l = OldList::parse(OldListKind::Tree, &b).unwrap();
        assert_eq!(l.write(), b);
        assert_eq!(l.len(), 2);
        assert_eq!(l.position(0)[0], 80.4375);
        assert_eq!(l.position(1), [1.0, 0.0, 2.0]);
        assert_eq!(l.scale(1), Some(2.0));
        assert!(OldList::parse(OldListKind::Tree, &b[..b.len() - 1]).is_err());
    }
}
