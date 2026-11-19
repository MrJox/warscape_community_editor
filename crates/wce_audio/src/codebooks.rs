//! Wwise's built-in Vorbis codebook library: a WEM's setup packet names its codebooks by 10-bit ID. The library is ww2ogg's
//! `packed_codebooks_aoTuV_603.bin` (BSD-3, `third_party/ww2ogg`): the packed codebooks, then a table of u32 offsets, the last u32 of
//! the file being the table's offset. A packed codebook is a standard Vorbis codebook written smaller (docs/formats/wem.md); [`rebuild`]
//! writes the standard form.

use std::sync::OnceLock;

use crate::bits::{BitReader, BitWriter, ilog};
use crate::{AudioError, Result};

static LIBRARY: &[u8] = include_bytes!("../../../third_party/ww2ogg/packed_codebooks_aoTuV_603.bin");

/// The library's codebooks, as packed bytes.
pub fn library() -> &'static [&'static [u8]] {
    static BOOKS: OnceLock<Vec<&'static [u8]>> = OnceLock::new();
    BOOKS.get_or_init(|| {
        let n = LIBRARY.len();
        let table = u32::from_le_bytes(LIBRARY[n - 4..].try_into().expect("4 bytes")) as usize;
        let offsets: Vec<usize> =
            LIBRARY[table..].chunks_exact(4).map(|c| u32::from_le_bytes(c.try_into().expect("4 bytes")) as usize).collect();
        offsets.windows(2).map(|w| &LIBRARY[w[0]..w[1]]).collect()
    })
}

/// The number of values a lookup-type-1 codebook stores: the largest r with r^dimensions <= entries.
fn lookup1_values(entries: u32, dimensions: u32) -> u32 {
    if dimensions == 0 {
        return 0;
    }

    let mut r = (f64::from(entries).powf(1.0 / f64::from(dimensions))).floor() as u32;
    while (u64::from(r) + 1).pow(dimensions) <= u64::from(entries) {
        r += 1;
    }
    while r > 0 && u64::from(r).pow(dimensions) > u64::from(entries) {
        r -= 1;
    }

    r
}

/// Writes the library's codebook `id` in the standard Vorbis form.
///
/// # Errors
///
/// When the ID is not in the library or its codebook is not one the packing can hold.
pub fn rebuild(id: u32, out: &mut BitWriter) -> Result<()> {
    let book = library().get(id as usize).ok_or_else(|| AudioError::Invalid(format!("codebook {id} is not in the library")))?;
    let mut r = BitReader::new(book);
    let dimensions = r.read(4);
    let entries = r.read(14);
    out.write(0x56_4342, 24);
    out.write(dimensions, 16);
    out.write(entries, 24);
    let ordered = r.read(1);
    out.write(ordered, 1);
    if ordered == 1 {
        out.write(r.read(5), 5);
        let mut current = 0;
        while current < entries {
            let bits = ilog(entries - current);
            let number = r.read(bits);
            out.write(number, bits);
            current += number;
        }
    } else {
        let length_bits = r.read(3);
        let sparse = r.read(1);
        if length_bits == 0 || length_bits > 5 {
            return Err(AudioError::Invalid(format!("codebook {id}: a codeword length of {length_bits} bits")));
        }

        out.write(sparse, 1);
        for _ in 0..entries {
            let present = if sparse == 1 { r.read(1) } else { 1 };
            if sparse == 1 {
                out.write(present, 1);
            }
            if present == 1 {
                out.write(r.read(length_bits), 5);
            }
        }
    }

    let lookup = r.read(1);
    out.write(lookup, 4);
    if lookup == 1 {
        out.write(r.read(32), 32);
        out.write(r.read(32), 32);
        let value_bits = r.read(4);
        out.write(value_bits, 4);
        out.write(r.read(1), 1);
        for _ in 0..lookup1_values(entries, dimensions) {
            out.write(r.read(value_bits + 1), value_bits + 1);
        }
    }

    if r.overran() {
        return Err(AudioError::Invalid(format!("codebook {id} ends early")));
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn library_has_every_codebook() {
        assert_eq!(library().len(), 598);
        for id in 0..library().len() as u32 {
            let mut w = BitWriter::new();
            rebuild(id, &mut w).unwrap();
            assert!(w.bit_len() > 64);
        }
    }

    #[test]
    fn lookup1() {
        assert_eq!(lookup1_values(81, 4), 3);
        assert_eq!(lookup1_values(80, 4), 2);
        assert_eq!(lookup1_values(625, 4), 5);
    }
}
