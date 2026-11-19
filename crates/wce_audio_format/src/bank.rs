//! Wwise sound banks (`.bnk`), bank version 112 (Attila; Rome 2's patch banks) and any other version as far as the chunks go.
//!
//! A bank is a list of chunks, each a 4-byte tag, a u32 size and its data, with nothing between them:
//!
//! - `BKHD` the header: bank version, bank ID (FNV-1 of the bank's name), language ID, then fields kept as bytes;
//! - `DIDX` the media index: (media ID, offset in `DATA`, size), 12 bytes each;
//! - `DATA` the media (`.wem` files) the index points into;
//! - `HIRC` the hierarchy: a u32 count, then objects (u8 kind, u32 size, then `size` bytes starting with the object's u32 ID);
//! - `STID` the bank names: u32 kind, u32 count, then (u32 bank ID, u8 length, name);
//! - anything else (`STMG`, `ENVS` in `init.bnk`) is kept as bytes.
//!
//! Format notes: docs/formats/wwise_bank.md.

use crate::io::{Put, Reader};
use crate::{FormatError, Result};

/// A sound bank, chunk by chunk in the file's order.
#[derive(Clone, Debug, PartialEq)]
pub struct Bank {
    pub chunks: Vec<Chunk>,
}

/// One chunk of a bank.
#[derive(Clone, Debug, PartialEq)]
pub enum Chunk {
    Header(Header),
    MediaIndex(Vec<MediaEntry>),
    Data(Vec<u8>),
    Hierarchy(Vec<HircObject>),
    BankNames(BankNames),
    Other { tag: [u8; 4], data: Vec<u8> },
}

/// `BKHD`.
#[derive(Clone, Debug, PartialEq)]
pub struct Header {
    pub version: u32,
    pub bank_id: u32,
    pub language_id: u32,
    /// The rest of the header (feedback flag, project ID, padding: 12 bytes in most banks, 20 in Attila's `init.bnk`).
    pub rest: Vec<u8>,
}

/// One `DIDX` entry: a media file inside `DATA`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MediaEntry {
    pub id: u32,
    pub offset: u32,
    pub size: u32,
}

/// `STID`: the names of banks, by ID.
#[derive(Clone, Debug, PartialEq)]
pub struct BankNames {
    pub kind: u32,
    pub names: Vec<(u32, Vec<u8>)>,
}

/// An object of the hierarchy, its body not interpreted (see [`crate::bank::kind_name`] for what the kinds are).
#[derive(Clone, Debug, PartialEq)]
pub struct HircObject {
    pub kind: u8,
    pub id: u32,
    /// The object's bytes after its ID.
    pub body: Vec<u8>,
}

/// What a hierarchy object's kind is, in bank version 112.
pub fn kind_name(kind: u8) -> &'static str {
    match kind {
        1 => "State",
        2 => "Sound",
        3 => "Action",
        4 => "Event",
        5 => "Random/Sequence Container",
        6 => "Switch Container",
        7 => "Actor-Mixer",
        8 => "Bus",
        9 => "Blend Container",
        10 => "Music Segment",
        11 => "Music Track",
        12 => "Music Switch Container",
        13 => "Music Playlist Container",
        14 => "Attenuation",
        15 => "Dialogue Event",
        16 => "Feedback Bus",
        17 => "Feedback Node",
        18 => "Effect ShareSet",
        19 => "Effect",
        20 => "Auxiliary Bus",
        _ => "Unknown",
    }
}

