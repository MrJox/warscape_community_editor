//! What a decompiled project carries of the binary it came from, for the stages whose outputs it cannot reproduce from its own files
//! (BOB's, from a project that only approximates BOB's inputs): the files, in a folder of the project, with fingerprints of the inputs
//! each group was made from. The compile writes a group back while its fingerprint is unchanged and makes it again otherwise
//! (docs/formats/tile_compile.md, "Carried outputs").

use std::collections::BTreeMap;

use wce_tile_model::{Blob, Raster, Samples, Tile};

/// The file holding the fingerprints, in a carried folder.
pub const FINGERPRINTS: &str = "fingerprints.txt";

/// FNV-1a, 64 bits: stable across runs and builds.
#[derive(Clone, Copy)]
pub(crate) struct Fnv(pub u64);

impl Fnv {
    pub fn new() -> Self {
        Self(0xcbf2_9ce4_8422_2325)
    }

    pub fn bytes(&mut self, b: &[u8]) -> &mut Self {
        for &x in b {
            self.0 = (self.0 ^ u64::from(x)).wrapping_mul(0x0100_0000_01b3);
        }

        self
    }

    pub fn u32(&mut self, v: u32) -> &mut Self {
        self.bytes(&v.to_le_bytes())
    }

    pub fn f32(&mut self, v: f32) -> &mut Self {
        self.u32(v.to_bits())
    }

    pub fn str(&mut self, v: &str) -> &mut Self {
        self.u32(v.len() as u32).bytes(v.as_bytes())
    }

    pub fn raster(&mut self, r: &Raster) -> &mut Self {
        self.u32(r.width).u32(r.height).u32(r.channels);
        match &r.samples {
            Samples::U8(v) => self.bytes(v),
            Samples::U16(v) => {
                for x in v {
                    self.bytes(&x.to_le_bytes());
                }

                self
            }

            Samples::F32(v) => {
                for x in v {
                    self.f32(*x);
                }

                self
            }
        }
    }
}

/// Fingerprints by group name, as [`FINGERPRINTS`] holds them: `<group> <hex>`, a line each.
pub fn text(prints: &BTreeMap<&str, u64>) -> String {
    prints.iter().map(|(k, v)| format!("{k} {v:016x}\n")).collect()
}

pub fn parse(text: &str) -> BTreeMap<String, u64> {
    text.lines().filter_map(|l| l.trim().split_once(' ').and_then(|(k, v)| Some((k.to_owned(), u64::from_str_radix(v.trim(), 16).ok()?)))).collect()
}

/// The files a project carries in a folder (names below it), and the fingerprints stored with them.
pub fn carried<'a>(raw: &'a Tile, dir: &str) -> (Vec<(&'a str, &'a Blob)>, BTreeMap<String, u64>) {
    let prefix = format!("{dir}/");
    let files: Vec<(&str, &Blob)> = raw.files.iter().filter_map(|f| f.path.strip_prefix(&prefix).map(|n| (n, &f.data))).collect();
    let prints = files
        .iter()
        .find(|(n, _)| *n == FINGERPRINTS)
        .and_then(|(_, b)| b.bytes().ok())
        .map(|b| parse(&String::from_utf8_lossy(&b)))
        .unwrap_or_default();
    (files.into_iter().filter(|(n, _)| *n != FINGERPRINTS).collect(), prints)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fingerprints_read_back() {
        let prints = BTreeMap::from([("trees", 1u64), ("grass", u64::MAX)]);
        let back = parse(&text(&prints));
        assert_eq!(back.get("trees"), Some(&1));
        assert_eq!(back.get("grass"), Some(&u64::MAX));
        assert_eq!(back.len(), 2);
    }
}
