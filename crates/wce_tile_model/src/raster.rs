//! A raster: the pixels of a height map, a blend map, a ground type map ... decoded, row-major from the first row of the file, channels
//! interleaved.

#[derive(Clone, Debug, PartialEq)]
pub enum Samples {
    U8(Vec<u8>),
    U16(Vec<u16>),
    F32(Vec<f32>),
}

impl Samples {
    pub fn len(&self) -> usize {
        match self {
            Self::U8(v) => v.len(),
            Self::U16(v) => v.len(),
            Self::F32(v) => v.len(),
        }
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// A 64-bit fingerprint of the samples, to tell whether they changed since a file was read (not cryptographic).
    pub fn fingerprint(&self) -> u64 {
        let (tag, bytes): (u64, &[u8]) = match self {
            Self::U8(v) => (1, v),
            Self::U16(v) => (2, bytemuck_u16(v)),
            Self::F32(v) => (3, bytemuck_f32(v)),
        };

        let mut h = 0xcbf2_9ce4_8422_2325u64 ^ tag;
        let (words, rest) = bytes.as_chunks::<8>();
        for w in words {
            h = (h ^ u64::from_le_bytes(*w)).wrapping_mul(0x0000_0100_0000_01b3).rotate_left(29);
        }

        for &b in rest {
            h = (h ^ u64::from(b)).wrapping_mul(0x0000_0100_0000_01b3);
        }

        h ^ bytes.len() as u64
    }
}

fn bytemuck_u16(v: &[u16]) -> &[u8] {
    // SAFETY: u16 has no padding and any byte pattern is valid; the length covers exactly the slice.
    unsafe { std::slice::from_raw_parts(v.as_ptr().cast(), std::mem::size_of_val(v)) }
}

fn bytemuck_f32(v: &[f32]) -> &[u8] {
    // SAFETY: as above.
    unsafe { std::slice::from_raw_parts(v.as_ptr().cast(), std::mem::size_of_val(v)) }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Raster {
    pub width: u32,
    pub height: u32,
    pub channels: u32,
    pub samples: Samples,
}

impl Raster {
    /// Whether the sample count matches the size.
    pub fn is_consistent(&self) -> bool {
        self.samples.len() == self.width as usize * self.height as usize * self.channels as usize
    }
}

#[cfg(test)]
mod tests {
    use super::Samples;

    /// Equal samples give equal fingerprints; one sample changed, one more sample, or the same bytes as another sample type give another.
    #[test]
    fn the_fingerprint_follows_the_samples() {
        let all = [
            Samples::U8((0..=255).collect()),
            Samples::U16((0..300).map(|i| i * 211).collect()),
            Samples::F32((0..77).map(|i| i as f32 * 0.25 - 3.0).collect()),
        ];
        for s in &all {
            assert_eq!(s.fingerprint(), s.clone().fingerprint());
            let mut changed = s.clone();
            let mut longer = s.clone();
            match (&mut changed, &mut longer) {
                (Samples::U8(c), Samples::U8(l)) => {
                    c[100] ^= 1;
                    l.push(0);
                }
                (Samples::U16(c), Samples::U16(l)) => {
                    c[299] ^= 0x100;
                    l.push(0);
                }
                (Samples::F32(c), Samples::F32(l)) => {
                    c[3] = -c[3];
                    l.push(0.0);
                }
                _ => unreachable!(),
            }

            assert_ne!(s.fingerprint(), changed.fingerprint());
            assert_ne!(s.fingerprint(), longer.fingerprint());
        }

        // The tail bytes past the last 8-byte word count too.
        let a = Samples::U8(vec![1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11]);
        let b = Samples::U8(vec![1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 12]);
        assert_ne!(a.fingerprint(), b.fingerprint());

        // The same bytes as another sample type: [0x0201, 0x0403] are the bytes 1, 2, 3, 4.
        assert_ne!(Samples::U8(vec![1, 2, 3, 4]).fingerprint(), Samples::U16(vec![0x0201, 0x0403]).fingerprint());
        assert_ne!(Samples::U8(vec![]).fingerprint(), Samples::U16(vec![]).fingerprint());
    }
}