impl Bank {
    /// Reads a bank.
    ///
    /// # Errors
    ///
    /// Fails when a chunk runs past the end of the file or a known chunk does not fill its size exactly.
    pub fn read(bytes: &[u8]) -> Result<Self> {
        if bytes.is_empty() {
            return Err(FormatError::Invalid("an empty file (a pack's way to remove a bank)".into()));
        }

        let mut r = Reader::new(bytes);
        let mut chunks = vec![];
        while r.left() > 0 {
            let at = r.pos();
            let tag: [u8; 4] = r.array()?;
            let size = r.u32()? as usize;
            let data = r.take(size).map_err(|_| FormatError::Invalid(format!("chunk {} at byte {at} runs past the end", show(&tag))))?;
            let chunk = read_chunk(tag, data).map_err(|e| FormatError::Invalid(format!("chunk {} at byte {at}: {e}", show(&tag))))?;
            chunks.push(chunk);
        }

        Ok(Self { chunks })
    }

    /// Writes the bank. What [`Bank::read`] read comes back byte for byte.
    pub fn write(&self) -> Vec<u8> {
        let mut out = vec![];
        for chunk in &self.chunks {
            let (tag, data) = write_chunk(chunk);
            out.extend_from_slice(&tag);
            out.put_u32(data.len() as u32);
            out.extend_from_slice(&data);
        }

        out
    }

    pub fn header(&self) -> Option<&Header> {
        self.chunks.iter().find_map(|c| if let Chunk::Header(h) = c { Some(h) } else { None })
    }

    /// The hierarchy's objects (empty when the bank has no `HIRC`).
    pub fn objects(&self) -> &[HircObject] {
        self.chunks.iter().find_map(|c| if let Chunk::Hierarchy(o) = c { Some(o.as_slice()) } else { None }).unwrap_or(&[])
    }

    /// The media index (empty when the bank holds no media).
    pub fn media_index(&self) -> &[MediaEntry] {
        self.chunks.iter().find_map(|c| if let Chunk::MediaIndex(m) = c { Some(m.as_slice()) } else { None }).unwrap_or(&[])
    }

    /// A media file held in the bank, by its ID.
    pub fn media(&self, id: u32) -> Option<&[u8]> {
        let entry = self.media_index().iter().find(|e| e.id == id)?;
        let data = self.chunks.iter().find_map(|c| if let Chunk::Data(d) = c { Some(d) } else { None })?;
        data.get(entry.offset as usize..entry.offset as usize + entry.size as usize)
    }
}

impl Chunk {
    /// The chunk's 4-byte tag.
    pub fn tag(&self) -> [u8; 4] {
        match self {
            Chunk::Header(_) => *b"BKHD",
            Chunk::MediaIndex(_) => *b"DIDX",
            Chunk::Data(_) => *b"DATA",
            Chunk::Hierarchy(_) => *b"HIRC",
            Chunk::BankNames(_) => *b"STID",
            Chunk::Other { tag, .. } => *tag,
        }
    }
}

fn show(tag: &[u8; 4]) -> String {
    String::from_utf8_lossy(tag).into_owned()
}

fn read_chunk(tag: [u8; 4], data: &[u8]) -> Result<Chunk> {
    let mut r = Reader::new(data);
    let chunk = match &tag {
        b"BKHD" => Chunk::Header(Header { version: r.u32()?, bank_id: r.u32()?, language_id: r.u32()?, rest: r.rest().to_vec() }),
        b"DIDX" => {
            if !data.len().is_multiple_of(12) {
                return Err(FormatError::Invalid(format!("{} bytes is not a whole number of 12-byte entries", data.len())));
            }

            let mut entries = vec![];
            while r.left() > 0 {
                entries.push(MediaEntry { id: r.u32()?, offset: r.u32()?, size: r.u32()? });
            }

            Chunk::MediaIndex(entries)
        }
        b"DATA" => Chunk::Data(r.rest().to_vec()),
        b"HIRC" => {
            let count = r.count(9)?;
            let mut objects = Vec::with_capacity(count);
            for _ in 0..count {
                let kind = r.u8()?;
                let size = r.u32()? as usize;
                if size < 4 {
                    return Err(FormatError::Invalid(format!("an object of {size} bytes has no ID")));
                }

                let mut body = Reader::new(r.take(size)?);
                objects.push(HircObject { kind, id: body.u32()?, body: body.rest().to_vec() });
            }

            Chunk::Hierarchy(objects)
        }
        b"STID" => {
            let kind = r.u32()?;
            let count = r.count(5)?;
            let mut names = Vec::with_capacity(count);
            for _ in 0..count {
                let id = r.u32()?;
                let len = r.u8()? as usize;
                names.push((id, r.take(len)?.to_vec()));
            }

            Chunk::BankNames(BankNames { kind, names })
        }
        _ => return Ok(Chunk::Other { tag, data: data.to_vec() }),
    };

    if r.left() != 0 {
        return Err(FormatError::Invalid(format!("{} bytes left over", r.left())));
    }

    Ok(chunk)
}

