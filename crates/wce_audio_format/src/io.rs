//! Little-endian reading and writing.

/// Why a sound file could not be read.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum FormatError {
    #[error("the file ends at byte {at}, {need} more bytes were expected")]
    Short { at: usize, need: usize },

    #[error("{0}")]
    Invalid(String),
}

/// Reads little-endian values from a byte slice, failing instead of panicking at its end.
pub(crate) struct Reader<'a> {
    bytes: &'a [u8],
    pos: usize,
}

impl<'a> Reader<'a> {
    pub fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, pos: 0 }
    }

    pub fn pos(&self) -> usize {
        self.pos
    }

    pub fn left(&self) -> usize {
        self.bytes.len() - self.pos
    }

    pub fn take(&mut self, n: usize) -> Result<&'a [u8], FormatError> {
        if n > self.left() {
            return Err(FormatError::Short { at: self.pos, need: n - self.left() });
        }

        let slice = &self.bytes[self.pos..self.pos + n];
        self.pos += n;
        Ok(slice)
    }

    pub fn rest(&mut self) -> &'a [u8] {
        let slice = &self.bytes[self.pos..];
        self.pos = self.bytes.len();
        slice
    }

    pub fn array<const N: usize>(&mut self) -> Result<[u8; N], FormatError> {
        let mut out = [0; N];
        out.copy_from_slice(self.take(N)?);
        Ok(out)
    }

    pub fn u8(&mut self) -> Result<u8, FormatError> {
        Ok(self.take(1)?[0])
    }

    pub fn u16(&mut self) -> Result<u16, FormatError> {
        Ok(u16::from_le_bytes(self.array()?))
    }

    pub fn u32(&mut self) -> Result<u32, FormatError> {
        Ok(u32::from_le_bytes(self.array()?))
    }

    pub fn f32(&mut self) -> Result<f32, FormatError> {
        Ok(f32::from_le_bytes(self.array()?))
    }

    pub fn f64(&mut self) -> Result<f64, FormatError> {
        Ok(f64::from_le_bytes(self.array()?))
    }

    /// A count of items each at least `min_item` bytes long, checked against what is left so a bad count fails early.
    pub fn count(&mut self, min_item: usize) -> Result<usize, FormatError> {
        let n = self.u32()? as usize;
        if n.saturating_mul(min_item) > self.left() {
            return Err(FormatError::Invalid(format!("a count of {n} at byte {} is more than the file holds", self.pos - 4)));
        }

        Ok(n)
    }
}

/// Appends little-endian values to a byte vector.
pub(crate) trait Put {
    fn put_u8(&mut self, v: u8);
    fn put_u16(&mut self, v: u16);
    fn put_u32(&mut self, v: u32);
    fn put_f32(&mut self, v: f32);
    fn put_f64(&mut self, v: f64);
}

impl Put for Vec<u8> {
    fn put_u8(&mut self, v: u8) {
        self.push(v);
    }

    fn put_u16(&mut self, v: u16) {
        self.extend_from_slice(&v.to_le_bytes());
    }

    fn put_u32(&mut self, v: u32) {
        self.extend_from_slice(&v.to_le_bytes());
    }

    fn put_f32(&mut self, v: f32) {
        self.extend_from_slice(&v.to_le_bytes());
    }

    fn put_f64(&mut self, v: f64) {
        self.extend_from_slice(&v.to_le_bytes());
    }
}
