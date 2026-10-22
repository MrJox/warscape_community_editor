//! `.markers`: TEd's stage-A exports of point-like things (props, decals, particle emitters, point lights, AI hints, trees, buildings).
//!
//! The framing: the repository name (`BASE_MARKER_REPOSITORY`, a u16-counted UTF-16 string), bytes up to the class name, the class name
//! (`PROP_MARKER` ...), 12 zero bytes, the class tag (equal to the u32 before the class name), the class version and the marker count,
//! then the markers. The markers are kept as bytes here (payloads per class: docs/formats/tile_binary.md).

#[derive(Debug, thiserror::Error)]
#[error("{0}")]
pub struct MarkersError(pub String);

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct MarkersFile {
    pub repository: String,

    /// The bytes between the repository name and the class name (they end with the class tag).
    pub prefix: Vec<u8>,
    pub class_name: String,
    pub class_tag: u32,
    pub class_version: u32,
    pub count: u32,

    /// Everything after the count.
    pub payload: Vec<u8>,
}

struct Reader<'a> {
    b: &'a [u8],
    p: usize,
}

impl Reader<'_> {
    fn take(&mut self, n: usize) -> Result<&[u8], MarkersError> {
        let r = self.b.get(self.p..self.p + n).ok_or_else(|| MarkersError(format!("truncated at {}", self.p)))?;
        self.p += n;
        Ok(r)
    }

    fn u32(&mut self) -> Result<u32, MarkersError> {
        Ok(u32::from_le_bytes(self.take(4)?.try_into().expect("4 bytes")))
    }

    fn str16(&mut self) -> Result<String, MarkersError> {
        let n = u16::from_le_bytes(self.take(2)?.try_into().expect("2 bytes")) as usize;
        let s = self.take(n * 2)?;
        let mut out = String::new();
        for c in s.chunks(2) {
            if c[1] != 0 || c[0] >= 0x80 {
                return Err(MarkersError("non-ASCII character in STR16".to_owned()));
            }

            out.push(char::from(c[0]));
        }

        Ok(out)
    }
}

fn put_str16(o: &mut Vec<u8>, s: &str) {
    o.extend_from_slice(&(s.len() as u16).to_le_bytes());
    for c in s.bytes() {
        o.push(c);
        o.push(0);
    }
}

impl MarkersFile {

    /// # Errors
    ///
    /// Not a markers file, or a truncated one.
    pub fn parse(bytes: &[u8]) -> Result<Self, MarkersError> {
        let mut r = Reader { b: bytes, p: 0 };
        let repository = r.str16()?;
        if repository != "BASE_MARKER_REPOSITORY" {
            return Err(MarkersError("not a markers file".to_owned()));
        }

        // The class name: the first plausible STR16 after the repository name.
        let start = r.p;
        let mut found = None;
        let mut off = start;
        while off + 2 <= bytes.len() && off < start + 80 {
            let mut t = Reader { b: bytes, p: off };
            if let Ok(s) = t.str16()
                && s.len() >= 3
                && s.contains("MARKER")
            {
                found = Some(off);
                break;
            }

            off += 1;
        }

        let Some(found) = found else { return Err(MarkersError("class name not found".to_owned())) };
        let prefix = bytes[start..found].to_vec();
        if prefix.len() < 4 {
            return Err(MarkersError("short prefix".to_owned()));
        }

        let class_tag = u32::from_le_bytes(prefix[prefix.len() - 4..].try_into().expect("4 bytes"));
        r.p = found;
        let class_name = r.str16()?;
        if r.take(12)?.iter().any(|&b| b != 0) {
            return Err(MarkersError("non-zero bytes after the class name".to_owned()));
        }

        if r.u32()? != class_tag {
            return Err(MarkersError("class tag mismatch".to_owned()));
        }

        let class_version = r.u32()?;
        let count = r.u32()?;
        Ok(Self { repository, prefix, class_name, class_tag, class_version, count, payload: bytes[r.p..].to_vec() })
    }

    pub fn write(&self) -> Vec<u8> {
        let mut o = vec![];
        put_str16(&mut o, &self.repository);
        o.extend_from_slice(&self.prefix);
        put_str16(&mut o, &self.class_name);
        o.extend_from_slice(&[0; 12]);
        for v in [self.class_tag, self.class_version, self.count] {
            o.extend_from_slice(&v.to_le_bytes());
        }

        o.extend_from_slice(&self.payload);
        o
    }
}
