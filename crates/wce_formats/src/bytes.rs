//! The little-endian byte reader the binary formats share (CHMF, markers, CAS2, models_naval, animations and their metadata): every
//! read is checked against the data's end, with the end computed without overflow, so a truncated or hostile file is an error and
//! never a panic. A format with pieces of its own (its strings, ids, lists) reads through a newtype of its module ([`format_reader!`])
//! that has them and derefs to this one; its errors come from [`Short`] through `From`.

use std::fmt;

/// A read past the end of the data: `need` bytes at `at`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Short {
    pub at: usize,
    pub need: usize,
}

impl fmt::Display for Short {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} bytes past the end at {:#x}", self.need, self.at)
    }
}

/// The formats whose errors are plain strings.
impl From<Short> for String {
    fn from(s: Short) -> Self {
        s.to_string()
    }
}

/// The data and the position read to. The fields are open: a format may seek (to a chunk's end) or look back at what it read.
pub struct ByteReader<'a> {
    pub b: &'a [u8],
    pub p: usize,
}

impl<'a> ByteReader<'a> {
    pub fn new(b: &'a [u8]) -> Self {
        Self { b, p: 0 }
    }

    /// The bytes not read yet (none past the end after a seek there).
    pub fn left(&self) -> usize {
        self.b.len().saturating_sub(self.p)
    }

    pub fn take(&mut self, n: usize) -> Result<&'a [u8], Short> {
        let s = self.p.checked_add(n).and_then(|end| self.b.get(self.p..end)).ok_or(Short { at: self.p, need: n })?;
        self.p += n;
        Ok(s)
    }

    fn array<const N: usize>(&mut self) -> Result<[u8; N], Short> {
        Ok(*self.take(N)?.first_chunk::<N>().expect("N bytes"))
    }

    pub fn u8(&mut self) -> Result<u8, Short> {
        Ok(self.take(1)?[0])
    }

    pub fn u16(&mut self) -> Result<u16, Short> {
        self.array().map(u16::from_le_bytes)
    }

    pub fn i16(&mut self) -> Result<i16, Short> {
        self.array().map(i16::from_le_bytes)
    }

    pub fn u32(&mut self) -> Result<u32, Short> {
        self.array().map(u32::from_le_bytes)
    }

    pub fn i32(&mut self) -> Result<i32, Short> {
        self.array().map(i32::from_le_bytes)
    }

    pub fn f32(&mut self) -> Result<f32, Short> {
        self.array().map(f32::from_le_bytes)
    }

    pub fn floats<const N: usize>(&mut self) -> Result<[f32; N], Short> {
        let mut out = [0.0; N];
        for v in &mut out {
            *v = self.f32()?;
        }

        Ok(out)
    }

    /// `n` UTF-16 code units.
    pub fn utf16(&mut self, n: usize) -> Result<Vec<u16>, Short> {
        let b = self.take(n.checked_mul(2).ok_or(Short { at: self.p, need: usize::MAX })?)?;
        Ok(b.as_chunks::<2>().0.iter().map(|c| u16::from_le_bytes(*c)).collect())
    }

    /// A u32 count of items of at least `min_item` bytes each: an error, not an allocation, when the bytes left cannot hold them.
    pub fn count(&mut self, min_item: usize) -> Result<usize, Short> {
        let at = self.p;
        let n = self.u32()? as usize;
        let need = n.saturating_mul(min_item.max(1));
        if need > self.left() {
            return Err(Short { at, need });
        }

        Ok(n)
    }
}

/// A format's reader: `struct $name<'a>(ByteReader<'a>)`, which derefs to the shared reader (its reads and its `b` and `p`), for the
/// format's own methods.
macro_rules! format_reader {
    ($name:ident) => {
        struct $name<'a>(crate::bytes::ByteReader<'a>);

        impl<'a> std::ops::Deref for $name<'a> {
            type Target = crate::bytes::ByteReader<'a>;

            fn deref(&self) -> &Self::Target {
                &self.0
            }
        }

        impl std::ops::DerefMut for $name<'_> {
            fn deref_mut(&mut self) -> &mut Self::Target {
                &mut self.0
            }
        }
    };
}

pub(crate) use format_reader;

#[cfg(test)]
mod tests {
    use super::{ByteReader, Short};

    #[test]
    fn reads_are_little_endian_and_checked() {
        let data = [1, 0, 2, 0, 0, 0, 0, 0, 0x80, 0x3f, 0xff];
        let mut r = ByteReader::new(&data);
        assert_eq!(r.u16(), Ok(1));
        assert_eq!(r.u32(), Ok(2));
        assert_eq!(r.f32(), Ok(1.0));
        assert_eq!(r.left(), 1);
        assert_eq!(r.u16(), Err(Short { at: 10, need: 2 }));
        assert_eq!(r.p, 10);
        assert_eq!(r.u8(), Ok(0xff));
        assert_eq!(r.u8(), Err(Short { at: 11, need: 1 }));

        // A position past the end (a seek) and lengths that overflow are errors, not panics.
        r.p = usize::MAX - 1;
        assert!(r.take(4).is_err());
        assert_eq!(r.left(), 0);
        assert!(ByteReader::new(&data).utf16(usize::MAX).is_err());
    }

    #[test]
    fn a_count_must_fit_in_the_bytes_left() {
        let mut data = 3u32.to_le_bytes().to_vec();
        data.extend([0; 12]);
        assert_eq!(ByteReader::new(&data).count(4), Ok(3));
        assert!(ByteReader::new(&data).count(5).is_err());
        assert!(ByteReader::new(&u32::MAX.to_le_bytes()).count(0).is_err());
    }
}