fn write_chunk(chunk: &Chunk) -> ([u8; 4], Vec<u8>) {
    let mut out = vec![];
    let tag = match chunk {
        Chunk::Header(h) => {
            out.put_u32(h.version);
            out.put_u32(h.bank_id);
            out.put_u32(h.language_id);
            out.extend_from_slice(&h.rest);
            *b"BKHD"
        }
        Chunk::MediaIndex(entries) => {
            for e in entries {
                out.put_u32(e.id);
                out.put_u32(e.offset);
                out.put_u32(e.size);
            }

            *b"DIDX"
        }
        Chunk::Data(data) => {
            out.extend_from_slice(data);
            *b"DATA"
        }
        Chunk::Hierarchy(objects) => {
            out.put_u32(objects.len() as u32);
            for o in objects {
                out.put_u8(o.kind);
                out.put_u32(4 + o.body.len() as u32);
                out.put_u32(o.id);
                out.extend_from_slice(&o.body);
            }

            *b"HIRC"
        }
        Chunk::BankNames(names) => {
            out.put_u32(names.kind);
            out.put_u32(names.names.len() as u32);
            for (id, name) in &names.names {
                out.put_u32(*id);
                out.put_u8(name.len() as u8);
                out.extend_from_slice(name);
            }

            *b"STID"
        }
        Chunk::Other { tag, data } => {
            out.extend_from_slice(data);
            *tag
        }
    };

    (tag, out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> Vec<u8> {
        let mut b = vec![];
        let mut chunk = |tag: &[u8; 4], data: &[u8]| {
            b.extend_from_slice(tag);
            b.put_u32(data.len() as u32);
            b.extend_from_slice(data);
        };
        chunk(b"BKHD", &[112, 0, 0, 0, 1, 2, 3, 4, 11, 0, 0, 0, 0, 0, 0, 0, 3, 3, 0, 0, 0, 0, 0, 0]);
        chunk(b"DIDX", &[7, 0, 0, 0, 0, 0, 0, 0, 3, 0, 0, 0]);
        chunk(b"DATA", &[9, 8, 7, 0]);
        // One Event (kind 4): ID 0x10, then one action ID.
        chunk(b"HIRC", &[1, 0, 0, 0, 4, 12, 0, 0, 0, 0x10, 0, 0, 0, 1, 0, 0, 0, 0x20, 0, 0, 0]);
        chunk(b"STID", &[1, 0, 0, 0, 1, 0, 0, 0, 5, 0, 0, 0, 2, b'a', b'b']);
        chunk(b"ENVS", &[1, 2]);
        b
    }

    #[test]
    fn round_trip() {
        let bytes = sample();
        let bank = Bank::read(&bytes).unwrap();
        assert_eq!(bank.write(), bytes);
        assert_eq!(bank.header().unwrap().version, 112);
        assert_eq!(bank.media(7), Some(&[9u8, 8, 7][..]));
        assert_eq!(bank.objects()[0].id, 0x10);
        assert_eq!(bank.objects()[0].kind, 4);
    }

    #[test]
    fn truncated_chunk_fails() {
        let mut bytes = sample();
        bytes.truncate(bytes.len() - 1);
        assert!(Bank::read(&bytes).is_err());
    }
}
