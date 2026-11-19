//! Vorbis bit packing: least significant bit first, within bytes in order.

/// Reads bits; past the end it reads zeros (and [`BitReader::overran`] says so).
pub struct BitReader<'a> {
    bytes: &'a [u8],
    pos: usize,
}

impl<'a> BitReader<'a> {
    pub fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, pos: 0 }
    }

    pub fn read(&mut self, n: u32) -> u32 {
        let mut v = 0u32;
        for i in 0..n {
            let byte = self.bytes.get(self.pos >> 3).copied().unwrap_or(0);
            v |= u32::from((byte >> (self.pos & 7)) & 1) << i;
            self.pos += 1;
        }

        v
    }

    /// Bits read so far.
    pub fn position(&self) -> usize {
        self.pos
    }

    /// Bits left before the end of the bytes.
    pub fn left(&self) -> usize {
        (self.bytes.len() * 8).saturating_sub(self.pos)
    }

    pub fn overran(&self) -> bool {
        self.pos > self.bytes.len() * 8
    }
}

/// Writes bits.
#[derive(Default)]
pub struct BitWriter {
    bytes: Vec<u8>,
    bits: usize,
}

impl BitWriter {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn write(&mut self, v: u32, n: u32) {
        for i in 0..n {
            if self.bits.is_multiple_of(8) {
                self.bytes.push(0);
            }

            if (v >> i) & 1 == 1 {
                *self.bytes.last_mut().expect("pushed") |= 1 << (self.bits & 7);
            }

            self.bits += 1;
        }
    }

    /// Copies `n` bits from a reader.
    pub fn copy(&mut self, r: &mut BitReader, n: usize) {
        let mut left = n;
        while left > 0 {
            let chunk = left.min(32) as u32;
            self.write(r.read(chunk), chunk);
            left -= chunk as usize;
        }
    }

    pub fn bit_len(&self) -> usize {
        self.bits
    }

    pub fn into_bytes(self) -> Vec<u8> {
        self.bytes
    }
}

/// ilog: the number of bits of `v` (0 for 0).
pub fn ilog(v: u32) -> u32 {
    32 - v.leading_zeros()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip() {
        let mut w = BitWriter::new();
        w.write(5, 3);
        w.write(0x1234, 13);
        w.write(1, 1);
        let bytes = w.into_bytes();
        let mut r = BitReader::new(&bytes);
        assert_eq!((r.read(3), r.read(13), r.read(1)), (5, 0x1234, 1));
        assert_eq!(ilog(0), 0);
        assert_eq!(ilog(1), 1);
        assert_eq!(ilog(4), 3);
    }
}
