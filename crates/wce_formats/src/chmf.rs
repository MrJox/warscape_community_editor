//! `CHMF`: the block height maps BOB writes (`hf_height_map.data`, `hf_water_map.data` of a tile, `lf_height_map.data` of a map).
//!
//! Version 5: 16 u32 of header (the magic, the version, sizes and counts; field meanings in docs/formats/tile_binary.md), the data size,
//! one u32 offset per block (the header's last field is the block count), then the data. The data is kept as bytes here.

#[derive(Debug, thiserror::Error)]
#[error("{0}")]
pub struct ChmfError(pub String);

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ChmfFile {
    /// The 16 u32 of the header, the magic and the version included.
    pub header: [u32; 16],

    /// Where each block starts in `data`.
    pub offsets: Vec<u32>,
    pub data: Vec<u8>,
}

impl ChmfFile {

    /// # Errors
    ///
    /// Not a version 5 CHMF file, or a truncated one.
    pub fn parse(b: &[u8]) -> Result<Self, ChmfError> {
        let rd32 = |o: usize| b.get(o..o + 4).map(|x| u32::from_le_bytes(x.try_into().expect("4 bytes"))).ok_or_else(|| ChmfError(format!("truncated at {o}")));
        if b.len() < 68 || &b[..4] != b"CHMF" {
            return Err(ChmfError("not CHMF".to_owned()));
        }

        let mut header = [0; 16];
        for (i, h) in header.iter_mut().enumerate() {
            *h = rd32(4 * i)?;
        }

        if header[1] != 5 {
            return Err(ChmfError(format!("unsupported version {}", header[1])));
        }

        let mut p = 64;
        let data_size = rd32(p)? as usize;
        p += 4;
        let mut offsets = Vec::with_capacity(header[15] as usize);
        for _ in 0..header[15] {
            offsets.push(rd32(p)?);
            p += 4;
        }

        if p + data_size != b.len() {
            return Err(ChmfError("data size does not match file size".to_owned()));
        }

        Ok(Self { header, offsets, data: b[p..].to_vec() })
    }

    pub fn write(&self) -> Vec<u8> {
        let mut o = Vec::with_capacity(68 + 4 * self.offsets.len() + self.data.len());
        for v in self.header.iter().chain([self.data.len() as u32].iter()).chain(self.offsets.iter()) {
            o.extend_from_slice(&v.to_le_bytes());
        }

        o.extend_from_slice(&self.data);
        o
    }
}
